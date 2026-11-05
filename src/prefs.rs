//! Article font size, column width, the order of unread items and the colours, remembered between runs. The width is in
//! multiples of the font size.
//! Kept apart from config.toml, which is yours to edit: this file is the app's own and may be rewritten anytime.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

const FONT: Range = Range { initial: 18, min: 12, max: 32, step: 1 };
const WIDTH: Range = Range { initial: 40, min: 20, max: 80, step: 4 };

struct Range {
    initial: i32,
    min: i32,
    max: i32,
    step: i32,
}

/// The window's colours on Linux. `System` is the Omarchy theme, and follows it. On macOS, Swift keeps this choice.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Colours {
    #[default]
    System,
    OnwardDark,
    OnwardLight,
}

impl Colours {
    /// As the windows name it.
    pub fn name(self) -> &'static str {
        match self {
            Colours::System => "system",
            Colours::OnwardDark => "onward-dark",
            Colours::OnwardLight => "onward-light",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        [Colours::System, Colours::OnwardDark, Colours::OnwardLight].into_iter().find(|c| c.name() == name)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Prefs {
    /// Article text size in px.
    pub font_size: i32,
    /// Column width, in multiples of the font size.
    pub width_factor: i32,
    /// Unread items newest first (`s`), instead of oldest first.
    pub newest_first: bool,
    pub colours: Colours,
    /// That a secret went to the file has been said, which is only said once.
    pub told_file: bool,
    /// The same, for what Omarchy's keyring is.
    pub told_omarchy: bool,
}

impl Default for Prefs {
    fn default() -> Self {
        Self {
            font_size: FONT.initial,
            width_factor: WIDTH.initial,
            newest_first: false,
            colours: Colours::System,
            told_file: false,
            told_omarchy: false,
        }
    }
}

impl Prefs {
    fn clamped(self) -> Self {
        Self {
            font_size: self.font_size.clamp(FONT.min, FONT.max),
            width_factor: self.width_factor.clamp(WIDTH.min, WIDTH.max),
            ..self
        }
    }

    pub fn bigger(&mut self) {
        *self = Self { font_size: self.font_size + FONT.step, ..*self }.clamped();
    }

    pub fn smaller(&mut self) {
        *self = Self { font_size: self.font_size - FONT.step, ..*self }.clamped();
    }

    pub fn wider(&mut self) {
        *self = Self { width_factor: self.width_factor + WIDTH.step, ..*self }.clamped();
    }

    pub fn narrower(&mut self) {
        *self = Self { width_factor: self.width_factor - WIDTH.step, ..*self }.clamped();
    }

    /// Ctrl+0 resets the size and width, and nothing else.
    pub fn reset(&mut self) {
        *self = Self { font_size: FONT.initial, width_factor: WIDTH.initial, ..*self };
    }

    pub fn flip_order(&mut self) {
        self.newest_first = !self.newest_first;
    }
}

/// `~/.local/state/onward/prefs.toml` on Linux, `~/Library/Application Support/onward/prefs.toml` on macOS.
fn path() -> Option<PathBuf> {
    dirs::state_dir().or_else(dirs::data_local_dir).map(|dir| crate::moved::dir(&dir).join("prefs.toml"))
}

/// What was saved, or the defaults when nothing (readable) was.
pub fn load() -> Prefs {
    path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|text| toml::from_str::<Prefs>(&text).ok())
        .unwrap_or_default()
        .clamped()
}

/// Saves quietly: prefs that aren't remembered are no reason to bother anyone.
pub fn save(prefs: Prefs) {
    let Some(path) = path() else { return };
    let Ok(text) = toml::to_string(&prefs) else { return };
    if let Some(dir) = path.parent()
        && std::fs::create_dir_all(dir).is_ok()
        && let Err(e) = std::fs::write(&path, text)
    {
        eprintln!("onward: could not save {}: {e}", path.display());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steps_stay_in_range() {
        let mut p = Prefs {
            font_size: 31,
            width_factor: 78,
            newest_first: true,
            colours: Colours::OnwardLight,
            ..Prefs::default()
        };
        p.bigger();
        p.bigger();
        p.wider();
        assert_eq!(
            p,
            Prefs {
                font_size: 32,
                width_factor: 80,
                newest_first: true,
                colours: Colours::OnwardLight,
                ..Prefs::default()
            }
        );
        p.reset();
        p.narrower();
        p.smaller();
        assert_eq!(
            p,
            Prefs {
                font_size: 17,
                width_factor: 36,
                newest_first: true,
                colours: Colours::OnwardLight,
                ..Prefs::default()
            }
        );
    }

    #[test]
    fn a_file_out_of_range_or_partial_is_made_sensible() {
        let p: Prefs = toml::from_str("font_size = 99").unwrap_or_default();
        assert_eq!(p.clamped(), Prefs { font_size: 32, ..Prefs::default() });
    }

    #[test]
    fn colours_are_saved_by_name_and_default_to_the_system() {
        let saved = toml::to_string(&Prefs { colours: Colours::OnwardDark, ..Prefs::default() }).unwrap_or_default();
        assert!(saved.contains(r#"colours = "onward-dark""#), "{saved}");
        let older: Prefs = toml::from_str("font_size = 20").unwrap_or_default();
        assert_eq!(older.colours, Colours::System);
        for colours in [Colours::System, Colours::OnwardDark, Colours::OnwardLight] {
            assert_eq!(Colours::from_name(colours.name()), Some(colours));
        }
    }
}
