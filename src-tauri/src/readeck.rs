//! Saves a URL to Readeck. API docs: <readeck>/docs/api

use serde_json::json;

use crate::config;

pub async fn save(http: &reqwest::Client, cfg: &config::Readeck, url: &str) -> Result<(), String> {
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

/// Checks that Readeck is reachable and accepts the token, without creating anything.
/// A token that only has `bookmarks:write` gets 403 on the profile endpoint, which still
/// proves the token itself is valid; only 401 means it is wrong.
pub async fn check(http: &reqwest::Client, cfg: &config::Readeck) -> Result<(), String> {
    let resp = http
        .get(format!("{}/api/profile", cfg.url))
        .bearer_auth(&cfg.token)
        .header("Accept", "application/json")
        .send()
        .await
        .map_err(|_| format!("Could not reach Readeck at {}. Check the address and that the server is running.", cfg.url))?;
    match resp.status().as_u16() {
        200 | 403 => Ok(()),
        401 => Err("Readeck rejected the token. Copy the token value itself, not its ID, and give it the “Bookmarks: write” permission.".into()),
        404 => Err(format!("{} doesn't look like a Readeck server", cfg.url)),
        code => Err(format!("Readeck answered with HTTP {code}")),
    }
}
