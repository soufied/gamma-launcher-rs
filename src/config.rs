use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const FOLDERS_TO_INSTALL: [&str; 3] = ["appdata", "db", "gamedata"];
pub const CONFIG_FILE_NAME: &str = "config.toml";
pub const DOWNLOADS_DIR_NAME: &str = "downloads";
pub const MODS_DIR_NAME: &str = "mods";
pub const GROK_INSTALLER_DIR_NAME: &str = ".Grok's Modpack Installer";
pub const MODPACK_DIR_NAME: &str = "G.A.M.M.A";
const FALLBACK_PROTON_PATH: &str = "/usr/share/steam/compatibilitytools.d/proton-cachyos-slr";
pub const NATIVE_STEAM_PATH_DEFAULT: &str = "$HOME/.local/share/Steam";
pub const SPACEWAR_APPID: &str = "480";

fn default_true() -> bool {
    true
}

fn default_false() -> bool {
    false
}

pub fn detected_cpu_count() -> u32 {
    std::thread::available_parallelism()
        .map(|value| value.get() as u32)
        .unwrap_or(4)
}

fn default_gameid() -> String {
    "stalker-anomaly-gamma".to_string()
}

pub const DEFAULT_MO2_SHORTCUT_TITLE: &str = "Anomaly (DX11-AVX)";

fn default_mo2_shortcut_title() -> String {
    DEFAULT_MO2_SHORTCUT_TITLE.to_string()
}

fn default_wine_prefix() -> Option<PathBuf> {
    Some(PathBuf::from(
        "$HOME/.local/share/wineprefixes/stalker_anomaly_gamma",
    ))
}

fn default_proton_path() -> Option<PathBuf> {
    let mut messages = Vec::new();
    let detected = crate::detect::detect_proton(&mut messages);
    Some(detected.unwrap_or_else(|| PathBuf::from(FALLBACK_PROTON_PATH)))
}

fn default_gamemode_enabled() -> bool {
    crate::fsutil::tool_available("gamemoderun")
}

fn default_umu_enabled() -> bool {
    crate::fsutil::tool_available("umu-run")
}

fn default_cache_path() -> Option<PathBuf> {
    Some(PathBuf::from("/tmp"))
}

fn default_proxy_port() -> u16 {
    1080
}

fn default_retry_threshold() -> usize {
    2
}

pub fn default_dll_overrides() -> String {
    "usvfs_x64=n,b;usvfs_x86=n,b;d3dcompiler_47=n,b;d3dcompiler_43=n,b;d3dx11_43=n,b;d3dx10_43=n,b;d3dx9_43=n,b".to_string()
}

fn default_omp_threads() -> u32 {
    detected_cpu_count()
}

pub fn default_dxvk_config() -> String {
    format!(
        "dxvk.numCompilerThreads = {}; d3d11.maxTessFactor = 8",
        detected_cpu_count()
    )
}

fn default_player_nickname() -> String {
    match std::env::var("USER") {
        Ok(value) if !value.trim().is_empty() && !value.trim().eq_ignore_ascii_case("steamuser") => {
            value.trim().to_string()
        }
        _ => "Stalker".to_string(),
    }
}

pub fn default_xray_dll_overrides() -> String {
    "openal32=n,b;d3dcompiler_47=n,b".to_string()
}

fn default_mangohud_enabled() -> bool {
    crate::fsutil::tool_available("mangohud")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunnerSettings {
    #[serde(default = "default_proton_path")]
    pub proton_path: Option<PathBuf>,
    #[serde(default = "default_wine_prefix")]
    pub wine_prefix: Option<PathBuf>,
    #[serde(default = "default_umu_enabled")]
    pub use_umu: bool,
    #[serde(default = "default_gamemode_enabled")]
    pub use_gamemode: bool,
    #[serde(default = "default_gameid")]
    pub umu_id: String,
    #[serde(default = "default_true")]
    pub wine_dll_overrides_enabled: bool,
    #[serde(default = "default_dll_overrides")]
    pub wine_dll_overrides: String,
    #[serde(default = "default_true")]
    pub omp_threads_enabled: bool,
    #[serde(default = "default_omp_threads")]
    pub omp_threads: u32,
    #[serde(default = "default_true")]
    pub dxvk_config_enabled: bool,
    #[serde(default = "default_dxvk_config")]
    pub dxvk_config: String,
    #[serde(default = "default_false")]
    pub fsr_enabled: bool,
    #[serde(default)]
    pub extra_env: Vec<(String, String)>,
    #[serde(default)]
    pub mo2_executable: Option<PathBuf>,
    #[serde(default)]
    pub launcher_executable: Option<PathBuf>,
    #[serde(default)]
    pub game_executable: Option<PathBuf>,
    #[serde(default = "default_mo2_shortcut_title")]
    pub mo2_shortcut_title: String,
    #[serde(default = "default_true")]
    pub headless_mod_launch: bool,
}

impl Default for RunnerSettings {
    fn default() -> Self {
        Self {
            proton_path: default_proton_path(),
            wine_prefix: default_wine_prefix(),
            use_umu: default_umu_enabled(),
            use_gamemode: default_gamemode_enabled(),
            umu_id: default_gameid(),
            wine_dll_overrides_enabled: true,
            wine_dll_overrides: default_dll_overrides(),
            omp_threads_enabled: true,
            omp_threads: default_omp_threads(),
            dxvk_config_enabled: true,
            dxvk_config: default_dxvk_config(),
            fsr_enabled: false,
            extra_env: Vec::new(),
            mo2_executable: None,
            launcher_executable: None,
            game_executable: None,
            mo2_shortcut_title: default_mo2_shortcut_title(),
            headless_mod_launch: true,
        }
    }
}

impl RunnerSettings {
    pub fn effective_mo2_shortcut_title(&self) -> String {
        let trimmed = self.mo2_shortcut_title.trim();
        if trimmed.is_empty() {
            DEFAULT_MO2_SHORTCUT_TITLE.to_string()
        } else {
            trimmed.to_string()
        }
    }

    pub fn reset_dll_overrides(&mut self) {
        self.wine_dll_overrides = default_dll_overrides();
    }

    pub fn reset_dxvk_config(&mut self) {
        self.dxvk_config = default_dxvk_config();
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PresentMode {
    Mailbox,
    Immediate,
    Fifo,
    RelaxedFifo,
}

impl PresentMode {
    pub fn mesa_value(&self) -> &'static str {
        match self {
            PresentMode::Mailbox => "mailbox",
            PresentMode::Immediate => "immediate",
            PresentMode::Fifo => "fifo",
            PresentMode::RelaxedFifo => "relaxed",
        }
    }
}

impl Default for PresentMode {
    fn default() -> Self {
        PresentMode::Mailbox
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SyncMode {
    Fsync,
    Esync,
    SystemDefault,
}

impl Default for SyncMode {
    fn default() -> Self {
        SyncMode::Fsync
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpacewarSettings {
    #[serde(default = "default_false")]
    pub steam_spacewar_mode: bool,
    #[serde(default = "default_player_nickname")]
    pub player_nickname: String,
    #[serde(default = "default_true")]
    pub force_nickname_override: bool,
    #[serde(default)]
    pub custom_steam_path: Option<PathBuf>,
    #[serde(default = "default_true")]
    pub steam_check_running: bool,
}

impl Default for SpacewarSettings {
    fn default() -> Self {
        Self {
            steam_spacewar_mode: false,
            player_nickname: default_player_nickname(),
            force_nickname_override: true,
            custom_steam_path: None,
            steam_check_running: true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CloseAction {
    Exit,
    MinimizeToTray,
}

impl Default for CloseAction {
    fn default() -> Self {
        CloseAction::Exit
    }
}

impl CloseAction {
    pub fn label(&self) -> &'static str {
        match self {
            CloseAction::Exit => "Exit",
            CloseAction::MinimizeToTray => "Minimize to tray",
        }
    }

    pub fn description(&self) -> &'static str {
        match self {
            CloseAction::Exit => {
                "Closing the window terminates the launcher immediately and cleanly. Recommended on KDE Plasma 6 / Wayland to avoid ghost taskbar entries."
            }
            CloseAction::MinimizeToTray => {
                "Closing the window hides the launcher to the system tray instead of quitting it. Any game or MO2 process already running keeps running."
            }
        }
    }
}

fn default_close_action() -> CloseAction {
    CloseAction::Exit
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraySettings {
    #[serde(default = "default_true")]
    pub minimize_to_tray: bool,
    #[serde(default = "default_close_action")]
    pub close_action: CloseAction,
    #[serde(default = "default_false")]
    pub start_in_tray: bool,
}

impl Default for TraySettings {
    fn default() -> Self {
        Self {
            minimize_to_tray: true,
            close_action: default_close_action(),
            start_in_tray: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphicsSettings {
    #[serde(default)]
    pub fps_limit: u32,
    #[serde(default = "default_true")]
    pub dxvk_async: bool,
    #[serde(default = "default_true")]
    pub dxvk_state_cache: bool,
    #[serde(default)]
    pub vk_wsi_present_mode: PresentMode,
    #[serde(default = "default_true")]
    pub nvidia_shader_cache_optimization: bool,
    #[serde(default = "default_true")]
    pub mesa_shader_cache_optimization: bool,
    #[serde(default = "default_true")]
    pub wine_large_address_aware: bool,
    #[serde(default)]
    pub sync_mechanism: SyncMode,
    #[serde(default = "default_xray_dll_overrides")]
    pub dll_overrides: String,
    #[serde(default = "default_mangohud_enabled")]
    pub enable_mangohud: bool,
}

impl GraphicsSettings {
    pub fn reset_dll_overrides(&mut self) {
        self.dll_overrides = default_xray_dll_overrides();
    }
}

impl Default for GraphicsSettings {
    fn default() -> Self {
        Self {
            fps_limit: 0,
            dxvk_async: true,
            dxvk_state_cache: true,
            vk_wsi_present_mode: PresentMode::default(),
            nvidia_shader_cache_optimization: true,
            mesa_shader_cache_optimization: true,
            wine_large_address_aware: true,
            sync_mechanism: SyncMode::default(),
            dll_overrides: default_xray_dll_overrides(),
            enable_mangohud: default_mangohud_enabled(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    #[serde(default)]
    pub anomaly_path: Option<PathBuf>,
    #[serde(default)]
    pub gamma_path: Option<PathBuf>,
    #[serde(default = "default_cache_path")]
    pub cache_path: Option<PathBuf>,
    #[serde(default = "default_repository")]
    pub custom_gamma_repository: String,
    #[serde(default)]
    pub custom_gamma_revision: Option<String>,
    #[serde(default)]
    pub preserve_user_config: bool,
    #[serde(default = "default_true")]
    pub update_gamma_definition: bool,
    #[serde(default = "default_true")]
    pub patch_anomaly: bool,
    #[serde(default = "default_true")]
    pub anomaly_verify: bool,
    #[serde(default = "default_true")]
    pub anomaly_purge_cache: bool,
    #[serde(default = "default_true")]
    pub purge_unused_downloads: bool,
    #[serde(default = "default_true")]
    pub update_download_cache: bool,
    #[serde(default = "default_false")]
    pub install_mod_organizer: bool,
    #[serde(default = "default_mo_version")]
    pub mo_version: String,
    #[serde(default)]
    pub usvfs_final_path: Option<PathBuf>,
    #[serde(default = "default_false")]
    pub force_recheck: bool,
    #[serde(default = "default_true")]
    pub dark_mode: bool,
    #[serde(default)]
    pub runner: RunnerSettings,
    #[serde(default)]
    pub proxy: ProxyConfig,
    #[serde(default)]
    pub spacewar: SpacewarSettings,
    #[serde(default)]
    pub tray: TraySettings,
    #[serde(default)]
    pub graphics: GraphicsSettings,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProxyConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub host: String,
    #[serde(default = "default_proxy_port")]
    pub port: u16,
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub password: String,
    #[serde(default = "default_false")]
    pub socks5_auto_retry: bool,
    #[serde(default = "default_retry_threshold")]
    pub socks5_retry_threshold: usize,
}

impl Default for ProxyConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            host: String::new(),
            port: default_proxy_port(),
            username: String::new(),
            password: String::new(),
            socks5_auto_retry: false,
            socks5_retry_threshold: default_retry_threshold(),
        }
    }
}

impl ProxyConfig {
    pub fn effective_threshold(&self) -> usize {
        self.socks5_retry_threshold.max(1)
    }
}

fn default_repository() -> String {
    "Grokitach/Stalker_GAMMA".to_string()
}

fn default_mo_version() -> String {
    "v2.5.2".to_string()
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            anomaly_path: None,
            gamma_path: None,
            cache_path: default_cache_path(),
            custom_gamma_repository: default_repository(),
            custom_gamma_revision: None,
            preserve_user_config: false,
            update_gamma_definition: true,
            patch_anomaly: true,
            anomaly_verify: true,
            anomaly_purge_cache: true,
            purge_unused_downloads: true,
            update_download_cache: true,
            install_mod_organizer: false,
            mo_version: default_mo_version(),
            usvfs_final_path: None,
            force_recheck: false,
            dark_mode: true,
            runner: RunnerSettings::default(),
            proxy: ProxyConfig::default(),
            spacewar: SpacewarSettings::default(),
            tray: TraySettings::default(),
            graphics: GraphicsSettings::default(),
        }
    }
}

impl AppConfig {
    pub fn executable_dir() -> Result<PathBuf> {
        let exe = std::env::current_exe().context("could not determine the running executable path")?;
        let parent = exe
            .parent()
            .context("the running executable has no parent directory")?;
        Ok(parent.to_path_buf())
    }

    pub fn config_path() -> Result<PathBuf> {
        Ok(Self::executable_dir()?.join(CONFIG_FILE_NAME))
    }

    pub fn load() -> Self {
        Self::try_load().unwrap_or_default()
    }

    fn try_load() -> Result<Self> {
        let path = Self::config_path()?;
        let data = std::fs::read_to_string(&path)?;
        let config = toml::from_str(&data).context("failed to parse config.toml")?;
        Ok(config)
    }

    pub fn save(&self) -> Result<()> {
        let path = Self::config_path()?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let data = toml::to_string_pretty(self).context("failed to serialize config.toml")?;
        std::fs::write(path, data)?;
        Ok(())
    }

    pub fn expanded_gamma_dir(&self) -> Option<PathBuf> {
        self.gamma_path.as_deref().map(expand_path)
    }

    pub fn expanded_anomaly_dir(&self) -> Option<PathBuf> {
        self.anomaly_path.as_deref().map(expand_path)
    }

    pub fn downloads_dir(&self) -> Option<PathBuf> {
        self.expanded_gamma_dir()
            .map(|gamma| gamma.join(DOWNLOADS_DIR_NAME))
    }

    pub fn mods_dir(&self) -> Option<PathBuf> {
        self.expanded_gamma_dir().map(|gamma| gamma.join(MODS_DIR_NAME))
    }

    pub fn grok_installer_dir(&self) -> Option<PathBuf> {
        self.expanded_gamma_dir()
            .map(|gamma| gamma.join(GROK_INSTALLER_DIR_NAME))
    }

    pub fn effective_cache_dir(&self) -> Option<PathBuf> {
        self.cache_path
            .as_deref()
            .map(expand_path)
            .or_else(|| self.expanded_anomaly_dir())
    }
}

pub fn expand_path(p: &Path) -> PathBuf {
    let raw = p.to_string_lossy().to_string();
    if let Some(stripped) = raw.strip_prefix("$HOME") {
        if let Some(home) = dirs::home_dir() {
            let remainder = stripped.trim_start_matches(['/', '\\']);
            return if remainder.is_empty() {
                home
            } else {
                home.join(remainder)
            };
        }
    }
    if let Ok(stripped) = p.strip_prefix("~") {
        if let Some(home) = dirs::home_dir() {
            return home.join(stripped);
        }
    }
    p.to_path_buf()
}
