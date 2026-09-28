use crate::commands::install::{FullInstall, PROFILE_SETTINGS};
use crate::commands::CommandArgs;
use crate::config::{
    default_dll_overrides, default_dxvk_config, expand_path, AppConfig, DOWNLOADS_DIR_NAME,
    MODPACK_DIR_NAME, MODS_DIR_NAME,
};
use crate::error::{LauncherError, Result};
use crate::fsutil::{self, PermissionOutcome};
use crate::mods::{modpack_data_dir, read_mod_maker};
use crate::process::{self, SharedProcessRegistry};
use crate::report::{human_bytes, Reporter};
use crate::runner::{escape_ini_backslashes, linux_to_wine_path, repaired_wine_path, wine_path_needs_repair};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

const TEMP_PREFIXES: [&str; 2] = ["gamma-launcher-", "gamma_launcher-"];
const INCOMPLETE_SUFFIXES: [&str; 5] = [".part", ".tmp", ".temp", ".crdownload", ".download"];
const ARCHIVE_SUFFIXES: [&str; 3] = [".zip", ".7z", ".rar"];
const GRAPHICS_CACHE_SUFFIXES: [&str; 4] = [
    ".dxvk-cache",
    ".dxvk-shader-cache",
    ".nvidiacache",
    ".d3d11cache",
];
const PREFIX_GRAPHICS_CACHES: [&str; 3] = [
    "drive_c/users/steamuser/AppData/Local/NVIDIA/GLCache",
    "drive_c/users/steamuser/AppData/Local/AMD/DxCache",
    "drive_c/ProgramData/NVIDIA Corporation/NV_Cache",
];
const MISSING_PREVIEW_LIMIT: usize = 8;
const INI_FILE_NAME: &str = "ModOrganizer.ini";
const INI_BACKUP_NAME: &str = "ModOrganizer.ini.bak";

#[derive(Debug, Clone, Copy, Default)]
pub struct SpaceEstimate {
    pub downloads: u64,
    pub incomplete: u64,
    pub temp_cache: u64,
}

fn has_suffix(path: &Path, suffixes: &[&str]) -> bool {
    let name = match path.file_name().and_then(|name| name.to_str()) {
        Some(name) => name.to_lowercase(),
        None => return false,
    };

    suffixes.iter().any(|suffix| name.ends_with(suffix))
}

fn has_temp_prefix(name: &str) -> bool {
    TEMP_PREFIXES
        .iter()
        .any(|prefix| name.starts_with(prefix))
}

fn top_level_files(dir: &Path) -> Vec<PathBuf> {
    match fs::read_dir(dir) {
        Ok(entries) => entries
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| path.is_file())
            .collect(),
        Err(_) => Vec::new(),
    }
}

fn incomplete_downloads(dir: &Path) -> Vec<PathBuf> {
    top_level_files(dir)
        .into_iter()
        .filter(|path| has_suffix(path, &INCOMPLETE_SUFFIXES))
        .collect()
}

fn launcher_temp_directories() -> Vec<PathBuf> {
    let temp = std::env::temp_dir();

    match fs::read_dir(&temp) {
        Ok(entries) => entries
            .flatten()
            .filter(|entry| {
                entry
                    .file_name()
                    .to_str()
                    .map(has_temp_prefix)
                    .unwrap_or(false)
            })
            .map(|entry| entry.path())
            .collect(),
        Err(_) => Vec::new(),
    }
}

pub fn estimate_reclaimable(config: &AppConfig) -> SpaceEstimate {
    let mut estimate = SpaceEstimate::default();

    if let Some(downloads) = config.downloads_dir() {
        estimate.downloads = fsutil::directory_size(&downloads);
        estimate.incomplete = incomplete_downloads(&downloads)
            .iter()
            .map(|path| fsutil::entry_size(path))
            .sum();
    }

    estimate.temp_cache = launcher_temp_directories()
        .iter()
        .map(|path| fsutil::entry_size(path))
        .sum();

    estimate
}

fn remove_entries(
    entries: Vec<PathBuf>,
    what: &str,
    reporter: &Reporter,
) -> Result<(usize, u64)> {
    let mut removed = 0usize;
    let mut freed = 0u64;
    let mut failures = 0usize;

    for entry in entries {
        reporter.checkpoint()?;

        match fsutil::remove_path(&entry) {
            Ok(size) => {
                reporter.info(format!(
                    "  - Removed {} ({})",
                    entry.display(),
                    human_bytes(size)
                ));
                removed += 1;
                freed += size;
            }
            Err(error) => {
                failures += 1;
                reporter.warn(format!("  ! Could not remove {}: {error}", entry.display()));
            }
        }
    }

    reporter.info(format!(
        "[+] {what}: {removed} entry/entries removed, {} reclaimed, {failures} failure(s)",
        human_bytes(freed)
    ));

    if failures > 0 && removed == 0 {
        return Err(LauncherError::Other(format!(
            "{what} could not remove anything, check the permissions of the reported paths"
        )));
    }

    Ok((removed, freed))
}

fn directory_entries(dir: &Path) -> Vec<PathBuf> {
    match fs::read_dir(dir) {
        Ok(entries) => entries.flatten().map(|entry| entry.path()).collect(),
        Err(_) => Vec::new(),
    }
}

pub fn purge_downloads(args: &CommandArgs, reporter: &Reporter) -> Result<()> {
    let downloads = args.downloads_dir()?;

    if !downloads.is_dir() {
        reporter.info(format!(
            "[*] Nothing to purge, {} does not exist",
            downloads.display()
        ));
        return Ok(());
    }

    reporter.info(format!("[+] Purging downloads in {}", downloads.display()));

    if downloads.is_symlink() {
        let target = fs::canonicalize(&downloads).unwrap_or_else(|_| downloads.clone());
        reporter.warn(format!(
            "[!] The downloads folder is a symlink, the archives are stored in {} and that is what gets purged",
            target.display()
        ));
    }

    remove_entries(directory_entries(&downloads), "Purge downloads", reporter)?;
    Ok(())
}

pub fn clear_temp_cache(reporter: &Reporter) -> Result<()> {
    let temp = std::env::temp_dir();
    reporter.info(format!(
        "[+] Clearing launcher staging directories in {}",
        temp.display()
    ));

    let entries = launcher_temp_directories();

    if entries.is_empty() {
        reporter.info("[*] No staging directory from a previous run is left behind");
        return Ok(());
    }

    remove_entries(entries, "Clear temp cache", reporter)?;
    Ok(())
}

pub fn prune_incomplete_downloads(args: &CommandArgs, reporter: &Reporter) -> Result<()> {
    let downloads = args.downloads_dir()?;
    reporter.info(format!(
        "[+] Pruning interrupted downloads in {}",
        downloads.display()
    ));

    if !downloads.is_dir() {
        reporter.info("[*] The downloads folder does not exist yet, nothing to prune");
        return Ok(());
    }

    let mut entries = incomplete_downloads(&downloads);

    for path in top_level_files(&downloads) {
        if has_suffix(&path, &ARCHIVE_SUFFIXES) && fsutil::entry_size(&path) == 0 {
            entries.push(path);
        }
    }

    if entries.is_empty() {
        reporter.info("[*] Every archive in the downloads folder is complete");
        return Ok(());
    }

    remove_entries(entries, "Prune incomplete downloads", reporter)?;
    Ok(())
}

pub fn kill_game_processes(
    reporter: &Reporter,
    adopted_processes: &SharedProcessRegistry,
) -> Result<()> {
    if !fsutil::tool_available("kill") {
        return Err(LauncherError::CommandFailed {
            command: "kill".to_string(),
            message: "kill is not installed or not in PATH, install util-linux".to_string(),
        });
    }

    match process::terminate_all(adopted_processes, reporter) {
        Ok(_terminated) => Ok(()),
        Err(error) => Err(LauncherError::Other(error.to_string())),
    }
}

pub fn reset_wine_prefix(config: &AppConfig, reporter: &Reporter) -> Result<()> {
    let prefix = config
        .runner
        .wine_prefix
        .as_deref()
        .map(expand_path)
        .ok_or_else(|| LauncherError::RunnerConfig("no wine prefix is configured".to_string()))?;

    if !prefix.exists() {
        reporter.info(format!(
            "[*] {} does not exist yet, the next launch will create it",
            prefix.display()
        ));
        return Ok(());
    }

    let (still_running, _) = process::scan_once();
    if !still_running.is_empty() {
        return Err(LauncherError::Other(
            "a game or Wine helper process is still running, terminate it before resetting the prefix"
                .to_string(),
        ));
    }

    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
    let name = prefix
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| "wineprefix".to_string());
    let backup = prefix.with_file_name(format!("{name}.bak-{stamp}"));

    reporter.info(format!(
        "[+] Moving {} aside to {}",
        prefix.display(),
        backup.display()
    ));

    fsutil::relax_tree_permissions(&prefix);
    fs::rename(&prefix, &backup)?;

    reporter.info("[+] The prefix will be recreated from scratch on the next launch");
    reporter.info(format!(
        "[*] Delete {} by hand once you are sure you do not need it anymore",
        backup.display()
    ));
    Ok(())
}

pub fn reset_graphics_state(
    args: &CommandArgs,
    config: &AppConfig,
    reporter: &Reporter,
) -> Result<()> {
    reporter.info("[+] Restoring the DirectX and DXVK environment to its defaults");
    reporter.info(format!(
        "[*] WINEDLLOVERRIDES is now: {}",
        default_dll_overrides()
    ));
    reporter.info(format!("[*] DXVK_CONFIG is now: {}", default_dxvk_config()));

    let mut entries: Vec<PathBuf> = Vec::new();

    if let Some(anomaly) = args.anomaly.as_deref() {
        for directory in [anomaly.to_path_buf(), anomaly.join("bin")] {
            entries.extend(
                top_level_files(&directory)
                    .into_iter()
                    .filter(|path| has_suffix(path, &GRAPHICS_CACHE_SUFFIXES)),
            );
        }
    }

    if let Some(prefix) = config.runner.wine_prefix.as_deref().map(expand_path) {
        for relative in PREFIX_GRAPHICS_CACHES {
            let candidate = prefix.join(relative);
            if candidate.exists() {
                entries.push(candidate);
            }
        }
    }

    if entries.is_empty() {
        reporter.info("[*] No stale shader or pipeline cache was found on disk");
        return Ok(());
    }

    remove_entries(entries, "Reset DXVK and D3D state", reporter)?;
    Ok(())
}

pub fn rebuild_mod_cache(args: &CommandArgs, reporter: &Reporter) -> Result<()> {
    let gamma = args.gamma_dir()?;
    let mods = args.mods_dir()?;
    let downloads = args.downloads_dir()?;
    let definition = modpack_data_dir(&gamma);

    reporter.info(format!(
        "[+] Re-indexing the mod installation in {}",
        mods.display()
    ));

    let entries = read_mod_maker(&definition, reporter)?;
    let expected: HashSet<String> = entries
        .iter()
        .map(|entry| entry.info().name.clone())
        .filter(|name| !name.is_empty())
        .collect();

    let mut installed = 0usize;
    let mut broken: Vec<String> = Vec::new();
    let mut orphaned: Vec<String> = Vec::new();
    let mut emptied: Vec<PathBuf> = Vec::new();

    for path in directory_entries(&mods) {
        reporter.checkpoint()?;

        if !path.is_dir() {
            continue;
        }

        let name = match path.file_name().and_then(|name| name.to_str()) {
            Some(name) => name.to_string(),
            None => continue,
        };

        if !fsutil::directory_has_entries(&path) {
            emptied.push(path);
            continue;
        }

        if path.join("meta.ini").is_file() {
            installed += 1;
        } else {
            broken.push(name.clone());
        }

        if !expected.contains(&name) {
            orphaned.push(name);
        }
    }

    if !emptied.is_empty() {
        remove_entries(emptied, "Empty mod folders", reporter)?;
    }

    let cached_archives = top_level_files(&downloads)
        .into_iter()
        .filter(|path| has_suffix(path, &ARCHIVE_SUFFIXES))
        .count();

    let missing: Vec<String> = expected
        .iter()
        .filter(|name| !mods.join(name).is_dir())
        .cloned()
        .collect();

    reporter.info(format!(
        "[+] Definition lists {} mod(s), {installed} are installed with metadata, {} archive(s) are cached",
        expected.len(),
        cached_archives
    ));

    for name in broken.iter().take(MISSING_PREVIEW_LIMIT) {
        reporter.warn(format!("  ! {name} has no meta.ini, Sync / Update will rebuild it"));
    }

    for name in orphaned.iter().take(MISSING_PREVIEW_LIMIT) {
        reporter.warn(format!(
            "  ! {name} is installed but is not part of the current definition"
        ));
    }

    for name in missing.iter().take(MISSING_PREVIEW_LIMIT) {
        reporter.warn(format!("  ! {name} is listed but not installed"));
    }

    if missing.len() > MISSING_PREVIEW_LIMIT {
        reporter.warn(format!(
            "  ! plus {} further mod(s) that are listed but not installed",
            missing.len() - MISSING_PREVIEW_LIMIT
        ));
    }

    reporter.info("[+] Mod index rebuilt, nothing that contained files was deleted");
    Ok(())
}

fn windows_path(path: &Path) -> String {
    linux_to_wine_path(path)
}

fn ini_value(value: String) -> String {
    escape_ini_backslashes(&value)
}

fn ini_path_target(section: &str, key: &str, anomaly: &Path, gamma: &Path) -> Option<PathBuf> {
    if section.eq_ignore_ascii_case("General") && key.eq_ignore_ascii_case("gamePath") {
        return Some(anomaly.to_path_buf());
    }

    if section.eq_ignore_ascii_case("Settings") && key.eq_ignore_ascii_case("base_directory") {
        return Some(gamma.to_path_buf());
    }

    None
}

fn repair_ini_text(original: &str, anomaly: &Path, gamma: &Path) -> (String, Vec<String>) {
    let mut output: Vec<String> = Vec::new();
    let mut repairs: Vec<String> = Vec::new();
    let mut section = String::new();

    for line in original.lines() {
        let trimmed = line.trim();

        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            section = trimmed[1..trimmed.len() - 1].to_string();
            output.push(line.to_string());
            continue;
        }

        let key = match ini_key(line) {
            Some(key) => key.to_string(),
            None => {
                output.push(line.to_string());
                continue;
            }
        };

        let value = match trimmed.split_once('=') {
            Some((_, value)) => value.trim().to_string(),
            None => {
                output.push(line.to_string());
                continue;
            }
        };

        let expected = match ini_path_target(&section, &key, anomaly, gamma) {
            Some(expected) => expected,
            None => {
                output.push(line.to_string());
                continue;
            }
        };

        if !wine_path_needs_repair(&value) {
            output.push(line.to_string());
            continue;
        }

        match repaired_wine_path(&value, &expected) {
            Some(fixed) => {
                repairs.push(format!("[{section}] {key}: {value} -> {fixed}"));
                output.push(format!("{key}={fixed}"));
            }
            None => output.push(line.to_string()),
        }
    }

    let mut text = output.join("\n");
    if !text.is_empty() {
        text.push('\n');
    }

    (text, repairs)
}

pub fn sanitize_mod_organizer_ini(args: &CommandArgs, reporter: &Reporter) -> Result<usize> {
    let gamma = args.gamma_dir()?;
    let anomaly = args.anomaly_dir()?;
    let target = gamma.join(INI_FILE_NAME);

    if !target.is_file() {
        return Ok(0);
    }

    let original = fs::read_to_string(&target)?;
    let (repaired, changes) = repair_ini_text(&original, &anomaly, &gamma);

    if changes.is_empty() {
        return Ok(0);
    }

    reporter.warn(format!(
        "[!] {} corrupted Wine path(s) detected in {}",
        changes.len(),
        target.display()
    ));

    let backup = gamma.join(INI_BACKUP_NAME);
    fs::copy(&target, &backup)?;
    reporter.info(format!("[*] Previous file saved as {}", backup.display()));

    fsutil::prepare_file_target(&target)?;
    fs::write(&target, repaired)?;

    for change in &changes {
        reporter.info(format!("  - repaired {change}"));
    }

    reporter.info(
        "[+] Every repaired path now uses forward slashes, which Qt QSettings never treats as escape sequences",
    );

    Ok(changes.len())
}

fn ini_key(line: &str) -> Option<&str> {
    let trimmed = line.trim();

    if trimmed.is_empty() || trimmed.starts_with(';') || trimmed.starts_with('#') {
        return None;
    }

    trimmed.split_once('=').map(|(key, _)| key.trim())
}

fn flush_pending(
    section: &str,
    desired: &[(&str, &str, String)],
    applied: &mut [bool],
    output: &mut Vec<String>,
) {
    if section.is_empty() {
        return;
    }

    let mut pending: Vec<String> = Vec::new();

    for (index, (wanted_section, key, value)) in desired.iter().enumerate() {
        if applied[index] || !wanted_section.eq_ignore_ascii_case(section) {
            continue;
        }
        applied[index] = true;
        pending.push(format!("{key}={value}"));
    }

    if pending.is_empty() {
        return;
    }

    while output
        .last()
        .map(|line| line.trim().is_empty())
        .unwrap_or(false)
    {
        output.pop();
    }

    output.extend(pending);
    output.push(String::new());
}

fn merge_ini(original: &str, desired: &[(&str, &str, String)]) -> String {
    let mut applied = vec![false; desired.len()];
    let mut output: Vec<String> = Vec::new();
    let mut section = String::new();

    for line in original.lines() {
        let trimmed = line.trim();

        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            flush_pending(&section, desired, &mut applied, &mut output);
            section = trimmed[1..trimmed.len() - 1].to_string();
            output.push(line.to_string());
            continue;
        }

        let key = match ini_key(line) {
            Some(key) => key,
            None => {
                output.push(line.to_string());
                continue;
            }
        };

        let position = desired.iter().position(|(wanted_section, wanted_key, _)| {
            wanted_section.eq_ignore_ascii_case(&section) && wanted_key.eq_ignore_ascii_case(key)
        });

        match position {
            Some(index) if !applied[index] => {
                applied[index] = true;
                output.push(format!("{}={}", desired[index].1, desired[index].2));
            }
            Some(_) => {}
            None => output.push(line.to_string()),
        }
    }

    flush_pending(&section, desired, &mut applied, &mut output);

    for index in 0..desired.len() {
        if applied[index] {
            continue;
        }

        let missing_section = desired[index].0;

        if !output.is_empty() {
            output.push(String::new());
        }

        output.push(format!("[{missing_section}]"));

        for inner in index..desired.len() {
            if applied[inner] || desired[inner].0 != missing_section {
                continue;
            }
            applied[inner] = true;
            output.push(format!("{}={}", desired[inner].1, desired[inner].2));
        }

        output.push(String::new());
    }

    while output
        .last()
        .map(|line| line.trim().is_empty())
        .unwrap_or(false)
    {
        output.pop();
    }

    let mut text = output.join("\n");
    text.push('\n');
    text
}

pub fn sync_mod_organizer_ini(
    args: &CommandArgs,
    config: &AppConfig,
    reporter: &Reporter,
) -> Result<()> {
    let gamma = args.gamma_dir()?;
    let anomaly = args.anomaly_dir()?;
    let target = gamma.join(INI_FILE_NAME);

    reporter.info(format!("[+] Synchronising {}", target.display()));

    let desired = vec![
        ("General", "gameName", ini_value("Stalker Anomaly".to_string())),
        (
            "General",
            "gamePath",
            ini_value(format!("@ByteArray({})", windows_path(&anomaly))),
        ),
        (
            "General",
            "selected_profile",
            ini_value(format!("@ByteArray({MODPACK_DIR_NAME})")),
        ),
        ("General", "first_start", "false".to_string()),
        (
            "General",
            "version",
            config.mo_version.trim_start_matches('v').to_string(),
        ),
        ("Settings", "base_directory", ini_value(windows_path(&gamma))),
        (
            "Settings",
            "download_directory",
            ini_value(format!("%BASE_DIR%/{DOWNLOADS_DIR_NAME}")),
        ),
        (
            "Settings",
            "mod_directory",
            ini_value(format!("%BASE_DIR%/{MODS_DIR_NAME}")),
        ),
        (
            "Settings",
            "profiles_directory",
            ini_value("%BASE_DIR%/profiles".to_string()),
        ),
        (
            "Settings",
            "overwrite_directory",
            ini_value("%BASE_DIR%/overwrite".to_string()),
        ),
    ];

    let original = match fs::read_to_string(&target) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            reporter.info("[*] No ModOrganizer.ini exists yet, writing a fresh one");
            String::new()
        }
        Err(error) => return Err(error.into()),
    };

    if !original.is_empty() {
        let backup = gamma.join(INI_BACKUP_NAME);
        fs::copy(&target, &backup)?;
        reporter.info(format!("[*] Previous file saved as {}", backup.display()));
    }

    fsutil::prepare_file_target(&target)?;
    fs::write(&target, merge_ini(&original, &desired))?;

    for (section, key, value) in &desired {
        reporter.info(format!("  - [{section}] {key} = {value}"));
    }

    reporter.info("[+] Mod Organizer 2 now points at the configured Anomaly and GAMMA folders");
    Ok(())
}

fn preset_sources(definition: &Path) -> Vec<PathBuf> {
    let mut sources: Vec<PathBuf> = Vec::new();

    for path in top_level_files(&definition.join("presets")) {
        if has_suffix(&path, &[".txt"]) {
            sources.push(path);
        }
    }

    for path in top_level_files(definition) {
        let name = match path.file_name().and_then(|name| name.to_str()) {
            Some(name) => name.to_lowercase(),
            None => continue,
        };

        if name.ends_with(".txt") && name.contains("modlist") && name != "modlist.txt" {
            sources.push(path);
        }
    }

    sources
}

pub fn reindex_presets(args: &CommandArgs, reporter: &Reporter) -> Result<()> {
    let gamma = args.gamma_dir()?;
    let definition = modpack_data_dir(&gamma);

    reporter.info(format!(
        "[+] Re-indexing the nominated presets shipped in {}",
        definition.display()
    ));

    if !definition.is_dir() {
        return Err(LauncherError::Other(format!(
            "{} does not exist, run GAMMA setup or Sync / Update first",
            definition.display()
        )));
    }

    FullInstall::install_modorganizer_profile(&gamma, reporter)?;

    let mut deployed = 1usize;

    for source in preset_sources(&definition) {
        let stem = match source.file_stem().and_then(|stem| stem.to_str()) {
            Some(stem) if stem.to_lowercase() != "modlist" => stem.to_string(),
            _ => continue,
        };

        let profile = gamma.join("profiles").join(&stem);
        let settings = profile.join("settings.txt");

        fsutil::ensure_directory(&profile)?;
        fsutil::prepare_file_target(&profile.join("modlist.txt"))?;
        fs::copy(&source, profile.join("modlist.txt"))?;

        if !settings.is_file() {
            fs::write(&settings, PROFILE_SETTINGS)?;
        }

        reporter.info(format!("  - Deployed preset {stem} to {}", profile.display()));
        deployed += 1;
    }

    reporter.info(format!(
        "[+] {deployed} profile(s) are now available in the Mod Organizer 2 profile selector"
    ));
    Ok(())
}

#[derive(Debug, Clone, Copy, Default)]
struct PermissionReport {
    directories: usize,
    files: usize,
    changed: usize,
    rejected: usize,
    skipped: usize,
}

fn normalize_permissions(root: &Path, report: &mut PermissionReport, reporter: &Reporter) -> Result<()> {
    for entry in WalkDir::new(root).into_iter().filter_map(|entry| entry.ok()) {
        reporter.checkpoint()?;

        let file_type = entry.file_type();

        if file_type.is_symlink() {
            report.skipped += 1;
            continue;
        }

        let mode = if file_type.is_dir() {
            report.directories += 1;
            fsutil::DIRECTORY_MODE
        } else {
            report.files += 1;
            fsutil::FILE_MODE
        };

        match fsutil::force_permissions(entry.path(), mode) {
            PermissionOutcome::Changed => report.changed += 1,
            PermissionOutcome::Rejected => report.rejected += 1,
            PermissionOutcome::Unchanged => {}
        }
    }

    Ok(())
}

pub fn fix_permissions(args: &CommandArgs, reporter: &Reporter) -> Result<()> {
    let mods = args.mods_dir()?;
    let downloads = args.downloads_dir()?;

    let mut roots: Vec<PathBuf> = Vec::new();

    for root in [mods, downloads] {
        if !root.exists() {
            reporter.warn(format!("[!] Skipping {}, it does not exist", root.display()));
            continue;
        }

        let resolved = if root.is_symlink() {
            let target = fs::canonicalize(&root).unwrap_or_else(|_| root.clone());
            reporter.info(format!(
                "[*] {} is a symlink, following it to {}",
                root.display(),
                target.display()
            ));
            target
        } else {
            root
        };

        roots.push(resolved);
    }

    if roots.is_empty() {
        return Err(LauncherError::Other(
            "neither the mods nor the downloads folder exists yet".to_string(),
        ));
    }

    let mut report = PermissionReport::default();

    for root in &roots {
        reporter.info(format!(
            "[+] Restoring {:o} on directories and {:o} on files under {}",
            fsutil::DIRECTORY_MODE,
            fsutil::FILE_MODE,
            root.display()
        ));
        normalize_permissions(root, &mut report, reporter)?;
    }

    reporter.info(format!(
        "[+] Visited {} directory/directories and {} file(s): {} updated, {} symlink(s) left alone",
        report.directories, report.files, report.changed, report.skipped
    ));

    if report.rejected > 0 {
        reporter.warn(format!(
            "[!] {} entry/entries refused the change, which is expected on NTFS, exFAT or FAT mounts that do not carry POSIX modes",
            report.rejected
        ));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn existing_keys_are_updated_in_place() {
        let original = "[General]\ngameName=Stalker Anomaly\ngamePath=@ByteArray(Z:\\old)\n";
        let desired = vec![(
            "General",
            "gamePath",
            "@ByteArray(Z:\\new)".to_string(),
        )];

        let merged = merge_ini(original, &desired);

        assert!(merged.contains("gamePath=@ByteArray(Z:\\new)"));
        assert!(!merged.contains("Z:\\old"));
        assert!(merged.contains("gameName=Stalker Anomaly"));
    }

    #[test]
    fn missing_sections_are_appended_once() {
        let original = "[General]\ngameName=Stalker Anomaly\n";
        let desired = vec![
            ("Settings", "base_directory", "Z:\\gamma".to_string()),
            ("Settings", "mod_directory", "%BASE_DIR%/mods".to_string()),
        ];

        let merged = merge_ini(original, &desired);

        assert_eq!(merged.matches("[Settings]").count(), 1);
        assert!(merged.contains("base_directory=Z:\\gamma"));
        assert!(merged.contains("mod_directory=%BASE_DIR%/mods"));
    }

    #[test]
    fn unrelated_keys_and_comments_survive() {
        let original = "; keep me\n[General]\nstyle=dark\n\n[Settings]\nnexus_api_key=secret\n";
        let desired = vec![("Settings", "mod_directory", "%BASE_DIR%/mods".to_string())];

        let merged = merge_ini(original, &desired);

        assert!(merged.contains("; keep me"));
        assert!(merged.contains("style=dark"));
        assert!(merged.contains("nexus_api_key=secret"));
    }

    #[test]
    fn duplicate_keys_collapse_into_one() {
        let original = "[Settings]\nmod_directory=old\nmod_directory=older\n";
        let desired = vec![("Settings", "mod_directory", "new".to_string())];

        let merged = merge_ini(original, &desired);

        assert_eq!(merged.matches("mod_directory=").count(), 1);
        assert!(merged.contains("mod_directory=new"));
    }

    #[test]
    fn unix_paths_become_forward_slash_wine_paths() {
        assert_eq!(
            windows_path(Path::new("/mnt/yo/Games/GAMMA")),
            "Z:/mnt/yo/Games/GAMMA"
        );
    }

    #[test]
    fn corrupted_base_directories_are_repaired() {
        let original = "[General]\ngamePath=@ByteArray(Z:\\mnt\\yo\\Anomaly)\n\n[Settings]\nbase_directory=Z:ntoamesTALKER_Gamma\nnexus_api_key=secret\n";
        let (repaired, changes) = repair_ini_text(
            original,
            Path::new("/mnt/yo/Anomaly"),
            Path::new("/mnt/yo/Games/STALKER_Gamma/S.T.A.L.K.E.R. - Gamma"),
        );

        assert_eq!(changes.len(), 2);
        assert!(repaired.contains("gamePath=@ByteArray(Z:/mnt/yo/Anomaly)"));
        assert!(repaired.contains(
            "base_directory=Z:/mnt/yo/Games/STALKER_Gamma/S.T.A.L.K.E.R. - Gamma"
        ));
        assert!(repaired.contains("nexus_api_key=secret"));
    }

    #[test]
    fn healthy_files_are_left_alone() {
        let original = "[Settings]\nbase_directory=Z:/mnt/yo/Games\nmod_directory=%BASE_DIR%/mods\n";
        let (_, changes) = repair_ini_text(original, Path::new("/a"), Path::new("/mnt/yo/Games"));

        assert!(changes.is_empty());
    }

    #[test]
    fn interrupted_downloads_are_recognised_by_suffix() {
        assert!(has_suffix(Path::new("/a/mod.zip.part"), &INCOMPLETE_SUFFIXES));
        assert!(has_suffix(Path::new("/a/mod.CRDOWNLOAD"), &INCOMPLETE_SUFFIXES));
        assert!(!has_suffix(Path::new("/a/mod.zip"), &INCOMPLETE_SUFFIXES));
        assert!(has_suffix(Path::new("/a/mod.7Z"), &ARCHIVE_SUFFIXES));
    }

    #[test]
    fn only_launcher_staging_directories_are_matched() {
        assert!(has_temp_prefix("gamma-launcher-modinstall-abcd"));
        assert!(!has_temp_prefix("systemd-private-abcd"));
    }
}
