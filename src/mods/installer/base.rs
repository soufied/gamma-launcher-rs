use crate::error::{LauncherError, Result};
use crate::mods::downloader::Downloader;
use crate::mods::info::ModInfo;
use crate::report::Reporter;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct BaseInstaller {
    info: ModInfo,
    downloader: Option<Downloader>,
}

impl BaseInstaller {
    pub fn new(info: ModInfo) -> Self {
        let downloader = Downloader::from_info(&info);
        Self { info, downloader }
    }

    pub fn info(&self) -> &ModInfo {
        &self.info
    }

    pub fn downloader(&self) -> Option<&Downloader> {
        self.downloader.as_ref()
    }

    pub fn downloader_mut(&mut self) -> Option<&mut Downloader> {
        self.downloader.as_mut()
    }

    pub fn into_downloader(self) -> Option<Downloader> {
        self.downloader
    }

    fn require_downloader(&self, capability: &str) -> Result<&Downloader> {
        self.downloader.as_ref().ok_or_else(|| {
            LauncherError::MissingModMetadata(format!(
                "{} does not support {capability} since no URL was provided",
                self.info.display_title()
            ))
        })
    }

    pub async fn check(
        &mut self,
        dl_dir: &Path,
        update_cache: bool,
        reporter: &Reporter,
    ) -> Result<()> {
        match self.downloader_mut() {
            Some(downloader) => downloader.check(dl_dir, update_cache, reporter).await,
            None => Ok(()),
        }
    }

    pub async fn download(
        &mut self,
        to: &Path,
        use_cached: bool,
        reporter: &Reporter,
    ) -> Result<PathBuf> {
        let title = self.info.display_title().to_string();
        let downloader = self.downloader_mut().ok_or_else(|| {
            LauncherError::MissingModMetadata(format!(
                "{title} does not support download() since no URL was provided"
            ))
        })?;

        downloader.download(to, use_cached, reporter).await
    }

    pub fn extract(&self, to: &Path) -> Result<()> {
        self.require_downloader("extract()")?.extract(to)
    }

    pub fn install(&self, to: &Path, _reporter: &Reporter) -> Result<()> {
        self.extract(to)
    }

    pub fn archive(&self) -> Result<PathBuf> {
        self.require_downloader("the archive property")?.archive()
    }

    pub fn revision(&self) -> Option<String> {
        self.downloader.as_ref().and_then(Downloader::revision)
    }
}
