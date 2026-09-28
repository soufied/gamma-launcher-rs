use crate::config::expand_path;
use crate::ui::app::{
    color_detect, color_ok, color_pending, color_missing, color_text_muted, draw_badge, section,
    LauncherApp, PathCheck, ACTION_SIZE, LOCK_HINT, ROW_SPACING,
};
use std::path::{Path, PathBuf};

struct PathBadgeState {
    text: &'static str,
    muted: bool,
    color: Option<egui::Color32>,
    tooltip: String,
}

fn badge_state(value: &str, check: PathCheck) -> PathBadgeState {
    let trimmed = value.trim();

    if trimmed.is_empty() {
        return match check {
            PathCheck::OptionalDirectory => PathBadgeState {
                text: "○ OPTIONAL",
                muted: true,
                color: None,
                tooltip: "Leave this empty to use the default location.".to_string(),
            },
            _ => PathBadgeState {
                text: "✖ NOT SET",
                muted: false,
                color: Some(color_missing()),
                tooltip: "This path is required but has not been configured yet. Fill it in or run Auto-Detect Paths.".to_string(),
            },
        };
    }

    let expanded = expand_path(Path::new(trimmed));
    let exists = match check {
        PathCheck::File => expanded.is_file(),
        _ => expanded.is_dir(),
    };

    if exists {
        return PathBadgeState {
            text: "✔ FOUND",
            muted: false,
            color: Some(color_ok()),
            tooltip: "This path exists on disk.".to_string(),
        };
    }

    if let PathCheck::DirectoryAutoCreate = check {
        return PathBadgeState {
            text: "◐ WILL CREATE",
            muted: false,
            color: Some(color_pending()),
            tooltip: "This prefix does not exist yet. Proton or Wine will create it automatically the first time you launch.".to_string(),
        };
    }

    PathBadgeState {
        text: "⚠ MISSING",
        muted: false,
        color: Some(color_missing()),
        tooltip: "This path does not exist yet. Fix it manually or run Auto-Detect Paths.".to_string(),
    }
}

fn draw_path_badge(ui: &mut egui::Ui, value: &str, check: PathCheck) {
    let state = badge_state(value, check);

    if state.muted {
        ui.label(
            egui::RichText::new(state.text)
                .color(color_text_muted(ui))
                .monospace()
                .size(13.0_f32),
        )
        .on_hover_text(state.tooltip);
        return;
    }

    ui.add(egui::Label::new(
        egui::RichText::new(state.text)
            .color(state.color.unwrap_or_else(|| color_text_muted(ui)))
            .strong()
            .monospace()
            .size(13.0_f32),
    ))
    .on_hover_text(state.tooltip);
}

fn path_row(
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

    egui::Frame::none()
        .inner_margin(egui::Margin::symmetric(0.0_f32, 6.0_f32))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(label).strong());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    draw_path_badge(ui, value.as_str(), check);
                });
            });

            ui.add_space(4.0_f32);

            ui.horizontal(|ui| {
                if ui
                    .add_sized([84.0_f32, 26.0_f32], egui::Button::new("Browse"))
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
        });

    ui.add_space(ROW_SPACING - 6.0_f32);
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
    path_row(
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
    path_row(
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

impl LauncherApp {
    pub(crate) fn draw_paths(&mut self, ui: &mut egui::Ui, locked: bool) {
        let enabled = !locked;
        ui.add_space(6.0_f32);

        self.draw_paths_summary(ui);

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

    fn draw_paths_summary(&self, ui: &mut egui::Ui) {
        let required = [
            (self.anomaly_input.as_str(), PathCheck::Directory),
            (self.gamma_input.as_str(), PathCheck::Directory),
            (self.prefix_input.as_str(), PathCheck::DirectoryAutoCreate),
            (self.proton_input.as_str(), PathCheck::Directory),
        ];

        let mut resolved = 0usize;
        for (value, check) in required {
            let state = badge_state(value, check);
            if state.text.starts_with('✔') || state.text.starts_with('◐') {
                resolved += 1;
            }
        }

        let all_resolved = resolved == required.len();

        egui::Frame::none()
            .fill(ui.visuals().faint_bg_color)
            .stroke(egui::Stroke::new(
                1.0_f32,
                ui.visuals().widgets.noninteractive.bg_stroke.color,
            ))
            .rounding(egui::Rounding::same(8.0_f32))
            .inner_margin(egui::Margin::symmetric(14.0_f32, 10.0_f32))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    draw_badge(
                        ui,
                        all_resolved,
                        "ALL REQUIRED PATHS RESOLVED",
                        "SOME REQUIRED PATHS ARE MISSING",
                        "Anomaly, Organizer / mods, Wine prefix and Proton are the paths needed before installing or launching anything.",
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            egui::RichText::new(format!("{resolved}/{} required paths ready", required.len()))
                                .color(color_text_muted(ui))
                                .small(),
                        );
                    });
                });
            });

        ui.add_space(ROW_SPACING);
    }
}
