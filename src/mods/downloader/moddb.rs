use crate::error::{LauncherError, Result};
use crate::mods::downloader::base::{request_with_failover, DefaultDownloader};
use crate::report::Reporter;
use regex::Regex;
use scraper::{Html, Selector};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct ModDbDownloader {
    inner: DefaultDownloader,
    iurl: String,
}

impl ModDbDownloader {
    pub fn new(url: impl Into<String>, iurl: impl Into<String>) -> Self {
        Self {
            inner: DefaultDownloader::new(url),
            iurl: iurl.into(),
        }
    }

    pub fn url(&self) -> &str {
        self.inner.url()
    }

    pub fn archive(&self) -> Result<PathBuf> {
        self.inner.archive()
    }

    pub fn extract(&self, to: &Path) -> Result<()> {
        self.inner.extract(to)
    }

    async fn parse_metadata(url: &str, reporter: &Reporter) -> Result<HashMap<String, String>> {
        let response = request_with_failover(url, None, true, reporter).await?;
        let text = response.text().await?;

        let document = Html::parse_document(&text);
        let row_sel = Selector::parse("div.row.clear").unwrap();
        let h5_sel = Selector::parse("h5").unwrap();
        let span_sel = Selector::parse("span").unwrap();
        let dl_sel = Selector::parse("#downloadmirrorstoggle").unwrap();

        let mut result = HashMap::new();
        for row in document.select(&row_sel) {
            let name = row.select(&h5_sel).next().map(|e| e.text().collect::<String>());
            let value = row.select(&span_sel).next().map(|e| e.text().collect::<String>());

            if let (Some(name), Some(value)) = (name, value) {
                let name = name.trim().to_string();
                let value = value.trim().to_string();
                if name == "Filename" || name == "MD5 Hash" {
                    result.insert(name, value);
                }
            }
        }

        if let Some(href) = document.select(&dl_sel).next().and_then(|e| e.value().attr("href")) {
            result.insert("Download".to_string(), href.trim().to_string());
        }

        Ok(result)
    }

    async fn get_download_url(url: &str, reporter: &Reporter) -> Result<String> {
        let id = url.rsplit('/').next().unwrap_or_default().to_string();
        let response = request_with_failover(url, None, false, reporter).await?;
        let text = response.text().await?;

        let pattern = format!(r#"/downloads/mirror/{}/[^"]*"#, regex::escape(&id));
        let re = Regex::new(&pattern).map_err(|e| LauncherError::ModDbParse {
            url: url.to_string(),
            message: e.to_string(),
        })?;
        let m = re.find(&text).ok_or_else(|| LauncherError::ModDbParse {
            url: url.to_string(),
            message: "download link not found".to_string(),
        })?;

        let mirror_url = format!("https://www.moddb.com{}", m.as_str());
        let response = request_with_failover(&mirror_url, None, false, reporter).await?;
        Ok(response.url().to_string())
    }

    async fn set_vars_from_metadata(&mut self, reporter: &Reporter) -> Result<HashMap<String, String>> {
        if self.iurl.is_empty() {
            return Ok(HashMap::new());
        }

        match Self::parse_metadata(&self.iurl, reporter).await {
            Ok(metadata) => {
                self.inner.set_archive_hash(metadata.get("MD5 Hash").cloned());
                self.inner.set_user_wanted_name(metadata.get("Filename").cloned());
                Ok(metadata)
            }
            Err(LauncherError::Network(e)) if e.is_status() => Ok(HashMap::new()),
            Err(e) => Err(e),
        }
    }

    pub async fn check(&mut self, to: &Path, update_cache: bool, reporter: &Reporter) -> Result<()> {
        if self.iurl.is_empty() {
            return Err(LauncherError::Other("No Info URL provided for this mod".to_string()));
        }

        let metadata = self.set_vars_from_metadata(reporter).await?;

        if self.inner.user_wanted_name().is_none() {
            return Err(LauncherError::ModDbParse {
                url: self.iurl.clone(),
                message: "could not find Filename".to_string(),
            });
        }

        if self.inner.archive_hash().is_none() {
            return Err(LauncherError::ModDbParse {
                url: self.iurl.clone(),
                message: "could not find archive hash".to_string(),
            });
        }

        let download_field = metadata.get("Download").cloned().unwrap_or_default();
        if !self.inner.url().contains(download_field.as_str()) {
            return Err(LauncherError::Other(format!(
                "Skipping {} since ModDB info do not match download url",
                self.inner.user_wanted_name().unwrap_or_default()
            )));
        }

        let resolved = Self::get_download_url(self.inner.url(), reporter).await?;
        self.inner.set_url(resolved);

        self.inner.check(to, update_cache, reporter).await
    }

    pub async fn download(&mut self, to: &Path, use_cached: bool, reporter: &Reporter) -> Result<PathBuf> {
        let _ = self.set_vars_from_metadata(reporter).await?;
        let resolved = Self::get_download_url(self.inner.url(), reporter).await?;
        self.inner.set_url(resolved);

        self.inner.download(to, use_cached, None, reporter).await
    }
}
