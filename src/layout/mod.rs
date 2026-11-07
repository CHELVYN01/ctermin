//! Tab and split pane layout.

mod tabs;
mod tree;

pub use tabs::{PaneId, Tabs};
pub use tree::{Axis, Branch, Direction, Divider, DIVIDER_THICKNESS};
