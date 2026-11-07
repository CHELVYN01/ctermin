//! Shortcut level aplikasi. Tombolnya diambil dari antrean event supaya tidak sampai ke terminal.

use eframe::egui::{self, Event, Key, Modifiers};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Shortcut {
    Open,
    Close,
    Next,
    Previous,
}

fn shortcut(key: Key, modifiers: Modifiers) -> Option<Shortcut> {
    let ctrl_shift = Modifiers::CTRL | Modifiers::SHIFT;
    match key {
        Key::T if modifiers.matches_exact(Modifiers::CTRL) => Some(Shortcut::Open),
        Key::W if modifiers.matches_exact(Modifiers::CTRL) => Some(Shortcut::Close),
        Key::Tab if modifiers.matches_exact(Modifiers::CTRL) => Some(Shortcut::Next),
        Key::Tab if modifiers.matches_exact(ctrl_shift) => Some(Shortcut::Previous),
        _ => None,
    }
}

/// Auto-repeat hanya berlaku untuk pindah tab. Menahan Ctrl+W tidak boleh menutup semua tab.
fn repeats(shortcut: Shortcut) -> bool {
    matches!(shortcut, Shortcut::Next | Shortcut::Previous)
}

/// Mengambil semua shortcut pada frame ini.
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

    /// Mengembalikan shortcut yang terambil dan jumlah event yang tersisa untuk terminal.
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
    fn pemetaan_tombol() {
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
    fn kombinasi_lain_tetap_milik_terminal() {
        assert_eq!(shortcut(Key::T, Modifiers::NONE), None);
        assert_eq!(shortcut(Key::Tab, Modifiers::NONE), None);
        assert_eq!(shortcut(Key::Tab, Modifiers::SHIFT), None);
        assert_eq!(shortcut(Key::W, Modifiers::CTRL | Modifiers::SHIFT), None);
        assert_eq!(shortcut(Key::C, Modifiers::CTRL), None);
    }

    #[test]
    fn shortcut_tidak_sampai_ke_terminal() {
        let (taken, left) = run(vec![
            key_event(Key::T, Modifiers::CTRL, false),
            key_event(Key::A, Modifiers::NONE, false),
        ]);
        assert_eq!(taken, vec![Shortcut::Open]);
        assert_eq!(left, 1);
    }

    #[test]
    fn menahan_ctrl_w_hanya_menutup_sekali() {
        let (taken, left) = run(vec![
            key_event(Key::W, Modifiers::CTRL, false),
            key_event(Key::W, Modifiers::CTRL, true),
            key_event(Key::W, Modifiers::CTRL, true),
        ]);
        assert_eq!(taken, vec![Shortcut::Close]);
        assert_eq!(left, 0);
    }

    #[test]
    fn menahan_ctrl_tab_terus_berpindah() {
        let (taken, _) = run(vec![
            key_event(Key::Tab, Modifiers::CTRL, false),
            key_event(Key::Tab, Modifiers::CTRL, true),
        ]);
        assert_eq!(taken, vec![Shortcut::Next, Shortcut::Next]);
    }
}
