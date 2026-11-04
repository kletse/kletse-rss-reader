//! The colours of the logo, as the same keys an Omarchy `colors.toml` has, so the rest of the app needs no platform
//! branch. The default on macOS and a choice in Settings on Linux. The accent is the logo's orange; in light mode a deeper
//! one, since the logo's own is too faint for text on a light background.

use std::collections::HashMap;

const ACCENT_LIGHT: &str = "#bf3f0c";
const ACCENT_DARK: &str = "#ff8a3d";

pub fn colors(dark: bool) -> HashMap<String, String> {
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
            ("accent", ACCENT_DARK),
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
            ("accent", ACCENT_LIGHT),
        ]
    };
    pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_palettes_have_every_key_the_app_reads() {
        for dark in [true, false] {
            let p = colors(dark);
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
        }
    }
}
