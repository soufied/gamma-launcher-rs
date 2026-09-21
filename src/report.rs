use crate::error::Result;
use crate::runner::{JobControl, JobHandle};
use chrono::{DateTime, Local};
use std::sync::mpsc::Sender;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum LogLevel {
    Info,
    Warn,
    Error,
}

#[derive(Debug, Clone)]
pub struct LogEntry {
    pub level: LogLevel,
    pub message: String,
    pub timestamp: DateTime<Local>,
}

#[derive(Debug, Clone)]
pub struct DownloadProgress {
    pub id: String,
    pub label: String,
    pub downloaded: u64,
    pub total: Option<u64>,
    pub speed_bps: Option<f64>,
}

#[derive(Debug, Clone)]
pub enum TaskEvent {
    Log(LogEntry),
    Progress(DownloadProgress),
    TaskStarted { id: String, label: String },
    TaskFinished { id: String, error: Option<String> },
    OverallProgress { current: usize, total: usize, label: String },
    JobFinished { success: bool },
    ProcessStateChanged(Option<String>),
}

#[derive(Clone)]
pub struct Reporter {
    tx: Sender<TaskEvent>,
    control: JobHandle,
}

impl Reporter {
    pub fn new(tx: Sender<TaskEvent>, control: JobHandle) -> Self {
        Self { tx, control }
    }

    pub fn detached(tx: Sender<TaskEvent>) -> Self {
        Self {
            tx,
            control: JobControl::idle(),
        }
    }

    pub fn control(&self) -> &JobHandle {
        &self.control
    }

    pub fn checkpoint(&self) -> Result<()> {
        self.control.checkpoint()
    }

    pub async fn checkpoint_async(&self) -> Result<()> {
        self.control.checkpoint_async().await
    }

    fn log(&self, level: LogLevel, message: impl Into<String>) {
        let entry = LogEntry {
            level,
            message: message.into(),
            timestamp: Local::now(),
        };
        let _ = self.tx.send(TaskEvent::Log(entry));
    }

    pub fn info(&self, message: impl Into<String>) {
        self.log(LogLevel::Info, message);
    }

    pub fn warn(&self, message: impl Into<String>) {
        self.log(LogLevel::Warn, message);
    }

    pub fn error(&self, message: impl Into<String>) {
        self.log(LogLevel::Error, message);
    }

    pub fn progress(&self, id: &str, label: &str, downloaded: u64, total: Option<u64>) {
        self.progress_with_speed(id, label, downloaded, total, None);
    }

    pub fn progress_with_speed(
        &self,
        id: &str,
        label: &str,
        downloaded: u64,
        total: Option<u64>,
        speed_bps: Option<f64>,
    ) {
        let _ = self.tx.send(TaskEvent::Progress(DownloadProgress {
            id: id.to_string(),
            label: label.to_string(),
            downloaded,
            total,
            speed_bps,
        }));
    }

    pub fn task_started(&self, id: &str, label: &str) {
        let _ = self.tx.send(TaskEvent::TaskStarted {
            id: id.to_string(),
            label: label.to_string(),
        });
    }

    pub fn task_finished(&self, id: &str, error: Option<String>) {
        let _ = self.tx.send(TaskEvent::TaskFinished {
            id: id.to_string(),
            error,
        });
    }

    pub fn overall_progress(&self, current: usize, total: usize, label: &str) {
        let _ = self.tx.send(TaskEvent::OverallProgress {
            current,
            total,
            label: label.to_string(),
        });
    }

    pub fn job_finished(&self, success: bool) {
        let _ = self.tx.send(TaskEvent::JobFinished { success });
    }

    pub fn process_state_changed(&self, label: Option<String>) {
        let _ = self.tx.send(TaskEvent::ProcessStateChanged(label));
    }
}

pub fn human_bytes(value: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];

    let mut size = value as f64;
    let mut unit = 0;

    while size >= 1024.0 && unit + 1 < UNITS.len() {
        size /= 1024.0;
        unit += 1;
    }

    if unit == 0 {
        format!("{value} {}", UNITS[0])
    } else {
        format!("{size:.1} {}", UNITS[unit])
    }
}

pub fn human_speed(bytes_per_second: f64) -> String {
    if !bytes_per_second.is_finite() || bytes_per_second <= 0.0 {
        return "-- MB/s".to_string();
    }
    format!("{:.2} MB/s", bytes_per_second / 1_000_000.0)
}
