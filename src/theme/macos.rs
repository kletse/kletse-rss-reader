//! Follows the macOS appearance, light or dark mode, with two palettes taken from the logo. The accent is the logo's
//! orange; in light mode a deeper one, since the logo's own is too faint for text on a light background.

use super::{OnChange, Theme};
use std::{
    collections::HashMap,
    process::Command,
    sync::{
        Arc, Mutex, PoisonError,
        atomic::{AtomicBool, Ordering},
    },
    thread,
};

/// Corner radius of macOS windows and controls.
const ROUNDING: i64 = 10;
const ACCENT_LIGHT: &str = "#bf3f0c";
const ACCENT_DARK: &str = "#ff8a3d";

pub fn load() -> Theme {
    theme(is_dark())
}

/// The theme for an appearance.
pub fn theme(dark: bool) -> Theme {
    Theme {
        colors: palette(dark),
        rounding: ROUNDING,
        font: Some("Menlo".into()), // the system monospace font Terminal and Xcode default to
    }
}

/// macOS has no file to watch: the window asks for a check when the system appearance changes and when it
/// regains focus. `on_change` hears only of actual differences.
pub struct Watcher {
    on_change: OnChange,
    checks: Arc<Checks>,
}

pub fn watch(on_change: OnChange) -> Watcher {
    Watcher { on_change, checks: Arc::new(Checks::default()) }
}

impl Watcher {
    pub fn recheck(&self) {
        recheck(&self.on_change, &self.checks);
    }
}

#[derive(Default)]
struct Checks {
    /// The theme last seen; `None` until the first check, which then reports once, harmlessly.
    last: Mutex<Option<Theme>>,
    /// A check is waiting to start. Requests meanwhile join it: it reads the system after they came in.
    queued: AtomicBool,
}

/// Loads the theme on a thread of its own: window events arrive on the main thread, and `load` waits for
/// `defaults` twice. At most one check runs and one waits, however fast the focus events come.
fn recheck(on_change: &OnChange, checks: &Arc<Checks>) {
    if checks.queued.swap(true, Ordering::AcqRel) {
        return;
    }
    let (on_change, checks) = (Arc::clone(on_change), Arc::clone(checks));
    thread::spawn(move || {
        let mut last = checks.last.lock().unwrap_or_else(PoisonError::into_inner);
        // From here on, a new request starts a new check: this one may already have read the system.
        checks.queued.store(false, Ordering::Release);
        let theme = load();
        if last.as_ref() != Some(&theme) {
            *last = Some(theme.clone());
            on_change(theme);
        }
    });
}

/// `defaults read -g <key>`, trimmed; `None` when the key is unset.
fn read_default(key: &str) -> Option<String> {
    let out = Command::new("defaults").args(["read", "-g", key]).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// `AppleInterfaceStyle` is `Dark` in dark mode and unset in light mode.
fn is_dark() -> bool {
    read_default("AppleInterfaceStyle").is_some_and(|s| s == "Dark")
}

/// The same keys an Omarchy `colors.toml` has, so the rest of the app needs no platform branch.
fn palette(dark: bool) -> HashMap<String, String> {
    let pairs: &[(&str, &str)] = if dark {
        &[
            ("mode", "dark"),
            ("background", "#1e130d"),
            ("lighter_background", "#2d1f17"),
            ("dark_background", "#160d09"),
            ("foreground", "#e2dcd3"),
            ("dark_foreground", "#a09488"),
            ("bright_foreground", "#ffffff"),
            ("muted", "#7c7065"),
            ("selection", "#4a3325"),
            ("red", "#ff6b6b"),
            ("yellow", "#f2c94c"),
            ("green", "#7bd88f"),
            ("cyan", "#5ac8fa"),
            ("blue", "#6fa8ff"),
            ("magenta", "#c792ea"),
            ("orange", "#ffa657"),
        ]
    } else {
        &[
            ("mode", "light"),
            ("background", "#f7f4ec"),
            ("lighter_background", "#ece8dd"),
            ("dark_background", "#e4dfd2"),
            ("foreground", "#2f2e2b"),
            ("dark_foreground", "#6f6c66"),
            ("bright_foreground", "#141311"),
            ("muted", "#8f8b83"),
            ("selection", "#d9d5cc"),
            ("red", "#c4432b"),
            ("yellow", "#9a6b00"),
            ("green", "#3b7a2a"),
            ("cyan", "#0b7285"),
            ("blue", "#1c5fb4"),
            ("magenta", "#7b3fa0"),
            ("orange", "#b85c1a"),
        ]
    };
    let mut colors: HashMap<String, String> = pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
    colors.insert("accent".into(), if dark { ACCENT_DARK } else { ACCENT_LIGHT }.into());
    colors
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn palettes_have_every_key_the_app_reads() {
        for dark in [true, false] {
            let p = palette(dark);
            for key in [
                "mode",
                "background",
                "lighter_background",
                "dark_background",
                "foreground",
                "dark_foreground",
                "bright_foreground",
                "accent",
                "muted",
                "selection",
                "red",
                "yellow",
                "green",
                "cyan",
                "blue",
                "magenta",
                "orange",
            ] {
                assert!(p.contains_key(key), "{key} missing from {} palette", if dark { "dark" } else { "light" });
            }
            assert_eq!(p["accent"], if dark { ACCENT_DARK } else { ACCENT_LIGHT });
        }
        assert_eq!(palette(true)["mode"], "dark");
        assert_eq!(palette(false)["mode"], "light");
    }
}
