//! No test here, or anywhere, can reach a real keyring: `system.rs` isn't compiled into them, `Secrets::system` gives
//! tests a store that panics, and every `Backend` a test makes has one from this file.

#![allow(clippy::unwrap_used)]

use super::*;
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

/// What tests get in place of the machine's store. Nothing should ever call it.
pub(crate) struct NoStoreInTests;

impl Store for NoStoreInTests {
    fn get<'a>(&'a self, _: &'a SecretId) -> Reply<'a, Option<String>> {
        panic!("a test reached for the machine's store");
    }
    fn put<'a>(&'a self, _: &'a SecretId, _: &'a str) -> Reply<'a, ()> {
        panic!("a test reached for the machine's store");
    }
    fn delete<'a>(&'a self, _: &'a SecretId) -> Reply<'a, ()> {
        panic!("a test reached for the machine's store");
    }
}

/// What a fake store does when it's called.
#[derive(Clone, PartialEq)]
pub(crate) enum Act {
    Works,
    Fails(Failure),
    /// Never answers, as a keyring waiting on a prompt nobody's at.
    Hangs,
    /// Says yes to anything and holds on to nothing.
    Forgets,
    /// Takes the first `n` secrets, then fails.
    FailsAfter(usize),
}

/// A store in memory that does as it's told.
#[derive(Clone, Default)]
pub(crate) struct Scripted(Arc<Inner>);

#[derive(Default)]
struct Inner {
    held: Mutex<HashMap<String, String>>,
    act: Mutex<Option<Act>>,
    puts: AtomicUsize,
    calls: AtomicUsize,
}

impl Scripted {
    pub fn set(&self, act: Act) {
        *self.0.act.lock().unwrap() = Some(act);
    }

    pub fn holds(&self, id: &SecretId) -> Option<String> {
        self.0.held.lock().unwrap().get(&id.account()).cloned()
    }

    pub fn put_in(&self, id: &SecretId, secret: &str) {
        self.0.held.lock().unwrap().insert(id.account(), secret.to_string());
    }

    pub fn calls(&self) -> usize {
        self.0.calls.load(Ordering::SeqCst)
    }

    fn act(&self) -> Act {
        self.0.calls.fetch_add(1, Ordering::SeqCst);
        self.0.act.lock().unwrap().clone().unwrap_or(Act::Works)
    }

    async fn stall_or(&self, act: Act) -> Result<(), Failure> {
        match act {
            Act::Hangs => std::future::pending().await,
            Act::Fails(failure) => Err(failure),
            _ => Ok(()),
        }
    }
}

impl Store for Scripted {
    fn get<'a>(&'a self, id: &'a SecretId) -> Reply<'a, Option<String>> {
        Box::pin(async move {
            let act = self.act();
            let refuses_reads = matches!(act, Act::Hangs | Act::Fails(_));
            self.stall_or(if refuses_reads { act } else { Act::Works }).await?;
            Ok(self.holds(id))
        })
    }

    fn put<'a>(&'a self, id: &'a SecretId, secret: &'a str) -> Reply<'a, ()> {
        Box::pin(async move {
            let act = self.act();
            if let Act::FailsAfter(n) = act {
                if self.0.puts.fetch_add(1, Ordering::SeqCst) >= n {
                    return Err(Failure::Unreachable);
                }
            } else if act == Act::Forgets {
                return Ok(());
            } else {
                self.stall_or(act).await?;
            }
            self.put_in(id, secret);
            Ok(())
        })
    }

    fn delete<'a>(&'a self, id: &'a SecretId) -> Reply<'a, ()> {
        Box::pin(async move {
            let act = self.act();
            self.stall_or(act).await?;
            self.0.held.lock().unwrap().remove(&id.account());
            Ok(())
        })
    }
}

pub(crate) const FILE: &str = "state/onward/secrets.toml";

/// The usual store for a test: empty, working, with its file under `dir`.
pub(crate) fn in_memory(dir: &Path) -> Secrets {
    scripted(dir, &Scripted::default())
}

pub(crate) fn scripted(dir: &Path, store: &Scripted) -> Secrets {
    Secrets::new(Box::new(store.clone()), Some(dir.join(FILE)), false, Duration::from_millis(50))
}

pub(crate) fn id(kind: Kind, url: &str, username: &str) -> SecretId {
    SecretId { kind, url: url.to_string(), username: username.to_string() }
}

fn miniflux() -> SecretId {
    id(Kind::Miniflux, "https://m.example.com", "")
}

fn greader() -> SecretId {
    id(Kind::GReader, "https://f.example.com/api/greader.php", "ward")
}

fn wanted(ids: &[&SecretId]) -> Vec<(SecretId, String)> {
    ids.iter().map(|&id| (id.clone(), format!("secret of {}", id.kind.name()))).collect()
}

async fn read_one(secrets: &Secrets, id: &SecretId) -> Result<Option<String>, Failure> {
    secrets.read(vec![id.clone()]).await.remove(0).1
}

#[test]
fn tests_are_handed_a_store_that_is_not_the_machines() {
    // Would not compile if `system` were part of the build: nothing in it is reachable from here.
    let source = include_str!("secrets.rs");
    assert!(source.contains("#[cfg(not(test))]\nmod system;"), "system.rs stays out of tests");
    let store = Secrets::system().store;
    let id = miniflux();
    let reached = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(store.get(&id))));
    assert!(reached.is_err(), "the store tests get refuses every call");
}

#[test]
fn only_system_rs_names_the_keyring_crates() {
    let mut files = vec![std::path::PathBuf::from("src")];
    while let Some(path) = files.pop() {
        if path.is_dir() {
            files.extend(std::fs::read_dir(&path).unwrap().map(|entry| entry.unwrap().path()));
        } else if path.extension().is_some_and(|e| e == "rs") && !path.ends_with("secrets/system.rs") {
            let text = std::fs::read_to_string(&path).unwrap();
            for name in ["secret_service::", "security_framework::", "zbus::", "system::store"] {
                let found = text.lines().any(|line| line.contains(name) && !line.contains("\""));
                let allowed = path.ends_with("secrets.rs") && name == "system::store";
                assert!(!found || allowed, "{name} in {}", path.display());
            }
        }
    }
    assert!(!Path::new("tests").exists(), "an integration test links the library as shipped, keyring and all");
}

#[tokio::test]
async fn a_secret_goes_to_the_store_and_comes_back() {
    let dir = tempfile::tempdir().unwrap();
    let store = Scripted::default();
    let secrets = scripted(dir.path(), &store);
    let place = secrets.save(wanted(&[&miniflux(), &greader()])).await.unwrap();
    assert_eq!(place, Place::Keyring { omarchy: false });
    assert_eq!(read_one(&secrets, &miniflux()).await, Ok(Some("secret of miniflux".into())));
    assert_eq!(read_one(&secrets, &greader()).await, Ok(Some("secret of greader".into())));
    assert!(!dir.path().join(FILE).exists(), "the file is only for when the store fails");
}

#[tokio::test]
async fn nothing_there_and_no_answer_are_not_the_same() {
    let dir = tempfile::tempdir().unwrap();
    let store = Scripted::default();
    let secrets = scripted(dir.path(), &store);
    assert_eq!(read_one(&secrets, &miniflux()).await, Ok(None));
    for failure in [Failure::Locked, Failure::Unreachable] {
        store.set(Act::Fails(failure.clone()));
        assert_eq!(read_one(&secrets, &miniflux()).await, Err(failure));
    }
}

#[tokio::test]
async fn a_store_that_never_answers_counts_as_failing_and_is_asked_once() {
    let dir = tempfile::tempdir().unwrap();
    let store = Scripted::default();
    store.set(Act::Hangs);
    let secrets = scripted(dir.path(), &store);
    let answers = secrets.read(vec![miniflux(), greader()]).await;
    assert!(answers.iter().all(|(_, answer)| *answer == Err(Failure::TimedOut)));
    assert_eq!(store.calls(), 1, "each would wait as long");
}

#[tokio::test]
async fn a_store_that_fails_sends_the_secret_to_a_private_file() {
    let dir = tempfile::tempdir().unwrap();
    let store = Scripted::default();
    store.set(Act::Fails(Failure::Unreachable));
    let secrets = scripted(dir.path(), &store);
    let place = secrets.save(wanted(&[&miniflux()])).await.unwrap();
    let path = dir.path().join(FILE);
    assert_eq!(place, Place::File(path.display().to_string()));
    assert_eq!(read_one(&secrets, &miniflux()).await, Ok(Some("secret of miniflux".into())));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
    }
}

#[tokio::test]
async fn a_secret_only_counts_as_stored_if_it_reads_back() {
    let dir = tempfile::tempdir().unwrap();
    let store = Scripted::default();
    store.set(Act::Forgets);
    let secrets = scripted(dir.path(), &store);
    assert!(matches!(secrets.save(wanted(&[&miniflux()])).await.unwrap(), Place::File(_)));
}

#[tokio::test]
async fn what_the_store_would_not_take_goes_to_the_file_and_the_rest_stays() {
    let dir = tempfile::tempdir().unwrap();
    let store = Scripted::default();
    store.set(Act::FailsAfter(1));
    let secrets = scripted(dir.path(), &store);
    assert!(matches!(secrets.save(wanted(&[&miniflux(), &greader()])).await.unwrap(), Place::File(_)));
    assert!(store.holds(&miniflux()).is_some());
    assert_eq!(store.holds(&greader()), None);
    assert_eq!(read_one(&secrets, &greader()).await, Ok(Some("secret of greader".into())));
}

#[tokio::test]
async fn moving_back_to_the_store_takes_the_copy_out_of_the_file() {
    let dir = tempfile::tempdir().unwrap();
    let store = Scripted::default();
    store.set(Act::Fails(Failure::Unreachable));
    let secrets = scripted(dir.path(), &store);
    secrets.save(wanted(&[&miniflux(), &greader()])).await.unwrap();
    store.set(Act::Works);
    assert_eq!(secrets.save(wanted(&[&miniflux(), &greader()])).await.unwrap(), Place::Keyring { omarchy: false });
    assert!(!dir.path().join(FILE).exists(), "with nothing left in it, the file goes");
}

#[cfg(unix)]
#[tokio::test]
async fn the_file_behind_a_symlink_is_replaced_and_emptied_there() {
    let dir = tempfile::tempdir().unwrap();
    let link = dir.path().join(FILE);
    let real = dir.path().join("elsewhere.toml");
    std::fs::create_dir_all(link.parent().unwrap()).unwrap();
    std::os::unix::fs::symlink(&real, &link).unwrap();
    let store = Scripted::default();
    store.set(Act::Fails(Failure::Unreachable));
    let secrets = scripted(dir.path(), &store);
    secrets.save(wanted(&[&miniflux()])).await.unwrap();
    assert!(link.symlink_metadata().unwrap().file_type().is_symlink());
    assert!(std::fs::read_to_string(&real).unwrap().contains("secret of miniflux"));
    store.set(Act::Works);
    secrets.save(wanted(&[&miniflux()])).await.unwrap();
    assert!(!real.exists(), "the secret itself is what goes, not the link to it");
}

#[cfg(unix)]
#[tokio::test]
async fn a_save_repairs_the_files_mode() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let store = Scripted::default();
    store.set(Act::Fails(Failure::Unreachable));
    let secrets = scripted(dir.path(), &store);
    secrets.save(wanted(&[&miniflux()])).await.unwrap();
    let path = dir.path().join(FILE);
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
    secrets.save(wanted(&[&greader()])).await.unwrap();
    assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
}

#[tokio::test]
async fn with_the_store_and_the_file_both_failing_the_save_fails_and_says_so() {
    let dir = tempfile::tempdir().unwrap();
    let blocked = dir.path().join("not-a-folder");
    std::fs::write(&blocked, "").unwrap();
    let store = Scripted::default();
    store.set(Act::Fails(Failure::Locked));
    let secrets = Secrets::new(Box::new(store), Some(blocked.join("secrets.toml")), false, WAIT);
    let message = secrets.save(wanted(&[&miniflux()])).await.unwrap_err().message();
    assert!(message.contains("Nothing was saved"), "{message}");
    assert!(message.contains("secrets.toml"), "{message}");
}

#[tokio::test]
async fn a_file_that_cannot_be_parsed_is_no_answer_and_is_not_replaced() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(FILE);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, "not = [toml").unwrap();
    let store = Scripted::default();
    let secrets = scripted(dir.path(), &store);
    assert!(matches!(read_one(&secrets, &miniflux()).await, Err(Failure::File(_))));
    store.put_in(&miniflux(), "in the store");
    assert_eq!(read_one(&secrets, &miniflux()).await, Ok(Some("in the store".into())));
    assert!(secrets.save(wanted(&[&miniflux()])).await.is_err());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "not = [toml");
}

#[tokio::test]
async fn forgetting_takes_a_secret_out_of_both() {
    let dir = tempfile::tempdir().unwrap();
    let store = Scripted::default();
    let secrets = scripted(dir.path(), &store);
    store.set(Act::Fails(Failure::Unreachable));
    secrets.save(wanted(&[&greader()])).await.unwrap();
    store.set(Act::Works);
    store.put_in(&miniflux(), "old");
    let place = Place::Keyring { omarchy: false };
    secrets.forget(vec![miniflux(), greader()], &place).await;
    assert_eq!(store.holds(&miniflux()), None);
    assert!(!dir.path().join(FILE).exists());
}

#[tokio::test]
async fn a_store_that_just_failed_isnt_asked_to_forget() {
    let dir = tempfile::tempdir().unwrap();
    let store = Scripted::default();
    let secrets = scripted(dir.path(), &store);
    secrets.forget(vec![miniflux()], &Place::File(String::new())).await;
    assert_eq!(store.calls(), 0);
}

#[test]
fn the_same_address_means_a_different_secret_for_another_kind_or_user() {
    let url = "https://rss.example.com";
    let accounts = [id(Kind::Miniflux, url, ""), id(Kind::GReader, url, "ward"), id(Kind::GReader, url, "sam")];
    let accounts = accounts.map(|id| id.account());
    assert_ne!(accounts[0], accounts[1]);
    assert_ne!(accounts[1], accounts[2]);
}

#[test]
fn nothing_debug_prints_holds_the_secret() {
    let entry = Entry { kind: Kind::Miniflux, url: "u".into(), username: String::new(), value: "hunter2".into() };
    assert!(!format!("{:?}", in_memory(Path::new("/x"))).contains("hunter2"));
    assert!(!format!("{:?}", miniflux()).contains("hunter2"));
    assert!(!format!("{entry:?}").contains("hunter2"));
}

#[test]
fn the_notices_name_the_file_and_stay_true_after_a_password_is_set() {
    assert!(
        told_file("/home/me/.local/state/onward/secrets.toml").contains("/home/me/.local/state/onward/secrets.toml")
    );
    assert!(told_file("x").contains("only you can read"));
    assert!(TOLD_OMARCHY.contains("until it has one"), "someone may have set one since");
}
