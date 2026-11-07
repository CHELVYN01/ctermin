//! One terminal session: connects a PTY to the alacritty_terminal emulator.
//!
//! `Term` is only touched on the UI thread, so no lock is needed. PTY output arrives through a
//! channel and is processed in steps in [`TerminalSession::pump`].

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

/// Output processed per frame, so a very chatty command cannot freeze the UI.
const MAX_BYTES_PER_PUMP: usize = 1024 * 1024;
const MIN_COLUMNS: usize = 2;
const MIN_LINES: usize = 1;
const MAX_TITLE_CHARS: usize = 256;
const EXE_SUFFIX: &str = ".exe";

/// Grid size in cells.
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

/// Forwards events from `Term` so they can be answered after the parser finishes.
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
    /// The program name, used as the title until the app sends its own.
    program: String,
    title: Option<String>,
    server_name: Option<String>,
}

impl TerminalSession {
    pub fn spawn(command: CommandBuilder, size: GridSize, notify: Notify) -> Result<Self> {
        let program = program_name(&command);
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
            program,
            title: None,
            server_name: None,
        })
    }

    pub fn title(&self) -> &str {
        self.server_name
            .as_deref()
            .or(self.title.as_deref())
            .unwrap_or(&self.program)
    }

    /// Marks this session as a server connection: its title stays the server name, even if the
    /// server sends its own.
    pub fn set_server_name(&mut self, name: &str) {
        self.server_name = Some(name.to_owned());
    }

    /// Processes the output that has arrived. Returns when the next frame is needed
    /// without waiting for new output.
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

    /// Sends keyboard input: the view returns to the newest line and the selection clears.
    pub fn send_input(&mut self, bytes: Vec<u8>) {
        self.term.selection = None;
        self.term.scroll_display(Scroll::Bottom);
        self.pty.write(bytes);
    }

    pub fn paste(&mut self, text: &str) {
        let bracketed = self.mode().contains(TermMode::BRACKETED_PASTE);
        self.send_input(encode_paste(text, bracketed));
    }

    /// The last lines of the screen and history, up to the cursor. Used as AI context.
    pub fn recent_output(&self, lines: usize) -> String {
        let grid = self.term.grid();
        let end_line = grid.cursor.point.line;
        let count = i32::try_from(lines).unwrap_or(i32::MAX);
        let start_line = Line((end_line.0 + 1).saturating_sub(count)).max(grid.topmost_line());
        let start = Point::new(start_line, Column(0));
        let end = Point::new(end_line, grid.last_column());
        self.term.bounds_to_string(start, end).trim().to_owned()
    }

    /// The line the cursor is on, usually the shell prompt.
    pub fn prompt_line(&self) -> String {
        self.recent_output(1)
    }

    /// Inserts a command on the prompt line without running it. The command must already be free of
    /// newlines and control characters.
    pub fn insert_command(&mut self, command: &str) {
        self.send_input(command.as_bytes().to_vec());
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

    /// Takes the selected text and clears the selection.
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
        let status = code.map_or_else(|| "unknown".to_owned(), |code| code.to_string());
        let (ended, next) = if self.server_name.is_some() {
            ("Disconnected", "reconnect")
        } else {
            ("Process exited", "start a new shell")
        };
        let message = format!("\r\n[{ended}, code {status}. Press Enter to {next}.]");
        self.processor.advance(&mut self.term, message.as_bytes());
    }

    /// Answers application requests. This is required: ConPTY and many apps wait for replies to
    /// queries such as the cursor position.
    ///
    /// Clipboard access through OSC 52 is ignored on purpose, so a remote server cannot read
    /// or overwrite the user's clipboard.
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
                Event::Title(title) => self.title = clean_title(&title),
                Event::ResetTitle => self.title = None,
                _ => {}
            }
        }
    }

    /// An app that stops in the middle of a synchronized update must not freeze the screen.
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

    #[cfg(test)]
    pub fn screen_text(&self) -> String {
        self.term.grid().display_iter().map(|cell| cell.c).collect()
    }

    #[cfg(test)]
    pub fn threads_alive(&self) -> std::sync::Weak<()> {
        self.pty.threads_alive()
    }
}

/// A program's file name without folder or extension.
fn program_name(command: &CommandBuilder) -> String {
    command
        .get_argv()
        .first()
        .map(|program| executable_name(&program.to_string_lossy()))
        .unwrap_or_default()
}

fn executable_name(path: &str) -> String {
    let file = path.rsplit(['\\', '/']).next().unwrap_or(path);
    match file.len().checked_sub(EXE_SUFFIX.len()) {
        Some(stem)
            if file.is_char_boundary(stem) && file[stem..].eq_ignore_ascii_case(EXE_SUFFIX) =>
        {
            file[..stem].to_owned()
        }
        _ => file.to_owned(),
    }
}

/// Tidies a title from an app. Titles can come from a remote server, so control characters
/// are removed and the length is capped. ConPTY sends the shell's full path, which is shown
/// as just the program name.
fn clean_title(raw: &str) -> Option<String> {
    let cleaned: String = raw
        .chars()
        .filter(|character| !character.is_control())
        .take(MAX_TITLE_CHARS)
        .collect();
    let title = cleaned.trim();
    if title.is_empty() {
        None
    } else if title.to_ascii_lowercase().ends_with(EXE_SUFFIX) {
        Some(executable_name(title))
    } else {
        Some(title.to_owned())
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
            .expect("the session must be created")
    }

    pub fn screen_text(session: &TerminalSession) -> String {
        session.screen_text()
    }

    #[test]
    fn initial_title_is_the_program_name() {
        let session = spawn(&["/c", "exit 0"]);
        assert_eq!(session.title(), "cmd");
    }

    #[test]
    fn app_title_is_used() {
        let mut session = spawn(&["/k", "title test-title"]);
        pump_until(&mut session, |s| s.title().contains("test-title"));
    }

    #[test]
    fn program_name_from_path() {
        let path = r"C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe";
        assert_eq!(executable_name(path), "powershell");
        assert_eq!(executable_name("cmd.EXE"), "cmd");
        assert_eq!(executable_name("/usr/bin/ssh"), "ssh");
        assert_eq!(executable_name("ssh"), "ssh");
        assert_eq!(executable_name("é.exe"), "é");
    }

    #[test]
    fn titles_are_cleaned() {
        assert_eq!(
            clean_title(r"C:\WINDOWS\system32\cmd.exe").as_deref(),
            Some("cmd")
        );
        assert_eq!(
            clean_title(r"Cargo.toml (D:\Project) - VIM").as_deref(),
            Some(r"Cargo.toml (D:\Project) - VIM")
        );
        assert_eq!(clean_title("a\x1b[31m\x07b\n").as_deref(), Some("a[31mb"));
        assert_eq!(clean_title("  \x07 "), None);
        let long = "x".repeat(MAX_TITLE_CHARS + 50);
        assert_eq!(
            clean_title(&long).map(|title| title.chars().count()),
            Some(MAX_TITLE_CHARS)
        );
    }

    pub fn pump_until(session: &mut TerminalSession, done: impl Fn(&TerminalSession) -> bool) {
        let start = Instant::now();
        while !done(session) {
            assert!(
                start.elapsed() < TIMEOUT,
                "timed out, screen contents: {}",
                screen_text(session).trim()
            );
            session.pump();
            thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn process_output_reaches_the_grid() {
        let mut session = spawn(&["/c", "echo ctermin-ok"]);
        pump_until(&mut session, |s| s.exited);
        assert!(screen_text(&session).contains("ctermin-ok"));
        assert!(screen_text(&session).contains("code 0"));
    }

    #[test]
    fn exit_code_is_reported() {
        let mut session = spawn(&["/c", "exit 7"]);
        pump_until(&mut session, |s| s.exited);
        assert!(screen_text(&session).contains("code 7"));
    }

    #[test]
    fn input_reaches_the_process() {
        let mut session = spawn(&[]);
        session.send_input(b"echo result-%OS%\r".to_vec());
        pump_until(&mut session, |s| {
            screen_text(s).contains("result-Windows_NT")
        });
    }

    #[test]
    fn resize_changes_the_grid_and_the_process_sees_it() {
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
    fn closing_a_session_stops_all_threads() {
        let mut session = spawn(&[]);
        pump_until(&mut session, |s| !screen_text(s).trim().is_empty());
        let threads = session.pty.threads_alive();
        assert!(threads.upgrade().is_some());

        drop(session);

        let start = Instant::now();
        while threads.upgrade().is_some() {
            assert!(start.elapsed() < TIMEOUT, "PTY threads are still alive");
            thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn threads_stop_when_the_process_exits_itself() {
        let mut session = spawn(&["/c", "exit 0"]);
        let threads = session.pty.threads_alive();
        pump_until(&mut session, |s| s.exited);

        let start = Instant::now();
        while threads.upgrade().is_some() {
            assert!(start.elapsed() < TIMEOUT, "PTY threads are still alive");
            thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn selection_and_scrollback() {
        let mut session = spawn(&["/c", "for /L %i in (1,1,200) do @echo line-%i"]);
        pump_until(&mut session, |s| s.exited);

        session.scroll(Scroll::Top);
        assert!(session.term.grid().display_offset() > 0);
        assert!(screen_text(&session).contains("line-1"));

        session.start_selection(0, 0, Side::Left);
        session.update_selection(5, 0, Side::Right);
        assert!(session.take_selection().is_some());
        assert!(session.term.selection.is_none());

        session.scroll(Scroll::Bottom);
        assert_eq!(session.term.grid().display_offset(), 0);
    }
}
