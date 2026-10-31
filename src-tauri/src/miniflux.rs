//! Minimal Miniflux API client: only what the reader needs.
//! API docs: https://miniflux.app/docs/api.html

use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;

use crate::config;

pub struct Client {
    http: reqwest::Client,
    base: String,
    key: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Category {
    pub id: i64,
    pub title: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Feed {
    pub id: i64,
    pub title: String,
    pub category: Category,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct EntryFeed {
    pub id: i64,
    pub title: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Entry {
    pub id: i64,
    pub title: String,
    pub url: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub content: String,
    pub published_at: String,
    pub status: String,
    #[serde(default)]
    pub reading_time: i64,
    pub feed: EntryFeed,
}

#[derive(Deserialize)]
struct EntryPage {
    total: i64,
    entries: Vec<Entry>,
}

#[derive(Deserialize)]
struct Counters {
    unreads: HashMap<String, i64>,
}

#[derive(Deserialize)]
struct FetchedContent {
    content: String,
}

/// What the picker shows: every feed with its category and unread count.
#[derive(Debug, Serialize)]
pub struct FeedCount {
    pub feed: Feed,
    pub unread: i64,
}

/// Which set of unread entries to read.
#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Selection {
    All,
    Category { id: i64 },
    Feed { id: i64 },
}

const PAGE: i64 = 250;

impl Client {
    pub fn new(http: reqwest::Client, cfg: &config::Miniflux) -> Self {
        Self { http, base: cfg.url.clone(), key: cfg.api_key.clone() }
    }

    fn req(&self, method: reqwest::Method, path: &str) -> reqwest::RequestBuilder {
        self.http
            .request(method, format!("{}/v1{}", self.base, path))
            .header("X-Auth-Token", &self.key)
    }

    async fn send(rb: reqwest::RequestBuilder) -> Result<reqwest::Response, String> {
        let resp = rb.send().await.map_err(|e| format!("Miniflux unreachable: {e}"))?;
        let status = resp.status();
        if status.is_success() {
            return Ok(resp);
        }
        let body = resp.text().await.unwrap_or_default();
        Err(format!("Miniflux {status}: {body}"))
    }

    pub async fn feed_counts(&self) -> Result<Vec<FeedCount>, String> {
        let feeds: Vec<Feed> = Self::send(self.req(reqwest::Method::GET, "/feeds"))
            .await?
            .json()
            .await
            .map_err(|e| e.to_string())?;
        let counters: Counters = Self::send(self.req(reqwest::Method::GET, "/feeds/counters"))
            .await?
            .json()
            .await
            .map_err(|e| e.to_string())?;
        Ok(feeds
            .into_iter()
            .map(|feed| {
                let unread = counters.unreads.get(&feed.id.to_string()).copied().unwrap_or(0);
                FeedCount { feed, unread }
            })
            .collect())
    }

    /// All unread entries of the selection, oldest first.
    pub async fn unread_entries(&self, sel: &Selection) -> Result<Vec<Entry>, String> {
        let prefix = match sel {
            Selection::All => String::new(),
            Selection::Category { id } => format!("/categories/{id}"),
            Selection::Feed { id } => format!("/feeds/{id}"),
        };
        let mut all = Vec::new();
        loop {
            let path = format!(
                "{prefix}/entries?status=unread&order=published_at&direction=asc&limit={PAGE}&offset={}",
                all.len()
            );
            let page: EntryPage = Self::send(self.req(reqwest::Method::GET, &path))
                .await?
                .json()
                .await
                .map_err(|e| e.to_string())?;
            let got = page.entries.len();
            all.extend(page.entries);
            if got == 0 || all.len() as i64 >= page.total {
                return Ok(all);
            }
        }
    }

    pub async fn set_status(&self, ids: &[i64], status: &str) -> Result<(), String> {
        if ids.is_empty() {
            return Ok(());
        }
        let body = json!({ "entry_ids": ids, "status": status });
        Self::send(self.req(reqwest::Method::PUT, "/entries").json(&body)).await?;
        Ok(())
    }

    /// Asks Miniflux to scrape the original page and returns the full content.
    pub async fn fetch_content(&self, id: i64) -> Result<String, String> {
        let path = format!("/entries/{id}/fetch-content?update_content=true");
        let fetched: FetchedContent = Self::send(self.req(reqwest::Method::GET, &path))
            .await?
            .json()
            .await
            .map_err(|e| e.to_string())?;
        Ok(fetched.content)
    }

    pub async fn refresh_all(&self) -> Result<(), String> {
        Self::send(self.req(reqwest::Method::PUT, "/feeds/refresh")).await?;
        Ok(())
    }
}
