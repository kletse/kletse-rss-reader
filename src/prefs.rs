//! Article font size and column width, remembered between runs. The width is in multiples of the font size.
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Prefs {
    /// Article text size in px.
    pub font_size: i32,
    /// Column width, in multiples of the font size.
    pub width_factor: i32,
}

impl Default for Prefs {
    fn default() -> Self {
        Self { font_size: FONT.initial, width_factor: WIDTH.initial }
    }
}

impl Prefs {
    fn clamped(self) -> Self {
        Self {
            font_size: self.font_size.clamp(FONT.min, FONT.max),
            width_factor: self.width_factor.clamp(WIDTH.min, WIDTH.max),
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

    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

/// `~/.local/state/kletse-rss/prefs.toml` on Linux, `~/Library/Application Support/kletse-rss/prefs.toml` on macOS.
fn path() -> Option<PathBuf> {
    dirs::state_dir().or_else(dirs::data_local_dir).map(|dir| dir.join("kletse-rss").join("prefs.toml"))
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
        eprintln!("kletse-rss: could not save {}: {e}", path.display());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steps_stay_in_range() {
        let mut p = Prefs { font_size: 31, width_factor: 78 };
        p.bigger();
        p.bigger();
        p.wider();
        assert_eq!(p, Prefs { font_size: 32, width_factor: 80 });
        p.reset();
        p.narrower();
        p.smaller();
        assert_eq!(p, Prefs { font_size: 17, width_factor: 36 });
    }

    #[test]
    fn a_file_out_of_range_or_partial_is_made_sensible() {
        let p: Prefs = toml::from_str("font_size = 99").unwrap_or_default();
        assert_eq!(p.clamped(), Prefs { font_size: 32, width_factor: 40 });
    }
}
