use crate::config::{expand_path, AppConfig, SPACEWAR_APPID};
use crate::error::{LauncherError, Result};
use crate::report::Reporter;
use crate::userltx::UserLtx;
use std::collections::BTreeSet;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

const STEAM_APPID_RELATIVE: [&str; 4] = [
    "steam_appid.txt",
    "bin/steam_appid.txt",
    "bin/x64/steam_appid.txt",
    "game/bin/steam_appid.txt",
];

const STEAM_SETTINGS_RELATIVE: [&str; 5] = [
    "steam_settings",
    "bin/steam_settings",
    "bin/x64/steam_settings",
    "game/steam_settings",
    "game/bin/steam_settings",
];

const STEAM_SETTINGS_FILES: [&str; 4] = [
    "force_account_name.txt",
    "settings/account_name.txt",
    "persona_name.txt",
    "settings/user_name.txt",
];

const STEAM_APPID_FILE_NAME: &str = "steam_appid.txt";
const MO2_APPID_RELATIVE_DIRS: [&str; 1] = ["overwrite"];
const USER_LTX_RELATIVE: [&str; 2] = ["appdata/user.ltx", "game/appdata/user.ltx"];
const LEGACY_IDENTITY: &str = "steamuser";
const NATIVE_STEAM_PATH_DEFAULT: &str = "$HOME/.local/share/Steam";
const USER_REG_FILE_NAME: &str = "user.reg";

#[derive(Debug, Clone, Default)]
pub struct IdentitySyncReport {
    pub steam_appid_written: Vec<PathBuf>,
    pub steam_exe_stub_disabled: Option<PathBuf>,
    pub steam_settings_updated: Vec<PathBuf>,
    pub user_ltx_updated: Vec<PathBuf>,
    pub warnings: Vec<String>,
}

pub fn effective_nickname(config: &AppConfig) -> String {
    let candidate = config.spacewar.player_nickname.trim();
    let candidate = if candidate.is_empty() || candidate.eq_ignore_ascii_case(LEGACY_IDENTITY) {
        let from_env = std::env::var("USER").unwrap_or_default();
        let from_env = from_env.trim();
        if from_env.is_empty() || from_env.eq_ignore_ascii_case(LEGACY_IDENTITY) {
            "Stalker".to_string()
        } else {
            from_env.to_string()
        }
    } else {
        candidate.to_string()
    };
    if candidate.eq_ignore_ascii_case(LEGACY_IDENTITY) {
        "Stalker".to_string()
    } else {
        candidate
    }
}

pub fn validate_nickname(nickname: &str) -> Vec<String> {
    let mut problems = Vec::new();
    if nickname.trim().is_empty() {
        problems.push("nickname cannot be empty".to_string());
        return problems;
    }
    if nickname.eq_ignore_ascii_case(LEGACY_IDENTITY) {
        problems.push(format!("nickname cannot be \"{LEGACY_IDENTITY}\""));
    }
    if !nickname
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == ' ' || c == '_' || c == '-')
    {
        problems.push(
            "nickname may only contain letters, digits, spaces, underscores and hyphens"
                .to_string(),
        );
    }
    problems
}

pub fn resolve_steam_path(config: &AppConfig) -> Option<PathBuf> {
    if let Some(custom) = config.spacewar.custom_steam_path.as_deref() {
        let expanded = expand_path(custom);
        if expanded.is_dir() {
            return Some(expanded);
        }
    }
    let default_path = expand_path(Path::new(NATIVE_STEAM_PATH_DEFAULT));
    if default_path.is_dir() {
        return Some(default_path);
    }
    None
}

#[derive(Debug, Clone, Default)]
pub struct SteamProcessScanResult {
    pub found: bool,
    pub matched_pids: Vec<(u32, String)>,
    pub flatpak_pids: Vec<(u32, String)>,
    pub scanned_process_count: usize,
    pub proc_read_error: Option<String>,
}

#[cfg(target_os = "linux")]
fn scan_proc_for_steam() -> SteamProcessScanResult {
    let mut result = SteamProcessScanResult::default();
    let entries = match fs::read_dir("/proc") {
        Ok(entries) => entries,
        Err(error) => {
            result.proc_read_error = Some(error.to_string());
            return result;
        }
    };
    for entry in entries.flatten() {
        let pid: u32 = match entry.file_name().to_str().and_then(|name| name.parse().ok()) {
            Some(pid) => pid,
            None => continue,
        };
        let comm = fs::read_to_string(format!("/proc/{pid}/comm"))
            .ok()
            .map(|value| value.trim().to_string())
            .unwrap_or_default();
        if comm.is_empty() {
            continue;
        }
        result.scanned_process_count += 1;
        let comm_lower = comm.to_lowercase();
        if comm_lower == "steam" {
            result.found = true;
            result.matched_pids.push((pid, comm.clone()));
        }
        if comm_lower == "pressure-vessel" || comm_lower.starts_with("bwrap") {
            let cmdline = fs::read_to_string(format!("/proc/{pid}/cmdline"))
                .map(|raw| raw.replace('\0', " ").trim().to_string())
                .unwrap_or_default();
            if cmdline.to_lowercase().contains("steam") {
                result.flatpak_pids.push((pid, comm.clone()));
            }
        }
    }
    result
}

#[cfg(not(target_os = "linux"))]
fn scan_proc_for_steam() -> SteamProcessScanResult {
    SteamProcessScanResult::default()
}

pub fn is_steam_running_now() -> bool {
    scan_proc_for_steam().found
}

pub fn is_steam_running_now_verbose(reporter: &Reporter) -> bool {
    let result = scan_proc_for_steam();
    if let Some(error) = &result.proc_read_error {
        reporter.warn(format!(
            "  [steam-detect] could not read /proc to scan for a native Steam process: {error}"
        ));
        return false;
    }
    reporter.info(format!(
        "  [steam-detect] scanned {} process entries under /proc",
        result.scanned_process_count
    ));
    if result.matched_pids.is_empty() {
        reporter.warn(
            "  [steam-detect] no process named \"steam\" was found running natively".to_string(),
        );
    } else {
        for (pid, comm) in &result.matched_pids {
            reporter.info(format!(
                "  [steam-detect] matched native Steam process: PID {pid}, comm=\"{comm}\""
            ));
        }
    }
    if !result.flatpak_pids.is_empty() {
        for (pid, comm) in &result.flatpak_pids {
            reporter.warn(format!(
                "  [steam-detect] detected a sandboxed Steam launch under flatpak/pressure-vessel: \
                 PID {pid}, comm=\"{comm}\" (this is not the same as a native Steam process)"
            ));
        }
    }
    result.found
}

pub fn spacewar_identity_env_vars(nickname: &str) -> Vec<(String, String)> {
    vec![
        ("SteamAppId".to_string(), SPACEWAR_APPID.to_string()),
        ("SteamGameId".to_string(), SPACEWAR_APPID.to_string()),
        ("SteamOverlayGameId".to_string(), SPACEWAR_APPID.to_string()),
        ("SteamPersonaName".to_string(), nickname.to_string()),
        ("STEAM_PERSONA_NAME".to_string(), nickname.to_string()),
        ("SteamPlayerName".to_string(), nickname.to_string()),
        ("STEAM_PLAYER_NAME".to_string(), nickname.to_string()),
    ]
}

pub fn spacewar_persona_env_vars(nickname: &str) -> Vec<(String, String)> {
    vec![
        ("WINEUSERNAME".to_string(), nickname.to_string()),
        ("USERNAME".to_string(), nickname.to_string()),
        ("USER".to_string(), nickname.to_string()),
    ]
}

const LSTEAMCLIENT_DLL: &str = "lsteamclient";

pub fn steam_ipc_env_vars() -> Vec<(String, String)> {
    let mut vars = Vec::new();
    if let Ok(xdg_runtime_dir) = std::env::var("XDG_RUNTIME_DIR") {
        if !xdg_runtime_dir.trim().is_empty() {
            vars.push(("XDG_RUNTIME_DIR".to_string(), xdg_runtime_dir));
        }
    }
    vars
}

pub fn dll_override_disables_lsteamclient(overrides: &str) -> bool {
    overrides.split(';').any(|entry| {
        let entry = entry.trim();
        let mut parts = entry.splitn(2, '=');
        let key = parts.next().unwrap_or("").trim();
        let value = parts.next().unwrap_or("").trim();
        if !key.eq_ignore_ascii_case(LSTEAMCLIENT_DLL) {
            return false;
        }
        value.is_empty() || value.eq_ignore_ascii_case("b") || value.eq_ignore_ascii_case("builtin")
    })
}

pub fn strip_lsteamclient_override(overrides: &str) -> String {
    overrides
        .split(';')
        .filter(|entry| {
            let key = entry.trim().split('=').next().unwrap_or("").trim();
            !key.eq_ignore_ascii_case(LSTEAMCLIENT_DLL)
        })
        .collect::<Vec<_>>()
        .join(";")
}

pub fn steam_appid_candidates(game_dir: &Path) -> Vec<PathBuf> {
    STEAM_APPID_RELATIVE
        .iter()
        .map(|relative| game_dir.join(relative))
        .collect()
}

pub fn steam_settings_candidates(game_dir: &Path) -> Vec<PathBuf> {
    STEAM_SETTINGS_RELATIVE
        .iter()
        .map(|relative| game_dir.join(relative))
        .collect()
}

pub fn steam_settings_file_names() -> &'static [&'static str] {
    &STEAM_SETTINGS_FILES
}

pub fn user_ltx_candidates(game_dir: &Path, mo2_dir: Option<&Path>) -> Vec<PathBuf> {
    let mut candidates: Vec<PathBuf> = USER_LTX_RELATIVE
        .iter()
        .map(|relative| game_dir.join(relative))
        .collect();
    if let Some(mo2_dir) = mo2_dir {
        candidates.extend(mo2_profile_ltx_paths(mo2_dir));
    }
    candidates
}

pub fn steam_exe_stub_path(wine_prefix: &Path) -> PathBuf {
    wine_prefix
        .join("drive_c")
        .join("Program Files (x86)")
        .join("Steam")
        .join("Steam.exe")
}

pub fn steam_appid_target_directories(
    game_dir: &Path,
    mo2_dir: Option<&Path>,
    executable_dirs: &[PathBuf],
) -> Vec<PathBuf> {
    let mut targets: BTreeSet<PathBuf> = BTreeSet::new();
    for relative in STEAM_APPID_RELATIVE {
        if let Some(parent) = game_dir.join(relative).parent() {
            targets.insert(parent.to_path_buf());
        }
    }
    if let Some(mo2_dir) = mo2_dir {
        targets.insert(mo2_dir.to_path_buf());
        for relative in MO2_APPID_RELATIVE_DIRS {
            targets.insert(mo2_dir.join(relative));
        }
        if let Ok(profile) = crate::mo2::resolve_active_profile(mo2_dir) {
            targets.insert(profile.path);
        }
    }
    for directory in executable_dirs {
        targets.insert(directory.clone());
    }
    targets.into_iter().collect()
}

fn write_and_verify_appid(
    directory: &Path,
    report: &mut IdentitySyncReport,
    reporter: &Reporter,
) {
    if let Err(error) = fs::create_dir_all(directory) {
        reporter.warn(format!(
            "  [identity] IO error creating {}: {error}",
            directory.display()
        ));
        report.warnings.push(format!(
            "could not create {}: {error}",
            directory.display()
        ));
        return;
    }
    let target = directory.join(STEAM_APPID_FILE_NAME);
    let already_present = target.is_file();
    let content = format!("{SPACEWAR_APPID}\n");
    let bytes = content.as_bytes();
    if let Err(error) = fs::write(&target, bytes) {
        reporter.warn(format!(
            "  [identity] IO error writing {}: {error}",
            target.display()
        ));
        report.warnings.push(format!(
            "could not write {}: {error}",
            target.display()
        ));
        return;
    }
    match fs::read(&target) {
        Ok(read_bytes) if read_bytes == bytes => {
            reporter.info(format!(
                "  [identity] {} steam_appid.txt at {} (appid={}, verified)",
                if already_present { "overwrote" } else { "wrote" },
                target.display(),
                SPACEWAR_APPID
            ));
            report.steam_appid_written.push(target);
        }
        Ok(read_bytes) => {
            reporter.warn(format!(
                "  [identity] verification failed for {}: expected \"{}\", found {} byte(s) of different content",
                target.display(),
                content.trim(),
                read_bytes.len()
            ));
            report.warnings.push(format!(
                "steam_appid.txt at {} did not verify",
                target.display()
            ));
        }
        Err(error) => {
            reporter.warn(format!(
                "  [identity] could not read back {}: {error}",
                target.display()
            ));
            report.warnings.push(format!(
                "could not verify {}: {error}",
                target.display()
            ));
        }
    }
}

pub fn write_steam_appid(
    game_dir: &Path,
    mo2_dir: Option<&Path>,
    executable_dirs: &[PathBuf],
    report: &mut IdentitySyncReport,
    reporter: &Reporter,
) -> Result<()> {
    reporter.info("  [identity] resolving steam_appid.txt placement targets".to_string());
    let targets = steam_appid_target_directories(game_dir, mo2_dir, executable_dirs);
    for directory in &targets {
        write_and_verify_appid(directory, report, reporter);
    }
    if report.steam_appid_written.is_empty() {
        return Err(LauncherError::SteamIdentity(
            "steam_appid.txt could not be written to any target directory".to_string(),
        ));
    }
    reporter.info(format!(
        "  [identity] steam_appid.txt verified in {} of {} target director(ies)",
        report.steam_appid_written.len(),
        targets.len()
    ));
    Ok(())
}

pub fn disable_goldberg_steam_stub(
    wine_prefix: &Path,
    game_dir: &Path,
    report: &mut IdentitySyncReport,
    reporter: &Reporter,
) -> Result<()> {
    let target = steam_exe_stub_path(wine_prefix);
    reporter.info(format!(
        "  [identity] real Spacewar mode is active, checking for a conflicting Goldberg \
         Steam.exe stub at {}",
        target.display()
    ));
    if target.is_file() {
        let disabled = target.with_extension("exe.disabled-by-gamma");
        if disabled.is_file() {
            if let Err(error) = fs::remove_file(&disabled) {
                reporter.warn(format!(
                    "  [identity] IO error removing stale {}: {error}",
                    disabled.display()
                ));
                report.warnings.push(format!(
                    "could not remove stale {}: {error}",
                    disabled.display()
                ));
            }
        }
        if !disabled.is_file() {
            match fs::rename(&target, &disabled) {
                Ok(()) => {
                    reporter.info(format!(
                        "  [identity] renamed conflicting Goldberg Steam.exe stub {} to {} so it can no \
                         longer intercept native Steam client calls",
                        target.display(),
                        disabled.display()
                    ));
                    report.steam_exe_stub_disabled = Some(disabled);
                }
                Err(error) => {
                    reporter.warn(format!(
                        "  [identity] IO error renaming Goldberg Steam.exe stub {}: {error}",
                        target.display()
                    ));
                    report.warnings.push(format!(
                        "could not rename conflicting Goldberg Steam.exe stub at {}: {error}",
                        target.display()
                    ));
                }
            }
        }
    } else {
        reporter.info(
            "  [identity] no Goldberg Steam.exe stub present, native Steam client bridge is clear"
                .to_string(),
        );
    }
    for relative in STEAM_SETTINGS_RELATIVE {
        let dir = game_dir.join(relative);
        if dir.is_dir() {
            let disabled = dir.with_extension("disabled-by-gamma");
            if disabled.is_dir() {
                let _ = fs::remove_dir_all(&disabled);
            }
            match fs::rename(&dir, &disabled) {
                Ok(()) => {
                    reporter.info(format!(
                        "  [identity] quarantined Goldberg steam_settings directory at {} to prevent stub conflicts",
                        dir.display()
                    ));
                }
                Err(error) => {
                    reporter.warn(format!(
                        "  [identity] failed to quarantine steam_settings directory at {}: {}",
                        dir.display(),
                        error
                    ));
                }
            }
        }
    }
    Ok(())
}

pub fn clean_poisoned_registry(wine_prefix: &Path, reporter: &Reporter) -> Result<()> {
    let path = wine_prefix.join(USER_REG_FILE_NAME);
    if !path.is_file() {
        return Ok(());
    }
    let original = match fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => return Ok(()),
    };
    if !original.contains("[Software\\\\Valve\\\\Steam]") {
        return Ok(());
    }
    
    let mut new_lines = Vec::new();
    let mut in_steam_section = false;
    for line in original.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with('[') {
            in_steam_section = trimmed.starts_with("[Software\\\\Valve\\\\Steam");
        }
        if !in_steam_section {
            new_lines.push(line);
        }
    }
    let cleaned = new_lines.join("\n") + "\n";
    
    let parent = path.parent().unwrap_or(wine_prefix);
    let mut tmp = tempfile::NamedTempFile::new_in(parent)?;
    tmp.write_all(cleaned.as_bytes())?;
    tmp.as_file().sync_all()?;
    tmp.persist(&path).map_err(|e| e.error)?;
    
    reporter.info("  [identity] cleaned poisoned Steam registry keys to let Proton rebuild them".to_string());
    Ok(())
}

pub fn mo2_profile_ltx_paths(mo2_dir: &Path) -> Vec<PathBuf> {
    let profiles_dir = mo2_dir.join("profiles");
    let entries = match fs::read_dir(&profiles_dir) {
        Ok(entries) => entries,
        Err(_) => return Vec::new(),
    };
    entries
        .flatten()
        .map(|entry| entry.path().join("user.ltx"))
        .filter(|path| path.is_file())
        .collect()
}

pub fn sync_user_ltx(
    game_dir: &Path,
    mo2_dir: Option<&Path>,
    nickname: &str,
    report: &mut IdentitySyncReport,
    reporter: &Reporter,
) -> Result<()> {
    reporter.info("  [identity] resolving user.ltx candidates for engine persona name".to_string());
    let base_candidates: Vec<PathBuf> = USER_LTX_RELATIVE
        .iter()
        .map(|relative| game_dir.join(relative))
        .collect();
    let mut candidates = Vec::new();
    for path in base_candidates {
        if path.is_file() {
            reporter.info(format!("  [identity] found user.ltx at {}", path.display()));
            candidates.push(path);
        } else {
            reporter.info(format!(
                "  [identity] user.ltx candidate not present: {}",
                path.display()
            ));
        }
    }
    if let Some(mo2_dir) = mo2_dir {
        let profile_paths = mo2_profile_ltx_paths(mo2_dir);
        reporter.info(format!(
            "  [identity] found {} MO2 profile user.ltx file(s) under {}",
            profile_paths.len(),
            mo2_dir.join("profiles").display()
        ));
        for path in &profile_paths {
            reporter.info(format!(
                "  [identity] found MO2 profile user.ltx at {}",
                path.display()
            ));
        }
        candidates.extend(profile_paths);
    }
    for path in candidates {
        let mut ltx = match UserLtx::open(&path) {
            Ok(ltx) => ltx,
            Err(error) => {
                reporter.warn(format!(
                    "  [identity] could not parse {}: {error}",
                    path.display()
                ));
                report
                    .warnings
                    .push(format!("could not parse {}: {error}", path.display()));
                continue;
            }
        };
        ltx.set("name", nickname);
        ltx.set("mm_net_player_name", nickname);
        match ltx.save(None) {
            Ok(()) => {
                reporter.info(format!(
                    "  [identity] updated \"name\" and \"mm_net_player_name\" to \"{nickname}\" in {}",
                    path.display()
                ));
                report.user_ltx_updated.push(path);
            }
            Err(error) => {
                reporter.warn(format!(
                    "  [identity] IO error saving {}: {error}",
                    path.display()
                ));
                report
                    .warnings
                    .push(format!("could not save {}: {error}", path.display()));
            }
        }
    }
    Ok(())
}

pub fn ensure_dotnet_temp_dir(wine_prefix: &Path) -> Result<PathBuf> {
    let temp_dir = wine_prefix.join("drive_c/temp");
    crate::fsutil::ensure_directory(&temp_dir)?;
    Ok(temp_dir)
}

fn resolve_wine_prefix(config: &AppConfig) -> Option<PathBuf> {
    config.runner.wine_prefix.as_deref().map(expand_path)
}

fn resolve_game_dir(config: &AppConfig) -> Option<PathBuf> {
    config.expanded_anomaly_dir()
}

fn resolve_mo2_dir(config: &AppConfig) -> Option<PathBuf> {
    config.expanded_gamma_dir()
}

pub fn sync_all(config: &AppConfig, reporter: &Reporter) -> Result<IdentitySyncReport> {
    let mut report = IdentitySyncReport::default();
    let game_dir = resolve_game_dir(config).ok_or_else(|| {
        LauncherError::SteamIdentity(
            "no Anomaly directory is configured, cannot sync Steam identity".to_string(),
        )
    })?;
    let wine_prefix = resolve_wine_prefix(config).ok_or_else(|| {
        LauncherError::SteamIdentity(
            "no Wine prefix is configured, cannot sync Steam identity".to_string(),
        )
    })?;
    let mo2_dir = resolve_mo2_dir(config);
    let nickname = effective_nickname(config);
    
    reporter.info(format!("[+] Syncing Steam Spacewar identity as \"{nickname}\""));
    reporter.info(format!("  [identity] game directory: {}", game_dir.display()));
    reporter.info(format!("  [identity] wine prefix: {}", wine_prefix.display()));
    
    let executable_dirs = crate::runner::resolved_executable_directories(config);
    
    write_steam_appid(&game_dir, mo2_dir.as_deref(), &executable_dirs, &mut report, reporter)?;
    disable_goldberg_steam_stub(&wine_prefix, &game_dir, &mut report, reporter)?;
    
    reporter.info(
        "  [identity] real Spacewar mode is active, delegating Steam registry bridge entirely to Proton/umu-run"
            .to_string(),
    );
    
    clean_poisoned_registry(&wine_prefix, reporter).unwrap_or_else(|e| {
        reporter.warn(format!("  [identity] failed to clean registry: {e}"));
    });
    
    sync_user_ltx(&game_dir, mo2_dir.as_deref(), &nickname, &mut report, reporter)?;
    ensure_dotnet_temp_dir(&wine_prefix)?;
    
    for warning in &report.warnings {
        reporter.warn(format!("  ! {warning}"));
    }
    
    reporter.info(format!(
        "[*] Steam identity sync complete: {} appid file(s), {} user.ltx file(s)",
        report.steam_appid_written.len(),
        report.user_ltx_updated.len()
    ));
    
    Ok(report)
}