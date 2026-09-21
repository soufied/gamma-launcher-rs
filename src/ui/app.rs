use crate::commands::{estimate_reclaimable, KeymapLayout, SpaceEstimate};
use crate::config::{detected_cpu_count, expand_path, AppConfig};
use crate::detect::{self, DetectionResult};
use crate::mods::downloader::base::fallback_is_active;
use crate::report::{human_bytes, human_speed, LogEntry, LogLevel, Reporter, TaskEvent};
use crate::runner::{self, ControlState, JobControl, JobHandle, LaunchTarget, ProcessSnapshot, ProcessState};
use crate::ui::job::{self, Job};
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::thread;
use std::time::Duration;

const MAX_LOG_ENTRIES: usize = 4000;
const MAX_VISIBLE_TASKS: usize = 6;
const ACTION_SIZE: [f32; 2] = [180.0_f32, 46.0_f32];
const SECONDARY_ACTION_SIZE: [f32; 2] = [188.0_f32, 38.0_f32];
const BADGE_ROUNDING: f32 = 8.0_f32;
const SECTION_ROUNDING: f32 = 10.0_f32;
const WIDGET_ROUNDING: f32 = 6.0_f32;
const SECTION_MARGIN: f32 = 16.0_f32;
const ROW_SPACING: f32 = 12.0_f32;
const CARD_ROUNDING: f32 = 6.0_f32;
const CARD_MARGIN: f32 = 12.0_f32;
const CARD_BUTTON_HEIGHT: f32 = 34.0_f32;
const WIDE_LAYOUT_WIDTH: f32 = 1000.0_f32;
const MEDIUM_LAYOUT_WIDTH: f32 = 660.0_f32;

const LOCK_HINT: &str = "Locked while a job or an external process is running. Every control re-enables itself automatically as soon as that process exits.";

fn accent_color() -> egui::Color32 {
    egui::Color32::from_rgb(30, 144, 255)
}

fn color_text_bright() -> egui::Color32 {
    egui::Color32::from_rgb(240, 244, 250)
}

fn color_text_normal() -> egui::Color32 {
    egui::Color32::from_rgb(220, 225, 230)
}

fn color_track() -> egui::Color32 {
    egui::Color32::from_rgb(32, 36, 44)
}

fn color_fill() -> egui::Color32 {
    egui::Color32::from_rgb(30, 144, 255)
}

fn color_fill_alternate() -> egui::Color32 {
    egui::Color32::from_rgb(0, 168, 204)
}

fn color_warning() -> egui::Color32 {
    egui::Color32::from_rgb(240, 180, 40)
}

fn color_danger() -> egui::Color32 {
    egui::Color32::from_rgb(235, 75, 75)
}

fn color_mo2() -> egui::Color32 {
    egui::Color32::from_rgb(61, 174, 233)
}

fn color_launcher() -> egui::Color32 {
    egui::Color32::from_rgb(233, 154, 62)
}

fn color_game() -> egui::Color32 {
    egui::Color32::from_rgb(90, 190, 130)
}

fn color_detect() -> egui::Color32 {
    egui::Color32::from_rgb(120, 130, 226)
}

fn color_ok() -> egui::Color32 {
    egui::Color32::from_rgb(96, 200, 128)
}

fn color_missing() -> egui::Color32 {
    color_danger()
}

fn color_pending() -> egui::Color32 {
    color_warning()
}

fn color_running() -> egui::Color32 {
    color_fill_alternate()
}

#[derive(Debug, Clone)]
struct TaskProgress {
    id: String,
    label: String,
    downloaded: u64,
    total: Option<u64>,
    speed_bps: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tab {
    Dashboard,
    Paths,
    Tweaks,
    Console,
}

#[derive(Debug, Clone, Copy)]
enum PathCheck {
    Directory,
    DirectoryAutoCreate,
    File,
    OptionalDirectory,
}

#[derive(Debug, Clone, Copy, Default)]
struct ToggleOutcome {
    changed: bool,
    reset: bool,
}

impl ToggleOutcome {
    fn dirty(&self) -> bool {
        self.changed || self.reset
    }
}

pub struct LauncherApp {
    config: AppConfig,
    config_location: String,
    active_tab: Tab,
    anomaly_input: String,
    gamma_input: String,
    prefix_input: String,
    proton_input: String,
    cache_input: String,
    final_input: String,
    mo2_input: String,
    launcher_input: String,
    game_input: String,
    repository_input: String,
    revision_input: String,
    umu_id_input: String,
    layout: KeymapLayout,
    logs: VecDeque<LogEntry>,
    tasks: Vec<TaskProgress>,
    overall: Option<(usize, usize, String)>,
    events: Option<Receiver<TaskEvent>>,
    detection: Option<Receiver<DetectionResult>>,
    background_tx: Sender<TaskEvent>,
    background_rx: Receiver<TaskEvent>,
    process_state: ProcessState,
    control: JobHandle,
    confirm_cancel: bool,
    process: Option<ProcessSnapshot>,
    space: Option<SpaceEstimate>,
    space_scan: Option<Receiver<SpaceEstimate>>,
    pending_job: Option<Job>,
    pending_save: bool,
    pending_detect: bool,
    pending_space_scan: bool,
    scanning_space: bool,
    detecting: bool,
    busy: bool,
    status: String,
}

impl LauncherApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let config = AppConfig::load();
        let (background_tx, background_rx) = mpsc::channel();

        apply_theme(&cc.egui_ctx, config.dark_mode);

        let config_location = AppConfig::config_path()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|_| "config.toml".to_string());

        Self {
            config_location,
            active_tab: Tab::Dashboard,
            anomaly_input: path_to_string(&config.anomaly_path),
            gamma_input: path_to_string(&config.gamma_path),
            prefix_input: path_to_string(&config.runner.wine_prefix),
            proton_input: path_to_string(&config.runner.proton_path),
            cache_input: path_to_string(&config.cache_path),
            final_input: path_to_string(&config.usvfs_final_path),
            mo2_input: path_to_string(&config.runner.mo2_executable),
            launcher_input: path_to_string(&config.runner.launcher_executable),
            game_input: path_to_string(&config.runner.game_executable),
            repository_input: config.custom_gamma_repository.clone(),
            revision_input: config.custom_gamma_revision.clone().unwrap_or_default(),
            umu_id_input: config.runner.umu_id.clone(),
            layout: KeymapLayout::default(),
            logs: VecDeque::new(),
            tasks: Vec::new(),
            overall: None,
            events: None,
            detection: None,
            background_tx,
            background_rx,
            process_state: runner::new_process_state(),
            control: runner::new_job_control(),
            confirm_cancel: false,
            process: None,
            space: None,
            space_scan: None,
            pending_job: None,
            pending_save: false,
            pending_detect: false,
            pending_space_scan: true,
            scanning_space: false,
            detecting: false,
            busy: false,
            status: "Idle".to_string(),
            config,
        }
    }

    fn apply_inputs(&mut self) {
        self.config.anomaly_path = optional_path(&self.anomaly_input);
        self.config.gamma_path = optional_path(&self.gamma_input);
        self.config.cache_path = optional_path(&self.cache_input);
        self.config.usvfs_final_path = optional_path(&self.final_input);
        self.config.runner.wine_prefix = optional_path(&self.prefix_input);
        self.config.runner.proton_path = optional_path(&self.proton_input);
        self.config.runner.mo2_executable = optional_path(&self.mo2_input);
        self.config.runner.launcher_executable = optional_path(&self.launcher_input);
        self.config.runner.game_executable = optional_path(&self.game_input);
        self.config.custom_gamma_repository = self.repository_input.trim().to_string();
        self.config.custom_gamma_revision = optional_text(&self.revision_input);
        self.config.runner.umu_id = self.umu_id_input.trim().to_string();
        self.config.proxy.host = self.config.proxy.host.trim().to_string();
    }

    fn save_config(&mut self) {
        self.apply_inputs();

        if let Err(error) = self.config.save() {
            self.push_log(LogLevel::Error, format!("Could not save settings: {error}"));
        } else {
            self.push_log(
                LogLevel::Info,
                format!("Settings saved to {}", self.config_location),
            );
        }
    }

    fn start_auto_detect(&mut self, ctx: &egui::Context) {
        if self.detecting {
            return;
        }

        self.apply_inputs();
        self.push_log(
            LogLevel::Info,
            "[*] Ranking every ModOrganizer.exe, AnomalyLauncher.exe, AnomalyDX*.exe and Proton build that can be found",
        );

        let config = self.config.clone();
        let (sender, receiver) = mpsc::channel();
        let repaint = ctx.clone();

        thread::spawn(move || {
            let _ = sender.send(detect::autodetect(&config));
            repaint.request_repaint();
        });

        self.detection = Some(receiver);
        self.detecting = true;
        self.status = "Scanning for installations".to_string();
    }

    fn start_space_scan(&mut self, ctx: &egui::Context) {
        if self.scanning_space {
            return;
        }

        self.apply_inputs();

        let config = self.config.clone();
        let (sender, receiver) = mpsc::channel();
        let repaint = ctx.clone();

        thread::spawn(move || {
            let _ = sender.send(estimate_reclaimable(&config));
            repaint.request_repaint();
        });

        self.space_scan = Some(receiver);
        self.scanning_space = true;
    }

    fn poll_space_scan(&mut self) {
        let receiver = match self.space_scan.take() {
            Some(receiver) => receiver,
            None => return,
        };

        match receiver.try_recv() {
            Ok(estimate) => {
                self.space = Some(estimate);
                self.scanning_space = false;
            }
            Err(TryRecvError::Empty) => self.space_scan = Some(receiver),
            Err(TryRecvError::Disconnected) => self.scanning_space = false,
        }
    }

    fn poll_detection(&mut self) {
        let receiver = match self.detection.take() {
            Some(receiver) => receiver,
            None => return,
        };

        match receiver.try_recv() {
            Ok(result) => {
                self.apply_detection(result);
                self.detecting = false;
                if !self.busy {
                    self.status = "Idle".to_string();
                }
            }
            Err(TryRecvError::Empty) => self.detection = Some(receiver),
            Err(TryRecvError::Disconnected) => {
                self.detecting = false;
                self.push_log(
                    LogLevel::Error,
                    "Auto-detection stopped before it could report a result",
                );
                if !self.busy {
                    self.status = "Idle".to_string();
                }
            }
        }
    }

    fn apply_detection(&mut self, result: DetectionResult) {
        for message in &result.messages {
            let level = if message.starts_with("[!]") {
                LogLevel::Warn
            } else {
                LogLevel::Info
            };
            self.push_log(level, message.clone());
        }

        if let Some(path) = result.mo2_executable {
            self.mo2_input = path.display().to_string();
            self.config.runner.mo2_executable = Some(path);
        }
        if let Some(path) = result.launcher_executable {
            self.launcher_input = path.display().to_string();
            self.config.runner.launcher_executable = Some(path);
        }
        if let Some(path) = result.game_executable {
            self.game_input = path.display().to_string();
            self.config.runner.game_executable = Some(path);
        }
        if let Some(path) = result.gamma_path {
            self.gamma_input = path.display().to_string();
            self.config.gamma_path = Some(path);
        }
        if let Some(path) = result.anomaly_path {
            self.anomaly_input = path.display().to_string();
            self.config.anomaly_path = Some(path);
        }
        if let Some(path) = result.proton_path {
            self.proton_input = path.display().to_string();
            self.config.runner.proton_path = Some(path);
        }

        self.push_log(LogLevel::Info, "[+] Auto-detection finished");
        self.pending_save = true;
    }

    fn push_log(&mut self, level: LogLevel, message: impl Into<String>) {
        self.logs.push_back(LogEntry {
            level,
            message: message.into(),
            timestamp: chrono::Local::now(),
        });

        while self.logs.len() > MAX_LOG_ENTRIES {
            self.logs.pop_front();
        }
    }

    fn start(&mut self, job: Job, ctx: &egui::Context) {
        if self.busy {
            return;
        }

        if let Some(active) = self.process.clone() {
            if !matches!(job, Job::KillProcesses) {
                self.push_log(
                    LogLevel::Warn,
                    format!(
                        "{} is still running under PID {}, wait for it to exit before starting a new job",
                        active.label, active.pid
                    ),
                );
                return;
            }
        }

        self.apply_inputs();
        let _ = self.config.save();

        self.tasks.clear();
        self.overall = None;
        self.busy = true;
        self.confirm_cancel = false;
        self.status = job.label();
        self.control = JobControl::running();
        self.events = Some(job::spawn(
            job,
            self.config.clone(),
            self.process_state.clone(),
            Reporter::detached(self.background_tx.clone()),
            self.control.clone(),
            ctx.clone(),
        ));
    }

    fn drain_events(&mut self) {
        let mut pending = Vec::new();
        while let Ok(event) = self.background_rx.try_recv() {
            pending.push(event);
        }

        let mut background_alive = true;
        for event in pending {
            self.handle_event(event, &mut background_alive);
        }

        let receiver = match self.events.take() {
            Some(receiver) => receiver,
            None => return,
        };

        let mut running = true;
        loop {
            match receiver.try_recv() {
                Ok(event) => self.handle_event(event, &mut running),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    running = false;
                    break;
                }
            }
        }

        if running {
            self.events = Some(receiver);
        } else {
            self.busy = false;
            self.tasks.clear();
        }
    }

    fn handle_event(&mut self, event: TaskEvent, running: &mut bool) {
        match event {
            TaskEvent::Log(entry) => {
                self.logs.push_back(entry);
                while self.logs.len() > MAX_LOG_ENTRIES {
                    self.logs.pop_front();
                }
            }
            TaskEvent::Progress(progress) => {
                match self.tasks.iter_mut().find(|task| task.id == progress.id) {
                    Some(task) => {
                        task.label = progress.label;
                        task.downloaded = progress.downloaded;
                        task.total = progress.total;
                        task.speed_bps = progress.speed_bps;
                    }
                    None => self.tasks.push(TaskProgress {
                        id: progress.id,
                        label: progress.label,
                        downloaded: progress.downloaded,
                        total: progress.total,
                        speed_bps: progress.speed_bps,
                    }),
                }
            }
            TaskEvent::TaskStarted { id, label } => {
                if !self.tasks.iter().any(|task| task.id == id) {
                    self.tasks.push(TaskProgress {
                        id,
                        label,
                        downloaded: 0,
                        total: None,
                        speed_bps: None,
                    });
                }
            }
            TaskEvent::TaskFinished { id, error } => {
                self.tasks.retain(|task| task.id != id);
                if let Some(message) = error {
                    self.push_log(LogLevel::Error, message);
                }
            }
            TaskEvent::OverallProgress {
                current,
                total,
                label,
            } => {
                self.overall = Some((current, total, label));
            }
            TaskEvent::JobFinished { success } => {
                *running = false;
                self.pending_space_scan = true;
                self.confirm_cancel = false;
                self.status = if self.control.is_cancelled() {
                    "Cancelled".to_string()
                } else if success {
                    "Completed".to_string()
                } else {
                    "Failed".to_string()
                };
            }
            TaskEvent::ProcessStateChanged(label) => {
                self.process = self.process_state.snapshot();
                match label {
                    Some(label) => self.status = format!("{label} is running"),
                    None => {
                        if !self.busy {
                            self.status = "Idle".to_string();
                        }
                    }
                }
            }
        }
    }

    fn draw_header(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("header").show(ctx, |ui| {
            ui.add_space(10.0_f32);
            ui.horizontal(|ui| {
                ui.heading("S.T.A.L.K.E.R. G.A.M.M.A. Launcher");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if self.busy || self.detecting {
                        ui.spinner();
                    }
                    ui.label(egui::RichText::new(self.status.as_str()).weak())
                        .on_hover_text("Current state of the background queue: Idle, the running job's name, Scanning, Completed or Failed.");
                    ui.add_space(14.0_f32);
                    if ui
                        .add_enabled(!self.busy, egui::Button::new("Save settings"))
                        .on_hover_text(format!(
                            "Writes every field on this screen to {}, next to the launcher executable, so the same setup is restored the next time it starts. Settings are also saved automatically before every job.",
                            self.config_location
                        ))
                        .on_disabled_hover_text(LOCK_HINT)
                        .clicked()
                    {
                        self.pending_save = true;
                    }
                    if ui
                        .checkbox(&mut self.config.dark_mode, "Dark theme")
                        .on_hover_text("Switches between a high-contrast dark theme tuned for CachyOS / KDE Plasma and the default light theme. Takes effect immediately and is remembered on save. Default: ON.")
                        .changed()
                    {
                        apply_theme(ctx, self.config.dark_mode);
                        self.pending_save = true;
                    }
                });
            });

            ui.add_space(10.0_f32);
            self.draw_tab_bar(ui);
            ui.add_space(8.0_f32);
            self.draw_process_badge(ui);
            ui.add_space(10.0_f32);
        });
    }

    fn draw_tab_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            tab_button(
                ui,
                &mut self.active_tab,
                Tab::Dashboard,
                "Dashboard",
                "Launch MO2, the Anomaly Launcher or the game directly, run maintenance jobs, and switch the in-game keymap layout.",
            );
            tab_button(
                ui,
                &mut self.active_tab,
                Tab::Paths,
                "Paths",
                "Point the launcher at your Anomaly install, GAMMA/MO2 folder, wine prefix, Proton build and executables, or let the ranked auto-detection find them for you.",
            );
            tab_button(
                ui,
                &mut self.active_tab,
                Tab::Tweaks,
                "Tweaks",
                "Fine-tune install behaviour, the Wine/Proton runtime, DXVK, GameMode, UMU and the SOCKS5 proxy used for downloads.",
            );
            tab_button(
                ui,
                &mut self.active_tab,
                Tab::Console,
                "Console Log",
                "Full scrolling output of every job: downloads, installs, verifications and game launches, each line timestamped.",
            );
        });
    }

    fn draw_process_badge(&mut self, ui: &mut egui::Ui) {
        let active = match self.process.clone() {
            Some(active) => active,
            None => return,
        };

        let time = ui.input(|input| input.time);
        let pulse = ((time * 2.4_f64).sin() * 0.5_f64 + 0.5_f64) as f32;
        let accent = color_running();

        egui::Frame::none()
            .fill(accent.linear_multiply(0.14_f32 + 0.12_f32 * pulse))
            .stroke(egui::Stroke::new(1.0_f32 + pulse, accent))
            .rounding(egui::Rounding::same(BADGE_ROUNDING))
            .inner_margin(egui::Margin::symmetric(14.0_f32, 8.0_f32))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(
                        egui::RichText::new(format!(
                            "Process Active: {} is running...",
                            active.label
                        ))
                        .color(accent)
                        .strong()
                        .size(15.0_f32),
                    );
                    ui.label(
                        egui::RichText::new(format!("PID {}", active.pid))
                            .monospace()
                            .weak(),
                    );
                })
                .response
                .on_hover_text("An external process started by this launcher is still alive. Every launch and maintenance action stays locked until it exits, so MO2, the launcher and the game cannot fight over the same wine prefix.");
            });
    }

    fn draw_launch_button(
        &mut self,
        ui: &mut egui::Ui,
        enabled: bool,
        target: LaunchTarget,
        label: &str,
        color: egui::Color32,
        tooltip: &str,
    ) {
        ui.vertical(|ui| {
            let clicked = ui
                .add_enabled_ui(enabled, |ui| {
                    ui.add_sized(
                        ACTION_SIZE,
                        egui::Button::new(
                            egui::RichText::new(label)
                                .color(egui::Color32::WHITE)
                                .strong()
                                .size(16.0_f32),
                        )
                        .fill(color),
                    )
                    .on_hover_text(tooltip)
                    .on_disabled_hover_text(LOCK_HINT)
                    .clicked()
                })
                .inner;

            if clicked {
                self.pending_job = Some(Job::Launch(target));
            }

            ui.add_space(6.0_f32);
            match runner::resolve_status(&self.config, target) {
                Some(path) => {
                    ui.horizontal(|ui| {
                        draw_badge(
                            ui,
                            true,
                            "[FOUND]",
                            "[MISSING]",
                            "This executable was located and is ready to launch.",
                        );
                        ui.label(egui::RichText::new(short_path(&path)).weak().small())
                            .on_hover_text(path.display().to_string());
                    });
                }
                None => {
                    draw_badge(
                        ui,
                        false,
                        "[FOUND]",
                        "[MISSING]",
                        "No executable could be resolved yet. Set it on the Paths tab or run Auto-Detect Paths.",
                    );
                }
            }
        });
    }

    fn draw_dashboard(&mut self, ui: &mut egui::Ui, locked: bool) {
        let enabled = !locked;

        ui.add_space(6.0_f32);
        section(ui, "Launch", |ui| {
            ui.horizontal_wrapped(|ui| {
                self.draw_launch_button(
                    ui,
                    enabled,
                    LaunchTarget::ModOrganizer,
                    "MO2",
                    color_mo2(),
                    "Starts Mod Organizer 2 through the configured Proton build. This is the normal way to launch GAMMA, since MO2 activates the virtual mod list before the game runs.",
                );
                self.draw_launch_button(
                    ui,
                    enabled,
                    LaunchTarget::Launcher,
                    "Launcher",
                    color_launcher(),
                    "Starts the vanilla AnomalyLauncher.exe directly, bypassing Mod Organizer 2. Use this only to verify Anomaly itself, since mods will not be active.",
                );
                self.draw_launch_button(
                    ui,
                    enabled,
                    LaunchTarget::Game,
                    "Game",
                    color_game(),
                    "Starts the Anomaly game binary directly, skipping both the launcher and Mod Organizer 2. Only use this if you already have a working MO2-deployed profile and know what you are doing.",
                );
            });
        });

        self.draw_pipeline_section(ui, enabled);
        self.draw_maintenance_section(ui, enabled);
    }

    fn draw_pipeline_section(&mut self, ui: &mut egui::Ui, enabled: bool) {
        section(ui, "Install & Sync", |ui| {
            ui.label(
                egui::RichText::new(
                    "Everything that downloads or installs content. These jobs need network access.",
                )
                .weak()
                .small(),
            );
            ui.add_space(10.0_f32);

            ui.horizontal_wrapped(|ui| {
                if sized_action(
                    ui,
                    enabled,
                    SECONDARY_ACTION_SIZE,
                    "▶  Sync / Update",
                    "Runs the full GAMMA pipeline: downloads, verifies and installs Anomaly and the modpack according to the Install options and Tweaks tabs. Run it for the first install and after every GAMMA definition update. Reversible only by reinstalling, but already valid mods are skipped.",
                ) {
                    self.pending_job = Some(Job::FullInstall);
                }
                if sized_action(
                    ui,
                    enabled,
                    SECONDARY_ACTION_SIZE,
                    "⬇  Anomaly Install",
                    "Downloads and installs a clean copy of S.T.A.L.K.E.R. Anomaly 1.5.3 into the configured Anomaly directory. Run it only when that folder is empty or broken. Irreversible: it overwrites the files it ships.",
                ) {
                    self.pending_job = Some(Job::AnomalyInstall);
                }
                if sized_action(
                    ui,
                    enabled,
                    SECONDARY_ACTION_SIZE,
                    "⬇  GAMMA Setup",
                    "Downloads the GAMMA base setup, installs Mod Organizer 2 when that option is enabled, and prepares downloads/ and mods/ next to it. Run it once before the first Sync / Update. Irreversible: it overwrites the setup files it ships.",
                ) {
                    self.pending_job = Some(Job::GammaSetup);
                }
                if sized_action(
                    ui,
                    enabled,
                    SECONDARY_ACTION_SIZE,
                    "🔍  Check MD5",
                    "Compares every downloaded mod archive against its expected checksum. Run it when installs fail in odd ways or after a disk problem. Read-only by itself, but the Install options tab can let it redownload or purge mismatching archives.",
                ) {
                    self.pending_job = Some(Job::CheckMd5);
                }
                if sized_action(
                    ui,
                    enabled,
                    SECONDARY_ACTION_SIZE,
                    "🔍  Test Mod Maker",
                    "Parses the modpack definition and reports malformed entries without touching your install. Run it when a mod refuses to install or before reporting a bug. Completely read-only.",
                ) {
                    self.pending_job = Some(Job::TestModMaker);
                }
            });
        });
    }

    fn draw_maintenance_section(&mut self, ui: &mut egui::Ui, enabled: bool) {
        section(ui, "Maintenance", |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(
                        "Grouped by what each action touches. Hover any button for what it does, when to run it and whether it can be undone.",
                    )
                    .weak()
                    .small(),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .add_enabled(
                            !self.scanning_space,
                            egui::Button::new("🔄  Refresh estimates"),
                        )
                        .on_hover_text("Measures the downloads folder, the interrupted transfers and the leftover staging directories again so the reclaimable space shown below is up to date. Read-only.")
                        .on_disabled_hover_text("A measurement is already running.")
                        .clicked()
                    {
                        self.pending_space_scan = true;
                    }
                    if self.scanning_space {
                        ui.spinner();
                    }
                });
            });

            ui.add_space(ROW_SPACING);

            let width = ui.available_width();

            if width >= WIDE_LAYOUT_WIDTH {
                ui.columns(3, |columns| {
                    self.draw_cache_card(&mut columns[0], enabled);
                    self.draw_diagnostics_card(&mut columns[1], enabled);
                    self.draw_mods_card(&mut columns[2], enabled);
                });
            } else if width >= MEDIUM_LAYOUT_WIDTH {
                ui.columns(2, |columns| {
                    self.draw_cache_card(&mut columns[0], enabled);
                    self.draw_diagnostics_card(&mut columns[1], enabled);
                });
                self.draw_mods_card(ui, enabled);
            } else {
                self.draw_cache_card(ui, enabled);
                self.draw_diagnostics_card(ui, enabled);
                self.draw_mods_card(ui, enabled);
            }
        });
    }

    fn draw_cache_card(&mut self, ui: &mut egui::Ui, enabled: bool) {
        let downloads = self.space.map(|space| space.downloads);
        let incomplete = self.space.map(|space| space.incomplete);
        let temp_cache = self.space.map(|space| space.temp_cache);

        maintenance_card(
            ui,
            "Cache & Downloads",
            "Disk space held by archives and staging folders",
            color_pending(),
            |ui| {
                if card_action(
                    ui,
                    enabled,
                    "🗑  Purge Downloads",
                    "Deletes every archive inside the downloads folder. Run it when the disk is full or when a whole batch of archives has to be fetched again. IRREVERSIBLE: the next Sync / Update has to redownload tens of gigabytes. If the downloads folder is a symlink to your download cache, the cache is what gets emptied.",
                    Some(color_missing()),
                ) {
                    self.pending_job = Some(Job::PurgeDownloads);
                }
                estimate_row(ui, "Archives stored", downloads);

                if card_action(
                    ui,
                    enabled,
                    "♻  Clear Temp Cache",
                    "Removes the gamma-launcher-* staging directories that a crashed or cancelled install left behind in TMPDIR. Run it after a failed extraction or when TMPDIR runs out of space. IRREVERSIBLE but harmless: these folders are scratch space only.",
                    Some(color_pending()),
                ) {
                    self.pending_job = Some(Job::ClearTempCache);
                }
                estimate_row(ui, "Staging leftovers", temp_cache);

                if card_action(
                    ui,
                    enabled,
                    "✂  Prune Incomplete Downloads",
                    "Deletes interrupted transfers (.part, .tmp, .crdownload) and zero-byte archives from the downloads folder. Run it after a connection drop or a proxy timeout. IRREVERSIBLE, but complete archives are never touched, so only the broken files are fetched again.",
                    Some(color_pending()),
                ) {
                    self.pending_job = Some(Job::PruneIncompleteDownloads);
                }
                estimate_row(ui, "Interrupted transfers", incomplete);

                if card_action(
                    ui,
                    enabled,
                    "⚡  Shader Cache Clean",
                    "Deletes the compiled shader cache in the Anomaly appdata folder. Run it after a driver, Proton or DXVK update, or when you see graphical corruption. REVERSIBLE: the cache rebuilds itself, so the first launch afterwards is slower.",
                    None,
                ) {
                    self.pending_job = Some(Job::PurgeShaderCache);
                }
            },
        );
    }

    fn draw_diagnostics_card(&mut self, ui: &mut egui::Ui, enabled: bool) {
        let running = self.process.is_some();
        let busy = self.busy;

        maintenance_card(
            ui,
            "Game & Prefix Diagnostics",
            "Emergency stop and Wine runtime recovery",
            color_missing(),
            |ui| {
                if card_action(
                    ui,
                    !busy,
                    "■  Kill Stalker & MO2 Processes",
                    "Sends SIGTERM, then SIGKILL after a few seconds, to every Mod Organizer 2, AnomalyLauncher, AnomalyDX, xrEngine, usvfs_proxy, umu-run and wineserver process. Run it when the game is frozen, a window is invisible or the launcher still reports a running process. IRREVERSIBLE: unsaved progress in the running game is lost. This control stays available even while a process is active, on purpose.",
                    Some(color_missing()),
                ) {
                    self.pending_job = Some(Job::KillProcesses);
                }

                if running {
                    ui.label(
                        egui::RichText::new("A tracked process is alive right now")
                            .color(color_running())
                            .small(),
                    )
                    .on_hover_text("The pulsing badge at the top of the window names it. Terminating it re-enables every other control.");
                    ui.add_space(6.0_f32);
                }

                if card_action(
                    ui,
                    enabled,
                    "🔄  Reset Wine Prefix",
                    "Moves the configured wine prefix aside to a timestamped .bak- folder so Proton recreates it from scratch on the next launch. Run it when the prefix is corrupted, after a Proton downgrade, or when USVFS hooks stop working. REVERSIBLE: nothing is deleted, the old prefix is kept next to the new one until you remove it yourself. In-prefix data such as Windows-side settings stays in the backup.",
                    Some(color_pending()),
                ) {
                    self.pending_job = Some(Job::ResetWinePrefix);
                }

                if card_action(
                    ui,
                    enabled,
                    "✔  Verify Anomaly Integrity",
                    "Hashes every file of the Anomaly install against tools/checksums.md5 and reports the ones that differ. Run it after a crash during install, a bad download or a disk error. Completely READ-ONLY: it changes nothing and only reports.",
                    None,
                ) {
                    self.pending_job = Some(Job::CheckAnomaly);
                }

                if card_action(
                    ui,
                    enabled,
                    "⚙  Reset DXVK / D3D Overrides",
                    "Restores WINEDLLOVERRIDES and DXVK_CONFIG to the defaults this launcher ships and removes stale pipeline and shader caches from the Anomaly folder and the prefix. Run it after editing those fields by hand or when DXVK starts crashing. REVERSIBLE: the values are just settings you can edit again on the Tweaks tab.",
                    None,
                ) {
                    self.config.runner.reset_dll_overrides();
                    self.config.runner.reset_dxvk_config();
                    self.push_log(
                        LogLevel::Info,
                        "[+] WINEDLLOVERRIDES and DXVK_CONFIG restored to their defaults",
                    );
                    self.pending_job = Some(Job::ResetGraphicsState);
                }

                if card_action(
                    ui,
                    enabled,
                    "✖  Remove ReShade",
                    "Strips the ReShade injector files and shader folder from the Anomaly bin directory, then clears the shader cache. Run it when ReShade crashes the game under Proton or after switching to a different injector. IRREVERSIBLE: the ReShade preset files in bin/ are deleted.",
                    Some(color_pending()),
                ) {
                    self.pending_job = Some(Job::RemoveReshade);
                }
            },
        );
    }

    fn draw_mods_card(&mut self, ui: &mut egui::Ui, enabled: bool) {
        let layout = self.layout;

        maintenance_card(
            ui,
            "Mods & Configuration",
            "Mod index, Mod Organizer 2 settings and file modes",
            color_detect(),
            |ui| {
                if card_action(
                    ui,
                    enabled,
                    "🔧  Rebuild Mod Cache",
                    "Re-reads the GAMMA definition, then walks the mods folder to report how many mods are installed, which ones lack a meta.ini, which ones are no longer part of the definition and which ones are missing. Empty leftover folders are removed. Run it when Sync / Update skips or reinstalls mods unexpectedly. SAFE: nothing that contains files is deleted.",
                    None,
                ) {
                    self.pending_job = Some(Job::RebuildModCache);
                }

                if card_action(
                    ui,
                    enabled,
                    "⇄  Sync ModOrganizer.ini",
                    "Rewrites the game path, profile, base, downloads, mods, profiles and overwrite directories in ModOrganizer.ini from the paths configured here, translating them into forward-slash Z: drive paths that Qt's INI engine never mangles. Every other key, including your Nexus settings, is preserved. Run it after moving a folder or when MO2 cannot find Anomaly. REVERSIBLE: the previous file is saved as ModOrganizer.ini.bak.",
                    None,
                ) {
                    self.pending_job = Some(Job::SyncModOrganizerIni);
                }

                if card_action(
                    ui,
                    enabled,
                    "🩹  Repair MO2 Wine Paths",
                    "Scans ModOrganizer.ini for paths corrupted by Qt's INI escaping, such as base_directory=Z:ntoamesTALKER_Gamma, and rewrites them as forward-slash Wine paths derived from the Anomaly and GAMMA folders configured here. Every other key is preserved. Run it when MO2 opens with an empty mod list or cannot find its base directory. REVERSIBLE: the previous file is saved as ModOrganizer.ini.bak.",
                    Some(color_warning()),
                ) {
                    self.pending_job = Some(Job::RepairModOrganizerPaths);
                }

                if card_action(
                    ui,
                    enabled,
                    "★  Re-index GAMMA Nominated Presets",
                    "Re-deploys the modlist shipped with the current GAMMA definition into the G.A.M.M.A profile and registers any extra preset list it finds as its own Mod Organizer 2 profile. Run it after a definition update or when the profile selector in MO2 is empty. REVERSIBLE: existing profile settings.txt files are left untouched, only the mod lists are refreshed.",
                    None,
                ) {
                    self.pending_job = Some(Job::ReindexPresets);
                }

                if card_action(
                    ui,
                    enabled,
                    "🔒  Fix File Permissions",
                    "Recursively restores 755 on directories and 644 on files across the mods and downloads folders, skipping symlinks. Run it after extracting an archive that carried read-only Windows attributes, after copying from another machine, or when an install fails with Permission denied. SAFE: only permission bits change, never file contents. Filesystems without POSIX modes, such as NTFS or exFAT, are reported and skipped.",
                    None,
                ) {
                    self.pending_job = Some(Job::FixPermissions);
                }

                if card_action(
                    ui,
                    enabled,
                    "📂  USVFS Workaround",
                    "Flattens the whole enabled mod list into the USVFS target folder, which replaces the virtual file system Mod Organizer 2 cannot provide under Wine. Run it when you want to launch the game without MO2. IRREVERSIBLE for the target folder, which is rewritten, and it needs as much free space as Anomaly plus every mod.",
                    None,
                ) {
                    self.pending_job = Some(Job::UsvfsWorkaround);
                }

                ui.add_space(4.0_f32);
                ui.separator();
                ui.add_space(4.0_f32);

                ui.add_enabled_ui(enabled, |ui| {
                    egui::ComboBox::from_label("Keymap")
                        .selected_text(self.layout.label())
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut self.layout, KeymapLayout::Azerty, "AZERTY");
                            ui.selectable_value(&mut self.layout, KeymapLayout::Dvorak, "DVORAK");
                        })
                        .response
                        .on_hover_text("Selects which physical keyboard layout the in-game keybinds should be rewritten for. The current user.ltx must still be in its original QWERTY state.")
                        .on_disabled_hover_text(LOCK_HINT);
                });

                ui.add_space(6.0_f32);

                if card_action(
                    ui,
                    enabled,
                    "⌨  Switch Keymap",
                    "Rewrites the Anomaly keybinds in appdata/user.ltx to match the layout selected above. Run it once after a fresh install if you do not use QWERTY. NOT REVERSIBLE from here: the job refuses to run twice because it only recognises a QWERTY user.ltx, so restore that file from its .bak copy to change layout again.",
                    None,
                ) {
                    self.pending_job = Some(Job::SwitchKeymap(layout));
                }
            },
        );
    }
}

impl LauncherApp {
    fn draw_paths(&mut self, ui: &mut egui::Ui, locked: bool) {
        let enabled = !locked;
        ui.add_space(6.0_f32);

        section(ui, "Directories", |ui| {
            ui.add_enabled_ui(enabled, |ui| {
                if folder_row(
                    ui,
                    "Anomaly",
                    &mut self.anomaly_input,
                    "$HOME/Games/Anomaly",
                    "Root folder of your S.T.A.L.K.E.R. Anomaly install, the one containing the bin/ and gamedata/ subfolders. Everything the launcher patches or verifies lives under it.",
                    PathCheck::Directory,
                ) {
                    self.pending_save = true;
                }
                if folder_row(
                    ui,
                    "Organizer / mods",
                    &mut self.gamma_input,
                    "$HOME/Games/GAMMA",
                    "Folder where Mod Organizer 2 and the GAMMA modpack live, usually a sibling of the Anomaly folder. The launcher creates downloads/, mods/ and the modpack installer folder inside it.",
                    PathCheck::Directory,
                ) {
                    self.pending_save = true;
                }
                if folder_row(
                    ui,
                    "Wine prefix",
                    &mut self.prefix_input,
                    "$HOME/.local/share/wineprefixes/stalker_anomaly_gamma",
                    "WINEPREFIX / Proton prefix used to run everything below. Keep this dedicated to Anomaly so other Wine applications cannot interfere with the USVFS hooks Mod Organizer 2 installs.",
                    PathCheck::DirectoryAutoCreate,
                ) {
                    self.pending_save = true;
                }
                if folder_row(
                    ui,
                    "Proton",
                    &mut self.proton_input,
                    "/usr/share/steam/compatibilitytools.d/proton-cachyos-slr",
                    "Folder of the Proton build the game runs under. Auto-detection prefers proton-cachyos-slr, then any other CachyOS build, then GE-Proton, then whatever else is installed.",
                    PathCheck::Directory,
                ) {
                    self.pending_save = true;
                }
                if folder_row(
                    ui,
                    "Download cache",
                    &mut self.cache_input,
                    "/tmp",
                    "Optional separate folder to store downloaded mod archives in, symlinked as the downloads folder. Point it at a large disk to keep tens of gigabytes of archives off your system drive. Leave empty to keep them inside the Anomaly folder.",
                    PathCheck::OptionalDirectory,
                ) {
                    self.pending_save = true;
                }
                if folder_row(
                    ui,
                    "USVFS target",
                    &mut self.final_input,
                    "$HOME/Games/GAMMA-flattened",
                    "Destination folder the USVFS workaround writes the flattened, fully merged install into. It needs as much free space as Anomaly plus every enabled mod.",
                    PathCheck::OptionalDirectory,
                ) {
                    self.pending_save = true;
                }
            });
        });

        section(ui, "Executables", |ui| {
            ui.add_enabled_ui(enabled, |ui| {
                if exe_row(
                    ui,
                    "MO2 executable",
                    &mut self.mo2_input,
                    "Explicit path to ModOrganizer.exe. Leave empty to auto-locate it inside the Organizer / mods folder at launch time.",
                ) {
                    self.pending_save = true;
                }
                if exe_row(
                    ui,
                    "Launcher executable",
                    &mut self.launcher_input,
                    "Explicit path to AnomalyLauncher.exe. Leave empty to auto-locate it inside the Anomaly folder at launch time.",
                ) {
                    self.pending_save = true;
                }
                if exe_row(
                    ui,
                    "Game executable",
                    &mut self.game_input,
                    "Explicit path to the Anomaly renderer binary. Leave empty to auto-locate the best one inside Anomaly/bin, preferring AnomalyDX11AVX.exe.",
                ) {
                    self.pending_save = true;
                }
            });
        });

        section(ui, "Auto-Detection", |ui| {
            ui.horizontal(|ui| {
                if ui
                    .add_enabled_ui(enabled, |ui| {
                        ui.add_sized(
                            ACTION_SIZE,
                            egui::Button::new(
                                egui::RichText::new("Auto-Detect Paths")
                                    .color(egui::Color32::WHITE)
                                    .strong()
                                    .size(15.0_f32),
                            )
                            .fill(color_detect()),
                        )
                        .on_hover_text("Scans the folders above plus your home directory, Steam libraries and mounted drives, then ranks every match instead of taking the first one. Paths containing G.A.M.M.A., STALKER_GAMMA or GAMMA win over any other install, backups and copies are pushed down, and the reasoning for each pick is written to the console log.")
                        .on_disabled_hover_text(LOCK_HINT)
                        .clicked()
                    })
                    .inner
                {
                    self.pending_detect = true;
                }

                ui.add_space(12.0_f32);
                let hint = if self.detecting {
                    "Scanning and scoring candidates in the background, the interface stays responsive..."
                } else {
                    "Every field it finds a confident match for is overwritten and saved immediately."
                };
                ui.label(egui::RichText::new(hint).weak());
            });
        });
    }

    fn draw_tweaks(&mut self, ui: &mut egui::Ui, locked: bool) {
        let enabled = !locked;
        ui.add_space(6.0_f32);

        section(ui, "Install options", |ui| {
            ui.add_enabled_ui(enabled, |ui| {
                if check_row(
                    ui,
                    &mut self.config.update_gamma_definition,
                    "Update the gamma definition",
                    "Re-downloads the modpack definition repository before installing, so the newest mod list and versions are used. Recommended: keep enabled. Default: ON.",
                ) {
                    self.pending_save = true;
                }
                if check_row(
                    ui,
                    &mut self.config.patch_anomaly,
                    "Patch the Anomaly directory",
                    "Applies the GAMMA-specific patches on top of a vanilla Anomaly install. Required for GAMMA to work correctly. Default: ON.",
                ) {
                    self.pending_save = true;
                }
                if check_row(
                    ui,
                    &mut self.config.preserve_user_config,
                    "Preserve user.ltx when patching",
                    "Keeps your existing graphics and input settings file instead of overwriting it while patching Anomaly. Enable only if you hand-tuned user.ltx. Default: OFF.",
                ) {
                    self.pending_save = true;
                }
                if check_row(
                    ui,
                    &mut self.config.anomaly_verify,
                    "Verify Anomaly after installation",
                    "Runs an MD5 pass over Anomaly's files right after installing, to catch corrupted downloads before they turn into crashes. Recommended: keep enabled. Default: ON.",
                ) {
                    self.pending_save = true;
                }
                if check_row(
                    ui,
                    &mut self.config.anomaly_purge_cache,
                    "Delete the Anomaly archive after install",
                    "Removes the downloaded Anomaly installer archive once installation succeeds, freeing several gigabytes of disk space. Default: ON.",
                ) {
                    self.pending_save = true;
                }
                if check_row(
                    ui,
                    &mut self.config.install_mod_organizer,
                    "Install MO2",
                    "Installs or updates Mod Organizer 2 as part of GAMMA setup. Leave disabled if you manage MO2 yourself or already have it installed. Default: OFF.",
                ) {
                    self.pending_save = true;
                }
                if check_row(
                    ui,
                    &mut self.config.update_download_cache,
                    "Redownload mismatching archives on MD5 check",
                    "When Check MD5 finds a mod archive whose checksum does not match, redownload it automatically instead of only reporting it. Default: ON.",
                ) {
                    self.pending_save = true;
                }
                if check_row(
                    ui,
                    &mut self.config.purge_unused_downloads,
                    "Purge unused downloads after MD5 check",
                    "Deletes archives in the download cache that are no longer referenced by the current modpack definition. Default: ON.",
                ) {
                    self.pending_save = true;
                }
                if check_row(
                    ui,
                    &mut self.config.force_recheck,
                    "Force full recheck on Sync / Update",
                    "Ignores the skip-if-already-installed shortcut and reverifies and reinstalls every mod during Sync / Update. Much slower; enable only when an install looks broken. Default: OFF.",
                ) {
                    self.pending_save = true;
                }

                ui.add_space(ROW_SPACING);

                if text_field_row(
                    ui,
                    "Repository",
                    &mut self.repository_input,
                    "Grokitach/Stalker_GAMMA",
                    "GitHub 'owner/repository' the GAMMA modpack definition is pulled from. Only change this if you use a fork. Default: Grokitach/Stalker_GAMMA.",
                ) {
                    self.pending_save = true;
                }
                if text_field_row(
                    ui,
                    "Revision",
                    &mut self.revision_input,
                    "leave empty to track the latest revision",
                    "Specific git branch, tag or commit of the repository above to install. Pin it to reproduce a known-good modpack state. Default: empty, which always tracks the latest revision.",
                ) {
                    self.pending_save = true;
                }
                if text_field_row(
                    ui,
                    "MO2 version",
                    &mut self.config.mo_version,
                    "v2.5.2",
                    "Mod Organizer 2 release tag to install. Must be a tag that exists in the ModOrganizer2 repository. Default: v2.5.2.",
                ) {
                    self.pending_save = true;
                }
                if text_field_row(
                    ui,
                    "UMU game id",
                    &mut self.umu_id_input,
                    "stalker-anomaly-gamma",
                    "GAMEID environment variable passed to umu-run. It identifies this prefix to UMU and Proton, so keeping it unique avoids sharing runtime state with other games. Default: stalker-anomaly-gamma.",
                ) {
                    self.pending_save = true;
                }
            });
        });

        section(ui, "Wine / Proton runtime", |ui| {
            ui.add_enabled_ui(enabled, |ui| {
                if check_row(
                    ui,
                    &mut self.config.runner.use_gamemode,
                    "GameMode",
                    "Wraps the launch command with gamemoderun, which asks the Linux kernel and GPU driver for a temporary performance boost while the game runs. Default: ON when the gamemoderun binary exists in PATH, otherwise OFF.",
                ) {
                    self.pending_save = true;
                }
                if check_row(
                    ui,
                    &mut self.config.runner.use_umu,
                    "UMU Runner",
                    "Launches through umu-run, which reproduces Steam's Proton environment outside of Steam. When disabled, the plain 'wine' binary is used instead and PROTONPATH is ignored. Default: ON when the umu-run binary exists in PATH, otherwise OFF.",
                ) {
                    self.pending_save = true;
                }
                if check_row(
                    ui,
                    &mut self.config.runner.fsr_enabled,
                    "Wine Fullscreen FSR",
                    "Sets WINE_FULLSCREEN_FSR, which lets Wine and Proton upscale the game with AMD FSR when it runs below your monitor's native resolution. Trades some sharpness for a large performance gain. Default: OFF.",
                ) {
                    self.pending_save = true;
                }

                ui.add_space(ROW_SPACING);

                ui.horizontal(|ui| {
                    if ui
                        .checkbox(&mut self.config.runner.omp_threads_enabled, "OMP_NUM_THREADS")
                        .on_hover_text("Enables the OMP_NUM_THREADS environment variable, which caps how many threads OpenMP-based engine code is allowed to use. Default: ON.")
                        .on_disabled_hover_text(LOCK_HINT)
                        .changed()
                    {
                        self.pending_save = true;
                    }
                    if ui
                        .add_enabled(
                            self.config.runner.omp_threads_enabled,
                            egui::DragValue::new(&mut self.config.runner.omp_threads).speed(1.0_f64),
                        )
                        .on_hover_text(format!(
                            "Number of threads to allow. Optimal value: this machine's detected core count ({}). Lowering it helps on few-core systems or when compiling in the background; raising it past the core count gains nothing.",
                            detected_cpu_count()
                        ))
                        .on_disabled_hover_text("Enable OMP_NUM_THREADS on the left to edit the thread count.")
                        .changed()
                    {
                        self.pending_save = true;
                    }
                    if ui
                        .add_enabled(
                            self.config.runner.omp_threads_enabled,
                            egui::Button::new("Reset"),
                        )
                        .on_hover_text("Restores the thread count to this machine's detected CPU core count.")
                        .on_disabled_hover_text("Enable OMP_NUM_THREADS on the left to reset the thread count.")
                        .clicked()
                    {
                        self.config.runner.omp_threads = detected_cpu_count();
                        self.pending_save = true;
                    }
                });

                ui.add_space(ROW_SPACING);

                let dxvk = toggle_text_with_reset(
                    ui,
                    &mut self.config.runner.dxvk_config_enabled,
                    "DXVK Configuration",
                    "Enables the DXVK_CONFIG environment variable, letting you override DXVK's Direct3D to Vulkan translation settings without editing a dxvk.conf file. Default: ON.",
                    &mut self.config.runner.dxvk_config,
                    "Semicolon-separated DXVK options. The recommended value raises the shader compiler thread count to this machine's core count and caps maxTessFactor at 8, which avoids a known Anomaly tessellation stutter.",
                    "Restores the recommended DXVK configuration for this machine, recomputed from its current core count.",
                );
                if dxvk.reset {
                    self.config.runner.reset_dxvk_config();
                }
                if dxvk.dirty() {
                    self.pending_save = true;
                }

                let overrides = toggle_text_with_reset(
                    ui,
                    &mut self.config.runner.wine_dll_overrides_enabled,
                    "WINEDLLOVERRIDES",
                    "Enables the WINEDLLOVERRIDES environment variable below. This is critical for Mod Organizer 2: USVFS hooks file access by replacing the game's DLLs at load time, and Wine's own DLL loading order can otherwise fight with that hook, leaving the virtual mod list silently inactive. Default: ON.",
                    &mut self.config.runner.wine_dll_overrides,
                    "Comma-separated 'dll=mode' pairs. 'n,b' means try the native DLL first and fall back to the builtin one. The recommended value forces usvfs and the d3dcompiler and d3dx redistributables to load that way, which USVFS and MO2 depend on to intercept file access correctly.",
                    "Restores the exact WINEDLLOVERRIDES value that Mod Organizer 2 and USVFS require.",
                );
                if overrides.reset {
                    self.config.runner.reset_dll_overrides();
                }
                if overrides.dirty() {
                    self.pending_save = true;
                }
            });
        });

        section(ui, "SOCKS5 Proxy", |ui| {
            ui.add_enabled_ui(enabled, |ui| {
                if check_row(
                    ui,
                    &mut self.config.proxy.enabled,
                    "Enable Proxy",
                    "Routes every download and every ModDB or GitHub request through the SOCKS5 proxy configured below. Host names are resolved by the proxy itself, so DNS is tunnelled too. Useful when your connection blocks ModDB or throttles large downloads. Default: OFF.",
                ) {
                    self.pending_save = true;
                }

                if check_row(
                    ui,
                    &mut self.config.proxy.socks5_auto_retry,
                    "Auto Enable on Retries",
                    "Keeps every transfer on the direct connection, but switches that single item to the SOCKS5 proxy configured below once it has failed the number of consecutive attempts set beside this box. A failure counts when the connection times out, DNS cannot be resolved, the connection is refused, or the server answers with 403, 408, 425, 429 or a 5xx status. As soon as the item finishes, the next one starts on the direct connection again. If no host is set, the fallback is skipped and a warning is logged instead. Default: OFF.",
                ) {
                    self.pending_save = true;
                }

                ui.add_space(6.0_f32);
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("Retry threshold").strong());
                    if ui
                        .add_enabled(
                            self.config.proxy.socks5_auto_retry,
                            egui::DragValue::new(&mut self.config.proxy.socks5_retry_threshold)
                                .speed(0.2_f64)
                                .range(1..=10),
                        )
                        .on_hover_text("Number of consecutive failed attempts on the direct connection before the proxy is engaged for the current item. Default: 2.")
                        .on_disabled_hover_text("Turn Auto Enable on Retries on to edit the threshold.")
                        .changed()
                    {
                        self.pending_save = true;
                    }
                    ui.label(
                        egui::RichText::new("consecutive failures")
                            .color(color_text_normal())
                            .small(),
                    );
                });

                let proxy_enabled = self.config.proxy.enabled || self.config.proxy.socks5_auto_retry;
                ui.add_space(ROW_SPACING);

                ui.label(egui::RichText::new("Host / IP").strong());
                if ui
                    .add_enabled(
                        proxy_enabled,
                        egui::TextEdit::singleline(&mut self.config.proxy.host)
                            .font(egui::TextStyle::Monospace)
                            .hint_text("127.0.0.1")
                            .desired_width(f32::INFINITY),
                    )
                    .on_hover_text("Hostname or IP address of the SOCKS5 server. Default: empty, which keeps the direct connection even when the toggle above is on.")
                    .on_disabled_hover_text("Turn Enable Proxy or Auto Enable on Retries on to edit the proxy host.")
                    .lost_focus()
                {
                    self.pending_save = true;
                }
                ui.add_space(10.0_f32);

                ui.label(egui::RichText::new("Port").strong());
                if ui
                    .add_enabled(
                        proxy_enabled,
                        egui::DragValue::new(&mut self.config.proxy.port).speed(1.0_f64),
                    )
                    .on_hover_text("TCP port the SOCKS5 server listens on. Default: 1080, the standard SOCKS port. Tor listens on 9050 and most SSH dynamic tunnels use 1080.")
                    .on_disabled_hover_text("Turn Enable Proxy or Auto Enable on Retries on to edit the proxy port.")
                    .changed()
                {
                    self.pending_save = true;
                }
                ui.add_space(10.0_f32);

                ui.label(egui::RichText::new("Username (optional)").strong());
                if ui
                    .add_enabled(
                        proxy_enabled,
                        egui::TextEdit::singleline(&mut self.config.proxy.username)
                            .font(egui::TextStyle::Monospace)
                            .desired_width(f32::INFINITY),
                    )
                    .on_hover_text("Username for an authenticated SOCKS5 proxy. Default: empty, which connects anonymously.")
                    .on_disabled_hover_text("Turn Enable Proxy or Auto Enable on Retries on to edit the proxy username.")
                    .lost_focus()
                {
                    self.pending_save = true;
                }
                ui.add_space(10.0_f32);

                ui.label(egui::RichText::new("Password (optional)").strong());
                if ui
                    .add_enabled(
                        proxy_enabled,
                        egui::TextEdit::singleline(&mut self.config.proxy.password)
                            .password(true)
                            .desired_width(f32::INFINITY),
                    )
                    .on_hover_text("Password paired with the username above. It is stored in plain text inside config.toml, so prefer a dedicated proxy account. Default: empty.")
                    .on_disabled_hover_text("Turn Enable Proxy or Auto Enable on Retries on to edit the proxy password.")
                    .lost_focus()
                {
                    self.pending_save = true;
                }

                ui.add_space(ROW_SPACING);
                ui.label(
                    egui::RichText::new(proxy_preview(&self.config))
                        .weak()
                        .monospace(),
                )
                .on_hover_text("The exact proxy URL handed to the HTTP client when the next download job starts. The password is masked here but sent in full.");
            });
        });
    }

    fn draw_progress_panel(&mut self, ui: &mut egui::Ui) {
        let state = self.control.state();

        self.draw_job_controls(ui, state);
        draw_progress(ui, &self.tasks, &self.overall, state);
    }

    fn draw_job_controls(&mut self, ui: &mut egui::Ui, state: ControlState) {
        if !state.is_active() {
            self.confirm_cancel = false;
            return;
        }

        let paused = state == ControlState::Paused;
        let cancelling = state == ControlState::Cancelling;

        let (badge, badge_color) = match state {
            ControlState::Paused => ("⏸  Paused by User", color_warning()),
            ControlState::Cancelling => ("⏹  Cancelling, finishing the current step", color_danger()),
            _ => ("●  Running", color_fill_alternate()),
        };

        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new(badge)
                    .color(badge_color)
                    .strong()
                    .size(15.0_f32),
            )
            .on_hover_text("State of the running job. Paused keeps every connection and worker thread alive, Cancelling aborts the transfers in flight and cleans up partial files.");

            if fallback_is_active() {
                ui.label(
                    egui::RichText::new("SOCKS5 FALLBACK")
                        .color(color_fill_alternate())
                        .strong()
                        .monospace()
                        .small(),
                )
                .on_hover_text("The direct connection failed for the current item, so it is being fetched through the configured SOCKS5 proxy. The next item starts on the direct connection again.");
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let cancel_label = if self.confirm_cancel {
                    "✖  Confirm Cancel"
                } else {
                    "✖  Cancel"
                };

                let cancel = egui::Button::new(
                    egui::RichText::new(cancel_label)
                        .color(egui::Color32::WHITE)
                        .strong(),
                )
                .fill(color_danger());

                if ui
                    .add_enabled(!cancelling, cancel)
                    .on_hover_text("Aborts every transfer in flight, deletes the partial .part files it created and returns the launcher to Idle. Press once to arm, once more to confirm. IRREVERSIBLE for the current job, but completed archives are kept.")
                    .on_disabled_hover_text("The job is already cancelling.")
                    .clicked()
                {
                    if self.confirm_cancel {
                        self.confirm_cancel = false;
                        self.control.cancel();
                        self.status = ControlState::Cancelling.label().to_string();
                        self.push_log(LogLevel::Warn, "[!] Cancel requested, aborting the running job");
                    } else {
                        self.confirm_cancel = true;
                    }
                }

                let toggle_label = if paused { "▶  Resume" } else { "⏸  Pause" };
                let toggle_color = if paused {
                    color_fill()
                } else {
                    color_warning()
                };

                let toggle = egui::Button::new(
                    egui::RichText::new(toggle_label)
                        .color(egui::Color32::BLACK)
                        .strong(),
                )
                .fill(toggle_color);

                if ui
                    .add_enabled(!cancelling, toggle)
                    .on_hover_text("Suspends or resumes the running job. Paused holds the worker loops and stops consuming bytes without dropping the connection, so the transfer continues into the same .part file when you resume.")
                    .on_disabled_hover_text("The job is already cancelling.")
                    .clicked()
                {
                    if paused {
                        self.control.resume();
                        self.status = ControlState::Running.label().to_string();
                        self.push_log(LogLevel::Info, "[*] Job resumed");
                    } else {
                        self.control.pause();
                        self.status = ControlState::Paused.label().to_string();
                        self.push_log(LogLevel::Warn, "[*] Job paused by user");
                    }
                }
            });
        });

        ui.add_space(6.0_f32);
    }

    fn draw_console(&mut self, ui: &mut egui::Ui) {
        ui.add_space(6.0_f32);
        egui::Frame::none()
            .fill(ui.visuals().faint_bg_color)
            .stroke(egui::Stroke::new(
                1.0_f32,
                ui.visuals().widgets.noninteractive.bg_stroke.color,
            ))
            .rounding(egui::Rounding::same(SECTION_ROUNDING))
            .inner_margin(egui::Margin::same(SECTION_MARGIN))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("Console Log").strong().size(17.0_f32));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .button("Clear log")
                            .on_hover_text("Clears every line currently shown below. This does not affect any running job.")
                            .clicked()
                        {
                            self.logs.clear();
                        }
                        ui.label(
                            egui::RichText::new(format!("{} lines", self.logs.len()))
                                .weak()
                                .small(),
                        )
                        .on_hover_text(format!(
                            "Number of log lines currently buffered. The oldest lines are dropped once {MAX_LOG_ENTRIES} lines are reached."
                        ));
                    });
                });
                ui.add_space(10.0_f32);
                draw_logs(ui, &self.logs);
            });
    }
}

impl eframe::App for LauncherApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.drain_events();
        self.poll_detection();
        self.poll_space_scan();
        self.process = self.process_state.snapshot();

        let locked = self.busy || self.detecting || self.process.is_some();

        if locked {
            ctx.request_repaint_after(Duration::from_millis(80));
        }

        self.draw_header(ctx);

        egui::TopBottomPanel::bottom("progress")
            .resizable(false)
            .show(ctx, |ui| {
                ui.add_space(8.0_f32);
                self.draw_progress_panel(ui);
                ui.add_space(8.0_f32);
            });

        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| match self.active_tab {
                    Tab::Dashboard => self.draw_dashboard(ui, locked),
                    Tab::Paths => self.draw_paths(ui, locked),
                    Tab::Tweaks => self.draw_tweaks(ui, locked),
                    Tab::Console => self.draw_console(ui),
                });
        });

        if self.pending_save {
            self.pending_save = false;
            self.save_config();
        }

        if self.pending_detect {
            self.pending_detect = false;
            self.start_auto_detect(ctx);
        }

        if self.pending_space_scan {
            self.pending_space_scan = false;
            self.start_space_scan(ctx);
        }

        if let Some(job) = self.pending_job.take() {
            self.start(job, ctx);
        }
    }
}

fn proxy_preview(config: &AppConfig) -> String {
    let proxy = &config.proxy;

    if !proxy.enabled && !proxy.socks5_auto_retry {
        return "Effective proxy: direct connection (proxy disabled)".to_string();
    }

    let host = proxy.host.trim();
    if host.is_empty() {
        return "Effective proxy: direct connection (no host set)".to_string();
    }

    let user = proxy.username.trim();
    let endpoint = if user.is_empty() {
        format!("socks5h://{host}:{}", proxy.port)
    } else {
        format!("socks5h://{user}:********@{host}:{}", proxy.port)
    };

    if proxy.enabled {
        format!("Effective proxy: {endpoint} (every request)")
    } else {
        format!(
            "Effective proxy: {endpoint} (fallback after {} failed attempt(s))",
            proxy.effective_threshold()
        )
    }
}

fn tab_button(ui: &mut egui::Ui, current: &mut Tab, value: Tab, label: &str, tooltip: &str) {
    let active = *current == value;

    let text = if active {
        egui::RichText::new(label)
            .strong()
            .size(16.0_f32)
            .color(egui::Color32::WHITE)
    } else {
        egui::RichText::new(label).size(16.0_f32)
    };

    let clicked = ui
        .scope(|ui| {
            let visuals = ui.visuals_mut();
            visuals.selection.bg_fill = accent_color();
            visuals.selection.stroke = egui::Stroke::new(2.0_f32, egui::Color32::WHITE);
            visuals.widgets.hovered.weak_bg_fill = accent_color().linear_multiply(0.35_f32);

            ui.add_sized(
                [156.0_f32, 34.0_f32],
                egui::SelectableLabel::new(active, text),
            )
            .on_hover_text(tooltip)
            .clicked()
        })
        .inner;

    if clicked {
        *current = value;
    }
}

fn apply_theme(ctx: &egui::Context, dark: bool) {
    let mut visuals = if dark {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    };

    if dark {
        visuals.panel_fill = egui::Color32::from_rgb(22, 25, 31);
        visuals.window_fill = egui::Color32::from_rgb(28, 32, 39);
        visuals.extreme_bg_color = color_track();
        visuals.faint_bg_color = egui::Color32::from_rgb(34, 38, 46);
        visuals.selection.bg_fill = accent_color();
        visuals.selection.stroke = egui::Stroke::new(1.0_f32, color_text_bright());
        visuals.hyperlink_color = color_fill_alternate();
        visuals.warn_fg_color = color_warning();
        visuals.error_fg_color = color_danger();

        let border = egui::Color32::from_rgb(64, 70, 82);
        let text = color_text_normal();

        visuals.widgets.noninteractive.bg_fill = egui::Color32::from_rgb(34, 38, 46);
        visuals.widgets.noninteractive.weak_bg_fill = egui::Color32::from_rgb(34, 38, 46);
        visuals.widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0_f32, border);
        visuals.widgets.noninteractive.fg_stroke = egui::Stroke::new(1.0_f32, text);

        visuals.widgets.inactive.bg_fill = egui::Color32::from_rgb(44, 49, 59);
        visuals.widgets.inactive.weak_bg_fill = egui::Color32::from_rgb(44, 49, 59);
        visuals.widgets.inactive.bg_stroke = egui::Stroke::new(1.0_f32, border);
        visuals.widgets.inactive.fg_stroke = egui::Stroke::new(1.0_f32, text);

        visuals.widgets.hovered.bg_fill = egui::Color32::from_rgb(58, 66, 80);
        visuals.widgets.hovered.weak_bg_fill = egui::Color32::from_rgb(58, 66, 80);
        visuals.widgets.hovered.bg_stroke = egui::Stroke::new(1.0_f32, color_fill());
        visuals.widgets.hovered.fg_stroke = egui::Stroke::new(1.5_f32, color_text_bright());

        visuals.widgets.active.bg_fill = color_fill();
        visuals.widgets.active.weak_bg_fill = color_fill();
        visuals.widgets.active.bg_stroke = egui::Stroke::new(1.0_f32, color_text_bright());
        visuals.widgets.active.fg_stroke = egui::Stroke::new(1.5_f32, egui::Color32::WHITE);

        visuals.widgets.open.bg_fill = egui::Color32::from_rgb(44, 49, 59);
        visuals.widgets.open.weak_bg_fill = egui::Color32::from_rgb(44, 49, 59);
        visuals.widgets.open.fg_stroke = egui::Stroke::new(1.0_f32, color_text_bright());
    }

    let rounding = egui::Rounding::same(WIDGET_ROUNDING);
    visuals.widgets.noninteractive.rounding = rounding;
    visuals.widgets.inactive.rounding = rounding;
    visuals.widgets.hovered.rounding = rounding;
    visuals.widgets.active.rounding = rounding;
    visuals.widgets.open.rounding = rounding;

    ctx.set_visuals(visuals);

    ctx.style_mut(|style| {
        style.spacing.item_spacing = egui::vec2(10.0_f32, 10.0_f32);
        style.spacing.button_padding = egui::vec2(12.0_f32, 8.0_f32);
        style.spacing.interact_size.y = 30.0_f32;
        style.text_styles.insert(
            egui::TextStyle::Heading,
            egui::FontId::new(22.0_f32, egui::FontFamily::Proportional),
        );
        style.text_styles.insert(
            egui::TextStyle::Body,
            egui::FontId::new(15.0_f32, egui::FontFamily::Proportional),
        );
        style.text_styles.insert(
            egui::TextStyle::Button,
            egui::FontId::new(15.0_f32, egui::FontFamily::Proportional),
        );
        style.text_styles.insert(
            egui::TextStyle::Monospace,
            egui::FontId::new(14.0_f32, egui::FontFamily::Monospace),
        );
    });
}

fn maintenance_card(
    ui: &mut egui::Ui,
    title: &str,
    subtitle: &str,
    accent: egui::Color32,
    add_contents: impl FnOnce(&mut egui::Ui),
) {
    egui::Frame::none()
        .fill(accent.linear_multiply(0.07_f32))
        .stroke(egui::Stroke::new(1.0_f32, accent.linear_multiply(0.55_f32)))
        .rounding(egui::Rounding::same(CARD_ROUNDING))
        .inner_margin(egui::Margin::same(CARD_MARGIN))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(
                egui::RichText::new(title)
                    .strong()
                    .size(15.0_f32)
                    .color(accent),
            );
            ui.label(egui::RichText::new(subtitle).weak().small());
            ui.add_space(10.0_f32);
            add_contents(ui);
        });
    ui.add_space(ROW_SPACING);
}

fn card_action(
    ui: &mut egui::Ui,
    enabled: bool,
    label: &str,
    tooltip: &str,
    tint: Option<egui::Color32>,
) -> bool {
    let size = egui::vec2(ui.available_width(), CARD_BUTTON_HEIGHT);
    let mut button = egui::Button::new(label).min_size(size);

    if let Some(color) = tint {
        button = egui::Button::new(
            egui::RichText::new(label)
                .color(color_text_bright())
                .strong(),
        )
        .min_size(size)
        .fill(color.linear_multiply(0.45_f32))
        .stroke(egui::Stroke::new(1.0_f32, color));
    }

    let clicked = ui
        .add_enabled(enabled, button)
        .on_hover_text(tooltip)
        .on_disabled_hover_text(LOCK_HINT)
        .clicked();

    ui.add_space(6.0_f32);
    clicked
}

fn estimate_row(ui: &mut egui::Ui, label: &str, value: Option<u64>) {
    let text = match value {
        Some(bytes) => format!("{label}: {}", human_bytes(bytes)),
        None => format!("{label}: not measured yet"),
    };

    ui.label(egui::RichText::new(text).weak().small())
        .on_hover_text("Space the action above can reclaim, measured when the window opened or when you pressed Refresh estimates.");
    ui.add_space(8.0_f32);
}

fn section(ui: &mut egui::Ui, title: &str, add_contents: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::none()
        .fill(ui.visuals().faint_bg_color)
        .stroke(egui::Stroke::new(
            1.0_f32,
            ui.visuals().widgets.noninteractive.bg_stroke.color,
        ))
        .rounding(egui::Rounding::same(SECTION_ROUNDING))
        .inner_margin(egui::Margin::same(SECTION_MARGIN))
        .show(ui, |ui| {
            ui.label(egui::RichText::new(title).strong().size(17.0_f32));
            ui.add_space(10.0_f32);
            add_contents(ui);
        });
    ui.add_space(ROW_SPACING);
}

fn sized_action(
    ui: &mut egui::Ui,
    enabled: bool,
    size: [f32; 2],
    label: &str,
    tooltip: &str,
) -> bool {
    ui.add_enabled_ui(enabled, |ui| {
        ui.add_sized(size, egui::Button::new(label))
            .on_hover_text(tooltip)
            .on_disabled_hover_text(LOCK_HINT)
            .clicked()
    })
    .inner
}

fn check_row(ui: &mut egui::Ui, value: &mut bool, label: &str, tooltip: &str) -> bool {
    ui.checkbox(value, label)
        .on_hover_text(tooltip)
        .on_disabled_hover_text(LOCK_HINT)
        .changed()
}

fn draw_badge(ui: &mut egui::Ui, ok: bool, ok_text: &str, bad_text: &str, tooltip: &str) {
    let (text, color) = if ok {
        (ok_text, color_ok())
    } else {
        (bad_text, color_missing())
    };
    ui.add(egui::Label::new(
        egui::RichText::new(text).color(color).strong().monospace(),
    ))
    .on_hover_text(tooltip);
}

fn draw_path_badge(ui: &mut egui::Ui, value: &str, check: PathCheck) {
    let trimmed = value.trim();

    if trimmed.is_empty() {
        if let PathCheck::OptionalDirectory = check {
            ui.label(egui::RichText::new("[OPTIONAL]").weak().monospace())
                .on_hover_text("Leave this empty to use the default location.");
        } else {
            draw_badge(
                ui,
                false,
                "[FOUND]",
                "[NOT SET]",
                "This path is required but has not been configured yet. Fill it in or run Auto-Detect Paths.",
            );
        }
        return;
    }

    let expanded = expand_path(Path::new(trimmed));
    let exists = match check {
        PathCheck::File => expanded.is_file(),
        _ => expanded.is_dir(),
    };

    if exists {
        draw_badge(ui, true, "[FOUND]", "[MISSING]", "This path exists on disk.");
        return;
    }

    if let PathCheck::DirectoryAutoCreate = check {
        ui.add(egui::Label::new(
            egui::RichText::new("[WILL CREATE]")
                .color(color_pending())
                .strong()
                .monospace(),
        ))
        .on_hover_text("This prefix does not exist yet. Proton or Wine will create it automatically the first time you launch.");
        return;
    }

    draw_badge(
        ui,
        false,
        "[FOUND]",
        "[MISSING]",
        "This path does not exist yet. Fix it manually or run Auto-Detect Paths.",
    );
}

fn human_duration(seconds: u64) -> String {
    let hours = seconds / 3600;
    let minutes = (seconds % 3600) / 60;
    let remainder = seconds % 60;

    if hours > 0 {
        format!("{hours}h {minutes:02}m")
    } else {
        format!("{minutes:02}:{remainder:02}")
    }
}

fn eta_text(downloaded: u64, total: Option<u64>, speed: Option<f64>) -> String {
    let remaining = match total {
        Some(total) => total.saturating_sub(downloaded),
        None => return "ETA --:--".to_string(),
    };

    match speed {
        Some(speed) if speed > 1.0_f64 => {
            format!("ETA {}", human_duration((remaining as f64 / speed) as u64))
        }
        _ => "ETA --:--".to_string(),
    }
}

fn progress_fill(state: ControlState, alternate: bool) -> egui::Color32 {
    match state {
        ControlState::Paused => color_warning(),
        ControlState::Cancelling => color_danger(),
        _ if alternate => color_fill_alternate(),
        _ => color_fill(),
    }
}

fn progress_caption(ui: &mut egui::Ui, text: String, tooltip: &str) {
    ui.add(egui::Label::new(
        egui::RichText::new(text)
            .color(color_text_bright())
            .monospace()
            .size(13.0_f32),
    ))
    .on_hover_text(tooltip);
}

fn progress_bar(ui: &mut egui::Ui, fraction: f32, animate: bool, fill: egui::Color32) {
    let previous = ui.visuals().extreme_bg_color;
    ui.visuals_mut().extreme_bg_color = color_track();
    ui.add(
        egui::ProgressBar::new(fraction)
            .fill(fill)
            .animate(animate),
    );
    ui.visuals_mut().extreme_bg_color = previous;
    ui.add_space(6.0_f32);
}

fn draw_progress(
    ui: &mut egui::Ui,
    tasks: &[TaskProgress],
    overall: &Option<(usize, usize, String)>,
    state: ControlState,
) {
    if tasks.is_empty() && overall.is_none() {
        ui.label(
            egui::RichText::new("No task running")
                .color(color_text_normal())
                .weak(),
        )
        .on_hover_text("Progress bars for downloads, extractions and checksum passes appear here while a job runs.");
        return;
    }

    if let Some((current, total, label)) = overall {
        let ratio = if *total == 0 {
            0.0_f32
        } else {
            *current as f32 / *total as f32
        };
        let fraction = ratio.clamp(0.0_f32, 1.0_f32);

        progress_caption(
            ui,
            format!(
                "{label} ({current}/{total}) [{:.1}%]",
                fraction * 100.0_f32
            ),
            "Overall progress across every item of the running job.",
        );
        progress_bar(ui, fraction, false, progress_fill(state, true));
    }

    for task in tasks.iter().take(MAX_VISIBLE_TASKS) {
        let speed = task
            .speed_bps
            .map(human_speed)
            .unwrap_or_else(|| "-- MB/s".to_string());

        match task.total {
            Some(total) if total > 0 => {
                let fraction = (task.downloaded as f32 / total as f32).clamp(0.0_f32, 1.0_f32);

                progress_caption(
                    ui,
                    format!(
                        "{} ({} / {}) - {speed} [{:.0}%] - {}",
                        task.label,
                        human_bytes(task.downloaded),
                        human_bytes(total),
                        fraction * 100.0_f32,
                        eta_text(task.downloaded, task.total, task.speed_bps)
                    ),
                    "Active item, bytes transferred, total size, smoothed transfer speed, completion percentage and estimated time left.",
                );
                progress_bar(ui, fraction, false, progress_fill(state, false));
            }
            _ => {
                progress_caption(
                    ui,
                    format!("{} ({}) - {speed}", task.label, human_bytes(task.downloaded)),
                    "The server did not report a total size, so only the transferred amount and the smoothed speed are known.",
                );
                progress_bar(ui, 0.0_f32, state == ControlState::Running, progress_fill(state, false));
            }
        }
    }

    if tasks.len() > MAX_VISIBLE_TASKS {
        ui.label(
            egui::RichText::new(format!(
                "+{} more stream(s) in flight",
                tasks.len() - MAX_VISIBLE_TASKS
            ))
            .color(color_text_normal())
            .small(),
        )
        .on_hover_text("Only the first streams are drawn to keep the interface readable; the rest keep running.");
    }

    let aggregate: f64 = tasks.iter().filter_map(|task| task.speed_bps).sum();
    if aggregate > 0.0_f64 {
        ui.label(
            egui::RichText::new(format!("Total throughput: {}", human_speed(aggregate)))
                .color(color_text_normal())
                .small(),
        )
        .on_hover_text("Combined smoothed speed of every active stream.");
    }
}

fn draw_logs(ui: &mut egui::Ui, logs: &VecDeque<LogEntry>) {
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .stick_to_bottom(true)
        .show(ui, |ui| {
            for entry in logs {
                let color = match entry.level {
                    LogLevel::Info => color_text_normal(),
                    LogLevel::Warn => color_warning(),
                    LogLevel::Error => color_danger(),
                };

                ui.label(
                    egui::RichText::new(format!(
                        "[{}] {}",
                        entry.timestamp.format("%H:%M:%S%.3f"),
                        entry.message
                    ))
                    .monospace()
                    .color(color),
                );
            }
        });
}

fn path_input(
    ui: &mut egui::Ui,
    label: &str,
    value: &mut String,
    hint: &str,
    tooltip: &str,
    check: PathCheck,
    pick: impl FnOnce() -> Option<PathBuf>,
    browse_tooltip: &str,
) -> bool {
    let mut should_save = false;

    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(label).strong());
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            draw_path_badge(ui, value.as_str(), check);
        });
    });

    ui.horizontal(|ui| {
        if ui
            .button("Browse")
            .on_hover_text(browse_tooltip)
            .on_disabled_hover_text(LOCK_HINT)
            .clicked()
        {
            if let Some(path) = pick() {
                *value = path.display().to_string();
                should_save = true;
            }
        }

        if ui
            .add(
                egui::TextEdit::singleline(value)
                    .font(egui::TextStyle::Monospace)
                    .hint_text(hint)
                    .desired_width(f32::INFINITY),
            )
            .on_hover_text(tooltip)
            .on_disabled_hover_text(LOCK_HINT)
            .lost_focus()
        {
            should_save = true;
        }
    });

    ui.add_space(ROW_SPACING);
    should_save
}

fn folder_row(
    ui: &mut egui::Ui,
    label: &str,
    value: &mut String,
    hint: &str,
    tooltip: &str,
    check: PathCheck,
) -> bool {
    let browse_tooltip = format!("Opens a folder picker to choose the {label} directory.");
    path_input(
        ui,
        label,
        value,
        hint,
        tooltip,
        check,
        || rfd::FileDialog::new().pick_folder(),
        &browse_tooltip,
    )
}

fn exe_row(ui: &mut egui::Ui, label: &str, value: &mut String, tooltip: &str) -> bool {
    let browse_tooltip = format!("Opens a file picker to choose the {label}.");
    path_input(
        ui,
        label,
        value,
        "leave empty to resolve it automatically",
        tooltip,
        PathCheck::File,
        || {
            rfd::FileDialog::new()
                .add_filter("Windows executable", &["exe"])
                .pick_file()
        },
        &browse_tooltip,
    )
}

fn text_field_row(
    ui: &mut egui::Ui,
    label: &str,
    value: &mut String,
    hint: &str,
    tooltip: &str,
) -> bool {
    ui.label(egui::RichText::new(label).strong());
    let response = ui
        .add(
            egui::TextEdit::singleline(value)
                .hint_text(hint)
                .desired_width(f32::INFINITY),
        )
        .on_hover_text(tooltip)
        .on_disabled_hover_text(LOCK_HINT);
    ui.add_space(10.0_f32);

    response.lost_focus()
}

fn toggle_text_with_reset(
    ui: &mut egui::Ui,
    enabled: &mut bool,
    toggle_label: &str,
    toggle_tooltip: &str,
    value: &mut String,
    field_tooltip: &str,
    reset_tooltip: &str,
) -> ToggleOutcome {
    let mut outcome = ToggleOutcome::default();

    if ui
        .checkbox(enabled, toggle_label)
        .on_hover_text(toggle_tooltip)
        .on_disabled_hover_text(LOCK_HINT)
        .changed()
    {
        outcome.changed = true;
    }

    let editable = *enabled;

    ui.horizontal(|ui| {
        if ui
            .add_enabled(editable, egui::Button::new("Reset"))
            .on_hover_text(reset_tooltip)
            .on_disabled_hover_text("Enable the toggle above to edit or reset this value.")
            .clicked()
        {
            outcome.reset = true;
        }

        if ui
            .add_enabled(
                editable,
                egui::TextEdit::singleline(value)
                    .font(egui::TextStyle::Monospace)
                    .desired_width(f32::INFINITY),
            )
            .on_hover_text(field_tooltip)
            .on_disabled_hover_text("Enable the toggle above to edit or reset this value.")
            .lost_focus()
        {
            outcome.changed = true;
        }
    });

    ui.add_space(ROW_SPACING);
    outcome
}

fn short_path(path: &Path) -> String {
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or_default();

    match path
        .parent()
        .and_then(|parent| parent.file_name())
        .and_then(|value| value.to_str())
    {
        Some(parent_name) => format!(".../{parent_name}/{name}"),
        None => name.to_string(),
    }
}

fn path_to_string(path: &Option<PathBuf>) -> String {
    path.as_ref()
        .map(|path| path.display().to_string())
        .unwrap_or_default()
}

fn optional_path(value: &str) -> Option<PathBuf> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(PathBuf::from(trimmed))
    }
}

fn optional_text(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blank_fields_become_none() {
        assert!(optional_path("   ").is_none());
        assert!(optional_text("\t").is_none());
    }

    #[test]
    fn filled_fields_are_trimmed() {
        assert_eq!(optional_path("  /tmp/x "), Some(PathBuf::from("/tmp/x")));
        assert_eq!(optional_text("  main "), Some("main".to_string()));
    }

    #[test]
    fn paths_are_shortened_to_their_last_two_segments() {
        assert_eq!(
            short_path(Path::new("/games/GAMMA/ModOrganizer.exe")),
            ".../GAMMA/ModOrganizer.exe"
        );
    }

    #[test]
    fn a_disabled_proxy_reports_a_direct_connection() {
        let config = AppConfig::default();
        assert!(proxy_preview(&config).contains("direct connection"));
    }

    #[test]
    fn an_enabled_proxy_reports_a_remote_resolving_url() {
        let mut config = AppConfig::default();
        config.proxy.enabled = true;
        config.proxy.host = "127.0.0.1".to_string();
        config.proxy.port = 9050;

        assert!(proxy_preview(&config).contains("socks5h://127.0.0.1:9050"));
    }

    #[test]
    fn a_dirty_toggle_is_reported_for_both_edits_and_resets() {
        assert!(!ToggleOutcome::default().dirty());
        assert!(ToggleOutcome {
            changed: false,
            reset: true
        }
        .dirty());
    }
}
