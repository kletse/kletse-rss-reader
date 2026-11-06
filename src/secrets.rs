//! Where what Onward signs in with is kept: the system's store (Secret Service on Linux, the Keychain on macOS), or
//! with none to be had a file only you can read. gh's model: try the store, fall back to a file, say so once.
//!
//! `config.toml` comes before both: a secret written there keeps working and wins, as someone put it there on purpose.
//! One leaves it at a save, and only once it's been stored and read back. Nothing here runs at startup by itself.
//!
//! A store that fails is not a store without a secret. `read` keeps the two apart, as "nothing there" lands you in the
//! form to start over, and a later save would then replace the real thing. Every call to a store has `WAIT` to answer,
//! because a locked one with a display can wait for ever on a prompt nobody is at.
//!
//! The store is handed in. `system.rs` is the only code that can reach the real one, and it isn't compiled into tests.

use serde::{Deserialize, Serialize};
use std::fmt;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::time::Duration;

use crate::config;
use crate::error::Error;

#[cfg(not(test))]
mod system;

/// Long enough to type a keyring's password into its prompt, which outlives this if it's left up: the retry finds it
/// unlocked. A store that's not there says so at once.
const WAIT: Duration = Duration::from_secs(10);

/// How the windows name the store.
const STORE: &str = if cfg!(target_os = "macos") { "Keychain" } else { "keyring" };

/// What a secret is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Miniflux,
    GReader,
    Readeck,
}

impl Kind {
    fn name(self) -> &'static str {
        match self {
            Kind::Miniflux => "miniflux",
            Kind::GReader => "greader",
            Kind::Readeck => "readeck",
        }
    }

    /// As the error messages call it.
    fn credential(self) -> &'static str {
        match self {
            Kind::Miniflux => "API key",
            Kind::GReader => "API password",
            Kind::Readeck => "token",
        }
    }
}

/// Which secret: the kind of server, its address, and the username where there is one. A `config.toml` edited to point
/// elsewhere asks for another secret and finds none, so what belongs to one server can't reach another.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SecretId {
    pub kind: Kind,
    pub url: String,
    #[serde(default)]
    pub username: String,
}

impl SecretId {
    /// What the keyring's own window lists it as.
    #[cfg_attr(any(test, not(target_os = "linux")), allow(dead_code))]
    fn label(&self) -> String {
        format!("Onward: {} at {}", self.kind.credential(), self.url)
    }

    /// One string for a store that has room for no more: the address has no spaces, and the username comes last.
    #[cfg_attr(not(any(test, target_os = "macos")), allow(dead_code))]
    fn account(&self) -> String {
        format!("{} {} {}", self.kind.name(), self.url, self.username)
    }

    pub(crate) fn missing_message(&self) -> String {
        let credential = self.kind.credential();
        format!("Onward has no {credential} for {}, in config.toml or anywhere else. Enter it here.", self.url)
    }
}

/// Why a store gave no answer. None of these says the secret isn't there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Failure {
    /// Locked, and whoever could unlock it didn't, or can't be asked.
    Locked,
    /// No store here, or one that broke off.
    Unreachable,
    /// `WAIT` went by.
    TimedOut,
    /// The file can't be read: its path.
    File(String),
}

impl Failure {
    /// On the feeds list, which is where the app waits for this. `r` tries again from there, and Settings stays open
    /// to it, so a keyring that's gone for good doesn't shut you out.
    pub fn message(&self) -> String {
        match self {
            Failure::Locked => format!(
                "Your {STORE} is locked, so Onward can't read what it signs in with. Your settings are as they were. \
                 Unlock it and press r to try again."
            ),
            Failure::Unreachable => format!(
                "Onward can't reach your {STORE} to read what it signs in with. Your settings are as they were. \
                 Press r to try again, or , to enter it again."
            ),
            Failure::TimedOut => format!(
                "Your {STORE} isn't answering. Your settings are as they were. If it's asking for a password, answer \
                 that, then press r to try again."
            ),
            Failure::File(path) => format!(
                "Onward can't read {path}. Your settings are as they were. Fix the file and press r to try again."
            ),
        }
    }

    /// Above the settings form, whose empty fields would otherwise say there's nothing stored.
    pub fn in_form(&self) -> String {
        let cause = match self {
            Failure::Locked => format!("Your {STORE} is locked"),
            Failure::Unreachable | Failure::TimedOut => format!("Onward can't reach your {STORE}"),
            Failure::File(path) => format!("Onward can't read {path}"),
        };
        format!("{cause}. What Onward signs in with isn't shown here, and what you leave empty stays as it is.")
    }
}

/// Said once, when a secret goes to the file.
pub fn told_file(path: &str) -> String {
    format!(
        "Onward couldn't use a {STORE} here, so what it signs in with is in {path}, a file only you can read. It'll \
         try the {STORE} again each time you save your settings."
    )
}

/// Said once, on Omarchy, whose installer makes a keyring with no password. Still true for someone who has set one.
pub const TOLD_OMARCHY: &str = "What Onward signs in with is in your keyring. Omarchy sets that keyring up with no \
     password, and until it has one, what's in it is unencrypted on this disk: no safer than a file only you can read.";

/// One call to a store, sent off and waited for by `Secrets`.
pub(crate) type Reply<'a, T> = Pin<Box<dyn Future<Output = Result<T, Failure>> + Send + 'a>>;

pub(crate) trait Store: Send + Sync {
    fn get<'a>(&'a self, id: &'a SecretId) -> Reply<'a, Option<String>>;
    fn put<'a>(&'a self, id: &'a SecretId, secret: &'a str) -> Reply<'a, ()>;
    fn delete<'a>(&'a self, id: &'a SecretId) -> Reply<'a, ()>;
}

/// Where a save put the secrets, for what's said about it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Place {
    /// `omarchy`: a stock Omarchy keyring has no password, which the notice says.
    Keyring { omarchy: bool },
    /// The store failed, so at least one of them is in this file.
    File(String),
}

/// The file's shape: `[[secret]]` tables.
#[derive(Default, Serialize, Deserialize)]
struct Held {
    #[serde(default, rename = "secret")]
    entries: Vec<Entry>,
}

/// `Debug` leaves out the value.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Entry {
    kind: Kind,
    url: String,
    #[serde(default)]
    username: String,
    value: String,
}

impl fmt::Debug for Entry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = config::Redacted(&self.value);
        f.debug_struct("Entry").field("kind", &self.kind).field("url", &self.url).field("value", &value).finish()
    }
}

impl Entry {
    fn is(&self, id: &SecretId) -> bool {
        self.kind == id.kind && self.url == id.url && self.username == id.username
    }
}

#[cfg(not(test))]
/// `$XDG_STATE_HOME/onward/secrets.toml`. Not next to `config.toml`, as that whole folder may be a dotfiles repo, and on
/// macOS, where state has no folder of its own and prefs live beside the config, a folder of its own.
fn file_path() -> Option<PathBuf> {
    if cfg!(target_os = "macos") {
        return dirs::data_local_dir().map(|dir| dir.join("be.kletse.onward").join("secrets.toml"));
    }
    dirs::state_dir()
        .or_else(|| dirs::home_dir().map(|home| home.join(".local").join("state")))
        .map(|dir| crate::moved::dir(&dir).join("secrets.toml"))
}

/// Everything in the file. No file is an empty one; one that can't be read or parsed is no answer.
fn read_file(path: Option<&Path>) -> Result<Vec<Entry>, Failure> {
    let Some(path) = path else { return Ok(Vec::new()) };
    let unreadable = || Failure::File(path.display().to_string());
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(_) => return Err(unreadable()),
    };
    toml::from_str::<Held>(&text).map(|held| held.entries).map_err(|_| unreadable())
}

/// Replaces the file in one step, mode 600 whatever it was. With nothing left to hold it goes, also behind a symlink.
fn write_file(path: &Path, entries: Vec<Entry>) -> std::io::Result<()> {
    if entries.is_empty() {
        return match std::fs::remove_file(config::resolve_symlinks(path)) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        };
    }
    let body = toml::to_string(&Held { entries }).map_err(std::io::Error::other)?;
    let text =
        format!("# What Onward signs in with, as no keyring could hold it here. Readable only by you.\n\n{body}");
    config::write_private(path, text.as_bytes())
}

/// Disk work, off the runtime's few workers.
async fn on_disk<T: Send + 'static>(work: impl FnOnce() -> T + Send + 'static) -> Result<T, std::io::Error> {
    tokio::task::spawn_blocking(work).await.map_err(std::io::Error::other)
}

/// The store and the file behind it.
pub struct Secrets {
    store: Box<dyn Store>,
    file: Option<PathBuf>,
    /// Decided once, outside: a stock keyring there has no password.
    omarchy: bool,
    wait: Duration,
}

impl fmt::Debug for Secrets {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secrets { .. }")
    }
}

impl Secrets {
    /// The machine's own.
    #[cfg(not(test))]
    pub fn system() -> Self {
        Self::new(system::store(), file_path(), crate::theme::on_omarchy(), WAIT)
    }

    /// Tests build things that call this, and none of them gets the machine's store: see `NoStoreInTests`.
    #[cfg(test)]
    pub fn system() -> Self {
        Self::new(Box::new(tests::NoStoreInTests), None, false, WAIT)
    }

    pub(crate) fn new(store: Box<dyn Store>, file: Option<PathBuf>, omarchy: bool, wait: Duration) -> Self {
        Self { store, file, omarchy, wait }
    }

    /// A store's answer, or `TimedOut`.
    async fn timed<T>(&self, call: Reply<'_, T>) -> Result<T, Failure> {
        tokio::time::timeout(self.wait, call).await.unwrap_or(Err(Failure::TimedOut))
    }

    /// Each of these from the file, or else the store. A file only ever holds what the store wouldn't take, and it's
    /// no prompt, which is why it comes first. Once the store has failed it's not asked again: each would wait as long.
    pub async fn read(&self, ids: Vec<SecretId>) -> Vec<(SecretId, Result<Option<String>, Failure>)> {
        let path = self.file.clone();
        let file = on_disk(move || read_file(path.as_deref())).await;
        let file = file.unwrap_or_else(|_| Err(Failure::File(String::new())));
        let mut dead: Option<Failure> = None;
        let mut answers = Vec::with_capacity(ids.len());
        for id in ids {
            let held = file.as_ref().ok().and_then(|entries| entries.iter().find(|e| e.is(&id)));
            let answer = match (held, &dead) {
                (Some(entry), _) => Ok(Some(entry.value.clone())),
                (None, Some(failure)) => Err(failure.clone()),
                (None, None) => match self.timed(self.store.get(&id)).await {
                    // Not in the store, but it could be in a file that can't be read.
                    Ok(None) => file.as_ref().map(|_| None).map_err(Clone::clone),
                    Err(failure) => {
                        dead = Some(failure.clone());
                        Err(failure)
                    }
                    found => found,
                },
            };
            answers.push((id, answer));
        }
        answers
    }

    /// Into the store, and it counts only if it reads back the same.
    async fn put_checked(&self, id: &SecretId, secret: &str) -> Result<(), Failure> {
        self.timed(self.store.put(id, secret)).await?;
        match self.timed(self.store.get(id)).await? {
            Some(back) if back == secret => Ok(()),
            _ => Err(Failure::Unreachable),
        }
    }

    /// Keeps these. Each goes to the store, and from the first it won't take, to the file. What's in the file for any
    /// of them that did go to the store leaves it. An `Err` means the file couldn't do its part, and the caller must
    /// not go on to take anything out of `config.toml`.
    pub async fn save(&self, wanted: Vec<(SecretId, String)>) -> Result<Place, Error> {
        let mut for_file: Vec<Entry> = Vec::new();
        for (id, secret) in &wanted {
            if !for_file.is_empty() || self.put_checked(id, secret).await.is_err() {
                let SecretId { kind, url, username } = id.clone();
                for_file.push(Entry { kind, url, username, value: secret.clone() });
            }
        }
        let place = match for_file.is_empty() {
            true => Place::Keyring { omarchy: self.omarchy },
            false => Place::File(self.file.as_ref().map_or_else(String::new, |p| p.display().to_string())),
        };
        let ids: Vec<SecretId> = wanted.into_iter().map(|(id, _)| id).collect();
        let (path, wrote) = (self.file.clone(), for_file.clone());
        let written = on_disk(move || rewrite(path.as_deref(), &ids, wrote)).await.and_then(|written| written);
        written.map_err(|source| failed_save(self.file.as_deref(), source))?;
        Ok(place)
    }

    /// Takes these out of both, once nothing points at them any more. It's done by then, so what fails here fails
    /// quietly: an old copy under an address that's no longer used. The store is left out if the save just found it
    /// failing, as this would only wait as long again.
    pub async fn forget(&self, ids: Vec<SecretId>, place: &Place) {
        if ids.is_empty() {
            return;
        }
        if matches!(place, Place::Keyring { .. }) {
            for id in &ids {
                if self.timed(self.store.delete(id)).await.is_err() {
                    break;
                }
            }
        }
        let path = self.file.clone();
        let _ = on_disk(move || rewrite(path.as_deref(), &ids, Vec::new())).await;
    }
}

/// The file with `gone` taken out and `new` put in, if that changes it. It's read back: the same rule as for the store.
fn rewrite(path: Option<&Path>, gone: &[SecretId], new: Vec<Entry>) -> std::io::Result<()> {
    let unreadable = |_| std::io::Error::other("it can't be read, and may hold more than this");
    let Some(path) = path else {
        return if new.is_empty() { Ok(()) } else { Err(std::io::Error::other("there's no home folder")) };
    };
    let old = read_file(Some(path)).map_err(unreadable)?;
    let mut entries: Vec<Entry> = old.iter().filter(|e| !gone.iter().any(|id| e.is(id))).cloned().collect();
    entries.extend(new);
    if entries == old {
        return Ok(());
    }
    write_file(path, entries.clone())?;
    if read_file(Some(path)).map_err(unreadable)? == entries {
        Ok(())
    } else {
        Err(std::io::Error::other("it didn't read back the same"))
    }
}

fn failed_save(file: Option<&Path>, source: std::io::Error) -> Error {
    let file = file.map_or_else(|| "a file".to_string(), |p| p.display().to_string());
    Error::Io { context: format!("Nothing was saved. Your {STORE} didn't take this, and neither did {file}"), source }
}

#[cfg(test)]
#[path = "secrets_tests.rs"]
pub(crate) mod tests;
