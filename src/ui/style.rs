pub(super) const ACTION_SIZE: [f32; 2] = [180.0_f32, 46.0_f32];
pub(super) const SECONDARY_ACTION_SIZE: [f32; 2] = [188.0_f32, 38.0_f32];
pub(super) const BADGE_ROUNDING: f32 = 8.0_f32;
pub(super) const SECTION_ROUNDING: f32 = 10.0_f32;
pub(super) const WIDGET_ROUNDING: f32 = 6.0_f32;
pub(super) const SECTION_MARGIN: f32 = 16.0_f32;
pub(super) const ROW_SPACING: f32 = 12.0_f32;
pub(super) const CARD_ROUNDING: f32 = 6.0_f32;
pub(super) const CARD_MARGIN: f32 = 12.0_f32;
pub(super) const CARD_BUTTON_HEIGHT: f32 = 34.0_f32;
pub(super) const WIDE_LAYOUT_WIDTH: f32 = 1000.0_f32;
pub(super) const MEDIUM_LAYOUT_WIDTH: f32 = 660.0_f32;

pub(super) const LOCK_HINT: &str = "Locked while a job or an external process is running. Every control re-enables itself automatically as soon as that process exits.";

pub(super) const BASE_SPACING: f32 = 10.0_f32;
pub(super) const BUTTON_PADDING: egui::Vec2 = egui::vec2(12.0_f32, 8.0_f32);
pub(super) const INTERACT_HEIGHT: f32 = 30.0_f32;
pub(super) const HEADING_FONT_SIZE: f32 = 22.0_f32;
pub(super) const BODY_FONT_SIZE: f32 = 15.0_f32;
pub(super) const BUTTON_FONT_SIZE: f32 = 15.0_f32;
pub(super) const MONOSPACE_FONT_SIZE: f32 = 14.0_f32;

pub(super) fn apply_style(ctx: &egui::Context) {
    ctx.style_mut(|style| {
        style.spacing.item_spacing = egui::vec2(BASE_SPACING, BASE_SPACING);
        style.spacing.button_padding = BUTTON_PADDING;
        style.spacing.interact_size.y = INTERACT_HEIGHT;
        style.text_styles.insert(
            egui::TextStyle::Heading,
            egui::FontId::new(HEADING_FONT_SIZE, egui::FontFamily::Proportional),
        );
        style.text_styles.insert(
            egui::TextStyle::Body,
            egui::FontId::new(BODY_FONT_SIZE, egui::FontFamily::Proportional),
        );
        style.text_styles.insert(
            egui::TextStyle::Button,
            egui::FontId::new(BUTTON_FONT_SIZE, egui::FontFamily::Proportional),
        );
        style.text_styles.insert(
            egui::TextStyle::Monospace,
            egui::FontId::new(MONOSPACE_FONT_SIZE, egui::FontFamily::Monospace),
        );
    });
}
