//! Follows the live Omarchy theme: colors.toml, Hyprland rounding and the Omarchy font.
//! Emits `theme-changed` whenever the current theme is switched.

use super::{Theme, CHANGED_EVENT};
use std::{collections::HashMap, path::PathBuf, process::Command, sync::mpsc, thread, time::Duration};
use tauri::{AppHandle, Emitter};

/// `~/.local/state/omarchy/current`: a symlink Omarchy repoints on every theme switch.
fn current_dir() -> Option<PathBuf> {
    dirs::state_dir()
        .or_else(|| dirs::home_dir().map(|home| home.join(".local").join("state")))
        .map(|dir| dir.join("omarchy").join("current"))
}

pub fn load() -> Theme {
    let colors = current_dir()
        .and_then(|dir| std::fs::read_to_string(dir.join("theme").join("colors.toml")).ok())
        .map(|text| parse_colors(&text))
        .unwrap_or_default();
    Theme { colors, rounding: rounding(), font: font() }
}

/// Keeps the string values of `colors.toml` (`background = "#1a1b26"`, `mode = "dark"`, …).
fn parse_colors(text: &str) -> HashMap<String, String> {
    text.parse::<toml::Table>()
        .map(|table| table.into_iter().filter_map(|(k, v)| v.as_str().map(|s| (k, s.to_string()))).collect())
        .unwrap_or_default()
}

/// Hyprland's `decoration:rounding`, so the UI corners match the windows.
fn rounding() -> i64 {
    Command::new("hyprctl")
        .args(["getoption", "decoration:rounding", "-j"])
        .output()
        .ok()
        .map(|o| parse_rounding(&o.stdout))
        .unwrap_or(0)
}

fn parse_rounding(json: &[u8]) -> i64 {
    serde_json::from_slice::<serde_json::Value>(json)
        .ok()
        .and_then(|v| v.get("int").and_then(serde_json::Value::as_i64))
        .unwrap_or(0)
}

/// The font Omarchy is set to (`omarchy font current`), if any.
fn font() -> Option<String> {
    let out = Command::new("omarchy").args(["font", "current"]).output().ok()?;
    let name = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (!name.is_empty()).then_some(name)
}

/// Watches the Omarchy state dir and emits `theme-changed` (debounced) on any change.
pub fn watch(app: AppHandle) {
    use notify::{RecursiveMode, Watcher};
    // Theme switches replace the `current` symlink/dir, so watch its parent.
    let Some(parent) = current_dir().and_then(|dir| dir.parent().map(PathBuf::from)) else { return };
    thread::spawn(move || {
        let (tx, rx) = mpsc::channel();
        let mut watcher = match notify::recommended_watcher(tx) {
            Ok(watcher) => watcher,
            Err(e) => {
                eprintln!("kletse-rss: not following theme changes: {e}");
                return;
            }
        };
        if let Err(e) = watcher.watch(&parent, RecursiveMode::Recursive) {
            eprintln!("kletse-rss: not following theme changes in {}: {e}", parent.display());
            return;
        }
        while rx.recv().is_ok() {
            // Debounce: a switch touches many files in a burst.
            while rx.recv_timeout(Duration::from_millis(300)).is_ok() {}
            let _ = app.emit(CHANGED_EVENT, load());
        }
    });
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn parse_colors_keeps_only_strings() {
        let colors = parse_colors("background = \"#1a1b26\"\nmode = \"dark\"\nrounding = 8\n[nested]\nx = \"y\"\n");
        assert_eq!(colors.get("background").map(String::as_str), Some("#1a1b26"));
        assert_eq!(colors.get("mode").map(String::as_str), Some("dark"));
        assert_eq!(colors.get("rounding"), None);
        assert_eq!(colors.get("nested"), None);
    }

    #[test]
    fn parse_colors_tolerates_garbage() {
        assert!(parse_colors("not = [toml").is_empty());
        assert!(parse_colors("").is_empty());
    }

    #[test]
    fn parse_rounding_reads_hyprctl_json() {
        assert_eq!(parse_rounding(br#"{"option":"decoration:rounding","int":8,"set":true}"#), 8);
        assert_eq!(parse_rounding(br#"{"option":"decoration:rounding"}"#), 0);
        assert_eq!(parse_rounding(b"no such option"), 0);
        assert_eq!(parse_rounding(b""), 0);
    }
}
