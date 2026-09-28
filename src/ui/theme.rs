pub(super) fn accent_color() -> egui::Color32 {
    egui::Color32::from_rgb(56, 142, 255)
}

pub(super) fn color_text_bright(ui: &egui::Ui) -> egui::Color32 {
    if ui.visuals().dark_mode {
        egui::Color32::from_rgb(244, 247, 252)
    } else {
        egui::Color32::from_rgb(15, 18, 24)
    }
}

pub(super) fn color_text_normal(ui: &egui::Ui) -> egui::Color32 {
    if ui.visuals().dark_mode {
        egui::Color32::from_rgb(214, 219, 226)
    } else {
        egui::Color32::from_rgb(38, 43, 51)
    }
}

pub(super) fn color_track() -> egui::Color32 {
    egui::Color32::from_rgb(26, 29, 36)
}

pub(super) fn color_fill() -> egui::Color32 {
    egui::Color32::from_rgb(56, 142, 255)
}

pub(super) fn color_fill_alternate() -> egui::Color32 {
    egui::Color32::from_rgb(0, 172, 209)
}

pub(super) fn color_warning() -> egui::Color32 {
    egui::Color32::from_rgb(232, 168, 30)
}

pub(super) fn color_danger() -> egui::Color32 {
    egui::Color32::from_rgb(230, 70, 70)
}

pub(super) fn color_success() -> egui::Color32 {
    egui::Color32::from_rgb(88, 196, 114)
}

pub(super) fn color_mo2() -> egui::Color32 {
    egui::Color32::from_rgb(61, 174, 233)
}

pub(super) fn color_launcher() -> egui::Color32 {
    egui::Color32::from_rgb(233, 154, 62)
}

pub(super) fn color_game() -> egui::Color32 {
    egui::Color32::from_rgb(84, 188, 126)
}

pub(super) fn color_detect() -> egui::Color32 {
    egui::Color32::from_rgb(120, 130, 226)
}

pub(super) fn color_ok() -> egui::Color32 {
    egui::Color32::from_rgb(88, 196, 122)
}

pub(super) fn color_missing() -> egui::Color32 {
    color_danger()
}

pub(super) fn color_pending() -> egui::Color32 {
    color_warning()
}

pub(super) fn color_running() -> egui::Color32 {
    color_fill_alternate()
}

pub(super) fn color_running_text(ui: &egui::Ui) -> egui::Color32 {
    if ui.visuals().dark_mode {
        color_running()
    } else {
        egui::Color32::from_rgb(0, 98, 120)
    }
}

pub(super) fn color_fill_alternate_text(ui: &egui::Ui) -> egui::Color32 {
    if ui.visuals().dark_mode {
        color_fill_alternate()
    } else {
        egui::Color32::from_rgb(0, 98, 120)
    }
}

pub(super) fn color_text_muted(ui: &egui::Ui) -> egui::Color32 {
    if ui.visuals().dark_mode {
        egui::Color32::from_rgb(158, 166, 180)
    } else {
        egui::Color32::from_rgb(90, 97, 108)
    }
}

pub(super) fn apply_theme(ctx: &egui::Context, dark: bool) {
    let mut visuals = if dark {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    };

    if dark {
        visuals.panel_fill = egui::Color32::from_rgb(18, 21, 27);
        visuals.window_fill = egui::Color32::from_rgb(24, 27, 34);
        visuals.extreme_bg_color = color_track();
        visuals.faint_bg_color = egui::Color32::from_rgb(30, 34, 42);
        visuals.selection.bg_fill = accent_color();
        visuals.selection.stroke = egui::Stroke::new(1.0_f32, egui::Color32::from_rgb(244, 247, 252));
        visuals.hyperlink_color = color_fill_alternate();
        visuals.warn_fg_color = color_warning();
        visuals.error_fg_color = color_danger();
        visuals.window_stroke = egui::Stroke::new(1.0_f32, egui::Color32::from_rgb(60, 67, 80));
        visuals.code_bg_color = egui::Color32::from_rgb(30, 34, 42);

        let border = egui::Color32::from_rgb(68, 75, 89);
        let text = egui::Color32::from_rgb(214, 219, 226);
        let text_bright = egui::Color32::from_rgb(244, 247, 252);

        visuals.widgets.noninteractive.bg_fill = egui::Color32::from_rgb(30, 34, 42);
        visuals.widgets.noninteractive.weak_bg_fill = egui::Color32::from_rgb(30, 34, 42);
        visuals.widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0_f32, border);
        visuals.widgets.noninteractive.fg_stroke = egui::Stroke::new(1.0_f32, text);

        visuals.widgets.inactive.bg_fill = egui::Color32::from_rgb(40, 45, 55);
        visuals.widgets.inactive.weak_bg_fill = egui::Color32::from_rgb(40, 45, 55);
        visuals.widgets.inactive.bg_stroke = egui::Stroke::new(1.0_f32, border);
        visuals.widgets.inactive.fg_stroke = egui::Stroke::new(1.0_f32, text);

        visuals.widgets.hovered.bg_fill = egui::Color32::from_rgb(54, 62, 76);
        visuals.widgets.hovered.weak_bg_fill = egui::Color32::from_rgb(54, 62, 76);
        visuals.widgets.hovered.bg_stroke = egui::Stroke::new(1.5_f32, color_fill());
        visuals.widgets.hovered.fg_stroke = egui::Stroke::new(1.5_f32, text_bright);

        visuals.widgets.active.bg_fill = color_fill();
        visuals.widgets.active.weak_bg_fill = color_fill();
        visuals.widgets.active.bg_stroke = egui::Stroke::new(1.0_f32, text_bright);
        visuals.widgets.active.fg_stroke = egui::Stroke::new(1.5_f32, egui::Color32::WHITE);

        visuals.widgets.open.bg_fill = egui::Color32::from_rgb(40, 45, 55);
        visuals.widgets.open.weak_bg_fill = egui::Color32::from_rgb(40, 45, 55);
        visuals.widgets.open.bg_stroke = egui::Stroke::new(1.0_f32, border);
        visuals.widgets.open.fg_stroke = egui::Stroke::new(1.0_f32, text_bright);
    } else {
        visuals.panel_fill = egui::Color32::from_rgb(242, 244, 248);
        visuals.window_fill = egui::Color32::from_rgb(255, 255, 255);
        visuals.extreme_bg_color = egui::Color32::from_rgb(226, 229, 236);
        visuals.faint_bg_color = egui::Color32::from_rgb(230, 233, 239);
        visuals.selection.bg_fill = accent_color();
        visuals.selection.stroke = egui::Stroke::new(1.0_f32, egui::Color32::from_rgb(15, 18, 24));
        visuals.hyperlink_color = egui::Color32::from_rgb(0, 90, 156);
        visuals.warn_fg_color = egui::Color32::from_rgb(133, 88, 0);
        visuals.error_fg_color = egui::Color32::from_rgb(168, 24, 24);
        visuals.window_stroke = egui::Stroke::new(1.0_f32, egui::Color32::from_rgb(170, 177, 188));
        visuals.code_bg_color = egui::Color32::from_rgb(226, 229, 236);

        let border = egui::Color32::from_rgb(150, 158, 170);
        let text = egui::Color32::from_rgb(32, 37, 45);
        let text_bright = egui::Color32::from_rgb(12, 15, 20);

        visuals.widgets.noninteractive.bg_fill = egui::Color32::from_rgb(232, 234, 239);
        visuals.widgets.noninteractive.weak_bg_fill = egui::Color32::from_rgb(232, 234, 239);
        visuals.widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0_f32, border);
        visuals.widgets.noninteractive.fg_stroke = egui::Stroke::new(1.0_f32, text);

        visuals.widgets.inactive.bg_fill = egui::Color32::from_rgb(220, 224, 231);
        visuals.widgets.inactive.weak_bg_fill = egui::Color32::from_rgb(220, 224, 231);
        visuals.widgets.inactive.bg_stroke = egui::Stroke::new(1.0_f32, border);
        visuals.widgets.inactive.fg_stroke = egui::Stroke::new(1.0_f32, text);

        visuals.widgets.hovered.bg_fill = egui::Color32::from_rgb(200, 209, 222);
        visuals.widgets.hovered.weak_bg_fill = egui::Color32::from_rgb(200, 209, 222);
        visuals.widgets.hovered.bg_stroke = egui::Stroke::new(1.5_f32, color_fill());
        visuals.widgets.hovered.fg_stroke = egui::Stroke::new(1.5_f32, text_bright);

        visuals.widgets.active.bg_fill = color_fill();
        visuals.widgets.active.weak_bg_fill = color_fill();
        visuals.widgets.active.bg_stroke = egui::Stroke::new(1.0_f32, color_fill().linear_multiply(0.6_f32));
        visuals.widgets.active.fg_stroke = egui::Stroke::new(1.5_f32, egui::Color32::WHITE);

        visuals.widgets.open.bg_fill = egui::Color32::from_rgb(220, 224, 231);
        visuals.widgets.open.weak_bg_fill = egui::Color32::from_rgb(220, 224, 231);
        visuals.widgets.open.bg_stroke = egui::Stroke::new(1.0_f32, border);
        visuals.widgets.open.fg_stroke = egui::Stroke::new(1.0_f32, text_bright);
    }

    let rounding = egui::Rounding::same(super::style::WIDGET_ROUNDING);
    visuals.widgets.noninteractive.rounding = rounding;
    visuals.widgets.inactive.rounding = rounding;
    visuals.widgets.hovered.rounding = rounding;
    visuals.widgets.active.rounding = rounding;
    visuals.widgets.open.rounding = rounding;

    ctx.set_visuals(visuals);
    super::style::apply_style(ctx);
}
