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
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct Category {
    pub id: i64,
    pub title: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct Feed {
    pub id: i64,
    pub title: String,
    pub category: Category,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct EntryFeed {
    pub id: i64,
    pub title: String,
}

/// Read state of an entry, as Miniflux spells it. (Miniflux also has `removed`, which we never see.)
#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "lowercase")]
pub enum EntryStatus {
    Read,
    Unread,
}

impl EntryStatus {
    fn as_str(self) -> &'static str {
        match self {
            EntryStatus::Read => "read",
            EntryStatus::Unread => "unread",
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct Entry {
    pub id: i64,
    pub title: String,
    pub url: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub content: String,
    pub published_at: String,
    pub status: EntryStatus,
    #[serde(default)]
    pub reading_time: i64,
    pub feed: EntryFeed,
}

/// One page of entries, plus how many the server has in total for the query.
#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct EntryPage {
    pub total: i64,
    pub entries: Vec<Entry>,
}

/// A page of entries when only their ids matter; serde skips the rest (content included).
#[derive(Deserialize)]
struct IdPage {
    total: i64,
    entries: Vec<EntryId>,
}

#[derive(Deserialize)]
struct EntryId {
    id: i64,
}

#[derive(Deserialize)]
struct Counters {
    #[serde(default)]
    reads: HashMap<String, i64>,
    #[serde(default)]
    unreads: HashMap<String, i64>,
}

#[derive(Deserialize)]
struct FetchedContent {
    content: String,
}

/// What the picker shows: every feed with its category and its unread and read counts.
#[derive(Debug, Serialize, PartialEq, Eq)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct FeedCount {
    pub feed: Feed,
    pub unread: i64,
    pub read: i64,
}

/// Which set of entries to read.
#[derive(Debug, Deserialize, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(test, derive(ts_rs::TS))]
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

/// Upper bound on one page requested by the UI; Miniflux's own maximum for `limit`.
const MAX_PAGE: usize = 1000;
/// Entries per request when walking a selection by id to mark it read.
const ID_PAGE: usize = 250;
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
        Ok(merge_counts(feeds, &counters))
    }

    /// One page of the selection's entries: unread ones oldest first, or read ones (the history) newest first.
    /// Both are loaded as you go. `past` is where the page starts, in unix seconds: entries published after it
    /// (unread) or before it (read); 0 starts at the beginning. See `nextPage` in src/paging.ts for why that and
    /// not an offset. `after` / `before` compare `published_at` strictly and exist since Miniflux 2.0.9;
    /// `published_after` / `published_before` mean the same but need 2.0.49, and an older server would silently
    /// ignore them.
    pub async fn entries(
        &self,
        sel: Selection,
        status: EntryStatus,
        past: i64,
        limit: usize,
    ) -> Result<EntryPage, String> {
        let limit = limit.clamp(1, MAX_PAGE);
        let (direction, bound) = match status {
            EntryStatus::Unread => ("asc", "after"),
            EntryStatus::Read => ("desc", "before"),
        };
        let mut path = format!(
            "{}/entries?status={}&order=published_at&direction={direction}&limit={limit}",
            sel.path_prefix(),
            status.as_str()
        );
        if past > 0 {
            path.push_str(&format!("&{bound}={past}"));
        }
        self.get_json(&path).await
    }

    /// The id of the selection's newest unread entry, 0 if there is none. Entry ids only grow, so anything with
    /// a higher id arrived later: that is how "mark all read" leaves alone what you haven't been shown.
    pub async fn newest_unread_id(&self, sel: Selection) -> Result<i64, String> {
        let path = format!("{}/entries?status=unread&order=id&direction=desc&limit=1", sel.path_prefix());
        let page: IdPage = self.get_json(&path).await?;
        Ok(page.entries.first().map_or(0, |e| e.id))
    }

    /// How many of the selection's entries with an id up to `up_to` are unread.
    pub async fn unread_count(&self, sel: Selection, up_to: i64) -> Result<i64, String> {
        if up_to < 1 {
            return Ok(0); // Miniflux ignores before_entry_id=0 and would count everything
        }
        let path = format!("{}/entries?status=unread&before_entry_id={}&limit=1", sel.path_prefix(), up_to + 1);
        let page: IdPage = self.get_json(&path).await?;
        Ok(page.total)
    }

    /// Marks the selection's unread entries with an id up to `up_to` read; returns how many.
    /// Walks them by id, which no status change can shift, a page at a time.
    pub async fn mark_read_up_to(&self, sel: Selection, up_to: i64) -> Result<usize, String> {
        let mut marked = 0;
        if up_to < 1 {
            return Ok(marked);
        }
        let mut after = 0; // after_entry_id=0 means no lower bound
        loop {
            let path = format!(
                "{}/entries?status=unread&order=id&direction=asc&limit={ID_PAGE}&before_entry_id={}&after_entry_id={after}",
                sel.path_prefix(),
                up_to + 1
            );
            let page: IdPage = self.get_json(&path).await?;
            let ids: Vec<i64> = page.entries.iter().map(|e| e.id).collect();
            // A server that ignored after_entry_id would hand out the same page forever.
            let Some(&last) = ids.last().filter(|&&last| last > after) else { return Ok(marked) };
            self.set_status(&ids, EntryStatus::Read).await?;
            marked += ids.len();
            if ids.len() < ID_PAGE {
                return Ok(marked);
            }
            after = last;
        }
    }

    pub async fn set_status(&self, ids: &[i64], status: EntryStatus) -> Result<(), String> {
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

/// Pairs each feed with its unread and read counts; feeds the counters don't mention have none.
fn merge_counts(feeds: Vec<Feed>, counters: &Counters) -> Vec<FeedCount> {
    feeds
        .into_iter()
        .map(|feed| {
            let id = feed.id.to_string();
            let unread = counters.unreads.get(&id).copied().unwrap_or(0);
            let read = counters.reads.get(&id).copied().unwrap_or(0);
            FeedCount { feed, unread, read }
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
    use wiremock::matchers::{body_json, header, method, path, query_param, query_param_is_missing};
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
    fn entry_status_uses_miniflux_spelling() {
        assert_eq!(serde_json::to_string(&EntryStatus::Read).unwrap(), r#""read""#);
        assert_eq!(serde_json::from_str::<EntryStatus>(r#""unread""#).unwrap(), EntryStatus::Unread);
        assert!(serde_json::from_str::<EntryStatus>(r#""removed""#).is_err());
    }

    #[test]
    fn entry_tolerates_missing_optional_fields() {
        let e: Entry = serde_json::from_value(entry_json(1)).unwrap();
        assert_eq!(e.author, "");
        assert_eq!(e.content, "");
        assert_eq!(e.reading_time, 0);
        assert_eq!(e.status, EntryStatus::Unread);
    }

    #[test]
    fn merge_counts_defaults_to_zero() {
        let feeds = vec![feed(1, "A", (1, "Cat")), feed(2, "B", (1, "Cat"))];
        let counters = Counters {
            unreads: HashMap::from([("1".to_string(), 4), ("99".to_string(), 9)]),
            reads: HashMap::from([("2".to_string(), 7)]),
        };
        let counts = merge_counts(feeds.clone(), &counters);
        assert_eq!(
            counts,
            vec![
                FeedCount { feed: feeds[0].clone(), unread: 4, read: 0 },
                FeedCount { feed: feeds[1].clone(), unread: 0, read: 7 }
            ]
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
        assert_eq!(
            counts.iter().map(|c| (c.feed.id, c.unread, c.read)).collect::<Vec<_>>(),
            vec![(1, 0, 5), (2, 3, 0)]
        );
    }

    #[tokio::test]
    async fn unread_entries_are_a_page_oldest_first_published_after_past() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/categories/10/entries"))
            .and(query_param("status", "unread"))
            .and(query_param("order", "published_at"))
            .and(query_param("direction", "asc"))
            .and(query_param("limit", "60"))
            .and(query_param("after", "1761286319"))
            .and(query_param_is_missing("offset"))
            .and(query_param_is_missing("before"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "total": 70, "entries": [entry_json(4)] })))
            .expect(1)
            .mount(&server)
            .await;
        let c = client(&server);
        let page = c.entries(Selection::Category { id: 10 }, EntryStatus::Unread, 1_761_286_319, 60).await;
        let page = page.unwrap();
        assert_eq!(page.total, 70);
        assert_eq!(page.entries.iter().map(|e| e.id).collect::<Vec<_>>(), vec![4]);
    }

    #[tokio::test]
    async fn the_first_page_has_no_bound() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/entries"))
            .and(query_param("status", "unread"))
            .and(query_param_is_missing("after"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "total": 1, "entries": [entry_json(1)] })))
            .expect(1)
            .mount(&server)
            .await;
        let page = client(&server).entries(Selection::All, EntryStatus::Unread, 0, 50).await.unwrap();
        assert_eq!(page.entries.len(), 1);
    }

    #[tokio::test]
    async fn read_entries_are_a_page_newest_first_published_before_past() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/feeds/7/entries"))
            .and(query_param("status", "read"))
            .and(query_param("order", "published_at"))
            .and(query_param("direction", "desc"))
            .and(query_param("limit", MAX_PAGE.to_string()))
            .and(query_param("before", "1761286321"))
            .and(query_param_is_missing("after"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "total": 41, "entries": [entry_json(3)] })))
            .expect(1)
            .mount(&server)
            .await;
        let c = client(&server);
        let page = c.entries(Selection::Feed { id: 7 }, EntryStatus::Read, 1_761_286_321, 100_000).await;
        assert_eq!(page.unwrap().total, 41);
    }

    #[tokio::test]
    async fn newest_unread_id_asks_for_the_highest_id() {
        let server = MockServer::start().await;
        let guard = Mock::given(method("GET"))
            .and(path("/v1/feeds/7/entries"))
            .and(query_param("status", "unread"))
            .and(query_param("order", "id"))
            .and(query_param("direction", "desc"))
            .and(query_param("limit", "1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "total": 9, "entries": [entry_json(81)] })))
            .expect(1)
            .mount_as_scoped(&server)
            .await;
        let c = client(&server);
        assert_eq!(c.newest_unread_id(Selection::Feed { id: 7 }).await, Ok(81));
        drop(guard);
        Mock::given(method("GET"))
            .and(path("/v1/feeds/7/entries"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "total": 0, "entries": [] })))
            .mount(&server)
            .await;
        assert_eq!(c.newest_unread_id(Selection::Feed { id: 7 }).await, Ok(0));
    }

    #[tokio::test]
    async fn unread_count_counts_up_to_an_id() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/categories/3/entries"))
            .and(query_param("status", "unread"))
            .and(query_param("before_entry_id", "81"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "total": 57, "entries": [entry_json(2)] })))
            .expect(1)
            .mount(&server)
            .await;
        let c = client(&server);
        assert_eq!(c.unread_count(Selection::Category { id: 3 }, 80).await, Ok(57));
        assert_eq!(c.unread_count(Selection::Category { id: 3 }, 0).await, Ok(0));
        // no request
    }

    /// Answers the id walk of `mark_read_up_to` with `ids`, for entries after `after`.
    async fn unread_ids(server: &MockServer, after: i64, ids: &[i64]) {
        let entries: Vec<_> = ids.iter().copied().map(entry_json).collect();
        Mock::given(method("GET"))
            .and(path("/v1/entries"))
            .and(query_param("status", "unread"))
            .and(query_param("order", "id"))
            .and(query_param("direction", "asc"))
            .and(query_param("before_entry_id", "1001"))
            .and(query_param("after_entry_id", after.to_string()))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "total": 0, "entries": entries })))
            .expect(1)
            .mount(server)
            .await;
    }

    #[tokio::test]
    async fn mark_read_up_to_walks_the_ids_and_marks_each_page() {
        let server = MockServer::start().await;
        let page1: Vec<i64> = (1..=ID_PAGE as i64).collect();
        let page2 = [ID_PAGE as i64 + 1, 1000];
        unread_ids(&server, 0, &page1).await;
        unread_ids(&server, ID_PAGE as i64, &page2).await;
        for ids in [&page1[..], &page2[..]] {
            Mock::given(method("PUT"))
                .and(path("/v1/entries"))
                .and(body_json(json!({ "entry_ids": ids, "status": "read" })))
                .respond_with(ResponseTemplate::new(204))
                .expect(1)
                .mount(&server)
                .await;
        }
        assert_eq!(client(&server).mark_read_up_to(Selection::All, 1000).await, Ok(ID_PAGE + 2));
    }

    #[tokio::test]
    async fn mark_read_up_to_stops_when_the_server_ignores_after_entry_id() {
        let server = MockServer::start().await;
        let page: Vec<_> = (1..=ID_PAGE as i64).map(entry_json).collect();
        Mock::given(method("GET"))
            .and(path("/v1/entries"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "total": 0, "entries": page })))
            .expect(2)
            .mount(&server)
            .await;
        Mock::given(method("PUT"))
            .and(path("/v1/entries"))
            .respond_with(ResponseTemplate::new(204))
            .expect(1)
            .mount(&server)
            .await;
        let c = client(&server);
        assert_eq!(c.mark_read_up_to(Selection::All, 1000).await, Ok(ID_PAGE));
        assert_eq!(c.mark_read_up_to(Selection::All, 0).await, Ok(0)); // no request
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
        assert_eq!(c.set_status(&[4, 5], EntryStatus::Read).await, Ok(()));
        assert_eq!(c.set_status(&[], EntryStatus::Read).await, Ok(()));
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
