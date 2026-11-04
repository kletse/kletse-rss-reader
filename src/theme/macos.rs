//! Follows the macOS appearance, light or dark mode, in the logo's colours. The window can ask for the original look
//! instead: neutral colours and the system accent.

use super::{OnChange, Theme, onward};
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

pub fn load() -> Theme {
    theme(is_dark(), None)
}

/// The theme for an appearance: the logo's colours, or with a system accent (`#rrggbb`) the original look.
pub fn theme(dark: bool, system_accent: Option<&str>) -> Theme {
    Theme {
        colors: palette(dark, system_accent),
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

/// The logo's colours, or the original look over them when there's a system accent.
fn palette(dark: bool, system_accent: Option<&str>) -> HashMap<String, String> {
    let mut colors = onward::colors(dark);
    if let Some(accent) = system_accent {
        let original: &[(&str, &str)] = if dark {
            &[
                ("background", "#1e1e20"),
                ("lighter_background", "#2b2b2f"),
                ("dark_background", "#161618"),
                ("foreground", "#d6d6dc"),
                ("dark_foreground", "#8e8e96"),
                ("muted", "#6b6b73"),
                ("selection", "#3a3a40"),
                ("accent", accent),
            ]
        } else {
            &[
                ("background", "#f7f6f3"),
                ("lighter_background", "#eceae5"),
                ("dark_background", "#e4e1da"),
                ("accent", accent),
            ]
        };
        colors.extend(original.iter().map(|(k, v)| (k.to_string(), v.to_string())));
    }
    colors
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_original_look_has_its_own_colours_and_the_system_accent() {
        let original = palette(true, Some("#0a84ff"));
        assert_eq!(original["background"], "#1e1e20");
        assert_eq!(original["accent"], "#0a84ff");
        assert_eq!(palette(false, Some("#0a84ff"))["background"], "#f7f6f3");
        assert_eq!(palette(true, None)["background"], "#1e130d");
    }
}
