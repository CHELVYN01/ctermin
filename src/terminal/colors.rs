//! Palet warna dan penentuan warna akhir tiap sel.

use alacritty_terminal::term::cell::{Cell, Flags};
use alacritty_terminal::term::color::{Colors, COUNT};
use alacritty_terminal::vte::ansi::{Color, NamedColor, Rgb};

const fn rgb(r: u8, g: u8, b: u8) -> Rgb {
    Rgb { r, g, b }
}

const ANSI: [Rgb; 16] = [
    rgb(0x0c, 0x0c, 0x0c),
    rgb(0xc5, 0x0f, 0x1f),
    rgb(0x13, 0xa1, 0x0e),
    rgb(0xc1, 0x9c, 0x00),
    rgb(0x00, 0x37, 0xda),
    rgb(0x88, 0x17, 0x98),
    rgb(0x3a, 0x96, 0xdd),
    rgb(0xcc, 0xcc, 0xcc),
    rgb(0x76, 0x76, 0x76),
    rgb(0xe7, 0x48, 0x56),
    rgb(0x16, 0xc6, 0x0c),
    rgb(0xf9, 0xf1, 0xa5),
    rgb(0x3b, 0x78, 0xff),
    rgb(0xb4, 0x00, 0x9e),
    rgb(0x61, 0xd6, 0xd6),
    rgb(0xf2, 0xf2, 0xf2),
];
const FOREGROUND: Rgb = rgb(0xcc, 0xcc, 0xcc);
const BACKGROUND: Rgb = rgb(0x0c, 0x0c, 0x0c);
const CURSOR: Rgb = rgb(0xe6, 0xe6, 0xe6);
const BRIGHT_FOREGROUND: Rgb = rgb(0xf2, 0xf2, 0xf2);
const DIM_FACTOR: f32 = 0.66;

pub const SELECTION: Rgb = rgb(0x26, 0x4f, 0x78);

/// Warna bawaan untuk seluruh indeks warna terminal.
pub struct Palette([Rgb; COUNT]);

impl Palette {
    pub fn new() -> Self {
        let mut colors = [BACKGROUND; COUNT];
        colors[..16].copy_from_slice(&ANSI);

        // Kubus warna 6x6x6.
        const LEVELS: [u8; 6] = [0, 95, 135, 175, 215, 255];
        let cube = LEVELS.iter().flat_map(|&r| {
            LEVELS
                .iter()
                .flat_map(move |&g| LEVELS.iter().map(move |&b| rgb(r, g, b)))
        });
        for (slot, color) in colors[16..232].iter_mut().zip(cube) {
            *slot = color;
        }

        // Gradasi abu-abu.
        for (slot, level) in colors[232..256].iter_mut().zip((8u8..).step_by(10)) {
            *slot = rgb(level, level, level);
        }

        colors[NamedColor::Foreground as usize] = FOREGROUND;
        colors[NamedColor::Background as usize] = BACKGROUND;
        colors[NamedColor::Cursor as usize] = CURSOR;
        colors[NamedColor::BrightForeground as usize] = BRIGHT_FOREGROUND;
        colors[NamedColor::DimForeground as usize] = dim(FOREGROUND);
        for (index, ansi) in ANSI[..8].iter().enumerate() {
            colors[NamedColor::DimBlack as usize + index] = dim(*ansi);
        }

        Self(colors)
    }

    /// Warna pada indeks tertentu; warna yang diubah aplikasi (OSC 4/10/11) didahulukan.
    pub fn indexed(&self, index: usize, overrides: &Colors) -> Rgb {
        overrides[index].unwrap_or(self.0[index])
    }

    pub fn named(&self, color: NamedColor, overrides: &Colors) -> Rgb {
        self.indexed(color as usize, overrides)
    }

    fn resolve(&self, color: Color, overrides: &Colors) -> Rgb {
        match color {
            Color::Named(named) => self.named(named, overrides),
            Color::Indexed(index) => self.indexed(usize::from(index), overrides),
            Color::Spec(rgb) => rgb,
        }
    }

    /// Warna depan dan latar sebuah sel setelah atribut bold, dim, inverse, dan hidden.
    pub fn cell_colors(&self, cell: &Cell, overrides: &Colors) -> (Rgb, Rgb) {
        let source = if cell.flags.contains(Flags::BOLD) {
            brighten(cell.fg)
        } else {
            cell.fg
        };
        let mut foreground = self.resolve(source, overrides);
        if cell.flags.contains(Flags::DIM) {
            foreground = dim(foreground);
        }

        let mut background = self.resolve(cell.bg, overrides);
        if cell.flags.contains(Flags::INVERSE) {
            std::mem::swap(&mut foreground, &mut background);
        }
        if cell.flags.contains(Flags::HIDDEN) {
            foreground = background;
        }
        (foreground, background)
    }
}

/// Teks bold pada 8 warna dasar memakai pasangan terangnya, seperti terminal pada umumnya.
fn brighten(color: Color) -> Color {
    match color {
        Color::Named(named) if (named as usize) < 8 || named == NamedColor::Foreground => {
            Color::Named(named.to_bright())
        }
        Color::Indexed(index) if index < 8 => Color::Indexed(index + 8),
        other => other,
    }
}

fn dim(color: Rgb) -> Rgb {
    let scale = |channel: u8| (f32::from(channel) * DIM_FACTOR) as u8;
    rgb(scale(color.r), scale(color.g), scale(color.b))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cell(fg: Color, bg: Color, flags: Flags) -> Cell {
        Cell {
            fg,
            bg,
            flags,
            ..Cell::default()
        }
    }

    #[test]
    fn palet_256_warna_sesuai_xterm() {
        let palette = Palette::new();
        let overrides = Colors::default();
        assert_eq!(palette.indexed(16, &overrides), rgb(0, 0, 0));
        assert_eq!(palette.indexed(21, &overrides), rgb(0, 0, 255));
        assert_eq!(palette.indexed(231, &overrides), rgb(255, 255, 255));
        assert_eq!(palette.indexed(232, &overrides), rgb(8, 8, 8));
        assert_eq!(palette.indexed(255, &overrides), rgb(238, 238, 238));
    }

    #[test]
    fn warna_dari_aplikasi_didahulukan() {
        let palette = Palette::new();
        let mut overrides = Colors::default();
        overrides[NamedColor::Background] = Some(rgb(1, 2, 3));
        assert_eq!(
            palette.named(NamedColor::Background, &overrides),
            rgb(1, 2, 3)
        );
    }

    #[test]
    fn atribut_sel() {
        let palette = Palette::new();
        let overrides = Colors::default();
        let red = Color::Named(NamedColor::Red);
        let blue = Color::Spec(rgb(0, 0, 200));

        let (fg, bg) = palette.cell_colors(&cell(red, blue, Flags::empty()), &overrides);
        assert_eq!((fg, bg), (ANSI[1], rgb(0, 0, 200)));

        let (fg, _) = palette.cell_colors(&cell(red, blue, Flags::BOLD), &overrides);
        assert_eq!(fg, ANSI[9]);

        let (fg, bg) = palette.cell_colors(&cell(red, blue, Flags::INVERSE), &overrides);
        assert_eq!((fg, bg), (rgb(0, 0, 200), ANSI[1]));

        let (fg, bg) = palette.cell_colors(&cell(red, blue, Flags::HIDDEN), &overrides);
        assert_eq!(fg, bg);
    }
}
