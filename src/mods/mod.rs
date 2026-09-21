pub mod downloader;
pub mod info;
pub mod installer;

use crate::config::{GROK_INSTALLER_DIR_NAME, MODPACK_DIR_NAME};
use crate::error::Result;
use crate::fsutil::read_text;
use crate::mods::downloader::Downloader;
use crate::mods::info::ModInfo;
use crate::mods::installer::base::BaseInstaller;
use crate::mods::installer::default::DefaultInstaller;
use crate::mods::installer::git::GitResourceInstaller;
use crate::mods::installer::separator::SeparatorInstaller;
use crate::report::Reporter;
use std::path::{Path, PathBuf, MAIN_SEPARATOR, MAIN_SEPARATOR_STR};

#[derive(Debug, Clone)]
pub enum ModEntry {
    Separator(SeparatorInstaller),
    Default(DefaultInstaller),
}

impl ModEntry {
    pub fn info(&self) -> &ModInfo {
        match self {
            ModEntry::Separator(installer) => installer.info(),
            ModEntry::Default(installer) => installer.info(),
        }
    }

    pub fn downloader(&self) -> Option<&Downloader> {
        match self {
            ModEntry::Separator(installer) => installer.base().downloader(),
            ModEntry::Default(installer) => installer.base().downloader(),
        }
    }

    pub fn into_downloader(self) -> Option<Downloader> {
        match self {
            ModEntry::Separator(installer) => installer.into_base().into_downloader(),
            ModEntry::Default(installer) => installer.into_base().into_downloader(),
        }
    }

    pub async fn download(&mut self, to: &Path, use_cached: bool, reporter: &Reporter) -> Result<()> {
        match self {
            ModEntry::Separator(_) => Ok(()),
            ModEntry::Default(installer) => installer
                .base_mut()
                .download(to, use_cached, reporter)
                .await
                .map(|_| ()),
        }
    }

    pub fn install(&self, to: &Path, reporter: &Reporter) -> Result<()> {
        match self {
            ModEntry::Separator(installer) => installer.install(to, reporter),
            ModEntry::Default(installer) => installer.install(to, reporter),
        }
    }
}

pub fn base_archive(url: impl Into<String>) -> BaseInstaller {
    BaseInstaller::new(ModInfo::from_url(url))
}

pub fn moddb_archive(
    name: impl Into<String>,
    url: impl Into<String>,
    iurl: impl Into<String>,
) -> BaseInstaller {
    BaseInstaller::new(ModInfo::moddb(name, url, iurl))
}

pub fn git_resource(url: impl Into<String>, gamedata: bool) -> GitResourceInstaller {
    GitResourceInstaller::new(ModInfo::from_url(url), gamedata)
}

fn parse_modpack_maker_line(line: &str) -> Option<ModInfo> {
    let fields: Vec<&str> = line.split('\t').collect();
    if fields.len() < 4 {
        return None;
    }

    let args = if fields.len() > 5 {
        Some(
            fields[5..]
                .iter()
                .flat_map(|field| field.split(' '))
                .map(str::to_string)
                .collect::<Vec<String>>(),
        )
    } else {
        None
    };

    let subdirs = if fields[1] != "0" {
        Some(
            fields[1]
                .split(':')
                .map(|subdir| {
                    subdir
                        .replace('\\', MAIN_SEPARATOR_STR)
                        .trim_start_matches(MAIN_SEPARATOR)
                        .to_string()
                })
                .collect::<Vec<String>>(),
        )
    } else {
        None
    };

    Some(ModInfo {
        author: fields[2].trim_matches(|c: char| c == '-' || c == ' ').to_string(),
        name: format!("{}{}", fields[3], fields[2]),
        title: fields[3].trim().to_string(),
        url: fields[0].to_string(),
        iurl: if fields.len() >= 5 {
            fields[4].to_string()
        } else {
            String::new()
        },
        subdirs,
        args,
    })
}

fn take_matching<F>(pending: &mut Vec<ModInfo>, name: &str, matches: F) -> Option<ModInfo>
where
    F: Fn(&ModInfo) -> bool,
{
    let position = pending.iter().position(|candidate| matches(candidate))?;
    let mut info = pending.remove(position);
    info.name = name.to_string();
    Some(info)
}

pub fn read_mod_maker(mod_path: &Path, reporter: &Reporter) -> Result<Vec<ModEntry>> {
    reporter.info(format!("[+] Reading mod definition from {} ...", mod_path.display()));

    let modlist_text = read_text(&mod_path.join("modlist.txt"))?;
    let mut names: Vec<String> = Vec::new();
    for line in modlist_text.split('\n') {
        if line.starts_with('+') || line.starts_with('-') {
            let name = line[1..].trim().to_string();
            if !names.contains(&name) {
                names.push(name);
            }
        }
    }

    let maker_text = read_text(&mod_path.join("modpack_maker_list.txt"))?;
    let mut modmaker: Vec<ModInfo> = Vec::new();
    for line in maker_text.split('\n') {
        if line.is_empty() || line.starts_with(' ') {
            continue;
        }

        match parse_modpack_maker_line(line) {
            Some(info) => modmaker.push(info),
            None => reporter.info(format!("   Skipping: {line}")),
        }
    }

    let mut matched: Vec<Option<ModInfo>> = vec![None; names.len()];

    for (index, name) in names.iter().enumerate() {
        if let Some(info) = take_matching(&mut modmaker, name, |maker| name.contains(maker.name.as_str())) {
            matched[index] = Some(info);
        }
    }

    for (index, name) in names.iter().enumerate() {
        if let Some(info) = take_matching(&mut modmaker, name, |maker| name.contains(maker.title.as_str())) {
            matched[index] = Some(info);
        }
    }

    for maker in &modmaker {
        reporter.warn(format!("No mod folder found for {}", maker.name));
    }

    let mut result = Vec::new();
    for (name, data) in names.into_iter().zip(matched.into_iter()) {
        if name.contains("separator") {
            result.push(ModEntry::Separator(SeparatorInstaller::new(name)));
            continue;
        }

        let info = match data {
            Some(info) => info,
            None => continue,
        };

        if info.url.contains("addons/start/222467") && info.iurl.contains("github.com") {
            continue;
        }

        result.push(ModEntry::Default(DefaultInstaller::new(info)));
    }

    Ok(result)
}

pub fn modpack_data_dir(gamma: &Path) -> PathBuf {
    gamma
        .join(GROK_INSTALLER_DIR_NAME)
        .join(MODPACK_DIR_NAME)
        .join("modpack_data")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mods::downloader::Downloader;
    use std::sync::mpsc;

    fn reporter() -> Reporter {
        let (sender, _receiver) = mpsc::channel();
        Reporter::detached(sender)
    }

    fn fixture_dir() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("data")
            .join("modpack_test")
    }

    fn find<'a>(entries: &'a [ModEntry], name: &str) -> &'a ModEntry {
        entries
            .iter()
            .find(|entry| entry.info().name == name)
            .unwrap_or_else(|| panic!("mod {name} not found"))
    }

    #[test]
    fn reads_every_entry_of_the_fixture() {
        let entries = read_mod_maker(&fixture_dir(), &reporter()).unwrap();
        assert_eq!(entries.len(), 9);
    }

    #[test]
    fn separators_carry_only_a_name() {
        let entries = read_mod_maker(&fixture_dir(), &reporter()).unwrap();

        for name in [
            "G.A.M.M.A. End of List_separator",
            "Alternative Addons & Patches_separator",
        ] {
            let entry = find(&entries, name);
            assert!(matches!(entry, ModEntry::Separator(_)));
            assert!(entry.downloader().is_none());
            assert_eq!(entry.info().url, "");
        }
    }

    #[test]
    fn parses_github_archive_entries() {
        let entries = read_mod_maker(&fixture_dir(), &reporter()).unwrap();
        let entry = find(&entries, "282- GAMMA Loading Screens - CS Eden");
        let info = entry.info();

        assert!(matches!(entry, ModEntry::Default(_)));
        assert_eq!(info.author, "CS Eden");
        assert_eq!(info.title, "GAMMA Loading Screens");
        assert_eq!(
            info.url,
            "https://github.com/Grokitach/gamma_loading_screens/archive/refs/heads/main.zip"
        );
        assert_eq!(info.iurl, "https://github.com/Grokitach/gamma_loading_screens");
        assert_eq!(
            info.subdirs,
            Some(vec!["gamma_loading_screens-main/CS Eden's GAMMA Loading Screens".to_string()])
        );
        assert_eq!(
            info.args,
            Some(vec![
                "gamma_loading_screens.zip".to_string(),
                "9b60acaf459a82185cbfbd517209a37f".to_string(),
            ])
        );
        assert!(matches!(entry.downloader(), Some(Downloader::Default(_))));
    }

    #[test]
    fn parses_multiple_subdirs() {
        let entries = read_mod_maker(&fixture_dir(), &reporter()).unwrap();
        let entry = find(&entries, "71- Weapons Reanimation and Rebalance - Blindside");

        assert_eq!(
            entry.info().subdirs,
            Some(vec![
                "blindside_reanimation_legacy-main/main".to_string(),
                "blindside_reanimation_legacy-main/[OPTIONALS]/Vanilla Weapon Stats".to_string(),
            ])
        );
    }

    #[test]
    fn parses_moddb_entries() {
        let entries = read_mod_maker(&fixture_dir(), &reporter()).unwrap();
        let entry = find(&entries, "60- Stash Overhaul - Grokitach");
        let info = entry.info();

        assert_eq!(info.author, "Grokitach");
        assert_eq!(info.title, "Stash Overhaul");
        assert_eq!(info.url, "https://www.moddb.com/addons/start/200140");
        assert_eq!(
            info.iurl,
            "https://www.moddb.com/mods/stalker-anomaly/addons/groks-stash-overhaul-redux"
        );
        assert_eq!(info.subdirs, Some(vec!["00. Grok's Stash Overhaul".to_string()]));
        assert!(matches!(entry.downloader(), Some(Downloader::ModDb(_))));
    }

    #[test]
    fn zero_subdirs_means_none() {
        let entries = read_mod_maker(&fixture_dir(), &reporter()).unwrap();
        let entry = find(&entries, "22- Agressor Reshade - Awene");

        assert!(entry.info().subdirs.is_none());
    }

    #[test]
    fn missing_info_url_stays_empty() {
        let entries = read_mod_maker(&fixture_dir(), &reporter()).unwrap();
        let entry = find(&entries, "18- Ambient Music Pack - Wojach");

        assert_eq!(entry.info().iurl, "");
        assert_eq!(entry.info().subdirs, Some(vec!["00 MAIN FILES".to_string()]));
    }

    #[test]
    fn rejects_lines_without_enough_fields() {
        assert!(parse_modpack_maker_line("https://example.com\t0\t - a").is_none());
    }
}
