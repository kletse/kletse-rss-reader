//! `~/.config/kletse-rss/config.toml`: Miniflux (required) and Readeck (optional).
//! Written by the onboarding/settings screen; can also be edited by hand.

use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub miniflux: Miniflux,
    /// `None` when Readeck is not set up: the bookmark button is hidden.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub readeck: Option<Readeck>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Miniflux {
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub api_key: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Readeck {
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub token: String,
}

/// `$XDG_CONFIG_HOME/kletse-rss/config.toml`, or `None` when there is no home directory at all.
pub fn path() -> Option<PathBuf> {
    dirs::config_dir()
        .or_else(|| dirs::home_dir().map(|home| home.join(".config")))
        .map(|dir| dir.join("kletse-rss").join("config.toml"))
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

    /// Checks that every configured server address is a usable http(s) URL.
    pub fn validate(&self) -> Result<(), String> {
        if !self.is_complete() {
            return Err("The Miniflux URL and API key are required.".into());
        }
        validate_url(&self.miniflux.url)?;
        if let Some(readeck) = &self.readeck {
            validate_url(&readeck.url)?;
        }
        Ok(())
    }
}

fn clean_url(url: &str) -> String {
    url.trim().trim_end_matches('/').to_string()
}

/// A server address must be an absolute http(s) URL with a host, e.g. `https://miniflux.example.com`.
pub fn validate_url(url: &str) -> Result<(), String> {
    let parsed = reqwest::Url::parse(url).map_err(|_| format!("{url} is not a valid URL"))?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err(format!("{url} should start with https:// or http://"));
    }
    if parsed.host_str().is_none_or(str::is_empty) {
        return Err(format!("{url} is missing a server name"));
    }
    Ok(())
}

/// Reads the config file as far as it goes; missing or unreadable means "empty".
pub fn read() -> Config {
    path().map(|p| read_from(&p)).unwrap_or_default()
}

fn read_from(path: &Path) -> Config {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Config::default(),
        Err(e) => {
            eprintln!("kletse-rss: could not read {}: {e}", path.display());
            return Config::default();
        }
    };
    match toml::from_str::<Config>(&text) {
        Ok(config) => config.normalized(),
        Err(e) => {
            eprintln!("kletse-rss: ignoring {}: {e}", path.display());
            Config::default()
        }
    }
}

/// Writes the config (mode 600: it holds API keys).
pub fn save(config: &Config) -> Result<(), String> {
    let path = path().ok_or("Could not find your home directory to save the settings in")?;
    save_to(config, &path)
}

fn save_to(config: &Config, path: &Path) -> Result<(), String> {
    let body = toml::to_string(config).map_err(|e| e.to_string())?;
    let text = format!(
        "# kletse-rss configuration. Also editable from the app: press , or use Settings in the feeds picker.\n\n{body}"
    );
    write_private(path, &text).map_err(|e| format!("Could not write {}: {e}", path.display()))
}

/// Creates the parent directory and writes the file readable by the owner only.
fn write_private(path: &Path, text: &str) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    write_file(path, text)
}

#[cfg(unix)]
fn write_file(path: &Path, text: &str) -> std::io::Result<()> {
    use std::{io::Write, os::unix::fs::OpenOptionsExt};
    // Set the mode at creation so the key is never world-readable, not even briefly.
    let mut file = fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(path)?;
    file.write_all(text.as_bytes())?;
    file.sync_all()?;
    // An existing file keeps its old mode on open; make sure it is 600 as well.
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
}

#[cfg(not(unix))]
fn write_file(path: &Path, text: &str) -> std::io::Result<()> {
    fs::write(path, text)
}

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn full() -> Config {
        Config {
            miniflux: Miniflux { url: "https://m.example.com".into(), api_key: "key".into() },
            readeck: Some(Readeck { url: "https://r.example.com".into(), token: "tok".into() }),
        }
    }

    #[test]
    fn normalized_trims_and_strips_trailing_slashes() {
        let cfg = Config {
            miniflux: Miniflux { url: "  https://m.example.com/// ".into(), api_key: " key\n".into() },
            readeck: Some(Readeck { url: " https://r.example.com/ ".into(), token: "\ttok ".into() }),
        }
        .normalized();
        assert_eq!(cfg, full());
    }

    #[test]
    fn half_empty_readeck_means_not_set_up() {
        for (url, token) in [("", "tok"), ("https://r.example.com", ""), ("  ", " ")] {
            let cfg =
                Config { miniflux: full().miniflux, readeck: Some(Readeck { url: url.into(), token: token.into() }) }
                    .normalized();
            assert_eq!(cfg.readeck, None, "url={url:?} token={token:?}");
        }
    }

    #[test]
    fn is_complete_needs_both_miniflux_fields() {
        assert!(full().is_complete());
        assert!(!Config::default().is_complete());
        let mut cfg = full();
        cfg.miniflux.api_key.clear();
        assert!(!cfg.is_complete());
        let mut cfg = full();
        cfg.miniflux.url.clear();
        assert!(!cfg.is_complete());
    }

    #[test]
    fn validate_accepts_http_and_https_hosts() {
        assert_eq!(full().validate(), Ok(()));
        let mut cfg = full();
        cfg.miniflux.url = "http://localhost:8080".into();
        cfg.readeck = None;
        assert_eq!(cfg.validate(), Ok(()));
    }

    #[test]
    fn validate_rejects_bad_urls() {
        assert!(Config::default().validate().unwrap_err().contains("required"));
        for bad in ["miniflux.example.com", "ftp://x", "https://", "javascript:alert(1)", "not a url"] {
            assert!(validate_url(bad).is_err(), "{bad} should be rejected");
        }
        let mut cfg = full();
        cfg.readeck = Some(Readeck { url: "readeck.example.com".into(), token: "t".into() });
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn toml_round_trip_and_optional_readeck() {
        let text = toml::to_string(&full()).unwrap();
        assert_eq!(toml::from_str::<Config>(&text).unwrap(), full());

        let mut without = full();
        without.readeck = None;
        let text = toml::to_string(&without).unwrap();
        assert!(!text.contains("[readeck]"), "{text}");
        assert_eq!(toml::from_str::<Config>(&text).unwrap(), without);
    }

    #[test]
    fn parses_hand_written_files_leniently() {
        let cfg: Config = toml::from_str("[miniflux]\nurl = \"https://m.example.com/\"\n").unwrap();
        assert!(!cfg.is_complete());
        assert_eq!(cfg.normalized().miniflux.url, "https://m.example.com");
        let cfg: Config = toml::from_str("").unwrap();
        assert_eq!(cfg, Config::default());
    }

    #[test]
    fn read_from_missing_or_broken_file_is_empty() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        assert_eq!(read_from(&path), Config::default());
        fs::write(&path, "this is = not [toml").unwrap();
        assert_eq!(read_from(&path), Config::default());
    }

    #[test]
    fn save_then_read_round_trips_with_private_mode() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join("config.toml");
        save_to(&full(), &path).unwrap();
        assert_eq!(read_from(&path), full());
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.starts_with("# kletse-rss configuration"));
        #[cfg(unix)]
        assert_eq!(fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);

        // Overwriting an existing, more permissive file tightens it again.
        #[cfg(unix)]
        {
            fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
            save_to(&full(), &path).unwrap();
            assert_eq!(fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
        }
    }
}
