use crate::config::{detected_cpu_count, PresentMode, SyncMode};
use crate::ui::app::{
    check_row, env_var_editor, section, text_field_row, toggle_text_with_reset, LauncherApp,
    LOCK_HINT, ROW_SPACING,
};

impl LauncherApp {
    pub(crate) fn draw_runtime(&mut self, ui: &mut egui::Ui, locked: bool) {
        let enabled = !locked;
        ui.add_space(6.0_f32);

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

        self.draw_custom_env_section(ui, enabled);
        self.draw_frame_rate_section(ui, enabled);
        self.draw_shader_driver_cache_section(ui, enabled);
        self.draw_xray_wine_core_section(ui, enabled);
    }

    fn draw_custom_env_section(&mut self, ui: &mut egui::Ui, enabled: bool) {
        section(ui, "Custom Environment Variables", |ui| {
            ui.label(
                egui::RichText::new("Extra key/value pairs exported into every launch, applied after every toggle above and able to override them.")
                    .weak()
                    .small(),
            );
            ui.add_space(8.0_f32);
            ui.add_enabled_ui(enabled, |ui| {
                if env_var_editor(
                    ui,
                    &mut self.config.runner.extra_env,
                    &mut self.extra_env_key_input,
                    &mut self.extra_env_value_input,
                ) {
                    self.pending_save = true;
                }
            });
        });
    }

    fn draw_frame_rate_section(&mut self, ui: &mut egui::Ui, enabled: bool) {
        section(ui, "Frame Rate & Presentation", |ui| {
            ui.add_enabled_ui(enabled, |ui| {
                const PRESET_CAPS: [u32; 8] = [0, 60, 75, 120, 144, 145, 165, 240];
                let mut is_custom = !PRESET_CAPS.contains(&self.config.graphics.fps_limit);

                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("FPS Limit").strong());
                    egui::ComboBox::from_id_salt("fps_limit_combo")
                        .selected_text(if is_custom {
                            "Custom".to_string()
                        } else if self.config.graphics.fps_limit == 0 {
                            "Unlimited".to_string()
                        } else {
                            self.config.graphics.fps_limit.to_string()
                        })
                        .show_ui(ui, |ui| {
                            for cap in PRESET_CAPS {
                                let label = if cap == 0 {
                                    "Unlimited".to_string()
                                } else {
                                    cap.to_string()
                                };
                                if ui
                                    .selectable_value(&mut self.config.graphics.fps_limit, cap, label)
                                    .changed()
                                {
                                    self.pending_save = true;
                                }
                            }
                            if ui.selectable_label(is_custom, "Custom").clicked() && !is_custom {
                                self.config.graphics.fps_limit = 200;
                                is_custom = true;
                                self.pending_save = true;
                            }
                        })
                        .response
                        .on_hover_text("Caps DXVK_FRAME_RATE. \"Unlimited\" removes the cap entirely. Default: Unlimited.");

                    if is_custom {
                        if ui
                            .add(
                                egui::DragValue::new(&mut self.config.graphics.fps_limit)
                                    .speed(1.0_f64)
                                    .range(1..=1000),
                            )
                            .on_hover_text("Custom frame rate cap, in frames per second.")
                            .changed()
                        {
                            self.pending_save = true;
                        }
                    }
                });

                ui.add_space(ROW_SPACING);

                if check_row(
                    ui,
                    &mut self.config.graphics.dxvk_async,
                    "DXVK Async",
                    "Compiles Direct3D shaders on background threads instead of stalling the render thread, trading a small chance of a one-time visual hitch for far fewer traversal stutters. Default: ON.",
                ) {
                    self.pending_save = true;
                }
                if check_row(
                    ui,
                    &mut self.config.graphics.dxvk_state_cache,
                    "DXVK State Cache",
                    "Persists compiled pipeline state to disk between runs, so shader compilation stutter mostly disappears after the first playthrough of an area. Default: ON.",
                ) {
                    self.pending_save = true;
                }

                ui.add_space(ROW_SPACING);

                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("Vulkan Present Mode").strong());
                    egui::ComboBox::from_id_salt("present_mode_combo")
                        .selected_text(match self.config.graphics.vk_wsi_present_mode {
                            PresentMode::Mailbox => "Mailbox",
                            PresentMode::Immediate => "Immediate",
                            PresentMode::Fifo => "Fifo (V-Sync)",
                            PresentMode::RelaxedFifo => "Relaxed Fifo",
                        })
                        .show_ui(ui, |ui| {
                            for (mode, label) in [
                                (PresentMode::Mailbox, "Mailbox"),
                                (PresentMode::Immediate, "Immediate"),
                                (PresentMode::Fifo, "Fifo (V-Sync)"),
                                (PresentMode::RelaxedFifo, "Relaxed Fifo"),
                            ] {
                                if ui
                                    .selectable_value(&mut self.config.graphics.vk_wsi_present_mode, mode, label)
                                    .changed()
                                {
                                    self.pending_save = true;
                                }
                            }
                        })
                        .response
                        .on_hover_text("Sets MESA_VK_WSI_PRESENT_MODE. Mailbox avoids tearing without adding input latency on Mesa drivers; Fifo is traditional V-Sync; Immediate allows tearing for lowest latency. Default: Mailbox.");
                });

                ui.add_space(ROW_SPACING);

                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("Sync Mechanism").strong());
                    for (mode, label, tooltip) in [
                        (SyncMode::Fsync, "Fsync", "Kernel futex-based sync, the battle-tested CachyOS default. Recommended unless you have a specific reason to change it."),
                        (SyncMode::Esync, "Esync", "Eventfd-based sync, useful on kernels without Fsync support."),
                        (SyncMode::SystemDefault, "System Default", "Leaves WINEFSYNC/WINEESYNC/PROTON_NO_FSYNC/PROTON_NO_ESYNC unset, deferring entirely to Wine/Proton's own defaults."),
                    ] {
                        if ui
                            .selectable_value(&mut self.config.graphics.sync_mechanism, mode, label)
                            .on_hover_text(tooltip)
                            .changed()
                        {
                            self.pending_save = true;
                        }
                    }
                });
            });
        });
    }

    fn draw_shader_driver_cache_section(&mut self, ui: &mut egui::Ui, enabled: bool) {
        section(ui, "Shader & Driver Caches", |ui| {
            ui.add_enabled_ui(enabled, |ui| {
                if check_row(
                    ui,
                    &mut self.config.graphics.nvidia_shader_cache_optimization,
                    "NVIDIA Shader Cache Optimization",
                    "Sets __GL_SHADER_DISK_CACHE_SIZE to 10 GiB and disables automatic cleanup, so NVIDIA's driver-level shader cache survives across GAMMA updates and rarely needs to recompile from scratch. Default: ON.",
                ) {
                    self.pending_save = true;
                }
                if check_row(
                    ui,
                    &mut self.config.graphics.mesa_shader_cache_optimization,
                    "Mesa / RADV Shader Cache Optimization",
                    "Raises Mesa's on-disk shader cache limit to 10G and enables RADV_PERFTEST=gpl (graphics pipeline library) for faster pipeline compilation on AMD GPUs. Default: ON.",
                ) {
                    self.pending_save = true;
                }
                if check_row(
                    ui,
                    &mut self.config.graphics.enable_mangohud,
                    "MangoHud Overlay",
                    "Shows an in-game FPS/frametime/GPU overlay via MangoHud. Requires the mangohud package to be installed. Default: ON if mangohud is detected in PATH, otherwise OFF.",
                ) {
                    self.pending_save = true;
                }
            });
        });
    }

    fn draw_xray_wine_core_section(&mut self, ui: &mut egui::Ui, enabled: bool) {
        section(ui, "X-Ray & Wine Core", |ui| {
            ui.add_enabled_ui(enabled, |ui| {
                if check_row(
                    ui,
                    &mut self.config.graphics.wine_large_address_aware,
                    "Wine Large Address Aware",
                    "Sets WINE_LARGE_ADDRESS_AWARE=1, letting the 32-bit X-Ray engine address more than 2 GiB of memory. Strongly recommended for a heavily modded GAMMA install. Default: ON.",
                ) {
                    self.pending_save = true;
                }

                ui.add_space(ROW_SPACING);

                ui.label(egui::RichText::new("X-Ray DLL Overrides").strong());
                if text_field_row(
                    ui,
                    "",
                    &mut self.graphics_dll_overrides_input,
                    "openal32=n,b;d3dcompiler_47=n,b",
                    "Semicolon-separated 'dll=mode' pairs specific to X-Ray's audio and shader compiler DLLs. Merged with the general WINEDLLOVERRIDES in the Wine / Proton runtime section above by DLL name, so this never silently deletes MO2's USVFS overrides; entries here win on collision.",
                ) {
                    self.pending_save = true;
                }
                if ui
                    .add_enabled(enabled, egui::Button::new("Reset"))
                    .on_hover_text("Restores the recommended X-Ray DLL overrides: openal32=n,b;d3dcompiler_47=n,b.")
                    .clicked()
                {
                    self.config.graphics.reset_dll_overrides();
                    self.graphics_dll_overrides_input = self.config.graphics.dll_overrides.clone();
                    self.pending_save = true;
                }

                ui.add_space(ROW_SPACING);
                ui.label(
                    egui::RichText::new("GameMode is controlled from the Wine / Proton runtime section above and applies to every launch target.")
                        .weak()
                        .small(),
                );
            });
        });
    }
}
