use crate::archive::list_archive_content;
use crate::config::FOLDERS_TO_INSTALL;
use crate::error::Result;
use crate::fsutil::copy_tree;
use crate::mods::info::ModInfo;
use crate::mods::installer::base::BaseInstaller;
use crate::report::Reporter;
use crate::staging;
use scraper::{Html, Selector};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

const ARCHIVE_PREVIEW_LIMIT: usize = 20;

#[derive(Debug, Clone)]
pub struct DefaultInstaller {
    base: BaseInstaller,
}

impl DefaultInstaller {
    pub fn new(info: ModInfo) -> Self {
        Self {
            base: BaseInstaller::new(info),
        }
    }

    pub fn base(&self) -> &BaseInstaller {
        &self.base
    }

    pub fn base_mut(&mut self) -> &mut BaseInstaller {
        &mut self.base
    }

    pub fn into_base(self) -> BaseInstaller {
        self.base
    }

    pub fn info(&self) -> &ModInfo {
        self.base.info()
    }

    pub fn read_fomod_directives(dir: &Path) -> HashMap<PathBuf, PathBuf> {
        let mut result = HashMap::new();
        let module_config = dir.join("fomod").join("ModuleConfig.xml");

        let bytes = match fs::read(&module_config) {
            Ok(bytes) => bytes,
            Err(_) => return result,
        };

        let document = Html::parse_document(&decode_text(&bytes));
        let selector = match Selector::parse("folder") {
            Ok(selector) => selector,
            Err(_) => return result,
        };

        for element in document.select(&selector) {
            let attributes = element.value();
            if let (Some(source), Some(destination)) =
                (attributes.attr("source"), attributes.attr("destination"))
            {
                result.insert(dir.join(source), PathBuf::from(destination));
            }
        }

        result
    }

    fn report_archive_layout(&self, reporter: &Reporter) {
        let archive = match self.base.archive() {
            Ok(archive) => archive,
            Err(error) => {
                reporter.warn(format!("        No archive to inspect: {error}"));
                return;
            }
        };

        match list_archive_content(&archive) {
            Ok(entries) => {
                let mut roots: Vec<String> = entries
                    .iter()
                    .filter_map(|entry| entry.split(|c| c == '/' || c == '\\').next())
                    .filter(|entry| !entry.trim().is_empty())
                    .map(|entry| entry.to_string())
                    .collect();
                roots.sort();
                roots.dedup();

                let total = roots.len();
                roots.truncate(ARCHIVE_PREVIEW_LIMIT);

                reporter.info(format!(
                    "        The archive really contains: {}{}",
                    roots.join(", "),
                    if total > ARCHIVE_PREVIEW_LIMIT {
                        format!(" and {} more", total - ARCHIVE_PREVIEW_LIMIT)
                    } else {
                        String::new()
                    }
                ));
            }
            Err(error) => reporter.warn(format!("        Could not inspect the archive: {error}")),
        }
    }

    fn write_ini_file(&self, ini_file: &Path) -> Result<()> {
        let archive = self.base.archive()?;
        let archive_name = archive
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_default();

        let info = self.base.info();
        let url = if info.iurl.is_empty() {
            &info.url
        } else {
            &info.iurl
        };

        let lines = [
            "[General]".to_string(),
            "gameName=stalkeranomaly".to_string(),
            "modid=0".to_string(),
            format!("ignoredversion={archive_name}"),
            format!("version={archive_name}"),
            format!("newestversion={archive_name}"),
            "category=\"-1,\"".to_string(),
            "nexusFileStatus=1".to_string(),
            format!("installationFile={archive_name}"),
            "repository=".to_string(),
            "comments=".to_string(),
            "notes=".to_string(),
            "nexusDescription=".to_string(),
            format!("url={url}"),
            "hasCustomURL=true".to_string(),
            "lastNexusQuery=".to_string(),
            "lastNexusUpdate=".to_string(),
            "nexusLastModified=2021-11-09T18:10:18Z".to_string(),
            "converted=false".to_string(),
            "validated=false".to_string(),
            "color=@Variant(\\0\\0\\0\\x43\\0\\xff\\xff\\0\\0\\0\\0\\0\\0\\0\\0)".to_string(),
            "tracked=0".to_string(),
            String::new(),
            "[installedFiles]".to_string(),
            "1\\modid=0".to_string(),
            "1\\fileid=0".to_string(),
            "size=1".to_string(),
        ];

        let mut content = lines.join("\n");
        content.push('\n');
        fs::write(ini_file, content)?;
        Ok(())
    }

    pub fn install(&self, to: &Path, reporter: &Reporter) -> Result<()> {
        let install_dir = to.join(&self.base.info().name);
        fs::create_dir_all(&install_dir)?;

        let staged = staging::extract_with_hotfixes("gamma-launcher-modinstall-", |dir| {
            self.base.extract(dir)
        })?;
        let pdir = staged.path().to_path_buf();

        let mut iterator = vec![pdir.clone()];
        if let Some(subdirs) = &self.base.info().subdirs {
            iterator.extend(subdirs.iter().map(|subdir| pdir.join(subdir)));
        }

        let directives = Self::read_fomod_directives(&pdir);
        let mut inspected_archive = false;

        for current in &iterator {
            let display_name = current
                .file_name()
                .map(|name| name.to_string_lossy().to_string())
                .unwrap_or_default();

            if *current != pdir {
                reporter.info(format!(
                    "    Installing {display_name} -> {}",
                    install_dir.display()
                ));
            }

            if !current.exists() {
                reporter.warn(format!(
                    "    {display_name} does not exist in the extracted archive"
                ));
                if !inspected_archive {
                    inspected_archive = true;
                    self.report_archive_layout(reporter);
                }
            }

            if let Some(destination) = directives.get(current) {
                let fomod_dir = install_dir.join(destination);
                reporter.info(format!(
                    "        Applying FOMOD directive to {} -> {}",
                    current.display(),
                    fomod_dir.display()
                ));
                fs::create_dir_all(&fomod_dir)?;
                copy_tree(current, &fomod_dir)?;
                continue;
            }

            for game_dir in FOLDERS_TO_INSTALL {
                let source = current.join(game_dir);
                if !source.exists() {
                    continue;
                }
                copy_tree(&source, &install_dir.join(game_dir))?;
            }
        }

        drop(staged);
        self.write_ini_file(&install_dir.join("meta.ini"))
    }
}

fn decode_text(bytes: &[u8]) -> String {
    if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        return String::from_utf8_lossy(&bytes[3..]).into_owned();
    }
    if bytes.starts_with(&[0xFF, 0xFE]) {
        return decode_utf16(&bytes[2..], u16::from_le_bytes);
    }
    if bytes.starts_with(&[0xFE, 0xFF]) {
        return decode_utf16(&bytes[2..], u16::from_be_bytes);
    }
    String::from_utf8_lossy(bytes).into_owned()
}

fn decode_utf16(bytes: &[u8], convert: fn([u8; 2]) -> u16) -> String {
    let units: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|pair| convert([pair[0], pair[1]]))
        .collect();
    String::from_utf16_lossy(&units)
}
