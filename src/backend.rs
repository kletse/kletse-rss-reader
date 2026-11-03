//! Runs the app's server effects: Miniflux, Readeck, the config file and the browser. The answer to each comes
//! back as an `Outcome`. With `KLETSE_RSS_DEMO=1` it serves the built-in sample data and never touches a server.

use crate::app::{Effect, Outcome, SettingsValues, Status};
use crate::config;
use crate::demo;
use crate::error::Error;
use crate::miniflux::{self, EntryPage, EntryStatus, Selection};
use crate::readeck;
use std::sync::RwLock;
use std::time::Duration;

pub struct Backend {
    /// The config can change at runtime (onboarding / settings), hence the lock.
    config: RwLock<config::Config>,
    /// What was wrong with the config file at startup, until a save replaces it.
    problem: RwLock<Option<String>>,
    http: reqwest::Client,
    demo: bool,
}

impl std::fmt::Debug for Backend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Backend { .. }")
    }
}

impl Backend {
    pub fn new(demo: bool) -> Self {
        let http = reqwest::Client::builder()
            .user_agent(concat!("kletse-rss/", env!("CARGO_PKG_VERSION")))
            .timeout(Duration::from_secs(30))
            .build()
            .expect("the HTTP client only needs built-in TLS roots");
        let (config, problem) = match config::read() {
            Ok(config) => (config, None),
            Err(e) => {
                eprintln!("kletse-rss: {}", e.message());
                (config::Config::default(), Some(e.message()))
            }
        };
        Self { config: RwLock::new(config), problem: RwLock::new(problem), http, demo }
    }

    fn config(&self) -> config::Config {
        // A poisoned lock only means another task panicked mid-write; the config is still plain data.
        self.config.read().unwrap_or_else(|e| e.into_inner()).clone()
    }

    fn set_config(&self, cfg: config::Config) {
        *self.config.write().unwrap_or_else(|e| e.into_inner()) = cfg;
        *self.problem.write().unwrap_or_else(|e| e.into_inner()) = None;
    }

    pub fn status(&self) -> Status {
        let cfg = self.config();
        Status { configured: self.demo || cfg.is_complete(), readeck: self.demo || cfg.readeck.is_some() }
    }

    fn settings(&self) -> SettingsValues {
        let cfg = self.config();
        let readeck = cfg.readeck.unwrap_or_default();
        SettingsValues {
            miniflux_url: cfg.miniflux.url,
            miniflux_api_key: cfg.miniflux.api_key,
            readeck_url: readeck.url,
            readeck_token: readeck.token,
        }
    }

    fn miniflux(&self) -> Result<miniflux::Client, Error> {
        let cfg = self.config();
        if !cfg.is_complete() {
            return Err(Error::Setup("Miniflux is not set up yet".into()));
        }
        Ok(miniflux::Client::new(self.http.clone(), &cfg.miniflux))
    }

    /// Validates the settings against the real servers, then saves and applies them.
    async fn save_settings(&self, values: SettingsValues) -> Result<Status, Error> {
        let has_readeck_url = !values.readeck_url.trim().is_empty();
        let has_readeck_token = !values.readeck_token.trim().is_empty();
        if has_readeck_url != has_readeck_token {
            return Err(Error::Setup("Fill in both Readeck fields, or leave both empty.".into()));
        }
        let cfg = config::Config {
            miniflux: config::Miniflux { url: values.miniflux_url, api_key: values.miniflux_api_key },
            readeck: Some(config::Readeck { url: values.readeck_url, token: values.readeck_token }),
        }
        .normalized();
        cfg.validate()?;
        if !self.demo {
            miniflux::Client::new(self.http.clone(), &cfg.miniflux).check().await?;
            if let Some(readeck) = &cfg.readeck {
                readeck::check(&self.http, readeck).await?;
            }
            // Writing and flushing the file blocks; keep it off the runtime's few workers.
            let saved = cfg.clone();
            tokio::task::spawn_blocking(move || config::save(&saved)).await.map_err(|e| Error::Io {
                context: "Could not save the settings".into(),
                source: std::io::Error::other(e),
            })??;
        }
        self.set_config(cfg);
        Ok(self.status())
    }

    async fn entries(&self, sel: Selection, status: EntryStatus, past: i64, limit: usize) -> Result<EntryPage, Error> {
        if self.demo {
            return Ok(demo::entries(sel, status, past, limit));
        }
        self.miniflux()?.entries(sel, status, past, limit).await
    }

    async fn newest_unread_id(&self, sel: Selection) -> Result<i64, Error> {
        if self.demo {
            return Ok(demo::newest_unread_id(sel));
        }
        self.miniflux()?.newest_unread_id(sel).await
    }

    /// Runs a server effect. The window's own effects (scrolling, prefs, quitting) aren't for here.
    pub async fn run(&self, effect: Effect) -> Option<Outcome> {
        let msg = |e: Error| e.message();
        Some(match effect {
            Effect::OpenSelection { load, selection, mode, request } => {
                let status = mode.status();
                let page = self.entries(selection, status, request.past, request.limit);
                let result = if status == EntryStatus::Unread {
                    tokio::try_join!(page, self.newest_unread_id(selection))
                } else {
                    page.await.map(|page| (page, 0))
                };
                Outcome::Opened { load, result: result.map_err(msg) }
            }
            Effect::LoadPage { load, selection, mode, request, arrivals } => {
                let result = self.entries(selection, mode.status(), request.past, request.limit).await;
                Outcome::Page { load, arrivals, result: result.map_err(msg) }
            }
            Effect::SetStatus { ids, status, revert, announce } => {
                let result = if self.demo { Ok(()) } else { self.set_status(&ids, status).await };
                Outcome::StatusSet { status, revert, announce, result: result.map_err(msg) }
            }
            Effect::FetchContent { index, id } => {
                let result = if self.demo {
                    Ok(demo::content(id))
                } else {
                    match self.miniflux() {
                        Ok(client) => client.fetch_content(id).await,
                        Err(e) => Err(e),
                    }
                };
                Outcome::ContentFetched { index, id, result: result.map_err(msg) }
            }
            Effect::FeedCounts => {
                let result = if self.demo {
                    Ok(demo::feed_counts())
                } else {
                    match self.miniflux() {
                        Ok(client) => client.feed_counts().await,
                        Err(e) => Err(e),
                    }
                };
                Outcome::FeedCounts(result.map_err(msg))
            }
            Effect::RefreshFeeds => {
                let result = if self.demo {
                    Ok(())
                } else {
                    match self.miniflux() {
                        Ok(client) => client.refresh_all().await,
                        Err(e) => Err(e),
                    }
                };
                Outcome::Refreshed(result.map_err(msg))
            }
            Effect::ReloadPickerAfter { millis } => {
                tokio::time::sleep(Duration::from_millis(millis)).await;
                Outcome::PickerReloadDue
            }
            Effect::SaveToReadeck { url } => {
                let result = if self.demo {
                    Ok(())
                } else {
                    match self.config().readeck {
                        Some(cfg) => readeck::save(&self.http, &cfg, &url).await,
                        None => Err(Error::Setup("Readeck is not set up. Add it in Settings.".into())),
                    }
                };
                Outcome::SavedToReadeck(result.map_err(msg))
            }
            Effect::OpenInBrowser { url, then } => {
                Outcome::OpenedInBrowser { then, result: open_in_browser(&url).map_err(msg) }
            }
            Effect::CountUnread { load, selection, up_to, later } => {
                let result = if self.demo {
                    Ok(demo::unread_count(selection, up_to))
                } else {
                    match self.miniflux() {
                        Ok(client) => client.unread_count(selection, up_to).await,
                        Err(e) => Err(e),
                    }
                };
                Outcome::UnreadCounted { load, selection, up_to, later, result: result.map_err(msg) }
            }
            Effect::MarkAllRead { load, selection, up_to, later } => {
                let result = if self.demo {
                    demo::mark_read_up_to(selection, up_to);
                    Ok(())
                } else {
                    match self.miniflux() {
                        Ok(client) => tokio::try_join!(
                            client.mark_read_up_to(selection, up_to),
                            client.set_status(&later, EntryStatus::Read)
                        )
                        .map(|_| ()),
                        Err(e) => Err(e),
                    }
                };
                Outcome::MarkedAllRead { load, result: result.map_err(msg) }
            }
            Effect::LoadSettings => Outcome::SettingsLoaded {
                values: self.settings(),
                problem: self.problem.read().unwrap_or_else(|e| e.into_inner()).clone(),
            },
            Effect::SaveSettings(values) => Outcome::SettingsSaved(self.save_settings(values.0).await.map_err(msg)),
            Effect::SavePrefs(_) | Effect::ScrollPage { .. } | Effect::Quit => return None,
        })
    }

    async fn set_status(&self, ids: &[i64], status: EntryStatus) -> Result<(), Error> {
        self.miniflux()?.set_status(ids, status).await
    }
}

/// Only web and mail links are handed to the system; anything else (file:, javascript:, …) is refused.
fn is_openable(url: &str) -> bool {
    reqwest::Url::parse(url).is_ok_and(|u| match u.scheme() {
        "http" | "https" => u.host_str().is_some_and(|h| !h.is_empty()),
        "mailto" => !u.path().is_empty(),
        _ => false,
    })
}

/// The system's "open this URL" command: `open` on macOS, `xdg-open` elsewhere
/// (on Omarchy that resolves to omarchy-launch-browser).
const OPENER: &str = if cfg!(target_os = "macos") { "open" } else { "xdg-open" };

/// Opens a link in the default browser.
/// Firefox, Chromium and Safari hand the URL to an already running window as a new tab.
pub fn open_in_browser(url: &str) -> Result<(), Error> {
    if !is_openable(url) {
        return Err(Error::Refused(format!("Refusing to open {url}")));
    }
    let mut child = std::process::Command::new(OPENER)
        .arg(url)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|source| Error::Io { context: format!("Could not start {OPENER}"), source })?;
    // Reap the child so it does not linger as a zombie.
    std::thread::spawn(move || child.wait());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::SettingsValuesBox;

    #[test]
    fn only_web_and_mail_links_are_openable() {
        for ok in ["https://example.com/a?b=c#d", "http://localhost:8080", "mailto:someone@example.com"] {
            assert!(is_openable(ok), "{ok}");
        }
        for bad in [
            "file:///etc/passwd",
            "javascript:alert(1)",
            "data:text/html,hi",
            "ftp://example.com",
            "https://",
            "mailto:",
            "example.com",
            "",
            "-x",
        ] {
            assert!(!is_openable(bad), "{bad}");
        }
    }

    fn values(url: &str, readeck_url: &str, readeck_token: &str) -> SettingsValues {
        SettingsValues {
            miniflux_url: url.into(),
            miniflux_api_key: "key".into(),
            readeck_url: readeck_url.into(),
            readeck_token: readeck_token.into(),
        }
    }

    async fn save(values: SettingsValues) -> Result<Status, String> {
        let backend = Backend::new(true);
        match backend.run(Effect::SaveSettings(SettingsValuesBox(values))).await {
            Some(Outcome::SettingsSaved(result)) => result,
            _ => Err("no answer".into()),
        }
    }

    #[tokio::test]
    async fn settings_need_both_readeck_fields_or_neither() {
        let err = save(values("https://m.example.com", "https://r.example.com", " ")).await.err();
        assert_eq!(err.as_deref(), Some("Fill in both Readeck fields, or leave both empty."));
        assert!(save(values("https://m.example.com", "", "")).await.is_ok());
    }

    #[tokio::test]
    async fn settings_need_a_real_miniflux_url() {
        let err = save(values("miniflux.example.com", "", "")).await.err();
        assert_eq!(err.as_deref(), Some("miniflux.example.com is not a valid URL"));
    }

    #[tokio::test]
    async fn demo_mode_serves_the_sample_data() {
        let backend = Backend::new(true);
        assert_eq!(backend.status(), Status { configured: true, readeck: true });
        let Some(Outcome::FeedCounts(Ok(counts))) = backend.run(Effect::FeedCounts).await else {
            panic!("no feed counts");
        };
        assert!(!counts.is_empty());
        assert!(backend.run(Effect::Quit).await.is_none());
    }
}
