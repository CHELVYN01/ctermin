//! App-level shortcuts. Their keys are taken out of the event queue so they never reach the terminal.

use eframe::egui::{self, Event, Key, Modifiers};

use crate::config::settings::ShortcutSettings;
use crate::layout::{Axis, Direction};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Shortcut {
    Open,
    Close,
    Next,
    Previous,
    Split(Axis),
    Focus(Direction),
    ToggleSidebar,
    AiBar,
    Settings,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Binding {
    key: Key,
    modifiers: Modifiers,
}

/// Reads a binding such as `Ctrl+Shift+D`. Modifier names ignore case.
fn parse_binding(text: &str) -> Option<Binding> {
    let mut parts: Vec<&str> = text.split('+').map(str::trim).collect();
    let key = parts.pop()?;
    let mut modifiers = Modifiers::NONE;
    for part in parts {
        match part.to_ascii_lowercase().as_str() {
            "ctrl" => modifiers.ctrl = true,
            "shift" => modifiers.shift = true,
            "alt" => modifiers.alt = true,
            _ => return None,
        }
    }
    let key = Key::from_name(key).or_else(|| Key::from_name(&key.to_ascii_uppercase()))?;
    Some(Binding { key, modifiers })
}

/// App shortcuts in effect, built from `config.toml`.
#[derive(Clone, Debug)]
pub struct Keymap(Vec<(Binding, Shortcut)>);

impl Keymap {
    /// Invalid bindings fall back to their default. The second value lists them for the user.
    pub fn from_settings(settings: &ShortcutSettings) -> (Self, Vec<String>) {
        let defaults = ShortcutSettings::default();
        let entries = [
            (&settings.new_tab, &defaults.new_tab, Shortcut::Open),
            (&settings.close_pane, &defaults.close_pane, Shortcut::Close),
            (&settings.next_tab, &defaults.next_tab, Shortcut::Next),
            (
                &settings.previous_tab,
                &defaults.previous_tab,
                Shortcut::Previous,
            ),
            (
                &settings.split_right,
                &defaults.split_right,
                Shortcut::Split(Axis::Horizontal),
            ),
            (
                &settings.split_down,
                &defaults.split_down,
                Shortcut::Split(Axis::Vertical),
            ),
            (
                &settings.focus_left,
                &defaults.focus_left,
                Shortcut::Focus(Direction::Left),
            ),
            (
                &settings.focus_right,
                &defaults.focus_right,
                Shortcut::Focus(Direction::Right),
            ),
            (
                &settings.focus_up,
                &defaults.focus_up,
                Shortcut::Focus(Direction::Up),
            ),
            (
                &settings.focus_down,
                &defaults.focus_down,
                Shortcut::Focus(Direction::Down),
            ),
            (
                &settings.toggle_sidebar,
                &defaults.toggle_sidebar,
                Shortcut::ToggleSidebar,
            ),
            (&settings.ai_bar, &defaults.ai_bar, Shortcut::AiBar),
            (&settings.settings, &defaults.settings, Shortcut::Settings),
        ];

        let mut bindings = Vec::new();
        let mut invalid = Vec::new();
        for (text, default, shortcut) in entries {
            let binding = parse_binding(text).or_else(|| {
                invalid.push(text.clone());
                parse_binding(default)
            });
            if let Some(binding) = binding {
                bindings.push((binding, shortcut));
            }
        }
        (Self(bindings), invalid)
    }

    fn lookup(&self, key: Key, modifiers: Modifiers) -> Option<Shortcut> {
        self.0
            .iter()
            .find(|(binding, _)| binding.key == key && modifiers.matches_exact(binding.modifiers))
            .map(|(_, shortcut)| *shortcut)
    }
}

impl Default for Keymap {
    fn default() -> Self {
        Self::from_settings(&ShortcutSettings::default()).0
    }
}

/// Auto-repeat only applies to moving around. Holding Ctrl+W must not close every pane,
/// and holding Ctrl+Shift+D must not create dozens of panes.
fn repeats(shortcut: Shortcut) -> bool {
    matches!(
        shortcut,
        Shortcut::Next | Shortcut::Previous | Shortcut::Focus(_)
    )
}

/// Takes every shortcut in this frame.
pub fn take(ctx: &egui::Context, keymap: &Keymap) -> Vec<Shortcut> {
    let mut taken = Vec::new();
    ctx.input_mut(|input| {
        input.events.retain(|event| {
            let Event::Key {
                key,
                pressed: true,
                repeat,
                modifiers,
                ..
            } = event
            else {
                return true;
            };
            let Some(shortcut) = keymap.lookup(*key, *modifiers) else {
                return true;
            };
            if !*repeat || repeats(shortcut) {
                taken.push(shortcut);
            }
            false
        });
    });
    taken
}

#[cfg(test)]
mod tests {
    use eframe::egui::RawInput;

    use super::*;

    fn shortcut(key: Key, modifiers: Modifiers) -> Option<Shortcut> {
        Keymap::default().lookup(key, modifiers)
    }

    fn key_event(key: Key, modifiers: Modifiers, repeat: bool) -> Event {
        Event::Key {
            key,
            physical_key: Some(key),
            pressed: true,
            repeat,
            modifiers,
        }
    }

    /// Returns the shortcuts taken and how many events remain for the terminal.
    fn run(events: Vec<Event>) -> (Vec<Shortcut>, usize) {
        let ctx = egui::Context::default();
        let mut result = (Vec::new(), 0);
        let input = RawInput {
            events,
            ..RawInput::default()
        };
        let mut output = ctx.run_ui(input, |ui| {
            let taken = take(ui.ctx(), &Keymap::default());
            let left = ui.input(|input| input.events.len());
            result = (taken, left);
        });
        output.textures_delta.clear();
        result
    }

    #[test]
    fn key_mapping() {
        let ctrl = Modifiers::CTRL;
        assert_eq!(shortcut(Key::T, ctrl), Some(Shortcut::Open));
        assert_eq!(shortcut(Key::W, ctrl), Some(Shortcut::Close));
        assert_eq!(shortcut(Key::Tab, ctrl), Some(Shortcut::Next));
        assert_eq!(
            shortcut(Key::Tab, ctrl | Modifiers::SHIFT),
            Some(Shortcut::Previous)
        );
    }

    #[test]
    fn other_combinations_stay_with_the_terminal() {
        assert_eq!(shortcut(Key::T, Modifiers::NONE), None);
        assert_eq!(shortcut(Key::Tab, Modifiers::NONE), None);
        assert_eq!(shortcut(Key::Tab, Modifiers::SHIFT), None);
        assert_eq!(shortcut(Key::W, Modifiers::CTRL | Modifiers::SHIFT), None);
        assert_eq!(shortcut(Key::C, Modifiers::CTRL), None);
    }

    #[test]
    fn shortcuts_do_not_reach_the_terminal() {
        let (taken, left) = run(vec![
            key_event(Key::T, Modifiers::CTRL, false),
            key_event(Key::A, Modifiers::NONE, false),
        ]);
        assert_eq!(taken, vec![Shortcut::Open]);
        assert_eq!(left, 1);
    }

    #[test]
    fn holding_ctrl_w_closes_only_once() {
        let (taken, left) = run(vec![
            key_event(Key::W, Modifiers::CTRL, false),
            key_event(Key::W, Modifiers::CTRL, true),
            key_event(Key::W, Modifiers::CTRL, true),
        ]);
        assert_eq!(taken, vec![Shortcut::Close]);
        assert_eq!(left, 0);
    }

    #[test]
    fn holding_ctrl_tab_keeps_moving() {
        let (taken, _) = run(vec![
            key_event(Key::Tab, Modifiers::CTRL, false),
            key_event(Key::Tab, Modifiers::CTRL, true),
        ]);
        assert_eq!(taken, vec![Shortcut::Next, Shortcut::Next]);
    }

    #[test]
    fn split_and_focus_mapping() {
        let ctrl_shift = Modifiers::CTRL | Modifiers::SHIFT;
        assert_eq!(
            shortcut(Key::D, ctrl_shift),
            Some(Shortcut::Split(Axis::Horizontal))
        );
        assert_eq!(
            shortcut(Key::E, ctrl_shift),
            Some(Shortcut::Split(Axis::Vertical))
        );
        assert_eq!(
            shortcut(Key::ArrowLeft, Modifiers::ALT),
            Some(Shortcut::Focus(Direction::Left))
        );
        assert_eq!(
            shortcut(Key::ArrowDown, Modifiers::ALT),
            Some(Shortcut::Focus(Direction::Down))
        );
        assert_eq!(shortcut(Key::ArrowLeft, Modifiers::NONE), None);
        assert_eq!(shortcut(Key::D, Modifiers::CTRL), None);
    }

    #[test]
    fn holding_ctrl_shift_d_splits_only_once() {
        let ctrl_shift = Modifiers::CTRL | Modifiers::SHIFT;
        let (taken, _) = run(vec![
            key_event(Key::D, ctrl_shift, false),
            key_event(Key::D, ctrl_shift, true),
        ]);
        assert_eq!(taken, vec![Shortcut::Split(Axis::Horizontal)]);
    }
}
