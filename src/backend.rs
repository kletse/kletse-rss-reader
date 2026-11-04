//! Runs the app's server effects: the feed server (Miniflux or a Google Reader API server), Readeck, the config
//! file and the browser. The answer to each comes
//! back as an `Outcome`. With `ONWARD_DEMO=1` it serves the built-in sample data and never touches a server.

use crate::app::{Effect, Outcome, Saved, SettingsValues, Status};
use crate::config;
use crate::demo;
use crate::error::{Error, RedirectRefused};
use crate::feeds::{Capabilities, EntryPage, EntryStatus, Selection};
use crate::greader;
use crate::miniflux;
use crate::paging::PageRequest;
use crate::readeck;
use crate::server::Server;
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

/// As many as reqwest's own policy follows.
const MAX_REDIRECTS: usize = 10;

pub struct Backend {
    /// The config can change at runtime (onboarding / settings), hence the lock.
    config: RwLock<config::Config>,
    /// What was wrong with the config file at startup, until a save replaces it.
    problem: RwLock<Option<String>>,
    /// The client for the configured server, made on first use and kept: a Google Reader client remembers its
    /// login and which stream each feed id stands for. A new config drops it.
    server: Mutex<Option<Arc<Server>>>,
    http: reqwest::Client,
    /// For images, wherever they live: nothing we sign in with goes through it.
    #[cfg(feature = "tui")]
    images: reqwest::Client,
    demo: bool,
    /// The demo's starred articles, which `*` changes in memory.
    stars: demo::Stars,
}

/// Settings that passed the checks, ready to save.
struct Checked {
    cfg: config::Config,
    /// Who the feed server says this is; demo mode asks no server.
    username: Option<String>,
    /// Why the Readeck in `cfg` isn't the one in the form.
    readeck_problem: Option<String>,
}

fn client() -> reqwest::ClientBuilder {
    reqwest::Client::builder()
        .user_agent(concat!("onward/", env!("CARGO_PKG_VERSION")))
        .timeout(Duration::from_secs(30))
}

/// The client for the servers we sign in to. A login gate in front of one answers with a redirect to itself, and
/// that mustn't get what we send: reqwest keeps `Authorization` from another host, but not `X-Auth-Token`, and a 307
/// sends a POST's body again, which for a Google Reader login is the password. So it only follows redirects that
/// stay on the server. Anything else goes through `client()`, where redirects are free: images live on CDNs.
pub(crate) fn api_client() -> reqwest::Client {
    let policy = reqwest::redirect::Policy::custom(|attempt| {
        let start = attempt.previous().first();
        if attempt.previous().len() >= MAX_REDIRECTS {
            attempt.error("too many redirects")
        } else if start.is_none_or(|start| stays_on_server(start, attempt.url())) {
            attempt.follow()
        } else {
            let to = attempt.url().origin().ascii_serialization();
            attempt.error(RedirectRefused { to })
        }
    });
    client().redirect(policy).build().expect("the HTTP client only needs built-in TLS roots")
}

/// The same host and port and scheme, or the same host going from http to https, on whichever port. Back from https
/// to http is out: it would send the secret in the clear.
fn stays_on_server(start: &reqwest::Url, next: &reqwest::Url) -> bool {
    let same_origin = start.scheme() == next.scheme() && start.port_or_known_default() == next.port_or_known_default();
    let secured = start.scheme() == "http" && next.scheme() == "https";
    start.host_str() == next.host_str() && (same_origin || secured)
}

impl std::fmt::Debug for Backend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Backend { .. }")
    }
}

impl Backend {
    pub fn new(demo: bool) -> Self {
        let http = api_client();
        let (config, problem) = match config::read() {
            Ok(config) => (config, None),
            Err(e) => {
                eprintln!("onward: {}", e.message());
                (config::Config::default(), Some(e.message()))
            }
        };
        Self {
            config: RwLock::new(config),
            problem: RwLock::new(problem),
            server: Mutex::new(None),
            http,
            #[cfg(feature = "tui")]
            images: client().build().expect("the HTTP client only needs built-in TLS roots"),
            demo,
            stars: demo::Stars::default(),
        }
    }

    fn config(&self) -> config::Config {
        // A poisoned lock only means another task panicked mid-write; the config is still plain data.
        self.config.read().unwrap_or_else(|e| e.into_inner()).clone()
    }

    fn set_config(&self, cfg: config::Config) {
        *self.config.write().unwrap_or_else(|e| e.into_inner()) = cfg;
        *self.problem.write().unwrap_or_else(|e| e.into_inner()) = None;
        *self.server.lock().unwrap_or_else(|e| e.into_inner()) = None;
    }

    /// `ONWARD_DEMO=1`: sample data, and nothing is saved, prefs included.
    pub fn demo(&self) -> bool {
        self.demo
    }

    pub fn status(&self) -> Status {
        let cfg = self.config();
        Status {
            configured: self.demo || cfg.is_complete(),
            readeck: self.demo || cfg.readeck.is_some(),
            capabilities: if self.demo { Capabilities::EVERYTHING } else { cfg.server().capabilities() },
        }
    }

    fn settings(&self) -> SettingsValues {
        let cfg = self.config();
        let server = cfg.server();
        let readeck = cfg.readeck.unwrap_or_default();
        let greader = cfg.greader.unwrap_or_default();
        SettingsValues {
            server,
            miniflux_url: cfg.miniflux.url,
            miniflux_api_key: cfg.miniflux.api_key,
            greader_url: greader.url,
            greader_username: greader.username,
            greader_password: greader.password,
            readeck_url: readeck.url,
            readeck_token: readeck.token,
        }
    }

    /// A client for the server in `cfg`.
    fn client_for(&self, cfg: &config::Config) -> Result<Server, Error> {
        Ok(match cfg.server() {
            config::Server::Miniflux => Server::Miniflux(miniflux::Client::new(self.http.clone(), &cfg.miniflux)),
            config::Server::GReader => {
                let greader = cfg.greader.as_ref().ok_or_else(|| Error::Setup(config::GREADER_INCOMPLETE.into()))?;
                Server::GReader(greader::Client::new(self.http.clone(), greader))
            }
        })
    }

    /// The configured server's client, the same one each time until the config changes.
    fn server(&self) -> Result<Arc<Server>, Error> {
        let cfg = self.config();
        if !cfg.is_complete() {
            return Err(Error::Setup("The feed server is not set up yet".into()));
        }
        let mut kept = self.server.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(server) = &*kept {
            return Ok(Arc::clone(server));
        }
        let server = Arc::new(self.client_for(&cfg)?);
        *kept = Some(Arc::clone(&server));
        Ok(server)
    }

    /// Validates the settings against the real servers, then saves and applies them. A feed server that fails
    /// stops everything; Readeck values that fail don't: see `check_readeck`.
    async fn save_settings(&self, values: SettingsValues) -> Result<Saved, Error> {
        let Checked { cfg, username, readeck_problem } = self.check_settings(values).await?;
        if !self.demo {
            // Writing and flushing the file blocks; keep it off the runtime's few workers.
            let saved = cfg.clone();
            tokio::task::spawn_blocking(move || config::save(&saved)).await.map_err(|e| Error::Io {
                context: "Could not save the settings".into(),
                source: std::io::Error::other(e),
            })??;
        }
        self.set_config(cfg);
        Ok(Saved { status: self.status(), username, readeck_problem })
    }

    /// What to save, once the servers have had their say. The other kind of server's fields are kept when they're
    /// complete, so switching back needs no typing; otherwise they're dropped.
    async fn check_settings(&self, values: SettingsValues) -> Result<Checked, Error> {
        let half_filled = values.readeck_url.trim().is_empty() != values.readeck_token.trim().is_empty();
        let mut cfg = config::Config {
            server: Some(values.server),
            miniflux: config::Miniflux { url: values.miniflux_url, api_key: values.miniflux_api_key },
            greader: Some(config::GReader {
                url: values.greader_url,
                username: values.greader_username,
                password: values.greader_password,
            }),
            readeck: Some(config::Readeck { url: values.readeck_url, token: values.readeck_token }),
        }
        .normalized();
        match values.server {
            config::Server::Miniflux => {
                cfg.greader = cfg.greader.filter(|g| g.is_complete() && config::validate_url(&g.url).is_ok());
            }
            config::Server::GReader => {
                if !(cfg.miniflux.is_complete() && config::validate_url(&cfg.miniflux.url).is_ok()) {
                    cfg.miniflux = config::Miniflux::default();
                }
            }
        }
        let readeck = cfg.readeck.take();
        cfg.validate()?;
        let username = if self.demo { None } else { Some(self.client_for(&cfg)?.check().await?) };
        let (readeck, readeck_problem) = match self.check_readeck(readeck, half_filled).await {
            Ok(readeck) => (readeck, None),
            Err(e) => (self.config().readeck, Some(e.message())),
        };
        cfg.readeck = readeck;
        Ok(Checked { cfg, username, readeck_problem })
    }

    /// The Readeck to save, if its values hold up. They're optional, so unlike the feed server's they never stop a
    /// save: when they fail, the Readeck already saved stays, and the message says why.
    async fn check_readeck(
        &self,
        readeck: Option<config::Readeck>,
        half_filled: bool,
    ) -> Result<Option<config::Readeck>, Error> {
        if half_filled {
            return Err(Error::Setup("Fill in both Readeck fields, or leave both empty.".into()));
        }
        if let Some(readeck) = &readeck {
            config::validate_url(&readeck.url)?;
            if !self.demo {
                readeck::check(&self.http, readeck).await?;
            }
        }
        Ok(readeck)
    }

    async fn entries(&self, sel: Selection, status: EntryStatus, request: PageRequest) -> Result<EntryPage, Error> {
        let PageRequest { past, limit, order } = request;
        if self.demo {
            return Ok(demo::entries(sel, status, order, past, limit, &self.stars));
        }
        self.server()?.entries(sel, status, order, past, limit).await
    }

    async fn search(&self, query: &str, offset: usize, limit: usize) -> Result<EntryPage, Error> {
        if self.demo {
            return Ok(demo::search(query, offset, limit, &self.stars));
        }
        self.server()?.search(query, offset, limit).await
    }

    /// The selection's newest unread id, and how many are unread up to it.
    async fn count_row(&self, sel: Selection) -> Result<(i64, i64), Error> {
        let newest = self.newest_unread_id(sel).await?;
        let count = if self.demo {
            demo::unread_count(sel, newest, &self.stars)
        } else {
            self.server()?.unread_count(sel, newest).await?
        };
        Ok((newest, count))
    }

    async fn newest_unread_id(&self, sel: Selection) -> Result<i64, Error> {
        if self.demo {
            return Ok(demo::newest_unread_id(sel, &self.stars));
        }
        self.server()?.newest_unread_id(sel).await
    }

    /// The image at `url`, for a window that fetches its own images: the terminal's viewer. Demo mode has
    /// `data:` images only, and stays off the network.
    #[cfg(feature = "tui")]
    pub async fn fetch_image(&self, url: &str) -> Result<Vec<u8>, Error> {
        if self.demo || !reqwest::Url::parse(url).is_ok_and(|u| matches!(u.scheme(), "http" | "https")) {
            return Err(Error::Refused(format!("Not loading {url}")));
        }
        let failed = |source| Error::Unexpected { context: "The image didn't load".into(), source: Some(source) };
        let response = self.images.get(url).send().await.and_then(|r| r.error_for_status()).map_err(failed)?;
        Ok(response.bytes().await.map_err(failed)?.to_vec())
    }

    /// Runs a server effect. The window's own effects (scrolling, prefs, quitting) aren't for here.
    pub async fn run(&self, effect: Effect) -> Option<Outcome> {
        let msg = |e: Error| e.message();
        Some(match effect {
            Effect::OpenSelection { load, selection, mode, request } => {
                let status = mode.status();
                let page = self.entries(selection, status, request);
                // The starred list can't be marked read all at once, so it needs no newest unread id.
                let result = if status == EntryStatus::Unread && selection != Selection::Starred {
                    tokio::try_join!(page, self.newest_unread_id(selection))
                } else {
                    page.await.map(|page| (page, 0))
                };
                Outcome::Opened { load, result: result.map_err(msg) }
            }
            Effect::LoadPage { load, selection, mode, request, arrivals } => {
                let result = self.entries(selection, mode.status(), request).await;
                Outcome::Page { load, arrivals, result: result.map_err(msg) }
            }
            Effect::Search { load, query, offset, limit, first } => {
                let result = self.search(&query, offset, limit).await.map_err(msg);
                if first {
                    Outcome::Opened { load, result: result.map(|page| (page, 0)) }
                } else {
                    Outcome::Page { load, arrivals: false, result }
                }
            }
            Effect::SetStatus { ids, status, revert, announce } => {
                let result = if self.demo { Ok(()) } else { self.set_status(&ids, status).await };
                Outcome::StatusSet { status, revert, announce, result: result.map_err(msg) }
            }
            Effect::SetStarred { index, id, starred } => {
                let result = if self.demo {
                    self.stars.set(id, starred);
                    Ok(())
                } else {
                    match self.server() {
                        Ok(client) => client.set_starred(id, starred).await,
                        Err(e) => Err(e),
                    }
                };
                Outcome::StarredSet { index, id, starred, result: result.map_err(msg) }
            }
            Effect::FetchContent { index, id } => {
                let result = if self.demo {
                    Ok(demo::content(id))
                } else {
                    match self.server() {
                        Ok(client) => client.fetch_content(id).await,
                        Err(e) => Err(e),
                    }
                };
                Outcome::ContentFetched { index, id, result: result.map_err(msg) }
            }
            Effect::FeedCounts => {
                let result = if self.demo {
                    Ok(demo::feed_counts(&self.stars))
                } else {
                    match self.server() {
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
                    match self.server() {
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
            Effect::SaveToReadeck { index, id, url } => {
                let result = if self.demo {
                    Ok(())
                } else {
                    match self.config().readeck {
                        Some(cfg) => readeck::save(&self.http, &cfg, &url).await,
                        None => Err(Error::Setup("Readeck is not set up. Add it in Settings.".into())),
                    }
                };
                Outcome::SavedToReadeck { index, id, result: result.map_err(msg) }
            }
            Effect::OpenInBrowser { url, then } => {
                Outcome::OpenedInBrowser { then, result: open_in_browser(&url).map_err(msg) }
            }
            Effect::CountUnread { load, selection, up_to, later } => {
                let result = if self.demo {
                    Ok(demo::unread_count(selection, up_to, &self.stars))
                } else {
                    match self.server() {
                        Ok(client) => client.unread_count(selection, up_to).await,
                        Err(e) => Err(e),
                    }
                };
                Outcome::UnreadCounted { load, selection, up_to, later, result: result.map_err(msg) }
            }
            Effect::MarkAllRead { load, selection, up_to, later } => {
                let result = if self.demo {
                    demo::mark_read_up_to(selection, up_to, &self.stars);
                    Ok(())
                } else {
                    match self.server() {
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
            Effect::CountRow { selection, label } => {
                Outcome::RowCounted { selection, label, result: self.count_row(selection).await.map_err(msg) }
            }
            Effect::MarkRowRead { selection, label, up_to } => {
                let result = if self.demo {
                    demo::mark_read_up_to(selection, up_to, &self.stars);
                    Ok(())
                } else {
                    match self.server() {
                        Ok(client) => client.mark_read_up_to(selection, up_to).await.map(|_| ()),
                        Err(e) => Err(e),
                    }
                };
                Outcome::RowMarkedRead { selection, label, up_to, result: result.map_err(msg) }
            }
            Effect::LoadSettings => Outcome::SettingsLoaded {
                values: self.settings(),
                problem: self.problem.read().unwrap_or_else(|e| e.into_inner()).clone(),
            },
            Effect::SaveSettings(values) => Outcome::SettingsSaved(self.save_settings(values.0).await.map_err(msg)),
            Effect::SavePrefs(_) | Effect::ScrollPage { .. } | Effect::Quit | Effect::ShowSettings => return None,
        })
    }

    async fn set_status(&self, ids: &[i64], status: EntryStatus) -> Result<(), Error> {
        self.server()?.set_status(ids, status).await
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
#[allow(clippy::unwrap_used)]
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

    #[cfg(feature = "tui")]
    #[tokio::test]
    async fn images_come_over_http_and_only_http() {
        use wiremock::matchers::path;
        use wiremock::{Mock, MockServer, ResponseTemplate};
        let server = MockServer::start().await;
        Mock::given(path("/a.png"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(*b"png"))
            .mount(&server)
            .await;
        let backend = Backend::new(false);
        assert_eq!(backend.fetch_image(&format!("{}/a.png", server.uri())).await.unwrap_or_default(), b"png");
        assert!(backend.fetch_image(&format!("{}/missing.png", server.uri())).await.is_err());
        assert!(backend.fetch_image("file:///etc/passwd").await.is_err());
    }

    #[cfg(feature = "tui")]
    #[tokio::test]
    async fn images_still_follow_a_redirect_to_another_host() {
        use wiremock::matchers::path;
        use wiremock::{Mock, MockServer, ResponseTemplate};
        let cdn = MockServer::start().await;
        Mock::given(path("/a.png")).respond_with(ResponseTemplate::new(200).set_body_bytes(*b"png")).mount(&cdn).await;
        let site = MockServer::start().await;
        let to = format!("{}/a.png", cdn.uri().replace("127.0.0.1", "localhost"));
        Mock::given(path("/image"))
            .respond_with(ResponseTemplate::new(302).insert_header("Location", to.as_str()))
            .mount(&site)
            .await;
        let image = Backend::new(false).fetch_image(&format!("{}/image", site.uri())).await;
        assert_eq!(image.unwrap_or_default(), b"png");
    }

    #[test]
    fn only_redirects_that_stay_on_the_server_are_followed() {
        let url = |s: &str| reqwest::Url::parse(s).unwrap();
        for (start, next) in [
            ("https://rss.example.com/v1/me", "https://rss.example.com/v1/me/"),
            ("https://rss.example.com/v1/me", "https://rss.example.com:443/other"),
            ("http://rss.example.com/v1/me", "https://rss.example.com/v1/me"),
            ("http://rss.example.com:8080/v1/me", "https://rss.example.com:8443/v1/me"),
            ("http://localhost:8080/a", "http://localhost:8080/b"),
        ] {
            assert!(stays_on_server(&url(start), &url(next)), "{start} to {next}");
        }
        for (start, next) in [
            ("https://rss.example.com/", "https://auth.example.com/"),
            ("https://example.com/", "https://www.example.com/"),
            ("https://example.com/", "https://example.com.evil.test/"),
            ("https://rss.example.com/", "http://rss.example.com/"),
            ("https://rss.example.com/", "https://rss.example.com:8443/"),
            ("http://127.0.0.1:8080/", "http://127.0.0.1:9090/"),
            ("http://127.0.0.1:8080/", "http://localhost:8080/"),
        ] {
            assert!(!stays_on_server(&url(start), &url(next)), "{start} to {next}");
        }
    }

    fn values(url: &str, readeck_url: &str, readeck_token: &str) -> SettingsValues {
        SettingsValues {
            miniflux_url: url.into(),
            miniflux_api_key: "key".into(),
            readeck_url: readeck_url.into(),
            readeck_token: readeck_token.into(),
            ..SettingsValues::default()
        }
    }

    fn greader_values(url: &str, password: &str) -> SettingsValues {
        SettingsValues {
            server: config::Server::GReader,
            greader_url: url.into(),
            greader_username: "ward".into(),
            greader_password: password.into(),
            ..SettingsValues::default()
        }
    }

    async fn save(values: SettingsValues) -> Result<Saved, String> {
        let backend = Backend::new(true);
        match backend.run(Effect::SaveSettings(SettingsValuesBox(values))).await {
            Some(Outcome::SettingsSaved(result)) => result,
            _ => Err("no answer".into()),
        }
    }

    #[tokio::test]
    async fn half_a_readeck_setup_is_reported_and_doesnt_stop_the_feed_server() {
        let saved = save(values("https://m.example.com", "https://r.example.com", " ")).await.unwrap();
        assert_eq!(saved.readeck_problem.as_deref(), Some("Fill in both Readeck fields, or leave both empty."));
        assert_eq!(saved.username, None, "demo mode asks no server");
        let saved = save(values("https://m.example.com", " ", "tok")).await.unwrap();
        assert_eq!(saved.readeck_problem.as_deref(), Some("Fill in both Readeck fields, or leave both empty."));
        assert_eq!(save(values("https://m.example.com", "", "")).await.unwrap().readeck_problem, None);
    }

    /// A backend that really checks, with this Readeck saved. Tests go through `check_settings`, which writes
    /// nothing: `save_settings` would replace the config of whoever runs them.
    fn live_backend(readeck: Option<config::Readeck>) -> Backend {
        let backend = Backend { demo: false, ..Backend::new(true) };
        backend.set_config(config::Config { readeck, ..config::Config::default() });
        backend
    }

    fn old_readeck() -> config::Readeck {
        config::Readeck { url: "https://old.example.com".into(), token: "old".into() }
    }

    /// A Miniflux that lets anyone in, and a Readeck that answers `readeck` to its profile check.
    async fn servers(readeck: u16) -> wiremock::MockServer {
        use wiremock::matchers::path;
        use wiremock::{Mock, MockServer, ResponseTemplate};
        let server = MockServer::start().await;
        let me = ResponseTemplate::new(200).set_body_json(serde_json::json!({ "username": "ward" }));
        Mock::given(path("/v1/me")).respond_with(me).mount(&server).await;
        Mock::given(path("/api/profile")).respond_with(ResponseTemplate::new(readeck)).mount(&server).await;
        server
    }

    #[tokio::test]
    async fn a_readeck_that_says_no_keeps_the_saved_one_and_still_takes_the_feed_server() {
        let server = servers(401).await;
        let backend = live_backend(Some(old_readeck()));
        let checked = backend.check_settings(values(&server.uri(), &server.uri(), "wrong")).await.unwrap();
        assert_eq!(checked.cfg.miniflux.url, server.uri());
        assert_eq!(checked.username.as_deref(), Some("ward"));
        assert_eq!(checked.cfg.readeck, Some(old_readeck()));
        assert!(checked.readeck_problem.unwrap().starts_with("Readeck rejected the token"));
    }

    #[tokio::test]
    async fn a_readeck_address_that_isnt_one_keeps_the_saved_one_and_still_takes_the_feed_server() {
        let server = servers(200).await;
        let backend = live_backend(Some(old_readeck()));
        let checked = backend.check_settings(values(&server.uri(), "readeck.example.com", "tok")).await.unwrap();
        assert_eq!(checked.cfg.miniflux.url, server.uri());
        assert_eq!(checked.cfg.readeck, Some(old_readeck()));
        assert_eq!(checked.readeck_problem.as_deref(), Some("readeck.example.com is not a valid URL"));
    }

    #[tokio::test]
    async fn a_readeck_that_cant_be_reached_keeps_the_saved_one() {
        let server = servers(200).await;
        let backend = live_backend(Some(old_readeck()));
        let checked = backend.check_settings(values(&server.uri(), "http://127.0.0.1:1", "tok")).await.unwrap();
        assert_eq!(checked.cfg.readeck, Some(old_readeck()));
        assert!(checked.readeck_problem.unwrap().starts_with("Could not reach Readeck at http://127.0.0.1:1"));
    }

    #[tokio::test]
    async fn with_no_readeck_saved_a_bad_one_leaves_it_unset() {
        let server = servers(401).await;
        let backend = live_backend(None);
        let checked = backend.check_settings(values(&server.uri(), &server.uri(), "wrong")).await.unwrap();
        assert_eq!(checked.cfg.readeck, None);
        assert!(checked.readeck_problem.is_some());
    }

    #[tokio::test]
    async fn good_readeck_values_replace_the_saved_ones_and_empty_ones_clear_them() {
        let server = servers(200).await;
        let backend = live_backend(Some(old_readeck()));
        let checked = backend.check_settings(values(&server.uri(), &server.uri(), "new")).await.unwrap();
        assert_eq!(checked.cfg.readeck, Some(config::Readeck { url: server.uri(), token: "new".into() }));
        assert_eq!(checked.readeck_problem, None);
        let checked = backend.check_settings(values(&server.uri(), "", "")).await.unwrap();
        assert_eq!((checked.cfg.readeck, checked.readeck_problem), (None, None));
    }

    #[tokio::test]
    async fn a_bad_feed_server_still_fails_whatever_readeck_says() {
        use wiremock::matchers::path;
        use wiremock::{Mock, MockServer, ResponseTemplate};
        let server = MockServer::start().await;
        Mock::given(path("/v1/me")).respond_with(ResponseTemplate::new(401)).mount(&server).await;
        let backend = live_backend(None);
        let err = backend.check_settings(values(&server.uri(), &server.uri(), "tok")).await.err().unwrap();
        assert_eq!(err.message(), "Miniflux rejected the API key");
        let err = backend.check_settings(values("miniflux.example.com", "half", "")).await.err().unwrap();
        assert_eq!(err.message(), "miniflux.example.com is not a valid URL");
    }

    #[tokio::test]
    async fn settings_need_a_real_miniflux_url() {
        let err = save(values("miniflux.example.com", "", "")).await.err();
        assert_eq!(err.as_deref(), Some("miniflux.example.com is not a valid URL"));
    }

    #[tokio::test]
    async fn a_google_reader_server_needs_all_three_fields_and_a_real_url() {
        let err = save(greader_values("https://rss.example.com/api/greader.php", "")).await.err();
        assert_eq!(err.as_deref(), Some("The Google Reader API server needs a URL, a username and a password."));
        let err = save(greader_values("rss.example.com", "pw")).await.err();
        assert_eq!(err.as_deref(), Some("rss.example.com is not a valid URL"));
        // Half a Miniflux setup next to it, left over in the hidden fields, doesn't stand in the way.
        let mut values = greader_values("https://rss.example.com/api/greader.php", "pw");
        values.miniflux_url = "https://m.example.com".into();
        assert!(save(values).await.is_ok());
    }

    #[tokio::test]
    async fn saving_checks_the_chosen_server_and_says_what_it_can_do() {
        use wiremock::matchers::path;
        use wiremock::{Mock, MockServer, ResponseTemplate};
        let server = MockServer::start().await;
        Mock::given(path("/accounts/ClientLogin"))
            .respond_with(ResponseTemplate::new(401))
            .expect(1)
            .mount(&server)
            .await;
        let backend = Backend { demo: false, ..Backend::new(true) };
        let err = backend.save_settings(greader_values(&server.uri(), "wrong")).await.unwrap_err();
        assert_eq!(err.message(), greader::LOGIN_REJECTED);
        let cfg = config::Config {
            server: Some(config::Server::GReader),
            greader: Some(config::GReader { url: server.uri(), username: "u".into(), password: "p".into() }),
            ..config::Config::default()
        };
        backend.set_config(cfg);
        assert_eq!(
            backend.status(),
            Status { configured: true, readeck: false, capabilities: config::Server::GReader.capabilities() }
        );
        assert_eq!(backend.settings().server, config::Server::GReader);
        assert!(matches!(*backend.server().unwrap(), Server::GReader(_)));
        let same = Arc::ptr_eq(&backend.server().unwrap(), &backend.server().unwrap());
        assert!(same, "the client, its login and its streams are kept");
    }

    #[test]
    fn a_google_reader_config_without_its_section_gets_no_miniflux_client() {
        let backend = Backend::new(true);
        let cfg = config::Config { server: Some(config::Server::GReader), ..config::Config::default() };
        let err = backend.client_for(&cfg).unwrap_err();
        assert_eq!(err.message(), config::GREADER_INCOMPLETE);
    }

    #[tokio::test]
    async fn demo_mode_serves_the_sample_data() {
        let backend = Backend::new(true);
        assert_eq!(
            backend.status(),
            Status { configured: true, readeck: true, capabilities: Capabilities::EVERYTHING }
        );
        let Some(Outcome::FeedCounts(Ok(counts))) = backend.run(Effect::FeedCounts).await else {
            panic!("no feed counts");
        };
        assert!(!counts.feeds.is_empty());
        assert_eq!(counts.starred, Some(2));
        assert!(backend.run(Effect::Quit).await.is_none());
    }

    #[tokio::test]
    async fn demo_mode_stars_in_memory() {
        let backend = Backend::new(true);
        let star = Effect::SetStarred { index: 0, id: 1, starred: true };
        let Some(Outcome::StarredSet { id: 1, starred: true, result: Ok(()), .. }) = backend.run(star).await else {
            panic!("not starred");
        };
        let Some(Outcome::FeedCounts(Ok(counts))) = backend.run(Effect::FeedCounts).await else {
            panic!("no feed counts");
        };
        assert_eq!(counts.starred, Some(3));
        let request = PageRequest { past: 0, limit: 50, order: crate::paging::Order::NewestFirst };
        let open = Effect::OpenSelection {
            load: 1,
            selection: Selection::Starred,
            mode: crate::paging::Mode::Unread,
            request,
        };
        let Some(Outcome::Opened { result: Ok((page, newest)), .. }) = backend.run(open).await else {
            panic!("not opened");
        };
        assert_eq!(page.entries.iter().map(|e| e.id).collect::<Vec<_>>(), [5, 2, 1]);
        assert_eq!(newest, 0, "the starred list asks for no newest unread id");
    }
}
