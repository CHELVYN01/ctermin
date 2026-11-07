//! Shared look for the whole app: colors, rounding, spacing, and dialog helpers.

use eframe::egui::{self, Color32, CornerRadius, FontId, Margin, RichText, Stroke, TextStyle};

pub const ACCENT: Color32 = Color32::from_rgb(0x3b, 0x82, 0xf6);
const ACCENT_HOVER: Color32 = Color32::from_rgb(0x5a, 0x98, 0xf8);
const PANEL: Color32 = Color32::from_rgb(0x1e, 0x1e, 0x22);
const WIDGET: Color32 = Color32::from_rgb(0x2a, 0x2a, 0x30);
const WIDGET_HOVER: Color32 = Color32::from_rgb(0x36, 0x36, 0x3e);
const WIDGET_ACTIVE: Color32 = Color32::from_rgb(0x42, 0x42, 0x4c);
const FIELD: Color32 = Color32::from_rgb(0x14, 0x14, 0x17);
const BORDER: Color32 = Color32::from_rgb(0x3a, 0x3a, 0x42);
const RADIUS: u8 = 6;

/// Applies the app-wide style. Call once at startup.
pub fn apply(ctx: &egui::Context) {
    ctx.style_mut_of(egui::Theme::Dark, |style| {
        let radius = CornerRadius::same(RADIUS);
        let spacing = &mut style.spacing;
        spacing.item_spacing = egui::vec2(8.0, 6.0);
        spacing.button_padding = egui::vec2(12.0, 5.0);
        spacing.interact_size.y = 26.0;
        spacing.menu_margin = Margin::same(6);
        spacing.window_margin = Margin::same(16);
        spacing.combo_width = 120.0;

        style.text_styles.insert(
            TextStyle::Heading,
            FontId::new(18.0, egui::FontFamily::Proportional),
        );

        let visuals = &mut style.visuals;
        visuals.window_corner_radius = CornerRadius::same(10);
        visuals.menu_corner_radius = radius;
        visuals.window_fill = PANEL;
        visuals.panel_fill = PANEL;
        visuals.window_stroke = Stroke::new(1.0, BORDER);
        visuals.extreme_bg_color = FIELD;
        visuals.selection.bg_fill = ACCENT;
        visuals.selection.stroke = Stroke::new(1.0, Color32::WHITE);
        visuals.hyperlink_color = ACCENT_HOVER;
        visuals.popup_shadow.color = Color32::from_black_alpha(140);
        visuals.window_shadow.color = Color32::from_black_alpha(160);

        let widgets = &mut visuals.widgets;
        for (state, fill) in [
            (&mut widgets.inactive, WIDGET),
            (&mut widgets.hovered, WIDGET_HOVER),
            (&mut widgets.active, WIDGET_ACTIVE),
            (&mut widgets.open, WIDGET_HOVER),
        ] {
            state.corner_radius = radius;
            state.weak_bg_fill = fill;
            state.bg_fill = fill;
        }
        widgets.inactive.bg_stroke = Stroke::new(1.0, BORDER);
        widgets.hovered.bg_stroke = Stroke::new(1.0, ACCENT);
        widgets.active.bg_stroke = Stroke::new(1.0, ACCENT);
        widgets.noninteractive.corner_radius = radius;
    });
}

/// Section title inside a dialog.
pub fn title(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text).heading().strong());
    ui.add_space(4.0);
}

/// Muted explanation under a control.
pub fn hint(ui: &mut egui::Ui, text: impl Into<String>) {
    ui.label(RichText::new(text.into()).small().weak());
}

/// Filled call-to-action button.
pub fn primary_button(ui: &mut egui::Ui, text: &str) -> egui::Response {
    let button = egui::Button::new(RichText::new(text).strong().color(Color32::WHITE))
        .fill(ACCENT)
        .stroke(Stroke::NONE);
    ui.add(button)
}
