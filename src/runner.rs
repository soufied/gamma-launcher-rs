use crate::config::{expand_path, AppConfig};
use crate::error::{LauncherError, Result};
use crate::fsutil::tool_available;
use crate::report::Reporter;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread;
use std::time::Duration;

const DEFAULT_UMU_ID: &str = "umu-stalkeranomaly";
const PAUSE_POLL_INTERVAL: Duration = Duration::from_millis(80);
const WINE_DRIVE: &str = "Z:";

pub type ProcessState = Arc<ProcessRegistry>;

#[derive(Clone)]
pub struct Waker {
    callback: Arc<dyn Fn() + Send + Sync>,
}

impl Waker {
    pub fn new(callback: impl Fn() + Send + Sync + 'static) -> Self {
        Self {
            callback: Arc::new(callback),
        }
    }

    pub fn wake(&self) {
        let callback: &(dyn Fn() + Send + Sync) = self.callback.as_ref();
        callback();
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlState {
    Idle,
    Running,
    Paused,
    Cancelling,
}

impl ControlState {
    pub fn label(&self) -> &'static str {
        match self {
            ControlState::Idle => "Idle",
            ControlState::Running => "Running",
            ControlState::Paused => "Paused by User",
            ControlState::Cancelling => "Cancelling",
        }
    }

    pub fn is_active(&self) -> bool {
        matches!(
            self,
            ControlState::Running | ControlState::Paused | ControlState::Cancelling
        )
    }
}

#[derive(Debug)]
pub struct JobControl {
    running: AtomicBool,
    paused: AtomicBool,
    cancelled: AtomicBool,
    gate: Mutex<bool>,
    resumed: Condvar,
}

pub type JobHandle = Arc<JobControl>;

impl Default for JobControl {
    fn default() -> Self {
        Self {
            running: AtomicBool::new(false),
            paused: AtomicBool::new(false),
            cancelled: AtomicBool::new(false),
            gate: Mutex::new(false),
            resumed: Condvar::new(),
        }
    }
}

impl JobControl {
    pub fn idle() -> JobHandle {
        Arc::new(Self::default())
    }

    pub fn running() -> JobHandle {
        let control = Self::default();
        control.running.store(true, Ordering::SeqCst);
        Arc::new(control)
    }

    fn gate(&self) -> MutexGuard<'_, bool> {
        match self.gate.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        }
    }

    pub fn state(&self) -> ControlState {
        if !self.running.load(Ordering::SeqCst) {
            return ControlState::Idle;
        }
        if self.cancelled.load(Ordering::SeqCst) {
            return ControlState::Cancelling;
        }
        if self.paused.load(Ordering::SeqCst) {
            return ControlState::Paused;
        }
        ControlState::Running
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }

    pub fn pause(&self) {
        if self.cancelled.load(Ordering::SeqCst) || !self.running.load(Ordering::SeqCst) {
            return;
        }
        let mut gate = self.gate();
        *gate = true;
        self.paused.store(true, Ordering::SeqCst);
    }

    pub fn resume(&self) {
        {
            let mut gate = self.gate();
            *gate = false;
            self.paused.store(false, Ordering::SeqCst);
        }
        self.resumed.notify_all();
    }

    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
        {
            let mut gate = self.gate();
            *gate = false;
            self.paused.store(false, Ordering::SeqCst);
        }
        self.resumed.notify_all();
    }

    pub fn finish(&self) {
        self.running.store(false, Ordering::SeqCst);
        {
            let mut gate = self.gate();
            *gate = false;
            self.paused.store(false, Ordering::SeqCst);
        }
        self.resumed.notify_all();
    }

    pub fn checkpoint(&self) -> Result<()> {
        if self.cancelled.load(Ordering::SeqCst) {
            return Err(LauncherError::Cancelled);
        }

        let mut gate = self.gate();
        while *gate && !self.cancelled.load(Ordering::SeqCst) {
            gate = match self.resumed.wait(gate) {
                Ok(guard) => guard,
                Err(poisoned) => poisoned.into_inner(),
            };
        }
        drop(gate);

        if self.cancelled.load(Ordering::SeqCst) {
            Err(LauncherError::Cancelled)
        } else {
            Ok(())
        }
    }

    pub async fn checkpoint_async(&self) -> Result<()> {
        loop {
            if self.cancelled.load(Ordering::SeqCst) {
                return Err(LauncherError::Cancelled);
            }
            if !self.paused.load(Ordering::SeqCst) {
                return Ok(());
            }
            tokio::time::sleep(PAUSE_POLL_INTERVAL).await;
        }
    }

    pub async fn sleep_for(&self, duration: Duration) -> Result<()> {
        let mut remaining = duration;

        while !remaining.is_zero() {
            self.checkpoint_async().await?;
            let slice = remaining.min(PAUSE_POLL_INTERVAL);
            tokio::time::sleep(slice).await;
            remaining = remaining.saturating_sub(slice);
        }

        self.checkpoint_async().await
    }
}

pub fn new_job_control() -> JobHandle {
    JobControl::idle()
}

fn normalized_wine_body(path: &Path) -> String {
    let raw = path.to_string_lossy().replace('\\', "/");
    let trimmed = raw.trim_end_matches('/');

    if trimmed.is_empty() {
        "/".to_string()
    } else if trimmed.starts_with('/') {
        trimmed.to_string()
    } else {
        format!("/{trimmed}")
    }
}

pub fn linux_to_wine_path(path: &Path) -> String {
    format!("{WINE_DRIVE}{}", normalized_wine_body(path))
}

pub fn escape_ini_backslashes(value: &str) -> String {
    value.replace('\\', "\\\\")
}

pub fn wine_path_body(value: &str) -> Option<&str> {
    let trimmed = value.trim();
    let inner = match trimmed.strip_prefix("@ByteArray(") {
        Some(rest) => rest.strip_suffix(')')?,
        None => trimmed,
    };

    let mut characters = inner.char_indices();
    let (_, drive) = characters.next()?;
    if !drive.is_ascii_alphabetic() {
        return None;
    }

    let (colon_index, colon) = characters.next()?;
    if colon != ':' {
        return None;
    }

    Some(&inner[colon_index + 1..])
}

pub fn wine_path_needs_repair(value: &str) -> bool {
    match wine_path_body(value) {
        Some(body) => !body.starts_with('/') && !body.starts_with("\\\\"),
        None => false,
    }
}

pub fn repaired_wine_path(value: &str, expected: &Path) -> Option<String> {
    if !wine_path_needs_repair(value) {
        return None;
    }

    let repaired = linux_to_wine_path(expected);
    if value.trim().starts_with("@ByteArray(") {
        Some(format!("@ByteArray({repaired})"))
    } else {
        Some(repaired)
    }
}

#[derive(Debug, Clone)]
pub struct ProcessSnapshot {
    pub label: String,
    pub pid: u32,
}

#[derive(Debug)]
struct ActiveProcess {
    generation: u64,
    label: String,
    pid: u32,
}

#[derive(Debug, Default)]
pub struct ProcessRegistry {
    active: Mutex<Option<ActiveProcess>>,
    generation: AtomicU64,
}

impl ProcessRegistry {
    fn guard(&self) -> MutexGuard<'_, Option<ActiveProcess>> {
        match self.active.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        }
    }

    pub fn snapshot(&self) -> Option<ProcessSnapshot> {
        self.guard().as_ref().map(|active| ProcessSnapshot {
            label: active.label.clone(),
            pid: active.pid,
        })
    }

    fn claim(&self, label: String, pid: u32) -> u64 {
        let generation = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        *self.guard() = Some(ActiveProcess {
            generation,
            label,
            pid,
        });
        generation
    }

    fn release(&self, generation: u64) -> Option<String> {
        let mut guard = self.guard();
        let owned = guard
            .as_ref()
            .map(|active| active.generation == generation)
            .unwrap_or(false);

        if owned {
            return guard.take().map(|active| active.label);
        }

        None
    }
}

pub fn new_process_state() -> ProcessState {
    Arc::new(ProcessRegistry::default())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaunchTarget {
    ModOrganizer,
    Launcher,
    Game,
}

impl LaunchTarget {
    pub fn label(&self) -> &'static str {
        match self {
            LaunchTarget::ModOrganizer => "Mod Organizer 2",
            LaunchTarget::Launcher => "Anomaly Launcher",
            LaunchTarget::Game => "Anomaly",
        }
    }
}

fn first_existing(candidates: &[PathBuf]) -> Option<PathBuf> {
    candidates
        .iter()
        .find(|candidate| candidate.is_file())
        .cloned()
}

fn resolve_executable(config: &AppConfig, target: LaunchTarget) -> Result<PathBuf> {
    let configured = match target {
        LaunchTarget::ModOrganizer => config.runner.mo2_executable.as_deref(),
        LaunchTarget::Launcher => config.runner.launcher_executable.as_deref(),
        LaunchTarget::Game => config.runner.game_executable.as_deref(),
    };

    if let Some(path) = configured.map(expand_path) {
        if path.is_file() {
            return Ok(path);
        }
        return Err(LauncherError::RunnerConfig(format!(
            "the configured executable for {} does not exist: {}",
            target.label(),
            path.display()
        )));
    }

    let gamma = config.expanded_gamma_dir();
    let anomaly = config.expanded_anomaly_dir();

    let candidates = match target {
        LaunchTarget::ModOrganizer => match gamma {
            Some(gamma) => vec![
                gamma.join("ModOrganizer.exe"),
                gamma.join("Mod Organizer.exe"),
            ],
            None => Vec::new(),
        },
        LaunchTarget::Launcher => match anomaly {
            Some(anomaly) => vec![
                anomaly.join("AnomalyLauncher.exe"),
                anomaly.join("Anomaly Launcher.exe"),
                anomaly.join("bin").join("AnomalyLauncher.exe"),
            ],
            None => Vec::new(),
        },
        LaunchTarget::Game => match anomaly {
            Some(anomaly) => vec![
                anomaly.join("bin").join("AnomalyDX11AVX.exe"),
                anomaly.join("bin").join("AnomalyDX11.exe"),
                anomaly.join("bin").join("AnomalyDX10AVX.exe"),
                anomaly.join("bin").join("AnomalyDX9AVX.exe"),
            ],
            None => Vec::new(),
        },
    };

    if candidates.is_empty() {
        return Err(LauncherError::RunnerConfig(format!(
            "no directory is configured to locate {}",
            target.label()
        )));
    }

    first_existing(&candidates).ok_or_else(|| {
        LauncherError::RunnerConfig(format!(
            "could not find an executable for {}, set one explicitly in the runner settings",
            target.label()
        ))
    })
}

pub fn resolve_status(config: &AppConfig, target: LaunchTarget) -> Option<PathBuf> {
    resolve_executable(config, target).ok()
}

fn build_invocation(config: &AppConfig, executable: &Path, reporter: &Reporter) -> Vec<String> {
    let mut parts: Vec<String> = Vec::new();

    if config.runner.use_gamemode {
        if tool_available("gamemoderun") {
            parts.push("gamemoderun".to_string());
        } else {
            reporter.warn("gamemoderun is not available in PATH, launching without it");
        }
    }

    if config.runner.use_umu {
        parts.push("umu-run".to_string());
    } else {
        parts.push("wine".to_string());
    }

    parts.push(executable.to_string_lossy().to_string());
    parts
}

fn apply_environment(command: &mut Command, config: &AppConfig, reporter: &Reporter) {
    let umu_id = if config.runner.umu_id.trim().is_empty() {
        DEFAULT_UMU_ID.to_string()
    } else {
        config.runner.umu_id.trim().to_string()
    };
    command.env("GAMEID", &umu_id);
    command.env("STORE", "none");

    if let Some(prefix) = config.runner.wine_prefix.as_deref().map(expand_path) {
        command.env("WINEPREFIX", &prefix);
        command.env("PROTONPREFIX", &prefix);
    }

    if let Some(proton) = config.runner.proton_path.as_deref().map(expand_path) {
        if !proton.is_dir() {
            reporter.warn(format!(
                "the configured Proton build does not exist: {}",
                proton.display()
            ));
        }
        command.env("PROTONPATH", &proton);
    }

    if config.runner.wine_dll_overrides_enabled {
        let overrides = config.runner.wine_dll_overrides.trim();
        if !overrides.is_empty() {
            command.env("WINEDLLOVERRIDES", overrides);
        }
    }

    if config.runner.omp_threads_enabled {
        command.env("OMP_NUM_THREADS", config.runner.omp_threads.to_string());
    }

    if config.runner.dxvk_config_enabled {
        let dxvk_config = config.runner.dxvk_config.trim();
        if !dxvk_config.is_empty() {
            command.env("DXVK_CONFIG", dxvk_config);
        }
    }

    command.env(
        "WINE_FULLSCREEN_FSR",
        if config.runner.fsr_enabled { "1" } else { "0" },
    );

    for (key, value) in &config.runner.extra_env {
        if key.trim().is_empty() {
            continue;
        }
        command.env(key.trim(), value);
    }
}

fn describe_tweaks(config: &AppConfig) -> String {
    let mut tweaks = Vec::new();

    if config.runner.wine_dll_overrides_enabled {
        tweaks.push("WINEDLLOVERRIDES".to_string());
    }
    if config.runner.omp_threads_enabled {
        tweaks.push(format!("OMP_NUM_THREADS={}", config.runner.omp_threads));
    }
    if config.runner.dxvk_config_enabled {
        tweaks.push("DXVK_CONFIG".to_string());
    }
    tweaks.push(format!(
        "WINE_FULLSCREEN_FSR={}",
        if config.runner.fsr_enabled { 1 } else { 0 }
    ));

    tweaks.join(", ")
}

fn watch(
    mut child: std::process::Child,
    state: ProcessState,
    generation: u64,
    label: String,
    reporter: Reporter,
    wake: Waker,
) {
    thread::spawn(move || {
        let outcome = child.wait();
        let label = state.release(generation).unwrap_or(label);

        match outcome {
            Ok(status) => match status.code() {
                Some(0) => reporter.info(format!("[*] {label} has exited cleanly")),
                Some(code) => reporter.warn(format!("[*] {label} has exited with code {code}")),
                None => reporter.warn(format!("[*] {label} was terminated by a signal")),
            },
            Err(error) => reporter.error(format!("[!] Lost track of {label}: {error}")),
        }

        reporter.process_state_changed(None);
        wake.wake();
    });
}

pub fn launch(
    config: &AppConfig,
    target: LaunchTarget,
    reporter: &Reporter,
    process_state: &ProcessState,
    bg_reporter: Reporter,
    wake: Waker,
) -> Result<()> {
    if let Some(active) = process_state.snapshot() {
        return Err(LauncherError::RunnerConfig(format!(
            "{} is already running under PID {}, wait for it to exit first",
            active.label, active.pid
        )));
    }

    let executable = resolve_executable(config, target)?;
    let invocation = build_invocation(config, &executable, reporter);

    let (program, arguments) = invocation
        .split_first()
        .ok_or_else(|| LauncherError::RunnerConfig("empty launch invocation".to_string()))?;

    let mut command = Command::new(program);
    command.args(arguments);

    if let Some(parent) = executable.parent() {
        command.current_dir(parent);
    }

    apply_environment(&mut command, config, reporter);

    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    reporter.info(format!(
        "[+] Launching {} via: {}",
        target.label(),
        invocation.join(" ")
    ));
    reporter.info(format!("[*] Active tweaks: {}", describe_tweaks(config)));

    let child = command
        .spawn()
        .map_err(|error| LauncherError::CommandFailed {
            command: program.clone(),
            message: error.to_string(),
        })?;

    let pid = child.id();
    let label = target.label().to_string();
    let generation = process_state.claim(label.clone(), pid);

    reporter.info(format!("[*] {label} started with PID {pid}"));
    bg_reporter.process_state_changed(Some(label.clone()));
    wake.wake();

    watch(
        child,
        Arc::clone(process_state),
        generation,
        label,
        bg_reporter,
        wake,
    );

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_registry_reports_no_process() {
        let state = new_process_state();
        assert!(state.snapshot().is_none());
    }

    #[test]
    fn claiming_publishes_a_snapshot() {
        let state = new_process_state();
        let generation = state.claim("Anomaly".to_string(), 4242);
        let snapshot = state.snapshot().unwrap();

        assert_eq!(snapshot.label, "Anomaly");
        assert_eq!(snapshot.pid, 4242);
        assert_eq!(state.release(generation).as_deref(), Some("Anomaly"));
        assert!(state.snapshot().is_none());
    }

    #[test]
    fn absolute_paths_become_forward_slash_drive_paths() {
        assert_eq!(
            linux_to_wine_path(Path::new("/mnt/yo/Games/STALKER_Gamma/S.T.A.L.K.E.R. - Gamma")),
            "Z:/mnt/yo/Games/STALKER_Gamma/S.T.A.L.K.E.R. - Gamma"
        );
    }

    #[test]
    fn corrupted_and_single_escaped_paths_are_detected() {
        assert!(wine_path_needs_repair("Z:ntoamesTALKER_Gamma"));
        assert!(wine_path_needs_repair("@ByteArray(Z:\\mnt\\yo)"));
        assert!(!wine_path_needs_repair("Z:/mnt/yo/Games"));
        assert!(!wine_path_needs_repair("Z:\\\\mnt\\\\yo"));
        assert!(!wine_path_needs_repair("%BASE_DIR%/mods"));
        assert!(!wine_path_needs_repair("Stalker Anomaly"));
    }

    #[test]
    fn repairs_keep_the_byte_array_wrapper() {
        let repaired = repaired_wine_path("@ByteArray(Z:ntoames)", Path::new("/mnt/yo/Games"));
        assert_eq!(repaired.as_deref(), Some("@ByteArray(Z:/mnt/yo/Games)"));
        assert!(repaired_wine_path("Z:/mnt/yo", Path::new("/mnt/yo")).is_none());
    }

    #[test]
    fn escaping_doubles_every_backslash() {
        assert_eq!(escape_ini_backslashes("Z:\\mnt\\yo"), "Z:\\\\mnt\\\\yo");
    }

    #[test]
    fn a_fresh_control_is_idle_then_tracks_pause_and_cancel() {
        let control = JobControl::running();
        assert_eq!(control.state(), ControlState::Running);

        control.pause();
        assert_eq!(control.state(), ControlState::Paused);

        control.resume();
        assert_eq!(control.state(), ControlState::Running);
        assert!(control.checkpoint().is_ok());

        control.cancel();
        assert_eq!(control.state(), ControlState::Cancelling);
        assert!(control.checkpoint().is_err());

        control.finish();
        assert_eq!(control.state(), ControlState::Idle);
    }

    #[test]
    fn an_idle_control_never_pauses() {
        let control = JobControl::idle();
        control.pause();

        assert_eq!(control.state(), ControlState::Idle);
        assert!(control.checkpoint().is_ok());
    }

    #[test]
    fn a_stale_watcher_never_clears_a_newer_process() {
        let state = new_process_state();
        let stale = state.claim("Mod Organizer 2".to_string(), 1);
        state.release(stale);

        let fresh = state.claim("Anomaly".to_string(), 2);
        assert!(state.release(stale).is_none());
        assert_eq!(state.snapshot().unwrap().label, "Anomaly");
        assert_eq!(state.release(fresh).as_deref(), Some("Anomaly"));
    }
}
