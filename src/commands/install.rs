use crate::commands::check::CheckAnomaly;
use crate::commands::maintenance;
use crate::commands::CommandArgs;
use crate::error::{LauncherError, Result};
use crate::config::MODPACK_DIR_NAME;
use crate::fsutil::{copy_tree, directory_has_entries, prepare_file_target};
use crate::mods::{base_archive, git_resource, modpack_data_dir, moddb_archive, read_mod_maker};
use crate::report::Reporter;
use crate::userltx::UserLtx;
use std::fs;
use std::path::Path;
use std::process::Command;

const GUIDE_URL: &str = "https://github.com/DravenusRex/stalker-gamma-linux-guide";
const ANOMALY_NAME: &str = "base-1.5.3";
const ANOMALY_URL: &str = "https://www.moddb.com/downloads/start/277404";
const ANOMALY_INFO_URL: &str =
    "https://www.moddb.com/mods/stalker-anomaly/downloads/stalker-anomaly-153";
const GAMMA_SETUP_URL: &str = "https://github.com/Grokitach/gamma_setup";
const LARGE_FILES_URL: &str = "https://github.com/Grokitach/gamma_large_files_v2";
const GUNSLINGER_URL: &str = "https://github.com/Grokitach/teivaz_anomaly_gunslinger";
const GUNSLINGER_MOD_DIR: &str = "312- Gunslinger Guns for Anomaly - Teivazcz & Gunslinger Team";
pub(crate) const PROFILE_SETTINGS: &str = "[General]\n\
LocalSaves=false\n\
LocalSettings=true\n\
AutomaticArchiveInvalidation=false\n";

fn available_space(dir: &Path) -> Option<u64> {
    let output = Command::new("df").arg("-Pk").arg(dir).output().ok()?;
    if !output.status.success() {
        return None;
    }

    let text = String::from_utf8_lossy(&output.stdout);
    let line = text.lines().nth(1)?;
    let kilobytes: u64 = line.split_whitespace().nth(3)?.parse().ok()?;
    Some(kilobytes * 1024)
}

pub fn check_tmp_free_space(size: u64, reporter: &Reporter) -> Result<()> {
    let dir = std::env::temp_dir();

    match available_space(&dir) {
        Some(free) if free < size * 1024 * 1024 * 1024 => Err(LauncherError::Other(format!(
            "You need at least {size} GiB of space in TMPDIR for this to work.\n\
             Please export TMPDIR to a folder with enough space available."
        ))),
        Some(_) => Ok(()),
        None => {
            reporter.warn(format!(
                "Could not determine free space in {}, continuing anyway",
                dir.display()
            ));
            Ok(())
        }
    }
}

#[cfg(unix)]
fn link_downloads(cache: &Path, downloads: &Path) -> Result<()> {
    if downloads.is_symlink() {
        return Ok(());
    }

    let target = cache.canonicalize().unwrap_or_else(|_| cache.to_path_buf());
    fs::remove_dir(downloads)?;
    std::os::unix::fs::symlink(target, downloads)?;
    Ok(())
}

#[cfg(not(unix))]
fn link_downloads(_cache: &Path, _downloads: &Path) -> Result<()> {
    Ok(())
}

pub struct AnomalyInstall;

impl AnomalyInstall {
    pub async fn run(args: &CommandArgs, reporter: &Reporter) -> Result<()> {
        let anomaly_dir = args.anomaly_dir()?;
        fs::create_dir_all(&anomaly_dir)?;

        let cache_dir = args.cache_dir()?;
        fs::create_dir_all(&cache_dir)?;

        reporter.info("[+] Installing base Anomaly 1.5.3");
        let mut mod_base = moddb_archive(ANOMALY_NAME, ANOMALY_URL, ANOMALY_INFO_URL);
        mod_base.download(&cache_dir, true, reporter).await?;

        reporter.info("  - Extracting");
        mod_base.install(&anomaly_dir, reporter)?;

        if args.anomaly_verify {
            CheckAnomaly::run(&anomaly_dir, reporter)?;
        }

        if args.anomaly_purge_cache {
            reporter.info("[+] Purging Anomaly archives");
            fs::remove_file(mod_base.archive()?)?;
        }

        Ok(())
    }
}

pub struct GammaSetup;

impl GammaSetup {
    async fn install_mod_organizer(
        version: &str,
        cache_dir: Option<&Path>,
        gamma_dir: &Path,
        reporter: &Reporter,
    ) -> Result<()> {
        let url = format!(
            "https://github.com/ModOrganizer2/modorganizer/releases/download/{version}/Mod.Organizer-{}.7z",
            version.trim_start_matches('v')
        );

        let staging = tempfile::Builder::new()
            .prefix("gamma-launcher-mo-setup-")
            .tempdir()?;
        let target = cache_dir.unwrap_or_else(|| staging.path());

        let mut archive = base_archive(url);
        archive.download(target, true, reporter).await?;
        archive.extract(gamma_dir)
    }

    pub async fn run(args: &CommandArgs, reporter: &Reporter) -> Result<()> {
        check_tmp_free_space(12, reporter)?;

        let gamma_dir = args.gamma_dir()?;
        let grok_mod_dir = args.grok_installer_dir()?.join(MODPACK_DIR_NAME);
        fs::create_dir_all(&grok_mod_dir)?;

        let cache_dir = args.cache_path.clone();
        if let Some(cache) = &cache_dir {
            fs::create_dir_all(cache)?;
        }

        reporter.info("[+] Installing base setup for GAMMA");
        if args.install_mod_organizer {
            Self::install_mod_organizer(
                &args.mo_version,
                cache_dir.as_deref(),
                &gamma_dir,
                reporter,
            )
            .await?;
        }

        let downloads_dir = args.downloads_dir()?;
        fs::create_dir_all(&downloads_dir)?;

        if let Some(cache) = &cache_dir {
            link_downloads(cache, &downloads_dir)?;
        }

        let mut archive = base_archive(GAMMA_SETUP_URL);
        archive.download(&downloads_dir, true, reporter).await?;
        archive.extract(&grok_mod_dir)?;

        fs::create_dir_all(args.mods_dir()?)?;
        Ok(())
    }
}

pub struct FullInstall;

impl FullInstall {
    async fn update_gamma_definition(
        repository: &str,
        dl_dir: &Path,
        grok_mod_dir: &Path,
        reporter: &Reporter,
    ) -> Result<()> {
        reporter.info("[+] Updating G.A.M.M.A. definition");

        let revision_file = grok_mod_dir.join("revision.txt");
        let mut archive = base_archive(format!("https://github.com/{repository}"));
        archive.download(dl_dir, true, reporter).await?;

        if let Ok(current) = fs::read_to_string(&revision_file) {
            let current = current.trim().to_string();

            if current.starts_with("Custom") {
                reporter.info(
                    "[*] A custom G.A.M.M.A. definition was used to init this installation, skipping...",
                );
                return Ok(());
            }

            if Some(current) == archive.revision() {
                reporter.info("[*] Already on the same revision, skipping...");
                return Ok(());
            }
        }

        archive.extract(grok_mod_dir)?;

        let revision = archive.revision().unwrap_or_default();
        fs::write(&revision_file, format!("{revision}\n"))?;
        Ok(())
    }

    async fn set_custom_gamma_definition(
        repository: &str,
        revision: &str,
        dl_dir: &Path,
        grok_mod_dir: &Path,
        reporter: &Reporter,
    ) -> Result<()> {
        let revision_file = grok_mod_dir.join("revision.txt");
        reporter.info(format!(
            "[+] Setting custom G.A.M.M.A. definition to: {revision}"
        ));

        let mut archive =
            base_archive(format!("https://github.com/{repository}/archive/{revision}.zip"));
        archive.download(dl_dir, false, reporter).await?;
        archive.extract(grok_mod_dir)?;

        fs::write(&revision_file, format!("Custom: {revision}\n"))?;
        Ok(())
    }

    fn patch_anomaly(
        anomaly_dir: &Path,
        grok_mod_dir: &Path,
        preserve_user_config: bool,
        reporter: &Reporter,
    ) -> Result<()> {
        reporter.info(format!("[+] Patching Anomaly in {}", anomaly_dir.display()));

        let user_config = anomaly_dir.join("appdata").join("user.ltx");
        let saved_config = anomaly_dir.join("appdata").join("user.ltx.bak");

        if user_config.is_file() {
            fs::copy(&user_config, &saved_config)?;
        }

        copy_tree(
            &grok_mod_dir.join("G.A.M.M.A").join("modpack_patches"),
            anomaly_dir,
        )?;

        if preserve_user_config {
            if saved_config.is_file() {
                fs::copy(&saved_config, &user_config)?;
            }
            return Ok(());
        }

        let mut config = if user_config.is_file() {
            UserLtx::open(user_config.as_path())?
        } else {
            UserLtx::new()
        };

        config.set("rs_screenmode", "borderless");
        config.save(Some(user_config.as_path()))
    }

    fn is_already_installed(mod_dir: &Path, name: &str) -> bool {
        if name.is_empty() {
            return false;
        }
        let install_dir = mod_dir.join(name);
        install_dir.join("meta.ini").is_file() && directory_has_entries(&install_dir)
    }

    async fn install_mods(
        gamma_dir: &Path,
        dl_dir: &Path,
        mod_dir: &Path,
        force_recheck: bool,
        reporter: &Reporter,
    ) -> Result<()> {
        let mut mods = read_mod_maker(&modpack_data_dir(gamma_dir), reporter)?;
        let total = mods.len();

        if force_recheck {
            reporter.info("[*] Force recheck is enabled, every mod will be verified and reinstalled");
        } else {
            reporter.info("[*] Mods that are already installed and valid will be skipped");
        }

        let mut installed = 0usize;
        let mut skipped = 0usize;

        for (index, entry) in mods.iter_mut().enumerate() {
            reporter.checkpoint_async().await?;

            let title = entry.info().display_title().to_string();
            let byline = entry.info().display_byline();
            let name = entry.info().name.clone();

            reporter.overall_progress(index, total, &title);
            reporter.info(format!(
                "[+] Processing mod {byline} ({}/{total})",
                index + 1
            ));

            if name == "164- Hunger Thirst Sleep UI 0.71 - xcvb" {
                reporter.info("    Skipping, this entry is excluded by the launcher");
                skipped += 1;
                continue;
            }
            if entry.info().title == "FDDA Redone Fixes" {
                reporter.info("    Skipping, this entry is excluded by the launcher");
                skipped += 1;
                continue;
            }

            if !force_recheck && Self::is_already_installed(mod_dir, &name) {
                reporter.info(format!(
                    "[=] Skipping {title}, it is already installed in {}",
                    mod_dir.join(&name).display()
                ));
                skipped += 1;
                continue;
            }

            entry.download(dl_dir, true, reporter).await?;
            reporter.info(format!("    Installing {title}"));
            entry.install(mod_dir, reporter)?;
            installed += 1;
        }

        reporter.overall_progress(total, total, "Mod installation complete");
        reporter.info(format!(
            "[+] Mods processed: {total} total, {installed} installed, {skipped} skipped"
        ));
        Ok(())
    }

    async fn install_git_resources(
        dl_dir: &Path,
        mod_dir: &Path,
        reporter: &Reporter,
    ) -> Result<()> {
        reporter.info("[+] Installing Git Resources");

        let mut resource = git_resource(LARGE_FILES_URL, false);
        resource.base_mut().download(dl_dir, false, reporter).await?;
        resource.install(mod_dir, reporter)?;

        let mut resource = git_resource(GUNSLINGER_URL, true);
        resource.base_mut().download(dl_dir, false, reporter).await?;
        resource.install(&mod_dir.join(GUNSLINGER_MOD_DIR), reporter)?;

        Ok(())
    }

    fn copy_gamma_modpack(grok_mod_dir: &Path, mod_dir: &Path, reporter: &Reporter) -> Result<()> {
        let path = grok_mod_dir.join("G.A.M.M.A").join("modpack_addons");
        reporter.info(format!(
            "[+] Copying G.A.M.M.A mods from \"{}\" to \"{}\"",
            path.display(),
            mod_dir.display()
        ));
        copy_tree(&path, mod_dir)
    }

    pub fn install_modorganizer_profile(gamma_dir: &Path, reporter: &Reporter) -> Result<()> {
        let profile_dir = gamma_dir.join("profiles").join("G.A.M.M.A");
        let settings = profile_dir.join("settings.txt");

        reporter.info(format!(
            "[+] Installing G.A.M.M.A profile in {}",
            profile_dir.display()
        ));
        fs::create_dir_all(&profile_dir)?;

        let modlist = modpack_data_dir(gamma_dir).join("modlist.txt");
        let deployed = profile_dir.join("modlist.txt");

        prepare_file_target(&deployed)?;
        fs::copy(&modlist, &deployed)?;
        fs::write(&settings, PROFILE_SETTINGS)?;

        Ok(())
    }

    pub async fn run(args: &CommandArgs, reporter: &Reporter) -> Result<()> {
        check_tmp_free_space(6, reporter)?;

        let anomaly_dir = args.anomaly_dir()?;
        let gamma_dir = args.gamma_dir()?;

        let dl_dir = args.downloads_dir()?;
        let mod_dir = args.mods_dir()?;
        let grok_mod_dir = args.grok_installer_dir()?;

        fs::create_dir_all(&dl_dir)?;

        maintenance::sanitize_mod_organizer_ini(args, reporter)?;

        if !anomaly_dir.join("bin").is_dir() {
            AnomalyInstall::run(args, reporter).await?;
        }

        if !(mod_dir.is_dir() && grok_mod_dir.is_dir()) {
            GammaSetup::run(args, reporter).await?;
        }

        if args.update_definition {
            match &args.custom_definition {
                Some(revision) => {
                    Self::set_custom_gamma_definition(
                        &args.custom_repository,
                        revision,
                        &dl_dir,
                        &grok_mod_dir,
                        reporter,
                    )
                    .await?
                }
                None => {
                    Self::update_gamma_definition(
                        &args.custom_repository,
                        &dl_dir,
                        &grok_mod_dir,
                        reporter,
                    )
                    .await?
                }
            }
        }

        if args.patch_anomaly {
            Self::patch_anomaly(
                &anomaly_dir,
                &grok_mod_dir,
                args.preserve_user_config,
                reporter,
            )?;
        }

        Self::install_mods(&gamma_dir, &dl_dir, &mod_dir, args.force_recheck, reporter).await?;
        Self::install_git_resources(&dl_dir, &mod_dir, reporter).await?;
        Self::install_modorganizer_profile(&gamma_dir, reporter)?;
        Self::copy_gamma_modpack(&grok_mod_dir, &mod_dir, reporter)?;

        reporter.info("[+] Setup ended... Enjoy your journey in the Zone o/");
        reporter.info(format!("[*] Linux setup guide: {GUIDE_URL}"));
        Ok(())
    }
}

