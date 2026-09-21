use crate::archive::extract_archive;
use crate::config::ProxyConfig;
use crate::error::{LauncherError, Result};
use crate::hash::check_hash;
use crate::report::{human_bytes, human_speed, Reporter};
use futures_util::StreamExt;
use regex::Regex;
use reqwest::{Client, Proxy, RequestBuilder, Response};
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock, RwLock};
use std::time::{Duration, Instant};

const USER_AGENT: &str = concat!("gamma-launcher-rust/", env!("CARGO_PKG_VERSION"));
const MAX_ATTEMPTS: u32 = 4;
const RETRY_DELAY: Duration = Duration::from_secs(30);
const PROGRESS_INTERVAL: Duration = Duration::from_millis(100);
const LOG_INTERVAL: Duration = Duration::from_secs(3);
const SPEED_SAMPLE_WINDOW: f64 = 0.25;
const SPEED_SMOOTHING: f64 = 0.3;

pub fn proxy_endpoint(proxy: &ProxyConfig) -> Option<String> {
    let host = proxy.host.trim();
    if host.is_empty() {
        return None;
    }

    let user = proxy.username.trim();
    if user.is_empty() {
        Some(format!("socks5h://{host}:{}", proxy.port))
    } else {
        Some(format!(
            "socks5h://{}:{}@{host}:{}",
            user,
            proxy.password.trim(),
            proxy.port
        ))
    }
}

fn build_client(proxy: Option<&str>) -> Result<Client> {
    let mut builder = Client::builder()
        .user_agent(USER_AGENT)
        .cookie_store(true)
        .gzip(true)
        .connect_timeout(Duration::from_secs(30));

    if let Some(url) = proxy {
        builder = builder.proxy(Proxy::all(url)?);
    }

    Ok(builder.build()?)
}

#[derive(Clone)]
struct NetworkPolicy {
    direct: Arc<Client>,
    proxy: Option<Arc<Client>>,
    endpoint: String,
    always_proxy: bool,
    auto_retry: bool,
    threshold: usize,
}

impl NetworkPolicy {
    fn direct_only() -> Self {
        let client = build_client(None).expect("failed to build the default HTTP client");
        Self {
            direct: Arc::new(client),
            proxy: None,
            endpoint: String::new(),
            always_proxy: false,
            auto_retry: false,
            threshold: 2,
        }
    }
}

static FAILURE_COUNT: AtomicUsize = AtomicUsize::new(0);
static FALLBACK_ACTIVE: AtomicBool = AtomicBool::new(false);

fn policy_slot() -> &'static RwLock<NetworkPolicy> {
    static POLICY: OnceLock<RwLock<NetworkPolicy>> = OnceLock::new();
    POLICY.get_or_init(|| RwLock::new(NetworkPolicy::direct_only()))
}

fn policy() -> NetworkPolicy {
    let guard = match policy_slot().read() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    guard.clone()
}

pub fn configure_http_client(proxy: &ProxyConfig, reporter: &Reporter) -> Result<()> {
    let endpoint = proxy_endpoint(proxy);
    let always_proxy = proxy.enabled && endpoint.is_some();
    let auto_retry = proxy.socks5_auto_retry;
    let threshold = proxy.effective_threshold();

    let direct = Arc::new(build_client(None)?);
    let routed = match endpoint.as_deref() {
        Some(url) if always_proxy || auto_retry => Some(Arc::new(build_client(Some(url))?)),
        _ => None,
    };

    let credentials = if proxy.username.trim().is_empty() {
        "anonymous"
    } else {
        "authenticated"
    };

    if always_proxy {
        reporter.info(format!(
            "[*] SOCKS5 proxy enabled for every request: {}:{} ({credentials})",
            proxy.host.trim(),
            proxy.port
        ));
    } else if auto_retry && routed.is_some() {
        reporter.info(format!(
            "[*] Networking: direct connection, SOCKS5 fallback to {}:{} armed after {threshold} failed attempt(s)",
            proxy.host.trim(),
            proxy.port
        ));
    } else if auto_retry {
        reporter.warn(
            "[!] Auto Enable on Retries is on but no SOCKS5 host is configured, the fallback stays disabled",
        );
    } else {
        reporter.info("[*] Networking: direct connection, no proxy configured");
    }

    FAILURE_COUNT.store(0, Ordering::SeqCst);
    FALLBACK_ACTIVE.store(false, Ordering::SeqCst);

    let updated = NetworkPolicy {
        direct,
        proxy: routed,
        endpoint: endpoint.unwrap_or_default(),
        always_proxy,
        auto_retry,
        threshold,
    };

    let mut guard = match policy_slot().write() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    *guard = updated;
    Ok(())
}

pub fn http_client() -> Arc<Client> {
    let policy = policy();

    if policy.always_proxy || FALLBACK_ACTIVE.load(Ordering::SeqCst) {
        if let Some(client) = policy.proxy.as_ref() {
            return Arc::clone(client);
        }
    }

    Arc::clone(&policy.direct)
}

pub fn begin_network_item(item: &str, reporter: &Reporter) {
    FAILURE_COUNT.store(0, Ordering::SeqCst);

    if FALLBACK_ACTIVE.swap(false, Ordering::SeqCst) {
        reporter.info(format!(
            "[Network] Reverting to a direct connection for {item}, the SOCKS5 fallback is armed again"
        ));
    }
}

fn failure_wording(failures: usize) -> String {
    match failures {
        1 => "once".to_string(),
        2 => "twice".to_string(),
        other => format!("{other} times"),
    }
}

pub fn record_network_failure(item: &str, reporter: &Reporter) -> bool {
    let failures = FAILURE_COUNT.fetch_add(1, Ordering::SeqCst) + 1;
    let policy = policy();

    if policy.always_proxy || !policy.auto_retry || failures < policy.threshold {
        return false;
    }

    if FALLBACK_ACTIVE.load(Ordering::SeqCst) {
        return false;
    }

    match policy.proxy.as_ref() {
        Some(_) => {
            FALLBACK_ACTIVE.store(true, Ordering::SeqCst);
            reporter.warn(format!(
                "[Network] Direct download failed {} for {item}. Engaging SOCKS5 proxy fallback...",
                failure_wording(failures)
            ));
            reporter.info(format!("[Network] Routing {item} through {}", policy.endpoint));
            true
        }
        None => {
            reporter.warn(format!(
                "[Network] Direct download failed {} for {item}, but no SOCKS5 proxy host is configured, staying on the direct connection",
                failure_wording(failures)
            ));
            false
        }
    }
}

pub fn fallback_is_active() -> bool {
    FALLBACK_ACTIVE.load(Ordering::SeqCst)
}

fn is_retryable(error: &reqwest::Error) -> bool {
    if error.is_timeout() || error.is_connect() || error.is_request() {
        return true;
    }

    match error.status() {
        Some(status) => matches!(status.as_u16(), 403 | 408 | 425 | 429 | 500..=599),
        None => true,
    }
}

async fn send_once(request: RequestBuilder, check_status: bool) -> Result<Response> {
    let response = request.send().await?;

    if check_status {
        return Ok(response.error_for_status()?);
    }

    Ok(response)
}

pub async fn request_with_failover(
    url: &str,
    accept: Option<&str>,
    check_status: bool,
    reporter: &Reporter,
) -> Result<Response> {
    let mut attempt: u32 = 1;

    loop {
        reporter.checkpoint_async().await?;

        let mut request = http_client().get(url);
        if let Some(value) = accept {
            request = request.header("Accept", value);
        }

        match send_once(request, check_status).await {
            Ok(response) => return Ok(response),
            Err(LauncherError::Network(error)) => {
                if attempt >= MAX_ATTEMPTS || !is_retryable(&error) {
                    return Err(LauncherError::Network(error));
                }

                reporter.warn(format!("[!] Request to {url} failed: {error}"));
                let engaged = record_network_failure(url, reporter);
                attempt += 1;

                if !engaged {
                    reporter.warn(format!(
                        "[!] Retrying in {}s (attempt {attempt}/{MAX_ATTEMPTS})",
                        RETRY_DELAY.as_secs()
                    ));
                    reporter.control().sleep_for(RETRY_DELAY).await?;
                }
            }
            Err(error) => return Err(error),
        }
    }
}

pub fn github_capture(url: &str) -> Option<(String, String, Option<String>)> {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    let pattern = PATTERN.get_or_init(|| {
        Regex::new(r"^https?://github\.com/([\w.-]+)/([\w.-]+)(/archive/(\w+)\.zip)?")
            .expect("the GitHub URL pattern is valid")
    });

    let captures = pattern.captures(url)?;
    let user = captures.get(1)?.as_str().to_string();
    let project = captures.get(2)?.as_str().to_string();
    let revision = captures.get(4).map(|found| found.as_str().to_string());
    Some((user, project, revision))
}

pub fn url_basename(url: &str) -> String {
    let path = match reqwest::Url::parse(url) {
        Ok(parsed) => parsed.path().to_string(),
        Err(_) => url.split(&['?', '#'][..]).next().unwrap_or_default().to_string(),
    };
    path.rsplit('/').next().unwrap_or_default().to_string()
}

fn file_name_of(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_default()
}

fn hash_label(archive: &Path) -> String {
    format!("Calculating hash of {}", file_name_of(archive))
}

fn partial_path(archive: &Path) -> PathBuf {
    let mut name = archive.as_os_str().to_os_string();
    name.push(".part");
    PathBuf::from(name)
}

#[derive(Debug, Clone)]
pub struct DefaultDownloader {
    url: String,
    archive: Option<PathBuf>,
    archive_hash: Option<String>,
    user_wanted_name: Option<String>,
}

impl DefaultDownloader {
    pub fn new(url: impl Into<String>) -> Self {
        Self::with_options(url, None, None)
    }

    pub fn with_options(
        url: impl Into<String>,
        filename: Option<String>,
        filehash: Option<String>,
    ) -> Self {
        Self {
            url: url.into(),
            archive: None,
            archive_hash: filehash,
            user_wanted_name: filename,
        }
    }

    pub fn url(&self) -> &str {
        &self.url
    }

    pub fn set_url(&mut self, url: String) {
        self.url = url;
    }

    pub fn archive(&self) -> Result<PathBuf> {
        self.archive.clone().ok_or_else(|| {
            LauncherError::Other("archive not available, run check() or download() first".to_string())
        })
    }

    pub fn set_archive(&mut self, archive: PathBuf) {
        self.archive = Some(archive);
    }

    pub fn archive_hash(&self) -> Option<&str> {
        self.archive_hash.as_deref()
    }

    pub fn set_archive_hash(&mut self, hash: Option<String>) {
        self.archive_hash = hash;
    }

    pub fn user_wanted_name(&self) -> Option<&str> {
        self.user_wanted_name.as_deref()
    }

    pub fn set_user_wanted_name(&mut self, name: Option<String>) {
        self.user_wanted_name = name;
    }

    fn set_archive_name(&mut self, to: &Path) -> Result<()> {
        if self.archive.is_some() {
            return Ok(());
        }

        let wanted = self
            .user_wanted_name
            .clone()
            .filter(|name| !name.is_empty());
        if let Some(name) = wanted {
            self.archive = Some(to.join(name));
            return Ok(());
        }

        if self.url.contains("github.com") {
            let (_, project, _) = github_capture(&self.url).ok_or_else(|| {
                LauncherError::Other(format!("{} is not a valid github.com URL", self.url))
            })?;
            self.archive = Some(to.join(format!("{project}-{}", url_basename(&self.url))));
            return Ok(());
        }

        self.archive = Some(to.join(url_basename(&self.url)));
        Ok(())
    }

    async fn check_if_non_exist(
        &mut self,
        to: &Path,
        update_cache: bool,
        reporter: &Reporter,
    ) -> Result<()> {
        let archive = self.archive()?;
        let name = file_name_of(&archive);

        if !update_cache {
            return Err(LauncherError::Other(format!(
                "Hash verification failed since {name} does not exist"
            )));
        }

        self.download(to, false, None, reporter).await?;

        if let Some(expected) = self.archive_hash.clone() {
            let label = hash_label(&archive);
            let outcome = check_hash(&archive, &expected, reporter, &label)?;
            if !outcome.matches() {
                reporter.error(format!("Hash verification failed after download for {name}"));
                return Err(LauncherError::HashMismatch {
                    file: archive,
                    expected: outcome.expected,
                    actual: outcome.computed,
                });
            }
        }

        Ok(())
    }

    async fn check_if_exist(
        &mut self,
        to: &Path,
        update_cache: bool,
        reporter: &Reporter,
    ) -> Result<()> {
        let expected = match self.archive_hash.clone() {
            Some(expected) => expected,
            None => return Ok(()),
        };

        let archive = self.archive()?;
        let label = hash_label(&archive);
        let outcome = check_hash(&archive, &expected, reporter, &label)?;
        if outcome.matches() {
            return Ok(());
        }

        if update_cache {
            fs::remove_file(&archive)?;
            return self.check_if_non_exist(to, update_cache, reporter).await;
        }

        Err(LauncherError::HashMismatch {
            file: archive,
            expected: outcome.expected,
            actual: outcome.computed,
        })
    }

    pub async fn check(&mut self, to: &Path, update_cache: bool, reporter: &Reporter) -> Result<()> {
        self.set_archive_name(to)?;

        if self.archive()?.exists() {
            self.check_if_exist(to, update_cache, reporter).await
        } else {
            self.check_if_non_exist(to, update_cache, reporter).await
        }
    }

    pub async fn download(
        &mut self,
        to: &Path,
        use_cached: bool,
        hash: Option<&str>,
        reporter: &Reporter,
    ) -> Result<PathBuf> {
        self.set_archive_name(to)?;
        let archive = self.archive()?;

        let expected: Option<String> = hash
            .map(str::to_string)
            .or_else(|| self.archive_hash.clone());

        let name = file_name_of(&archive);

        if archive.exists() && use_cached {
            match expected.as_deref() {
                None => {
                    reporter.info(format!(
                        "[=] Skipping download of {name}, a cached archive of {} already exists",
                        human_bytes(archive_size(&archive))
                    ));
                    return Ok(archive);
                }
                Some(checksum) => {
                    reporter.info(format!("[*] Verifying cached archive {name} before download"));
                    let label = hash_label(&archive);
                    let outcome = check_hash(&archive, checksum, reporter, &label)?;
                    if outcome.matches() {
                        reporter.info(format!(
                            "[=] Skipping download of {name}, the cached archive matches its checksum"
                        ));
                        return Ok(archive);
                    }
                    reporter.warn(format!(
                        "[!] Cached archive {name} expected {} but hashes to {}, redownloading",
                        outcome.expected, outcome.computed
                    ));
                }
            }
        } else if archive.exists() {
            reporter.info(format!("[*] Forcing a fresh download of {name}"));
        } else {
            reporter.info(format!("[*] {name} is not cached yet, downloading"));
        }

        fs::create_dir_all(to)?;

        let mut attempt: u32 = 1;
        loop {
            reporter.checkpoint_async().await?;

            match self.fetch(&archive, reporter).await {
                Ok(()) => return Ok(archive),
                Err(LauncherError::Cancelled) => return Err(LauncherError::Cancelled),
                Err(LauncherError::Network(error)) if attempt < MAX_ATTEMPTS && is_retryable(&error) => {
                    reporter.warn(format!("[!] Transfer of {name} failed: {error}"));
                    let engaged = record_network_failure(&name, reporter);
                    attempt += 1;

                    if !engaged {
                        reporter.warn(format!(
                            "[!] Retrying in {}s (attempt {attempt}/{MAX_ATTEMPTS})",
                            RETRY_DELAY.as_secs()
                        ));
                        reporter.control().sleep_for(RETRY_DELAY).await?;
                    }
                }
                Err(error) => return Err(error),
            }
        }
    }

    async fn fetch(&self, archive: &Path, reporter: &Reporter) -> Result<()> {
        let response = send_once(http_client().get(&self.url), true).await?;

        let total = response.content_length();
        let id = self.url.clone();
        let label = format!("Downloading {}", file_name_of(archive));

        match total {
            Some(size) => reporter.info(format!(
                "[>] {label} ({}) from {}",
                human_bytes(size),
                self.url
            )),
            None => reporter.info(format!("[>] {label} (unknown size) from {}", self.url)),
        }

        reporter.task_started(&id, &label);
        let outcome = write_body(response, archive, &id, &label, total, reporter).await;
        reporter.task_finished(&id, outcome.as_ref().err().map(|error| error.to_string()));
        outcome
    }

    pub fn extract(&self, to: &Path) -> Result<()> {
        extract_archive(&self.archive()?, to)
    }
}

async fn write_body(
    response: Response,
    archive: &Path,
    id: &str,
    label: &str,
    total: Option<u64>,
    reporter: &Reporter,
) -> Result<()> {
    let partial = partial_path(archive);

    match stream_to_file(response, &partial, id, label, total, reporter).await {
        Ok(()) => {
            fs::rename(&partial, archive)?;
            Ok(())
        }
        Err(error) => {
            let _ = fs::remove_file(&partial);
            Err(error)
        }
    }
}

fn archive_size(path: &Path) -> u64 {
    fs::metadata(path).map(|meta| meta.len()).unwrap_or(0)
}

fn average_speed(downloaded: u64, started: Instant) -> Option<f64> {
    let elapsed = started.elapsed().as_secs_f64();
    if elapsed <= 0.0 {
        return None;
    }
    Some(downloaded as f64 / elapsed)
}

struct SpeedMeter {
    sampled_at: Instant,
    sampled_bytes: u64,
    smoothed: Option<f64>,
}

impl SpeedMeter {
    fn new() -> Self {
        Self {
            sampled_at: Instant::now(),
            sampled_bytes: 0,
            smoothed: None,
        }
    }

    fn sample(&mut self, downloaded: u64) -> Option<f64> {
        let elapsed = self.sampled_at.elapsed().as_secs_f64();
        if elapsed < SPEED_SAMPLE_WINDOW {
            return self.smoothed;
        }

        let transferred = downloaded.saturating_sub(self.sampled_bytes) as f64;
        let instant = transferred / elapsed;

        self.sampled_at = Instant::now();
        self.sampled_bytes = downloaded;
        self.smoothed = Some(match self.smoothed {
            Some(previous) => previous * (1.0 - SPEED_SMOOTHING) + instant * SPEED_SMOOTHING,
            None => instant,
        });

        self.smoothed
    }

    fn current(&self) -> Option<f64> {
        self.smoothed
    }
}

async fn stream_to_file(
    response: Response,
    partial: &Path,
    id: &str,
    label: &str,
    total: Option<u64>,
    reporter: &Reporter,
) -> Result<()> {
    let mut file = File::create(partial)?;
    let mut stream = response.bytes_stream();
    let mut downloaded: u64 = 0;
    let started = Instant::now();
    let mut meter = SpeedMeter::new();
    let mut last_report = Instant::now();
    let mut last_log = Instant::now();

    while let Some(chunk) = stream.next().await {
        reporter.checkpoint_async().await?;

        let chunk = chunk?;
        file.write_all(&chunk)?;
        downloaded += chunk.len() as u64;
        meter.sample(downloaded);

        if last_report.elapsed() >= PROGRESS_INTERVAL {
            reporter.progress_with_speed(id, label, downloaded, total, meter.current());
            last_report = Instant::now();
        }

        if last_log.elapsed() >= LOG_INTERVAL {
            let speed = meter.current().unwrap_or(0.0);
            match total {
                Some(size) if size > 0 => reporter.info(format!(
                    "    {label}: {} / {} ({:.1}%) at {}",
                    human_bytes(downloaded),
                    human_bytes(size),
                    downloaded as f64 * 100.0 / size as f64,
                    human_speed(speed)
                )),
                _ => reporter.info(format!(
                    "    {label}: {} at {}",
                    human_bytes(downloaded),
                    human_speed(speed)
                )),
            }
            last_log = Instant::now();
        }
    }

    file.flush()?;
    let average = average_speed(downloaded, started);
    reporter.progress_with_speed(id, label, downloaded, total, average);
    reporter.info(format!(
        "[+] {label} finished: {} in {:.1}s (average {})",
        human_bytes(downloaded),
        started.elapsed().as_secs_f64(),
        human_speed(average.unwrap_or(0.0))
    ));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn captures_repository_url() {
        let captured = github_capture("https://github.com/Grokitach/gamma_setup").unwrap();
        assert_eq!(captured.0, "Grokitach");
        assert_eq!(captured.1, "gamma_setup");
        assert_eq!(captured.2, None);
    }

    #[test]
    fn captures_archive_revision() {
        let captured =
            github_capture("https://github.com/Grokitach/Stalker_GAMMA/archive/abc123.zip").unwrap();
        assert_eq!(captured.1, "Stalker_GAMMA");
        assert_eq!(captured.2, Some("abc123".to_string()));
    }

    #[test]
    fn ignores_branch_archive_revision() {
        let captured = github_capture(
            "https://github.com/Grokitach/gamma_loading_screens/archive/refs/heads/main.zip",
        )
        .unwrap();
        assert_eq!(captured.2, None);
    }

    #[test]
    fn rejects_foreign_hosts() {
        assert!(github_capture("https://example.com/Grokitach/gamma_setup").is_none());
    }

    #[test]
    fn basename_ignores_query_string() {
        assert_eq!(
            url_basename("https://example.com/files/archive.7z?token=1"),
            "archive.7z"
        );
    }

    #[test]
    fn names_github_archives_after_project() {
        let mut downloader = DefaultDownloader::new(
            "https://github.com/ModOrganizer2/modorganizer/releases/download/v2.5.2/Mod.Organizer-2.5.2.7z",
        );
        downloader.set_archive_name(Path::new("/tmp")).unwrap();
        assert_eq!(
            downloader.archive().unwrap(),
            PathBuf::from("/tmp/modorganizer-Mod.Organizer-2.5.2.7z")
        );
    }

    #[test]
    fn prefers_user_wanted_name() {
        let mut downloader = DefaultDownloader::with_options(
            "https://example.com/a/b.zip",
            Some("custom.zip".to_string()),
            None,
        );
        downloader.set_archive_name(Path::new("/tmp")).unwrap();
        assert_eq!(downloader.archive().unwrap(), PathBuf::from("/tmp/custom.zip"));
    }

    #[test]
    fn archive_is_unavailable_before_naming() {
        let downloader = DefaultDownloader::new("https://example.com/a.zip");
        assert!(downloader.archive().is_err());
    }
}
