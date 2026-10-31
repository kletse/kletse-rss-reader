//! Follows the macOS appearance: light or dark mode and the accent colour from System Settings.
//! Two hand-picked palettes carry the rest; the accent is the system one.

use super::{Theme, CHANGED_EVENT};
use std::{collections::HashMap, process::Command, sync::Mutex, thread};
use tauri::{AppHandle, Emitter, Manager, WindowEvent};

/// Corner radius of macOS windows and controls.
const ROUNDING: i64 = 10;

pub fn load() -> Theme {
    let dark = is_dark();
    Theme {
        colors: palette(dark, accent(read_default("AppleAccentColor"), dark)),
        rounding: ROUNDING,
        font: Some("ui-monospace".into()),
    }
}

/// Re-reads the theme when the system appearance changes and when the window regains focus
/// (the accent colour has no change event), emitting only when something actually differs.
pub fn watch(app: AppHandle) {
    let Some(window) = app.get_webview_window("main") else { return };
    let last = Mutex::new(load());
    let handle = app.clone();
    window.on_window_event(move |event| {
        if !matches!(event, WindowEvent::ThemeChanged(_) | WindowEvent::Focused(true)) {
            return;
        }
        let theme = load();
        let mut last = last.lock().unwrap_or_else(|e| e.into_inner());
        if *last != theme {
            *last = theme.clone();
            let handle = handle.clone();
            thread::spawn(move || handle.emit(CHANGED_EVENT, theme));
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

/// The system accent colour as macOS draws it, from the `AppleAccentColor` index.
/// Unset means the default blue (also what "multicolour" reports).
fn accent(index: Option<String>, dark: bool) -> &'static str {
    match (index.as_deref().and_then(|s| s.parse::<i8>().ok()), dark) {
        (Some(-1), false) => "#8e8e93", // graphite
        (Some(-1), true) => "#98989d",
        (Some(0), false) => "#ff3b30", // red
        (Some(0), true) => "#ff453a",
        (Some(1), false) => "#ff9500", // orange
        (Some(1), true) => "#ff9f0a",
        (Some(2), false) => "#e6b800", // yellow, darkened a touch for contrast on light
        (Some(2), true) => "#ffd60a",
        (Some(3), false) => "#34c759", // green
        (Some(3), true) => "#30d158",
        (Some(5), false) => "#af52de", // purple
        (Some(5), true) => "#bf5af2",
        (Some(6), false) => "#ff2d55", // pink
        (Some(6), true) => "#ff375f",
        (_, false) => "#007aff", // blue
        (_, true) => "#0a84ff",
    }
}

/// The same keys an Omarchy `colors.toml` has, so the webview needs no platform branch.
fn palette(dark: bool, accent: &str) -> HashMap<String, String> {
    let pairs: &[(&str, &str)] = if dark {
        &[
            ("mode", "dark"),
            ("background", "#1e1e20"),
            ("lighter_background", "#2b2b2f"),
            ("dark_background", "#161618"),
            ("foreground", "#d6d6dc"),
            ("dark_foreground", "#8e8e96"),
            ("bright_foreground", "#ffffff"),
            ("muted", "#6b6b73"),
            ("selection", "#3a3a40"),
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
            ("background", "#f7f6f3"),
            ("lighter_background", "#eceae5"),
            ("dark_background", "#e4e1da"),
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
    colors.insert("accent".into(), accent.into());
    colors
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn accent_index_maps_to_system_colours() {
        assert_eq!(accent(None, false), "#007aff");
        assert_eq!(accent(Some("4".into()), true), "#0a84ff");
        assert_eq!(accent(Some("-1".into()), false), "#8e8e93");
        assert_eq!(accent(Some("6".into()), true), "#ff375f");
        assert_eq!(accent(Some("garbage".into()), true), "#0a84ff");
    }

    #[test]
    fn palettes_have_every_key_the_webview_reads() {
        for dark in [true, false] {
            let p = palette(dark, "#123456");
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
            assert_eq!(p["accent"], "#123456");
        }
        assert_eq!(palette(true, "#000")["mode"], "dark");
        assert_eq!(palette(false, "#000")["mode"], "light");
    }
}
