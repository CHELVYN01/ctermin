//! Terminal emulation: PTY, alacritty_terminal integration, input mapping, and renderer.

mod colors;
mod fonts;
mod input;
mod pty;
mod session;
mod shell;
mod view;

pub use colors::ColorScheme;
pub use fonts::{font_definitions, TerminalFont};
pub use session::{GridSize, TerminalSession};
pub use shell::ShellKind;
pub use view::TerminalView;
