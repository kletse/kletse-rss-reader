mod config;
mod miniflux;
mod readeck;
mod theme;

use miniflux::{Entry, FeedCount, Selection};
use tauri::State;

/// Shared state: config plus API clients, or the reason they could not be built.
struct App {
    setup: Result<(config::Config, miniflux::Client), String>,
    http: reqwest::Client,
}

impl App {
    fn new() -> Self {
        let http = reqwest::Client::builder()
            .user_agent("kletse-rss/1.0")
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .expect("http client");
        let setup = config::load().map(|cfg| {
            let client = miniflux::Client::new(http.clone(), &cfg.miniflux);
            (cfg, client)
        });
        Self { setup, http }
    }

    fn miniflux(&self) -> Result<&miniflux::Client, String> {
        self.setup.as_ref().map(|(_, c)| c).map_err(Clone::clone)
    }
}

#[tauri::command]
fn setup_error(app: State<'_, App>) -> Option<String> {
    app.setup.as_ref().err().cloned()
}

#[tauri::command]
fn get_theme() -> theme::Theme {
    theme::load()
}

#[tauri::command]
async fn feed_counts(app: State<'_, App>) -> Result<Vec<FeedCount>, String> {
    app.miniflux()?.feed_counts().await
}

#[tauri::command]
async fn unread_entries(app: State<'_, App>, selection: Selection) -> Result<Vec<Entry>, String> {
    app.miniflux()?.unread_entries(&selection).await
}

#[tauri::command]
async fn set_status(app: State<'_, App>, ids: Vec<i64>, status: String) -> Result<(), String> {
    if status != "read" && status != "unread" {
        return Err(format!("invalid status {status}"));
    }
    app.miniflux()?.set_status(&ids, &status).await
}

#[tauri::command]
async fn fetch_content(app: State<'_, App>, id: i64) -> Result<String, String> {
    app.miniflux()?.fetch_content(id).await
}

#[tauri::command]
async fn refresh_feeds(app: State<'_, App>) -> Result<(), String> {
    app.miniflux()?.refresh_all().await
}

#[tauri::command]
async fn save_to_readeck(app: State<'_, App>, url: String) -> Result<(), String> {
    let (cfg, _) = app.setup.as_ref().map_err(Clone::clone)?;
    readeck::save(&app.http, &cfg.readeck, &url).await
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(App::new())
        .setup(|app| {
            theme::watch(app.handle().clone());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            setup_error,
            get_theme,
            feed_counts,
            unread_entries,
            set_status,
            fetch_content,
            refresh_feeds,
            save_to_readeck
        ])
        .run(tauri::generate_context!())
        .expect("error while running kletse-rss");
}
