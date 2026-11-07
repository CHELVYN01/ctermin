//! Satu sesi terminal: menghubungkan PTY dengan emulator alacritty_terminal.
//!
//! `Term` hanya disentuh di UI thread, sehingga tidak perlu lock. Output PTY datang lewat
//! channel dan diproses bertahap di [`TerminalSession::pump`].

use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};

use alacritty_terminal::event::{Event, EventListener, WindowSize};
use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::index::{Column, Line, Point, Side};
use alacritty_terminal::selection::{Selection, SelectionType};
use alacritty_terminal::term::{Config, TermMode};
use alacritty_terminal::vte::ansi::{Processor, Rgb};
use alacritty_terminal::Term;
use anyhow::Result;
use portable_pty::{CommandBuilder, PtySize};

use super::colors::Palette;
use super::input::encode_paste;
use super::pty::{Notify, Pty, PtyEvent};

/// Batas output yang diproses per frame, supaya perintah yang sangat ramai tidak membekukan UI.
const MAX_BYTES_PER_PUMP: usize = 1024 * 1024;
const MIN_COLUMNS: usize = 2;
const MIN_LINES: usize = 1;

/// Ukuran grid dalam sel.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct GridSize {
    pub columns: usize,
    pub lines: usize,
}

impl Dimensions for GridSize {
    fn total_lines(&self) -> usize {
        self.lines
    }

    fn screen_lines(&self) -> usize {
        self.lines
    }

    fn columns(&self) -> usize {
        self.columns
    }
}

impl GridSize {
    pub fn new(columns: usize, lines: usize) -> Self {
        Self {
            columns: columns.max(MIN_COLUMNS),
            lines: lines.max(MIN_LINES),
        }
    }

    fn to_pty_size(self) -> PtySize {
        PtySize {
            rows: u16::try_from(self.lines).unwrap_or(u16::MAX),
            cols: u16::try_from(self.columns).unwrap_or(u16::MAX),
            pixel_width: 0,
            pixel_height: 0,
        }
    }
}

/// Meneruskan kejadian dari `Term` agar bisa dibalas setelah parser selesai.
#[derive(Clone)]
pub struct EventProxy(Sender<Event>);

impl EventListener for EventProxy {
    fn send_event(&self, event: Event) {
        let _ = self.0.send(event);
    }
}

pub struct TerminalSession {
    term: Term<EventProxy>,
    processor: Processor,
    pty: Pty,
    term_events: Receiver<Event>,
    palette: Palette,
    size: GridSize,
    cell_pixels: (u16, u16),
    exited: bool,
}

impl TerminalSession {
    pub fn spawn(command: CommandBuilder, size: GridSize, notify: Notify) -> Result<Self> {
        let pty = Pty::spawn(command, size.to_pty_size(), notify)?;
        let (event_tx, term_events) = mpsc::channel();
        Ok(Self {
            term: Term::new(Config::default(), &size, EventProxy(event_tx)),
            processor: Processor::new(),
            pty,
            term_events,
            palette: Palette::new(),
            size,
            cell_pixels: (0, 0),
            exited: false,
        })
    }

    /// Memproses output yang sudah tiba. Mengembalikan kapan frame berikutnya dibutuhkan
    /// tanpa menunggu output baru.
    pub fn pump(&mut self) -> Option<Duration> {
        let mut processed = 0;
        while processed < MAX_BYTES_PER_PUMP {
            match self.pty.try_next_event() {
                Some(PtyEvent::Output(bytes)) => {
                    self.processor.advance(&mut self.term, &bytes);
                    processed += bytes.len();
                }
                Some(PtyEvent::Exited(code)) => self.mark_exited(code),
                None => break,
            }
        }
        self.answer_term_events();

        if processed >= MAX_BYTES_PER_PUMP {
            return Some(Duration::ZERO);
        }
        self.expire_synchronized_update()
    }

    pub fn exited(&self) -> bool {
        self.exited
    }

    pub fn term(&self) -> &Term<EventProxy> {
        &self.term
    }

    pub fn palette(&self) -> &Palette {
        &self.palette
    }

    pub fn mode(&self) -> TermMode {
        *self.term.mode()
    }

    /// Mengirim input dari keyboard: tampilan kembali ke baris terbaru dan seleksi dilepas.
    pub fn send_input(&mut self, bytes: Vec<u8>) {
        self.term.selection = None;
        self.term.scroll_display(Scroll::Bottom);
        self.pty.write(bytes);
    }

    pub fn paste(&mut self, text: &str) {
        let bracketed = self.mode().contains(TermMode::BRACKETED_PASTE);
        self.send_input(encode_paste(text, bracketed));
    }

    pub fn resize(&mut self, size: GridSize, cell_pixels: (u16, u16)) {
        self.cell_pixels = cell_pixels;
        if size != self.size {
            self.size = size;
            self.term.resize(size);
            self.pty.resize(size.to_pty_size());
        }
    }

    pub fn scroll(&mut self, scroll: Scroll) {
        self.term.scroll_display(scroll);
    }

    pub fn start_selection(&mut self, column: usize, line: usize, side: Side) {
        let point = self.display_point(column, line);
        self.term.selection = Some(Selection::new(SelectionType::Simple, point, side));
    }

    pub fn update_selection(&mut self, column: usize, line: usize, side: Side) {
        let point = self.display_point(column, line);
        if let Some(selection) = &mut self.term.selection {
            selection.update(point, side);
        }
    }

    pub fn clear_selection(&mut self) {
        self.term.selection = None;
    }

    /// Mengambil teks yang terseleksi lalu melepas seleksinya.
    pub fn take_selection(&mut self) -> Option<String> {
        let text = self.term.selection_to_string();
        self.term.selection = None;
        text.filter(|text| !text.is_empty())
    }

    fn display_point(&self, column: usize, line: usize) -> Point {
        let offset = self.term.grid().display_offset();
        Point::new(
            Line(line as i32 - offset as i32),
            Column(column.min(self.size.columns - 1)),
        )
    }

    fn mark_exited(&mut self, code: Option<u32>) {
        self.exited = true;
        let status = code.map_or_else(|| "tidak diketahui".to_owned(), |code| code.to_string());
        let message =
            format!("\r\n[Proses selesai, kode {status}. Tekan Enter untuk memulai shell baru.]");
        self.processor.advance(&mut self.term, message.as_bytes());
    }

    /// Membalas permintaan aplikasi. Ini wajib: ConPTY dan banyak aplikasi menunggu jawaban
    /// atas query seperti posisi kursor.
    ///
    /// Akses clipboard lewat OSC 52 sengaja diabaikan, supaya server jauh tidak bisa membaca
    /// atau menimpa clipboard user.
    fn answer_term_events(&mut self) {
        while let Ok(event) = self.term_events.try_recv() {
            match event {
                Event::PtyWrite(text) => self.pty.write(text.into_bytes()),
                Event::ColorRequest(index, format) => {
                    let color: Rgb = self.palette.indexed(index, self.term.colors());
                    self.pty.write(format(color).into_bytes());
                }
                Event::TextAreaSizeRequest(format) => {
                    let (cell_width, cell_height) = self.cell_pixels;
                    let size = self.size.to_pty_size();
                    let reply = format(WindowSize {
                        num_lines: size.rows,
                        num_cols: size.cols,
                        cell_width,
                        cell_height,
                    });
                    self.pty.write(reply.into_bytes());
                }
                _ => {}
            }
        }
    }

    /// Aplikasi yang berhenti di tengah synchronized update tidak boleh membekukan layar.
    fn expire_synchronized_update(&mut self) -> Option<Duration> {
        let deadline = self.processor.sync_timeout().sync_timeout()?;
        let now = Instant::now();
        if now >= deadline {
            self.processor.stop_sync(&mut self.term);
            self.answer_term_events();
            None
        } else {
            Some(deadline - now)
        }
    }
}

#[cfg(test)]
pub(super) mod tests {
    use std::sync::Arc;
    use std::thread;

    use super::*;

    const TIMEOUT: Duration = Duration::from_secs(20);

    pub fn spawn(args: &[&str]) -> TerminalSession {
        let mut command = CommandBuilder::new("cmd.exe");
        command.args(args);
        TerminalSession::spawn(command, GridSize::new(100, 30), Arc::new(|| {}))
            .expect("sesi harus berhasil dibuat")
    }

    pub fn screen_text(session: &TerminalSession) -> String {
        session
            .term
            .grid()
            .display_iter()
            .map(|cell| cell.c)
            .collect()
    }

    pub fn pump_until(session: &mut TerminalSession, done: impl Fn(&TerminalSession) -> bool) {
        let start = Instant::now();
        while !done(session) {
            assert!(
                start.elapsed() < TIMEOUT,
                "waktu habis, isi layar: {}",
                screen_text(session).trim()
            );
            session.pump();
            thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn output_proses_masuk_ke_grid() {
        let mut session = spawn(&["/c", "echo ctermin-ok"]);
        pump_until(&mut session, |s| s.exited);
        assert!(screen_text(&session).contains("ctermin-ok"));
        assert!(screen_text(&session).contains("kode 0"));
    }

    #[test]
    fn kode_keluar_dilaporkan() {
        let mut session = spawn(&["/c", "exit 7"]);
        pump_until(&mut session, |s| s.exited);
        assert!(screen_text(&session).contains("kode 7"));
    }

    #[test]
    fn input_sampai_ke_proses() {
        let mut session = spawn(&[]);
        session.send_input(b"echo hasil-%OS%\r".to_vec());
        pump_until(&mut session, |s| {
            screen_text(s).contains("hasil-Windows_NT")
        });
    }

    #[test]
    fn resize_mengubah_grid_dan_proses_melihatnya() {
        let mut session = spawn(&[]);
        session.resize(GridSize::new(70, 20), (8, 16));
        assert_eq!(session.term.columns(), 70);
        assert_eq!(session.term.screen_lines(), 20);

        session.send_input(b"mode con\r".to_vec());
        pump_until(&mut session, |s| {
            let text: String = screen_text(s).split_whitespace().collect();
            text.contains("Columns:70") || text.contains("Kolom:70")
        });
    }

    #[test]
    fn menutup_sesi_menghentikan_semua_thread() {
        let mut session = spawn(&[]);
        pump_until(&mut session, |s| !screen_text(s).trim().is_empty());
        let threads = session.pty.threads_alive();
        assert!(threads.upgrade().is_some());

        drop(session);

        let start = Instant::now();
        while threads.upgrade().is_some() {
            assert!(start.elapsed() < TIMEOUT, "thread PTY masih hidup");
            thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn thread_berhenti_saat_proses_keluar_sendiri() {
        let mut session = spawn(&["/c", "exit 0"]);
        let threads = session.pty.threads_alive();
        pump_until(&mut session, |s| s.exited);

        let start = Instant::now();
        while threads.upgrade().is_some() {
            assert!(start.elapsed() < TIMEOUT, "thread PTY masih hidup");
            thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn seleksi_dan_scrollback() {
        let mut session = spawn(&["/c", "for /L %i in (1,1,200) do @echo baris-%i"]);
        pump_until(&mut session, |s| s.exited);

        session.scroll(Scroll::Top);
        assert!(session.term.grid().display_offset() > 0);
        assert!(screen_text(&session).contains("baris-1"));

        session.start_selection(0, 0, Side::Left);
        session.update_selection(5, 0, Side::Right);
        assert!(session.take_selection().is_some());
        assert!(session.term.selection.is_none());

        session.scroll(Scroll::Bottom);
        assert_eq!(session.term.grid().display_offset(), 0);
    }
}
