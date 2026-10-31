//! Minimal Miniflux API client: only what the reader needs.
//! API docs: https://miniflux.app/docs/api.html

use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;

use crate::config;

#[derive(Debug, Clone)]
pub struct Client {
    http: reqwest::Client,
    base: String,
    key: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct Category {
    pub id: i64,
    pub title: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct Feed {
    pub id: i64,
    pub title: String,
    pub category: Category,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct EntryFeed {
    pub id: i64,
    pub title: String,
}

/// Read state of an entry, as Miniflux spells it. (Miniflux also has `removed`, which we never see.)
#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Read,
    Unread,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct Entry {
    pub id: i64,
    pub title: String,
    pub url: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub content: String,
    pub published_at: String,
    pub status: Status,
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
    #[serde(default)]
    unreads: HashMap<String, i64>,
}

#[derive(Deserialize)]
struct FetchedContent {
    content: String,
}

/// What the picker shows: every feed with its category and unread count.
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct FeedCount {
    pub feed: Feed,
    pub unread: i64,
}

/// Which set of unread entries to read.
#[derive(Debug, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Selection {
    All,
    Category { id: i64 },
    Feed { id: i64 },
}

impl Selection {
    /// The API path prefix that scopes `/entries` to this selection.
    fn path_prefix(self) -> String {
        match self {
            Selection::All => String::new(),
            Selection::Category { id } => format!("/categories/{id}"),
            Selection::Feed { id } => format!("/feeds/{id}"),
        }
    }
}

/// Entries per request when paging through a selection.
const PAGE: usize = 250;
/// Upper bound on entries loaded for one selection; keeps a confused server from making us loop forever.
const MAX_ENTRIES: usize = 10_000;
/// How much of an error response body to echo in a message.
const MAX_ERROR_BODY: usize = 300;

impl Client {
    pub fn new(http: reqwest::Client, cfg: &config::Miniflux) -> Self {
        Self { http, base: cfg.url.clone(), key: cfg.api_key.clone() }
    }

    fn req(&self, method: reqwest::Method, path: &str) -> reqwest::RequestBuilder {
        self.http.request(method, format!("{}/v1{path}", self.base)).header("X-Auth-Token", &self.key)
    }

    async fn send(rb: reqwest::RequestBuilder) -> Result<reqwest::Response, String> {
        let resp = rb.send().await.map_err(|e| format!("Miniflux unreachable: {e}"))?;
        let status = resp.status();
        if status.is_success() {
            return Ok(resp);
        }
        let body = resp.text().await.unwrap_or_default();
        Err(format!("Miniflux {status}: {}", truncate(&body, MAX_ERROR_BODY)))
    }

    async fn get_json<T: serde::de::DeserializeOwned>(&self, path: &str) -> Result<T, String> {
        Self::send(self.req(reqwest::Method::GET, path))
            .await?
            .json()
            .await
            .map_err(|e| format!("Unexpected answer from Miniflux: {e}"))
    }

    /// Checks URL and API key; returns the username.
    pub async fn check(&self) -> Result<String, String> {
        #[derive(Deserialize)]
        struct Me {
            username: String,
        }
        let resp = self.req(reqwest::Method::GET, "/me").send().await.map_err(|_| {
            format!("Could not reach Miniflux at {}. Check the address and that the server is running.", self.base)
        })?;
        match resp.status().as_u16() {
            200 => resp
                .json::<Me>()
                .await
                .map(|me| me.username)
                .map_err(|_| format!("{} doesn't look like a Miniflux server", self.base)),
            401 | 403 => Err("Miniflux rejected the API key".into()),
            404 => Err(format!("{} doesn't look like a Miniflux server", self.base)),
            code => Err(format!("Miniflux answered with HTTP {code}")),
        }
    }

    pub async fn feed_counts(&self) -> Result<Vec<FeedCount>, String> {
        let feeds: Vec<Feed> = self.get_json("/feeds").await?;
        let counters: Counters = self.get_json("/feeds/counters").await?;
        Ok(merge_counts(feeds, &counters.unreads))
    }

    /// All unread entries of the selection, oldest first.
    pub async fn unread_entries(&self, sel: Selection) -> Result<Vec<Entry>, String> {
        let prefix = sel.path_prefix();
        let mut all: Vec<Entry> = Vec::new();
        loop {
            let path = format!(
                "{prefix}/entries?status=unread&order=published_at&direction=asc&limit={PAGE}&offset={}",
                all.len()
            );
            let page: EntryPage = self.get_json(&path).await?;
            let got = page.entries.len();
            all.extend(page.entries);
            let total = usize::try_from(page.total).unwrap_or(0);
            if got == 0 || all.len() >= total || all.len() >= MAX_ENTRIES {
                return Ok(all);
            }
        }
    }

    pub async fn set_status(&self, ids: &[i64], status: Status) -> Result<(), String> {
        if ids.is_empty() {
            return Ok(());
        }
        let body = json!({ "entry_ids": ids, "status": status });
        Self::send(self.req(reqwest::Method::PUT, "/entries").json(&body)).await?;
        Ok(())
    }

    /// Asks Miniflux to scrape the original page and returns the full content.
    pub async fn fetch_content(&self, id: i64) -> Result<String, String> {
        let fetched: FetchedContent =
            self.get_json(&format!("/entries/{id}/fetch-content?update_content=true")).await?;
        Ok(fetched.content)
    }

    pub async fn refresh_all(&self) -> Result<(), String> {
        Self::send(self.req(reqwest::Method::PUT, "/feeds/refresh")).await?;
        Ok(())
    }
}

/// Pairs each feed with its unread count; feeds the counters don't mention have none.
fn merge_counts(feeds: Vec<Feed>, unreads: &HashMap<String, i64>) -> Vec<FeedCount> {
    feeds
        .into_iter()
        .map(|feed| {
            let unread = unreads.get(&feed.id.to_string()).copied().unwrap_or(0);
            FeedCount { feed, unread }
        })
        .collect()
}

/// Cuts `text` to at most `max` characters, marking the cut.
pub fn truncate(text: &str, max: usize) -> String {
    let text = text.trim();
    match text.char_indices().nth(max) {
        Some((at, _)) => format!("{}…", &text[..at]),
        None => text.to_string(),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use wiremock::matchers::{body_json, header, method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn client(server: &MockServer) -> Client {
        let cfg = config::Miniflux { url: server.uri(), api_key: "secret".into() };
        Client::new(reqwest::Client::new(), &cfg)
    }

    fn feed(id: i64, title: &str, category: (i64, &str)) -> Feed {
        Feed { id, title: title.into(), category: Category { id: category.0, title: category.1.into() } }
    }

    fn entry_json(id: i64) -> serde_json::Value {
        json!({
            "id": id, "title": format!("Entry {id}"), "url": format!("https://example.com/{id}"),
            "published_at": "2026-10-24T08:12:00+02:00", "status": "unread",
            "feed": { "id": 1, "title": "Feed" }
        })
    }

    #[test]
    fn selection_deserializes_from_tagged_json() {
        let sel: Selection = serde_json::from_str(r#"{"kind":"all"}"#).unwrap();
        assert_eq!(sel, Selection::All);
        let sel: Selection = serde_json::from_str(r#"{"kind":"category","id":3}"#).unwrap();
        assert_eq!(sel, Selection::Category { id: 3 });
        let sel: Selection = serde_json::from_str(r#"{"kind":"feed","id":7}"#).unwrap();
        assert_eq!(sel, Selection::Feed { id: 7 });
        assert!(serde_json::from_str::<Selection>(r#"{"kind":"starred"}"#).is_err());
        assert_eq!(Selection::All.path_prefix(), "");
        assert_eq!(Selection::Category { id: 3 }.path_prefix(), "/categories/3");
        assert_eq!(Selection::Feed { id: 7 }.path_prefix(), "/feeds/7");
    }

    #[test]
    fn status_uses_miniflux_spelling() {
        assert_eq!(serde_json::to_string(&Status::Read).unwrap(), r#""read""#);
        assert_eq!(serde_json::from_str::<Status>(r#""unread""#).unwrap(), Status::Unread);
        assert!(serde_json::from_str::<Status>(r#""removed""#).is_err());
    }

    #[test]
    fn entry_tolerates_missing_optional_fields() {
        let e: Entry = serde_json::from_value(entry_json(1)).unwrap();
        assert_eq!(e.author, "");
        assert_eq!(e.content, "");
        assert_eq!(e.reading_time, 0);
        assert_eq!(e.status, Status::Unread);
    }

    #[test]
    fn merge_counts_defaults_to_zero() {
        let feeds = vec![feed(1, "A", (1, "Cat")), feed(2, "B", (1, "Cat"))];
        let unreads = HashMap::from([("1".to_string(), 4), ("99".to_string(), 9)]);
        let counts = merge_counts(feeds.clone(), &unreads);
        assert_eq!(
            counts,
            vec![FeedCount { feed: feeds[0].clone(), unread: 4 }, FeedCount { feed: feeds[1].clone(), unread: 0 }]
        );
    }

    #[test]
    fn truncate_cuts_on_char_boundaries() {
        assert_eq!(truncate("short", 10), "short");
        assert_eq!(truncate("  padded  ", 10), "padded");
        assert_eq!(truncate("héllo wörld", 5), "héllo…");
        assert_eq!(truncate("exact", 5), "exact");
    }

    /// Mounts a `/me` answer for the duration of the returned guard.
    async fn me_answers(server: &MockServer, response: ResponseTemplate) -> wiremock::MockGuard {
        Mock::given(method("GET"))
            .and(path("/v1/me"))
            .and(header("X-Auth-Token", "secret"))
            .respond_with(response)
            .expect(1)
            .mount_as_scoped(server)
            .await
    }

    #[tokio::test]
    async fn check_maps_status_codes_to_messages() {
        let server = MockServer::start().await;
        let c = client(&server);

        let guard = me_answers(&server, ResponseTemplate::new(200).set_body_json(json!({ "username": "ward" }))).await;
        assert_eq!(c.check().await, Ok("ward".into()));
        drop(guard);

        for code in [401, 403] {
            let guard = me_answers(&server, ResponseTemplate::new(code)).await;
            assert_eq!(c.check().await.unwrap_err(), "Miniflux rejected the API key");
            drop(guard);
        }

        let guard = me_answers(&server, ResponseTemplate::new(404)).await;
        assert!(c.check().await.unwrap_err().contains("doesn't look like a Miniflux server"));
        drop(guard);

        let guard = me_answers(&server, ResponseTemplate::new(200).set_body_string("<html>not json</html>")).await;
        assert!(c.check().await.unwrap_err().contains("doesn't look like a Miniflux server"));
        drop(guard);

        let guard = me_answers(&server, ResponseTemplate::new(502)).await;
        assert_eq!(c.check().await.unwrap_err(), "Miniflux answered with HTTP 502");
        drop(guard);
    }

    #[tokio::test]
    async fn check_reports_unreachable_server() {
        // Port 1 is never listening; a freed mock port could be reused by a parallel test.
        let uri = "http://127.0.0.1:1".to_string();
        let cfg = config::Miniflux { url: uri.clone(), api_key: "k".into() };
        let err = Client::new(reqwest::Client::new(), &cfg).check().await.unwrap_err();
        assert!(err.starts_with(&format!("Could not reach Miniflux at {uri}")), "{err}");
    }

    #[tokio::test]
    async fn feed_counts_joins_feeds_with_counters() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/feeds"))
            .and(header("X-Auth-Token", "secret"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!([
                { "id": 1, "title": "A", "category": { "id": 10, "title": "Dev" }, "site_url": "ignored" },
                { "id": 2, "title": "B", "category": { "id": 10, "title": "Dev" } }
            ])))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/v1/feeds/counters"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!({ "reads": { "1": 5 }, "unreads": { "2": 3 } })),
            )
            .mount(&server)
            .await;
        let counts = client(&server).feed_counts().await.unwrap();
        assert_eq!(counts.iter().map(|c| (c.feed.id, c.unread)).collect::<Vec<_>>(), vec![(1, 0), (2, 3)]);
    }

    #[tokio::test]
    async fn unread_entries_pages_until_total() {
        let server = MockServer::start().await;
        let page1: Vec<_> = (1..=PAGE as i64).map(entry_json).collect();
        let page2: Vec<_> = (PAGE as i64 + 1..=PAGE as i64 + 2).map(entry_json).collect();
        Mock::given(method("GET"))
            .and(path("/v1/categories/10/entries"))
            .and(query_param("status", "unread"))
            .and(query_param("order", "published_at"))
            .and(query_param("direction", "asc"))
            .and(query_param("offset", "0"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "total": PAGE + 2, "entries": page1 })))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/v1/categories/10/entries"))
            .and(query_param("offset", PAGE.to_string()))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "total": PAGE + 2, "entries": page2 })))
            .expect(1)
            .mount(&server)
            .await;
        let entries = client(&server).unread_entries(Selection::Category { id: 10 }).await.unwrap();
        assert_eq!(entries.len(), PAGE + 2);
        assert_eq!(entries.last().unwrap().id, PAGE as i64 + 2);
    }

    #[tokio::test]
    async fn unread_entries_stops_on_empty_page_even_if_total_lies() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/entries"))
            .and(query_param("offset", "0"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "total": 999, "entries": [entry_json(1)] })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/v1/entries"))
            .and(query_param("offset", "1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "total": 999, "entries": [] })))
            .expect(1)
            .mount(&server)
            .await;
        let entries = client(&server).unread_entries(Selection::All).await.unwrap();
        assert_eq!(entries.len(), 1);
    }

    #[tokio::test]
    async fn set_status_sends_ids_and_skips_empty() {
        let server = MockServer::start().await;
        Mock::given(method("PUT"))
            .and(path("/v1/entries"))
            .and(body_json(json!({ "entry_ids": [4, 5], "status": "read" })))
            .respond_with(ResponseTemplate::new(204))
            .expect(1)
            .mount(&server)
            .await;
        let c = client(&server);
        assert_eq!(c.set_status(&[4, 5], Status::Read).await, Ok(()));
        assert_eq!(c.set_status(&[], Status::Read).await, Ok(()));
    }

    #[tokio::test]
    async fn fetch_content_and_refresh() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/entries/42/fetch-content"))
            .and(query_param("update_content", "true"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!({ "content": "<p>full</p>", "reading_time": 3 })),
            )
            .mount(&server)
            .await;
        Mock::given(method("PUT"))
            .and(path("/v1/feeds/refresh"))
            .respond_with(ResponseTemplate::new(204))
            .expect(1)
            .mount(&server)
            .await;
        let c = client(&server);
        assert_eq!(c.fetch_content(42).await, Ok("<p>full</p>".into()));
        assert_eq!(c.refresh_all().await, Ok(()));
    }

    #[tokio::test]
    async fn errors_carry_status_and_truncated_body() {
        let server = MockServer::start().await;
        let long = "x".repeat(1000);
        Mock::given(method("PUT"))
            .and(path("/v1/feeds/refresh"))
            .respond_with(ResponseTemplate::new(500).set_body_string(long))
            .mount(&server)
            .await;
        let err = client(&server).refresh_all().await.unwrap_err();
        assert!(err.starts_with("Miniflux 500 Internal Server Error: xxx"), "{err}");
        assert!(err.chars().count() < MAX_ERROR_BODY + 50, "{}", err.len());
        assert!(err.ends_with('…'));
    }
}
