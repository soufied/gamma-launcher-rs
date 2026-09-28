use crate::process::{AdoptedProcess, ProcessCategory};
use crate::report::{LogLevel, Reporter};
use crate::ui::app::{
    accent_color, apply_theme, color_danger, color_game, color_mo2, color_ok, color_running,
    color_text_bright, color_warning, LauncherApp, Tab, BADGE_ROUNDING, LOCK_HINT,
};
use std::thread;

const HEADER_HORIZONTAL_MARGIN: f32 = 18.0_f32;
const HEADER_VERTICAL_MARGIN: f32 = 12.0_f32;
const CONTROL_HEIGHT: f32 = 32.0_f32;
const CONTROL_ROUNDING: f32 = 8.0_f32;
const TAB_TRACK_ROUNDING: f32 = 12.0_f32;
const TAB_TRACK_PADDING: f32 = 4.0_f32;
const TAB_HEIGHT: f32 = 34.0_f32;
const TAB_HORIZONTAL_PADDING: f32 = 20.0_f32;
const TAB_ROUNDING: f32 = 9.0_f32;
const TAB_UNDERLINE_HEIGHT: f32 = 3.0_f32;
const TAB_FONT_SIZE: f32 = 15.0_f32;
const TITLE_FONT_SIZE: f32 = 21.0_f32;
const SUBTITLE_FONT_SIZE: f32 = 12.5_f32;
const BRAND_BAR_WIDTH: f32 = 5.0_f32;
const BRAND_BAR_HEIGHT_TALL: f32 = 40.0_f32;
const BRAND_BAR_HEIGHT_SHORT: f32 = 28.0_f32;
const STATUS_DOT_RADIUS: f32 = 4.0_f32;
const STATUS_PILL_MIN_WIDTH: f32 = 128.0_f32;
const THEME_SEGMENT_WIDTH: f32 = 62.0_f32;
const SAVE_BUTTON_WIDTH: f32 = 116.0_f32;
const CONFIG_CHIP_MAX_CHARS: usize = 26;
const TELEMETRY_COMPACT_WIDTH: f32 = 1240.0_f32;
const BRAND_ROW_WIDE_WIDTH: f32 = 1120.0_f32;
const STATUS_TEXT_MAX_CHARS: usize = 30;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StatusTone {
    Idle,
    Busy,
}

#[derive(Debug, Default, Clone, Copy)]
struct ProcessSummary {
    game_count: usize,
    game_pid: u32,
    mo2_count: usize,
    mo2_pid: u32,
    runner_count: usize,
    total_memory_bytes: u64,
    total_count: usize,
}

impl ProcessSummary {
    fn from_processes(processes: &[AdoptedProcess]) -> Self {
        let mut summary = Self::default();

        for process in processes {
            summary.total_count += 1;
            summary.total_memory_bytes += process.memory_bytes.unwrap_or_default();

            match process.category {
                ProcessCategory::Game => {
                    if summary.game_count == 0 {
                        summary.game_pid = process.pid;
                    }
                    summary.game_count += 1;
                }
                ProcessCategory::ModOrganizer => {
                    if summary.mo2_count == 0 {
                        summary.mo2_pid = process.pid;
                    }
                    summary.mo2_count += 1;
                }
                ProcessCategory::RunnerWrapper => {
                    summary.runner_count += 1;
                }
            }
        }

        summary
    }
}

struct TabSpec {
    tab: Tab,
    label: &'static str,
    tooltip: &'static str,
}

const TAB_SPECS: [TabSpec; 5] = [
    TabSpec {
        tab: Tab::Dashboard,
        label: "Dashboard",
        tooltip: "Launch MO2, the Anomaly Launcher or the game directly, run maintenance jobs, and switch the in-game keymap layout.",
    },
    TabSpec {
        tab: Tab::Paths,
        label: "Paths",
        tooltip: "Point the launcher at your Anomaly install, GAMMA/MO2 folder, wine prefix, Proton build and executables, or let the ranked auto-detection find them for you.",
    },
    TabSpec {
        tab: Tab::Tweaks,
        label: "Tweaks",
        tooltip: "Fine-tune install behaviour, multiplayer identity, the desktop tray, frame pacing, shader caches and the SOCKS5 proxy used for downloads.",
    },
    TabSpec {
        tab: Tab::Runtime,
        label: "Runtime",
        tooltip: "Low-level Wine/Proton runtime settings: DXVK, GameMode, UMU, the X-Ray core, and DLL overrides.",
    },
    TabSpec {
        tab: Tab::Console,
        label: "Console Log",
        tooltip: "Full scrolling output of every job: downloads, installs, verifications and game launches, each line timestamped.",
    },
];

impl LauncherApp {
    pub(super) fn draw_header(&mut self, ctx: &egui::Context) {
        let panel_fill = ctx.style().visuals.panel_fill;

        egui::TopBottomPanel::top("header")
            .show_separator_line(true)
            .frame(
                egui::Frame::none()
                    .fill(panel_fill)
                    .inner_margin(egui::Margin::symmetric(
                        HEADER_HORIZONTAL_MARGIN,
                        HEADER_VERTICAL_MARGIN,
                    )),
            )
            .show(ctx, |ui| {
                self.draw_brand_row(ui, ctx);
                ui.add_space(12.0_f32);
                self.draw_navigation_row(ui);
                self.draw_process_badge(ui);
            });
    }

    fn draw_brand_row(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        if ui.available_width() >= BRAND_ROW_WIDE_WIDTH {
            ui.horizontal(|ui| {
                self.draw_brand(ui, true);

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    self.draw_save_button(ui);
                    self.draw_theme_switch(ui, ctx);
                    self.draw_config_chip(ui);
                    self.draw_status_pill(ui);
                });
            });
        } else {
            ui.horizontal(|ui| {
                self.draw_brand(ui, false);

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    self.draw_save_button(ui);
                    self.draw_theme_switch(ui, ctx);
                });
            });
            ui.add_space(8.0_f32);
            ui.horizontal(|ui| {
                self.draw_status_pill(ui);
                self.draw_config_chip(ui);
            });
        }
    }

    fn draw_brand(&self, ui: &mut egui::Ui, show_subtitle: bool) {
        let bar_height = if show_subtitle {
            BRAND_BAR_HEIGHT_TALL
        } else {
            BRAND_BAR_HEIGHT_SHORT
        };
        let (bar_rect, _) = ui.allocate_exact_size(
            egui::vec2(BRAND_BAR_WIDTH, bar_height),
            egui::Sense::hover(),
        );
        ui.painter().rect_filled(
            bar_rect,
            egui::Rounding::same(BRAND_BAR_WIDTH * 0.5_f32),
            accent_color(),
        );

        ui.add_space(4.0_f32);

        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 1.0_f32;
            ui.label(
                egui::RichText::new("S.T.A.L.K.E.R. G.A.M.M.A. Launcher")
                    .size(TITLE_FONT_SIZE)
                    .strong(),
            );
            if show_subtitle {
                ui.label(
                    egui::RichText::new("Native Rust installer and manager for Anomaly on Linux")
                        .size(SUBTITLE_FONT_SIZE)
                        .weak(),
                );
            }
        });
    }

    fn draw_status_pill(&self, ui: &mut egui::Ui) {
        let tone = if self.busy || self.detecting || self.process.is_some() {
            StatusTone::Busy
        } else {
            StatusTone::Idle
        };
        let dot_color = match tone {
            StatusTone::Idle => color_ok(),
            StatusTone::Busy => color_running(),
        };

        let border = ui.visuals().widgets.noninteractive.bg_stroke.color;
        let fill = ui.visuals().faint_bg_color;

        let response = egui::Frame::none()
            .fill(fill)
            .stroke(egui::Stroke::new(1.0_f32, border))
            .rounding(egui::Rounding::same(CONTROL_ROUNDING))
            .inner_margin(egui::Margin::symmetric(12.0_f32, 0.0_f32))
            .show(ui, |ui| {
                ui.set_min_height(CONTROL_HEIGHT);
                ui.set_min_width(STATUS_PILL_MIN_WIDTH);
                ui.horizontal_centered(|ui| {
                    ui.spacing_mut().item_spacing.x = 8.0_f32;

                    let pulse = if tone == StatusTone::Busy {
                        let time = ui.input(|input| input.time);
                        ((time * 3.0_f64).sin() * 0.5_f64 + 0.5_f64) as f32
                    } else {
                        1.0_f32
                    };
                    let (dot_rect, _) = ui.allocate_exact_size(
                        egui::vec2(STATUS_DOT_RADIUS * 3.0_f32, STATUS_DOT_RADIUS * 3.0_f32),
                        egui::Sense::hover(),
                    );
                    let radius = if tone == StatusTone::Busy {
                        STATUS_DOT_RADIUS + pulse * 1.5_f32
                    } else {
                        STATUS_DOT_RADIUS
                    };
                    ui.painter().circle_filled(
                        dot_rect.center(),
                        radius,
                        dot_color.linear_multiply(0.45_f32 + 0.55_f32 * pulse),
                    );

                    ui.label(
                        egui::RichText::new(shorten_middle(
                            self.status.as_str(),
                            STATUS_TEXT_MAX_CHARS,
                        ))
                        .strong(),
                    );

                    if self.busy || self.detecting {
                        ui.add(egui::Spinner::new().size(14.0_f32));
                    }
                });
            })
            .response;

        response.on_hover_text(
            "Current state of the background queue: Idle, the running job's name, Scanning, Completed or Failed.",
        );
    }

    fn draw_config_chip(&self, ui: &mut egui::Ui) {
        let border = ui.visuals().widgets.noninteractive.bg_stroke.color;
        let file_name = std::path::Path::new(self.config_location.as_str())
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| self.config_location.clone());
        let file_name = shorten_middle(&file_name, CONFIG_CHIP_MAX_CHARS);

        let response = egui::Frame::none()
            .fill(egui::Color32::TRANSPARENT)
            .stroke(egui::Stroke::new(1.0_f32, border))
            .rounding(egui::Rounding::same(CONTROL_ROUNDING))
            .inner_margin(egui::Margin::symmetric(10.0_f32, 0.0_f32))
            .show(ui, |ui| {
                ui.set_min_height(CONTROL_HEIGHT);
                ui.horizontal_centered(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0_f32;
                    ui.label(egui::RichText::new("Config").small().weak());
                    ui.label(egui::RichText::new(file_name).monospace().small());
                });
            })
            .response;

        response.on_hover_text(format!(
            "Settings file location: {}\nThis file sits next to the launcher executable and is rewritten by Save settings and before every job.",
            self.config_location
        ));
    }

    fn draw_theme_switch(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let border = ui.visuals().widgets.noninteractive.bg_stroke.color;
        let track_fill = ui.visuals().extreme_bg_color;
        let mut chosen_dark = self.config.dark_mode;

        egui::Frame::none()
            .fill(track_fill)
            .stroke(egui::Stroke::new(1.0_f32, border))
            .rounding(egui::Rounding::same(CONTROL_ROUNDING))
            .inner_margin(egui::Margin::same(3.0_f32))
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.x = 2.0_f32;
                ui.horizontal(|ui| {
                    let light_clicked = theme_segment(
                        ui,
                        "Light",
                        !chosen_dark,
                        "Switches to the default light theme. Takes effect immediately and is remembered on save.",
                    );
                    let dark_clicked = theme_segment(
                        ui,
                        "Dark",
                        chosen_dark,
                        "Switches to the high-contrast dark theme tuned for CachyOS / KDE Plasma. Takes effect immediately and is remembered on save. Default: ON.",
                    );

                    if light_clicked {
                        chosen_dark = false;
                    }
                    if dark_clicked {
                        chosen_dark = true;
                    }
                });
            });

        if chosen_dark != self.config.dark_mode {
            self.config.dark_mode = chosen_dark;
            apply_theme(ctx, self.config.dark_mode);
            self.pending_save = true;
        }
    }

    fn draw_save_button(&mut self, ui: &mut egui::Ui) {
        let label = egui::RichText::new("Save settings")
            .strong()
            .color(egui::Color32::WHITE);

        let button = egui::Button::new(label)
            .fill(accent_color())
            .stroke(egui::Stroke::new(
                1.0_f32,
                accent_color().linear_multiply(1.25_f32),
            ))
            .rounding(egui::Rounding::same(CONTROL_ROUNDING));

        let clicked = ui
            .add_enabled_ui(!self.busy, |ui| {
                ui.add_sized([SAVE_BUTTON_WIDTH, CONTROL_HEIGHT], button)
                    .on_hover_text(format!(
                        "Writes every field on this screen to {}, next to the launcher executable, so the same setup is restored the next time it starts. Settings are also saved automatically before every job.",
                        self.config_location
                    ))
                    .on_disabled_hover_text(LOCK_HINT)
                    .clicked()
            })
            .inner;

        if clicked {
            self.pending_save = true;
        }
    }

    fn draw_navigation_row(&mut self, ui: &mut egui::Ui) {
        let processes = self.adopted_processes.snapshot();
        let summary = ProcessSummary::from_processes(&processes);
        let compact = ui.available_width() < TELEMETRY_COMPACT_WIDTH;

        if compact {
            self.draw_tab_track(ui);
            if summary.total_count > 0 {
                ui.add_space(10.0_f32);
                self.draw_process_telemetry(ui, &summary);
            }
        } else {
            ui.horizontal(|ui| {
                self.draw_tab_track(ui);
                if summary.total_count > 0 {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        self.draw_process_telemetry(ui, &summary);
                    });
                }
            });
        }
    }

    fn draw_tab_track(&mut self, ui: &mut egui::Ui) {
        let border = ui.visuals().widgets.noninteractive.bg_stroke.color;
        let track_fill = ui.visuals().extreme_bg_color;
        let mut selected = self.active_tab;

        egui::Frame::none()
            .fill(track_fill)
            .stroke(egui::Stroke::new(1.0_f32, border))
            .rounding(egui::Rounding::same(TAB_TRACK_ROUNDING))
            .inner_margin(egui::Margin::same(TAB_TRACK_PADDING))
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.x = 4.0_f32;
                ui.horizontal(|ui| {
                    for spec in TAB_SPECS.iter() {
                        if pill_tab(ui, selected == spec.tab, spec.label, spec.tooltip) {
                            selected = spec.tab;
                        }
                    }
                });
            });

        self.active_tab = selected;
    }

    fn draw_process_telemetry(&mut self, ui: &mut egui::Ui, summary: &ProcessSummary) {
        let border = ui.visuals().widgets.noninteractive.bg_stroke.color;
        let fill = ui.visuals().faint_bg_color;

        let mut kill_requested = false;

        egui::Frame::none()
            .fill(fill)
            .stroke(egui::Stroke::new(1.0_f32, border))
            .rounding(egui::Rounding::same(TAB_TRACK_ROUNDING))
            .inner_margin(egui::Margin::symmetric(10.0_f32, TAB_TRACK_PADDING))
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.x = 8.0_f32;
                ui.horizontal(|ui| {
                    if summary.game_count > 0 {
                        process_chip(
                            ui,
                            "Game",
                            format!("PID {}", summary.game_pid),
                            color_game(),
                            "The S.T.A.L.K.E.R. Anomaly game process is running.",
                        );
                    }
                    if summary.mo2_count > 0 {
                        process_chip(
                            ui,
                            "MO2",
                            format!("PID {}", summary.mo2_pid),
                            color_mo2(),
                            "Mod Organizer 2 is running.",
                        );
                    }
                    if summary.runner_count > 0 {
                        process_chip(
                            ui,
                            "Wine",
                            format!("{} helper(s)", summary.runner_count),
                            color_warning(),
                            "Wine/Proton helper processes such as umu-run, wineserver and winedevice are running.",
                        );
                    }

                    if summary.total_memory_bytes > 0 {
                        draw_ram_readout(ui, summary.total_memory_bytes);
                    }

                    let (divider_rect, _) = ui.allocate_exact_size(
                        egui::vec2(1.0_f32, CONTROL_HEIGHT - 8.0_f32),
                        egui::Sense::hover(),
                    );
                    ui.painter()
                        .rect_filled(divider_rect, egui::Rounding::ZERO, border);

                    if kill_button(ui, summary.total_count) {
                        kill_requested = true;
                    }
                });
            });

        if kill_requested {
            self.spawn_kill_thread(ui.ctx());
        }
    }

    fn draw_process_badge(&mut self, ui: &mut egui::Ui) {
        let active = match self.process.clone() {
            Some(active) => active,
            None => return,
        };

        let time = ui.input(|input| input.time);
        let pulse = ((time * 2.4_f64).sin() * 0.5_f64 + 0.5_f64) as f32;
        let accent = color_running();

        ui.add_space(10.0_f32);

        egui::Frame::none()
            .fill(accent.linear_multiply(0.10_f32 + 0.10_f32 * pulse))
            .stroke(egui::Stroke::new(1.0_f32 + pulse, accent))
            .rounding(egui::Rounding::same(BADGE_ROUNDING))
            .inner_margin(egui::Margin::symmetric(14.0_f32, 7.0_f32))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
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

    fn spawn_kill_thread(&mut self, ctx: &egui::Context) {
        let registry = self.adopted_processes.clone();
        let reporter = Reporter::detached(self.background_tx.clone());
        let wake_ctx = ctx.clone();

        thread::spawn(move || {
            if let Err(error) = crate::process::terminate_all(&registry, &reporter) {
                reporter.warn(format!(
                    "[!] Could not fully terminate tracked processes: {error}"
                ));
            }
            wake_ctx.request_repaint();
        });

        self.push_log(
            LogLevel::Info,
            "[*] Sent termination signal to every tracked Game, Mod Organizer 2 and Wine/Proton helper process",
        );
    }
}

fn pill_tab(ui: &mut egui::Ui, active: bool, label: &str, tooltip: &str) -> bool {
    let text_color = if active {
        egui::Color32::WHITE
    } else {
        ui.visuals().widgets.inactive.fg_stroke.color
    };

    let font = egui::FontId::new(TAB_FONT_SIZE, egui::FontFamily::Proportional);
    let galley = ui
        .painter()
        .layout_no_wrap(label.to_owned(), font, text_color);

    let size = egui::vec2(
        galley.size().x + TAB_HORIZONTAL_PADDING * 2.0_f32,
        TAB_HEIGHT,
    );
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());

    if ui.is_rect_visible(rect) {
        let hovered = response.hovered();
        let painter = ui.painter();

        if active {
            painter.rect_filled(rect, egui::Rounding::same(TAB_ROUNDING), accent_color());

            let underline = egui::Rect::from_min_max(
                egui::pos2(
                    rect.left() + TAB_HORIZONTAL_PADDING,
                    rect.bottom() - TAB_UNDERLINE_HEIGHT - 3.0_f32,
                ),
                egui::pos2(
                    rect.right() - TAB_HORIZONTAL_PADDING,
                    rect.bottom() - 3.0_f32,
                ),
            );
            painter.rect_filled(
                underline,
                egui::Rounding::same(TAB_UNDERLINE_HEIGHT * 0.5_f32),
                egui::Color32::WHITE.linear_multiply(0.85_f32),
            );
        } else if hovered {
            painter.rect_filled(
                rect,
                egui::Rounding::same(TAB_ROUNDING),
                accent_color().linear_multiply(0.22_f32),
            );
        }

        let text_pos = egui::pos2(
            rect.center().x - galley.size().x * 0.5_f32,
            rect.center().y - galley.size().y * 0.5_f32 - if active { 2.0_f32 } else { 0.0_f32 },
        );
        let final_color = if active || !hovered {
            text_color
        } else {
            color_text_bright(ui)
        };
        painter.galley(text_pos, galley, final_color);
    }

    response
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text(tooltip)
        .clicked()
}

fn theme_segment(ui: &mut egui::Ui, label: &str, active: bool, tooltip: &str) -> bool {
    let text_color = if active {
        egui::Color32::WHITE
    } else {
        ui.visuals().widgets.inactive.fg_stroke.color
    };

    let font = egui::FontId::new(13.5_f32, egui::FontFamily::Proportional);
    let galley = ui
        .painter()
        .layout_no_wrap(label.to_owned(), font, text_color);

    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(THEME_SEGMENT_WIDTH, CONTROL_HEIGHT - 6.0_f32),
        egui::Sense::click(),
    );

    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        let rounding = egui::Rounding::same(CONTROL_ROUNDING - 2.0_f32);

        if active {
            painter.rect_filled(rect, rounding, accent_color());
        } else if response.hovered() {
            painter.rect_filled(rect, rounding, accent_color().linear_multiply(0.22_f32));
        }

        let text_pos = rect.center() - galley.size() * 0.5_f32;
        painter.galley(text_pos, galley, text_color);
    }

    response
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text(tooltip)
        .clicked()
        && !active
}

fn process_chip(
    ui: &mut egui::Ui,
    title: &str,
    detail: String,
    accent: egui::Color32,
    tooltip: &str,
) {
    let response = ui
        .horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0_f32;

            let (dot_rect, _) = ui.allocate_exact_size(
                egui::vec2(STATUS_DOT_RADIUS * 2.0_f32, STATUS_DOT_RADIUS * 2.0_f32),
                egui::Sense::hover(),
            );
            ui.painter()
                .circle_filled(dot_rect.center(), STATUS_DOT_RADIUS, accent);

            ui.label(egui::RichText::new(title).strong().color(accent));
            ui.label(egui::RichText::new(detail).monospace().small().weak());
        })
        .response;

    response.on_hover_text(tooltip);
}

fn draw_ram_readout(ui: &mut egui::Ui, total_memory_bytes: u64) {
    let response = ui
        .horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0_f32;
            ui.label(egui::RichText::new("RAM").small().weak());
            ui.label(
                egui::RichText::new(format_total_ram(total_memory_bytes))
                    .monospace()
                    .strong(),
            );
        })
        .response;

    response.on_hover_text("Sum of resident memory (VmRSS) across every tracked Game, Mod Organizer 2 and Wine/Proton helper process. Shared library pages counted by more than one process can inflate this beyond the true delta shown in htop or ps.");
}

fn kill_button(ui: &mut egui::Ui, total: usize) -> bool {
    let danger = color_danger();

    let label = egui::RichText::new(format!("Terminate all ({total})"))
        .strong()
        .color(egui::Color32::WHITE);

    let button = egui::Button::new(label)
        .fill(danger.linear_multiply(0.85_f32))
        .stroke(egui::Stroke::new(1.0_f32, danger))
        .rounding(egui::Rounding::same(CONTROL_ROUNDING));

    ui.add_enabled_ui(total > 0, |ui| {
        ui.add_sized([150.0_f32, CONTROL_HEIGHT], button)
            .on_hover_text("Sends a graceful shutdown to every tracked Game, Mod Organizer 2 and Wine/Proton helper process, escalating to a forced kill after a grace period.")
            .on_disabled_hover_text("Nothing to kill: no tracked processes are currently active.")
            .clicked()
    })
    .inner
}

fn format_total_ram(bytes: u64) -> String {
    const MB: f64 = 1024.0_f64 * 1024.0_f64;
    const GB: f64 = MB * 1024.0_f64;
    let bytes = bytes as f64;
    if bytes >= GB {
        format!("{:.1} GB", bytes / GB)
    } else {
        format!("{:.1} MB", bytes / MB)
    }
}

fn shorten_middle(value: &str, max_chars: usize) -> String {
    let chars: Vec<char> = value.chars().collect();
    if chars.len() <= max_chars || max_chars < 5 {
        return value.to_string();
    }

    let keep = max_chars - 3;
    let head = keep / 2 + keep % 2;
    let tail = keep / 2;

    let mut shortened: String = chars[..head].iter().collect();
    shortened.push_str("...");
    shortened.extend(chars[chars.len() - tail..].iter());
    shortened
}
