//! Widget egui: menggambar grid terminal dan menerjemahkan event ke [`TerminalSession`].

use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::index::{Point, Side};
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::TermMode;
use alacritty_terminal::vte::ansi::{CursorShape, NamedColor, Rgb};
use eframe::egui::{
    self, pos2, vec2, Align2, Color32, CursorIcon, Event, FontId, Key, Modifiers, MouseWheelUnit,
    Painter, Pos2, Rect, Sense, Stroke, StrokeKind, ViewportCommand,
};

use super::colors::SELECTION;
use super::input::encode_key;
use super::session::{GridSize, TerminalSession};

const FONT_SIZE: f32 = 14.0;
const PADDING: f32 = 4.0;
const WHEEL_LINES_PER_TICK: f32 = 3.0;
const CURSOR_THICKNESS: f32 = 2.0;
const INTERRUPT: u8 = 0x03;
const CUT_CONTROL: u8 = 0x18;

#[derive(Clone, Copy)]
struct CellMetrics {
    width: f32,
    height: f32,
}

pub struct TerminalView {
    font: FontId,
    /// Sisa scroll roda mouse yang belum mencapai satu baris.
    wheel_remainder: f32,
}

impl TerminalView {
    pub fn new() -> Self {
        Self {
            font: FontId::monospace(FONT_SIZE),
            wheel_remainder: 0.0,
        }
    }

    pub fn show(&mut self, ui: &mut egui::Ui, session: &mut TerminalSession) {
        let rect = ui.max_rect();
        let response = ui
            .allocate_rect(rect, Sense::click_and_drag())
            .on_hover_cursor(CursorIcon::Text);
        let grid_rect = rect.shrink(PADDING);
        let metrics = self.cell_metrics(ui.ctx());

        session.resize(
            GridSize::new(
                (grid_rect.width() / metrics.width) as usize,
                (grid_rect.height() / metrics.height) as usize,
            ),
            (metrics.width as u16, metrics.height as u16),
        );

        self.handle_events(ui.ctx(), session, metrics);
        handle_selection(&response, session, grid_rect.min, metrics);

        let focused = ui.input(|input| input.focused);
        let painter = ui.painter_at(rect);
        self.paint(&painter, rect, grid_rect.min, metrics, session, focused);
    }

    fn cell_metrics(&self, ctx: &egui::Context) -> CellMetrics {
        let scale = ctx.pixels_per_point();
        let (width, height) = ctx.fonts_mut(|fonts| {
            (
                fonts.glyph_width(&self.font, 'M'),
                fonts.row_height(&self.font),
            )
        });
        CellMetrics {
            // Lebar tidak dibulatkan agar sama dengan jarak glyph hasil layout egui.
            width,
            height: (height * scale).ceil() / scale,
        }
    }

    fn handle_events(
        &mut self,
        ctx: &egui::Context,
        session: &mut TerminalSession,
        metrics: CellMetrics,
    ) {
        let (events, modifiers) = ctx.input(|input| (input.events.clone(), input.modifiers));
        for event in events {
            match event {
                // Dengan Alt, byte-nya sudah dikirim dari event Key sebagai ESC + karakter.
                Event::Text(text) if !modifiers.alt => session.send_input(text.into_bytes()),
                Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } => handle_key(ctx, session, key, modifiers),
                // egui menelan Ctrl+C dan Ctrl+X sebagai event clipboard.
                Event::Copy => match session.take_selection() {
                    Some(text) => ctx.copy_text(text),
                    None => session.send_input(vec![INTERRUPT]),
                },
                Event::Cut => session.send_input(vec![CUT_CONTROL]),
                Event::Paste(text) => session.paste(&text),
                Event::MouseWheel { unit, delta, .. } => {
                    self.handle_wheel(session, unit, delta.y, metrics);
                }
                _ => {}
            }
        }
    }

    fn handle_wheel(
        &mut self,
        session: &mut TerminalSession,
        unit: MouseWheelUnit,
        delta: f32,
        metrics: CellMetrics,
    ) {
        let lines = match unit {
            MouseWheelUnit::Point => delta / metrics.height,
            MouseWheelUnit::Line => delta * WHEEL_LINES_PER_TICK,
            MouseWheelUnit::Page => delta * session.term().screen_lines() as f32,
        };
        self.wheel_remainder += lines;
        let whole = self.wheel_remainder.trunc();
        self.wheel_remainder -= whole;
        let whole = whole as i32;
        if whole == 0 {
            return;
        }

        let mode = session.mode();
        if mode.contains(TermMode::ALT_SCREEN | TermMode::ALTERNATE_SCROLL) {
            // Layar alternatif (vim, less) tidak punya scrollback: roda menjadi tombol panah.
            let arrow = if whole > 0 {
                Key::ArrowUp
            } else {
                Key::ArrowDown
            };
            let app_cursor = mode.contains(TermMode::APP_CURSOR);
            if let Some(bytes) = encode_key(arrow, Modifiers::NONE, app_cursor) {
                session.send_input(bytes.repeat(whole.unsigned_abs() as usize));
            }
        } else if !mode.contains(TermMode::ALT_SCREEN) {
            session.scroll(Scroll::Delta(whole));
        }
    }

    fn paint(
        &self,
        painter: &Painter,
        rect: Rect,
        origin: Pos2,
        metrics: CellMetrics,
        session: &TerminalSession,
        focused: bool,
    ) {
        let term = session.term();
        let palette = session.palette();
        let content = term.renderable_content();
        let overrides = content.colors;
        let default_bg = palette.named(NamedColor::Background, overrides);
        let cursor_color = palette.named(NamedColor::Cursor, overrides);
        let cursor = content.cursor;
        let offset = content.display_offset as i32;
        let selection = content.selection;
        let block_cursor = focused && cursor.shape == CursorShape::Block;
        let scale = painter.pixels_per_point();
        let layout = Layout {
            origin,
            metrics,
            scale,
        };

        painter.rect_filled(rect, 0.0, color32(default_bg));

        let mut text = TextRun::default();
        for indexed in content.display_iter {
            let flags = indexed.cell.flags;
            if flags.intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER) {
                continue;
            }

            let point = indexed.point;
            let row = point.line.0 + offset;
            let Ok(row) = usize::try_from(row) else {
                continue;
            };
            let column = point.column.0;
            let width = if flags.contains(Flags::WIDE_CHAR) {
                2
            } else {
                1
            };

            let (mut foreground, mut background) = palette.cell_colors(indexed.cell, overrides);
            if selection.is_some_and(|range| range.contains(point)) {
                background = SELECTION;
            }
            if block_cursor && point == cursor.point {
                foreground = default_bg;
                background = cursor_color;
            }

            if background != default_bg {
                painter.rect_filled(layout.cells(row, column, width), 0.0, color32(background));
            }
            layout.decorations(painter, flags, row, column, width, foreground);

            let character = indexed.cell.c;
            if character == ' ' || character == '\0' || foreground == background {
                text.flush(painter, &layout, &self.font);
                continue;
            }
            // Karakter non-ASCII bisa memakai font cadangan dengan lebar berbeda, jadi
            // masing-masing digambar sendiri agar kolom berikutnya tidak bergeser.
            if !character.is_ascii() || !text.continues(row, column, foreground) {
                text.flush(painter, &layout, &self.font);
                text.start(row, column, foreground);
            }
            text.push(character);
            if let Some(combining) = indexed.cell.zerowidth() {
                text.extend(combining);
            }
            if !character.is_ascii() {
                text.flush(painter, &layout, &self.font);
            }
        }
        text.flush(painter, &layout, &self.font);

        paint_cursor(
            painter,
            &layout,
            cursor.shape,
            cursor.point,
            offset,
            cursor_color,
            focused,
        );
    }
}

/// Shortcut milik aplikasi didahulukan, sisanya diteruskan ke proses di terminal.
fn handle_key(ctx: &egui::Context, session: &mut TerminalSession, key: Key, modifiers: Modifiers) {
    let ctrl_shift = Modifiers::CTRL | Modifiers::SHIFT;
    if modifiers.matches_exact(ctrl_shift) && key == Key::C {
        if let Some(text) = session.take_selection() {
            ctx.copy_text(text);
        }
    } else if modifiers.matches_exact(ctrl_shift) && key == Key::V {
        ctx.send_viewport_cmd(ViewportCommand::RequestPaste);
    } else if modifiers.matches_exact(Modifiers::SHIFT) && key == Key::PageUp {
        session.scroll(Scroll::PageUp);
    } else if modifiers.matches_exact(Modifiers::SHIFT) && key == Key::PageDown {
        session.scroll(Scroll::PageDown);
    } else {
        let app_cursor = session.mode().contains(TermMode::APP_CURSOR);
        if let Some(bytes) = encode_key(key, modifiers, app_cursor) {
            session.send_input(bytes);
        }
    }
}

fn handle_selection(
    response: &egui::Response,
    session: &mut TerminalSession,
    origin: Pos2,
    metrics: CellMetrics,
) {
    if response.clicked() {
        session.clear_selection();
    }
    let Some(position) = response.interact_pointer_pos() else {
        return;
    };
    if response.drag_started() {
        // drag_started baru aktif setelah pointer bergerak melewati ambang, jadi seleksi harus
        // dimulai dari titik tekan, bukan dari posisi pointer saat ini.
        let press = response.ctx.input(|input| input.pointer.press_origin());
        let (column, line, side) = cell_at(press.unwrap_or(position), origin, metrics);
        session.start_selection(column, line, side);
    }
    if response.dragged() {
        let (column, line, side) = cell_at(position, origin, metrics);
        session.update_selection(column, line, side);
    }
}

fn cell_at(position: Pos2, origin: Pos2, metrics: CellMetrics) -> (usize, usize, Side) {
    let x = ((position.x - origin.x) / metrics.width).max(0.0);
    let y = ((position.y - origin.y) / metrics.height).max(0.0);
    let side = if x.fract() < 0.5 {
        Side::Left
    } else {
        Side::Right
    };
    (x as usize, y as usize, side)
}

fn paint_cursor(
    painter: &Painter,
    layout: &Layout,
    shape: CursorShape,
    point: Point,
    offset: i32,
    color: Rgb,
    focused: bool,
) {
    let Ok(row) = usize::try_from(point.line.0 + offset) else {
        return;
    };
    let cell = layout.cells(row, point.column.0, 1);
    let color = color32(color);
    match shape {
        CursorShape::Hidden => {}
        CursorShape::Block if focused => {}
        CursorShape::Block | CursorShape::HollowBlock => {
            painter.rect_stroke(cell, 0.0, Stroke::new(1.0, color), StrokeKind::Inside);
        }
        CursorShape::Beam => {
            let beam = Rect::from_min_size(cell.min, vec2(CURSOR_THICKNESS, cell.height()));
            painter.rect_filled(beam, 0.0, color);
        }
        CursorShape::Underline => {
            let top = pos2(cell.min.x, cell.max.y - CURSOR_THICKNESS);
            painter.rect_filled(Rect::from_min_max(top, cell.max), 0.0, color);
        }
    }
}

/// Posisi sel di layar.
struct Layout {
    origin: Pos2,
    metrics: CellMetrics,
    scale: f32,
}

impl Layout {
    fn position(&self, row: usize, column: usize) -> Pos2 {
        self.origin
            + vec2(
                column as f32 * self.metrics.width,
                row as f32 * self.metrics.height,
            )
    }

    /// Dibulatkan ke piksel supaya tidak ada celah tipis di antara latar sel yang bersebelahan.
    fn cells(&self, row: usize, column: usize, width: usize) -> Rect {
        let snap = |value: f32| (value * self.scale).round() / self.scale;
        let min = self.position(row, column);
        let max = self.position(row + 1, column + width);
        Rect::from_min_max(
            pos2(snap(min.x), snap(min.y)),
            pos2(snap(max.x), snap(max.y)),
        )
    }

    fn decorations(
        &self,
        painter: &Painter,
        flags: Flags,
        row: usize,
        column: usize,
        width: usize,
        color: Rgb,
    ) {
        if !flags.intersects(Flags::ALL_UNDERLINES | Flags::STRIKEOUT) {
            return;
        }
        let cell = self.cells(row, column, width);
        let stroke = Stroke::new(1.0, color32(color));
        let line = |y: f32| {
            painter.line_segment([pos2(cell.min.x, y), pos2(cell.max.x, y)], stroke);
        };
        if flags.intersects(Flags::ALL_UNDERLINES) {
            line(cell.max.y - 1.5);
        }
        if flags.contains(Flags::STRIKEOUT) {
            line(cell.center().y);
        }
    }
}

/// Karakter berurutan dengan warna sama, digambar sekaligus agar jauh lebih hemat.
#[derive(Default)]
struct TextRun {
    text: String,
    row: usize,
    start_column: usize,
    next_column: usize,
    color: Option<Rgb>,
}

impl TextRun {
    fn continues(&self, row: usize, column: usize, color: Rgb) -> bool {
        self.color == Some(color) && self.row == row && self.next_column == column
    }

    fn start(&mut self, row: usize, column: usize, color: Rgb) {
        self.row = row;
        self.start_column = column;
        self.next_column = column;
        self.color = Some(color);
    }

    fn push(&mut self, character: char) {
        self.text.push(character);
        self.next_column += 1;
    }

    fn extend(&mut self, characters: &[char]) {
        self.text.extend(characters);
    }

    fn flush(&mut self, painter: &Painter, layout: &Layout, font: &FontId) {
        if let Some(color) = self.color.take() {
            if !self.text.is_empty() {
                painter.text(
                    layout.position(self.row, self.start_column),
                    Align2::LEFT_TOP,
                    &self.text,
                    font.clone(),
                    color32(color),
                );
            }
        }
        self.text.clear();
    }
}

fn color32(color: Rgb) -> Color32 {
    Color32::from_rgb(color.r, color.g, color.b)
}

#[cfg(test)]
mod tests {
    use alacritty_terminal::grid::Dimensions;
    use eframe::egui::{FullOutput, OutputCommand, PointerButton, RawInput};

    use super::super::session::tests::{pump_until, screen_text, spawn};
    use super::*;

    /// Menjalankan `TerminalView` tanpa jendela, dengan input sintetis dan proses sungguhan.
    struct Harness {
        ctx: egui::Context,
        view: TerminalView,
        session: TerminalSession,
        time: f64,
    }

    impl Harness {
        fn new(args: &[&str]) -> Self {
            let mut harness = Self {
                ctx: egui::Context::default(),
                view: TerminalView::new(),
                session: spawn(args),
                time: 0.0,
            };
            harness.frame(Vec::new());
            harness
        }

        fn frame(&mut self, events: Vec<Event>) -> FullOutput {
            self.session.pump();
            self.time += 0.05;
            let input = RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0))),
                time: Some(self.time),
                events,
                ..RawInput::default()
            };
            let (view, session) = (&mut self.view, &mut self.session);
            let mut output = self.ctx.run_ui(input, |ui| view.show(ui, session));
            output.textures_delta.clear();
            output
        }

        fn wait_for(&mut self, text: &str) {
            pump_until(&mut self.session, |s| screen_text(s).contains(text));
            self.frame(Vec::new());
        }

        /// Titik tengah sel tempat teks tampil di layar.
        fn cell_center(&self, text: &str, extra_columns: usize) -> Pos2 {
            let screen = screen_text(&self.session);
            let index = screen.find(text).expect("teks harus ada di layar");
            let columns = self.session.term().columns();
            let metrics = self.view.cell_metrics(&self.ctx);
            let column = index % columns + extra_columns;
            let row = index / columns;
            pos2(
                PADDING + (column as f32 + 0.25) * metrics.width,
                PADDING + (row as f32 + 0.5) * metrics.height,
            )
        }

        fn drag(&mut self, from: Pos2, to: Pos2) {
            let button = |pos, pressed| Event::PointerButton {
                pos,
                button: PointerButton::Primary,
                pressed,
                modifiers: Modifiers::NONE,
            };
            self.frame(vec![Event::PointerMoved(from)]);
            self.frame(vec![button(from, true)]);
            self.frame(vec![Event::PointerMoved(to)]);
            self.frame(vec![Event::PointerMoved(to)]);
            self.frame(vec![button(to, false)]);
        }
    }

    fn copied(output: &FullOutput) -> Option<&str> {
        output
            .platform_output
            .commands
            .iter()
            .find_map(|command| match command {
                OutputCommand::CopyText(text) => Some(text.as_str()),
                _ => None,
            })
    }

    #[test]
    fn drag_lalu_ctrl_c_menyalin_tanpa_interrupt() {
        let mut harness = Harness::new(&["/k", "echo teks-seleksi"]);
        harness.wait_for("teks-seleksi");
        let start = harness.cell_center("teks-seleksi", 0);
        // Sisi kiri sel sesudah huruf terakhir, supaya huruf terakhir ikut terpilih.
        let end = harness.cell_center("teks-seleksi", 12);
        harness.drag(start, end);

        let output = harness.frame(vec![Event::Copy]);
        assert_eq!(copied(&output), Some("teks-seleksi"));
        assert!(harness.session.term().selection.is_none());
    }

    #[test]
    fn ctrl_c_tanpa_seleksi_mengirim_interrupt() {
        let mut harness = Harness::new(&[]);
        harness.frame(vec![Event::Paste("ping -t 127.0.0.1\n".to_owned())]);
        harness.wait_for("127.0.0.1:");

        let output = harness.frame(vec![Event::Copy]);
        assert_eq!(copied(&output), None);
        harness.wait_for("^C");
    }

    #[test]
    fn paste_sampai_ke_shell() {
        let mut harness = Harness::new(&[]);
        harness.frame(vec![Event::Paste("echo tempel-%OS%\n".to_owned())]);
        harness.wait_for("tempel-Windows_NT");
    }

    #[test]
    fn klik_melepas_seleksi() {
        let mut harness = Harness::new(&["/k", "echo teks-seleksi"]);
        harness.wait_for("teks-seleksi");
        let start = harness.cell_center("teks-seleksi", 0);
        let end = harness.cell_center("teks-seleksi", 11);
        harness.drag(start, end);
        assert!(harness.session.term().selection.is_some());

        harness.drag(start, start);
        assert!(harness.session.term().selection.is_none());
    }

    #[test]
    fn roda_mouse_menggulir_riwayat() {
        let mut harness = Harness::new(&["/k", "for /L %i in (1,1,100) do @echo baris-%i"]);
        harness.wait_for("baris-100");
        harness.frame(vec![Event::MouseWheel {
            unit: MouseWheelUnit::Line,
            delta: vec2(0.0, 1.0),
            phase: egui::TouchPhase::Move,
            modifiers: Modifiers::NONE,
            source: egui::MouseWheelSource::Unknown,
        }]);
        assert_eq!(harness.session.term().grid().display_offset(), 3);

        // Mengetik mengembalikan tampilan ke baris terbaru.
        harness.frame(vec![Event::Text("a".to_owned())]);
        assert_eq!(harness.session.term().grid().display_offset(), 0);
    }
}
