//! Reads `~/.config/kletse-rss/config.toml`, creating a template on first run.

use serde::Deserialize;
use std::{fs, path::PathBuf};

const TEMPLATE: &str = r#"# kletse-rss configuration

[miniflux]
url = "https://miniflux.example.com"
# Miniflux → Settings → API Keys → Create a new API key
api_key = ""

[readeck]
url = "https://readeck.example.com"
# Readeck → Profile → API Tokens → Create a new token (needs bookmarks write)
token = ""
"#;

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub miniflux: Miniflux,
    pub readeck: Readeck,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Miniflux {
    pub url: String,
    pub api_key: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Readeck {
    pub url: String,
    pub token: String,
}

pub fn path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("~/.config"))
        .join("kletse-rss/config.toml")
}

/// Loads the config. Writes the template (mode 600) if the file does not exist yet.
pub fn load() -> Result<Config, String> {
    let path = path();
    if !path.exists() {
        write_template(&path).map_err(|e| format!("Could not create {}: {e}", path.display()))?;
    }
    let text = fs::read_to_string(&path).map_err(|e| format!("Could not read {}: {e}", path.display()))?;
    let mut config: Config =
        toml::from_str(&text).map_err(|e| format!("Invalid {}: {e}", path.display()))?;
    config.miniflux.url = config.miniflux.url.trim_end_matches('/').to_string();
    config.readeck.url = config.readeck.url.trim_end_matches('/').to_string();
    if config.miniflux.api_key.trim().is_empty() {
        return Err(format!("Add your Miniflux API key to {}", path.display()));
    }
    Ok(config)
}

fn write_template(path: &PathBuf) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    fs::write(path, TEMPLATE)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}
