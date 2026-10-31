//! Saves a URL to Readeck. API docs: <readeck>/docs/api

use serde_json::json;

use crate::config;

pub async fn save(http: &reqwest::Client, cfg: &config::Readeck, url: &str) -> Result<(), String> {
    if cfg.token.trim().is_empty() {
        return Err(format!("Add your Readeck token to {}", config::path().display()));
    }
    let resp = http
        .post(format!("{}/api/bookmarks", cfg.url))
        .bearer_auth(&cfg.token)
        .json(&json!({ "url": url }))
        .send()
        .await
        .map_err(|e| format!("Readeck unreachable: {e}"))?;
    let status = resp.status();
    if status.is_success() {
        Ok(())
    } else {
        Err(format!("Readeck {status}: {}", resp.text().await.unwrap_or_default()))
    }
}
