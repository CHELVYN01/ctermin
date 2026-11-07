use std::sync::Arc;

use eframe::egui;

use crate::terminal::{default_shell, GridSize, TerminalSession, TerminalView};

const INITIAL_COLUMNS: usize = 80;
const INITIAL_LINES: usize = 24;

/// State aplikasi utama.
pub struct CterminApp {
    session: Result<TerminalSession, String>,
    view: TerminalView,
}

impl CterminApp {
    pub fn new(ctx: &egui::Context) -> Self {
        // Ctrl+Plus dan Ctrl+Minus harus sampai ke terminal, bukan mengubah zoom egui.
        ctx.options_mut(|options| options.zoom_with_keyboard = false);
        Self {
            session: spawn_shell(ctx),
            view: TerminalView::new(),
        }
    }
}

impl eframe::App for CterminApp {
    // Output diproses di sini, bukan di `ui`, karena `ui` tidak dipanggil saat jendela
    // diminimalkan dan shell akan terhenti menunggu antrean kosong.
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let Ok(session) = &mut self.session else {
            return;
        };
        if let Some(delay) = session.pump() {
            ctx.request_repaint_after(delay);
        }
        if session.exited() && ctx.input(|input| input.key_pressed(egui::Key::Enter)) {
            self.session = spawn_shell(ctx);
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        match &mut self.session {
            Ok(session) => self.view.show(ui, session),
            Err(message) => {
                egui::CentralPanel::default().show(ui, |ui| {
                    ui.colored_label(ui.visuals().error_fg_color, message.as_str());
                });
            }
        }
    }
}

fn spawn_shell(ctx: &egui::Context) -> Result<TerminalSession, String> {
    let ctx = ctx.clone();
    let notify = Arc::new(move || ctx.request_repaint());
    let size = GridSize::new(INITIAL_COLUMNS, INITIAL_LINES);
    TerminalSession::spawn(default_shell(), size, notify).map_err(|err| format!("{err:#}"))
}
