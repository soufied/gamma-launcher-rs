pub mod app;
pub mod diagnostics;
pub mod header;
pub mod job;
pub mod style;
pub mod tabs;
pub mod theme;

use app::LauncherApp;
use std::sync::mpsc;
use std::time::Duration;

pub fn run() -> eframe::Result<()> {
    let config = crate::config::AppConfig::load();
    let adopted_processes = crate::process::new_registry();
    let (tray_tx, tray_rx) = mpsc::channel::<crate::tray::TrayCommand>();

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1180.0_f32, 780.0_f32])
            .with_min_inner_size([900.0_f32, 600.0_f32])
            .with_title("Gamma Launcher")
            .with_visible(!config.tray.start_in_tray),
        ..Default::default()
    };

    eframe::run_native(
        "Gamma Launcher",
        options,
        Box::new(move |cc| {
            let scanner_registry = adopted_processes.clone();
            let scanner_ctx = cc.egui_ctx.clone();
            let wake = crate::runner::Waker::new(move || scanner_ctx.request_repaint());
            let _scanner_handle = crate::process::spawn_background_scanner(
                scanner_registry,
                wake,
                Duration::from_secs(2),
            );

            let tray_handle = match crate::tray::spawn(tray_tx.clone()) {
                Ok(handle) => Some(handle),
                Err(error) => {
                    eprintln!("[!] System tray unavailable: {error}");
                    None
                }
            };

            Ok(Box::new(LauncherApp::new(
                cc,
                adopted_processes,
                tray_handle,
                tray_rx,
            )))
        }),
    )
}
