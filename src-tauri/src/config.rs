//! `~/.config/kletse-rss/config.toml`: Miniflux (required) and Readeck (optional).
//! Written by the onboarding/settings screen; can also be edited by hand.

use serde::{Deserialize, Serialize};
use std::{
    fmt, fs,
    path::{Path, PathBuf},
};

use crate::error::Error;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub miniflux: Miniflux,
    /// `None` when Readeck is not set up: the bookmark button is hidden.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub readeck: Option<Readeck>,
}

/// `Debug` leaves out the API key.
#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Miniflux {
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub api_key: String,
}

/// `Debug` leaves out the token.
#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Readeck {
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub token: String,
}

impl fmt::Debug for Miniflux {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Miniflux").field("url", &self.url).field("api_key", &Redacted(&self.api_key)).finish()
    }
}

impl fmt::Debug for Readeck {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Readeck").field("url", &self.url).field("token", &Redacted(&self.token)).finish()
    }
}

/// Stands in for a secret in `Debug` output: tells whether it is set, never what it is.
pub struct Redacted<'a>(pub &'a str);

impl fmt::Debug for Redacted<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(if self.0.is_empty() { "\"\"" } else { "<redacted>" })
    }
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
    pub fn validate(&self) -> Result<(), Error> {
        if !self.is_complete() {
            return Err(Error::Setup("The Miniflux URL and API key are required.".into()));
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
pub fn validate_url(url: &str) -> Result<(), Error> {
    let parsed = reqwest::Url::parse(url).map_err(|_| Error::Setup(format!("{url} is not a valid URL")))?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err(Error::Setup(format!("{url} should start with https:// or http://")));
    }
    if parsed.host_str().is_none_or(str::is_empty) {
        return Err(Error::Setup(format!("{url} is missing a server name")));
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
pub fn save(config: &Config) -> Result<(), Error> {
    let path =
        path().ok_or_else(|| Error::Setup("Could not find your home directory to save the settings in".into()))?;
    save_to(config, &path)
}

fn save_to(config: &Config, path: &Path) -> Result<(), Error> {
    let body = toml::to_string(config)
        .map_err(|e| Error::Io { context: "Could not write the settings".into(), source: std::io::Error::other(e) })?;
    let text = format!(
        "# kletse-rss configuration. Also editable from the app: press , or use Settings in the feeds picker.\n\n{body}"
    );
    write_private(path, &text)
        .map_err(|source| Error::Io { context: format!("Could not write {}", path.display()), source })
}

/// Writes the file readable by the owner only, creating its directory. The text goes to a new temporary file
/// next to it (random name, created exclusively, mode 600), which then replaces the old file in one step: a crash
/// or a full disk leaves the old config, never half a new one, which `read` would take for no config at all.
///
/// The price of replacing rather than overwriting: the directory must be writable, not just the file, and the
/// new file is a new inode (hard links to the old one keep the old contents).
fn write_private(path: &Path, text: &str) -> std::io::Result<()> {
    use std::io::Write;
    let path = resolve_symlinks(path);
    let dir = path.parent().unwrap_or(Path::new(""));
    fs::create_dir_all(dir)?;
    let mut prefix = std::ffi::OsString::from(".");
    prefix.push(path.file_name().unwrap_or_default());
    prefix.push(".");
    let mut tmp = tempfile::Builder::new().prefix(&prefix).suffix(".tmp").tempfile_in(dir)?;
    tmp.write_all(text.as_bytes())?;
    tmp.as_file().sync_all()?;
    tmp.persist(&path).map_err(|e| e.error)?; // on failure, dropping the temporary file deletes it
    sync_dir(dir);
    Ok(())
}

/// Where a symlinked config (a dotfiles repo, say) really lives, so a save replaces the target and the link
/// stays. Unlike `canonicalize` this also follows a link whose target doesn't exist yet.
fn resolve_symlinks(path: &Path) -> PathBuf {
    let mut path = path.to_path_buf();
    // The same limit Linux puts on a chain of links; a loop then fails at the write, as it should.
    for _ in 0..40 {
        let Ok(target) = fs::read_link(&path) else { break };
        // A relative target is relative to the link's directory; `join` keeps an absolute one as is.
        path = path.parent().map_or_else(|| target.clone(), |dir| dir.join(&target));
    }
    path
}

/// Makes the rename itself survive a power cut. Best effort: by now the new config is in place either way.
fn sync_dir(dir: &Path) {
    #[cfg(unix)]
    if let Ok(dir) = fs::File::open(dir) {
        let _ = dir.sync_all();
    }
    #[cfg(not(unix))]
    let _ = dir;
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;

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
        full().validate().unwrap();
        let mut cfg = full();
        cfg.miniflux.url = "http://localhost:8080".into();
        cfg.readeck = None;
        cfg.validate().unwrap();
    }

    #[test]
    fn validate_rejects_bad_urls() {
        assert!(Config::default().validate().unwrap_err().message().contains("required"));
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

        // Nothing is left behind next to it.
        let names: Vec<_> = fs::read_dir(path.parent().unwrap()).unwrap().map(|e| e.unwrap().file_name()).collect();
        assert_eq!(names, ["config.toml"]);
    }

    #[test]
    fn a_left_over_file_with_the_old_temporary_name_is_left_alone() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        // What an earlier version left after a crash; a directory, so that writing into it would fail.
        fs::create_dir(dir.path().join(".config.toml.tmp")).unwrap();
        save_to(&full(), &path).unwrap();
        assert_eq!(read_from(&path), full());
    }

    #[cfg(unix)]
    #[test]
    fn a_failed_save_keeps_the_old_config_and_leaves_nothing_behind() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        save_to(&full(), &path).unwrap();
        let mut changed = full();
        changed.miniflux.url = "https://other.example.com".into();
        // A read-only directory: no temporary file can be created, so the save fails before touching the config.
        fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o500)).unwrap();
        let saved = save_to(&changed, &path);
        fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o700)).unwrap();
        let Err(err) = saved else { return }; // running as root, which ignores the mode
        assert!(err.message().starts_with(&format!("Could not write {}: ", path.display())), "{}", err.message());
        assert_eq!(read_from(&path), full());
        let names: Vec<_> = fs::read_dir(dir.path()).unwrap().map(|e| e.unwrap().file_name()).collect();
        assert_eq!(names, ["config.toml"]);
    }

    #[cfg(unix)]
    #[test]
    fn saving_through_a_symlink_keeps_the_link() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("dotfiles").join("kletse-rss.toml");
        fs::create_dir(target.parent().unwrap()).unwrap();
        fs::write(&target, "").unwrap();
        let link = dir.path().join("config.toml");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        save_to(&full(), &link).unwrap();
        assert!(fs::symlink_metadata(&link).unwrap().file_type().is_symlink());
        assert_eq!(read_from(&target), full());
        assert_eq!(fs::metadata(&target).unwrap().permissions().mode() & 0o777, 0o600);
    }

    #[cfg(unix)]
    #[test]
    fn saving_through_a_relative_symlink_to_a_file_not_there_yet_creates_it() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join("dotfiles")).unwrap();
        let link = dir.path().join("config.toml");
        std::os::unix::fs::symlink("dotfiles/kletse-rss.toml", &link).unwrap();
        save_to(&full(), &link).unwrap();
        assert!(fs::symlink_metadata(&link).unwrap().file_type().is_symlink());
        assert_eq!(read_from(&dir.path().join("dotfiles").join("kletse-rss.toml")), full());
    }

    #[test]
    fn debug_output_leaves_out_secrets() {
        let mut cfg = full();
        cfg.miniflux.api_key = "mf-secret-123".into();
        cfg.readeck = Some(Readeck { url: "https://r.example.com".into(), token: "rd-secret-456".into() });
        let shown = format!("{cfg:?} {cfg:#?}");
        assert!(!shown.contains("mf-secret-123") && !shown.contains("rd-secret-456"), "{shown}");
        assert!(shown.contains("https://m.example.com") && shown.contains("<redacted>"), "{shown}");
        assert!(format!("{:?}", Config::default()).contains(r#"api_key: """#));
    }
}
