use crate::config::{expand_path, AppConfig, GraphicsSettings, SyncMode, SPACEWAR_APPID};
use crate::error::{LauncherError, Result};
use crate::fsutil::tool_available;
use crate::mo2;
use crate::process::{ProcessCategory, SharedProcessRegistry};
use crate::report::Reporter;
use crate::steam_identity;
use chrono::Local;
use std::os::unix::process::CommandExt;
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
    ModdedGame,
}

impl LaunchTarget {
    pub fn label(&self) -> &'static str {
        match self {
            LaunchTarget::ModOrganizer => "Mod Organizer 2",
            LaunchTarget::Launcher => "Anomaly Launcher",
            LaunchTarget::Game => "Anomaly",
            LaunchTarget::ModdedGame => "Anomaly (Modded)",
        }
    }

    pub fn runs_game_process(&self) -> bool {
        matches!(self, LaunchTarget::Game | LaunchTarget::ModdedGame)
    }

    pub fn executable_target(&self) -> LaunchTarget {
        match self {
            LaunchTarget::ModdedGame => LaunchTarget::ModOrganizer,
            other => *other,
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
    let target = target.executable_target();
    let configured = match target {
        LaunchTarget::ModOrganizer => config.runner.mo2_executable.as_deref(),
        LaunchTarget::Launcher => config.runner.launcher_executable.as_deref(),
        LaunchTarget::Game => config.runner.game_executable.as_deref(),
        LaunchTarget::ModdedGame => None,
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
        LaunchTarget::ModdedGame => Vec::new(),
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

pub fn resolve_mo2_root(config: &AppConfig) -> Option<PathBuf> {
    if let Some(executable) = resolve_status(config, LaunchTarget::ModOrganizer) {
        if let Some(parent) = executable.parent() {
            return Some(parent.to_path_buf());
        }
    }
    config.expanded_gamma_dir()
}

fn sync_active_profile_identity(config: &AppConfig, target: LaunchTarget, reporter: &Reporter) {
    if !matches!(target, LaunchTarget::ModOrganizer | LaunchTarget::ModdedGame) {
        return;
    }
    let Some(mo2_root) = resolve_mo2_root(config) else {
        return;
    };
    let persona = steam_identity::effective_nickname(config);
    match mo2::sync_identity_into_active_profile(&mo2_root, &persona, &persona) {
        Ok(profile) => {
            reporter.info(format!(
                "[*] Synced persona \"{persona}\" into the active MO2 profile \"{}\" (user.ltx)",
                profile.name
            ));
        }
        Err(error) => {
            reporter.warn(format!(
                "[!] Could not sync persona into the active MO2 profile: {error}"
            ));
        }
    }
}

pub fn mo2_launch_available(config: &AppConfig) -> bool {
    config.runner.headless_mod_launch
        && resolve_status(config, LaunchTarget::ModOrganizer).is_some()
}

pub fn effective_launch_target(config: &AppConfig, requested: LaunchTarget) -> LaunchTarget {
    match requested {
        LaunchTarget::Game if mo2_launch_available(config) => LaunchTarget::ModdedGame,
        other => other,
    }
}

pub fn resolved_executable_directories(config: &AppConfig) -> Vec<PathBuf> {
    [
        LaunchTarget::ModOrganizer,
        LaunchTarget::Launcher,
        LaunchTarget::Game,
    ]
    .into_iter()
    .filter_map(|target| resolve_status(config, target))
    .filter_map(|executable| executable.parent().map(Path::to_path_buf))
    .collect()
}

fn build_invocation(
    config: &AppConfig,
    executable: &Path,
    target: LaunchTarget,
    reporter: &Reporter,
) -> Vec<String> {
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
    if target == LaunchTarget::ModdedGame {
        parts.push(moshortcut_argument(&config.runner.effective_mo2_shortcut_title()));
    }
    if target == LaunchTarget::Game && config.spacewar.steam_spacewar_mode {
        let nickname = steam_identity::effective_nickname(config);
        parts.push("-name".to_string());
        parts.push(nickname.clone());
        parts.push("-user".to_string());
        parts.push(nickname);
    }
    parts
}

pub fn moshortcut_argument(title: &str) -> String {
    format!("moshortcut://{title}")
}

fn apply_graphics_environment(command: &mut Command, graphics: &GraphicsSettings) {
    if graphics.fps_limit > 0 {
        command.env("DXVK_FRAME_RATE", graphics.fps_limit.to_string());
    }
    command.env("DXVK_ASYNC", if graphics.dxvk_async { "1" } else { "0" });
    command.env(
        "DXVK_STATE_CACHE",
        if graphics.dxvk_state_cache { "1" } else { "0" },
    );
    if graphics.nvidia_shader_cache_optimization {
        command.env("__GL_SHADER_DISK_CACHE", "1");
        command.env("__GL_SHADER_DISK_CACHE_SKIP_CLEANUP", "1");
        command.env("__GL_SHADER_DISK_CACHE_SIZE", "10737418240");
    }
    if graphics.mesa_shader_cache_optimization {
        command.env("MESA_SHADER_CACHE_DISABLE", "false");
        command.env("MESA_SHADER_CACHE_MAX_SIZE", "10G");
        command.env("RADV_PERFTEST", "gpl");
    }
    command.env(
        "MESA_VK_WSI_PRESENT_MODE",
        graphics.vk_wsi_present_mode.mesa_value(),
    );
    if graphics.wine_large_address_aware {
        command.env("WINE_LARGE_ADDRESS_AWARE", "1");
    }
    match graphics.sync_mechanism {
        SyncMode::Fsync => {
            command.env("PROTON_NO_FSYNC", "0");
            command.env("PROTON_NO_ESYNC", "1");
            command.env("WINEFSYNC", "1");
            command.env("WINEESYNC", "0");
        }
        SyncMode::Esync => {
            command.env("PROTON_NO_FSYNC", "1");
            command.env("PROTON_NO_ESYNC", "0");
            command.env("WINEFSYNC", "0");
            command.env("WINEESYNC", "1");
        }
        SyncMode::SystemDefault => {
            command.env("PROTON_NO_FSYNC", "0");
            command.env("PROTON_NO_ESYNC", "0");
        }
    }
    if graphics.enable_mangohud {
        command.env("MANGOHUD", "1");
    }
}

fn merge_dll_overrides(base: &str, additional: &str) -> String {
    let mut merged: Vec<(String, String)> = Vec::new();
    let mut insert_or_replace = |entry: &str| {
        let entry = entry.trim();
        if entry.is_empty() {
            return;
        }
        let key = entry.split('=').next().unwrap_or(entry).trim().to_string();
        let existing = merged.iter_mut().find(|(k, _)| k == &key);
        match existing {
            Some(existing) => existing.1 = entry.to_string(),
            None => merged.push((key, entry.to_string())),
        }
    };
    for entry in base.split(';') {
        insert_or_replace(entry);
    }
    for entry in additional.split(';') {
        insert_or_replace(entry);
    }
    merged
        .into_iter()
        .map(|(_, value)| value)
        .collect::<Vec<_>>()
        .join(";")
}

fn spacewar_identity_applies(config: &AppConfig, target: LaunchTarget) -> bool {
    config.spacewar.steam_spacewar_mode
        && matches!(
            target,
            LaunchTarget::Game | LaunchTarget::ModOrganizer | LaunchTarget::ModdedGame
        )
}

fn log_env_var(reporter: &Reporter, key: &str, value: &str) {
    reporter.info(format!("  [env] {key}={value}"));
}

fn apply_environment(
    command: &mut Command,
    config: &AppConfig,
    target: LaunchTarget,
    reporter: &Reporter,
) -> Result<Option<steam_identity::IdentitySyncReport>> {
    reporter.info(format!(
        "[*] Preparing environment for target: {} (Spacewar mode: {})",
        target.label(),
        if config.spacewar.steam_spacewar_mode { "enabled" } else { "disabled" }
    ));
    let umu_id = if config.runner.umu_id.trim().is_empty() {
        DEFAULT_UMU_ID.to_string()
    } else {
        config.runner.umu_id.trim().to_string()
    };
    command.env("GAMEID", &umu_id);
    
    if spacewar_identity_applies(config, target) {
        command.env("STORE", "steam");
        log_env_var(reporter, "STORE", "steam");
    } else {
        command.env("STORE", "none");
    }
    
    let wine_prefix = config.runner.wine_prefix.as_deref().map(expand_path);
    if let Some(prefix) = wine_prefix.as_deref() {
        let prefix_display = prefix.to_string_lossy().to_string();
        log_env_var(reporter, "WINEPREFIX", &prefix_display);
        log_env_var(reporter, "PROTONPREFIX", &prefix_display);
        command.env("WINEPREFIX", prefix);
        command.env("PROTONPREFIX", prefix);
    } else {
        reporter.warn("  [env] no Wine prefix is configured, WINEPREFIX will not be set".to_string());
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
    
    let mut wine_dll_overrides = String::new();
    if config.runner.wine_dll_overrides_enabled {
        wine_dll_overrides = config.runner.wine_dll_overrides.trim().to_string();
    }
    if spacewar_identity_applies(config, target)
        && steam_identity::dll_override_disables_lsteamclient(&wine_dll_overrides)
    {
        reporter.warn(
            "  [env] WINEDLLOVERRIDES disables lsteamclient, which Wine uses to bridge \
             steam_api(64).dll calls to the native Steam client; removing that override so \
             Spacewar multiplayer can still reach Steam"
                .to_string(),
        );
        wine_dll_overrides = steam_identity::strip_lsteamclient_override(&wine_dll_overrides);
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
    command.env("DOTNET_BUNDLE_EXTRACT_BASE_DIR", "C:\\temp");
    if let Some(prefix) = wine_prefix.as_deref() {
        steam_identity::ensure_dotnet_temp_dir(prefix)?;
    }

    if target != LaunchTarget::ModOrganizer {
        apply_graphics_environment(command, &config.graphics);
        wine_dll_overrides = merge_dll_overrides(&wine_dll_overrides, &config.graphics.dll_overrides);
    }

    let mut identity_report = None;
    if spacewar_identity_applies(config, target) {
        reporter.info(format!(
            "[*] Spacewar mode is active for target {}, injecting Steam identity environment",
            target.label()
        ));
        
        let report = steam_identity::sync_all(config, reporter)?;
        let nickname = steam_identity::effective_nickname(config);

        for (key, value) in steam_identity::steam_ipc_env_vars() {
            log_env_var(reporter, &key, &value);
            command.env(&key, &value);
        }

        command.env("GAMEID", SPACEWAR_APPID);
        log_env_var(reporter, "GAMEID", SPACEWAR_APPID);

        let identity_vars = steam_identity::spacewar_identity_env_vars(&nickname);
        for (key, value) in &identity_vars {
            log_env_var(reporter, key, value);
            command.env(key, value);
        }

        let persona_vars = steam_identity::spacewar_persona_env_vars(&nickname);
        for (key, value) in &persona_vars {
            log_env_var(reporter, key, value);
            command.env(key, value);
        }

        if config.spacewar.steam_check_running {
            reporter.info(
                "  [steam-detect] verifying native Steam process before launching"
                    .to_string(),
            );
            if !steam_identity::is_steam_running_now_verbose(reporter) {
                reporter.warn(
                    "[!] Steam does not appear to be running natively. Spacewar mode works best \
                     with the native Steam client running in the background for multiplayer features.",
                );
            }
        }
        identity_report = Some(report);
    } else {
        reporter.info(format!(
            "[*] Spacewar mode does not apply to target {}, no Steam identity environment injected",
            target.label()
        ));
    }

    if !wine_dll_overrides.is_empty() {
        command.env("WINEDLLOVERRIDES", &wine_dll_overrides);
    }

    Ok(identity_report)
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

fn category_for_target(target: LaunchTarget) -> ProcessCategory {
    match target {
        LaunchTarget::Game => ProcessCategory::Game,
        LaunchTarget::ModOrganizer | LaunchTarget::ModdedGame => ProcessCategory::ModOrganizer,
        LaunchTarget::Launcher => ProcessCategory::RunnerWrapper,
    }
}

fn open_log_file(executable: &Path, target: LaunchTarget) -> Option<std::fs::File> {
    let logs_dir = executable.parent()?.join("logs");
    if std::fs::create_dir_all(&logs_dir).is_err() {
        return None;
    }
    let stamp = Local::now().format("%Y%m%d-%H%M%S");
    let file_name = format!("{}-{stamp}.log", target.label().to_lowercase().replace(' ', "_"));
    std::fs::File::create(logs_dir.join(file_name)).ok()
}

pub fn launch(
    config: &AppConfig,
    target: LaunchTarget,
    reporter: &Reporter,
    process_state: &ProcessState,
    adopted_processes: &SharedProcessRegistry,
    bg_reporter: Reporter,
    wake: Waker,
) -> Result<()> {
    if let Some(active) = process_state.snapshot() {
        return Err(LauncherError::RunnerConfig(format!(
            "{} is already running under PID {}, wait for it to exit first",
            active.label, active.pid
        )));
    }
    if target == LaunchTarget::Game && !mo2_launch_available(config) {
        reporter.warn(
            "[!] Launching the Anomaly binary directly. Direct binary. Skips launcher and MO2. \
             Mods are not active."
                .to_string(),
        );
    }
    if target == LaunchTarget::ModdedGame {
        reporter.info(format!(
            "[*] Headless mod launch: Mod Organizer 2 will start \"{}\" without showing its window",
            config.runner.effective_mo2_shortcut_title()
        ));
        reporter.info(
            "[*] The shortcut title must match an executable configured inside Mod Organizer 2, \
             otherwise it will refuse to start"
                .to_string(),
        );
    }
    sync_active_profile_identity(config, target, reporter);
    let executable = resolve_executable(config, target)?;
    let invocation = build_invocation(config, &executable, target, reporter);
    let (program, arguments) = invocation
        .split_first()
        .ok_or_else(|| LauncherError::RunnerConfig("empty launch invocation".to_string()))?;
    let mut command = Command::new(program);
    command.args(arguments);
    if let Some(parent) = executable.parent() {
        command.current_dir(parent);
    }
    apply_environment(&mut command, config, target, reporter)?;
    command.stdin(Stdio::null());
    let log_file = open_log_file(&executable, target).and_then(|file| {
        file.try_clone().ok().map(|clone| (file, clone))
    });
    match log_file {
        Some((stderr_file, stdout_file)) => {
            command.stdout(Stdio::from(stdout_file));
            command.stderr(Stdio::from(stderr_file));
        }
        None => {
            command.stdout(Stdio::null());
            command.stderr(Stdio::null());
        }
    }
    command.process_group(0);
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
    adopted_processes.adopt_immediately(pid, label.clone(), category_for_target(target));
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
    fn dll_overrides_merge_with_additional_winning_on_collision() {
        let merged = merge_dll_overrides(
            "openal32=n,b;d3dcompiler_47=n,b",
            "d3dcompiler_47=n;xaudio2_7=n,b",
        );
        assert_eq!(merged, "openal32=n,b;d3dcompiler_47=n;xaudio2_7=n,b");
    }

    #[test]
    fn dll_overrides_merge_keeps_base_when_additional_is_empty() {
        assert_eq!(merge_dll_overrides("openal32=n,b", ""), "openal32=n,b");
        assert_eq!(merge_dll_overrides("", "openal32=n,b"), "openal32=n,b");
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

    #[test]
    fn spacewar_identity_applies_to_game_and_mo2_but_not_launcher() {
        let mut config = AppConfig::default();
        config.spacewar.steam_spacewar_mode = true;
        assert!(spacewar_identity_applies(&config, LaunchTarget::Game));
        assert!(spacewar_identity_applies(&config, LaunchTarget::ModOrganizer));
        assert!(!spacewar_identity_applies(&config, LaunchTarget::Launcher));
    }

    #[test]
    fn spacewar_identity_never_applies_when_mode_is_off() {
        let config = AppConfig::default();
        assert!(!spacewar_identity_applies(&config, LaunchTarget::Game));
        assert!(!spacewar_identity_applies(&config, LaunchTarget::ModOrganizer));
        assert!(!spacewar_identity_applies(&config, LaunchTarget::Launcher));
    }

    #[test]
    fn spacewar_persona_env_vars_cover_exactly_the_username_overrides() {
        let vars = steam_identity::spacewar_persona_env_vars("Nick");
        let mut keys: Vec<&str> = vars.iter().map(|(key, _)| key.as_str()).collect();
        keys.sort_unstable();
        assert_eq!(keys, ["USER", "USERNAME", "WINEUSERNAME"]);
        assert!(vars.iter().all(|(_, value)| value == "Nick"));
    }

    #[test]
    fn moshortcut_argument_wraps_the_title_verbatim() {
        assert_eq!(
            moshortcut_argument("Anomaly (DX11-AVX)"),
            "moshortcut://Anomaly (DX11-AVX)"
        );
    }

    #[test]
    fn modded_game_resolves_through_the_mo2_executable_and_runs_a_game_process() {
        assert_eq!(
            LaunchTarget::ModdedGame.executable_target(),
            LaunchTarget::ModOrganizer
        );
        assert_eq!(
            LaunchTarget::ModOrganizer.executable_target(),
            LaunchTarget::ModOrganizer
        );
        assert_eq!(LaunchTarget::Game.executable_target(), LaunchTarget::Game);
        assert!(LaunchTarget::ModdedGame.runs_game_process());
        assert!(LaunchTarget::Game.runs_game_process());
        assert!(!LaunchTarget::ModOrganizer.runs_game_process());
        assert!(!LaunchTarget::Launcher.runs_game_process());
    }

    #[test]
    fn modded_game_shares_the_mo2_process_category_but_has_its_own_label() {
        assert_eq!(
            category_for_target(LaunchTarget::ModdedGame),
            category_for_target(LaunchTarget::ModOrganizer)
        );
        assert_ne!(
            LaunchTarget::ModdedGame.label(),
            LaunchTarget::ModOrganizer.label()
        );
    }

    #[test]
    fn shortcut_title_defaults_and_falls_back_when_blank() {
        let mut config = AppConfig::default();
        assert_eq!(
            config.runner.effective_mo2_shortcut_title(),
            "Anomaly (DX11-AVX)"
        );
        config.runner.mo2_shortcut_title = "   ".to_string();
        assert_eq!(
            config.runner.effective_mo2_shortcut_title(),
            "Anomaly (DX11-AVX)"
        );
        config.runner.mo2_shortcut_title = "  My Shortcut  ".to_string();
        assert_eq!(config.runner.effective_mo2_shortcut_title(), "My Shortcut");
    }

    #[test]
    fn game_requests_stay_direct_when_mo2_cannot_be_resolved() {
        let config = AppConfig::default();
        assert!(!mo2_launch_available(&config));
        assert_eq!(
            effective_launch_target(&config, LaunchTarget::Game),
            LaunchTarget::Game
        );
    }

    #[test]
    fn game_requests_are_promoted_when_mo2_exists_and_headless_is_enabled() {
        let temp = tempfile::TempDir::new().unwrap();
        let gamma = temp.path().join("gamma");
        std::fs::create_dir_all(&gamma).unwrap();
        std::fs::write(gamma.join("ModOrganizer.exe"), b"").unwrap();
        let mut config = AppConfig::default();
        config.gamma_path = Some(gamma);
        assert!(mo2_launch_available(&config));
        assert_eq!(
            effective_launch_target(&config, LaunchTarget::Game),
            LaunchTarget::ModdedGame
        );
        config.runner.headless_mod_launch = false;
        assert!(!mo2_launch_available(&config));
        assert_eq!(
            effective_launch_target(&config, LaunchTarget::Game),
            LaunchTarget::Game
        );
    }

    #[test]
    fn only_game_requests_are_ever_promoted() {
        let temp = tempfile::TempDir::new().unwrap();
        let gamma = temp.path().join("gamma");
        std::fs::create_dir_all(&gamma).unwrap();
        std::fs::write(gamma.join("ModOrganizer.exe"), b"").unwrap();
        let mut config = AppConfig::default();
        config.gamma_path = Some(gamma);
        for target in [
            LaunchTarget::ModOrganizer,
            LaunchTarget::Launcher,
            LaunchTarget::ModdedGame,
        ] {
            assert_eq!(effective_launch_target(&config, target), target);
        }
    }

    #[test]
    fn spacewar_identity_applies_to_the_modded_game() {
        let mut config = AppConfig::default();
        config.spacewar.steam_spacewar_mode = true;
        assert!(spacewar_identity_applies(&config, LaunchTarget::ModdedGame));
    }

    #[test]
    fn wine_dll_overrides_disabling_lsteamclient_are_stripped_before_spawn() {
        let overrides = "openal32=n,b;lsteamclient=b;xaudio2_7=n";
        assert!(steam_identity::dll_override_disables_lsteamclient(overrides));
        let cleaned = steam_identity::strip_lsteamclient_override(overrides);
        assert!(!steam_identity::dll_override_disables_lsteamclient(&cleaned));
        assert_eq!(cleaned, "openal32=n,b;xaudio2_7=n");
    }
}