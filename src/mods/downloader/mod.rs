pub mod base;
pub mod github;
pub mod moddb;

use crate::error::Result;
use crate::mods::downloader::base::{begin_network_item, DefaultDownloader};
use crate::mods::downloader::github::GithubDownloader;
use crate::mods::downloader::moddb::ModDbDownloader;
use crate::mods::info::ModInfo;
use crate::report::Reporter;
use std::path::{Path, PathBuf};

const PLAIN_ARCHIVE_SUFFIXES: [&str; 3] = [".zip", ".7z", ".rar"];

#[derive(Debug, Clone)]
pub enum Downloader {
    Default(DefaultDownloader),
    Github(GithubDownloader),
    ModDb(ModDbDownloader),
}

impl Downloader {
    pub fn from_info(info: &ModInfo) -> Option<Self> {
        if info.url.is_empty() {
            return None;
        }

        if info.url.contains("moddb.com") {
            return Some(Downloader::ModDb(ModDbDownloader::new(
                info.url.clone(),
                info.iurl.clone(),
            )));
        }

        let is_plain_archive = PLAIN_ARCHIVE_SUFFIXES
            .iter()
            .any(|suffix| info.url.ends_with(suffix));

        if info.url.contains("github.com") && !is_plain_archive {
            return Some(Downloader::Github(GithubDownloader::new(info.url.clone())));
        }

        let args = info.args.clone().unwrap_or_default();
        Some(Downloader::Default(DefaultDownloader::with_options(
            info.url.clone(),
            args.first().cloned(),
            args.get(1).cloned(),
        )))
    }

    pub fn url(&self) -> &str {
        match self {
            Downloader::Default(downloader) => downloader.url(),
            Downloader::Github(downloader) => downloader.url(),
            Downloader::ModDb(downloader) => downloader.url(),
        }
    }

    pub fn archive(&self) -> Result<PathBuf> {
        match self {
            Downloader::Default(downloader) => downloader.archive(),
            Downloader::Github(downloader) => downloader.archive(),
            Downloader::ModDb(downloader) => downloader.archive(),
        }
    }

    pub async fn check(&mut self, to: &Path, update_cache: bool, reporter: &Reporter) -> Result<()> {
        reporter.checkpoint_async().await?;
        begin_network_item(self.url(), reporter);

        match self {
            Downloader::Default(downloader) => downloader.check(to, update_cache, reporter).await,
            Downloader::Github(downloader) => downloader.check(to, update_cache, reporter).await,
            Downloader::ModDb(downloader) => downloader.check(to, update_cache, reporter).await,
        }
    }

    pub async fn download(
        &mut self,
        to: &Path,
        use_cached: bool,
        reporter: &Reporter,
    ) -> Result<PathBuf> {
        reporter.checkpoint_async().await?;
        begin_network_item(self.url(), reporter);

        match self {
            Downloader::Default(downloader) => {
                downloader.download(to, use_cached, None, reporter).await
            }
            Downloader::Github(downloader) => downloader.download(to, use_cached, reporter).await,
            Downloader::ModDb(downloader) => downloader.download(to, use_cached, reporter).await,
        }
    }

    pub fn extract(&self, to: &Path) -> Result<()> {
        match self {
            Downloader::Default(downloader) => downloader.extract(to),
            Downloader::Github(downloader) => downloader.extract(to),
            Downloader::ModDb(downloader) => downloader.extract(to),
        }
    }

    pub fn revision(&self) -> Option<String> {
        match self {
            Downloader::Github(downloader) => downloader.revision(),
            _ => None,
        }
    }
}
