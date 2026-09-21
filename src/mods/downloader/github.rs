use crate::error::{LauncherError, Result};
use crate::fsutil::copy_tree;
use crate::mods::downloader::base::{github_capture, request_with_failover, DefaultDownloader};
use crate::report::Reporter;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Git,
    Http,
}

fn git_available() -> bool {
    static AVAILABLE: OnceLock<bool> = OnceLock::new();
    *AVAILABLE.get_or_init(|| {
        Command::new("git")
            .arg("--version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    })
}

fn run_git(args: &[&str], cwd: Option<&Path>) -> Result<()> {
    let mut cmd = Command::new("git");
    cmd.args(args);
    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }

    let status = cmd.status().map_err(|e| LauncherError::CommandFailed {
        command: "git".to_string(),
        message: e.to_string(),
    })?;

    if !status.success() {
        return Err(LauncherError::CommandFailed {
            command: "git".to_string(),
            message: format!("git {} exited with {status}", args.join(" ")),
        });
    }

    Ok(())
}

fn run_git_output(args: &[&str], cwd: Option<&Path>) -> Result<String> {
    let mut cmd = Command::new("git");
    cmd.args(args);
    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }

    let output = cmd.output().map_err(|e| LauncherError::CommandFailed {
        command: "git".to_string(),
        message: e.to_string(),
    })?;

    if !output.status.success() {
        return Err(LauncherError::CommandFailed {
            command: "git".to_string(),
            message: String::from_utf8_lossy(&output.stderr).to_string(),
        });
    }

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

#[derive(Debug, Clone)]
pub struct GithubDownloader {
    inner: DefaultDownloader,
    revision: Option<String>,
    user: Option<String>,
    project: Option<String>,
    mode: Option<Mode>,
}

impl GithubDownloader {
    pub fn new(url: impl Into<String>) -> Self {
        Self {
            inner: DefaultDownloader::new(url),
            revision: None,
            user: None,
            project: None,
            mode: None,
        }
    }

    pub fn url(&self) -> &str {
        self.inner.url()
    }

    pub fn archive(&self) -> Result<PathBuf> {
        self.inner.archive()
    }

    pub async fn check(&mut self, _to: &Path, _update_cache: bool, _reporter: &Reporter) -> Result<()> {
        Ok(())
    }

    pub async fn download(&mut self, to: &Path, use_cached: bool, reporter: &Reporter) -> Result<PathBuf> {
        reporter.checkpoint_async().await?;

        if git_available() {
            self.mode = Some(Mode::Git);
            self.download_git(to, reporter)
        } else {
            self.mode = Some(Mode::Http);
            self.download_http(to, use_cached, reporter).await
        }
    }

    fn download_git(&mut self, to: &Path, reporter: &Reporter) -> Result<PathBuf> {
        let (user, project, revision) = github_capture(self.inner.url()).ok_or_else(|| {
            LauncherError::Other(format!("{} is not a valid github.com URL", self.inner.url()))
        })?;

        let archive = to.join(format!("{project}.git"));
        self.inner.set_archive(archive.clone());
        self.revision = Some(revision.unwrap_or_else(|| format!("{user}/main")));
        self.user = Some(user.clone());
        self.project = Some(project.clone());

        if !archive.is_dir() {
            std::fs::create_dir_all(&archive)?;
            run_git(&["init", "--bare"], Some(&archive))?;
        }

        let remote_url = format!("https://github.com/{user}/{project}");
        let remotes = run_git_output(&["remote"], Some(&archive)).unwrap_or_default();
        if !remotes.lines().any(|l| l.trim() == user) {
            run_git(&["remote", "add", &user, &remote_url], Some(&archive))?;
        }

        let label = format!("Fetching remote {user} from {project}");
        reporter.task_started(self.inner.url(), &label);
        let result = run_git(&["fetch", &user], Some(&archive));
        reporter.task_finished(self.inner.url(), result.as_ref().err().map(|e| e.to_string()));
        result?;

        Ok(archive)
    }

    async fn download_http(&mut self, to: &Path, use_cached: bool, reporter: &Reporter) -> Result<PathBuf> {
        let (user, project, _) = github_capture(self.inner.url()).ok_or_else(|| {
            LauncherError::Other(format!("{} is not a valid github.com URL", self.inner.url()))
        })?;

        if self.inner.url().contains("release") || self.inner.url().ends_with(".zip") {
            let name = self.inner.url().rsplit('/').next().unwrap_or_default();
            let revision = name.split('.').next().unwrap_or_default().to_string();
            self.revision = Some(revision.clone());
            self.inner.set_archive(to.join(format!("{project}-{revision}.zip")));
            return self.inner.download(to, use_cached, None, reporter).await;
        }

        let repo_info: serde_json::Value = request_with_failover(
            &format!("https://api.github.com/repos/{user}/{project}"),
            Some("application/json"),
            true,
            reporter,
        )
        .await?
        .json()
        .await?;
        let branch = repo_info
            .get("default_branch")
            .and_then(|v| v.as_str())
            .unwrap_or("main")
            .to_string();

        let branch_info: serde_json::Value = request_with_failover(
            &format!("https://api.github.com/repos/{user}/{project}/branches/{branch}"),
            Some("application/json"),
            true,
            reporter,
        )
        .await?
        .json()
        .await?;
        let sha = branch_info
            .get("commit")
            .and_then(|c| c.get("sha"))
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();

        self.revision = Some(sha.clone());
        self.inner
            .set_url(format!("https://github.com/{user}/{project}/archive/refs/heads/{branch}.zip"));
        self.inner.set_archive(to.join(format!("{project}-{sha}.zip")));

        self.inner.download(to, use_cached, None, reporter).await
    }

    pub fn extract(&self, to: &Path) -> Result<()> {
        match self.mode {
            Some(Mode::Git) => self.extract_git(to),
            _ => self.extract_http(to),
        }
    }

    fn extract_git(&self, to: &Path) -> Result<()> {
        let archive = self.inner.archive()?;
        let revision = self
            .revision
            .clone()
            .ok_or_else(|| LauncherError::Other("no revision set, call download() first".to_string()))?;

        let tmp = tempfile::Builder::new()
            .prefix("gamma-launcher-github-extract-")
            .tempdir()?;

        run_git(
            &["worktree", "add", "--detach", &tmp.path().to_string_lossy(), &revision],
            Some(&archive),
        )?;

        if tmp.path() != to {
            copy_tree(tmp.path(), to)?;
        }

        run_git(&["worktree", "prune"], Some(&archive))?;
        Ok(())
    }

    fn extract_http(&self, to: &Path) -> Result<()> {
        let tmp = tempfile::Builder::new()
            .prefix("gamma-launcher-github-extract-")
            .tempdir()?;

        crate::archive::extract_archive(&self.inner.archive()?, tmp.path())?;

        let mut entries: Vec<PathBuf> = std::fs::read_dir(tmp.path())?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .collect();

        let source = if entries.len() == 1 {
            entries.remove(0)
        } else {
            tmp.path().to_path_buf()
        };

        copy_tree(&source, to)
    }

    pub fn revision(&self) -> Option<String> {
        let archive = self.inner.archive().ok()?;
        match self.mode {
            Some(Mode::Git) => {
                let revision = self.revision.as_ref()?;
                let output = Command::new("git")
                    .arg("-C")
                    .arg(&archive)
                    .arg("rev-parse")
                    .arg(revision)
                    .output()
                    .ok()?;
                if !output.status.success() {
                    return None;
                }
                Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
            }
            _ => self.revision.clone(),
        }
    }
}
