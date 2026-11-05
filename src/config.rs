//! `~/.config/onward/config.toml`: the feed server, Miniflux or a Google Reader API server (one is
//! required), and Readeck (optional). Written by the onboarding/settings screen; can also be edited by hand.

use serde::{Deserialize, Serialize};
use std::{
    fmt, fs,
    path::{Path, PathBuf},
};

use crate::error::Error;
use crate::feeds::Capabilities;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Config {
    /// Which server to read from, when the file says. Without it, the one whose section is complete; Miniflux
    /// when both are (see `server()`). The settings form always writes it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server: Option<Server>,
    #[serde(default, skip_serializing_if = "Miniflux::is_empty")]
    pub miniflux: Miniflux,
    /// A Google Reader API server: FreshRSS or Miniflux's own integration.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub greader: Option<GReader>,
    /// `None` when Readeck is not set up: the bookmark button is hidden.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub readeck: Option<Readeck>,
}

/// The kind of feed server.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Server {
    /// Miniflux's own API (`/v1`).
    #[default]
    Miniflux,
    /// The Google Reader API (`/reader/api/0`).
    GReader,
}

impl Server {
    /// Every kind, in the order the settings form offers them.
    pub const ALL: [Server; 2] = [Server::Miniflux, Server::GReader];

    /// What this kind of server can do.
    pub fn capabilities(self) -> Capabilities {
        crate::providers::provider(self).capabilities
    }

    /// Where the kind stands in `ALL` and `providers::PROVIDERS`. A new kind stops the build here, and belongs in both.
    pub(crate) const fn place(self) -> usize {
        match self {
            Server::Miniflux => 0,
            Server::GReader => 1,
        }
    }

    /// As the config file and the windows name it.
    pub fn name(self) -> &'static str {
        match self {
            Server::Miniflux => "miniflux",
            Server::GReader => "greader",
        }
    }

    /// The other way round; `None` for a name that isn't a kind of server, which must not pass for one.
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "miniflux" => Some(Server::Miniflux),
            "greader" => Some(Server::GReader),
            _ => None,
        }
    }
}

const _: () = {
    let mut at = 0;
    while at < Server::ALL.len() {
        assert!(Server::ALL[at].place() == at, "Server::ALL lists every kind, in the order of `place`");
        at += 1;
    }
};

/// `Debug` leaves out the API key.
#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Miniflux {
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub api_key: String,
}

impl Miniflux {
    fn is_empty(&self) -> bool {
        self.url.is_empty() && self.api_key.is_empty()
    }

    pub fn is_complete(&self) -> bool {
        !self.url.is_empty() && !self.api_key.is_empty()
    }
}

/// A Google Reader API server. `url` is the API's root, the part before `/accounts/ClientLogin`:
/// `https://host/api/greader.php` for FreshRSS, `https://host` for Miniflux. `Debug` leaves out the password.
#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GReader {
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub username: String,
    /// The API password, which FreshRSS keeps apart from the one you log in to the web page with.
    #[serde(default)]
    pub password: String,
}

impl GReader {
    /// Whether this is FreshRSS, whose API lives in `greader.php`: no other server has that in its address, and every
    /// config has it, however old, as FreshRSS answers nowhere else. Asking the server would be a request that can
    /// only answer for itself: Miniflux's `subscription/export` gives 200 and `[]`, so a route that answers proves
    /// nothing. An address that hides it behind a rewrite reads as "not FreshRSS", which offers less, and never
    /// claims more than happened.
    pub fn is_freshrss(&self) -> bool {
        is_freshrss(&self.url)
    }

    fn filled(&self) -> usize {
        [&self.url, &self.username, &self.password].iter().filter(|s| !s.is_empty()).count()
    }

    pub fn is_complete(&self) -> bool {
        self.filled() == 3
    }
}

/// See `GReader::is_freshrss`.
pub(crate) fn is_freshrss(url: &str) -> bool {
    url.trim_end_matches('/').to_ascii_lowercase().ends_with("/greader.php")
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

impl fmt::Debug for GReader {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GReader")
            .field("url", &self.url)
            .field("username", &self.username)
            .field("password", &Redacted(&self.password))
            .finish()
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

/// `$XDG_CONFIG_HOME/onward/config.toml`, or `None` when there is no home directory at all.
pub fn path() -> Option<PathBuf> {
    dirs::config_dir()
        .or_else(|| dirs::home_dir().map(|home| home.join(".config")))
        .map(|dir| crate::moved::dir(&dir).join("config.toml"))
}

impl Config {
    /// Trims values, drops trailing slashes, and treats a half-empty Readeck section, or an empty Google Reader
    /// one, as "not set up". (A half-filled Google Reader section is kept, for `validate` to say what's missing.)
    pub fn normalized(mut self) -> Self {
        self.miniflux.url = clean_url(&self.miniflux.url);
        self.miniflux.api_key = self.miniflux.api_key.trim().to_string();
        self.greader = self.greader.and_then(|g| {
            // A password may well begin or end with a space; it is kept as typed.
            let g = GReader { url: clean_url(&g.url), username: g.username.trim().to_string(), password: g.password };
            (g.filled() > 0).then_some(g)
        });
        self.readeck = self.readeck.and_then(|r| {
            let r = Readeck { url: clean_url(&r.url), token: r.token.trim().to_string() };
            (!r.url.is_empty() && !r.token.is_empty()).then_some(r)
        });
        self
    }

    /// The server to read from: the one the file names, or else the one whose section is complete, Miniflux
    /// when both or neither are.
    pub fn server(&self) -> Server {
        match self.server {
            Some(server) => server,
            None if !self.miniflux.is_complete() && self.greader.as_ref().is_some_and(GReader::is_complete) => {
                Server::GReader
            }
            None => Server::Miniflux,
        }
    }

    /// What the chosen server can do: what its kind can, and what's particular to the one at this address.
    pub fn capabilities(&self) -> Capabilities {
        let server = self.server();
        let freshrss = server == Server::GReader && self.greader.as_ref().is_some_and(GReader::is_freshrss);
        Capabilities { import_feeds: freshrss || server.capabilities().import_feeds, ..server.capabilities() }
    }

    /// True when the chosen server's settings are all there.
    pub fn is_complete(&self) -> bool {
        match self.server() {
            Server::Miniflux => self.miniflux.is_complete(),
            Server::GReader => self.greader.as_ref().is_some_and(GReader::is_complete),
        }
    }

    /// Checks that the chosen server is filled in, that a Google Reader section isn't half filled, and that every
    /// configured server address is a usable http(s) URL.
    pub fn validate(&self) -> Result<(), Error> {
        if let Some(greader) = &self.greader
            && !greader.is_complete()
        {
            return Err(Error::Setup(GREADER_INCOMPLETE.into()));
        }
        if !self.is_complete() {
            let missing = match self.server() {
                Server::Miniflux => "The Miniflux URL and API key are required.",
                Server::GReader => GREADER_INCOMPLETE,
            };
            return Err(Error::Setup(missing.into()));
        }
        if self.miniflux.is_complete() {
            validate_url(&self.miniflux.url)?;
        }
        if let Some(greader) = &self.greader {
            validate_url(&greader.url)?;
        }
        if let Some(readeck) = &self.readeck {
            validate_url(&readeck.url)?;
        }
        Ok(())
    }
}

pub(crate) const GREADER_INCOMPLETE: &str = "The Google Reader API server needs a URL, a username and a password.";

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

/// Reads the config file. No file (or no home directory) is an empty config; a file that can't be read or
/// parsed is an error that says where, so the settings form can show it instead of quietly starting over.
pub fn read(path: Option<&Path>) -> Result<Config, Error> {
    path.map_or(Ok(Config::default()), read_from)
}

fn read_from(path: &Path) -> Result<Config, Error> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Config::default()),
        Err(source) => return Err(Error::Io { context: format!("Could not read {}", path.display()), source }),
    };
    parse(&bytes).map(Config::normalized).map_err(|problem| {
        Error::Setup(format!(
            "Couldn't use {}. {problem}. Fix the file and restart the app, or save this form; the old file is then \
             kept as {}.",
            path.display(),
            broken_name(path).display(),
        ))
    })
}

/// A config file's contents, or what is wrong with them ("Line 3: string values must be quoted").
fn parse(bytes: &[u8]) -> Result<Config, String> {
    let text = std::str::from_utf8(bytes).map_err(|_| "It is not a text file".to_string())?;
    let config: Config = toml::from_str(text).map_err(|e| match e.span() {
        Some(span) => {
            let line = text[..span.start.min(text.len())].matches('\n').count() + 1;
            format!("Line {line}: {}", e.message().trim_end_matches('.'))
        }
        None => e.message().trim_end_matches('.').to_string(),
    })?;
    // Said here rather than left to `validate`, so it shows when the app starts, above the form that fixes it.
    if let Some(greader) = &config.greader
        && greader.filled() > 0
        && !greader.is_complete()
    {
        return Err("The [greader] section needs a url, a username and a password".into());
    }
    Ok(config)
}

/// Where a config file that doesn't parse is kept when a save replaces it: `config.toml.broken`.
fn broken_name(path: &Path) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".broken");
    path.with_file_name(name)
}

/// Writes the config (mode 600: it holds API keys).
pub fn save(config: &Config, path: Option<&Path>) -> Result<(), Error> {
    let path = path.ok_or_else(|| Error::Setup("Could not find your home directory to save the settings in".into()))?;
    save_to(config, path)
}

fn save_to(config: &Config, path: &Path) -> Result<(), Error> {
    let body = toml::to_string(config)
        .map_err(|e| Error::Io { context: "Could not write the settings".into(), source: std::io::Error::other(e) })?;
    let text = format!(
        "# onward configuration. Also editable from the app: press , or use Settings in the feeds picker.\n\n{body}"
    );
    keep_if_broken(path)?;
    write_private(path, text.as_bytes())
        .map_err(|source| Error::Io { context: format!("Could not write {}", path.display()), source })
}

/// Before a save replaces a config that doesn't parse, copies it to `<name>.broken`: it may hold changes made
/// by hand with a typo in them. One that can't even be read is left alone, and the save refused.
fn keep_if_broken(path: &Path) -> Result<(), Error> {
    let path = resolve_symlinks(path);
    let bytes = match fs::read(&path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(source) => {
            let context = format!("Could not read {} to keep a copy before replacing it", path.display());
            return Err(Error::Io { context, source });
        }
    };
    if parse(&bytes).is_ok() {
        return Ok(());
    }
    let broken = broken_name(&path);
    write_private(&broken, &bytes)
        .map_err(|source| Error::Io { context: format!("Could not keep a copy as {}", broken.display()), source })
}

/// Writes the file readable by the owner only, creating its directory. The text goes to a new temporary file
/// next to it (random name, created exclusively, mode 600), which then replaces the old file in one step: a crash
/// or a full disk leaves the old config, never half a new one, which `read` would take for no config at all.
///
/// The price of replacing rather than overwriting: the directory must be writable, not just the file, and the
/// new file is a new inode (hard links to the old one keep the old contents).
fn write_private(path: &Path, contents: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let path = resolve_symlinks(path);
    let dir = path.parent().unwrap_or(Path::new(""));
    fs::create_dir_all(dir)?;
    let mut prefix = std::ffi::OsString::from(".");
    prefix.push(path.file_name().unwrap_or_default());
    prefix.push(".");
    let mut tmp = tempfile::Builder::new().prefix(&prefix).suffix(".tmp").tempfile_in(dir)?;
    tmp.write_all(contents)?;
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
            ..Config::default()
        }
    }

    fn greader() -> Config {
        Config {
            server: Some(Server::GReader),
            greader: Some(GReader {
                url: "https://rss.example.com/api/greader.php".into(),
                username: "ward".into(),
                password: "pass word".into(),
            }),
            ..Config::default()
        }
    }

    #[test]
    fn normalized_trims_and_strips_trailing_slashes() {
        let cfg = Config {
            miniflux: Miniflux { url: "  https://m.example.com/// ".into(), api_key: " key\n".into() },
            readeck: Some(Readeck { url: " https://r.example.com/ ".into(), token: "\ttok ".into() }),
            ..Config::default()
        }
        .normalized();
        assert_eq!(cfg, full());
    }

    #[test]
    fn half_empty_readeck_means_not_set_up() {
        for (url, token) in [("", "tok"), ("https://r.example.com", ""), ("  ", " ")] {
            let readeck = Some(Readeck { url: url.into(), token: token.into() });
            let cfg = Config { miniflux: full().miniflux, readeck, ..Config::default() }.normalized();
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
    fn a_missing_file_is_an_empty_config() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(read_from(&dir.path().join("config.toml")).unwrap(), Config::default());
    }

    #[test]
    fn a_broken_file_is_an_error_that_says_where_and_what_saving_does() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        fs::write(&path, "[miniflux]\nurl = \"https://m.example.com\"\napi_key = oops\n").unwrap();
        let err = read_from(&path).unwrap_err();
        assert_eq!(err.kind(), crate::error::ErrorKind::Setup);
        let message = err.message();
        assert!(message.starts_with(&format!("Couldn't use {}. Line 3: ", path.display())), "{message}");
        assert!(message.ends_with(&format!("the old file is then kept as {}.broken.", path.display())), "{message}");

        fs::write(&path, "[miniflux]\nurl = 5\n").unwrap();
        assert!(read_from(&path).unwrap_err().message().contains(". Line 2: invalid type"));
        fs::write(&path, b"\xff\xfe").unwrap();
        assert!(read_from(&path).unwrap_err().message().contains(". It is not a text file. "));
    }

    #[test]
    fn saving_over_a_broken_file_keeps_it_as_broken() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let typo = "[miniflux]\nurl = \"https://m.example.com\"\napi_key = oops\n# notes I want to keep\n";
        fs::write(&path, typo).unwrap();
        save_to(&full(), &path).unwrap();
        assert_eq!(read_from(&path).unwrap(), full());
        let broken = dir.path().join("config.toml.broken");
        assert_eq!(fs::read_to_string(&broken).unwrap(), typo);
        #[cfg(unix)]
        assert_eq!(fs::metadata(&broken).unwrap().permissions().mode() & 0o777, 0o600);

        // A good file is simply replaced; the copy from before stays as it was.
        let mut changed = full();
        changed.readeck = None;
        save_to(&changed, &path).unwrap();
        assert_eq!(read_from(&path).unwrap(), changed);
        assert_eq!(fs::read_to_string(&broken).unwrap(), typo);
    }

    #[cfg(unix)]
    #[test]
    fn a_file_that_cant_be_read_is_not_replaced() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        fs::write(&path, "whatever is in here").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o000)).unwrap();
        let saved = save_to(&full(), &path);
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        let Err(err) = saved else { return }; // running as root, which ignores the mode
        assert!(err.message().starts_with(&format!("Could not read {} to keep a copy", path.display())));
        assert!(read_from(&path).is_err(), "the original is still there, unparsed");
        assert_eq!(fs::read_to_string(&path).unwrap(), "whatever is in here");
    }

    #[test]
    fn save_then_read_round_trips_with_private_mode() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join("config.toml");
        save_to(&full(), &path).unwrap();
        assert_eq!(read_from(&path).unwrap(), full());
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.starts_with("# onward configuration"));
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
        assert_eq!(read_from(&path).unwrap(), full());
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
        assert_eq!(read_from(&path).unwrap(), full());
        let names: Vec<_> = fs::read_dir(dir.path()).unwrap().map(|e| e.unwrap().file_name()).collect();
        assert_eq!(names, ["config.toml"]);
    }

    #[cfg(unix)]
    #[test]
    fn saving_through_a_symlink_keeps_the_link() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("dotfiles").join("onward.toml");
        fs::create_dir(target.parent().unwrap()).unwrap();
        fs::write(&target, "").unwrap();
        let link = dir.path().join("config.toml");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        save_to(&full(), &link).unwrap();
        assert!(fs::symlink_metadata(&link).unwrap().file_type().is_symlink());
        assert_eq!(read_from(&target).unwrap(), full());
        assert_eq!(fs::metadata(&target).unwrap().permissions().mode() & 0o777, 0o600);
    }

    #[cfg(unix)]
    #[test]
    fn saving_through_a_relative_symlink_to_a_file_not_there_yet_creates_it() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join("dotfiles")).unwrap();
        let link = dir.path().join("config.toml");
        std::os::unix::fs::symlink("dotfiles/onward.toml", &link).unwrap();
        save_to(&full(), &link).unwrap();
        assert!(fs::symlink_metadata(&link).unwrap().file_type().is_symlink());
        assert_eq!(read_from(&dir.path().join("dotfiles").join("onward.toml")).unwrap(), full());
    }

    #[test]
    fn an_old_miniflux_file_reads_as_it_always_did() {
        let text = "[miniflux]\nurl = \"https://m.example.com\"\napi_key = \"key\"\n\n[readeck]\nurl = \"https://r.example.com\"\ntoken = \"tok\"\n";
        let cfg = parse(text.as_bytes()).unwrap().normalized();
        assert_eq!(cfg, full());
        assert_eq!(cfg.server(), Server::Miniflux);
        assert!(cfg.is_complete());
        cfg.validate().unwrap();
        // And saving it again writes no new keys.
        let saved = toml::to_string(&cfg).unwrap();
        assert!(!saved.contains("server") && !saved.contains("greader"), "{saved}");
    }

    #[test]
    fn a_greader_file_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        save_to(&greader(), &path).unwrap();
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains("server = \"greader\"") && text.contains("[greader]"), "{text}");
        assert!(!text.contains("[miniflux]"), "an empty Miniflux section isn't written: {text}");
        let cfg = read_from(&path).unwrap();
        assert_eq!(cfg, greader());
        assert_eq!(cfg.server(), Server::GReader);
        assert!(cfg.is_complete());
        cfg.validate().unwrap();
    }

    #[test]
    fn the_server_is_the_complete_section_or_the_one_the_file_names() {
        let mut cfg = greader();
        cfg.server = None;
        assert_eq!(cfg.server(), Server::GReader, "only the Google Reader section is complete");
        cfg.miniflux = full().miniflux;
        assert_eq!(cfg.server(), Server::Miniflux, "both are: Miniflux, as before");
        cfg.server = Some(Server::GReader);
        assert_eq!(cfg.server(), Server::GReader);
        let named: Config =
            toml::from_str("server = \"greader\"\n[miniflux]\nurl = \"https://m\"\napi_key = \"k\"\n").unwrap();
        assert_eq!(named.server(), Server::GReader);
        assert!(!named.is_complete(), "the named server has no settings");
        assert!(named.validate().unwrap_err().message().contains("Google Reader"));
    }

    #[test]
    fn freshrss_is_known_by_its_address_also_in_a_config_from_before_detection() {
        let at = |url: &str| {
            let file =
                format!("server = \"greader\"\n[greader]\nurl = \"{url}\"\nusername = \"u\"\npassword = \"p\"\n");
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("config.toml");
            fs::write(&path, file).unwrap();
            read(Some(&path)).unwrap().capabilities()
        };
        for url in [
            "https://rss.example.com/api/greader.php",
            "https://rss.example.com/api/greader.php/",
            "https://example.com/freshrss/p/api/GReader.php",
        ] {
            assert!(at(url).import_feeds, "{url}");
        }
        // Miniflux's integration, and whatever else speaks the protocol.
        for url in ["https://rss.example.com", "https://rss.example.com/miniflux", "https://x.example/greader.php.bak"]
        {
            assert!(!at(url).import_feeds, "{url}");
            assert!(at(url).export_feeds, "{url}");
        }
    }

    #[test]
    fn miniflux_does_both_and_each_operation_stands_alone() {
        assert!(full().capabilities().import_feeds && full().capabilities().export_feeds);
        let freshrss = GReader { url: "https://a.example/api/greader.php".into(), ..GReader::default() };
        let with = Config { server: Some(Server::GReader), greader: Some(freshrss), ..Config::default() };
        assert!(with.capabilities().import_feeds);
        assert!(!with.capabilities().fetch_full && !with.capabilities().refresh, "the rest stays the protocol's");
        let chose_miniflux = Config { server: Some(Server::Miniflux), ..with };
        assert!(chose_miniflux.capabilities().fetch_full);
    }

    #[test]
    fn a_name_that_is_no_kind_of_server_is_not_taken_for_one() {
        for kind in Server::ALL {
            assert_eq!(Server::from_name(kind.name()), Some(kind));
        }
        for name in ["", "Miniflux", "feedbin"] {
            assert_eq!(Server::from_name(name), None, "{name:?}");
        }
    }

    #[test]
    fn a_config_file_naming_an_unknown_server_is_refused_not_read_as_miniflux() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        fs::write(&path, "server = \"feedbin\"\n[miniflux]\nurl = \"https://m\"\napi_key = \"k\"\n").unwrap();
        assert!(read_from(&path).unwrap_err().message().contains("feedbin"));
    }

    #[test]
    fn a_half_filled_greader_section_is_an_error_that_says_what_is_missing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        fs::write(&path, "[greader]\nurl = \"https://rss.example.com/api/greader.php\"\nusername = \"ward\"\n")
            .unwrap();
        let message = read_from(&path).unwrap_err().message();
        assert!(message.contains("The [greader] section needs a url, a username and a password"), "{message}");

        let mut cfg = greader();
        cfg.greader.as_mut().unwrap().password.clear();
        let err = cfg.validate().unwrap_err();
        assert_eq!(err.message(), "The Google Reader API server needs a URL, a username and a password.");
        // An empty one is just not set up.
        let cfg = Config { greader: Some(GReader::default()), ..full() }.normalized();
        assert_eq!(cfg.greader, None);
    }

    #[test]
    fn debug_output_leaves_out_secrets() {
        let mut cfg = full();
        cfg.miniflux.api_key = "mf-secret-123".into();
        cfg.readeck = Some(Readeck { url: "https://r.example.com".into(), token: "rd-secret-456".into() });
        cfg.greader = Some(GReader { password: "gr-secret-789".into(), ..greader().greader.unwrap() });
        let shown = format!("{cfg:?} {cfg:#?}");
        assert!(!shown.contains("mf-secret-123") && !shown.contains("rd-secret-456"), "{shown}");
        assert!(!shown.contains("gr-secret-789") && shown.contains("ward"), "{shown}");
        assert!(shown.contains("https://m.example.com") && shown.contains("<redacted>"), "{shown}");
        assert!(format!("{:?}", Config::default()).contains(r#"api_key: """#));
    }
}
