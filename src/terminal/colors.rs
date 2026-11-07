//! Color schemes, the palette, and final color resolution for each cell.

use alacritty_terminal::term::cell::{Cell, Flags};
use alacritty_terminal::term::color::{Colors, COUNT};
use alacritty_terminal::vte::ansi::{Color, NamedColor, Rgb};
use serde::{Deserialize, Serialize};

const fn rgb(r: u8, g: u8, b: u8) -> Rgb {
    Rgb { r, g, b }
}

const fn hex(value: u32) -> Rgb {
    rgb((value >> 16) as u8, (value >> 8) as u8, value as u8)
}

const DIM_FACTOR: f32 = 0.66;

pub const SELECTION: Rgb = rgb(0x26, 0x4f, 0x78);

struct SchemeColors {
    ansi: [Rgb; 16],
    foreground: Rgb,
    background: Rgb,
    cursor: Rgb,
}

const CAMPBELL: SchemeColors = SchemeColors {
    ansi: [
        hex(0x0c0c0c),
        hex(0xc50f1f),
        hex(0x13a10e),
        hex(0xc19c00),
        hex(0x0037da),
        hex(0x881798),
        hex(0x3a96dd),
        hex(0xcccccc),
        hex(0x767676),
        hex(0xe74856),
        hex(0x16c60c),
        hex(0xf9f1a5),
        hex(0x3b78ff),
        hex(0xb4009e),
        hex(0x61d6d6),
        hex(0xf2f2f2),
    ],
    foreground: hex(0xcccccc),
    background: hex(0x0c0c0c),
    cursor: hex(0xe6e6e6),
};

const ONE_HALF_DARK: SchemeColors = SchemeColors {
    ansi: [
        hex(0x282c34),
        hex(0xe06c75),
        hex(0x98c379),
        hex(0xe5c07b),
        hex(0x61afef),
        hex(0xc678dd),
        hex(0x56b6c2),
        hex(0xdcdfe4),
        hex(0x5a6374),
        hex(0xe06c75),
        hex(0x98c379),
        hex(0xe5c07b),
        hex(0x61afef),
        hex(0xc678dd),
        hex(0x56b6c2),
        hex(0xdcdfe4),
    ],
    foreground: hex(0xdcdfe4),
    background: hex(0x282c34),
    cursor: hex(0xa3b3cc),
};

const SOLARIZED_DARK: SchemeColors = SchemeColors {
    ansi: [
        hex(0x073642),
        hex(0xdc322f),
        hex(0x859900),
        hex(0xb58900),
        hex(0x268bd2),
        hex(0xd33682),
        hex(0x2aa198),
        hex(0xeee8d5),
        hex(0x002b36),
        hex(0xcb4b16),
        hex(0x586e75),
        hex(0x657b83),
        hex(0x839496),
        hex(0x6c71c4),
        hex(0x93a1a1),
        hex(0xfdf6e3),
    ],
    foreground: hex(0x839496),
    background: hex(0x002b36),
    cursor: hex(0x93a1a1),
};

const DRACULA: SchemeColors = SchemeColors {
    ansi: [
        hex(0x21222c),
        hex(0xff5555),
        hex(0x50fa7b),
        hex(0xf1fa8c),
        hex(0xbd93f9),
        hex(0xff79c6),
        hex(0x8be9fd),
        hex(0xf8f8f2),
        hex(0x6272a4),
        hex(0xff6e6e),
        hex(0x69ff94),
        hex(0xffffa5),
        hex(0xd6acff),
        hex(0xff92df),
        hex(0xa4ffff),
        hex(0xffffff),
    ],
    foreground: hex(0xf8f8f2),
    background: hex(0x282a36),
    cursor: hex(0xf8f8f2),
};

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ColorScheme {
    #[default]
    Campbell,
    OneHalfDark,
    SolarizedDark,
    Dracula,
}

impl ColorScheme {
    pub const ALL: [Self; 4] = [
        Self::Campbell,
        Self::OneHalfDark,
        Self::SolarizedDark,
        Self::Dracula,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Campbell => "Campbell",
            Self::OneHalfDark => "One Half Dark",
            Self::SolarizedDark => "Solarized Dark",
            Self::Dracula => "Dracula",
        }
    }

    fn colors(self) -> &'static SchemeColors {
        match self {
            Self::Campbell => &CAMPBELL,
            Self::OneHalfDark => &ONE_HALF_DARK,
            Self::SolarizedDark => &SOLARIZED_DARK,
            Self::Dracula => &DRACULA,
        }
    }
}

/// Default colors for every terminal color index.
pub struct Palette([Rgb; COUNT]);

impl Palette {
    pub fn new(scheme: ColorScheme) -> Self {
        let scheme = scheme.colors();
        let mut colors = [scheme.background; COUNT];
        colors[..16].copy_from_slice(&scheme.ansi);

        // 6x6x6 color cube.
        const LEVELS: [u8; 6] = [0, 95, 135, 175, 215, 255];
        let cube = LEVELS.iter().flat_map(|&r| {
            LEVELS
                .iter()
                .flat_map(move |&g| LEVELS.iter().map(move |&b| rgb(r, g, b)))
        });
        for (slot, color) in colors[16..232].iter_mut().zip(cube) {
            *slot = color;
        }

        // Grayscale ramp.
        for (slot, level) in colors[232..256].iter_mut().zip((8u8..).step_by(10)) {
            *slot = rgb(level, level, level);
        }

        colors[NamedColor::Foreground as usize] = scheme.foreground;
        colors[NamedColor::Background as usize] = scheme.background;
        colors[NamedColor::Cursor as usize] = scheme.cursor;
        colors[NamedColor::BrightForeground as usize] = scheme.ansi[15];
        colors[NamedColor::DimForeground as usize] = dim(scheme.foreground);
        for (index, ansi) in scheme.ansi[..8].iter().enumerate() {
            colors[NamedColor::DimBlack as usize + index] = dim(*ansi);
        }

        Self(colors)
    }

    /// The color at an index; colors changed by an app (OSC 4/10/11) win.
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

    /// A cell's foreground and background after bold, dim, inverse, and hidden.
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

/// Bold text in the 8 base colors uses its bright pair, like most terminals.
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
    fn palette_of_256_colors_matches_xterm() {
        let palette = Palette::new(ColorScheme::Campbell);
        let overrides = Colors::default();
        assert_eq!(palette.indexed(16, &overrides), rgb(0, 0, 0));
        assert_eq!(palette.indexed(21, &overrides), rgb(0, 0, 255));
        assert_eq!(palette.indexed(231, &overrides), rgb(255, 255, 255));
        assert_eq!(palette.indexed(232, &overrides), rgb(8, 8, 8));
        assert_eq!(palette.indexed(255, &overrides), rgb(238, 238, 238));
    }

    #[test]
    fn app_colors_take_priority() {
        let palette = Palette::new(ColorScheme::Campbell);
        let mut overrides = Colors::default();
        overrides[NamedColor::Background] = Some(rgb(1, 2, 3));
        assert_eq!(
            palette.named(NamedColor::Background, &overrides),
            rgb(1, 2, 3)
        );
    }

    #[test]
    fn cell_attributes() {
        let palette = Palette::new(ColorScheme::Campbell);
        let overrides = Colors::default();
        let red = Color::Named(NamedColor::Red);
        let blue = Color::Spec(rgb(0, 0, 200));

        let (fg, bg) = palette.cell_colors(&cell(red, blue, Flags::empty()), &overrides);
        assert_eq!((fg, bg), (CAMPBELL.ansi[1], rgb(0, 0, 200)));

        let (fg, _) = palette.cell_colors(&cell(red, blue, Flags::BOLD), &overrides);
        assert_eq!(fg, CAMPBELL.ansi[9]);

        let (fg, bg) = palette.cell_colors(&cell(red, blue, Flags::INVERSE), &overrides);
        assert_eq!((fg, bg), (rgb(0, 0, 200), CAMPBELL.ansi[1]));

        let (fg, bg) = palette.cell_colors(&cell(red, blue, Flags::HIDDEN), &overrides);
        assert_eq!(fg, bg);
    }
}
