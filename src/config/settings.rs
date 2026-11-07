//! Application settings stored in `config.toml`.

use serde::{Deserialize, Serialize};

use crate::ai::settings::AiSettings;
use crate::terminal::{ColorScheme, ShellKind, TerminalFont};

pub const MIN_FONT_SIZE: f32 = 8.0;
pub const MAX_FONT_SIZE: f32 = 32.0;
const DEFAULT_FONT_SIZE: f32 = 14.0;

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Appearance {
    pub color_scheme: ColorScheme,
    pub font: TerminalFont,
    pub font_size: f32,
}

impl Default for Appearance {
    fn default() -> Self {
        Self {
            color_scheme: ColorScheme::default(),
            font: TerminalFont::default(),
            font_size: DEFAULT_FONT_SIZE,
        }
    }
}

impl Appearance {
    /// `config.toml` can be edited by hand, so the size is limited wherever it is used.
    pub fn clamped_font_size(&self) -> f32 {
        if self.font_size.is_finite() {
            self.font_size.clamp(MIN_FONT_SIZE, MAX_FONT_SIZE)
        } else {
            DEFAULT_FONT_SIZE
        }
    }
}

#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct TerminalSettings {
    pub default_shell: ShellKind,
    pub restore_session: bool,
}

impl Default for TerminalSettings {
    fn default() -> Self {
        Self {
            default_shell: ShellKind::default(),
            restore_session: true,
        }
    }
}

/// Key bindings written like `Ctrl+Shift+D`. They are edited in `config.toml`.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct ShortcutSettings {
    pub new_tab: String,
    pub close_pane: String,
    pub next_tab: String,
    pub previous_tab: String,
    pub split_right: String,
    pub split_down: String,
    pub focus_left: String,
    pub focus_right: String,
    pub focus_up: String,
    pub focus_down: String,
    pub toggle_sidebar: String,
    pub ai_bar: String,
    pub settings: String,
}

impl Default for ShortcutSettings {
    fn default() -> Self {
        let binding = |text: &str| text.to_owned();
        Self {
            new_tab: binding("Ctrl+T"),
            close_pane: binding("Ctrl+W"),
            next_tab: binding("Ctrl+Tab"),
            previous_tab: binding("Ctrl+Shift+Tab"),
            split_right: binding("Ctrl+Shift+D"),
            split_down: binding("Ctrl+Shift+E"),
            focus_left: binding("Alt+Left"),
            focus_right: binding("Alt+Right"),
            focus_up: binding("Alt+Up"),
            focus_down: binding("Alt+Down"),
            toggle_sidebar: binding("Ctrl+Shift+B"),
            ai_bar: binding("Ctrl+K"),
            settings: binding("Ctrl+Comma"),
        }
    }
}

#[derive(Clone, PartialEq, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    pub ai: AiSettings,
    pub appearance: Appearance,
    pub terminal: TerminalSettings,
    pub shortcuts: ShortcutSettings,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_config_without_new_sections_still_loads() {
        let config: AppConfig =
            toml::from_str("[ai]\nprovider = \"api-key\"\n").expect("old config is valid");
        assert!(config.terminal.restore_session);
        assert_eq!(config.shortcuts.new_tab, "Ctrl+T");
        assert_eq!(config.appearance.font_size, DEFAULT_FONT_SIZE);
    }

    #[test]
    fn partial_sections_keep_their_other_defaults() {
        let config: AppConfig =
            toml::from_str("[shortcuts]\nclose_pane = \"Ctrl+Shift+W\"\n").expect("valid");
        assert_eq!(config.shortcuts.close_pane, "Ctrl+Shift+W");
        assert_eq!(config.shortcuts.new_tab, "Ctrl+T");
    }

    #[test]
    fn settings_survive_a_save_and_load() {
        let mut config = AppConfig::default();
        config.appearance.color_scheme = ColorScheme::Dracula;
        config.appearance.font = TerminalFont::Consolas;
        config.terminal.default_shell = ShellKind::GitBash;
        config.terminal.restore_session = false;

        let text = toml::to_string_pretty(&config).expect("serializable");
        assert_eq!(toml::from_str::<AppConfig>(&text).ok(), Some(config));
    }

    #[test]
    fn hand_edited_font_sizes_are_limited() {
        let size = |font_size| {
            Appearance {
                font_size,
                ..Appearance::default()
            }
            .clamped_font_size()
        };
        assert_eq!(size(2.0), MIN_FONT_SIZE);
        assert_eq!(size(500.0), MAX_FONT_SIZE);
        assert_eq!(size(f32::NAN), DEFAULT_FONT_SIZE);
    }
}
