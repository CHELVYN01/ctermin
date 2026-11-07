//! Draggable dividers between panes.

use eframe::egui::{self, Color32, CursorIcon, Rect, Sense, Stroke, StrokeKind};

use crate::layout::{Axis, Branch, Divider};

/// The drag area is wider than the line so it is easy to grab.
const GRAB_MARGIN: f32 = 3.0;
const DIVIDER_COLOR: Color32 = Color32::from_rgb(0x33, 0x33, 0x33);
const DIVIDER_ACTIVE_COLOR: Color32 = Color32::from_rgb(0x3a, 0x96, 0xdd);
const FOCUS_COLOR: Color32 = Color32::from_rgb(0x3a, 0x96, 0xdd);

/// The new ratio from a divider being dragged.
pub struct RatioChange {
    pub path: Vec<Branch>,
    pub ratio: f32,
}

pub fn show_dividers(ui: &mut egui::Ui, dividers: &[Divider]) -> Option<RatioChange> {
    let base = ui.scope_id().with("divider");
    let mut change = None;
    for divider in dividers {
        let grab = divider.rect.expand(GRAB_MARGIN);
        let id = base.with(&divider.path);
        let cursor = match divider.axis {
            Axis::Horizontal => CursorIcon::ResizeHorizontal,
            Axis::Vertical => CursorIcon::ResizeVertical,
        };
        let response = ui.interact(grab, id, Sense::drag()).on_hover_cursor(cursor);

        if response.dragged() {
            if let Some(pointer) = response.interact_pointer_pos() {
                change = Some(RatioChange {
                    path: divider.path.clone(),
                    ratio: divider.ratio_at(pointer),
                });
            }
        }

        let active = response.hovered() || response.dragged();
        let color = if active {
            DIVIDER_ACTIVE_COLOR
        } else {
            DIVIDER_COLOR
        };
        ui.painter().rect_filled(divider.rect, 0.0, color);
    }
    change
}

/// A thin outline marking the pane that receives the keyboard.
pub fn paint_focus(ui: &egui::Ui, rect: Rect) {
    let stroke = Stroke::new(1.0, FOCUS_COLOR);
    ui.painter()
        .rect_stroke(rect, 0.0, stroke, StrokeKind::Inside);
}
