use crate::commands::CommandArgs;
use crate::error::{LauncherError, Result};
use crate::hash::check_hash;
use crate::mods::downloader::Downloader;
use crate::mods::{modpack_data_dir, read_mod_maker};
use crate::report::Reporter;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf, MAIN_SEPARATOR_STR};

pub struct CheckAnomaly;

impl CheckAnomaly {
    fn read_checksums(anomaly: &Path) -> Result<Vec<(PathBuf, String)>> {
        let checksums = anomaly.join("tools").join("checksums.md5");
        let text = fs::read_to_string(&checksums)?;

        let mut entries = Vec::new();
        for line in text.split('\n') {
            let line = line.trim_end_matches('\r');
            if line.is_empty() {
                continue;
            }

            let (hash, file) = line.split_once(' ').ok_or_else(|| {
                LauncherError::Other(format!(
                    "malformed line in {}: {line}",
                    checksums.display()
                ))
            })?;

            let relative = file
                .trim_start_matches('*')
                .replace('\\', MAIN_SEPARATOR_STR);
            entries.push((anomaly.join(relative), hash.to_string()));
        }

        Ok(entries)
    }

    pub fn run(anomaly: &Path, reporter: &Reporter) -> Result<()> {
        let mut errors: Vec<String> = Vec::new();

        for (file, hash) in Self::read_checksums(anomaly)? {
            reporter.checkpoint()?;

            let label = format!("Checking Anomaly file: {}...", file.display());
            match check_hash(&file, &hash, reporter, &label) {
                Ok(outcome) if outcome.matches() => {}
                Ok(_) => errors.push(file.display().to_string()),
                Err(LauncherError::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => {
                    errors.push(file.display().to_string())
                }
                Err(error) => return Err(error),
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(LauncherError::Other(format!(
                "Invalid file(s) detected:\n{}",
                errors.join("\n")
            )))
        }
    }
}

pub struct CheckMd5;

impl CheckMd5 {
    fn purge_unused_files(dl_dir: &Path, downloaders: &[Downloader], reporter: &Reporter) -> Result<()> {
        let mut files_in_use: HashSet<PathBuf> = HashSet::new();
        for downloader in downloaders {
            if let Ok(archive) = downloader.archive() {
                files_in_use.insert(archive);
            }
        }

        files_in_use.insert(dl_dir.join("Anomaly-1.5.3-Full.2.7z"));
        files_in_use.insert(dl_dir.join("modorganizer-Mod.Organizer-2.5.2.7z"));

        for entry in fs::read_dir(dl_dir)? {
            let entry = entry?;
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();

            if name.ends_with(".git") || files_in_use.contains(&path) || path.is_dir() {
                continue;
            }

            reporter.info(format!("[+] Purging {}...", path.display()));
            fs::remove_file(&path)?;
        }

        Ok(())
    }

    pub async fn run(args: &CommandArgs, reporter: &Reporter) -> Result<()> {
        let dl_dir = args.downloads_dir()?;
        let modpack_data = modpack_data_dir(&args.gamma_dir()?);

        let mut order: Vec<String> = Vec::new();
        let mut by_url: HashMap<String, Downloader> = HashMap::new();
        for installer in read_mod_maker(&modpack_data, reporter)? {
            if let Some(downloader) = installer.into_downloader() {
                let url = downloader.url().to_string();
                if !by_url.contains_key(&url) {
                    order.push(url.clone());
                }
                by_url.insert(url, downloader);
            }
        }

        let mut downloaders: Vec<Downloader> = order
            .iter()
            .filter_map(|url| by_url.remove(url))
            .collect();

        reporter.warn(
            "This is a bit intensive for ModDB. You may be heavily checked by Cloudflare \
             and this command may stall if this is the case.",
        );
        reporter.info("-- Starting MD5 Check");

        let mut errors: Vec<String> = Vec::new();
        let total = downloaders.len();
        for (index, downloader) in downloaders.iter_mut().enumerate() {
            reporter.checkpoint_async().await?;

            let url = downloader.url().to_string();
            reporter.overall_progress(index, total, &url);

            match downloader.check(&dl_dir, args.update_cache, reporter).await {
                Ok(()) => {}
                Err(LauncherError::Cancelled) => return Err(LauncherError::Cancelled),
                Err(error) => errors.push(error.to_string()),
            }
        }
        reporter.overall_progress(total, total, "MD5 check complete");

        if args.remove_unused {
            Self::purge_unused_files(&dl_dir, &downloaders, reporter)?;
        }

        for error in &errors {
            reporter.error(error.clone());
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(LauncherError::Other(format!(
                "{} archive(s) failed verification",
                errors.len()
            )))
        }
    }
}
