//! Follows the live Omarchy theme: colors.toml, Hyprland rounding and the Omarchy font.
//! Emits `theme-changed` whenever the current theme is switched.

use serde::Serialize;
use std::{collections::HashMap, path::PathBuf, process::Command, sync::mpsc, thread, time::Duration};
use tauri::{AppHandle, Emitter};

#[derive(Debug, Serialize, Clone)]
pub struct Theme {
    pub colors: HashMap<String, String>,
    pub rounding: i64,
    pub font: Option<String>,
}

fn current_dir() -> PathBuf {
    dirs::state_dir()
        .unwrap_or_else(|| PathBuf::from("~/.local/state"))
        .join("omarchy/current")
}

pub fn load() -> Theme {
    let colors = std::fs::read_to_string(current_dir().join("theme/colors.toml"))
        .ok()
        .and_then(|text| text.parse::<toml::Table>().ok())
        .map(|table| {
            table
                .into_iter()
                .filter_map(|(k, v)| v.as_str().map(|s| (k, s.to_string())))
                .collect()
        })
        .unwrap_or_default();
    Theme { colors, rounding: rounding(), font: font() }
}

fn rounding() -> i64 {
    Command::new("hyprctl")
        .args(["getoption", "decoration:rounding", "-j"])
        .output()
        .ok()
        .and_then(|o| serde_json::from_slice::<serde_json::Value>(&o.stdout).ok())
        .and_then(|v| v.get("int").and_then(|i| i.as_i64()))
        .unwrap_or(0)
}

fn font() -> Option<String> {
    let out = Command::new("omarchy").args(["font", "current"]).output().ok()?;
    let name = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (!name.is_empty()).then_some(name)
}

/// Watches the Omarchy state dir and emits `theme-changed` (debounced) on any change.
pub fn watch(app: AppHandle) {
    use notify::{RecursiveMode, Watcher};
    thread::spawn(move || {
        let (tx, rx) = mpsc::channel();
        let Ok(mut watcher) = notify::recommended_watcher(tx) else { return };
        // Theme switches replace the `current` symlink/dir, so watch its parent.
        let Some(parent) = current_dir().parent().map(PathBuf::from) else { return };
        if watcher.watch(&parent, RecursiveMode::Recursive).is_err() {
            return;
        }
        while rx.recv().is_ok() {
            // Debounce: a switch touches many files in a burst.
            while rx.recv_timeout(Duration::from_millis(300)).is_ok() {}
            let _ = app.emit("theme-changed", load());
        }
    });
}
