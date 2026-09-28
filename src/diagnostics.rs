use crate::config::AppConfig;
use crate::fsutil;
use crate::process::SharedProcessRegistry;
use crate::steam_identity;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagnosticStatus {
    Pass,
    Warn,
    Fail,
}

#[derive(Debug, Clone)]
pub struct DiagnosticCheck {
    pub label: String,
    pub status: DiagnosticStatus,
    pub detail: String,
}

#[derive(Debug, Clone, Default)]
pub struct DiagnosticReport {
    pub checks: Vec<DiagnosticCheck>,
}

impl DiagnosticReport {
    pub fn worst(&self) -> DiagnosticStatus {
        let mut worst = DiagnosticStatus::Pass;

        for check in &self.checks {
            match check.status {
                DiagnosticStatus::Fail => return DiagnosticStatus::Fail,
                DiagnosticStatus::Warn => worst = DiagnosticStatus::Warn,
                DiagnosticStatus::Pass => {}
            }
        }

        worst
    }
}

fn check(label: &str, status: DiagnosticStatus, detail: impl Into<String>) -> DiagnosticCheck {
    DiagnosticCheck {
        label: label.to_string(),
        status,
        detail: detail.into(),
    }
}

fn check_steam_status(adopted_processes: &SharedProcessRegistry) -> DiagnosticCheck {
    let steam_installed = fsutil::which("steam").is_some();
    let steam_running = adopted_processes.is_steam_running();

    if !steam_installed {
        return check(
            "Steam installation",
            DiagnosticStatus::Warn,
            "No 'steam' binary was found in PATH. Spacewar mode still works through the identity files and env vars, but the real Steam client is unavailable on this system.",
        );
    }

    if steam_running {
        check(
            "Steam installation",
            DiagnosticStatus::Pass,
            "Steam is installed and currently running.",
        )
    } else {
        check(
            "Steam installation",
            DiagnosticStatus::Warn,
            "Steam is installed but does not appear to be running right now.",
        )
    }
}

fn check_adopted_processes(adopted_processes: &SharedProcessRegistry) -> DiagnosticCheck {
    let snapshot = adopted_processes.snapshot();

    if snapshot.is_empty() {
        return check(
            "Adopted process table",
            DiagnosticStatus::Pass,
            "No Game, Mod Organizer 2 or Wine/Proton helper processes are currently tracked.",
        );
    }

    let mut counts: Vec<(String, usize)> = Vec::new();
    for process in &snapshot {
        let label = process.category.label().to_string();
        if let Some(entry) = counts.iter_mut().find(|(name, _)| *name == label) {
            entry.1 += 1;
        } else {
            counts.push((label, 1));
        }
    }

    let summary = counts
        .into_iter()
        .map(|(label, count)| format!("{count} {label}"))
        .collect::<Vec<_>>()
        .join(", ");

    check(
        "Adopted process table",
        DiagnosticStatus::Warn,
        format!("Currently tracking: {summary}. This is informational, not a failure — it just means something is already running."),
    )
}

fn check_nickname(config: &AppConfig) -> DiagnosticCheck {
    let problems = steam_identity::validate_nickname(&config.spacewar.player_nickname);

    if problems.is_empty() {
        check(
            "Multiplayer nickname",
            DiagnosticStatus::Pass,
            format!(
                "'{}' is a valid nickname.",
                config.spacewar.player_nickname.trim()
            ),
        )
    } else {
        check(
            "Multiplayer nickname",
            DiagnosticStatus::Fail,
            problems.join("; "),
        )
    }
}

fn check_steam_appid(config: &AppConfig) -> DiagnosticCheck {
    let game_dir = match config.expanded_anomaly_dir() {
        Some(dir) => dir,
        None => {
            return check(
                "steam_appid.txt",
                DiagnosticStatus::Fail,
                "Anomaly directory is not configured, so steam_appid.txt cannot be located.",
            )
        }
    };

    let candidates = steam_identity::steam_appid_candidates(&game_dir);
    let mut correct = Vec::new();
    let mut wrong = Vec::new();

    for candidate in candidates {
        if !candidate.is_file() {
            continue;
        }

        match std::fs::read_to_string(&candidate) {
            Ok(text) if text.trim() == "480" => correct.push(candidate),
            _ => wrong.push(candidate),
        }
    }

    if !wrong.is_empty() {
        check(
            "steam_appid.txt",
            DiagnosticStatus::Fail,
            format!(
                "{} file(s) exist but do not contain exactly \"480\": {}",
                wrong.len(),
                paths_to_string(&wrong)
            ),
        )
    } else if correct.is_empty() {
        check(
            "steam_appid.txt",
            DiagnosticStatus::Warn,
            "No steam_appid.txt was found yet at any known location. It is written automatically the next time the game launches with Spacewar mode enabled.",
        )
    } else {
        check(
            "steam_appid.txt",
            DiagnosticStatus::Pass,
            format!("{} file(s) correctly contain \"480\".", correct.len()),
        )
    }
}

fn check_steam_exe_stub(config: &AppConfig) -> DiagnosticCheck {
    let wine_prefix = match config.runner.wine_prefix.as_deref() {
        Some(prefix) => crate::config::expand_path(prefix),
        None => {
            return check(
                "Steam.exe stub",
                DiagnosticStatus::Fail,
                "Wine prefix is not configured, so the Steam.exe stub cannot be located.",
            )
        }
    };

    let stub_path = steam_identity::steam_exe_stub_path(&wine_prefix);

    if stub_path.is_file() {
        check(
            "Steam.exe stub",
            DiagnosticStatus::Fail,
            format!(
                "A Goldberg Steam.exe stub is present at {}. This intercepts Wine's native Steam client bridge and breaks real Steamworks multiplayer; it is renamed out of the way automatically the next time the game launches with Spacewar mode enabled.",
                stub_path.display()
            ),
        )
    } else {
        check(
            "Steam.exe stub",
            DiagnosticStatus::Pass,
            "No conflicting Goldberg Steam.exe stub is present, so Wine's native Steam client bridge (lsteamclient) is free to handle Steamworks calls.",
        )
    }
}

fn check_user_ltx(config: &AppConfig) -> DiagnosticCheck {
    let game_dir = match config.expanded_anomaly_dir() {
        Some(dir) => dir,
        None => {
            return check(
                "user.ltx identity",
                DiagnosticStatus::Fail,
                "Anomaly directory is not configured, so user.ltx cannot be located.",
            )
        }
    };

    let mo2_dir = config.expanded_gamma_dir();
    let nickname = steam_identity::effective_nickname(config);
    let candidates = steam_identity::user_ltx_candidates(&game_dir, mo2_dir.as_deref());
    let existing: Vec<_> = candidates.into_iter().filter(|path| path.is_file()).collect();

    if existing.is_empty() {
        return check(
            "user.ltx identity",
            DiagnosticStatus::Warn,
            "No user.ltx was found yet. It is written the first time Anomaly or a Mod Organizer 2 profile runs.",
        );
    }

    let mut mismatched = Vec::new();

    for path in &existing {
        let text = std::fs::read_to_string(path).unwrap_or_default();
        let contains_nickname = text.contains(&format!("name {nickname}"))
            || text.contains(&format!("name  {nickname}"))
            || text.contains(&nickname);
        let contains_legacy = text.to_lowercase().contains("steamuser");

        if !contains_nickname || contains_legacy {
            mismatched.push(path.clone());
        }
    }

    if mismatched.is_empty() {
        check(
            "user.ltx identity",
            DiagnosticStatus::Pass,
            format!("{} file(s) already reflect the configured nickname.", existing.len()),
        )
    } else {
        check(
            "user.ltx identity",
            DiagnosticStatus::Warn,
            format!(
                "{} of {} file(s) do not yet reflect the configured nickname: {}. They will be updated on the next Spacewar launch.",
                mismatched.len(),
                existing.len(),
                paths_to_string(&mismatched)
            ),
        )
    }
}

fn check_steam_settings(config: &AppConfig) -> DiagnosticCheck {
    let game_dir = match config.expanded_anomaly_dir() {
        Some(dir) => dir,
        None => {
            return check(
                "steam_settings identity",
                DiagnosticStatus::Fail,
                "Anomaly directory is not configured, so steam_settings cannot be located.",
            )
        }
    };

    let nickname = steam_identity::effective_nickname(config);
    let candidates = steam_identity::steam_settings_candidates(&game_dir);
    let existing_dirs: Vec<_> = candidates.into_iter().filter(|dir| dir.is_dir()).collect();

    if existing_dirs.is_empty() {
        return check(
            "steam_settings identity",
            DiagnosticStatus::Warn,
            "No steam_settings directory was found yet. It is created the next time the game launches with Spacewar mode enabled.",
        );
    }

    let mut mismatched = Vec::new();

    for dir in &existing_dirs {
        for file_name in steam_identity::steam_settings_file_names() {
            let file_path = dir.join(file_name);
            if !file_path.is_file() {
                continue;
            }

            let content = std::fs::read_to_string(&file_path).unwrap_or_default();
            if content.trim() != nickname {
                mismatched.push(file_path);
            }
        }
    }

    if mismatched.is_empty() {
        check(
            "steam_settings identity",
            DiagnosticStatus::Pass,
            format!("{} director(y/ies) already reflect the configured nickname.", existing_dirs.len()),
        )
    } else {
        check(
            "steam_settings identity",
            DiagnosticStatus::Warn,
            format!(
                "{} file(s) do not yet reflect the configured nickname: {}. They will be overwritten on the next Spacewar launch.",
                mismatched.len(),
                paths_to_string(&mismatched)
            ),
        )
    }
}

fn check_graphics_tooling() -> DiagnosticCheck {
    let vulkan_ok = fsutil::which("vulkaninfo").is_some()
        || Path::new("/usr/share/vulkan/icd.d").read_dir().map(|mut entries| entries.next().is_some()).unwrap_or(false);
    let gamemode_ok = fsutil::tool_available("gamemoderun");

    match (vulkan_ok, gamemode_ok) {
        (true, true) => check(
            "Graphics & performance tooling",
            DiagnosticStatus::Pass,
            "Vulkan ICDs and gamemoderun were both found.",
        ),
        (true, false) => check(
            "Graphics & performance tooling",
            DiagnosticStatus::Warn,
            "Vulkan ICDs were found, but gamemoderun is missing. GameMode will be unavailable even if enabled in Tweaks.",
        ),
        (false, true) => check(
            "Graphics & performance tooling",
            DiagnosticStatus::Fail,
            "No Vulkan ICD was found. DXVK requires Vulkan to run at all.",
        ),
        (false, false) => check(
            "Graphics & performance tooling",
            DiagnosticStatus::Fail,
            "No Vulkan ICD was found and gamemoderun is missing.",
        ),
    }
}

fn paths_to_string(paths: &[std::path::PathBuf]) -> String {
    paths
        .iter()
        .map(|path| path.display().to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

pub fn run(config: &AppConfig, adopted_processes: &SharedProcessRegistry) -> DiagnosticReport {
    let checks = vec![
        check_steam_status(adopted_processes),
        check_adopted_processes(adopted_processes),
        check_nickname(config),
        check_steam_appid(config),
        check_steam_exe_stub(config),
        check_user_ltx(config),
        check_steam_settings(config),
        check_graphics_tooling(),
    ];

    DiagnosticReport { checks }
}
