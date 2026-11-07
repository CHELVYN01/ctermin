//! App-level shortcuts. Their keys are taken out of the event queue so they never reach the terminal.

use eframe::egui::{self, Event, Key, Modifiers};

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
}

fn shortcut(key: Key, modifiers: Modifiers) -> Option<Shortcut> {
    let ctrl_shift = Modifiers::CTRL | Modifiers::SHIFT;
    match key {
        Key::T if modifiers.matches_exact(Modifiers::CTRL) => Some(Shortcut::Open),
        Key::W if modifiers.matches_exact(Modifiers::CTRL) => Some(Shortcut::Close),
        Key::Tab if modifiers.matches_exact(Modifiers::CTRL) => Some(Shortcut::Next),
        Key::Tab if modifiers.matches_exact(ctrl_shift) => Some(Shortcut::Previous),
        Key::D if modifiers.matches_exact(ctrl_shift) => Some(Shortcut::Split(Axis::Horizontal)),
        Key::E if modifiers.matches_exact(ctrl_shift) => Some(Shortcut::Split(Axis::Vertical)),
        Key::B if modifiers.matches_exact(ctrl_shift) => Some(Shortcut::ToggleSidebar),
        Key::K if modifiers.matches_exact(Modifiers::CTRL) => Some(Shortcut::AiBar),
        Key::ArrowLeft if modifiers.matches_exact(Modifiers::ALT) => {
            Some(Shortcut::Focus(Direction::Left))
        }
        Key::ArrowRight if modifiers.matches_exact(Modifiers::ALT) => {
            Some(Shortcut::Focus(Direction::Right))
        }
        Key::ArrowUp if modifiers.matches_exact(Modifiers::ALT) => {
            Some(Shortcut::Focus(Direction::Up))
        }
        Key::ArrowDown if modifiers.matches_exact(Modifiers::ALT) => {
            Some(Shortcut::Focus(Direction::Down))
        }
        _ => None,
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
pub fn take(ctx: &egui::Context) -> Vec<Shortcut> {
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
            let Some(shortcut) = shortcut(*key, *modifiers) else {
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
            let taken = take(ui.ctx());
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
