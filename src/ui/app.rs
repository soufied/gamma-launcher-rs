use crate::commands::{estimate_reclaimable, KeymapLayout, SpaceEstimate};
use crate::config::AppConfig;
use crate::detect::{self, DetectionResult};
use crate::mods::downloader::base::fallback_is_active;
use crate::report::{human_bytes, human_speed, LogEntry, LogLevel, Reporter, TaskEvent};
use crate::runner::{self, ControlState, JobControl, JobHandle, LaunchTarget, ProcessSnapshot, ProcessState};
use crate::ui::job::{self, Job};
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::thread;
use std::time::{Duration, Instant};

const MAX_LOG_ENTRIES: usize = 4000;
const MAX_VISIBLE_TASKS: usize = 6;

pub(super) use crate::ui::style::{
    ACTION_SIZE, BADGE_ROUNDING, CARD_BUTTON_HEIGHT, CARD_MARGIN, CARD_ROUNDING, LOCK_HINT,
    MEDIUM_LAYOUT_WIDTH, ROW_SPACING, SECONDARY_ACTION_SIZE, SECTION_MARGIN, SECTION_ROUNDING,
    WIDE_LAYOUT_WIDTH,
};
pub(super) use crate::ui::theme::{
    accent_color, apply_theme, color_danger, color_detect, color_fill, color_fill_alternate,
    color_fill_alternate_text, color_game, color_launcher, color_missing, color_mo2, color_ok,
    color_pending, color_running, color_running_text, color_success, color_text_bright,
    color_text_muted, color_text_normal, color_track, color_warning,
};

#[derive(Debug, Clone)]
struct TaskProgress {
    id: String,
    label: String,
    downloaded: u64,
    total: Option<u64>,
    speed_bps: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Tab {
    Dashboard,
    Paths,
    Tweaks,
    Runtime,
    Console,
}

#[derive(Debug, Clone, Copy)]
pub(super) enum PathCheck {
    Directory,
    DirectoryAutoCreate,
    File,
    OptionalDirectory,
}

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct ToggleOutcome {
    changed: bool,
    pub reset: bool,
}

impl ToggleOutcome {
    pub fn dirty(&self) -> bool {
        self.changed || self.reset
    }
}

pub struct LauncherApp {
    pub(super) config: AppConfig,
    pub(super) config_location: String,
    pub(super) active_tab: Tab,
    pub(super) anomaly_input: String,
    pub(super) gamma_input: String,
    pub(super) prefix_input: String,
    pub(super) proton_input: String,
    pub(super) cache_input: String,
    pub(super) final_input: String,
    pub(super) mo2_input: String,
    pub(super) launcher_input: String,
    pub(super) game_input: String,
    pub(crate) repository_input: String,
    pub(crate) revision_input: String,
    pub(crate) umu_id_input: String,
    pub(crate) mo2_shortcut_input: String,
    pub(super) nickname_input: String,
    pub(super) custom_steam_path_input: String,
    pub(super) graphics_dll_overrides_input: String,
    pub(super) extra_env_key_input: String,
    pub(super) extra_env_value_input: String,
    pub(super) layout: KeymapLayout,
    logs: VecDeque<LogEntry>,
    logs_copied_at: Option<Instant>,
    tasks: Vec<TaskProgress>,
    overall: Option<(usize, usize, String)>,
    events: Option<Receiver<TaskEvent>>,
    detection: Option<Receiver<DetectionResult>>,
    pub(super) background_tx: Sender<TaskEvent>,
    background_rx: Receiver<TaskEvent>,
    process_state: ProcessState,
    pub(super) adopted_processes: crate::process::SharedProcessRegistry,
    pub(super) tray_handle: Option<crate::tray::TrayHandle>,
    pub(super) tray_commands: Receiver<crate::tray::TrayCommand>,
    pub(super) last_diagnostics: Option<crate::diagnostics::DiagnosticReport>,
    pub(super) window_visible: bool,
    last_close_request: Option<Instant>,
    control: JobHandle,
    confirm_cancel: bool,
    pub(super) process: Option<ProcessSnapshot>,
    pub(super) space: Option<SpaceEstimate>,
    space_scan: Option<Receiver<SpaceEstimate>>,
    pub(super) pending_job: Option<Job>,
    pub(super) pending_save: bool,
    pub(super) pending_detect: bool,
    pub(super) pending_space_scan: bool,
    pub(super) scanning_space: bool,
    pub(super) detecting: bool,
    pub(super) busy: bool,
    pub(super) status: String,
}

impl LauncherApp {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        adopted_processes: crate::process::SharedProcessRegistry,
        tray_handle: Option<crate::tray::TrayHandle>,
        tray_commands: Receiver<crate::tray::TrayCommand>,
    ) -> Self {
        let config = AppConfig::load();
        let (background_tx, background_rx) = mpsc::channel();

        apply_theme(&cc.egui_ctx, config.dark_mode);

        let config_location = AppConfig::config_path()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|_| "config.toml".to_string());

        let window_visible = !config.tray.start_in_tray;

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
            mo2_shortcut_input: config.runner.mo2_shortcut_title.clone(),
            nickname_input: config.spacewar.player_nickname.clone(),
            custom_steam_path_input: path_to_string(&config.spacewar.custom_steam_path),
            graphics_dll_overrides_input: config.graphics.dll_overrides.clone(),
            extra_env_key_input: String::new(),
            extra_env_value_input: String::new(),
            layout: KeymapLayout::default(),
            logs: VecDeque::new(),
            logs_copied_at: None,
            tasks: Vec::new(),
            overall: None,
            events: None,
            detection: None,
            background_tx,
            background_rx,
            process_state: runner::new_process_state(),
            adopted_processes,
            tray_handle,
            tray_commands,
            last_diagnostics: None,
            window_visible,
            last_close_request: None,
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
        self.config.runner.mo2_shortcut_title = self.mo2_shortcut_input.trim().to_string();
        self.config.proxy.host = self.config.proxy.host.trim().to_string();
        self.config.spacewar.player_nickname = self.nickname_input.trim().to_string();
        self.config.spacewar.custom_steam_path = optional_path(&self.custom_steam_path_input);
        self.config.graphics.dll_overrides = self.graphics_dll_overrides_input.trim().to_string();
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

    pub(super) fn push_log(&mut self, level: LogLevel, message: impl Into<String>) {
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
            self.adopted_processes.clone(),
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
}

impl LauncherApp {
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
                        .color(color_fill_alternate_text(ui))
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
                        let copy_button = ui
                            .add(egui::Button::new(
                                egui::RichText::new("Copy Logs").strong(),
                            ))
                            .on_hover_text("Copies the entire buffered console log to the system clipboard.");
                        if copy_button.clicked() {
                            let combined = self
                                .logs
                                .iter()
                                .map(|entry| {
                                    format!(
                                        "[{}] [{}] {}",
                                        entry.timestamp.format("%Y-%m-%d %H:%M:%S%.3f"),
                                        log_level_label(entry.level),
                                        entry.message
                                    )
                                })
                                .collect::<Vec<_>>()
                                .join("\n");
                            ui.output_mut(|o| o.copied_text = combined);
                            self.logs_copied_at = Some(Instant::now());
                        }
                        if let Some(copied_at) = self.logs_copied_at {
                            let elapsed = copied_at.elapsed();
                            let fade_duration = Duration::from_millis(2000);
                            if elapsed < fade_duration {
                                let remaining = fade_duration.saturating_sub(elapsed).as_secs_f32();
                                let alpha = (remaining / fade_duration.as_secs_f32()).clamp(0.0_f32, 1.0_f32);
                                let mut color = color_success();
                                color = egui::Color32::from_rgba_unmultiplied(
                                    color.r(),
                                    color.g(),
                                    color.b(),
                                    (255.0_f32 * alpha) as u8,
                                );
                                ui.label(
                                    egui::RichText::new("Logs copied to clipboard")
                                        .small()
                                        .color(color),
                                );
                                ui.ctx().request_repaint();
                            } else {
                                self.logs_copied_at = None;
                            }
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

impl LauncherApp {
    fn drain_tray_commands(&mut self, ctx: &egui::Context) {
        use crate::tray::TrayCommand;
        use std::sync::mpsc::TryRecvError;

        loop {
            match self.tray_commands.try_recv() {
                Ok(TrayCommand::RestoreWindow) => {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                    self.window_visible = true;
                    self.last_close_request = None;
                }
                Ok(TrayCommand::LaunchGame) => {
                    let target = runner::effective_launch_target(&self.config, LaunchTarget::Game);
                    self.pending_job = Some(Job::Launch(target));
                }
                Ok(TrayCommand::KillAll) => {
                    self.pending_job = Some(Job::KillProcesses);
                }
                Ok(TrayCommand::QuitKeepRunning) => {
                    self.shutdown_and_exit();
                }
                Ok(TrayCommand::QuitEverything) => {
                    let registry = self.adopted_processes.clone();
                    let reporter = Reporter::detached(self.background_tx.clone());
                    let _ = crate::process::terminate_all(&registry, &reporter);
                    self.shutdown_and_exit();
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => break,
            }
        }
    }
}

impl eframe::App for LauncherApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.drain_tray_commands(ctx);
        self.drain_events();
        self.poll_detection();
        self.poll_space_scan();
        self.process = self.process_state.snapshot();

        if let Some(handle) = self.tray_handle.as_ref() {
            crate::tray::update_counts(handle, self.adopted_processes.count());
        }

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
                    Tab::Runtime => self.draw_runtime(ui, locked),
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

        if ctx.input(|i| i.viewport().close_requested()) {
            match self.config.tray.close_action {
                crate::config::CloseAction::Exit => {
                    self.shutdown_and_exit();
                }
                crate::config::CloseAction::MinimizeToTray => {
                    ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                    self.hide_to_tray(ctx);
                }
            }
        }

        let minimized = ctx.input(|i| i.viewport().minimized.unwrap_or(false));
        if minimized && self.config.tray.minimize_to_tray && self.window_visible {
            self.hide_to_tray(ctx);
        }
    }
}

impl LauncherApp {
    fn hide_to_tray(&mut self, ctx: &egui::Context) {
        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
        self.window_visible = false;
        self.last_close_request = None;
    }

    fn shutdown_and_exit(&mut self) {
        if let Some(handle) = self.tray_handle.take() {
            drop(handle);
        }
        std::process::exit(0);
    }
}

pub(super) fn proxy_preview(config: &AppConfig) -> String {
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

pub(super) fn maintenance_card(
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

pub(super) fn card_action(
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
                .color(color_text_bright(ui))
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

pub(super) fn estimate_row(ui: &mut egui::Ui, label: &str, value: Option<u64>) {
    let text = match value {
        Some(bytes) => format!("{label}: {}", human_bytes(bytes)),
        None => format!("{label}: not measured yet"),
    };

    ui.label(egui::RichText::new(text).weak().small())
        .on_hover_text("Space the action above can reclaim, measured when the window opened or when you pressed Refresh estimates.");
    ui.add_space(8.0_f32);
}

pub(super) fn section(ui: &mut egui::Ui, title: &str, add_contents: impl FnOnce(&mut egui::Ui)) {
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

pub(super) fn collapsible_section(
    ui: &mut egui::Ui,
    id: &str,
    title: &str,
    subtitle: &str,
    accent: egui::Color32,
    default_open: bool,
    add_contents: impl FnOnce(&mut egui::Ui),
) {
    egui::Frame::none()
        .fill(ui.visuals().faint_bg_color)
        .stroke(egui::Stroke::new(
            1.0_f32,
            ui.visuals().widgets.noninteractive.bg_stroke.color,
        ))
        .rounding(egui::Rounding::same(SECTION_ROUNDING))
        .inner_margin(egui::Margin::same(SECTION_MARGIN))
        .show(ui, |ui| {
            egui::CollapsingHeader::new(
                egui::RichText::new(title)
                    .strong()
                    .size(16.0_f32)
                    .color(color_text_bright(ui)),
            )
            .id_salt(id)
            .default_open(default_open)
            .show(ui, |ui| {
                if !subtitle.is_empty() {
                    ui.label(
                        egui::RichText::new(subtitle)
                            .color(color_text_muted(ui))
                            .size(12.5_f32),
                    );
                    ui.add_space(8.0_f32);
                }
                let previous = ui.visuals().selection.bg_fill;
                ui.visuals_mut().selection.bg_fill = accent;
                add_contents(ui);
                ui.visuals_mut().selection.bg_fill = previous;
            });
        });
    ui.add_space(ROW_SPACING);
}

pub(super) fn toggle_row(
    ui: &mut egui::Ui,
    value: &mut bool,
    label: &str,
    subtitle: &str,
    tooltip: &str,
) -> bool {
    let response = ui
        .horizontal(|ui| {
            let changed = ui
                .add(egui::Checkbox::new(value, ""))
                .on_hover_text(tooltip)
                .on_disabled_hover_text(LOCK_HINT)
                .changed();

            ui.vertical(|ui| {
                ui.label(egui::RichText::new(label).strong().size(14.0_f32));
                if !subtitle.is_empty() {
                    ui.label(
                        egui::RichText::new(subtitle)
                            .color(color_text_muted(ui))
                            .size(11.5_f32),
                    );
                }
            });

            changed
        })
        .inner;

    ui.add_space(8.0_f32);
    response
}

pub(super) fn sized_action(
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

pub(super) fn check_row(ui: &mut egui::Ui, value: &mut bool, label: &str, tooltip: &str) -> bool {
    ui.checkbox(value, label)
        .on_hover_text(tooltip)
        .on_disabled_hover_text(LOCK_HINT)
        .changed()
}

pub(super) fn env_var_editor(
    ui: &mut egui::Ui,
    entries: &mut Vec<(String, String)>,
    key_draft: &mut String,
    value_draft: &mut String,
) -> bool {
    let mut changed = false;
    let mut remove_index: Option<usize> = None;

    if entries.is_empty() {
        ui.label(
            egui::RichText::new("No custom environment variables configured.")
                .color(color_text_muted(ui))
                .italics()
                .size(12.5_f32),
        );
        ui.add_space(6.0_f32);
    } else {
        for (index, (key, value)) in entries.iter_mut().enumerate() {
            ui.horizontal(|ui| {
                if ui
                    .add(
                        egui::TextEdit::singleline(key)
                            .font(egui::TextStyle::Monospace)
                            .desired_width(200.0_f32)
                            .hint_text("KEY"),
                    )
                    .on_hover_text("Environment variable name passed to the launch command.")
                    .on_disabled_hover_text(LOCK_HINT)
                    .lost_focus()
                {
                    changed = true;
                }
                ui.label(egui::RichText::new("=").color(color_text_muted(ui)).strong());
                if ui
                    .add(
                        egui::TextEdit::singleline(value)
                            .font(egui::TextStyle::Monospace)
                            .desired_width(f32::INFINITY)
                            .hint_text("value"),
                    )
                    .on_hover_text("Value assigned to this environment variable.")
                    .on_disabled_hover_text(LOCK_HINT)
                    .lost_focus()
                {
                    changed = true;
                }
                if ui
                    .add(egui::Button::new(
                        egui::RichText::new("✖").color(color_missing()),
                    ))
                    .on_hover_text("Removes this environment variable row.")
                    .on_disabled_hover_text(LOCK_HINT)
                    .clicked()
                {
                    remove_index = Some(index);
                }
            });
            ui.add_space(4.0_f32);
        }
    }

    if let Some(index) = remove_index {
        entries.remove(index);
        changed = true;
    }

    ui.add_space(6.0_f32);
    ui.separator();
    ui.add_space(6.0_f32);

    ui.horizontal(|ui| {
        ui.add(
            egui::TextEdit::singleline(key_draft)
                .font(egui::TextStyle::Monospace)
                .desired_width(200.0_f32)
                .hint_text("NEW_VARIABLE"),
        )
        .on_hover_text("Name of the new environment variable to add.")
        .on_disabled_hover_text(LOCK_HINT);
        ui.label(egui::RichText::new("=").color(color_text_muted(ui)).strong());
        ui.add(
            egui::TextEdit::singleline(value_draft)
                .font(egui::TextStyle::Monospace)
                .desired_width(f32::INFINITY)
                .hint_text("value"),
        )
        .on_hover_text("Value for the new environment variable.")
        .on_disabled_hover_text(LOCK_HINT);

        let key_ready = !key_draft.trim().is_empty();
        if ui
            .add_enabled(key_ready, egui::Button::new("+ Add"))
            .on_hover_text("Adds this key/value pair to the list of custom environment variables passed to every launch.")
            .on_disabled_hover_text("Type a variable name on the left before adding it.")
            .clicked()
        {
            entries.push((key_draft.trim().to_string(), value_draft.clone()));
            key_draft.clear();
            value_draft.clear();
            changed = true;
        }
    });

    changed
}

pub(super) fn draw_badge(ui: &mut egui::Ui, ok: bool, ok_text: &str, bad_text: &str, tooltip: &str) {
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
            .color(color_text_bright(ui))
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
                .color(color_text_normal(ui))
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
            .color(color_text_normal(ui))
            .small(),
        )
        .on_hover_text("Only the first streams are drawn to keep the interface readable; the rest keep running.");
    }

    let aggregate: f64 = tasks.iter().filter_map(|task| task.speed_bps).sum();
    if aggregate > 0.0_f64 {
        ui.label(
            egui::RichText::new(format!("Total throughput: {}", human_speed(aggregate)))
                .color(color_text_normal(ui))
                .small(),
        )
        .on_hover_text("Combined smoothed speed of every active stream.");
    }
}

fn log_level_label(level: LogLevel) -> &'static str {
    match level {
        LogLevel::Info => "INFO",
        LogLevel::Warn => "WARN",
        LogLevel::Error => "ERROR",
    }
}

fn draw_logs(ui: &mut egui::Ui, logs: &VecDeque<LogEntry>) {
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .stick_to_bottom(true)
        .show(ui, |ui| {
            for entry in logs {
                let color = match entry.level {
                    LogLevel::Info => color_text_normal(ui),
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

pub(super) fn text_field_row(
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

pub(super) fn toggle_text_with_reset(
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

pub(super) fn short_path(path: &Path) -> String {
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
