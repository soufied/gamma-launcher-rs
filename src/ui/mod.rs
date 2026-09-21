pub mod app;
pub mod job;

use app::LauncherApp;

pub fn run() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1180.0_f32, 780.0_f32])
            .with_min_inner_size([900.0_f32, 600.0_f32])
            .with_title("Gamma Launcher"),
        ..Default::default()
    };

    eframe::run_native(
        "Gamma Launcher",
        options,
        Box::new(|cc| Ok(Box::new(LauncherApp::new(cc)))),
    )
}
