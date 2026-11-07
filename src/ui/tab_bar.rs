//! Bilah tab di atas area terminal.

use eframe::egui::{
    self, pos2, vec2, Align2, Button, Color32, FontId, Painter, Popup, Rect, ScrollArea, Sense,
    Shape, Stroke,
};

use crate::layout::Axis;
use crate::ssh::model::ServerId;

pub const BAR_HEIGHT: f32 = 32.0;
const MAX_TAB_WIDTH: f32 = 200.0;
const MIN_TAB_WIDTH: f32 = 72.0;
const NEW_BUTTON_WIDTH: f32 = 32.0;
const DROPDOWN_WIDTH: f32 = 22.0;
const CHEVRON_RADIUS: f32 = 4.0;
const MENU_WIDTH: f32 = 220.0;
const MENU_MAX_HEIGHT: f32 = 320.0;
const CLOSE_BUTTON_SIZE: f32 = 18.0;
const TEXT_MARGIN: f32 = 10.0;
const FONT_SIZE: f32 = 13.0;

const BAR_COLOR: Color32 = Color32::from_rgb(0x1c, 0x1c, 0x1c);
const ACTIVE_COLOR: Color32 = Color32::from_rgb(0x0c, 0x0c, 0x0c);
const HOVER_COLOR: Color32 = Color32::from_rgb(0x2a, 0x2a, 0x2a);
const TEXT_COLOR: Color32 = Color32::from_rgb(0xe6, 0xe6, 0xe6);
const INACTIVE_TEXT_COLOR: Color32 = Color32::from_rgb(0x9a, 0x9a, 0x9a);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TabAction {
    Select(usize),
    Close(usize),
    New,
    Duplicate(usize),
    Split(usize, Axis),
    ClosePane(usize),
    Connect(ServerId),
}

/// Data satu tab untuk ditampilkan.
pub struct TabItem {
    pub title: String,
    pub pane_count: usize,
}

/// Server dalam satu folder untuk menu tab baru. `title` kosong berarti tanpa folder.
pub struct ServerGroup {
    pub title: Option<String>,
    pub servers: Vec<(ServerId, String)>,
}

/// Lebar tiap tab: mengecil saat tab bertambah, sampai batas minimum.
fn tab_width(bar_width: f32, count: usize) -> f32 {
    let available = bar_width - NEW_BUTTON_WIDTH - DROPDOWN_WIDTH;
    (available / count.max(1) as f32).clamp(MIN_TAB_WIDTH, MAX_TAB_WIDTH)
}

fn tab_rect(bar: Rect, width: f32, index: usize) -> Rect {
    let min = bar.min + vec2(index as f32 * width, 0.0);
    Rect::from_min_size(min, vec2(width, bar.height()))
}

fn close_rect(tab: Rect) -> Rect {
    let center = pos2(tab.right() - CLOSE_BUTTON_SIZE / 2.0 - 6.0, tab.center().y);
    Rect::from_center_size(center, egui::Vec2::splat(CLOSE_BUTTON_SIZE))
}

fn new_button_rect(bar: Rect, width: f32, count: usize) -> Rect {
    let right_limit = bar.right() - NEW_BUTTON_WIDTH - DROPDOWN_WIDTH;
    let left = (bar.left() + width * count as f32).min(right_limit);
    Rect::from_min_size(pos2(left, bar.top()), vec2(NEW_BUTTON_WIDTH, bar.height()))
}

fn dropdown_rect(new_button: Rect) -> Rect {
    Rect::from_min_size(
        new_button.right_top(),
        vec2(DROPDOWN_WIDTH, new_button.height()),
    )
}

/// Menu klik kanan. Aksinya berlaku untuk tab yang diklik, walaupun tab itu tidak aktif.
fn tab_menu(ui: &mut egui::Ui, index: usize, pane_count: usize) -> Option<TabAction> {
    let mut action = None;
    if ui.button("Duplikat tab").clicked() {
        action = Some(TabAction::Duplicate(index));
    }
    ui.separator();
    let split_right = Button::new("Split kanan").shortcut_text("Ctrl+Shift+D");
    if ui.add(split_right).clicked() {
        action = Some(TabAction::Split(index, Axis::Horizontal));
    }
    let split_down = Button::new("Split bawah").shortcut_text("Ctrl+Shift+E");
    if ui.add(split_down).clicked() {
        action = Some(TabAction::Split(index, Axis::Vertical));
    }
    let close_split = Button::new("Tutup split").shortcut_text("Ctrl+W");
    if ui.add_enabled(pane_count > 1, close_split).clicked() {
        action = Some(TabAction::ClosePane(index));
    }
    ui.separator();
    if ui.button("Tutup tab").clicked() {
        action = Some(TabAction::Close(index));
    }
    if action.is_some() {
        ui.close();
    }
    action
}

pub fn show(
    ui: &mut egui::Ui,
    items: &[TabItem],
    active: usize,
    groups: &[ServerGroup],
) -> Option<TabAction> {
    let bar = ui.max_rect();
    let painter = ui.painter_at(bar);
    painter.rect_filled(bar, 0.0, BAR_COLOR);

    let width = tab_width(bar.width(), items.len());
    let base = ui.scope_id().with("tab_bar");
    let mut action = None;

    for (index, item) in items.iter().enumerate() {
        let title = &item.title;
        let rect = tab_rect(bar, width, index);
        let close = close_rect(rect);
        let tab_id = base.with(index);
        let response = ui.interact(rect, tab_id, Sense::click());
        let close_response = ui.interact(close, tab_id.with("close"), Sense::click());

        if close_response.clicked() {
            action = Some(TabAction::Close(index));
        } else if response.clicked() {
            action = Some(TabAction::Select(index));
        }
        response.context_menu(|ui| {
            if let Some(chosen) = tab_menu(ui, index, item.pane_count) {
                action = Some(chosen);
            }
        });

        let is_active = index == active;
        let hovered = response.hovered() || close_response.hovered();
        if is_active {
            painter.rect_filled(rect, 0.0, ACTIVE_COLOR);
        } else if hovered {
            painter.rect_filled(rect, 0.0, HOVER_COLOR);
        }

        let text_color = if is_active {
            TEXT_COLOR
        } else {
            INACTIVE_TEXT_COLOR
        };
        let mut text_rect = rect;
        text_rect.set_right(close.left());
        painter.with_clip_rect(text_rect.intersect(bar)).text(
            pos2(rect.left() + TEXT_MARGIN, rect.center().y),
            Align2::LEFT_CENTER,
            title,
            FontId::proportional(FONT_SIZE),
            text_color,
        );

        if close_response.hovered() {
            painter.rect_filled(close, 3.0, HOVER_COLOR);
        }
        painter.text(
            close.center(),
            Align2::CENTER_CENTER,
            "×",
            FontId::proportional(FONT_SIZE + 2.0),
            text_color,
        );
    }

    let new_button = new_button_rect(bar, width, items.len());
    let response = ui.interact(new_button, base.with("new"), Sense::click());
    if response.clicked() {
        action = Some(TabAction::New);
    }
    if response.hovered() {
        painter.rect_filled(new_button, 0.0, HOVER_COLOR);
    }
    painter.text(
        new_button.center(),
        Align2::CENTER_CENTER,
        "+",
        FontId::proportional(FONT_SIZE + 4.0),
        TEXT_COLOR,
    );

    let dropdown = dropdown_rect(new_button);
    let response = ui.interact(dropdown, base.with("new_menu"), Sense::click());
    if response.hovered() {
        painter.rect_filled(dropdown, 0.0, HOVER_COLOR);
    }
    paint_chevron(&painter, dropdown);
    Popup::menu(&response).show(|ui| {
        ui.set_min_width(MENU_WIDTH);
        if let Some(chosen) = new_tab_menu(ui, groups) {
            action = Some(chosen);
        }
    });

    action
}

/// Font bawaan tidak punya glyph panah, jadi ikon digambar sebagai bentuk.
fn paint_chevron(painter: &Painter, rect: Rect) {
    let center = rect.center();
    let points = vec![
        pos2(center.x - CHEVRON_RADIUS, center.y - CHEVRON_RADIUS / 2.0),
        pos2(center.x + CHEVRON_RADIUS, center.y - CHEVRON_RADIUS / 2.0),
        pos2(center.x, center.y + CHEVRON_RADIUS / 2.0 + 1.0),
    ];
    painter.add(Shape::convex_polygon(points, TEXT_COLOR, Stroke::NONE));
}

/// Menu tab baru: shell, lalu server per folder.
fn new_tab_menu(ui: &mut egui::Ui, groups: &[ServerGroup]) -> Option<TabAction> {
    let mut action = None;
    if ui
        .add(Button::new("PowerShell").shortcut_text("Ctrl+T"))
        .clicked()
    {
        action = Some(TabAction::New);
    }
    ui.separator();
    if groups.is_empty() {
        ui.weak("Belum ada server");
    }
    ScrollArea::vertical()
        .max_height(MENU_MAX_HEIGHT)
        .show(ui, |ui| {
            for group in groups {
                if let Some(title) = &group.title {
                    ui.weak(title);
                }
                for (id, name) in &group.servers {
                    if ui.button(name).clicked() {
                        action = Some(TabAction::Connect(*id));
                    }
                }
            }
        });
    if action.is_some() {
        ui.close();
    }
    action
}

/// Posisi layar untuk dipakai test klik.
#[cfg(test)]
pub mod test_positions {
    use eframe::egui::Pos2;

    use super::*;

    fn bar(window_width: f32) -> Rect {
        Rect::from_min_size(Pos2::ZERO, vec2(window_width, BAR_HEIGHT))
    }

    pub fn tab(window_width: f32, count: usize, index: usize) -> Pos2 {
        let width = tab_width(window_width, count);
        let rect = tab_rect(bar(window_width), width, index);
        pos2(rect.left() + TEXT_MARGIN, rect.center().y)
    }

    pub fn close(window_width: f32, count: usize, index: usize) -> Pos2 {
        let width = tab_width(window_width, count);
        close_rect(tab_rect(bar(window_width), width, index)).center()
    }

    pub fn dropdown(window_width: f32, count: usize) -> Pos2 {
        let width = tab_width(window_width, count);
        dropdown_rect(new_button_rect(bar(window_width), width, count)).center()
    }

    pub fn new_button(window_width: f32, count: usize) -> Pos2 {
        let width = tab_width(window_width, count);
        new_button_rect(bar(window_width), width, count).center()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lebar_tab_mengikuti_jumlah_dengan_batas() {
        assert_eq!(tab_width(1000.0, 1), MAX_TAB_WIDTH);
        assert_eq!(tab_width(1000.0, 0), MAX_TAB_WIDTH);
        assert_eq!(tab_width(454.0, 5), 80.0);
        assert_eq!(tab_width(400.0, 50), MIN_TAB_WIDTH);
    }

    #[test]
    fn tombol_baru_tetap_di_dalam_bilah() {
        let bar = Rect::from_min_size(pos2(0.0, 0.0), vec2(400.0, BAR_HEIGHT));
        let rect = dropdown_rect(new_button_rect(bar, MIN_TAB_WIDTH, 50));
        assert!(bar.contains_rect(rect));
    }
}
