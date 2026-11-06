//! Minimal Miniflux API client: only what the reader needs.
//! API docs: https://miniflux.app/docs/api.html

use serde::Deserialize;
use serde_json::json;
use std::collections::HashMap;

use crate::config;
use crate::error::{Error, Service, ok_status};
use crate::feeds::{Counts, EntryPage, EntryStatus, Feed, FeedCount, Selection};
use crate::paging::Order;

/// Why a password didn't become a key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MintFailed {
    /// 401: the password is wrong, or this account has none, or a proxy removed it. These can't be told apart.
    Refused,
    /// 404, which older servers answer before they look at the password: it says nothing about that.
    NoSuchCall,
    /// It redirected, and a password isn't something to send after it.
    Redirected,
    /// Anything else, as a message.
    Other(String),
}

/// Trades a username and password for a key of Onward's own, once. `http` follows no redirects. The description must
/// be new: Miniflux refuses a second key with the same one.
pub async fn mint_key(
    http: &reqwest::Client,
    base: &str,
    username: &str,
    password: &str,
    description: &str,
) -> Result<String, MintFailed> {
    #[derive(Deserialize)]
    struct Minted {
        token: String,
    }
    #[derive(Deserialize)]
    struct Refusal {
        error_message: String,
    }
    let response = http
        .post(format!("{base}/v1/api-keys"))
        .basic_auth(username, Some(password))
        .json(&json!({ "description": description }))
        .send()
        .await
        .map_err(|e| MintFailed::Other(Error::unreachable(Service::Miniflux, base, e).message()))?;
    let status = response.status();
    match status.as_u16() {
        201 => match response.json::<Minted>().await {
            Ok(minted) if !minted.token.is_empty() => Ok(minted.token),
            _ => Err(MintFailed::Other("Miniflux's answer had no key in it.".into())),
        },
        401 => Err(MintFailed::Refused),
        404 => Err(MintFailed::NoSuchCall),
        300..=399 => Err(MintFailed::Redirected),
        _ => Err(MintFailed::Other(match response.json::<Refusal>().await {
            Ok(refusal) => format!("Miniflux answered: {}", refusal.error_message),
            Err(_) => format!("Miniflux answered with HTTP {status}"),
        })),
    }
}

/// `Debug` leaves out the API key.
#[derive(Clone)]
pub struct Client {
    http: reqwest::Client,
    base: String,
    key: String,
}

impl std::fmt::Debug for Client {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Client")
            .field("base", &self.base)
            .field("key", &config::Redacted(&self.key))
            .finish_non_exhaustive()
    }
}

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

fn status_name(status: EntryStatus) -> &'static str {
    match status {
        EntryStatus::Read => "read",
        EntryStatus::Unread => "unread",
    }
}

/// `/entries` scoped to the selection, with `query` (which mustn't be empty) after the `?`.
fn entries_path(sel: Selection, query: &str) -> String {
    match sel {
        Selection::All => format!("/entries?{query}"),
        Selection::Category { id } => format!("/categories/{id}/entries?{query}"),
        Selection::Feed { id } => format!("/feeds/{id}/entries?{query}"),
        Selection::Starred => format!("/entries?starred=true&{query}"),
    }
}

/// Upper bound on one page requested by the UI; Miniflux's own maximum for `limit`.
const MAX_PAGE: usize = 1000;
/// Entries per request when walking a selection by id to mark it read.
const ID_PAGE: usize = 250;

impl Client {
    pub fn new(http: reqwest::Client, cfg: &config::Miniflux) -> Self {
        Self { http, base: cfg.url.clone(), key: cfg.api_key.clone() }
    }

    fn req(&self, method: reqwest::Method, path: &str) -> reqwest::RequestBuilder {
        self.http.request(method, format!("{}/v1{path}", self.base)).header("X-Auth-Token", &self.key)
    }

    async fn send(&self, rb: reqwest::RequestBuilder) -> Result<reqwest::Response, Error> {
        let resp = rb.send().await.map_err(|e| Error::unreachable(Service::Miniflux, &self.base, e))?;
        ok_status(Service::Miniflux, resp).await
    }

    async fn get_json<T: serde::de::DeserializeOwned>(&self, path: &str) -> Result<T, Error> {
        self.json(self.req(reqwest::Method::GET, path)).await
    }

    async fn json<T: serde::de::DeserializeOwned>(&self, rb: reqwest::RequestBuilder) -> Result<T, Error> {
        self.send(rb).await?.json().await.map_err(|e| Error::reading(Service::Miniflux, &self.base, e))
    }

    /// Checks URL and API key; returns the username.
    pub async fn check(&self) -> Result<String, Error> {
        #[derive(Deserialize)]
        struct Me {
            username: String,
        }
        let not_miniflux = || Error::Setup(format!("{} doesn't look like a Miniflux server", self.base));
        let resp = self
            .req(reqwest::Method::GET, "/me")
            .send()
            .await
            .map_err(|e| Error::unreachable(Service::Miniflux, &self.base, e))?;
        match resp.status().as_u16() {
            200 => resp.json::<Me>().await.map(|me| me.username).map_err(|_| not_miniflux()),
            401 | 403 => Err(Error::Rejected("Miniflux rejected the API key".into())),
            404 => Err(not_miniflux()),
            code => Err(Error::unexpected(format!("Miniflux answered with HTTP {code}"))),
        }
    }

    /// Every feed with its counts, and how many entries are starred (read or unread).
    pub async fn feed_counts(&self) -> Result<Counts, Error> {
        let feeds: Vec<Feed> = self.get_json("/feeds").await?;
        let (counters, starred) = tokio::try_join!(
            self.get_json::<Counters>("/feeds/counters"),
            self.get_json::<IdPage>("/entries?starred=true&status=unread&status=read&limit=1"),
        )?;
        Ok(Counts { feeds: merge_counts(feeds, &counters), starred: Some(starred.total) })
    }

    /// One page of the selection's unread or read entries, oldest or newest first, loaded as you go. `past` is
    /// where the page starts, in unix seconds: entries published after it (oldest first) or before it (newest
    /// first); 0 starts at the beginning. See `next_page` in src/paging.rs for why that and not an offset.
    /// `after` / `before` compare `published_at` strictly and exist since Miniflux 2.0.9; `published_after` /
    /// `published_before` mean the same but need 2.0.49, and an older server would silently ignore them.
    ///
    /// `Selection::Starred` takes no status: starred entries come read and unread alike, so `status` is ignored.
    /// (Not without any status filter: that would bring `removed` ones too.)
    pub async fn entries(
        &self,
        sel: Selection,
        status: EntryStatus,
        order: Order,
        past: i64,
        limit: usize,
    ) -> Result<EntryPage, Error> {
        let limit = limit.clamp(1, MAX_PAGE);
        let (direction, bound) = match order {
            Order::OldestFirst => ("asc", "after"),
            Order::NewestFirst => ("desc", "before"),
        };
        let status = match sel {
            Selection::Starred => "status=unread&status=read".to_string(),
            _ => format!("status={}", status_name(status)),
        };
        let mut path = entries_path(sel, &format!("{status}&order=published_at&direction={direction}&limit={limit}"));
        if past > 0 {
            path.push_str(&format!("&{bound}={past}"));
        }
        self.get_json(&path).await
    }

    /// A page of the entries matching `query`, read and unread, in every feed: the best matches first (Miniflux
    /// ranks them, a little in favour of recent ones), then the newest. A search has no status filter, so reading
    /// doesn't move anything out of it and an offset is safe here.
    pub async fn search(&self, query: &str, offset: usize, limit: usize) -> Result<EntryPage, Error> {
        let limit = limit.clamp(1, MAX_PAGE).to_string();
        let offset = offset.to_string();
        let params = [
            ("search", query),
            ("status", "unread"),
            ("status", "read"),
            ("order", "published_at"),
            ("direction", "desc"),
            ("offset", &offset),
            ("limit", &limit),
        ];
        self.json(self.req(reqwest::Method::GET, "/entries").query(&params)).await
    }

    /// The id of the selection's newest unread entry, 0 if there is none. Entry ids only grow, so anything with
    /// a higher id arrived later: that is how "mark all read" leaves alone what you haven't been shown.
    pub async fn newest_unread_id(&self, sel: Selection) -> Result<i64, Error> {
        let path = entries_path(sel, "status=unread&order=id&direction=desc&limit=1");
        let page: IdPage = self.get_json(&path).await?;
        Ok(page.entries.first().map_or(0, |e| e.id))
    }

    /// How many of the selection's entries with an id up to `up_to` are unread.
    pub async fn unread_count(&self, sel: Selection, up_to: i64) -> Result<i64, Error> {
        if up_to < 1 {
            return Ok(0); // Miniflux ignores before_entry_id=0 and would count everything
        }
        let path = entries_path(sel, &format!("status=unread&before_entry_id={}&limit=1", up_to + 1));
        let page: IdPage = self.get_json(&path).await?;
        Ok(page.total)
    }

    /// Marks the selection's unread entries with an id up to `up_to` read; returns how many.
    /// Walks them by id, which no status change can shift, a page at a time.
    pub async fn mark_read_up_to(&self, sel: Selection, up_to: i64) -> Result<usize, Error> {
        let mut marked = 0;
        if up_to < 1 {
            return Ok(marked);
        }
        let mut after = 0; // after_entry_id=0 means no lower bound
        loop {
            let path = entries_path(
                sel,
                &format!(
                    "status=unread&order=id&direction=asc&limit={ID_PAGE}&before_entry_id={}&after_entry_id={after}",
                    up_to + 1
                ),
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

    pub async fn set_status(&self, ids: &[i64], status: EntryStatus) -> Result<(), Error> {
        if ids.is_empty() {
            return Ok(());
        }
        let body = json!({ "entry_ids": ids, "status": status });
        self.send(self.req(reqwest::Method::PUT, "/entries").json(&body)).await?;
        Ok(())
    }

    /// Stars or unstars an entry. Miniflux only has a toggle (`PUT /v1/entries/{id}/bookmark`), so this asks what
    /// the entry is first and flips it only when that differs: starred meanwhile elsewhere (the web UI, a phone),
    /// a blind toggle would do the opposite of what the app shows.
    pub async fn set_starred(&self, id: i64, starred: bool) -> Result<(), Error> {
        #[derive(Deserialize)]
        struct Starred {
            #[serde(default)]
            starred: bool,
        }
        let now: Starred = self.get_json(&format!("/entries/{id}")).await?;
        if now.starred != starred {
            self.send(self.req(reqwest::Method::PUT, &format!("/entries/{id}/bookmark"))).await?;
        }
        Ok(())
    }

    /// Asks Miniflux to scrape the original page and returns the full content.
    pub async fn fetch_content(&self, id: i64) -> Result<String, Error> {
        let fetched: FetchedContent =
            self.get_json(&format!("/entries/{id}/fetch-content?update_content=true")).await?;
        Ok(fetched.content)
    }

    pub async fn refresh_all(&self) -> Result<(), Error> {
        self.send(self.req(reqwest::Method::PUT, "/feeds/refresh")).await?;
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
            FeedCount { feed, unread: Some(unread), read: Some(read) }
        })
        .collect()
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::dates;
    use crate::error::ErrorKind;
    use crate::feeds::{Category, Entry};
    use std::time::Duration;
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
        let sel: Selection = serde_json::from_str(r#"{"kind":"starred"}"#).unwrap();
        assert_eq!(sel, Selection::Starred);
        assert!(serde_json::from_str::<Selection>(r#"{"kind":"bookmarks"}"#).is_err());
        assert_eq!(entries_path(Selection::All, "limit=1"), "/entries?limit=1");
        assert_eq!(entries_path(Selection::Category { id: 3 }, "limit=1"), "/categories/3/entries?limit=1");
        assert_eq!(entries_path(Selection::Feed { id: 7 }, "limit=1"), "/feeds/7/entries?limit=1");
        assert_eq!(entries_path(Selection::Starred, "limit=1"), "/entries?starred=true&limit=1");
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
        assert!(!e.starred);
        assert_eq!(e.order_ms, 0);
        let mut starred = entry_json(2);
        starred["starred"] = json!(true);
        assert!(serde_json::from_value::<Entry>(starred).unwrap().starred);
        assert_eq!(e.order_millis(), dates::unix_millis("2026-10-24T08:12:00+02:00"), "Miniflux pages by published_at");
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
                FeedCount { feed: feeds[0].clone(), unread: Some(4), read: Some(0) },
                FeedCount { feed: feeds[1].clone(), unread: Some(0), read: Some(7) }
            ]
        );
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

    #[test]
    fn debug_output_leaves_out_the_key() {
        let cfg = config::Miniflux { url: "https://m.example.com".into(), api_key: "mf-secret-123".into() };
        let shown = format!("{:?}", Client::new(reqwest::Client::new(), &cfg));
        assert!(!shown.contains("mf-secret-123") && shown.contains("<redacted>"), "{shown}");
    }

    #[tokio::test]
    async fn check_maps_status_codes_to_messages() {
        let server = MockServer::start().await;
        let c = client(&server);

        let guard = me_answers(&server, ResponseTemplate::new(200).set_body_json(json!({ "username": "ward" }))).await;
        assert_eq!(c.check().await.unwrap(), "ward");
        drop(guard);

        for code in [401, 403] {
            let guard = me_answers(&server, ResponseTemplate::new(code)).await;
            let err = c.check().await.unwrap_err();
            assert_eq!(err.kind(), ErrorKind::Rejected);
            assert_eq!(err.message(), "Miniflux rejected the API key");
            drop(guard);
        }

        let guard = me_answers(&server, ResponseTemplate::new(404)).await;
        let err = c.check().await.unwrap_err();
        assert_eq!(err.kind(), ErrorKind::Setup);
        assert!(err.message().contains("doesn't look like a Miniflux server"));
        drop(guard);

        let guard = me_answers(&server, ResponseTemplate::new(200).set_body_string("<html>not json</html>")).await;
        assert!(c.check().await.unwrap_err().message().contains("doesn't look like a Miniflux server"));
        drop(guard);

        let guard = me_answers(&server, ResponseTemplate::new(502)).await;
        assert_eq!(c.check().await.unwrap_err().message(), "Miniflux answered with HTTP 502");
        drop(guard);
    }

    /// Answers `path` with a redirect to `to`.
    async fn redirects(server: &MockServer, path_to: &str, code: u16, to: &str) {
        Mock::given(path(path_to))
            .respond_with(ResponseTemplate::new(code).insert_header("Location", to))
            .mount(server)
            .await;
    }

    /// The same server by another name, so only the host differs.
    fn by_name(server: &MockServer) -> String {
        server.uri().replace("127.0.0.1", "localhost")
    }

    fn api_client_for(server: &MockServer) -> Client {
        let cfg = config::Miniflux { url: server.uri(), api_key: "secret".into() };
        Client::new(crate::backend::api_client(), &cfg)
    }

    #[tokio::test]
    async fn the_api_key_doesnt_follow_a_redirect_to_another_host() {
        let server = MockServer::start().await;
        let elsewhere = MockServer::start().await;
        Mock::given(path("/login")).respond_with(ResponseTemplate::new(200)).expect(0).mount(&elsewhere).await;
        for code in [301, 302, 303, 307, 308] {
            let guard = Mock::given(path("/v1/me"))
                .respond_with(
                    ResponseTemplate::new(code).insert_header("Location", format!("{}/login", by_name(&elsewhere))),
                )
                .mount_as_scoped(&server)
                .await;
            let err = api_client_for(&server).check().await.unwrap_err();
            assert_eq!(err.kind(), ErrorKind::Setup, "HTTP {code}");
            let message = err.message();
            for part in ["redirected to http://localhost", "API key", "sign-in page"] {
                assert!(message.contains(part), "{part}: {message}");
            }
            drop(guard);
        }
    }

    #[tokio::test]
    async fn the_api_key_doesnt_follow_a_redirect_to_the_same_server_by_another_name() {
        let server = MockServer::start().await;
        redirects(&server, "/v1/me", 302, &format!("{}/login", by_name(&server))).await;
        Mock::given(path("/login")).respond_with(ResponseTemplate::new(200)).expect(0).mount(&server).await;
        assert_eq!(api_client_for(&server).check().await.unwrap_err().kind(), ErrorKind::Setup);
    }

    #[tokio::test]
    async fn a_redirect_while_reading_says_so_rather_than_failing_to_parse() {
        let server = MockServer::start().await;
        redirects(&server, "/v1/entries", 302, "https://auth.example.com/login?rd=secret").await;
        let page = api_client_for(&server)
            .entries(Selection::All, EntryStatus::Unread, Order::OldestFirst, 0, 10)
            .await
            .unwrap_err()
            .message();
        assert!(page.contains("redirected to https://auth.example.com,"), "{page}");
        assert!(!page.contains("rd=secret"), "only the host: {page}");
    }

    #[tokio::test]
    async fn redirects_on_the_same_server_still_work_and_keep_the_key() {
        let server = MockServer::start().await;
        redirects(&server, "/v1/me", 301, "/v1/me/").await;
        Mock::given(path("/v1/me/"))
            .and(header("X-Auth-Token", "secret"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "username": "ward" })))
            .expect(1)
            .mount(&server)
            .await;
        assert_eq!(api_client_for(&server).check().await.unwrap(), "ward");
    }

    #[tokio::test]
    async fn a_redirect_to_another_port_on_the_same_host_is_refused() {
        let server = MockServer::start().await;
        let other = MockServer::start().await;
        Mock::given(path("/login")).respond_with(ResponseTemplate::new(200)).expect(0).mount(&other).await;
        redirects(&server, "/v1/me", 302, &format!("{}/login", other.uri())).await;
        assert_eq!(api_client_for(&server).check().await.unwrap_err().kind(), ErrorKind::Setup);
    }

    #[tokio::test]
    async fn a_redirect_that_goes_round_in_circles_ends() {
        let server = MockServer::start().await;
        redirects(&server, "/v1/me", 302, "/v1/me").await;
        assert_eq!(api_client_for(&server).check().await.unwrap_err().kind(), ErrorKind::Unreachable);
    }

    #[tokio::test]
    async fn check_reports_unreachable_server() {
        // Port 1 is never listening; a freed mock port could be reused by a parallel test.
        let uri = "http://127.0.0.1:1".to_string();
        let cfg = config::Miniflux { url: uri.clone(), api_key: "k".into() };
        let err = Client::new(reqwest::Client::new(), &cfg).check().await.unwrap_err();
        assert_eq!(err.kind(), ErrorKind::Unreachable);
        // The root cause ("Connection refused", worded per OS) follows, so the user can tell DNS from TLS from down.
        let message = err.message();
        let prefix = format!("Could not reach Miniflux at {uri}: ");
        assert!(message.starts_with(&prefix) && message.len() > prefix.len(), "{message}");
        assert!(message.ends_with(". Check the address and that the server is running."), "{message}");
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
        Mock::given(method("GET"))
            .and(path("/v1/entries"))
            .and(query_param("starred", "true"))
            .and(query_param("limit", "1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "total": 4, "entries": [entry_json(9)] })))
            .expect(1)
            .mount(&server)
            .await;
        let counts = client(&server).feed_counts().await.unwrap();
        assert_eq!(
            counts.feeds.iter().map(|c| (c.feed.id, c.unread, c.read)).collect::<Vec<_>>(),
            vec![(1, Some(0), Some(5)), (2, Some(3), Some(0))]
        );
        assert_eq!(counts.starred, Some(4));
        let requests = server.received_requests().await.unwrap();
        let starred = requests.iter().find(|r| r.url.query().is_some_and(|q| q.contains("starred"))).unwrap();
        let statuses: Vec<String> =
            starred.url.query_pairs().filter(|(k, _)| k == "status").map(|(_, v)| v.into_owned()).collect();
        assert_eq!(statuses, ["unread", "read"], "read and unread ones, and no removed ones");
    }

    #[tokio::test]
    async fn starred_entries_are_read_and_unread_newest_first_before_past() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/entries"))
            .and(query_param("starred", "true"))
            .and(query_param("order", "published_at"))
            .and(query_param("direction", "desc"))
            .and(query_param("before", "1761286321"))
            .and(query_param_is_missing("after"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "total": 3, "entries": [entry_json(5)] })))
            .expect(1)
            .mount(&server)
            .await;
        let c = client(&server);
        // The status doesn't narrow the starred list.
        let page = c.entries(Selection::Starred, EntryStatus::Read, Order::NewestFirst, 1_761_286_321, 50).await;
        assert_eq!(page.unwrap().total, 3);
        let requests = server.received_requests().await.unwrap();
        let statuses: Vec<String> =
            requests[0].url.query_pairs().filter(|(k, _)| k == "status").map(|(_, v)| v.into_owned()).collect();
        assert_eq!(statuses, ["unread", "read"]);
    }

    #[tokio::test]
    async fn set_starred_toggles_the_bookmark_only_when_the_server_has_it_the_other_way() {
        let server = MockServer::start().await;
        // The server says it isn't starred.
        Mock::given(method("GET"))
            .and(path("/v1/entries/42"))
            .and(header("X-Auth-Token", "secret"))
            .respond_with(ResponseTemplate::new(200).set_body_json(entry_json(42)))
            .mount(&server)
            .await;
        Mock::given(method("PUT"))
            .and(path("/v1/entries/42/bookmark"))
            .and(header("X-Auth-Token", "secret"))
            .respond_with(ResponseTemplate::new(204))
            .expect(1)
            .mount(&server)
            .await;
        let c = client(&server);
        c.set_starred(42, true).await.unwrap();
        // Unstarring what the server already has unstarred (starred elsewhere and back, say) toggles nothing.
        c.set_starred(42, false).await.unwrap();
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
        let page =
            c.entries(Selection::Category { id: 10 }, EntryStatus::Unread, Order::OldestFirst, 1_761_286_319, 60).await;
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
        let page =
            client(&server).entries(Selection::All, EntryStatus::Unread, Order::OldestFirst, 0, 50).await.unwrap();
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
        let page = c.entries(Selection::Feed { id: 7 }, EntryStatus::Read, Order::NewestFirst, 1_761_286_321, 100_000);
        assert_eq!(page.await.unwrap().total, 41);
    }

    #[tokio::test]
    async fn unread_entries_newest_first_are_a_page_published_before_past() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/entries"))
            .and(query_param("status", "unread"))
            .and(query_param("direction", "desc"))
            .and(query_param("before", "1761286321"))
            .and(query_param_is_missing("after"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "total": 2, "entries": [entry_json(3)] })))
            .expect(1)
            .mount(&server)
            .await;
        let c = client(&server);
        let page = c.entries(Selection::All, EntryStatus::Unread, Order::NewestFirst, 1_761_286_321, 50).await;
        assert_eq!(page.unwrap().total, 2);
    }

    #[tokio::test]
    async fn search_looks_through_read_and_unread_entries_by_offset() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/entries"))
            .and(query_param("search", "rust & “async”"))
            .and(query_param("offset", "50"))
            .and(query_param("limit", "50"))
            .and(query_param("direction", "desc"))
            .and(query_param_is_missing("after"))
            .and(query_param_is_missing("before"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "total": 51, "entries": [entry_json(9)] })))
            .expect(1)
            .mount(&server)
            .await;
        let page = client(&server).search("rust & “async”", 50, 50).await.unwrap();
        assert_eq!((page.total, page.entries[0].id), (51, 9));
        let requests = server.received_requests().await.unwrap();
        let statuses: Vec<String> =
            requests[0].url.query_pairs().filter(|(k, _)| k == "status").map(|(_, v)| v.into_owned()).collect();
        assert_eq!(statuses, ["unread", "read"]);
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
        assert_eq!(c.newest_unread_id(Selection::Feed { id: 7 }).await.unwrap(), 81);
        drop(guard);
        Mock::given(method("GET"))
            .and(path("/v1/feeds/7/entries"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "total": 0, "entries": [] })))
            .mount(&server)
            .await;
        assert_eq!(c.newest_unread_id(Selection::Feed { id: 7 }).await.unwrap(), 0);
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
        assert_eq!(c.unread_count(Selection::Category { id: 3 }, 80).await.unwrap(), 57);
        assert_eq!(c.unread_count(Selection::Category { id: 3 }, 0).await.unwrap(), 0);
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
        assert_eq!(client(&server).mark_read_up_to(Selection::All, 1000).await.unwrap(), ID_PAGE + 2);
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
        assert_eq!(c.mark_read_up_to(Selection::All, 1000).await.unwrap(), ID_PAGE);
        assert_eq!(c.mark_read_up_to(Selection::All, 0).await.unwrap(), 0); // no request
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
        c.set_status(&[4, 5], EntryStatus::Read).await.unwrap();
        c.set_status(&[], EntryStatus::Read).await.unwrap();
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
        assert_eq!(c.fetch_content(42).await.unwrap(), "<p>full</p>");
        c.refresh_all().await.unwrap();
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
        assert_eq!(err.kind(), ErrorKind::Server);
        let message = err.message();
        assert!(message.starts_with("Miniflux 500 Internal Server Error: xxx"), "{message}");
        assert!(message.chars().count() < 350, "{}", message.len());
        assert!(message.ends_with('…'));
    }

    #[tokio::test]
    async fn a_key_turned_down_while_reading_is_rejected_not_a_server_error() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/feeds"))
            .respond_with(ResponseTemplate::new(401).set_body_string("Access Unauthorized"))
            .mount(&server)
            .await;
        let err = client(&server).feed_counts().await.unwrap_err();
        assert_eq!(err.kind(), ErrorKind::Rejected);
        assert_eq!(err.message(), "Miniflux turned down the API key (401 Unauthorized). Check it in Settings.");
    }

    #[tokio::test]
    async fn an_answer_cut_off_by_a_timeout_is_unreachable_not_unusable() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/feeds"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!([])).set_delay(Duration::from_secs(5)))
            .mount(&server)
            .await;
        let http = reqwest::Client::builder().timeout(Duration::from_millis(200)).build().unwrap();
        let cfg = config::Miniflux { url: server.uri(), api_key: "secret".into() };
        let err = Client::new(http, &cfg).feed_counts().await.unwrap_err();
        assert_eq!(err.kind(), ErrorKind::Unreachable, "{}", err.message());
    }

    #[tokio::test]
    async fn an_unusable_answer_names_what_was_wrong() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/feeds"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!([{ "id": 1 }])))
            .mount(&server)
            .await;
        let err = client(&server).feed_counts().await.unwrap_err();
        assert_eq!(err.kind(), ErrorKind::Server);
        assert!(err.message().starts_with("Unexpected answer from Miniflux: "), "{}", err.message());
        assert!(err.message().contains("title"), "names the missing field: {}", err.message());
    }
}
