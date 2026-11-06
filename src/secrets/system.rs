//! The system's own store: Secret Service on Linux, the Keychain on macOS. This is the only file that can reach one,
//! and `secrets.rs` leaves it out of tests altogether. To run it, see "Keyrings" in AGENTS.md.

use super::Store;

const _: () = assert!(!cfg!(test), "tests must not be able to reach a real keyring");

#[cfg(target_os = "linux")]
pub(super) fn store() -> Box<dyn Store> {
    Box::new(linux::SecretServiceStore)
}

#[cfg(target_os = "macos")]
pub(super) fn store() -> Box<dyn Store> {
    Box::new(macos::Keychain)
}

/// Elsewhere there's only the file.
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
pub(super) fn store() -> Box<dyn Store> {
    Box::new(None)
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
struct None;

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
impl Store for None {
    fn get<'a>(&'a self, _: &'a super::SecretId) -> super::Reply<'a, Option<String>> {
        Box::pin(async { Err(super::Failure::Unreachable) })
    }
    fn put<'a>(&'a self, _: &'a super::SecretId, _: &'a str) -> super::Reply<'a, ()> {
        Box::pin(async { Err(super::Failure::Unreachable) })
    }
    fn delete<'a>(&'a self, _: &'a super::SecretId) -> super::Reply<'a, ()> {
        Box::pin(async { Err(super::Failure::Unreachable) })
    }
}

#[cfg(target_os = "linux")]
mod linux {
    use super::super::{Failure, Reply, SecretId, Store};
    use secret_service::{EncryptionType, Error, SecretService};
    use std::collections::HashMap;

    pub(super) struct SecretServiceStore;

    fn failed(error: Error) -> Failure {
        match error {
            // A prompt that was dismissed, or that couldn't open: no display, as over SSH.
            Error::Locked | Error::Prompt | Error::PromptDisconnected => Failure::Locked,
            _ => Failure::Unreachable,
        }
    }

    /// The username is always there, if empty: a search matches on the attributes it's given, and no others.
    fn attributes(id: &SecretId) -> HashMap<&str, &str> {
        HashMap::from([
            ("application", "onward"),
            ("kind", id.kind.name()),
            ("url", id.url.as_str()),
            ("username", id.username.as_str()),
        ])
    }

    async fn connect<'a>() -> Result<SecretService<'a>, Failure> {
        SecretService::connect(EncryptionType::Dh).await.map_err(failed)
    }

    impl Store for SecretServiceStore {
        fn get<'a>(&'a self, id: &'a SecretId) -> Reply<'a, Option<String>> {
            Box::pin(async move {
                let service = connect().await?;
                let found = service.search_items(attributes(id)).await.map_err(failed)?;
                let item = match (found.unlocked.first(), found.locked.first()) {
                    (Some(item), _) => item,
                    (None, Some(item)) => {
                        item.unlock().await.map_err(failed)?;
                        item
                    }
                    (None, None) => return Ok(None),
                };
                let bytes = item.get_secret().await.map_err(failed)?;
                String::from_utf8(bytes).map(Some).map_err(|_| Failure::Unreachable)
            })
        }

        /// In the default collection and no other: the fallback some clients use, `session`, is gone at logout.
        fn put<'a>(&'a self, id: &'a SecretId, secret: &'a str) -> Reply<'a, ()> {
            Box::pin(async move {
                let service = connect().await?;
                let collection = service.get_default_collection().await.map_err(failed)?;
                if collection.is_locked().await.map_err(failed)? {
                    collection.unlock().await.map_err(failed)?;
                }
                let label = id.label();
                let bytes = secret.as_bytes();
                collection.create_item(&label, attributes(id), bytes, true, "text/plain").await.map_err(failed)?;
                Ok(())
            })
        }

        fn delete<'a>(&'a self, id: &'a SecretId) -> Reply<'a, ()> {
            Box::pin(async move {
                let service = connect().await?;
                let found = service.search_items(attributes(id)).await.map_err(failed)?;
                for item in found.unlocked.iter().chain(&found.locked) {
                    if item.is_locked().await.map_err(failed)? {
                        item.unlock().await.map_err(failed)?;
                    }
                    item.delete().await.map_err(failed)?;
                }
                Ok(())
            })
        }
    }
}

#[cfg(target_os = "macos")]
mod macos {
    use super::super::{Failure, Reply, SecretId, Store};
    use security_framework::base::Error;
    use security_framework::passwords::{
        PasswordOptions, delete_generic_password, generic_password, set_generic_password,
    };

    /// The item's service; its account is `SecretId::account`.
    const SERVICE: &str = "be.kletse.onward";
    const NOT_FOUND: i32 = -25300; // errSecItemNotFound
    const AUTH_FAILED: i32 = -25293; // errSecAuthFailed
    const INTERACTION_NOT_ALLOWED: i32 = -25308; // errSecInteractionNotAllowed
    const USER_CANCELED: i32 = -128; // errSecUserCanceled

    pub(super) struct Keychain;

    fn failed(error: &Error) -> Failure {
        match error.code() {
            AUTH_FAILED | INTERACTION_NOT_ALLOWED | USER_CANCELED => Failure::Locked,
            _ => Failure::Unreachable,
        }
    }

    /// The calls block, on a prompt if there is one. If the wait runs out this thread carries on alone.
    async fn on_thread<T: Send + 'static>(
        call: impl FnOnce() -> Result<T, Failure> + Send + 'static,
    ) -> Result<T, Failure> {
        tokio::task::spawn_blocking(call).await.unwrap_or(Err(Failure::Unreachable))
    }

    impl Store for Keychain {
        fn get<'a>(&'a self, id: &'a SecretId) -> Reply<'a, Option<String>> {
            let account = id.account();
            Box::pin(on_thread(move || {
                match generic_password(PasswordOptions::new_generic_password(SERVICE, &account)) {
                    Ok(bytes) => String::from_utf8(bytes).map(Some).map_err(|_| Failure::Unreachable),
                    Err(e) if e.code() == NOT_FOUND => Ok(None),
                    Err(e) => Err(failed(&e)),
                }
            }))
        }

        fn put<'a>(&'a self, id: &'a SecretId, secret: &'a str) -> Reply<'a, ()> {
            let (account, secret) = (id.account(), secret.to_string());
            Box::pin(on_thread(move || {
                set_generic_password(SERVICE, &account, secret.as_bytes()).map_err(|e| failed(&e))
            }))
        }

        fn delete<'a>(&'a self, id: &'a SecretId) -> Reply<'a, ()> {
            let account = id.account();
            Box::pin(on_thread(move || match delete_generic_password(SERVICE, &account) {
                Err(e) if e.code() != NOT_FOUND => Err(failed(&e)),
                _ => Ok(()),
            }))
        }
    }
}
