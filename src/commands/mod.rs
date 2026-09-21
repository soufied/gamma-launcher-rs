pub mod check;
pub mod install;
pub mod keymap;
pub mod maintenance;
pub mod shader;
pub mod tests;
pub mod usvfs;

pub use check::{CheckAnomaly, CheckMd5};
pub use install::{AnomalyInstall, FullInstall, GammaSetup};
pub use keymap::{KeymapLayout, SwitchKeymap};
pub use maintenance::{estimate_reclaimable, SpaceEstimate};
pub use shader::{PurgeShaderCache, RemoveReshade};
pub use tests::TestModMaker;
pub use usvfs::Usvfs;

use crate::config::{expand_path, AppConfig};
use crate::error::{LauncherError, Result};
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct CommandArgs {
    pub anomaly: Option<PathBuf>,
    pub gamma: Option<PathBuf>,
    pub cache_path: Option<PathBuf>,
    pub effective_cache: Option<PathBuf>,
    pub downloads: Option<PathBuf>,
    pub mods: Option<PathBuf>,
    pub grok_installer: Option<PathBuf>,
    pub anomaly_verify: bool,
    pub anomaly_purge_cache: bool,
    pub install_mod_organizer: bool,
    pub mo_version: String,
    pub custom_definition: Option<String>,
    pub custom_repository: String,
    pub update_definition: bool,
    pub patch_anomaly: bool,
    pub preserve_user_config: bool,
    pub update_cache: bool,
    pub remove_unused: bool,
    pub force_recheck: bool,
    pub final_path: Option<PathBuf>,
}

impl CommandArgs {
    pub fn from_config(config: &AppConfig) -> Self {
        let revision = config
            .custom_gamma_revision
            .clone()
            .map(|revision| revision.trim().to_string())
            .filter(|revision| !revision.is_empty());

        Self {
            anomaly: config.expanded_anomaly_dir(),
            gamma: config.expanded_gamma_dir(),
            cache_path: config.cache_path.as_deref().map(expand_path),
            effective_cache: config.effective_cache_dir(),
            downloads: config.downloads_dir(),
            mods: config.mods_dir(),
            grok_installer: config.grok_installer_dir(),
            anomaly_verify: config.anomaly_verify,
            anomaly_purge_cache: config.anomaly_purge_cache,
            install_mod_organizer: config.install_mod_organizer,
            mo_version: config.mo_version.clone(),
            custom_definition: revision,
            custom_repository: config.custom_gamma_repository.clone(),
            update_definition: config.update_gamma_definition,
            patch_anomaly: config.patch_anomaly,
            preserve_user_config: config.preserve_user_config,
            update_cache: config.update_download_cache,
            remove_unused: config.purge_unused_downloads,
            force_recheck: config.force_recheck,
            final_path: config.usvfs_final_path.as_deref().map(expand_path),
        }
    }

    pub fn anomaly_dir(&self) -> Result<PathBuf> {
        self.anomaly
            .clone()
            .ok_or_else(|| LauncherError::Other("The Anomaly directory is not configured".into()))
    }

    pub fn gamma_dir(&self) -> Result<PathBuf> {
        self.gamma
            .clone()
            .ok_or_else(|| LauncherError::Other("The GAMMA directory is not configured".into()))
    }

    pub fn cache_dir(&self) -> Result<PathBuf> {
        self.effective_cache.clone().ok_or_else(|| {
            LauncherError::Other(
                "Neither a download cache nor an Anomaly directory is configured".into(),
            )
        })
    }

    pub fn downloads_dir(&self) -> Result<PathBuf> {
        self.downloads
            .clone()
            .ok_or_else(|| LauncherError::Other("The GAMMA directory is not configured".into()))
    }

    pub fn mods_dir(&self) -> Result<PathBuf> {
        self.mods
            .clone()
            .ok_or_else(|| LauncherError::Other("The GAMMA directory is not configured".into()))
    }

    pub fn grok_installer_dir(&self) -> Result<PathBuf> {
        self.grok_installer
            .clone()
            .ok_or_else(|| LauncherError::Other("The GAMMA directory is not configured".into()))
    }

    pub fn final_dir(&self) -> Result<PathBuf> {
        self.final_path.clone().ok_or_else(|| {
            LauncherError::Other("The final install directory is not configured".into())
        })
    }
}
