//! Tabel mapping tombol ke byte yang dikirim ke PTY (mengikuti konvensi xterm).

use eframe::egui::{Key, Modifiers};

const ESC: u8 = 0x1b;
const DEL: u8 = 0x7f;

/// Menerjemahkan tombol. `None` berarti tombol ini tidak mengirim apa pun, atau teksnya
/// sudah dikirim lewat `Event::Text`.
pub fn encode_key(key: Key, mods: Modifiers, app_cursor: bool) -> Option<Vec<u8>> {
    // Di Windows, AltGr dilaporkan sebagai Ctrl+Alt. Karakter hasil AltGr tidak boleh
    // berubah menjadi urutan kontrol.
    if mods.ctrl && mods.alt {
        return None;
    }

    let modifier = modifier_param(mods);
    let bytes = match key {
        Key::ArrowUp => cursor_key(b'A', modifier, app_cursor),
        Key::ArrowDown => cursor_key(b'B', modifier, app_cursor),
        Key::ArrowRight => cursor_key(b'C', modifier, app_cursor),
        Key::ArrowLeft => cursor_key(b'D', modifier, app_cursor),
        Key::Home => cursor_key(b'H', modifier, app_cursor),
        Key::End => cursor_key(b'F', modifier, app_cursor),
        Key::Insert => tilde_key(2, modifier),
        Key::Delete => tilde_key(3, modifier),
        Key::PageUp => tilde_key(5, modifier),
        Key::PageDown => tilde_key(6, modifier),
        Key::F1 => cursor_key(b'P', modifier, true),
        Key::F2 => cursor_key(b'Q', modifier, true),
        Key::F3 => cursor_key(b'R', modifier, true),
        Key::F4 => cursor_key(b'S', modifier, true),
        Key::F5 => tilde_key(15, modifier),
        Key::F6 => tilde_key(17, modifier),
        Key::F7 => tilde_key(18, modifier),
        Key::F8 => tilde_key(19, modifier),
        Key::F9 => tilde_key(20, modifier),
        Key::F10 => tilde_key(21, modifier),
        Key::F11 => tilde_key(23, modifier),
        Key::F12 => tilde_key(24, modifier),
        Key::Tab if mods.shift => vec![ESC, b'[', b'Z'],
        Key::Tab => with_alt(b'\t', mods),
        Key::Enter => with_alt(b'\r', mods),
        Key::Escape => with_alt(ESC, mods),
        Key::Backspace if mods.ctrl => with_alt(0x08, mods),
        Key::Backspace => with_alt(DEL, mods),
        _ if mods.ctrl => with_alt(control_byte(key)?, mods),
        _ if mods.alt => with_alt(alt_char(key, mods.shift)?, mods),
        _ => return None,
    };
    Some(bytes)
}

/// Menyiapkan teks paste. Karakter ESC dibuang supaya isi clipboard tidak bisa keluar dari
/// mode bracketed paste lalu dieksekusi.
pub fn encode_paste(text: &str, bracketed: bool) -> Vec<u8> {
    let cleaned: String = text
        .replace("\r\n", "\r")
        .replace('\n', "\r")
        .replace('\x1b', "");
    if bracketed {
        format!("\x1b[200~{cleaned}\x1b[201~").into_bytes()
    } else {
        cleaned.into_bytes()
    }
}

fn modifier_param(mods: Modifiers) -> u8 {
    1 + u8::from(mods.shift) + u8::from(mods.alt) * 2 + u8::from(mods.ctrl) * 4
}

fn cursor_key(letter: u8, modifier: u8, application: bool) -> Vec<u8> {
    if modifier > 1 {
        format!("\x1b[1;{modifier}{}", char::from(letter)).into_bytes()
    } else if application {
        vec![ESC, b'O', letter]
    } else {
        vec![ESC, b'[', letter]
    }
}

fn tilde_key(code: u8, modifier: u8) -> Vec<u8> {
    if modifier > 1 {
        format!("\x1b[{code};{modifier}~").into_bytes()
    } else {
        format!("\x1b[{code}~").into_bytes()
    }
}

fn with_alt(byte: u8, mods: Modifiers) -> Vec<u8> {
    if mods.alt {
        vec![ESC, byte]
    } else {
        vec![byte]
    }
}

fn control_byte(key: Key) -> Option<u8> {
    match key {
        Key::Space | Key::Num2 => Some(0x00),
        Key::OpenBracket => Some(ESC),
        Key::Backslash => Some(0x1c),
        Key::CloseBracket => Some(0x1d),
        Key::Num6 => Some(0x1e),
        Key::Minus | Key::Slash => Some(0x1f),
        _ => letter(key).map(|letter| letter - b'A' + 1),
    }
}

fn alt_char(key: Key, shift: bool) -> Option<u8> {
    if key == Key::Space {
        return Some(b' ');
    }
    if let Some(letter) = letter(key) {
        return Some(if shift {
            letter
        } else {
            letter.to_ascii_lowercase()
        });
    }
    match key.name().as_bytes() {
        [digit] if digit.is_ascii_digit() => Some(*digit),
        _ => None,
    }
}

/// Huruf kapital A-Z milik tombol huruf.
fn letter(key: Key) -> Option<u8> {
    match key.name().as_bytes() {
        [byte] if byte.is_ascii_uppercase() => Some(*byte),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NONE: Modifiers = Modifiers::NONE;
    const CTRL: Modifiers = Modifiers::CTRL;
    const SHIFT: Modifiers = Modifiers::SHIFT;
    const ALT: Modifiers = Modifiers::ALT;

    fn key(key: Key, mods: Modifiers) -> Option<Vec<u8>> {
        encode_key(key, mods, false)
    }

    #[test]
    fn ctrl_huruf_menjadi_byte_kontrol() {
        assert_eq!(key(Key::C, CTRL), Some(vec![0x03]));
        assert_eq!(key(Key::A, CTRL), Some(vec![0x01]));
        assert_eq!(key(Key::Z, CTRL), Some(vec![0x1a]));
        assert_eq!(key(Key::Space, CTRL), Some(vec![0x00]));
        assert_eq!(key(Key::OpenBracket, CTRL), Some(vec![0x1b]));
    }

    #[test]
    fn panah_mengikuti_mode_aplikasi() {
        assert_eq!(key(Key::ArrowUp, NONE), Some(b"\x1b[A".to_vec()));
        assert_eq!(
            encode_key(Key::ArrowUp, NONE, true),
            Some(b"\x1bOA".to_vec())
        );
        assert_eq!(key(Key::ArrowLeft, CTRL), Some(b"\x1b[1;5D".to_vec()));
        assert_eq!(key(Key::ArrowRight, SHIFT), Some(b"\x1b[1;2C".to_vec()));
        // Dengan modifier, formatnya tetap CSI walau di mode aplikasi.
        assert_eq!(
            encode_key(Key::ArrowUp, CTRL, true),
            Some(b"\x1b[1;5A".to_vec())
        );
    }

    #[test]
    fn tombol_navigasi_dan_fungsi() {
        assert_eq!(key(Key::Delete, NONE), Some(b"\x1b[3~".to_vec()));
        assert_eq!(key(Key::PageUp, NONE), Some(b"\x1b[5~".to_vec()));
        assert_eq!(key(Key::PageDown, CTRL), Some(b"\x1b[6;5~".to_vec()));
        assert_eq!(key(Key::F1, NONE), Some(b"\x1bOP".to_vec()));
        assert_eq!(key(Key::F5, NONE), Some(b"\x1b[15~".to_vec()));
        assert_eq!(key(Key::F12, NONE), Some(b"\x1b[24~".to_vec()));
        assert_eq!(key(Key::F3, SHIFT), Some(b"\x1b[1;2R".to_vec()));
    }

    #[test]
    fn enter_tab_backspace() {
        assert_eq!(key(Key::Enter, NONE), Some(vec![b'\r']));
        assert_eq!(key(Key::Tab, NONE), Some(vec![b'\t']));
        assert_eq!(key(Key::Tab, SHIFT), Some(b"\x1b[Z".to_vec()));
        assert_eq!(key(Key::Backspace, NONE), Some(vec![0x7f]));
        assert_eq!(key(Key::Backspace, CTRL), Some(vec![0x08]));
        assert_eq!(key(Key::Backspace, ALT), Some(vec![0x1b, 0x7f]));
    }

    #[test]
    fn alt_mengawali_dengan_esc() {
        assert_eq!(key(Key::B, ALT), Some(b"\x1bb".to_vec()));
        assert_eq!(key(Key::B, ALT | SHIFT), Some(b"\x1bB".to_vec()));
        assert_eq!(key(Key::Num3, ALT), Some(b"\x1b3".to_vec()));
    }

    #[test]
    fn huruf_biasa_lewat_event_text() {
        assert_eq!(key(Key::A, NONE), None);
        assert_eq!(key(Key::A, SHIFT), None);
        assert_eq!(key(Key::Space, NONE), None);
    }

    #[test]
    fn altgr_tidak_menjadi_urutan_kontrol() {
        assert_eq!(key(Key::Q, CTRL | ALT), None);
    }

    #[test]
    fn paste_mengubah_baris_baru_dan_membuang_esc() {
        assert_eq!(encode_paste("a\r\nb\nc", false), b"a\rb\rc".to_vec());
        assert_eq!(
            encode_paste("x\x1b[201~y", true),
            b"\x1b[200~x[201~y\x1b[201~".to_vec()
        );
    }
}
