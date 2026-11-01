#[cfg(test)]
mod bindings;
mod config;
mod demo;
mod error;
mod miniflux;
mod readeck;
mod theme;

use error::Error;
use miniflux::{EntryPage, EntryStatus, FeedCount, Selection};
use serde::{Deserialize, Serialize};
use std::sync::RwLock;
use tauri::State;

/// Shared state. The config can change at runtime (onboarding / settings), hence the lock.
struct App {
    config: RwLock<config::Config>,
    http: reqwest::Client,
    /// `KLETSE_RSS_DEMO=1`: serve built-in sample data, never touch a server.
    demo: bool,
}

impl App {
    fn new() -> Self {
        let http = reqwest::Client::builder()
            .user_agent(concat!("kletse-rss/", env!("CARGO_PKG_VERSION")))
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .expect("the HTTP client only needs built-in TLS roots");
        let demo = std::env::var("KLETSE_RSS_DEMO").is_ok_and(|v| v == "1");
        Self { config: RwLock::new(config::read()), http, demo }
    }

    fn config(&self) -> config::Config {
        // A poisoned lock only means another command panicked mid-write; the config is still plain data.
        self.config.read().unwrap_or_else(|e| e.into_inner()).clone()
    }

    fn set_config(&self, cfg: config::Config) {
        *self.config.write().unwrap_or_else(|e| e.into_inner()) = cfg;
    }

    fn miniflux(&self) -> Result<miniflux::Client, Error> {
        let cfg = self.config();
        if !cfg.is_complete() {
            return Err(Error::Setup("Miniflux is not set up yet".into()));
        }
        Ok(miniflux::Client::new(self.http.clone(), &cfg.miniflux))
    }
}

/// What the UI needs to know at startup and after saving settings.
#[derive(Debug, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
struct AppStatus {
    /// Miniflux URL and key are present; otherwise show onboarding.
    configured: bool,
    /// Readeck is set up; otherwise hide the bookmark button.
    readeck: bool,
}

#[tauri::command]
fn app_status(app: State<'_, App>) -> AppStatus {
    let cfg = app.config();
    AppStatus { configured: app.demo || cfg.is_complete(), readeck: app.demo || cfg.readeck.is_some() }
}

/// The settings form, flat. Empty Readeck fields mean "not set up".
/// No `Debug` on purpose: it holds the API key and token.
#[derive(Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
struct Settings {
    miniflux_url: String,
    miniflux_api_key: String,
    readeck_url: String,
    readeck_token: String,
}

#[tauri::command]
fn get_settings(app: State<'_, App>) -> Settings {
    let cfg = app.config();
    let readeck = cfg.readeck.unwrap_or_default();
    Settings {
        miniflux_url: cfg.miniflux.url,
        miniflux_api_key: cfg.miniflux.api_key,
        readeck_url: readeck.url,
        readeck_token: readeck.token,
    }
}

/// Validates the settings against the real servers, then saves and applies them.
#[tauri::command]
async fn save_settings(app: State<'_, App>, settings: Settings) -> Result<AppStatus, Error> {
    let has_readeck_url = !settings.readeck_url.trim().is_empty();
    let has_readeck_token = !settings.readeck_token.trim().is_empty();
    if has_readeck_url != has_readeck_token {
        return Err(Error::Setup("Fill in both Readeck fields, or leave both empty.".into()));
    }
    let cfg = config::Config {
        miniflux: config::Miniflux { url: settings.miniflux_url, api_key: settings.miniflux_api_key },
        readeck: Some(config::Readeck { url: settings.readeck_url, token: settings.readeck_token }),
    }
    .normalized();
    cfg.validate()?;

    if !app.demo {
        miniflux::Client::new(app.http.clone(), &cfg.miniflux).check().await?;
        if let Some(readeck) = &cfg.readeck {
            readeck::check(&app.http, readeck).await?;
        }
        // Writing and flushing the file blocks; keep it off the async runtime's few workers.
        let saved = cfg.clone();
        tauri::async_runtime::spawn_blocking(move || config::save(&saved)).await.map_err(|e| Error::Io {
            context: "Could not save the settings".into(),
            source: std::io::Error::other(e),
        })??;
    }
    app.set_config(cfg);
    Ok(app_status(app))
}

/// `theme::load` starts `hyprctl` / `omarchy` / `defaults` and waits for them, so it runs on a thread meant for
/// blocking work: off the main thread, which would freeze the window, and off the async runtime's few workers.
#[tauri::command]
async fn get_theme() -> theme::Theme {
    // Only a panic in `load` fails the join, and a release build aborts on panic first; in a debug build the
    // webview gets an empty theme, which it copes with.
    tauri::async_runtime::spawn_blocking(theme::load).await.unwrap_or_default()
}

#[tauri::command]
async fn feed_counts(app: State<'_, App>) -> Result<Vec<FeedCount>, Error> {
    if app.demo {
        return Ok(demo::feed_counts());
    }
    app.miniflux()?.feed_counts().await
}

/// One page: unread entries oldest first, published after `past`, or read ones newest first, published before it.
#[tauri::command]
async fn entries(
    app: State<'_, App>,
    selection: Selection,
    status: EntryStatus,
    past: i64,
    limit: usize,
) -> Result<EntryPage, Error> {
    if app.demo {
        return Ok(demo::entries(selection, status, past, limit));
    }
    app.miniflux()?.entries(selection, status, past, limit).await
}

/// The selection's newest unread entry id when it's opened: "mark all read" goes up to there.
#[tauri::command]
async fn newest_unread_id(app: State<'_, App>, selection: Selection) -> Result<i64, Error> {
    if app.demo {
        return Ok(demo::newest_unread_id(selection));
    }
    app.miniflux()?.newest_unread_id(selection).await
}

#[tauri::command]
async fn unread_count(app: State<'_, App>, selection: Selection, up_to: i64) -> Result<i64, Error> {
    if app.demo {
        return Ok(demo::unread_count(selection, up_to));
    }
    app.miniflux()?.unread_count(selection, up_to).await
}

/// Marks the selection's unread entries up to an id read; returns how many.
#[tauri::command]
async fn mark_read_up_to(app: State<'_, App>, selection: Selection, up_to: i64) -> Result<usize, Error> {
    if app.demo {
        return Ok(demo::mark_read_up_to(selection, up_to));
    }
    app.miniflux()?.mark_read_up_to(selection, up_to).await
}

#[tauri::command]
async fn set_status(app: State<'_, App>, ids: Vec<i64>, status: EntryStatus) -> Result<(), Error> {
    if app.demo {
        return Ok(());
    }
    app.miniflux()?.set_status(&ids, status).await
}

#[tauri::command]
async fn fetch_content(app: State<'_, App>, id: i64) -> Result<String, Error> {
    if app.demo {
        return Ok(demo::content(id));
    }
    app.miniflux()?.fetch_content(id).await
}

#[tauri::command]
async fn refresh_feeds(app: State<'_, App>) -> Result<(), Error> {
    if app.demo {
        return Ok(());
    }
    app.miniflux()?.refresh_all().await
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
#[tauri::command]
async fn open_in_browser(url: String) -> Result<(), Error> {
    if !is_openable(&url) {
        return Err(Error::Refused(format!("Refusing to open {url}")));
    }
    let mut child = std::process::Command::new(OPENER)
        .arg(&url)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|source| Error::Io { context: format!("Could not start {OPENER}"), source })?;
    // Reap the child so it does not linger as a zombie.
    std::thread::spawn(move || child.wait());
    Ok(())
}

#[tauri::command]
async fn save_to_readeck(app: State<'_, App>, url: String) -> Result<(), Error> {
    if app.demo {
        return Ok(());
    }
    let readeck =
        app.config().readeck.ok_or_else(|| Error::Setup("Readeck is not set up. Add it in Settings.".into()))?;
    readeck::save(&app.http, &readeck, &url).await
}

/// `q`: a clean exit, independent of what the webview thinks `window.close()` may do.
#[tauri::command]
fn quit(app: tauri::AppHandle) {
    app.exit(0);
}

pub fn run() {
    let builder = tauri::Builder::default();
    // Mac apps reopen where they were left; under a tiling compositor the WM decides.
    #[cfg(target_os = "macos")]
    let builder = builder.plugin(tauri_plugin_window_state::Builder::default().build());
    builder
        .manage(App::new())
        .setup(|app| {
            theme::watch(app.handle().clone());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            app_status,
            get_settings,
            save_settings,
            get_theme,
            feed_counts,
            entries,
            newest_unread_id,
            unread_count,
            mark_read_up_to,
            set_status,
            fetch_content,
            refresh_feeds,
            save_to_readeck,
            open_in_browser,
            quit
        ])
        .run(tauri::generate_context!())
        .expect("error while running kletse-rss");
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

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

    #[test]
    fn settings_round_trip_the_webview_field_names() {
        let json = r#"{"miniflux_url":"https://m","miniflux_api_key":"k","readeck_url":"","readeck_token":""}"#;
        let s: Settings = serde_json::from_str(json).unwrap();
        assert_eq!(s.miniflux_url, "https://m");
        assert_eq!(serde_json::to_string(&s).unwrap(), json);
    }

    #[test]
    fn app_status_serializes_flat_booleans() {
        let s = AppStatus { configured: true, readeck: false };
        assert_eq!(serde_json::to_string(&s).unwrap(), r#"{"configured":true,"readeck":false}"#);
    }
}
