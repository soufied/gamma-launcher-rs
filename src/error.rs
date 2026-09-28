use std::path::PathBuf;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum LauncherError {
    #[error("network request failed: {0}")]
    Network(#[from] reqwest::Error),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("hash mismatch for {file}: expected {expected}, got {actual}")]
    HashMismatch {
        file: PathBuf,
        expected: String,
        actual: String,
    },
    #[error("archive {0} has an unrecognized or unsupported format")]
    UnknownArchiveFormat(PathBuf),
    #[error("failed to extract archive {archive}: {message}")]
    ExtractionFailed { archive: PathBuf, message: String },
    #[error("ModDB request for {url} failed: {message}")]
    ModDbParse { url: String, message: String },
    #[error("mod definition is missing required metadata: {0}")]
    MissingModMetadata(String),
    #[error("game runner configuration is invalid: {0}")]
    RunnerConfig(String),
    #[error("command '{command}' failed: {message}")]
    CommandFailed { command: String, message: String },
    #[error("the operation was cancelled by the user")]
    Cancelled,
    #[error("Steam Spacewar integration failed: {0}")]
    SteamIdentity(String),
    #[error("system tray service failed: {0}")]
    Tray(String),
    #[error("process discovery/termination failed: {0}")]
    ProcessControl(String),
    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, LauncherError>;
