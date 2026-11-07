use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use eframe::egui::{self, pos2, Key, Rect, UiBuilder, ViewportCommand};
use portable_pty::CommandBuilder;

use crate::layout::{Axis, PaneId, Tabs, DIVIDER_THICKNESS};
use crate::terminal::{default_shell, GridSize, TerminalSession, TerminalView};
use crate::ui::shortcuts::{self, Shortcut};
use crate::ui::split_view;
use crate::ui::tab_bar::{self, TabAction, TabItem, BAR_HEIGHT};

const INITIAL_COLUMNS: usize = 80;
const INITIAL_LINES: usize = 24;
const APP_NAME: &str = "ctermin";
const FAILED_TAB_TITLE: &str = "Gagal";

/// Sesi terminal, atau pesan error jika gagal dijalankan.
type Pane = Result<TerminalSession, String>;

/// State aplikasi utama.
pub struct CterminApp {
    tabs: Tabs,
    panes: HashMap<PaneId, Pane>,
    view: TerminalView,
    shell: fn() -> CommandBuilder,
    window_title: String,
}

impl CterminApp {
    pub fn new(ctx: &egui::Context) -> Self {
        Self::with_shell(ctx, default_shell)
    }

    fn with_shell(ctx: &egui::Context, shell: fn() -> CommandBuilder) -> Self {
        // Ctrl+Plus dan Ctrl+Minus harus sampai ke terminal, bukan mengubah zoom egui.
        ctx.options_mut(|options| options.zoom_with_keyboard = false);
        let mut app = Self {
            tabs: Tabs::default(),
            panes: HashMap::new(),
            view: TerminalView::new(),
            shell,
            window_title: String::new(),
        };
        app.open_tab(ctx);
        app
    }

    fn open_tab(&mut self, ctx: &egui::Context) {
        let pane = self.tabs.open();
        let session = self.spawn_shell(ctx);
        self.panes.insert(pane, session);
    }

    fn split_focused_pane(&mut self, ctx: &egui::Context, axis: Axis) {
        if let Some(pane) = self.tabs.split_focused(axis) {
            let session = self.spawn_shell(ctx);
            self.panes.insert(pane, session);
        }
    }

    /// Melepas sesi akan mematikan proses dan menghentikan thread PTY-nya.
    fn close_tab(&mut self, index: usize) {
        for pane in self.tabs.close(index) {
            self.panes.remove(&pane);
        }
    }

    fn close_focused_pane(&mut self) {
        if let Some(pane) = self.tabs.close_focused() {
            self.panes.remove(&pane);
        }
    }

    fn spawn_shell(&self, ctx: &egui::Context) -> Pane {
        let ctx = ctx.clone();
        let notify = Arc::new(move || ctx.request_repaint());
        let size = GridSize::new(INITIAL_COLUMNS, INITIAL_LINES);
        TerminalSession::spawn((self.shell)(), size, notify).map_err(|err| format!("{err:#}"))
    }

    /// Semua tab diproses, bukan hanya yang aktif: tab yang tidak dibaca akan memenuhi antrean
    /// output dan menghentikan prosesnya.
    fn pump_all(&mut self) -> Option<Duration> {
        self.panes
            .values_mut()
            .filter_map(|pane| pane.as_mut().ok())
            .filter_map(TerminalSession::pump)
            .min()
    }

    fn restart_active_if_requested(&mut self, ctx: &egui::Context) {
        let Some(pane) = self.tabs.active() else {
            return;
        };
        let exited = matches!(self.panes.get(&pane), Some(Ok(session)) if session.exited());
        if exited && ctx.input(|input| input.key_pressed(Key::Enter)) {
            let session = self.spawn_shell(ctx);
            self.panes.insert(pane, session);
        }
    }

    fn apply_shortcuts(&mut self, ctx: &egui::Context) {
        for shortcut in shortcuts::take(ctx) {
            match shortcut {
                Shortcut::Open => self.open_tab(ctx),
                Shortcut::Close => self.close_focused_pane(),
                Shortcut::Next => self.tabs.next(),
                Shortcut::Previous => self.tabs.previous(),
                Shortcut::Split(axis) => self.split_focused_pane(ctx, axis),
                Shortcut::Focus(direction) => self.tabs.focus_direction(direction),
            }
        }
    }

    fn apply_tab_action(&mut self, ctx: &egui::Context, action: TabAction) {
        match action {
            TabAction::Select(index) => self.tabs.select(index),
            TabAction::Close(index) => self.close_tab(index),
            TabAction::New => self.open_tab(ctx),
            TabAction::Duplicate(index) => {
                let pane = self.tabs.open_after(index);
                let session = self.spawn_shell(ctx);
                self.panes.insert(pane, session);
            }
            TabAction::Split(index, axis) => {
                self.tabs.select(index);
                self.split_focused_pane(ctx, axis);
            }
            TabAction::ClosePane(index) => {
                self.tabs.select(index);
                self.close_focused_pane();
            }
        }
    }

    fn tab_items(&self) -> Vec<TabItem> {
        self.tab_titles()
            .into_iter()
            .zip(self.tabs.pane_counts())
            .map(|(title, pane_count)| TabItem { title, pane_count })
            .collect()
    }

    fn tab_titles(&self) -> Vec<String> {
        self.tabs
            .focused_panes()
            .iter()
            .map(|pane| match self.panes.get(pane) {
                Some(Ok(session)) => session.title().to_owned(),
                _ => FAILED_TAB_TITLE.to_owned(),
            })
            .collect()
    }

    fn sync_window_title(&mut self, ctx: &egui::Context) {
        let title = match self.tabs.active().and_then(|pane| self.panes.get(&pane)) {
            Some(Ok(session)) => format!("{} - {APP_NAME}", session.title()),
            _ => APP_NAME.to_owned(),
        };
        if title != self.window_title {
            ctx.send_viewport_cmd(ViewportCommand::Title(title.clone()));
            self.window_title = title;
        }
    }

    fn show(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        self.apply_shortcuts(&ctx);

        let window = ui.max_rect();
        let bar = Rect::from_min_max(window.min, pos2(window.right(), window.top() + BAR_HEIGHT));
        let content = Rect::from_min_max(pos2(window.left(), bar.bottom()), window.max);

        let items = self.tab_items();
        let active = self.tabs.active_index();
        let action = ui
            .scope_builder(UiBuilder::new().max_rect(bar), |ui| {
                tab_bar::show(ui, &items, active)
            })
            .inner;
        if let Some(action) = action {
            self.apply_tab_action(&ctx, action);
        }

        if self.tabs.is_empty() {
            ctx.send_viewport_cmd(ViewportCommand::Close);
            return;
        }

        self.show_panes(ui, content);
        self.sync_window_title(&ctx);
    }

    fn show_panes(&mut self, ui: &mut egui::Ui, content: Rect) {
        let Some(layout) = self.tabs.active_layout() else {
            return;
        };
        let arranged = layout.arrange(content, DIVIDER_THICKNESS);
        let dividers = layout.dividers(content);
        let focused = self.tabs.active();

        let base = ui.scope_id();
        let mut pressed = None;
        for (pane, rect) in &arranged {
            let keyboard_focus = Some(*pane) == focused;
            let id = base.with(pane);
            let session = self.panes.get_mut(pane);
            let view = &mut self.view;
            let scope = UiBuilder::new().id_salt(pane).max_rect(*rect);
            let response = ui.scope_builder(scope, |ui| match session {
                Some(Ok(session)) => view.show(ui, id, session, keyboard_focus),
                Some(Err(message)) => {
                    ui.colored_label(ui.visuals().error_fg_color, message.as_str());
                    false
                }
                None => false,
            });
            if response.inner {
                pressed = Some(*pane);
            }
        }

        if let Some(change) = split_view::show_dividers(ui, &dividers) {
            if let Some(layout) = self.tabs.active_layout_mut() {
                layout.set_ratio(&change.path, change.ratio);
            }
        }

        // Penanda fokus hanya berguna jika ada lebih dari satu panel.
        if arranged.len() > 1 {
            let focused_rect = arranged.iter().find(|(pane, _)| Some(*pane) == focused);
            if let Some((_, rect)) = focused_rect {
                split_view::paint_focus(ui, *rect);
            }
        }

        if let Some(pane) = pressed {
            self.tabs.focus(pane);
        }
    }
}

impl eframe::App for CterminApp {
    // Output diproses di sini, bukan di `ui`, karena `ui` tidak dipanggil saat jendela
    // diminimalkan.
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if let Some(delay) = self.pump_all() {
            ctx.request_repaint_after(delay);
        }
        self.restart_active_if_requested(ctx);
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.show(ui);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Weak;
    use std::thread;
    use std::time::Instant;

    use alacritty_terminal::grid::Dimensions;
    use eframe::egui::{
        vec2, Event, FullOutput, Modifiers, PointerButton, Pos2, RawInput, ViewportId,
    };

    use super::*;
    use crate::ui::tab_bar::test_positions;

    const WIDTH: f32 = 800.0;
    const HEIGHT: f32 = 600.0;
    const TIMEOUT: Duration = Duration::from_secs(20);

    fn cmd_shell() -> CommandBuilder {
        CommandBuilder::new("cmd.exe")
    }

    /// Menjalankan aplikasi tanpa jendela, dengan urutan yang sama seperti eframe.
    struct Harness {
        ctx: egui::Context,
        app: CterminApp,
        time: f64,
        size: egui::Vec2,
    }

    impl Harness {
        fn new() -> Self {
            let ctx = egui::Context::default();
            let app = CterminApp::with_shell(&ctx, cmd_shell);
            let mut harness = Self {
                ctx,
                app,
                time: 0.0,
                size: vec2(WIDTH, HEIGHT),
            };
            harness.frame(Vec::new());
            harness
        }

        fn frame(&mut self, events: Vec<Event>) -> FullOutput {
            self.time += 0.05;
            let input = RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, self.size)),
                time: Some(self.time),
                events,
                ..RawInput::default()
            };
            let app = &mut self.app;
            let mut output = self.ctx.run_ui(input, |ui| {
                app.pump_all();
                app.restart_active_if_requested(ui.ctx());
                app.show(ui);
            });
            output.textures_delta.clear();
            output
        }

        fn press(&mut self, key: Key, modifiers: Modifiers) -> FullOutput {
            let event = |pressed| Event::Key {
                key,
                physical_key: Some(key),
                pressed,
                repeat: false,
                modifiers,
            };
            self.frame(vec![event(true), event(false)])
        }

        fn click(&mut self, pos: Pos2) {
            let button = |pressed| Event::PointerButton {
                pos,
                button: PointerButton::Primary,
                pressed,
                modifiers: Modifiers::NONE,
            };
            self.frame(vec![Event::PointerMoved(pos)]);
            self.frame(vec![button(true)]);
            self.frame(vec![button(false)]);
            self.frame(Vec::new());
        }

        fn drag(&mut self, from: Pos2, to: Pos2) {
            let button = |pos, pressed| Event::PointerButton {
                pos,
                button: PointerButton::Primary,
                pressed,
                modifiers: Modifiers::NONE,
            };
            self.frame(vec![Event::PointerMoved(from)]);
            self.frame(vec![Event::PointerMoved(from)]);
            self.frame(vec![button(from, true)]);
            self.frame(vec![Event::PointerMoved(to)]);
            self.frame(vec![Event::PointerMoved(to)]);
            self.frame(vec![button(to, false)]);
        }

        fn right_click(&mut self, pos: Pos2) {
            let button = |pressed| Event::PointerButton {
                pos,
                button: PointerButton::Secondary,
                pressed,
                modifiers: Modifiers::NONE,
            };
            self.frame(vec![Event::PointerMoved(pos)]);
            self.frame(vec![button(true)]);
            self.frame(vec![button(false)]);
            self.frame(Vec::new());
        }

        fn type_line(&mut self, line: &str) {
            self.frame(vec![Event::Paste(format!("{line}\n"))]);
        }

        fn wait_until(&mut self, done: impl Fn(&CterminApp) -> bool) {
            let start = Instant::now();
            while !done(&self.app) {
                assert!(start.elapsed() < TIMEOUT, "waktu habis");
                self.frame(Vec::new());
                thread::sleep(Duration::from_millis(10));
            }
        }

        fn count(&self) -> usize {
            self.app.tabs.focused_panes().len()
        }

        fn active_session(&self) -> &TerminalSession {
            active_session(&self.app)
        }

        fn thread_tokens(&self) -> Vec<Weak<()>> {
            self.app
                .panes
                .values()
                .filter_map(|pane| pane.as_ref().ok())
                .map(TerminalSession::threads_alive)
                .collect()
        }
    }

    fn active_session(app: &CterminApp) -> &TerminalSession {
        app.tabs
            .active()
            .and_then(|pane| app.panes.get(&pane))
            .and_then(|pane| pane.as_ref().ok())
            .expect("tab aktif harus punya sesi")
    }

    fn screen(app: &CterminApp) -> String {
        active_session(app).screen_text()
    }

    fn wait_threads_stopped(tokens: &[Weak<()>]) {
        let start = Instant::now();
        while tokens.iter().any(|token| token.upgrade().is_some()) {
            assert!(start.elapsed() < TIMEOUT, "thread PTY masih hidup");
            thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn ctrl_t_membuka_dan_ctrl_w_menutup() {
        let mut harness = Harness::new();
        assert_eq!(harness.count(), 1);

        harness.press(Key::T, Modifiers::CTRL);
        assert_eq!(harness.count(), 2);
        assert_eq!(harness.app.tabs.active_index(), 1);
        assert_eq!(harness.app.panes.len(), 2);

        harness.press(Key::W, Modifiers::CTRL);
        assert_eq!(harness.count(), 1);
        assert_eq!(harness.app.panes.len(), 1);
    }

    #[test]
    fn ctrl_tab_berpindah_dan_berputar() {
        let mut harness = Harness::new();
        harness.press(Key::T, Modifiers::CTRL);
        harness.press(Key::T, Modifiers::CTRL);

        harness.press(Key::Tab, Modifiers::CTRL);
        assert_eq!(harness.app.tabs.active_index(), 0);
        harness.press(Key::Tab, Modifiers::CTRL | Modifiers::SHIFT);
        assert_eq!(harness.app.tabs.active_index(), 2);
    }

    #[test]
    fn tiap_tab_punya_sesi_sendiri() {
        let mut harness = Harness::new();
        harness.type_line("echo isi-tab-satu");
        harness.wait_until(|app| screen(app).matches("isi-tab-satu").count() >= 2);

        harness.press(Key::T, Modifiers::CTRL);
        harness.wait_until(|app| screen(app).contains('>'));
        assert!(!harness
            .active_session()
            .screen_text()
            .contains("isi-tab-satu"));

        harness.press(Key::Tab, Modifiers::CTRL);
        assert!(harness
            .active_session()
            .screen_text()
            .contains("isi-tab-satu"));
    }

    #[test]
    fn tab_latar_tetap_berjalan() {
        let mut harness = Harness::new();
        harness.type_line("ping -t 127.0.0.1");
        harness.wait_until(|app| screen(app).contains("127.0.0.1:"));
        let first = harness.app.tabs.focused_panes()[0];

        harness.press(Key::T, Modifiers::CTRL);
        let replies = move |app: &CterminApp| match app.panes.get(&first) {
            Some(Ok(session)) => session.screen_text().matches("127.0.0.1:").count(),
            _ => 0,
        };
        let before = replies(&harness.app);
        harness.wait_until(move |app| replies(app) > before);
    }

    #[test]
    fn enam_tab_berhenti_bersih_saat_ditutup() {
        let mut harness = Harness::new();
        for _ in 0..5 {
            harness.press(Key::T, Modifiers::CTRL);
        }
        assert_eq!(harness.count(), 6);
        harness.wait_until(|app| screen(app).contains('>'));

        let tokens = harness.thread_tokens();
        assert_eq!(tokens.len(), 6);
        for _ in 0..6 {
            harness.press(Key::W, Modifiers::CTRL);
        }

        assert!(harness.app.panes.is_empty());
        wait_threads_stopped(&tokens);
    }

    #[test]
    fn menutup_tab_terakhir_menutup_jendela() {
        let mut harness = Harness::new();
        let output = harness.press(Key::W, Modifiers::CTRL);
        let commands = &output.viewport_output[&ViewportId::ROOT].commands;
        assert!(commands.contains(&ViewportCommand::Close));
    }

    #[test]
    fn klik_bilah_tab() {
        let mut harness = Harness::new();

        harness.click(test_positions::new_button(WIDTH, 1));
        assert_eq!(harness.count(), 2);

        harness.click(test_positions::tab(WIDTH, 2, 0));
        assert_eq!(harness.app.tabs.active_index(), 0);

        harness.click(test_positions::close(WIDTH, 2, 1));
        assert_eq!(harness.count(), 1);
        assert_eq!(harness.app.tabs.active_index(), 0);
    }

    #[test]
    fn tombol_tab_tidak_memindahkan_fokus_ke_bilah_tab() {
        let mut harness = Harness::new();
        harness.frame(Vec::new());
        let focused = harness.ctx.memory(|memory| memory.focused());
        assert!(focused.is_some());

        for _ in 0..4 {
            harness.press(Key::Tab, Modifiers::NONE);
        }
        assert_eq!(harness.ctx.memory(|memory| memory.focused()), focused);
    }

    #[test]
    fn judul_tab_dan_jendela_mengikuti_sesi() {
        let mut harness = Harness::new();
        harness.type_line("title judul-tab");
        harness.wait_until(|app| active_session(app).title().contains("judul-tab"));

        // Judul baru dikirim pada frame yang sama dengan saat ia terbaca, atau frame berikutnya.
        harness.frame(Vec::new());
        assert!(harness.app.tab_titles()[0].contains("judul-tab"));
        assert!(harness.app.window_title.contains("judul-tab"));
        assert!(harness.app.window_title.ends_with(APP_NAME));
    }

    #[test]
    fn enter_memulai_ulang_shell_yang_keluar() {
        let mut harness = Harness::new();
        harness.type_line("exit 3");
        harness.wait_until(|app| active_session(app).exited());
        assert!(harness.active_session().screen_text().contains("kode 3"));

        let old = harness.thread_tokens();
        let pane = harness.app.tabs.active();
        harness.press(Key::Enter, Modifiers::NONE);

        assert_eq!(harness.app.tabs.active(), pane);
        assert!(!harness.active_session().exited());
        wait_threads_stopped(&old);
    }

    fn ctrl_shift() -> Modifiers {
        Modifiers::CTRL | Modifiers::SHIFT
    }

    fn pane_count(app: &CterminApp) -> usize {
        app.tabs
            .active_layout()
            .map_or(0, |layout| layout.panes().len())
    }

    fn content() -> Rect {
        Rect::from_min_max(pos2(0.0, BAR_HEIGHT), pos2(WIDTH, HEIGHT))
    }

    fn pane_rect(app: &CterminApp, pane: PaneId) -> Rect {
        let layout = app.tabs.active_layout().expect("ada tab aktif");
        layout
            .arrange(content(), DIVIDER_THICKNESS)
            .into_iter()
            .find(|(id, _)| *id == pane)
            .map(|(_, rect)| rect)
            .expect("panel ada di layout")
    }

    fn session_of(app: &CterminApp, pane: PaneId) -> &TerminalSession {
        match app.panes.get(&pane) {
            Some(Ok(session)) => session,
            _ => panic!("panel harus punya sesi"),
        }
    }

    #[test]
    fn ctrl_shift_d_dan_e_membuat_panel_bertingkat() {
        let mut harness = Harness::new();
        harness.press(Key::D, ctrl_shift());
        harness.press(Key::E, ctrl_shift());
        harness.press(Key::D, ctrl_shift());

        assert_eq!(pane_count(&harness.app), 4);
        assert_eq!(harness.app.panes.len(), 4);
        assert_eq!(harness.count(), 1, "tetap satu tab");
    }

    #[test]
    fn ukuran_grid_mengikuti_panel() {
        let mut harness = Harness::new();
        let left = harness.app.tabs.active().expect("ada panel");
        let full = harness.active_session().term().columns();

        harness.press(Key::D, ctrl_shift());
        harness.frame(Vec::new());
        let right = harness.app.tabs.active().expect("ada panel");

        let left_columns = session_of(&harness.app, left).term().columns();
        let right_columns = session_of(&harness.app, right).term().columns();
        assert!(left_columns < full * 6 / 10, "{left_columns} dari {full}");
        assert!(right_columns < full * 6 / 10, "{right_columns} dari {full}");
    }

    #[test]
    fn keyboard_hanya_masuk_ke_panel_fokus() {
        let mut harness = Harness::new();
        let left = harness.app.tabs.active().expect("ada panel");
        harness.press(Key::D, ctrl_shift());
        let right = harness.app.tabs.active().expect("ada panel");

        harness.type_line("echo hanya-kanan");
        harness.wait_until(move |app| {
            session_of(app, right)
                .screen_text()
                .matches("hanya-kanan")
                .count()
                >= 2
        });
        assert!(!session_of(&harness.app, left)
            .screen_text()
            .contains("hanya-kanan"));
    }

    #[test]
    fn klik_memindahkan_fokus() {
        let mut harness = Harness::new();
        let left = harness.app.tabs.active().expect("ada panel");
        harness.press(Key::D, ctrl_shift());
        harness.frame(Vec::new());

        let target = pane_rect(&harness.app, left).center();
        harness.click(target);
        assert_eq!(harness.app.tabs.active(), Some(left));

        harness.type_line("echo ke-kiri");
        harness.wait_until(move |app| {
            session_of(app, left)
                .screen_text()
                .matches("ke-kiri")
                .count()
                >= 2
        });
    }

    #[test]
    fn alt_panah_memindahkan_fokus() {
        let mut harness = Harness::new();
        let left = harness.app.tabs.active();
        harness.press(Key::D, ctrl_shift());
        let right = harness.app.tabs.active();

        harness.press(Key::ArrowLeft, Modifiers::ALT);
        assert_eq!(harness.app.tabs.active(), left);
        harness.press(Key::ArrowRight, Modifiers::ALT);
        assert_eq!(harness.app.tabs.active(), right);
    }

    #[test]
    fn ctrl_w_menutup_panel_bukan_tab() {
        let mut harness = Harness::new();
        let left = harness.app.tabs.active();
        harness.press(Key::D, ctrl_shift());

        harness.press(Key::W, Modifiers::CTRL);

        assert_eq!(harness.count(), 1);
        assert_eq!(pane_count(&harness.app), 1);
        assert_eq!(harness.app.panes.len(), 1);
        assert_eq!(harness.app.tabs.active(), left);
    }

    #[test]
    fn panel_yang_tersisa_mengambil_kembali_ruangnya() {
        let mut harness = Harness::new();
        let full = harness.active_session().term().columns();
        harness.press(Key::D, ctrl_shift());
        harness.frame(Vec::new());
        harness.press(Key::W, Modifiers::CTRL);
        harness.frame(Vec::new());
        assert_eq!(harness.active_session().term().columns(), full);
    }

    #[test]
    fn menutup_tab_berisi_split_menghentikan_semua_sesi() {
        let mut harness = Harness::new();
        let keep = harness.thread_tokens();
        harness.press(Key::T, Modifiers::CTRL);
        harness.press(Key::D, ctrl_shift());
        harness.press(Key::E, ctrl_shift());
        harness.press(Key::D, ctrl_shift());
        assert_eq!(harness.app.panes.len(), 5);

        let all = harness.thread_tokens();
        harness.app.close_tab(1);
        assert_eq!(harness.app.panes.len(), 1);

        let closed: Vec<_> = all
            .into_iter()
            .filter(|token| !token.ptr_eq(&keep[0]))
            .collect();
        assert_eq!(closed.len(), 4);
        wait_threads_stopped(&closed);
        assert!(keep[0].upgrade().is_some(), "tab lain tetap hidup");
    }

    #[test]
    fn drag_pembatas_mengubah_ukuran_panel() {
        let mut harness = Harness::new();
        let left = harness.app.tabs.active().expect("ada panel");
        harness.press(Key::D, ctrl_shift());
        harness.frame(Vec::new());
        let before = session_of(&harness.app, left).term().columns();

        let edge = pane_rect(&harness.app, left).right() + 1.0;
        harness.drag(pos2(edge, 300.0), pos2(edge - 200.0, 300.0));
        harness.frame(Vec::new());

        let after = session_of(&harness.app, left).term().columns();
        assert!(after + 10 < before, "kolom {before} menjadi {after}");
    }

    #[test]
    fn roda_mouse_mengikuti_pointer_bukan_fokus() {
        let mut harness = Harness::new();
        harness.type_line("for /L %i in (1,1,100) do @echo baris-%i");
        harness.wait_until(|app| screen(app).contains("baris-100"));
        let left = harness.app.tabs.active().expect("ada panel");
        harness.press(Key::D, ctrl_shift());
        harness.frame(Vec::new());
        let right = harness.app.tabs.active().expect("ada panel");

        let target = pane_rect(&harness.app, left).center();
        harness.frame(vec![Event::PointerMoved(target)]);
        harness.frame(vec![Event::MouseWheel {
            unit: egui::MouseWheelUnit::Line,
            delta: vec2(0.0, 1.0),
            phase: egui::TouchPhase::Move,
            modifiers: Modifiers::NONE,
            source: egui::MouseWheelSource::Unknown,
        }]);

        let offset = |pane| {
            session_of(&harness.app, pane)
                .term()
                .grid()
                .display_offset()
        };
        assert!(offset(left) > 0);
        assert_eq!(offset(right), 0);
        assert_eq!(harness.app.tabs.active(), Some(right));
    }

    #[test]
    fn enter_hanya_memulai_ulang_panel_fokus_yang_keluar() {
        let mut harness = Harness::new();
        let left = harness.app.tabs.active().expect("ada panel");
        harness.press(Key::D, ctrl_shift());
        harness.type_line("exit 0");
        harness.wait_until(|app| active_session(app).exited());

        harness.press(Key::Enter, Modifiers::NONE);
        assert!(!harness.active_session().exited());
        assert!(!session_of(&harness.app, left).exited());
        assert_eq!(pane_count(&harness.app), 2);
    }

    #[test]
    fn jendela_sangat_kecil_tidak_panik() {
        let mut harness = Harness::new();
        for _ in 0..4 {
            harness.press(Key::D, ctrl_shift());
            harness.press(Key::E, ctrl_shift());
        }
        harness.size = vec2(60.0, 40.0);
        harness.frame(Vec::new());
        harness.frame(Vec::new());
        assert_eq!(pane_count(&harness.app), 9);
    }

    #[test]
    fn klik_kanan_tab_membuka_menu_dan_duplikat_berjalan() {
        let mut harness = Harness::new();
        let source = harness.app.tabs.active();
        let position = test_positions::tab(WIDTH, 1, 0);
        harness.right_click(position);
        assert!(egui::Popup::is_any_open(&harness.ctx));

        // Baris pertama menu tepat di bawah pointer adalah "Duplikat tab".
        harness.click(position + vec2(30.0, 14.0));

        assert_eq!(harness.count(), 2);
        assert_eq!(harness.app.panes.len(), 2);
        assert_eq!(harness.app.tabs.active_index(), 1);
        assert_ne!(harness.app.tabs.active(), source);
        assert!(!egui::Popup::is_any_open(&harness.ctx));
    }

    #[test]
    fn menu_split_berlaku_untuk_tab_yang_diklik() {
        let mut harness = Harness::new();
        harness.press(Key::T, Modifiers::CTRL);
        assert_eq!(harness.app.tabs.active_index(), 1);

        let ctx = harness.ctx.clone();
        harness
            .app
            .apply_tab_action(&ctx, TabAction::Split(0, Axis::Horizontal));

        assert_eq!(harness.app.tabs.active_index(), 0);
        assert_eq!(harness.app.tabs.pane_counts(), vec![2, 1]);
        assert_eq!(harness.app.panes.len(), 3);
    }

    #[test]
    fn menu_tutup_split_hanya_menutup_panel_fokus() {
        let mut harness = Harness::new();
        harness.press(Key::D, ctrl_shift());
        harness.press(Key::T, Modifiers::CTRL);

        let ctx = harness.ctx.clone();
        harness.app.apply_tab_action(&ctx, TabAction::ClosePane(0));

        assert_eq!(harness.app.tabs.pane_counts(), vec![1, 1]);
        assert_eq!(harness.app.panes.len(), 2);
    }

    #[test]
    fn menu_duplikat_menaruh_tab_di_samping_sumber() {
        let mut harness = Harness::new();
        harness.press(Key::T, Modifiers::CTRL);
        let last = harness.app.tabs.active();

        let ctx = harness.ctx.clone();
        harness.app.apply_tab_action(&ctx, TabAction::Duplicate(0));

        assert_eq!(harness.count(), 3);
        assert_eq!(harness.app.tabs.active_index(), 1);
        assert_eq!(harness.app.tabs.focused_panes().last().copied(), last);
    }
}
