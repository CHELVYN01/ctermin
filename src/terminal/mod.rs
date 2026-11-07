//! Terminal emulation: PTY, alacritty_terminal integration, input mapping, and renderer.

mod colors;
mod input;
mod pty;
mod session;
mod view;

pub use pty::default_shell;
pub use session::{GridSize, TerminalSession};
pub use view::TerminalView;
