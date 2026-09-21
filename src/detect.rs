use crate::config::{expand_path, AppConfig};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use walkdir::{DirEntry, WalkDir};

const SCAN_MAX_DEPTH: usize = 6;
const MAX_CANDIDATES: usize = 64;
const MAX_RUNNER_UPS: usize = 3;
const PREFERRED_PROTON: &str = "/usr/share/steam/compatibilitytools.d/proton-cachyos-slr";

const SYSTEM_COMPAT_TOOLS: [&str; 1] = ["/usr/share/steam/compatibilitytools.d"];
const USER_COMPAT_TOOLS: [&str; 3] = [
    "$HOME/.steam/root/compatibilitytools.d",
    "$HOME/.steam/steam/compatibilitytools.d",
    "$HOME/.local/share/Steam/compatibilitytools.d",
];

const PRUNED_DIRECTORIES: [&str; 13] = [
    ".git",
    ".svn",
    ".cache",
    "__pycache__",
    "node_modules",
    "proc",
    "sys",
    "dev",
    "lost+found",
    "shadercache",
    "compatdata",
    "trash",
    ".trash",
];

const SCORE_GAMMA_DOTTED: i32 = 400;
const SCORE_GAMMA_PREFIXED: i32 = 350;
const SCORE_GAMMA_PLAIN: i32 = 300;
const SCORE_GAMMA_COMPONENT: i32 = 60;
const SCORE_GAMMA_NEIGHBOUR: i32 = 70;
const SCORE_MODPACK_MARKER: i32 = 90;
const SCORE_INSTANCE_LAYOUT: i32 = 60;
const SCORE_DOWNLOAD_CACHE: i32 = 20;
const SCORE_ANOMALY_LAYOUT: i32 = 40;
const SCORE_ANOMALY_KEYWORD: i32 = 30;
const SCORE_CONFIGURED_ROOT: i32 = 45;
const SCORE_SHARED_PARENT: i32 = 45;
const PENALTY_ARCHIVED: i32 = -120;
const PENALTY_THIRD_PARTY: i32 = -40;
const PENALTY_PER_COMPONENT: i32 = -2;

const ARCHIVED_MARKERS: [&str; 7] = [
    "backup", "_old", " old", "copy", "recycle", ".bak", "archive",
];
const THIRD_PARTY_MARKERS: [&str; 4] = ["program files", "downloads/", "temp/", "tmp/"];

#[derive(Debug, Clone, Default)]
pub struct DetectionResult {
    pub mo2_executable: Option<PathBuf>,
    pub launcher_executable: Option<PathBuf>,
    pub game_executable: Option<PathBuf>,
    pub gamma_path: Option<PathBuf>,
    pub anomaly_path: Option<PathBuf>,
    pub proton_path: Option<PathBuf>,
    pub messages: Vec<String>,
}

#[derive(Debug, Clone)]
struct Candidate {
    path: PathBuf,
    score: i32,
    reasons: Vec<String>,
}

impl Candidate {
    fn new(path: PathBuf) -> Self {
        Self {
            path,
            score: 0,
            reasons: Vec::new(),
        }
    }

    fn award(&mut self, points: i32, reason: impl Into<String>) {
        if points == 0 {
            return;
        }
        self.score += points;
        self.reasons.push(format!("{} ({points:+})", reason.into()));
    }

    fn summary(&self) -> String {
        if self.reasons.is_empty() {
            return "no distinguishing signal".to_string();
        }
        self.reasons.join(", ")
    }

    fn depth(&self) -> usize {
        self.path.components().count()
    }
}

fn lowercase(path: &Path) -> String {
    path.to_string_lossy().to_lowercase().replace('\\', "/")
}

fn is_named(path: &Path, name: &str) -> bool {
    path.file_name()
        .and_then(|value| value.to_str())
        .map(|value| value.eq_ignore_ascii_case(name))
        .unwrap_or(false)
}

fn is_dir_named(path: &Path, name: &str) -> bool {
    path.file_name()
        .and_then(|value| value.to_str())
        .map(|value| value.eq_ignore_ascii_case(name))
        .unwrap_or(false)
}

fn is_anomaly_binary(path: &Path) -> bool {
    let in_bin = path
        .parent()
        .map(|parent| is_dir_named(parent, "bin"))
        .unwrap_or(false);

    if !in_bin {
        return false;
    }

    let name = match path.file_name().and_then(|value| value.to_str()) {
        Some(name) => name.to_lowercase(),
        None => return false,
    };

    name.starts_with("anomalydx") && name.ends_with(".exe")
}

fn renderer_rank(path: &Path) -> (i32, &'static str) {
    let name = path
        .file_name()
        .map(|value| value.to_string_lossy().to_lowercase())
        .unwrap_or_default();

    if name.starts_with("anomalydx11avx") {
        return (14, "DirectX 11 AVX renderer, the fastest GAMMA target");
    }
    if name.starts_with("anomalydx11") {
        return (12, "DirectX 11 renderer, the renderer GAMMA expects");
    }
    if name.starts_with("anomalydx10") {
        return (6, "DirectX 10 renderer");
    }
    (3, "legacy DirectX 9 renderer")
}

fn gamma_marker(haystack: &str) -> Option<(i32, &'static str)> {
    if haystack.contains("g.a.m.m.a") {
        return Some((
            SCORE_GAMMA_DOTTED,
            "detected the preferred G.A.M.M.A. variant",
        ));
    }
    if haystack.contains("stalker_gamma") || haystack.contains("stalker-gamma") {
        return Some((
            SCORE_GAMMA_PREFIXED,
            "detected a preferred STALKER_GAMMA variant",
        ));
    }
    if haystack.contains("gamma") {
        return Some((SCORE_GAMMA_PLAIN, "detected a preferred GAMMA variant"));
    }
    None
}

fn has_gamma_component(path: &Path) -> bool {
    path.components().any(|component| {
        component
            .as_os_str()
            .to_str()
            .map(|value| {
                let value = value.to_lowercase();
                value == "gamma" || value == "g.a.m.m.a" || value == "g.a.m.m.a."
            })
            .unwrap_or(false)
    })
}

fn neighbourhood_has_gamma(root: &Path) -> bool {
    let parent = match root.parent() {
        Some(parent) => parent,
        None => return false,
    };

    let entries = match std::fs::read_dir(parent) {
        Ok(entries) => entries,
        Err(_) => return false,
    };

    entries.flatten().any(|entry| {
        entry.path().is_dir()
            && entry
                .file_name()
                .to_str()
                .map(|name| gamma_marker(&name.to_lowercase()).is_some())
                .unwrap_or(false)
    })
}

fn sibling_exists(path: &Path, name: &str) -> bool {
    path.parent()
        .map(|parent| parent.join(name).exists())
        .unwrap_or(false)
}

fn anomaly_root_of(executable: &Path) -> Option<PathBuf> {
    let parent = executable.parent()?;
    if is_dir_named(parent, "bin") {
        return parent.parent().map(|root| root.to_path_buf());
    }
    Some(parent.to_path_buf())
}

fn looks_like_anomaly_root(root: &Path) -> bool {
    root.join("bin").is_dir()
        && (root.join("gamedata").is_dir()
            || root.join("db").is_dir()
            || root.join("appdata").is_dir())
}

fn expanded_common_roots() -> Vec<PathBuf> {
    let raw_roots = [
        "$HOME",
        "$HOME/Games",
        "$HOME/games",
        "$HOME/GAMMA",
        "$HOME/Gamma",
        "$HOME/Desktop",
        "$HOME/.local/share",
        "$HOME/.steam/steam/steamapps/common",
        "$HOME/.local/share/Steam/steamapps/common",
        "/mnt",
        "/media",
        "/run/media",
    ];

    raw_roots
        .iter()
        .map(|root| expand_path(Path::new(root)))
        .filter(|root| root.is_dir())
        .collect()
}

fn configured_roots(config: &AppConfig) -> Vec<PathBuf> {
    let mut roots = Vec::new();

    for configured in [config.gamma_path.as_deref(), config.anomaly_path.as_deref()]
        .into_iter()
        .flatten()
    {
        let expanded = expand_path(configured);
        if expanded.is_dir() {
            roots.push(expanded.clone());
        }
        if let Some(parent) = expanded.parent() {
            if parent.is_dir() {
                roots.push(parent.to_path_buf());
            }
        }
    }

    roots
}

fn drop_nested(roots: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut kept: Vec<PathBuf> = Vec::new();

    for root in roots {
        if kept.iter().any(|existing| root.starts_with(existing)) {
            continue;
        }
        kept.retain(|existing| !existing.starts_with(&root));
        kept.push(root);
    }

    kept
}

fn scan_roots(config: &AppConfig) -> Vec<PathBuf> {
    let mut roots = configured_roots(config);
    roots.extend(expanded_common_roots());
    roots.retain(|root| root.is_dir());
    drop_nested(roots)
}

fn is_pruned(entry: &DirEntry) -> bool {
    if !entry.file_type().is_dir() {
        return false;
    }

    let name = entry.file_name().to_string_lossy().to_lowercase();
    PRUNED_DIRECTORIES.contains(&name.as_str())
}

fn gather<F>(roots: &[PathBuf], matcher: F) -> Vec<PathBuf>
where
    F: Fn(&Path) -> bool,
{
    let mut seen: HashSet<PathBuf> = HashSet::new();
    let mut found: Vec<PathBuf> = Vec::new();

    for root in roots {
        let walker = WalkDir::new(root)
            .max_depth(SCAN_MAX_DEPTH)
            .follow_links(false)
            .into_iter()
            .filter_entry(|entry| entry.depth() == 0 || !is_pruned(entry));

        for entry in walker.filter_map(|entry| entry.ok()) {
            if !entry.file_type().is_file() || !matcher(entry.path()) {
                continue;
            }

            let path = entry.path().to_path_buf();
            let key = path.canonicalize().unwrap_or_else(|_| path.clone());
            if seen.insert(key) {
                found.push(path);
            }

            if found.len() >= MAX_CANDIDATES {
                return found;
            }
        }
    }

    found
}

fn score_common(candidate: &mut Candidate, configured: &[PathBuf]) {
    let haystack = lowercase(&candidate.path);

    if let Some((points, reason)) = gamma_marker(&haystack) {
        candidate.award(points, reason);
    }

    if has_gamma_component(&candidate.path) {
        candidate.award(
            SCORE_GAMMA_COMPONENT,
            "a whole directory is named after GAMMA",
        );
    }

    if ARCHIVED_MARKERS
        .iter()
        .any(|marker| haystack.contains(marker))
    {
        candidate.award(PENALTY_ARCHIVED, "looks like a backup or archived copy");
    }

    if THIRD_PARTY_MARKERS
        .iter()
        .any(|marker| haystack.contains(marker))
    {
        candidate.award(
            PENALTY_THIRD_PARTY,
            "lives in a generic or temporary location",
        );
    }

    if configured
        .iter()
        .any(|root| candidate.path.starts_with(root))
    {
        candidate.award(
            SCORE_CONFIGURED_ROOT,
            "sits inside a directory you already configured",
        );
    }

    let depth_penalty = PENALTY_PER_COMPONENT * candidate.depth() as i32;
    candidate.score += depth_penalty;
}

fn score_mo2(path: PathBuf, configured: &[PathBuf]) -> Candidate {
    let mut candidate = Candidate::new(path);
    score_common(&mut candidate, configured);

    if sibling_exists(&candidate.path, ".Grok's Modpack Installer") {
        candidate.award(
            SCORE_MODPACK_MARKER,
            "the GAMMA modpack installer folder sits next to it",
        );
    }

    if sibling_exists(&candidate.path, "mods") && sibling_exists(&candidate.path, "profiles") {
        candidate.award(
            SCORE_INSTANCE_LAYOUT,
            "a complete Mod Organizer 2 instance layout",
        );
    }

    if sibling_exists(&candidate.path, "downloads") {
        candidate.award(SCORE_DOWNLOAD_CACHE, "a populated download cache is present");
    }

    candidate
}

fn score_anomaly(path: PathBuf, configured: &[PathBuf], affinity: Option<&Path>) -> Candidate {
    let mut candidate = Candidate::new(path);
    score_common(&mut candidate, configured);

    if let Some(root) = anomaly_root_of(&candidate.path) {
        if looks_like_anomaly_root(&root) {
            candidate.award(
                SCORE_ANOMALY_LAYOUT,
                "the folder carries a real Anomaly layout",
            );
        }
        if neighbourhood_has_gamma(&root) {
            candidate.award(
                SCORE_GAMMA_NEIGHBOUR,
                "a GAMMA installation sits in the same folder",
            );
        }
    }

    if lowercase(&candidate.path).contains("anomaly") {
        candidate.award(SCORE_ANOMALY_KEYWORD, "path contains the Anomaly keyword");
    }

    if let Some(sibling) = affinity.and_then(|path| path.parent()) {
        if candidate.path.starts_with(sibling) {
            candidate.award(
                SCORE_SHARED_PARENT,
                "shares a parent folder with the selected GAMMA installation",
            );
        }
    }

    candidate
}

fn select(
    mut candidates: Vec<Candidate>,
    target: &str,
    messages: &mut Vec<String>,
) -> Option<PathBuf> {
    if candidates.is_empty() {
        messages.push(format!("[!] No candidate was found for {target}"));
        return None;
    }

    candidates.sort_by(|left, right| {
        right
            .score
            .cmp(&left.score)
            .then_with(|| left.depth().cmp(&right.depth()))
            .then_with(|| left.path.cmp(&right.path))
    });

    let best = candidates.remove(0);
    messages.push(format!(
        "[+] {target}: selected {} with score {} because {}",
        best.path.display(),
        best.score,
        best.summary()
    ));

    for runner_up in candidates.iter().take(MAX_RUNNER_UPS) {
        messages.push(format!(
            "    rejected {} with score {} ({})",
            runner_up.path.display(),
            runner_up.score,
            runner_up.summary()
        ));
    }

    if candidates.len() > MAX_RUNNER_UPS {
        messages.push(format!(
            "    plus {} further candidate(s) with a lower score",
            candidates.len() - MAX_RUNNER_UPS
        ));
    }

    Some(best.path)
}

fn compat_tool_dirs() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = SYSTEM_COMPAT_TOOLS
        .iter()
        .map(|raw| PathBuf::from(*raw))
        .chain(
            USER_COMPAT_TOOLS
                .iter()
                .map(|raw| expand_path(Path::new(raw))),
        )
        .filter(|dir| dir.is_dir())
        .collect();

    dirs.dedup();
    dirs
}

fn proton_builds(messages: &mut Vec<String>) -> Vec<PathBuf> {
    let mut builds = Vec::new();

    for base in compat_tool_dirs() {
        match std::fs::read_dir(&base) {
            Ok(entries) => {
                let mut found: Vec<PathBuf> = entries
                    .flatten()
                    .map(|entry| entry.path())
                    .filter(|path| path.is_dir())
                    .collect();
                found.sort();
                builds.extend(found);
            }
            Err(error) => messages.push(format!("[!] Could not read {}: {error}", base.display())),
        }
    }

    builds
}

fn build_named(builds: &[PathBuf], needle: &str) -> Option<PathBuf> {
    builds
        .iter()
        .find(|path| {
            path.file_name()
                .map(|name| name.to_string_lossy().to_lowercase().contains(needle))
                .unwrap_or(false)
        })
        .cloned()
}

pub fn detect_proton(messages: &mut Vec<String>) -> Option<PathBuf> {
    let preferred = PathBuf::from(PREFERRED_PROTON);
    if preferred.is_dir() {
        messages.push(format!(
            "[+] Proton: selected the preferred build at {}",
            preferred.display()
        ));
        return Some(preferred);
    }

    let builds = proton_builds(messages);
    if builds.is_empty() {
        messages.push("[!] No Proton build was found in any compatibility tools folder".to_string());
        return None;
    }

    for (needle, reason) in [
        ("cachyos", "an alternate CachyOS Proton build"),
        ("ge-proton", "a GE-Proton build"),
        ("proton", "a generic Proton build"),
    ] {
        if let Some(found) = build_named(&builds, needle) {
            messages.push(format!(
                "[+] Proton: selected {reason} at {}",
                found.display()
            ));
            return Some(found);
        }
    }

    let fallback = builds[0].clone();
    messages.push(format!(
        "[+] Proton: falling back to the first compatibility tool at {}",
        fallback.display()
    ));
    Some(fallback)
}

pub fn autodetect(config: &AppConfig) -> DetectionResult {
    let mut result = DetectionResult::default();
    let roots = scan_roots(config);
    let configured = configured_roots(config);

    if roots.is_empty() {
        result
            .messages
            .push("[!] No candidate directory was available to scan".to_string());
        result.proton_path = detect_proton(&mut result.messages);
        return result;
    }

    result.messages.push(format!(
        "[*] Scanning {} root(s) up to {SCAN_MAX_DEPTH} levels deep",
        roots.len()
    ));

    let mo2_candidates: Vec<Candidate> = gather(&roots, |path| is_named(path, "ModOrganizer.exe"))
        .into_iter()
        .map(|path| score_mo2(path, &configured))
        .collect();
    result.mo2_executable = select(mo2_candidates, "Mod Organizer 2", &mut result.messages);
    result.gamma_path = result
        .mo2_executable
        .as_ref()
        .and_then(|path| path.parent())
        .map(|parent| parent.to_path_buf());

    let affinity = result.gamma_path.clone();

    let launcher_candidates: Vec<Candidate> =
        gather(&roots, |path| is_named(path, "AnomalyLauncher.exe"))
            .into_iter()
            .map(|path| score_anomaly(path, &configured, affinity.as_deref()))
            .collect();
    result.launcher_executable = select(
        launcher_candidates,
        "the Anomaly launcher",
        &mut result.messages,
    );
    result.anomaly_path = result
        .launcher_executable
        .as_deref()
        .and_then(anomaly_root_of);

    let game_candidates: Vec<Candidate> = gather(&roots, is_anomaly_binary)
        .into_iter()
        .map(|path| {
            let mut candidate = score_anomaly(path, &configured, affinity.as_deref());
            let (points, reason) = renderer_rank(&candidate.path);
            candidate.award(points, reason);
            candidate
        })
        .collect();
    result.game_executable = select(game_candidates, "the game binary", &mut result.messages);

    if result.anomaly_path.is_none() {
        result.anomaly_path = result.game_executable.as_deref().and_then(anomaly_root_of);
    }

    result.proton_path = detect_proton(&mut result.messages);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gamma_variants_outrank_every_other_signal() {
        let configured = Vec::new();
        let gamma = score_mo2(
            PathBuf::from("/Games/STALKER_gamma/ModOrganizer.exe"),
            &configured,
        );
        let other = score_mo2(
            PathBuf::from("/Games/STALKER_soufied/ModOrganizer.exe"),
            &configured,
        );

        assert!(gamma.score > other.score);
    }

    #[test]
    fn dotted_variants_outrank_plain_ones() {
        let configured = Vec::new();
        let dotted = score_mo2(PathBuf::from("/Games/G.A.M.M.A./ModOrganizer.exe"), &configured);
        let plain = score_mo2(PathBuf::from("/Games/gamma_pack/ModOrganizer.exe"), &configured);

        assert!(dotted.score > plain.score);
    }

    #[test]
    fn backups_lose_against_live_installations() {
        let configured = Vec::new();
        let live = score_mo2(PathBuf::from("/Games/GAMMA/ModOrganizer.exe"), &configured);
        let backup = score_mo2(
            PathBuf::from("/Games/GAMMA_backup/ModOrganizer.exe"),
            &configured,
        );

        assert!(live.score > backup.score);
    }

    #[test]
    fn renderers_are_ranked_from_fastest_to_slowest() {
        let avx = renderer_rank(Path::new("/a/bin/AnomalyDX11AVX.exe")).0;
        let dx11 = renderer_rank(Path::new("/a/bin/AnomalyDX11.exe")).0;
        let dx10 = renderer_rank(Path::new("/a/bin/AnomalyDX10AVX.exe")).0;
        let dx9 = renderer_rank(Path::new("/a/bin/AnomalyDX9AVX.exe")).0;

        assert!(avx > dx11 && dx11 > dx10 && dx10 > dx9);
    }

    #[test]
    fn only_binaries_inside_a_bin_folder_are_accepted() {
        assert!(is_anomaly_binary(Path::new("/a/bin/AnomalyDX11AVX.exe")));
        assert!(!is_anomaly_binary(Path::new("/a/AnomalyDX11AVX.exe")));
        assert!(!is_anomaly_binary(Path::new("/a/bin/notes.txt")));
    }

    #[test]
    fn nested_roots_are_scanned_only_once() {
        let roots = drop_nested(vec![
            PathBuf::from("/home/user/Games/GAMMA"),
            PathBuf::from("/home/user"),
            PathBuf::from("/mnt/games"),
        ]);

        assert_eq!(
            roots,
            vec![PathBuf::from("/home/user"), PathBuf::from("/mnt/games")]
        );
    }

    #[test]
    fn deeply_nested_copies_lose_to_shallow_installations() {
        let configured = Vec::new();
        let shallow = score_mo2(PathBuf::from("/Games/GAMMA/ModOrganizer.exe"), &configured);
        let deep = score_mo2(
            PathBuf::from("/Games/GAMMA/nested/copy2/ModOrganizer.exe"),
            &configured,
        );

        assert!(shallow.score > deep.score);
    }
}
