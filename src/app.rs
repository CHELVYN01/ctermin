use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use eframe::egui::{self, pos2, Key, Rect, UiBuilder, ViewportCommand};
use portable_pty::CommandBuilder;

use crate::layout::{PaneId, Tabs};
use crate::terminal::{default_shell, GridSize, TerminalSession, TerminalView};
use crate::ui::shortcuts::{self, Shortcut};
use crate::ui::tab_bar::{self, TabAction, BAR_HEIGHT};

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

    /// Melepas sesi akan mematikan proses dan menghentikan thread PTY-nya.
    fn close_tab(&mut self, index: usize) {
        if let Some(pane) = self.tabs.close(index) {
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
                Shortcut::Close => self.close_tab(self.tabs.active_index()),
                Shortcut::Next => self.tabs.next(),
                Shortcut::Previous => self.tabs.previous(),
            }
        }
    }

    fn apply_tab_action(&mut self, ctx: &egui::Context, action: TabAction) {
        match action {
            TabAction::Select(index) => self.tabs.select(index),
            TabAction::Close(index) => self.close_tab(index),
            TabAction::New => self.open_tab(ctx),
        }
    }

    fn tab_titles(&self) -> Vec<String> {
        self.tabs
            .panes()
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

        let titles = self.tab_titles();
        let active = self.tabs.active_index();
        let action = ui
            .scope_builder(UiBuilder::new().max_rect(bar), |ui| {
                tab_bar::show(ui, &titles, active)
            })
            .inner;
        if let Some(action) = action {
            self.apply_tab_action(&ctx, action);
        }

        if self.tabs.is_empty() {
            ctx.send_viewport_cmd(ViewportCommand::Close);
            return;
        }

        let pane = self
            .tabs
            .active()
            .and_then(|pane| self.panes.get_mut(&pane));
        let view = &mut self.view;
        ui.scope_builder(UiBuilder::new().max_rect(content), |ui| match pane {
            Some(Ok(session)) => view.show(ui, session),
            Some(Err(message)) => {
                ui.colored_label(ui.visuals().error_fg_color, message.as_str());
            }
            None => {}
        });

        self.sync_window_title(&ctx);
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

    use eframe::egui::{
        vec2, Event, FullOutput, Modifiers, PointerButton, Pos2, RawInput, ViewportId,
    };

    use super::*;
    use crate::ui::tab_bar::test_positions;

    const WIDTH: f32 = 800.0;
    const TIMEOUT: Duration = Duration::from_secs(20);

    fn cmd_shell() -> CommandBuilder {
        CommandBuilder::new("cmd.exe")
    }

    /// Menjalankan aplikasi tanpa jendela, dengan urutan yang sama seperti eframe.
    struct Harness {
        ctx: egui::Context,
        app: CterminApp,
        time: f64,
    }

    impl Harness {
        fn new() -> Self {
            let ctx = egui::Context::default();
            let app = CterminApp::with_shell(&ctx, cmd_shell);
            let mut harness = Self {
                ctx,
                app,
                time: 0.0,
            };
            harness.frame(Vec::new());
            harness
        }

        fn frame(&mut self, events: Vec<Event>) -> FullOutput {
            self.time += 0.05;
            let input = RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(WIDTH, 600.0))),
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
            self.app.tabs.panes().len()
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
        let first = harness.app.tabs.panes()[0];

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
}
