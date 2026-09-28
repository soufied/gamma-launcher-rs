use crate::config::CloseAction;
use crate::ui::app::{
    accent_color, collapsible_section, color_danger, color_detect, color_fill_alternate,
    color_mo2, color_text_normal, draw_badge, proxy_preview, text_field_row, toggle_row,
    LauncherApp, ROW_SPACING,
};
use crate::steam_identity;

impl LauncherApp {
    pub(crate) fn draw_tweaks(&mut self, ui: &mut egui::Ui, locked: bool) {
        let enabled = !locked;
        ui.add_space(6.0_f32);

        self.draw_pipeline_options_card(ui, enabled);
        self.draw_proxy_card(ui, enabled);
        self.draw_multiplayer_spacewar_section(ui, enabled);
        self.draw_desktop_tray_card(ui, enabled);
    }

    fn draw_pipeline_options_card(&mut self, ui: &mut egui::Ui, enabled: bool) {
        collapsible_section(
            ui,
            "pipeline_options",
            "Engine & Pipeline",
            "Controls what Sync / Update patches, verifies and cleans up, plus the modpack source it pulls from.",
            accent_color(),
            true,
            |ui| {
                ui.add_enabled_ui(enabled, |ui| {
                    if toggle_row(
                        ui,
                        &mut self.config.update_gamma_definition,
                        "Update the gamma definition",
                        "Re-downloads the modpack definition before installing. Default: ON.",
                        "Re-downloads the modpack definition repository before installing, so the newest mod list and versions are used. Recommended: keep enabled. Default: ON.",
                    ) {
                        self.pending_save = true;
                    }
                    if toggle_row(
                        ui,
                        &mut self.config.patch_anomaly,
                        "Patch the Anomaly directory",
                        "Applies GAMMA-specific patches on top of a vanilla Anomaly install. Default: ON.",
                        "Applies the GAMMA-specific patches on top of a vanilla Anomaly install. Required for GAMMA to work correctly. Default: ON.",
                    ) {
                        self.pending_save = true;
                    }
                    if toggle_row(
                        ui,
                        &mut self.config.preserve_user_config,
                        "Preserve user.ltx when patching",
                        "Keeps your existing graphics and input settings instead of overwriting them. Default: OFF.",
                        "Keeps your existing graphics and input settings file instead of overwriting it while patching Anomaly. Enable only if you hand-tuned user.ltx. Default: OFF.",
                    ) {
                        self.pending_save = true;
                    }
                    if toggle_row(
                        ui,
                        &mut self.config.anomaly_verify,
                        "Verify Anomaly after installation",
                        "Runs an MD5 pass right after installing, to catch corrupted downloads early. Default: ON.",
                        "Runs an MD5 pass over Anomaly's files right after installing, to catch corrupted downloads before they turn into crashes. Recommended: keep enabled. Default: ON.",
                    ) {
                        self.pending_save = true;
                    }
                    if toggle_row(
                        ui,
                        &mut self.config.anomaly_purge_cache,
                        "Delete the Anomaly archive after install",
                        "Frees several gigabytes once installation succeeds. Default: ON.",
                        "Removes the downloaded Anomaly installer archive once installation succeeds, freeing several gigabytes of disk space. Default: ON.",
                    ) {
                        self.pending_save = true;
                    }
                    if toggle_row(
                        ui,
                        &mut self.config.install_mod_organizer,
                        "Install MO2",
                        "Installs or updates Mod Organizer 2 as part of GAMMA setup. Default: OFF.",
                        "Installs or updates Mod Organizer 2 as part of GAMMA setup. Leave disabled if you manage MO2 yourself or already have it installed. Default: OFF.",
                    ) {
                        self.pending_save = true;
                    }
                    if toggle_row(
                        ui,
                        &mut self.config.update_download_cache,
                        "Redownload mismatching archives on MD5 check",
                        "Fixes bad checksums automatically instead of only reporting them. Default: ON.",
                        "When Check MD5 finds a mod archive whose checksum does not match, redownload it automatically instead of only reporting it. Default: ON.",
                    ) {
                        self.pending_save = true;
                    }
                    if toggle_row(
                        ui,
                        &mut self.config.purge_unused_downloads,
                        "Purge unused downloads after MD5 check",
                        "Deletes archives no longer referenced by the current modpack definition. Default: ON.",
                        "Deletes archives in the download cache that are no longer referenced by the current modpack definition. Default: ON.",
                    ) {
                        self.pending_save = true;
                    }
                    if toggle_row(
                        ui,
                        &mut self.config.force_recheck,
                        "Force full recheck on Sync / Update",
                        "Reverifies and reinstalls every mod. Much slower. Default: OFF.",
                        "Ignores the skip-if-already-installed shortcut and reverifies and reinstalls every mod during Sync / Update. Much slower; enable only when an install looks broken. Default: OFF.",
                    ) {
                        self.pending_save = true;
                    }
                    if toggle_row(
                        ui,
                        &mut self.config.runner.headless_mod_launch,
                        "Headless mod launch",
                        "Lets Launch Modded Game start through MO2 without showing its window. Default: ON.",
                        "Lets Launch Modded Game and the tray Launch Game entry start the game through Mod Organizer 2 without showing its window, so GAMMA and other mods are active. When OFF, the tray entry falls back to the direct binary, which skips MO2 and runs without mods. Default: ON.",
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
                    if text_field_row(
                        ui,
                        "MO2 shortcut title",
                        &mut self.mo2_shortcut_input,
                        "Anomaly (DX11-AVX)",
                        "Name of the executable inside Mod Organizer 2 that headless launch starts, passed as moshortcut://<title>. It must match the executable name shown in MO2 exactly, including spaces and parentheses, or MO2 will refuse to start. Default: Anomaly (DX11-AVX).",
                    ) {
                        self.pending_save = true;
                    }
                });
            },
        );
    }

    fn draw_proxy_card(&mut self, ui: &mut egui::Ui, enabled: bool) {
        collapsible_section(
            ui,
            "proxy_options",
            "SOCKS5 Proxy",
            "Routes downloads through a SOCKS5 proxy, either always or only after a run of consecutive failures.",
            color_fill_alternate(),
            false,
            |ui| {
                ui.add_enabled_ui(enabled, |ui| {
                    if toggle_row(
                        ui,
                        &mut self.config.proxy.enabled,
                        "Enable Proxy",
                        "Routes every download through the SOCKS5 proxy below. Default: OFF.",
                        "Routes every download and every ModDB or GitHub request through the SOCKS5 proxy configured below. Host names are resolved by the proxy itself, so DNS is tunnelled too. Useful when your connection blocks ModDB or throttles large downloads. Default: OFF.",
                    ) {
                        self.pending_save = true;
                    }
                    if toggle_row(
                        ui,
                        &mut self.config.proxy.socks5_auto_retry,
                        "Auto Enable on Retries",
                        "Switches a failing item to the proxy after enough consecutive failures. Default: OFF.",
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
                                .color(color_text_normal(ui))
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
            },
        );
    }

    fn draw_multiplayer_spacewar_section(&mut self, ui: &mut egui::Ui, enabled: bool) {
        collapsible_section(
            ui,
            "multiplayer_spacewar",
            "Multiplayer & Steam Integration (Spacewar)",
            "Spoofs the Spacewar AppID so Steam's multiplayer, voice and overlay plumbing works for Anomaly Together.",
            color_mo2(),
            false,
            |ui| {
                ui.add_enabled_ui(enabled, |ui| {
                    if toggle_row(
                        ui,
                        &mut self.config.spacewar.steam_spacewar_mode,
                        "Launch as Spacewar in Steam",
                        "Writes Steam identity files into the prefix on the next launch. Default: OFF.",
                        "Spoofs the Steam AppID 480 (Spacewar) so Steam's own multiplayer, voice and overlay plumbing works for a game that has no native Steam release. Writes steam_appid.txt, a Steam.exe stub, steam_settings and user.ltx identity files into the prefix on the next game launch. Default: OFF.",
                    ) {
                        self.pending_save = true;
                    }

                    ui.add_space(ROW_SPACING);

                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new("Multiplayer Callout / Nickname").strong());
                        let problems = steam_identity::validate_nickname(&self.nickname_input);
                        draw_badge(
                            ui,
                            problems.is_empty(),
                            "VALID",
                            "INVALID",
                            if problems.is_empty() {
                                "This nickname will be written into every Steam identity file and env var on the next Spacewar launch.".to_string()
                            } else {
                                problems.join("; ")
                            }
                            .as_str(),
                        );
                    });
                    if text_field_row(
                        ui,
                        "",
                        &mut self.nickname_input,
                        "Stalker",
                        "Name shown to other players and written into steam_settings, user.ltx and user.reg. Letters, digits, spaces, underscores and hyphens only, and cannot be \"steamuser\". Default: your system username, or \"Stalker\" if that isn't usable.",
                    ) {
                        self.pending_save = true;
                    }
                    if ui
                        .add_enabled(enabled, egui::Button::new("Use System Username"))
                        .on_hover_text("Fills the nickname above with $USER, or \"Stalker\" if $USER is empty or \"steamuser\".")
                        .clicked()
                    {
                        self.nickname_input = steam_identity::effective_nickname(&self.config);
                        self.pending_save = true;
                    }

                    ui.add_space(ROW_SPACING);

                    if toggle_row(
                        ui,
                        &mut self.config.spacewar.force_nickname_override,
                        "Force nickname override on every launch",
                        "Re-writes every identity file on every Spacewar launch. Default: ON.",
                        "Re-writes every identity file on every Spacewar launch, even if they already contain the configured nickname. Disable only if you edit these files by hand between launches. Default: ON.",
                    ) {
                        self.pending_save = true;
                    }
                    if toggle_row(
                        ui,
                        &mut self.config.spacewar.steam_check_running,
                        "Warn if Steam is not running",
                        "Shows a non-fatal warning before launch. Default: ON.",
                        "Shows a non-fatal warning before launch if the real Steam client does not appear to be running. The game still launches either way. Default: ON.",
                    ) {
                        self.pending_save = true;
                    }

                    ui.add_space(6.0_f32);
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new("Steam status:").strong());
                        draw_badge(
                            ui,
                            self.adopted_processes.is_steam_running(),
                            "RUNNING",
                            "NOT RUNNING",
                            "Whether the native Steam client is currently detected on this system. Refreshed roughly every 2 seconds.",
                        );
                    });

                    self.draw_diagnostics_panel(ui, enabled);
                });
            },
        );
    }

    fn draw_desktop_tray_card(&mut self, ui: &mut egui::Ui, enabled: bool) {
        collapsible_section(
            ui,
            "desktop_tray",
            "Desktop & Tray",
            "Window behavior for closing, minimizing and starting the launcher on KDE Plasma.",
            color_detect(),
            false,
            |ui| {
                ui.add_enabled_ui(enabled, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new("Close Button Action").strong());
                        for (action, label) in [
                            (CloseAction::Exit, CloseAction::Exit.label()),
                            (CloseAction::MinimizeToTray, CloseAction::MinimizeToTray.label()),
                        ] {
                            let tooltip = action.description();
                            if ui
                                .selectable_value(&mut self.config.tray.close_action, action, label)
                                .on_hover_text(tooltip)
                                .changed()
                            {
                                self.pending_save = true;
                            }
                        }
                    });
                    ui.label(
                        egui::RichText::new(self.config.tray.close_action.description())
                            .color(color_text_normal(ui))
                            .small(),
                    );
                    ui.add_space(ROW_SPACING);
                    if toggle_row(
                        ui,
                        &mut self.config.tray.minimize_to_tray,
                        "Minimize to tray",
                        "Minimizing sends the window to the tray instead of the taskbar. Default: ON.",
                        "Minimizing the window sends it to the system tray instead of the taskbar. Default: ON.",
                    ) {
                        self.pending_save = true;
                    }
                    if toggle_row(
                        ui,
                        &mut self.config.tray.start_in_tray,
                        "Start minimized to tray",
                        "The window stays hidden on startup. Takes effect next launch. Default: OFF.",
                        "The launcher window stays hidden when the application starts; use the tray icon's \"Open Launcher\" entry to show it. Takes effect the next time the launcher starts. Default: OFF.",
                    ) {
                        self.pending_save = true;
                    }
                    ui.add_space(4.0_f32);
                    ui.label(
                        egui::RichText::new("Exit performs a clean, immediate shutdown on KDE Plasma 6 / Wayland, avoiding orphaned taskbar entries.")
                            .color(color_danger())
                            .small(),
                    );
                });
            },
        );
    }
}
