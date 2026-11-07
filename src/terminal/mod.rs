//! Emulasi terminal: PTY, integrasi alacritty_terminal, input mapping, dan renderer.

mod colors;
mod input;
mod pty;
mod session;
mod view;

pub use pty::default_shell;
pub use session::{GridSize, TerminalSession};
pub use view::TerminalView;
