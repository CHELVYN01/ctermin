//! The session file: which tabs and panes to bring back on the next start.
//!
//! Only the arrangement is saved. Screen contents and the active folder are not.

use serde::{Deserialize, Serialize};

use super::tree::Axis;
use crate::ssh::model::ServerId;
use crate::terminal::ShellKind;

/// What a pane runs. Split, duplicate, and reconnect use the same source.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum PaneSource {
    Shell { shell: ShellKind },
    Server { id: ServerId },
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(tag = "node", rename_all = "lowercase")]
pub enum SavedNode {
    Pane {
        source: PaneSource,
    },
    Split {
        axis: Axis,
        ratio: f32,
        first: Box<SavedNode>,
        second: Box<SavedNode>,
    },
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct SavedTab {
    pub root: SavedNode,
}

#[derive(Clone, PartialEq, Debug, Default, Serialize, Deserialize)]
pub struct SavedSession {
    #[serde(default)]
    pub active: usize,
    #[serde(default)]
    pub tabs: Vec<SavedTab>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nested_layout_survives_toml() {
        let shell = |shell| SavedNode::Pane {
            source: PaneSource::Shell { shell },
        };
        let session = SavedSession {
            active: 1,
            tabs: vec![
                SavedTab {
                    root: shell(ShellKind::Cmd),
                },
                SavedTab {
                    root: SavedNode::Split {
                        axis: Axis::Horizontal,
                        ratio: 0.25,
                        first: Box::new(shell(ShellKind::PowerShell)),
                        second: Box::new(SavedNode::Split {
                            axis: Axis::Vertical,
                            ratio: 0.5,
                            first: Box::new(SavedNode::Pane {
                                source: PaneSource::Server { id: ServerId(7) },
                            }),
                            second: Box::new(shell(ShellKind::GitBash)),
                        }),
                    },
                },
            ],
        };

        let text = toml::to_string_pretty(&session).expect("serializable");
        assert_eq!(toml::from_str::<SavedSession>(&text).ok(), Some(session));
    }
}
