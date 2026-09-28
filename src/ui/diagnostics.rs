use crate::diagnostics::{self, DiagnosticStatus};
use crate::ui::app::{color_missing, color_ok, color_pending, sized_action, LauncherApp, SECONDARY_ACTION_SIZE};

impl LauncherApp {
    pub(super) fn draw_diagnostics_panel(&mut self, ui: &mut egui::Ui, enabled: bool) {
        ui.add_space(8.0_f32);

        if sized_action(
            ui,
            enabled,
            SECONDARY_ACTION_SIZE,
            "Run Pre-Flight Checks",
            "Runs a read-only set of checks over the Steam Spacewar identity files, wine prefix, and graphics tooling, without changing anything on disk.",
        ) {
            self.last_diagnostics = Some(diagnostics::run(&self.config, &self.adopted_processes));
        }

        ui.add_space(8.0_f32);

        let report = match self.last_diagnostics.clone() {
            Some(report) => report,
            None => return,
        };

        for check in &report.checks {
            let (color, symbol) = match check.status {
                DiagnosticStatus::Pass => (color_ok(), "PASS"),
                DiagnosticStatus::Warn => (color_pending(), "WARN"),
                DiagnosticStatus::Fail => (color_missing(), "FAIL"),
            };

            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(symbol)
                        .color(color)
                        .strong()
                        .monospace(),
                );
                ui.label(egui::RichText::new(&check.label).strong());
            });
            ui.label(egui::RichText::new(&check.detail).weak().small());
            ui.add_space(6.0_f32);
        }
    }
}
