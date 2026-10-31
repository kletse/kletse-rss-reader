//! `~/.config/kletse-rss/config.toml`: Miniflux (required) and Readeck (optional).
//! Written by the onboarding/settings screen; can also be edited by hand.

use serde::{Deserialize, Serialize};
use std::{fs, path::PathBuf};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub miniflux: Miniflux,
    /// `None` when Readeck is not set up: the bookmark button is hidden.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub readeck: Option<Readeck>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Miniflux {
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub api_key: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Readeck {
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub token: String,
}

pub fn path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("~/.config"))
        .join("kletse-rss/config.toml")
}

impl Config {
    /// Trims values, drops trailing slashes, and treats a half-empty Readeck section as "not set up".
    pub fn normalized(mut self) -> Self {
        self.miniflux.url = clean_url(&self.miniflux.url);
        self.miniflux.api_key = self.miniflux.api_key.trim().to_string();
        self.readeck = self.readeck.and_then(|r| {
            let r = Readeck { url: clean_url(&r.url), token: r.token.trim().to_string() };
            (!r.url.is_empty() && !r.token.is_empty()).then_some(r)
        });
        self
    }

    /// True when the required Miniflux settings are present.
    pub fn is_complete(&self) -> bool {
        !self.miniflux.url.is_empty() && !self.miniflux.api_key.is_empty()
    }
}

fn clean_url(url: &str) -> String {
    url.trim().trim_end_matches('/').to_string()
}

/// Reads the config file as far as it goes; missing or unreadable means "empty".
pub fn read() -> Config {
    fs::read_to_string(path())
        .ok()
        .and_then(|text| toml::from_str::<Config>(&text).ok())
        .unwrap_or_default()
        .normalized()
}

/// Writes the config (mode 600: it holds API keys).
pub fn save(config: &Config) -> Result<(), String> {
    let path = path();
    let body = toml::to_string(config).map_err(|e| e.to_string())?;
    let text = format!("# kletse-rss configuration. Also editable from the app: press , or use Settings in the feeds picker.\n\n{body}");
    let write = || -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        fs::write(&path, text)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
        }
        Ok(())
    };
    write().map_err(|e| format!("Could not write {}: {e}", path.display()))
}
