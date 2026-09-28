use crate::error::Result;
use crate::report::Reporter;
use crate::runner::Waker;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;
use std::time::Duration;

pub const GAME_NEEDLES: [&str; 6] = [
    "xrengine",
    "anomalydx",
    "anomalylauncher",
    "xrmpe",
    "xrplay",
    "reshade",
];
pub const MO2_NEEDLES: [&str; 2] = ["modorganizer", "usvfs_proxy"];
pub const RUNNER_NEEDLES: [&str; 3] = ["umu-run", "wineserver", "winedevice"];
const NATIVE_STEAM_BINARY: &str = "steam";
const GRACE_PERIOD_ATTEMPTS: u32 = 12;
const GRACE_PERIOD_DELAY: Duration = Duration::from_millis(250);
const CLK_TCK_ASSUMED: u64 = 100;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ProcessCategory {
    Game,
    ModOrganizer,
    RunnerWrapper,
}

impl ProcessCategory {
    pub fn label(&self) -> &'static str {
        match self {
            ProcessCategory::Game => "Game",
            ProcessCategory::ModOrganizer => "Mod Organizer 2",
            ProcessCategory::RunnerWrapper => "Wine/Proton helper",
        }
    }

    fn classify(comm_lower: &str, basename_lower: &str) -> Option<Self> {
        let haystacks = [comm_lower, basename_lower];

        if GAME_NEEDLES
            .iter()
            .any(|needle| haystacks.iter().any(|value| value.contains(needle)))
        {
            return Some(ProcessCategory::Game);
        }

        if MO2_NEEDLES
            .iter()
            .any(|needle| haystacks.iter().any(|value| value.contains(needle)))
        {
            return Some(ProcessCategory::ModOrganizer);
        }

        if RUNNER_NEEDLES
            .iter()
            .any(|needle| haystacks.iter().any(|value| value.contains(needle)))
        {
            return Some(ProcessCategory::RunnerWrapper);
        }

        None
    }
}

#[derive(Debug, Clone)]
pub struct AdoptedProcess {
    pub pid: u32,
    pub name: String,
    pub category: ProcessCategory,
    pub pgid: Option<u32>,
    pub memory_bytes: Option<u64>,
    pub run_time_secs: Option<u64>,
}

#[derive(Debug, Default)]
pub struct ProcessRegistry {
    processes: Mutex<HashMap<u32, AdoptedProcess>>,
    steam_running: AtomicBool,
}

pub type SharedProcessRegistry = Arc<ProcessRegistry>;

impl ProcessRegistry {
    fn guard(&self) -> MutexGuard<'_, HashMap<u32, AdoptedProcess>> {
        match self.processes.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        }
    }

    pub fn snapshot(&self) -> Vec<AdoptedProcess> {
        let mut entries: Vec<AdoptedProcess> = self.guard().values().cloned().collect();
        entries.sort_by(|a, b| (a.category, a.pid).cmp(&(b.category, b.pid)));
        entries
    }

    pub fn count(&self) -> usize {
        self.guard().len()
    }

    pub fn is_steam_running(&self) -> bool {
        self.steam_running.load(Ordering::SeqCst)
    }

    fn replace(&self, processes: Vec<AdoptedProcess>, steam_running: bool) {
        let mut guard = self.guard();
        guard.clear();
        for process in processes {
            guard.insert(process.pid, process);
        }
        drop(guard);
        self.steam_running.store(steam_running, Ordering::SeqCst);
    }

    pub fn adopt_immediately(&self, pid: u32, name: String, category: ProcessCategory) {
        let mut guard = self.guard();
        guard.insert(
            pid,
            AdoptedProcess {
                pid,
                name,
                category,
                pgid: process_stat_fields(pid).map(|(pgrp, _)| pgrp),
                memory_bytes: process_memory_bytes(pid),
                run_time_secs: process_stat_fields(pid)
                    .and_then(|(_, starttime)| run_time_secs(starttime)),
            },
        );
    }
}

impl ProcessCategory {
    fn discriminant(&self) -> u8 {
        match self {
            ProcessCategory::Game => 0,
            ProcessCategory::ModOrganizer => 1,
            ProcessCategory::RunnerWrapper => 2,
        }
    }
}

impl PartialOrd for ProcessCategory {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for ProcessCategory {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.discriminant().cmp(&other.discriminant())
    }
}

pub fn new_registry() -> SharedProcessRegistry {
    Arc::new(ProcessRegistry::default())
}

#[cfg(target_os = "linux")]
fn process_identity(pid: u32) -> Option<(String, String)> {
    let comm = fs::read_to_string(format!("/proc/{pid}/comm"))
        .ok()
        .map(|value| value.trim().to_string())
        .unwrap_or_default();

    let cmdline = fs::read(format!("/proc/{pid}/cmdline")).unwrap_or_default();
    let program = cmdline
        .split(|byte| *byte == 0)
        .next()
        .map(|slice| String::from_utf8_lossy(slice).to_string())
        .unwrap_or_default();

    let basename = Path::new(&program)
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_default();

    if comm.is_empty() && basename.is_empty() {
        return None;
    }

    Some((comm, basename))
}

#[cfg(not(target_os = "linux"))]
fn process_identity(_pid: u32) -> Option<(String, String)> {
    None
}

#[cfg(target_os = "linux")]
fn process_stat_fields(pid: u32) -> Option<(u32, u64)> {
    let raw = fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let after_comm = raw.rsplit_once(')')?.1;
    let fields: Vec<&str> = after_comm.split_whitespace().collect();

    let pgrp: u32 = fields.get(2).and_then(|value| value.parse().ok())?;
    let starttime: u64 = fields.get(19).and_then(|value| value.parse().ok())?;

    Some((pgrp, starttime))
}

#[cfg(not(target_os = "linux"))]
fn process_stat_fields(_pid: u32) -> Option<(u32, u64)> {
    None
}

#[cfg(target_os = "linux")]
fn process_memory_bytes(pid: u32) -> Option<u64> {
    let status = fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
    for line in status.lines() {
        if let Some(rest) = line.strip_prefix("VmRSS:") {
            let kb: u64 = rest.trim().trim_end_matches("kB").trim().parse().ok()?;
            return Some(kb * 1024);
        }
    }
    None
}

#[cfg(not(target_os = "linux"))]
fn process_memory_bytes(_pid: u32) -> Option<u64> {
    None
}

#[cfg(target_os = "linux")]
fn uptime_secs() -> Option<u64> {
    let raw = fs::read_to_string("/proc/uptime").ok()?;
    let first = raw.split_whitespace().next()?;
    first.parse::<f64>().ok().map(|value| value as u64)
}

#[cfg(not(target_os = "linux"))]
fn uptime_secs() -> Option<u64> {
    None
}

fn run_time_secs(starttime_ticks: u64) -> Option<u64> {
    let uptime = uptime_secs()?;
    let started_at = starttime_ticks / CLK_TCK_ASSUMED;
    Some(uptime.saturating_sub(started_at))
}

#[cfg(target_os = "linux")]
fn wine_prefix_for_pid(pid: u32) -> Option<PathBuf> {
    let raw = fs::read(format!("/proc/{pid}/environ")).ok()?;
    for entry in raw.split(|byte| *byte == 0) {
        let text = String::from_utf8_lossy(entry);
        if let Some(value) = text.strip_prefix("WINEPREFIX=") {
            if !value.is_empty() {
                return Some(PathBuf::from(value));
            }
        }
    }
    None
}

#[cfg(not(target_os = "linux"))]
fn wine_prefix_for_pid(_pid: u32) -> Option<PathBuf> {
    None
}

#[cfg(target_os = "linux")]
pub fn scan_once() -> (Vec<AdoptedProcess>, bool) {
    let own = std::process::id();
    let mut found = Vec::new();
    let mut steam_running = false;

    let entries = match fs::read_dir("/proc") {
        Ok(entries) => entries,
        Err(_) => return (found, steam_running),
    };

    for entry in entries.flatten() {
        let pid: u32 = match entry.file_name().to_str().and_then(|name| name.parse().ok()) {
            Some(pid) => pid,
            None => continue,
        };

        if pid == own || pid <= 1 {
            continue;
        }

        let (comm, basename) = match process_identity(pid) {
            Some(identity) => identity,
            None => continue,
        };

        let comm_lower = comm.to_lowercase();
        let basename_lower = basename.to_lowercase();

        if comm_lower == NATIVE_STEAM_BINARY || basename_lower == NATIVE_STEAM_BINARY {
            steam_running = true;
            continue;
        }

        let category = match ProcessCategory::classify(&comm_lower, &basename_lower) {
            Some(category) => category,
            None => continue,
        };

        let name = if basename.is_empty() { comm } else { basename };
        let (pgid, starttime) = process_stat_fields(pid)
            .map(|(pgrp, starttime)| (Some(pgrp), Some(starttime)))
            .unwrap_or((None, None));

        found.push(AdoptedProcess {
            pid,
            name,
            category,
            pgid,
            memory_bytes: process_memory_bytes(pid),
            run_time_secs: starttime.and_then(run_time_secs),
        });
    }

    found.sort_by(|a, b| (a.category, a.pid).cmp(&(b.category, b.pid)));
    (found, steam_running)
}

#[cfg(not(target_os = "linux"))]
pub fn scan_once() -> (Vec<AdoptedProcess>, bool) {
    (Vec::new(), false)
}

pub fn spawn_background_scanner(
    registry: SharedProcessRegistry,
    wake: Waker,
    poll_interval: Duration,
) -> thread::JoinHandle<()> {
    thread::spawn(move || loop {
        let (processes, steam_running) = scan_once();
        registry.replace(processes, steam_running);
        wake.wake();
        thread::sleep(poll_interval);
    })
}

fn send_signal(signal: &str, pid: i32) -> bool {
    Command::new("kill")
        .arg(format!("-{signal}"))
        .arg(pid.to_string())
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

pub fn terminate_all(registry: &SharedProcessRegistry, reporter: &Reporter) -> Result<usize> {
    let snapshot = registry.snapshot();

    if snapshot.is_empty() {
        reporter.info("[*] No adopted Zone process is currently tracked");
        return Ok(0);
    }

    reporter.info(format!(
        "[+] Terminating {} adopted process(es)",
        snapshot.len()
    ));

    let mut prefixes: HashSet<PathBuf> = HashSet::new();
    for process in &snapshot {
        if let Some(prefix) = wine_prefix_for_pid(process.pid) {
            prefixes.insert(prefix);
        }
    }

    for prefix in &prefixes {
        reporter.info(format!("  - wineserver -k for prefix {}", prefix.display()));
        let outcome = Command::new("wineserver")
            .arg("-k")
            .env("WINEPREFIX", prefix)
            .output();

        match outcome {
            Ok(output) if output.status.success() => {}
            Ok(output) => reporter.warn(format!(
                "  ! wineserver -k for {} exited with status {}",
                prefix.display(),
                output.status
            )),
            Err(error) => reporter.warn(format!(
                "  ! could not run wineserver -k for {}: {error}",
                prefix.display()
            )),
        }
    }

    for process in &snapshot {
        reporter.info(format!(
            "  - SIGTERM {} (PID {})",
            process.name, process.pid
        ));
        send_signal("TERM", process.pid as i32);

        if let Some(pgid) = process.pgid {
            if pgid != process.pid {
                send_signal("TERM", -(pgid as i32));
            }
        }
    }

    let mut exited_early = false;
    for _ in 0..GRACE_PERIOD_ATTEMPTS {
        thread::sleep(GRACE_PERIOD_DELAY);
        let (remaining, _) = scan_once();
        if remaining.is_empty() {
            exited_early = true;
            break;
        }
    }

    if exited_early {
        reporter.info("[+] Every adopted process has exited");
        let (final_state, steam_running) = scan_once();
        registry.replace(final_state, steam_running);
        return Ok(snapshot.len());
    }

    let (survivors, _) = scan_once();

    for process in &survivors {
        reporter.warn(format!(
            "  ! {} (PID {}) ignored SIGTERM, sending SIGKILL",
            process.name, process.pid
        ));
        send_signal("KILL", process.pid as i32);

        if let Some(pgid) = process.pgid {
            if pgid != process.pid {
                send_signal("KILL", -(pgid as i32));
            }
        }
    }

    thread::sleep(GRACE_PERIOD_DELAY);
    let (final_state, steam_running) = scan_once();
    let still_present: Vec<&AdoptedProcess> = final_state
        .iter()
        .filter(|p| snapshot.iter().any(|s| s.pid == p.pid))
        .collect();

    for process in &snapshot {
        if !still_present.iter().any(|p| p.pid == process.pid) {
            reporter.info(format!(
                "[*] {} (PID {}) terminated",
                process.name, process.pid
            ));
        }
    }

    let terminated = snapshot.len() - still_present.len();
    registry.replace(final_state.clone(), steam_running);

    if !still_present.is_empty() {
        return Err(crate::error::LauncherError::ProcessControl(format!(
            "{} process(es) survived SIGKILL, they are probably stuck in an uninterruptible state",
            still_present.len()
        )));
    }

    Ok(terminated)
}
