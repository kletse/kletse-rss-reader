//! The look of the app comes from the desktop it runs on: the live Omarchy theme on Linux,
//! the system appearance (light/dark + accent colour) on macOS. Both fill the same `Theme`,
//! which `src/theme.ts` maps onto CSS variables.

use serde::Serialize;
use std::collections::HashMap;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
use macos as provider;

#[cfg(not(target_os = "macos"))]
mod omarchy;
#[cfg(not(target_os = "macos"))]
use omarchy as provider;

#[derive(Debug, Serialize, Clone, Default, PartialEq, Eq)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct Theme {
    /// Omarchy `colors.toml` keys: `background`, `foreground`, `accent`, `mode`, …
    pub colors: HashMap<String, String>,
    /// Corner radius in px.
    pub rounding: i64,
    /// UI (monospace) font family, if the desktop has a preference. Left out of the JSON when there's none.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub font: Option<String>,
}

/// Name of the event sent to the webview when the theme changes.
pub const CHANGED_EVENT: &str = "theme-changed";

/// The current theme. Shells out, so call it off the main thread.
pub fn load() -> Theme {
    provider::load()
}

/// Starts following theme changes; emits `theme-changed` with the new `Theme`.
pub fn watch(app: tauri::AppHandle) {
    provider::watch(app)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn missing_font_is_left_out_of_the_json() {
        let theme = Theme { colors: HashMap::new(), rounding: 0, font: None };
        assert_eq!(serde_json::to_string(&theme).unwrap(), r#"{"colors":{},"rounding":0}"#);
    }
}
