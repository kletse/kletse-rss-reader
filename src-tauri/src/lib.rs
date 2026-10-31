mod config;
mod demo;
mod miniflux;
mod readeck;
mod theme;

use miniflux::{Entry, FeedCount, Selection};
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
            .expect("http client");
        let demo = std::env::var("KLETSE_RSS_DEMO").is_ok_and(|v| v == "1");
        Self { config: RwLock::new(config::read()), http, demo }
    }

    fn config(&self) -> config::Config {
        self.config.read().expect("config lock").clone()
    }

    fn miniflux(&self) -> Result<miniflux::Client, String> {
        let cfg = self.config();
        if !cfg.is_complete() {
            return Err("Miniflux is not set up yet".into());
        }
        Ok(miniflux::Client::new(self.http.clone(), &cfg.miniflux))
    }
}

/// What the UI needs to know at startup and after saving settings.
#[derive(Serialize)]
struct Status {
    /// Miniflux URL and key are present; otherwise show onboarding.
    configured: bool,
    /// Readeck is set up; otherwise hide the bookmark button.
    readeck: bool,
    demo: bool,
}

#[tauri::command]
fn app_status(app: State<'_, App>) -> Status {
    let cfg = app.config();
    Status {
        configured: app.demo || cfg.is_complete(),
        readeck: app.demo || cfg.readeck.is_some(),
        demo: app.demo,
    }
}

/// The settings form, flat. Empty Readeck fields mean "not set up".
#[derive(Serialize, Deserialize)]
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
async fn save_settings(app: State<'_, App>, settings: Settings) -> Result<Status, String> {
    let has_readeck_url = !settings.readeck_url.trim().is_empty();
    let has_readeck_token = !settings.readeck_token.trim().is_empty();
    if has_readeck_url != has_readeck_token {
        return Err("Fill in both Readeck fields, or leave both empty.".into());
    }
    let cfg = config::Config {
        miniflux: config::Miniflux { url: settings.miniflux_url, api_key: settings.miniflux_api_key },
        readeck: Some(config::Readeck { url: settings.readeck_url, token: settings.readeck_token }),
    }
    .normalized();

    if cfg.miniflux.url.is_empty() || cfg.miniflux.api_key.is_empty() {
        return Err("The Miniflux URL and API key are required.".into());
    }
    for url in std::iter::once(&cfg.miniflux.url).chain(cfg.readeck.as_ref().map(|r| &r.url)) {
        if !(url.starts_with("https://") || url.starts_with("http://")) {
            return Err(format!("{url} should start with https:// or http://"));
        }
    }

    if !app.demo {
        miniflux::Client::new(app.http.clone(), &cfg.miniflux).check().await?;
        if let Some(readeck) = &cfg.readeck {
            readeck::check(&app.http, readeck).await?;
        }
        config::save(&cfg)?;
    }
    *app.config.write().expect("config lock") = cfg;
    Ok(app_status(app))
}

#[tauri::command]
fn get_theme() -> theme::Theme {
    theme::load()
}

#[tauri::command]
async fn feed_counts(app: State<'_, App>) -> Result<Vec<FeedCount>, String> {
    if app.demo {
        return Ok(demo::feed_counts());
    }
    app.miniflux()?.feed_counts().await
}

#[tauri::command]
async fn unread_entries(app: State<'_, App>, selection: Selection) -> Result<Vec<Entry>, String> {
    if app.demo {
        return Ok(demo::unread_entries(&selection));
    }
    app.miniflux()?.unread_entries(&selection).await
}

#[tauri::command]
async fn set_status(app: State<'_, App>, ids: Vec<i64>, status: String) -> Result<(), String> {
    if status != "read" && status != "unread" {
        return Err(format!("invalid status {status}"));
    }
    if app.demo {
        return Ok(());
    }
    app.miniflux()?.set_status(&ids, &status).await
}

#[tauri::command]
async fn fetch_content(app: State<'_, App>, id: i64) -> Result<String, String> {
    if app.demo {
        return Ok(demo::content(id));
    }
    app.miniflux()?.fetch_content(id).await
}

#[tauri::command]
async fn refresh_feeds(app: State<'_, App>) -> Result<(), String> {
    if app.demo {
        return Ok(());
    }
    app.miniflux()?.refresh_all().await
}

/// Opens a link in the default browser via xdg-open (on Omarchy: omarchy-launch-browser).
/// Firefox and Chromium hand the URL to an already running window as a new tab.
#[tauri::command]
fn open_in_browser(url: String) -> Result<(), String> {
    if !(url.starts_with("https://") || url.starts_with("http://") || url.starts_with("mailto:")) {
        return Err(format!("Refusing to open {url}"));
    }
    let mut child = std::process::Command::new("xdg-open")
        .arg(&url)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| format!("Could not start xdg-open: {e}"))?;
    // Reap the child so it does not linger as a zombie.
    std::thread::spawn(move || child.wait());
    Ok(())
}

#[tauri::command]
async fn save_to_readeck(app: State<'_, App>, url: String) -> Result<(), String> {
    if app.demo {
        return Ok(());
    }
    let readeck = app.config().readeck.ok_or("Readeck is not set up. Add it in Settings.")?;
    readeck::save(&app.http, &readeck, &url).await
}

pub fn run() {
    tauri::Builder::default()
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
            unread_entries,
            set_status,
            fetch_content,
            refresh_feeds,
            save_to_readeck,
            open_in_browser
        ])
        .run(tauri::generate_context!())
        .expect("error while running kletse-rss");
}
