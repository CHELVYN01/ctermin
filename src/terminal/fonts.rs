//! Terminal font choices.

use std::path::Path;
use std::sync::Arc;

use eframe::egui::{FontData, FontDefinitions, FontFamily};
use serde::{Deserialize, Serialize};

const CUSTOM_FONT_NAME: &str = "ctermin-terminal";

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TerminalFont {
    /// The font bundled with egui. Always available.
    #[default]
    Hack,
    Consolas,
    CascadiaMono,
    CourierNew,
}

impl TerminalFont {
    pub const ALL: [Self; 4] = [
        Self::Hack,
        Self::Consolas,
        Self::CascadiaMono,
        Self::CourierNew,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Hack => "Hack",
            Self::Consolas => "Consolas",
            Self::CascadiaMono => "Cascadia Mono",
            Self::CourierNew => "Courier New",
        }
    }

    fn file_name(self) -> Option<&'static str> {
        match self {
            Self::Hack => None,
            Self::Consolas => Some("consola.ttf"),
            Self::CascadiaMono => Some("CascadiaMono.ttf"),
            Self::CourierNew => Some("cour.ttf"),
        }
    }
}

/// Puts `bytes` first in the monospace family. The bundled fonts stay as fallbacks for glyphs
/// the chosen font does not have.
fn with_primary_font(bytes: Vec<u8>) -> FontDefinitions {
    let mut definitions = FontDefinitions::default();
    definitions.font_data.insert(
        CUSTOM_FONT_NAME.to_owned(),
        Arc::new(FontData::from_owned(bytes)),
    );
    definitions
        .families
        .entry(FontFamily::Monospace)
        .or_default()
        .insert(0, CUSTOM_FONT_NAME.to_owned());
    definitions
}

/// The egui font setup for a choice, or a message if the font cannot be loaded.
pub fn font_definitions(font: TerminalFont) -> Result<FontDefinitions, String> {
    let Some(file) = font.file_name() else {
        return Ok(FontDefinitions::default());
    };
    let root = std::env::var_os("SystemRoot").unwrap_or_default();
    let path = Path::new(&root).join("Fonts").join(file);
    match std::fs::read(path) {
        Ok(bytes) => Ok(with_primary_font(bytes)),
        Err(_) => Err(format!("The font {} is not installed.", font.label())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_font_needs_no_file() {
        assert!(font_definitions(TerminalFont::Hack).is_ok());
    }

    #[test]
    fn chosen_font_goes_first_and_fallbacks_stay() {
        let default_count = FontDefinitions::default().families[&FontFamily::Monospace].len();
        let definitions = with_primary_font(vec![1, 2, 3]);
        let family = &definitions.families[&FontFamily::Monospace];
        assert_eq!(family[0], CUSTOM_FONT_NAME);
        assert_eq!(family.len(), default_count + 1);
    }

    #[test]
    fn every_choice_has_a_name() {
        for font in TerminalFont::ALL {
            assert!(!font.label().is_empty());
        }
    }
}
