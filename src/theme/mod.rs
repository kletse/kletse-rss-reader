//! The look of the app comes from the desktop it runs on: the live Omarchy theme on Linux,
//! the system appearance (light or dark) on macOS. Both fill the same `Theme`,
//! which `Palette` turns into the colours the window and the article renderer use.

use crate::prefs::Colours;
use std::{collections::HashMap, sync::Arc};

mod onward;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
use macos as provider;

#[cfg(not(target_os = "macos"))]
mod omarchy;
#[cfg(not(target_os = "macos"))]
use omarchy as provider;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Theme {
    /// Omarchy `colors.toml` keys: `background`, `foreground`, `accent`, `mode`, …
    pub colors: HashMap<String, String>,
    /// Corner radius in px.
    pub rounding: i64,
    /// UI (monospace) font family, if the desktop has a preference.
    pub font: Option<String>,
}

impl Theme {
    /// This theme in the chosen colours. The font and the rounding stay the desktop's, whatever the colours.
    pub fn in_colours(mut self, colours: Colours) -> Self {
        match colours {
            Colours::System => {}
            Colours::OnwardDark => self.colors = onward::colors(true),
            Colours::OnwardLight => self.colors = onward::colors(false),
        }
        self
    }
}

/// Called with the new theme, from a background thread, when it changes.
pub type OnChange = Arc<dyn Fn(Theme) + Send + Sync>;

/// The current theme. Shells out, so call it off the main thread when that matters.
pub fn load() -> Theme {
    provider::load()
}

/// The macOS theme for an appearance: the logo's colours, or with a system accent (`#rrggbb`) the original look. The
/// native app reads both from AppKit, which knows them better than `defaults` does, and is told when they change.
#[cfg(target_os = "macos")]
pub fn macos(dark: bool, system_accent: Option<&str>) -> Theme {
    provider::theme(dark, system_accent)
}

/// Follows theme changes for as long as the returned watcher lives.
pub fn watch(on_change: OnChange) -> Watcher {
    Watcher(provider::watch(on_change))
}

pub struct Watcher(provider::Watcher);

impl std::fmt::Debug for Watcher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Watcher { .. }")
    }
}

impl Watcher {
    /// The window regained focus or the system appearance changed: check again. Only macOS needs this; on
    /// Omarchy a file watcher already hears every switch.
    pub fn recheck(&self) {
        self.0.recheck();
    }
}

/// The colours the UI uses, with the same Tokyo Night fallbacks the app always had for keys a theme leaves out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Palette {
    pub bg: String,
    pub bg_light: String,
    pub bg_dark: String,
    pub fg: String,
    pub fg_dim: String,
    pub fg_bright: String,
    pub accent: String,
    pub muted: String,
    pub selection: String,
    pub red: String,
    pub bright_red: String,
    pub green: String,
    pub yellow: String,
    pub blue: String,
    pub magenta: String,
    pub cyan: String,
    pub orange: String,
    pub ui_font: String,
    pub rounding: i64,
    pub dark: bool,
}

impl Default for Palette {
    fn default() -> Self {
        Self::from(&Theme::default())
    }
}

impl From<&Theme> for Palette {
    fn from(theme: &Theme) -> Self {
        let c = &theme.colors;
        let pick = |keys: &[&str], fallback: &str| {
            keys.iter().find_map(|k| c.get(*k)).cloned().unwrap_or_else(|| fallback.to_string())
        };
        let red = pick(&["red"], "#f7768e");
        Self {
            bg: pick(&["background"], "#1a1b26"),
            bg_light: pick(&["lighter_background"], "#24283b"),
            bg_dark: pick(&["dark_background"], "#16161e"),
            fg: pick(&["foreground"], "#c0caf5"),
            fg_dim: pick(&["dark_foreground"], "#787c99"),
            fg_bright: pick(&["bright_foreground", "light_foreground"], "#ffffff"),
            accent: pick(&["accent"], "#7aa2f7"),
            muted: pick(&["muted"], "#565f89"),
            selection: pick(&["selection"], "#33467c"),
            bright_red: pick(&["bright_red"], &red),
            red,
            green: pick(&["green"], "#9ece6a"),
            yellow: pick(&["yellow"], "#e0af68"),
            blue: pick(&["blue"], "#7aa2f7"),
            magenta: pick(&["magenta"], "#bb9af7"),
            cyan: pick(&["cyan"], "#7dcfff"),
            orange: pick(&["orange"], "#ff9e64"),
            ui_font: theme.font.clone().filter(|f| !f.trim().is_empty()).unwrap_or_else(|| "monospace".into()),
            rounding: theme.rounding.max(0),
            dark: c.get("mode").is_none_or(|m| m != "light"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_keys_fall_back_and_present_ones_win() {
        let mut theme = Theme::default();
        theme.colors.insert("accent".into(), "#123456".into());
        theme.colors.insert("light_foreground".into(), "#eeeeee".into());
        theme.colors.insert("mode".into(), "light".into());
        theme.font = Some("Iosevka".into());
        let p = Palette::from(&theme);
        assert_eq!(p.accent, "#123456");
        assert_eq!(p.fg_bright, "#eeeeee"); // bright_foreground missing: light_foreground stands in
        assert_eq!(p.bg, "#1a1b26");
        assert_eq!(p.bright_red, p.red);
        assert_eq!(p.ui_font, "Iosevka");
        assert!(!p.dark);
    }

    #[test]
    fn no_font_means_the_generic_monospace() {
        assert_eq!(Palette::default().ui_font, "monospace");
        assert!(Palette::default().dark);
    }
}
