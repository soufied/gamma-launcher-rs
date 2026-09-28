use crate::commands::KeymapLayout;
use crate::config::{expand_path, SPACEWAR_APPID};
use crate::report::LogLevel;
use crate::runner::{self, LaunchTarget};
use crate::steam_identity::effective_nickname;
use crate::ui::app::{
    accent_color, card_action, color_detect, color_game, color_launcher, color_missing, color_mo2,
    color_ok, color_pending, color_running, color_running_text, color_text_bright, color_text_muted,
    color_warning,
    estimate_row, maintenance_card, section, short_path, LauncherApp, BADGE_ROUNDING,
    CARD_ROUNDING, LOCK_HINT, MEDIUM_LAYOUT_WIDTH, ROW_SPACING, SECTION_MARGIN, SECTION_ROUNDING,
    WIDE_LAYOUT_WIDTH,
};
use crate::ui::job::Job;

const PANEL_TITLE_SIZE: f32 = 17.0_f32;
const PANEL_SUBTITLE_SIZE: f32 = 12.5_f32;
const TILE_TITLE_SIZE: f32 = 15.0_f32;
const TILE_MARGIN: f32 = 14.0_f32;
const TILE_ROUNDING: f32 = 10.0_f32;
const TILE_SPACING: f32 = 12.0_f32;
const HERO_BUTTON_HEIGHT: f32 = 54.0_f32;
const HERO_MARGIN: f32 = 18.0_f32;
const HERO_ROUNDING: f32 = 12.0_f32;
const HERO_BUTTON_MIN_WIDTH: f32 = 210.0_f32;
const HERO_STACK_WIDTH: f32 = 720.0_f32;
const LAUNCH_BUTTON_HEIGHT: f32 = 44.0_f32;
const ACTION_ROW_HEIGHT: f32 = 52.0_f32;
const ACTION_ROW_ROUNDING: f32 = 8.0_f32;
const ACTION_ROW_PADDING: f32 = 12.0_f32;
const ACTION_ACCENT_WIDTH: f32 = 4.0_f32;
const ACTION_TITLE_SIZE: f32 = 14.5_f32;
const ACTION_DESCRIPTION_SIZE: f32 = 12.0_f32;
const CHIP_HEIGHT: f32 = 26.0_f32;
const CHIP_ROUNDING: f32 = 13.0_f32;
const CHIP_DOT_RADIUS: f32 = 3.5_f32;
const TILE_TRIPLE_WIDTH: f32 = 900.0_f32;
const STATUS_CARD_ROUNDING: f32 = 12.0_f32;
const STATUS_CARD_MARGIN: f32 = 16.0_f32;
const STATUS_BADGE_HEIGHT: f32 = 30.0_f32;
const STATUS_BADGE_ROUNDING: f32 = 15.0_f32;
const STATUS_BADGE_DOT_RADIUS: f32 = 4.0_f32;
const STATUS_BADGE_GAP: f32 = 10.0_f32;
const STATUS_BADGE_VALUE_MAX_CHARS: usize = 30;
const STATUS_BADGE_STACK_WIDTH: f32 = 620.0_f32;
const STATUS_LABEL_SIZE: f32 = 11.5_f32;
const STATUS_VALUE_SIZE: f32 = 13.0_f32;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Readiness {
    Ready,
    Attention,
    Missing,
    Running,
}

impl Readiness {
    fn color(self) -> egui::Color32 {
        match self {
            Readiness::Ready => color_ok(),
            Readiness::Attention => color_pending(),
            Readiness::Missing => color_missing(),
            Readiness::Running => color_running(),
        }
    }
}

struct ActionSpec {
    label: &'static str,
    description: &'static str,
    tooltip: &'static str,
    accent: egui::Color32,
    job: Job,
}

struct LaunchSpec {
    target: LaunchTarget,
    title: &'static str,
    caption: &'static str,
    tooltip: &'static str,
    accent: egui::Color32,
}

fn mo2_spec() -> LaunchSpec {
    LaunchSpec {
        target: LaunchTarget::ModOrganizer,
        title: "Mod Organizer 2",
        caption: "Recommended for GAMMA. Activates the virtual mod list, then runs the game.",
        tooltip: "Starts Mod Organizer 2 through the configured Proton build. This is the normal way to launch GAMMA, since MO2 activates the virtual mod list before the game runs.",
        accent: color_mo2(),
    }
}

fn launcher_spec() -> LaunchSpec {
    LaunchSpec {
        target: LaunchTarget::Launcher,
        title: "Anomaly Launcher",
        caption: "Vanilla launcher. Mods are not active.",
        tooltip: "Starts the vanilla AnomalyLauncher.exe directly, bypassing Mod Organizer 2. Use this only to verify Anomaly itself, since mods will not be active.",
        accent: color_launcher(),
    }
}

pub const DIRECT_LAUNCH_CAPTION: &str = "Direct binary. Skips launcher and MO2. Mods are not active.";

fn game_spec() -> LaunchSpec {
    LaunchSpec {
        target: LaunchTarget::Game,
        title: "Anomaly Game",
        caption: DIRECT_LAUNCH_CAPTION,
        tooltip: "Starts the Anomaly game binary directly, skipping both the launcher and Mod Organizer 2. Mods are not active. To play with GAMMA, use Launch Modded Game, which starts the game through Mod Organizer 2 without showing its window.",
        accent: color_game(),
    }
}

fn modded_spec() -> LaunchSpec {
    LaunchSpec {
        target: LaunchTarget::ModdedGame,
        title: "Modded Game",
        caption: "Headless. Mod Organizer 2 starts the game with the configuration currently active in MO2.",
        tooltip: "Runs ModOrganizer.exe with a moshortcut:// argument so Mod Organizer 2 activates the profile that is currently selected inside MO2 (its virtual mod list, load order and user.ltx), then starts the game without opening the MO2 window. The shortcut title is set on the Tweaks tab and must match an executable configured inside Mod Organizer 2. Intended for standalone modpacks (e.g. Redux) or custom builds. G.A.M.M.A. will likely fail via direct launch because it requires MO2's VFS.",
        accent: color_mo2(),
    }
}

fn install_actions() -> [ActionSpec; 2] {
    [
        ActionSpec {
            label: "Anomaly Install",
            description: "Clean S.T.A.L.K.E.R. Anomaly 1.5.3",
            tooltip: "Downloads and installs a clean copy of S.T.A.L.K.E.R. Anomaly 1.5.3 into the configured Anomaly directory. Run it only when that folder is empty or broken. Irreversible: it overwrites the files it ships.",
            accent: color_launcher(),
            job: Job::AnomalyInstall,
        },
        ActionSpec {
            label: "GAMMA Setup",
            description: "Base setup, MO2 and folders",
            tooltip: "Downloads the GAMMA base setup, installs Mod Organizer 2 when that option is enabled, and prepares downloads/ and mods/ next to it. Run it once before the first Sync / Update. Irreversible: it overwrites the setup files it ships.",
            accent: color_mo2(),
            job: Job::GammaSetup,
        },
    ]
}

fn sync_actions() -> [ActionSpec; 1] {
    [ActionSpec {
        label: "Sync / Update",
        description: "Full pipeline: download, verify, install",
        tooltip: "Runs the full GAMMA pipeline: downloads, verifies and installs Anomaly and the modpack according to the Install options and Tweaks tabs. Run it for the first install and after every GAMMA definition update. Reversible only by reinstalling, but already valid mods are skipped.",
        accent: accent_color(),
        job: Job::FullInstall,
    }]
}

fn verify_actions() -> [ActionSpec; 2] {
    [
        ActionSpec {
            label: "Check MD5",
            description: "Compare archives to checksums",
            tooltip: "Compares every downloaded mod archive against its expected checksum. Run it when installs fail in odd ways or after a disk problem. Read-only by itself, but the Install options tab can let it redownload or purge mismatching archives.",
            accent: color_detect(),
            job: Job::CheckMd5,
        },
        ActionSpec {
            label: "Test Mod Maker",
            description: "Validate the modpack definition",
            tooltip: "Parses the modpack definition and reports malformed entries without touching your install. Run it when a mod refuses to install or before reporting a bug. Completely read-only.",
            accent: color_detect(),
            job: Job::TestModMaker,
        },
    ]
}

impl LauncherApp {
    pub(crate) fn draw_dashboard(&mut self, ui: &mut egui::Ui, locked: bool) {
        let enabled = !locked;

        ui.add_space(6.0_f32);
        self.draw_launch_section(ui, enabled);
        self.draw_pipeline_section(ui, enabled);
        self.draw_maintenance_section(ui, enabled);
    }

    fn draw_launch_section(&mut self, ui: &mut egui::Ui, enabled: bool) {
        let mo2 = mo2_spec();
        let modded = modded_spec();
        let launcher = launcher_spec();
        let game = game_spec();

        let ready_count = [&mo2, &launcher, &game]
            .iter()
            .filter(|spec| runner::resolve_status(&self.config, spec.target).is_some())
            .count();

        let border = ui.visuals().widgets.noninteractive.bg_stroke.color;
        let fill = ui.visuals().faint_bg_color;

        egui::Frame::none()
            .fill(fill)
            .stroke(egui::Stroke::new(1.0_f32, border))
            .rounding(egui::Rounding::same(SECTION_ROUNDING))
            .inner_margin(egui::Margin::same(SECTION_MARGIN))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());

                panel_header(
                    ui,
                    "Launch",
                    "Start GAMMA through Mod Organizer 2, or run the vanilla pieces on their own.",
                    |ui| {
                        readiness_summary(ui, ready_count, 3);
                    },
                );

                ui.add_space(ROW_SPACING);
                self.draw_hero_launch(ui, enabled, &mo2);
                ui.add_space(TILE_SPACING);

                if ui.available_width() >= TILE_TRIPLE_WIDTH {
                    ui.columns(3, |columns| {
                        self.draw_launch_tile(&mut columns[0], enabled, &modded);
                        self.draw_launch_tile(&mut columns[1], enabled, &launcher);
                        self.draw_launch_tile(&mut columns[2], enabled, &game);
                    });
                } else {
                    self.draw_launch_tile(ui, enabled, &modded);
                    ui.add_space(TILE_SPACING);
                    self.draw_launch_tile(ui, enabled, &launcher);
                    ui.add_space(TILE_SPACING);
                    self.draw_launch_tile(ui, enabled, &game);
                }

                ui.add_space(TILE_SPACING);
                self.draw_runtime_status_card(ui);
            });

        ui.add_space(ROW_SPACING);
    }

    fn draw_hero_launch(&mut self, ui: &mut egui::Ui, enabled: bool, spec: &LaunchSpec) {
        let resolved = runner::resolve_status(&self.config, spec.target);
        let readiness = self.launch_readiness(spec.target, resolved.is_some());
        let stacked = ui.available_width() < HERO_STACK_WIDTH;

        let mut clicked = false;

        egui::Frame::none()
            .fill(spec.accent.linear_multiply(0.10_f32))
            .stroke(egui::Stroke::new(
                1.5_f32,
                spec.accent.linear_multiply(0.75_f32),
            ))
            .rounding(egui::Rounding::same(HERO_ROUNDING))
            .inner_margin(egui::Margin::same(HERO_MARGIN))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());

                if stacked {
                    hero_description(ui, spec, readiness, resolved.as_ref());
                    ui.add_space(ROW_SPACING);
                    let full_width = ui.available_width();
                    clicked = hero_button(ui, enabled && resolved.is_some(), spec, full_width);
                } else {
                    ui.horizontal(|ui| {
                        let button_width = HERO_BUTTON_MIN_WIDTH;
                        let text_width =
                            (ui.available_width() - button_width - ui.spacing().item_spacing.x)
                                .max(120.0_f32);

                        ui.allocate_ui_with_layout(
                            egui::vec2(text_width, HERO_BUTTON_HEIGHT + 20.0_f32),
                            egui::Layout::top_down(egui::Align::Min),
                            |ui| {
                                hero_description(ui, spec, readiness, resolved.as_ref());
                            },
                        );

                        ui.with_layout(
                            egui::Layout::right_to_left(egui::Align::Center),
                            |ui| {
                                clicked = hero_button(
                                    ui,
                                    enabled && resolved.is_some(),
                                    spec,
                                    button_width,
                                );
                            },
                        );
                    });
                }
            });

        if clicked {
            self.pending_job = Some(Job::Launch(spec.target));
        }
    }

    fn draw_launch_tile(&mut self, ui: &mut egui::Ui, enabled: bool, spec: &LaunchSpec) {
        let resolved = runner::resolve_status(&self.config, spec.target);
        let readiness = self.launch_readiness(spec.target, resolved.is_some());
        let launchable = resolved.is_some() && readiness != Readiness::Missing;

        let border = ui.visuals().widgets.noninteractive.bg_stroke.color;
        let mut clicked = false;

        egui::Frame::none()
            .fill(ui.visuals().extreme_bg_color)
            .stroke(egui::Stroke::new(1.0_f32, border))
            .rounding(egui::Rounding::same(TILE_ROUNDING))
            .inner_margin(egui::Margin::same(TILE_MARGIN))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());

                ui.horizontal(|ui| {
                    accent_bar(ui, spec.accent, 20.0_f32);
                    ui.label(
                        egui::RichText::new(spec.title)
                            .strong()
                            .size(TILE_TITLE_SIZE)
                            .color(color_text_bright(ui)),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        readiness_chip(ui, readiness);
                    });
                });

                ui.add_space(4.0_f32);
                ui.label(egui::RichText::new(self.launch_caption(spec)).weak().small());
                ui.add_space(8.0_f32);
                executable_line(ui, resolved.as_ref());
                ui.add_space(10.0_f32);

                let button = egui::Button::new(
                    egui::RichText::new(format!("Launch {}", short_title(spec.title)))
                        .color(egui::Color32::WHITE)
                        .strong()
                        .size(15.0_f32),
                )
                .fill(spec.accent.linear_multiply(0.85_f32))
                .stroke(egui::Stroke::new(1.0_f32, spec.accent))
                .rounding(egui::Rounding::same(CARD_ROUNDING + 2.0_f32))
                .min_size(egui::vec2(ui.available_width(), LAUNCH_BUTTON_HEIGHT));

                clicked = ui
                    .add_enabled(enabled && launchable, button)
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .on_hover_text(spec.tooltip)
                    .on_disabled_hover_text(disabled_launch_hint(enabled, launchable))
                    .clicked();
            });

        if clicked {
            self.pending_job = Some(Job::Launch(spec.target));
        }
    }

    fn launch_caption(&self, spec: &LaunchSpec) -> &'static str {
        match spec.target {
            LaunchTarget::Game => DIRECT_LAUNCH_CAPTION,
            LaunchTarget::ModdedGame if !self.config.runner.headless_mod_launch => {
                "Disabled. Enable headless mod launch on the Tweaks tab."
            }
            LaunchTarget::ModdedGame if runner::resolve_status(&self.config, spec.target).is_none() => {
                "Mod Organizer 2 was not found. Use the direct binary instead. Mods are not active."
            }
            _ => spec.caption,
        }
    }

    fn launch_readiness(&self, target: LaunchTarget, resolved: bool) -> Readiness {
        if let Some(active) = self.process.as_ref() {
            if active.label == target.label() {
                return Readiness::Running;
            }
        }

        if target == LaunchTarget::ModdedGame && !self.config.runner.headless_mod_launch {
            return Readiness::Missing;
        }

        if resolved {
            Readiness::Ready
        } else {
            Readiness::Missing
        }
    }

    fn draw_runtime_status_card(&self, ui: &mut egui::Ui) {
        let border = ui.visuals().widgets.noninteractive.bg_stroke.color;
        let fill = ui.visuals().extreme_bg_color;

        let gamemode_active = self.config.runner.use_gamemode;
        let mangohud_active = self.config.graphics.enable_mangohud;

        let runner_badge = self.runtime_runner_badge();
        let prefix_badge = self.runtime_prefix_badge();
        let identity_badge = self.runtime_identity_badge();

        egui::Frame::none()
            .fill(fill)
            .stroke(egui::Stroke::new(1.0_f32, border))
            .rounding(egui::Rounding::same(STATUS_CARD_ROUNDING))
            .inner_margin(egui::Margin::same(STATUS_CARD_MARGIN))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());

                ui.horizontal(|ui| {
                    accent_bar(ui, accent_color(), 18.0_f32);
                    ui.label(
                        egui::RichText::new("Runtime Status")
                            .strong()
                            .size(TILE_TITLE_SIZE)
                            .color(color_text_bright(ui)),
                    );
                });
                ui.add_space(2.0_f32);
                ui.label(
                    egui::RichText::new(
                        "What Play will actually use right now, at a glance.",
                    )
                    .weak()
                    .small(),
                );
                ui.add_space(10.0_f32);

                let stacked = ui.available_width() < STATUS_BADGE_STACK_WIDTH;

                let draw_badges = |ui: &mut egui::Ui| {
                    toggle_badge(
                        ui,
                        "GameMode",
                        gamemode_active,
                        "Active",
                        "Inactive",
                        "Wraps launches in gamemoderun when it is installed and enabled. Toggle it on the Runtime tab.",
                    );
                    toggle_badge(
                        ui,
                        "MangoHud",
                        mangohud_active,
                        "Enabled",
                        "Disabled",
                        "Overlays FPS and hardware stats over the game when mangohud is installed and enabled. Toggle it on the Runtime tab.",
                    );
                    status_badge(
                        ui,
                        "Runner",
                        &runner_badge.0,
                        runner_badge.1.color(),
                        &runner_badge.2,
                    );
                    status_badge(
                        ui,
                        "Wine Prefix",
                        &prefix_badge.0,
                        prefix_badge.1.color(),
                        &prefix_badge.2,
                    );
                    status_badge(
                        ui,
                        "Steam Identity",
                        &identity_badge.0,
                        identity_badge.1.color(),
                        &identity_badge.2,
                    );
                };

                if stacked {
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = STATUS_BADGE_GAP;
                        draw_badges(ui);
                    });
                } else {
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing =
                            egui::vec2(STATUS_BADGE_GAP, STATUS_BADGE_GAP);
                        draw_badges(ui);
                    });
                }
            });
    }

    fn runtime_runner_badge(&self) -> (String, Readiness, String) {
        if self.config.runner.use_umu {
            let umu_id_text = if self.config.runner.umu_id.trim().is_empty() {
                "default".to_string()
            } else {
                self.config.runner.umu_id.trim().to_string()
            };
            (
                format!("umu-run ({umu_id_text})"),
                Readiness::Ready,
                "Launches go through umu-run, which pulls the Proton build and Steam Linux Runtime it needs by itself.".to_string(),
            )
        } else {
            match self.config.runner.proton_path.as_ref() {
                Some(path) => {
                    let expanded = expand_path(path);
                    if expanded.exists() {
                        (
                            short_path(&expanded),
                            Readiness::Ready,
                            format!("Proton build in use: {}", expanded.display()),
                        )
                    } else {
                        (
                            short_path(&expanded),
                            Readiness::Missing,
                            format!(
                                "The configured Proton build does not exist: {}",
                                expanded.display()
                            ),
                        )
                    }
                }
                None => (
                    "not set".to_string(),
                    Readiness::Missing,
                    "UMU is off and no Proton build is configured. Set one on the Paths tab."
                        .to_string(),
                ),
            }
        }
    }

    fn runtime_prefix_badge(&self) -> (String, Readiness, String) {
        match self.config.runner.wine_prefix.as_ref() {
            Some(path) => {
                let expanded = expand_path(path);
                if expanded.is_dir() {
                    (
                        short_path(&expanded),
                        Readiness::Ready,
                        format!(
                            "The wine prefix exists and will be reused: {}",
                            expanded.display()
                        ),
                    )
                } else {
                    (
                        short_path(&expanded),
                        Readiness::Attention,
                        format!(
                            "This prefix does not exist yet. Proton or Wine creates it automatically on the first launch: {}",
                            expanded.display()
                        ),
                    )
                }
            }
            None => (
                "not set".to_string(),
                Readiness::Missing,
                "No wine prefix is configured. Set one on the Paths tab or run Auto-Detect Paths."
                    .to_string(),
            ),
        }
    }

    fn runtime_identity_badge(&self) -> (String, Readiness, String) {
        if self.config.spacewar.steam_spacewar_mode {
            let persona = effective_nickname(&self.config);
            (
                format!("Spacewar ({persona})"),
                Readiness::Ready,
                format!(
                    "Steamworks / Spacewar identity (AppID {SPACEWAR_APPID}) is active. Persona name in use: {persona}."
                ),
            )
        } else {
            (
                "off".to_string(),
                Readiness::Attention,
                format!(
                    "Steamworks / Spacewar identity (AppID {SPACEWAR_APPID}) is not active. Enable it on the Runtime tab if the game needs a Steam identity to run."
                ),
            )
        }
    }

    fn draw_pipeline_section(&mut self, ui: &mut egui::Ui, enabled: bool) {
        let border = ui.visuals().widgets.noninteractive.bg_stroke.color;
        let fill = ui.visuals().faint_bg_color;

        egui::Frame::none()
            .fill(fill)
            .stroke(egui::Stroke::new(1.0_f32, border))
            .rounding(egui::Rounding::same(SECTION_ROUNDING))
            .inner_margin(egui::Margin::same(SECTION_MARGIN))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());

                panel_header(
                    ui,
                    "Install & Sync",
                    "Everything that downloads or installs content. These jobs need network access.",
                    |ui| {
                        network_badge(ui);
                    },
                );

                ui.add_space(ROW_SPACING);

                let width = ui.available_width();

                if width >= WIDE_LAYOUT_WIDTH {
                    ui.columns(3, |columns| {
                        self.draw_install_group(&mut columns[0], enabled);
                        self.draw_sync_group(&mut columns[1], enabled);
                        self.draw_verify_group(&mut columns[2], enabled);
                    });
                } else if width >= MEDIUM_LAYOUT_WIDTH {
                    ui.columns(2, |columns| {
                        self.draw_install_group(&mut columns[0], enabled);
                        self.draw_sync_group(&mut columns[1], enabled);
                    });
                    ui.add_space(TILE_SPACING);
                    self.draw_verify_group(ui, enabled);
                } else {
                    self.draw_install_group(ui, enabled);
                    ui.add_space(TILE_SPACING);
                    self.draw_sync_group(ui, enabled);
                    ui.add_space(TILE_SPACING);
                    self.draw_verify_group(ui, enabled);
                }
            });

        ui.add_space(ROW_SPACING);
    }

    fn draw_install_group(&mut self, ui: &mut egui::Ui, enabled: bool) {
        let actions = install_actions();
        self.draw_action_group(
            ui,
            enabled,
            "Install",
            "Fresh copies of Anomaly and the GAMMA base setup.",
            color_launcher(),
            &actions,
        );
    }

    fn draw_sync_group(&mut self, ui: &mut egui::Ui, enabled: bool) {
        let actions = sync_actions();
        self.draw_action_group(
            ui,
            enabled,
            "Sync & Update",
            "Bring the whole modpack in line with the current definition.",
            accent_color(),
            &actions,
        );
    }

    fn draw_verify_group(&mut self, ui: &mut egui::Ui, enabled: bool) {
        let actions = verify_actions();
        self.draw_action_group(
            ui,
            enabled,
            "Verify",
            "Check downloads and the modpack definition. Nothing is installed.",
            color_detect(),
            &actions,
        );
    }

    fn draw_action_group(
        &mut self,
        ui: &mut egui::Ui,
        enabled: bool,
        title: &str,
        subtitle: &str,
        accent: egui::Color32,
        actions: &[ActionSpec],
    ) {
        let mut chosen: Option<Job> = None;

        egui::Frame::none()
            .fill(accent.linear_multiply(0.07_f32))
            .stroke(egui::Stroke::new(1.0_f32, accent.linear_multiply(0.55_f32)))
            .rounding(egui::Rounding::same(TILE_ROUNDING))
            .inner_margin(egui::Margin::same(TILE_MARGIN))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());

                ui.horizontal(|ui| {
                    accent_bar(ui, accent, 18.0_f32);
                    ui.label(
                        egui::RichText::new(title)
                            .strong()
                            .size(TILE_TITLE_SIZE)
                            .color(accent),
                    );
                });
                ui.add_space(2.0_f32);
                ui.label(egui::RichText::new(subtitle).weak().small());
                ui.add_space(10.0_f32);

                for action in actions {
                    if action_row(ui, enabled, action) {
                        chosen = Some(action.job);
                    }
                    ui.add_space(6.0_f32);
                }
            });

        if let Some(job) = chosen {
            self.pending_job = Some(job);
        }
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
                            .color(color_running_text(ui))
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
                    "🔄  Sync ModOrganizer.ini",
                    "Rewrites the game path, profile, base, downloads, mods, profiles and overwrite directories in ModOrganizer.ini from the paths configured here, translating them into forward-slash Z: drive paths that Qt's INI engine never mangles. Every other key, including your Nexus settings, is preserved. Run it after moving a folder or when MO2 cannot find Anomaly. REVERSIBLE: the previous file is saved as ModOrganizer.ini.bak.",
                    None,
                ) {
                    self.pending_job = Some(Job::SyncModOrganizerIni);
                }

                if card_action(
                    ui,
                    enabled,
                    "✔  Repair MO2 Wine Paths",
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

fn panel_header(
    ui: &mut egui::Ui,
    title: &str,
    subtitle: &str,
    add_trailing: impl FnOnce(&mut egui::Ui),
) {
    ui.horizontal(|ui| {
        accent_bar(ui, accent_color(), 30.0_f32);

        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 2.0_f32;
            ui.label(
                egui::RichText::new(title)
                    .strong()
                    .size(PANEL_TITLE_SIZE)
                    .color(color_text_bright(ui)),
            );
            ui.label(
                egui::RichText::new(subtitle)
                    .size(PANEL_SUBTITLE_SIZE)
                    .weak(),
            );
        });

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            add_trailing(ui);
        });
    });
}

fn accent_bar(ui: &mut egui::Ui, color: egui::Color32, height: f32) {
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(ACTION_ACCENT_WIDTH, height),
        egui::Sense::hover(),
    );
    ui.painter().rect_filled(
        rect,
        egui::Rounding::same(ACTION_ACCENT_WIDTH * 0.5_f32),
        color,
    );
}

fn readiness_summary(ui: &mut egui::Ui, ready: usize, total: usize) {
    let tone = if ready == total {
        Readiness::Ready
    } else if ready == 0 {
        Readiness::Missing
    } else {
        Readiness::Attention
    };

    let color = tone.color();

    let response = egui::Frame::none()
        .fill(color.linear_multiply(0.14_f32))
        .stroke(egui::Stroke::new(1.0_f32, color.linear_multiply(0.7_f32)))
        .rounding(egui::Rounding::same(BADGE_ROUNDING))
        .inner_margin(egui::Margin::symmetric(12.0_f32, 5.0_f32))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0_f32;
                paint_dot(ui, color, CHIP_DOT_RADIUS);
                ui.label(
                    egui::RichText::new(format!("{ready} of {total} targets ready"))
                        .strong()
                        .color(color),
                );
            });
        })
        .response;

    response.on_hover_text(
        "How many of Mod Organizer 2, the Anomaly Launcher and the game executable were located on disk. Missing ones can be set on the Paths tab or found with Auto-Detect Paths.",
    );
}

fn network_badge(ui: &mut egui::Ui) {
    let color = color_warning();

    let response = egui::Frame::none()
        .fill(color.linear_multiply(0.12_f32))
        .stroke(egui::Stroke::new(1.0_f32, color.linear_multiply(0.6_f32)))
        .rounding(egui::Rounding::same(BADGE_ROUNDING))
        .inner_margin(egui::Margin::symmetric(12.0_f32, 5.0_f32))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0_f32;
                paint_dot(ui, color, CHIP_DOT_RADIUS);
                ui.label(egui::RichText::new("Network required").strong().color(color));
            });
        })
        .response;

    response.on_hover_text(
        "Every action in this panel downloads data. The SOCKS5 proxy from the Tweaks tab is applied when it is enabled.",
    );
}

fn paint_dot(ui: &mut egui::Ui, color: egui::Color32, radius: f32) {
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(radius * 2.0_f32, radius * 2.0_f32),
        egui::Sense::hover(),
    );
    ui.painter().circle_filled(rect.center(), radius, color);
}

fn readiness_chip(ui: &mut egui::Ui, readiness: Readiness) {
    let (text, tooltip) = match readiness {
        Readiness::Ready => (
            "Ready",
            "This executable was located and is ready to launch.",
        ),
        Readiness::Running => (
            "Running",
            "This target is running right now. Launch controls unlock when it exits.",
        ),
        Readiness::Attention => (
            "Check",
            "This target needs attention before it can be launched.",
        ),
        Readiness::Missing => (
            "Missing",
            "No executable could be resolved yet. Set it on the Paths tab or run Auto-Detect Paths.",
        ),
    };

    let color = readiness.color();

    let response = egui::Frame::none()
        .fill(color.linear_multiply(0.16_f32))
        .stroke(egui::Stroke::new(1.0_f32, color.linear_multiply(0.7_f32)))
        .rounding(egui::Rounding::same(CHIP_ROUNDING))
        .inner_margin(egui::Margin::symmetric(10.0_f32, 2.0_f32))
        .show(ui, |ui| {
            ui.set_min_height(CHIP_HEIGHT - 4.0_f32);
            ui.horizontal_centered(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0_f32;
                paint_dot(ui, color, CHIP_DOT_RADIUS);
                ui.label(egui::RichText::new(text).strong().small().color(color));
            });
        })
        .response;

    response.on_hover_text(tooltip);
}

fn toggle_badge(
    ui: &mut egui::Ui,
    title: &str,
    active: bool,
    active_label: &str,
    inactive_label: &str,
    tooltip: &str,
) {
    let color = if active { color_ok() } else { color_text_muted(ui) };
    let value = if active { active_label } else { inactive_label };

    let response = egui::Frame::none()
        .fill(color.linear_multiply(if active { 0.16_f32 } else { 0.10_f32 }))
        .stroke(egui::Stroke::new(1.0_f32, color.linear_multiply(0.7_f32)))
        .rounding(egui::Rounding::same(STATUS_BADGE_ROUNDING))
        .inner_margin(egui::Margin::symmetric(12.0_f32, 6.0_f32))
        .show(ui, |ui| {
            ui.set_min_height(STATUS_BADGE_HEIGHT - 8.0_f32);
            ui.horizontal_centered(|ui| {
                ui.spacing_mut().item_spacing.x = 7.0_f32;
                paint_dot(ui, color, STATUS_BADGE_DOT_RADIUS);
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 0.0_f32;
                    ui.label(
                        egui::RichText::new(title)
                            .size(STATUS_LABEL_SIZE)
                            .weak(),
                    );
                    ui.label(
                        egui::RichText::new(value)
                            .size(STATUS_VALUE_SIZE)
                            .strong()
                            .color(color),
                    );
                });
            });
        })
        .response;

    response.on_hover_text(tooltip);
}

fn status_badge(ui: &mut egui::Ui, title: &str, value: &str, color: egui::Color32, tooltip: &str) {
    let response = egui::Frame::none()
        .fill(color.linear_multiply(0.10_f32))
        .stroke(egui::Stroke::new(1.0_f32, color.linear_multiply(0.7_f32)))
        .rounding(egui::Rounding::same(STATUS_BADGE_ROUNDING))
        .inner_margin(egui::Margin::symmetric(12.0_f32, 6.0_f32))
        .show(ui, |ui| {
            ui.set_min_height(STATUS_BADGE_HEIGHT - 8.0_f32);
            ui.horizontal_centered(|ui| {
                ui.spacing_mut().item_spacing.x = 7.0_f32;
                paint_dot(ui, color, STATUS_BADGE_DOT_RADIUS);
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 0.0_f32;
                    ui.label(
                        egui::RichText::new(title)
                            .size(STATUS_LABEL_SIZE)
                            .weak(),
                    );
                    ui.label(
                        egui::RichText::new(shorten_end(value, STATUS_BADGE_VALUE_MAX_CHARS))
                            .size(STATUS_VALUE_SIZE)
                            .strong()
                            .monospace()
                            .color(color),
                    );
                });
            });
        })
        .response;

    response.on_hover_text(tooltip);
}

fn executable_line(ui: &mut egui::Ui, resolved: Option<&std::path::PathBuf>) {
    match resolved {
        Some(path) => {
            ui.label(
                egui::RichText::new(short_path(path))
                    .monospace()
                    .small()
                    .weak(),
            )
            .on_hover_text(path.display().to_string());
        }
        None => {
            ui.label(
                egui::RichText::new("No executable resolved")
                    .monospace()
                    .small()
                    .color(color_missing()),
            )
            .on_hover_text(
                "No executable could be resolved yet. Set it on the Paths tab or run Auto-Detect Paths.",
            );
        }
    }
}

fn hero_description(
    ui: &mut egui::Ui,
    spec: &LaunchSpec,
    readiness: Readiness,
    resolved: Option<&std::path::PathBuf>,
) {
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(spec.title)
                .strong()
                .size(20.0_f32)
                .color(color_text_bright(ui)),
        );
        readiness_chip(ui, readiness);
    });
    ui.add_space(2.0_f32);
    ui.label(egui::RichText::new(spec.caption).size(13.5_f32).weak());
    ui.add_space(6.0_f32);
    executable_line(ui, resolved);
}

fn hero_button(ui: &mut egui::Ui, enabled: bool, spec: &LaunchSpec, width: f32) -> bool {
    let button = egui::Button::new(
        egui::RichText::new(format!("Launch {}", short_title(spec.title)))
            .color(egui::Color32::WHITE)
            .strong()
            .size(17.0_f32),
    )
    .fill(spec.accent)
    .stroke(egui::Stroke::new(
        1.5_f32,
        spec.accent.linear_multiply(1.3_f32),
    ))
    .rounding(egui::Rounding::same(HERO_ROUNDING - 2.0_f32))
    .min_size(egui::vec2(width, HERO_BUTTON_HEIGHT));

    let resolved = enabled;

    ui.add_enabled(resolved, button)
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text(spec.tooltip)
        .on_disabled_hover_text(disabled_launch_hint(true, resolved))
        .clicked()
}

fn short_title(title: &str) -> &str {
    match title {
        "Mod Organizer 2" => "MO2",
        "Anomaly Launcher" => "Launcher",
        "Anomaly Game" => "Game",
        other => other,
    }
}

fn disabled_launch_hint(unlocked: bool, resolved: bool) -> &'static str {
    if !unlocked {
        LOCK_HINT
    } else if !resolved {
        "No executable could be resolved for this target yet. Set it on the Paths tab or run Auto-Detect Paths. Without Mod Organizer 2, only the direct binary can be launched, and mods are not active."
    } else {
        LOCK_HINT
    }
}

fn action_row(ui: &mut egui::Ui, enabled: bool, action: &ActionSpec) -> bool {
    let width = ui.available_width();
    let sense = if enabled {
        egui::Sense::click()
    } else {
        egui::Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, ACTION_ROW_HEIGHT), sense);

    if ui.is_rect_visible(rect) {
        let hovered = response.hovered() && enabled;
        let pressed = response.is_pointer_button_down_on() && enabled;
        let visuals = ui.visuals().clone();
        let border = visuals.widgets.noninteractive.bg_stroke.color;

        let (fill, stroke_color) = if !enabled {
            (visuals.widgets.noninteractive.weak_bg_fill, border)
        } else if pressed {
            (
                action.accent.linear_multiply(0.42_f32),
                action.accent,
            )
        } else if hovered {
            (
                action.accent.linear_multiply(0.26_f32),
                action.accent,
            )
        } else {
            (visuals.widgets.inactive.weak_bg_fill, border)
        };

        let painter = ui.painter();
        painter.rect_filled(rect, egui::Rounding::same(ACTION_ROW_ROUNDING), fill);
        painter.rect_stroke(
            rect,
            egui::Rounding::same(ACTION_ROW_ROUNDING),
            egui::Stroke::new(1.0_f32, stroke_color),
        );

        let bar_color = if enabled {
            action.accent
        } else {
            action.accent.linear_multiply(0.3_f32)
        };
        let bar_rect = egui::Rect::from_min_max(
            egui::pos2(rect.left() + 8.0_f32, rect.top() + 12.0_f32),
            egui::pos2(
                rect.left() + 8.0_f32 + ACTION_ACCENT_WIDTH,
                rect.bottom() - 12.0_f32,
            ),
        );
        painter.rect_filled(
            bar_rect,
            egui::Rounding::same(ACTION_ACCENT_WIDTH * 0.5_f32),
            bar_color,
        );

        let text_color = if enabled {
            color_text_bright(ui)
        } else {
            color_text_muted(ui)
        };
        let description_color = if enabled {
            color_text_muted(ui)
        } else {
            color_text_muted(ui).linear_multiply(0.7_f32)
        };

        let text_left = bar_rect.right() + ACTION_ROW_PADDING;

        let title_galley = painter.layout_no_wrap(
            action.label.to_owned(),
            egui::FontId::new(ACTION_TITLE_SIZE, egui::FontFamily::Proportional),
            text_color,
        );
        let description_galley = painter.layout_no_wrap(
            action.description.to_owned(),
            egui::FontId::new(ACTION_DESCRIPTION_SIZE, egui::FontFamily::Proportional),
            description_color,
        );

        let block_height = title_galley.size().y + 2.0_f32 + description_galley.size().y;
        let block_top = rect.center().y - block_height * 0.5_f32;

        painter.galley(egui::pos2(text_left, block_top), title_galley, text_color);
        painter.galley(
            egui::pos2(text_left, block_top + block_height - description_galley.size().y),
            description_galley,
            description_color,
        );
    }

    let response = if enabled {
        response.on_hover_cursor(egui::CursorIcon::PointingHand)
    } else {
        response
    };

    response
        .on_hover_text(action.tooltip)
        .on_disabled_hover_text(LOCK_HINT)
        .clicked()
        && enabled
}

fn shorten_end(value: &str, max_chars: usize) -> String {
    let chars: Vec<char> = value.chars().collect();
    if chars.len() <= max_chars || max_chars < 4 {
        return value.to_string();
    }

    let keep = max_chars - 3;
    let mut shortened = String::from("...");
    shortened.extend(chars[chars.len() - keep..].iter());
    shortened
}
