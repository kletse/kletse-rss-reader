//! The feed server the app reads from: Miniflux's own API, or the Google Reader API. backend.rs calls this and
//! doesn't care which; each method hands the call to the client for the configured kind.

use crate::error::Error;
use crate::feeds::{
    self, Applied, Articles, Counts, Destination, EntryPage, EntryStatus, FeedChange, FeedList, Selection,
};
use crate::greader;
use crate::miniflux;
use crate::paging::Order;
use std::time::Duration;

/// How long an import may take, against the 30 s of everything else. FreshRSS fetches every feed before it answers:
/// 3.2 s for four fast local ones, and it stops itself after 300 s, so a long list from slow sites can take that long.
const IMPORT_TIMEOUT: Duration = Duration::from_secs(600);

/// No answer in time, or a proxy in front that gave up waiting: the server may well get there.
fn still_working(e: Error) -> Error {
    let waited = match &e {
        Error::Unreachable { source, .. } => source.is_timeout(),
        Error::Http { status, .. } => status.as_u16() == 502 || status.as_u16() == 504,
        _ => false,
    };
    if waited { Error::Refused(feeds::IMPORT_STILL_WORKING.into()) } else { e }
}

/// `still_working`, for subscribing: FreshRSS fetches the feed before it answers.
fn may_yet_arrive(e: Error) -> Error {
    match still_working(e) {
        Error::Refused(message) if message == feeds::IMPORT_STILL_WORKING => {
            Error::Refused(feeds::ADD_STILL_WORKING.into())
        }
        e => e,
    }
}

#[derive(Debug)]
pub enum Server {
    Miniflux(miniflux::Client),
    GReader(greader::Client),
}

impl Server {
    /// Checks the address and credentials; returns the username.
    pub async fn check(&self) -> Result<String, Error> {
        match self {
            Server::Miniflux(c) => c.check().await,
            Server::GReader(c) => c.check().await,
        }
    }

    /// Every feed with its counts, and how many entries are starred.
    pub async fn feed_counts(&self) -> Result<Counts, Error> {
        match self {
            Server::Miniflux(c) => c.feed_counts().await,
            Server::GReader(c) => c.feed_counts().await,
        }
    }

    /// A page of the selection's entries with `status`; for `Selection::Starred`, read and unread alike: the
    /// status is ignored there.
    pub async fn entries(
        &self,
        sel: Selection,
        status: EntryStatus,
        order: Order,
        past: i64,
        limit: usize,
    ) -> Result<EntryPage, Error> {
        match self {
            Server::Miniflux(c) => c.entries(sel, status, order, past, limit).await,
            Server::GReader(c) => c.entries(sel, status, order, past, limit).await,
        }
    }

    pub async fn search(&self, query: &str, offset: usize, limit: usize) -> Result<EntryPage, Error> {
        match self {
            Server::Miniflux(c) => c.search(query, offset, limit).await,
            Server::GReader(c) => c.search(query, offset, limit).await,
        }
    }

    pub async fn newest_unread_id(&self, sel: Selection) -> Result<i64, Error> {
        match self {
            Server::Miniflux(c) => c.newest_unread_id(sel).await,
            Server::GReader(c) => c.newest_unread_id(sel).await,
        }
    }

    pub async fn unread_count(&self, sel: Selection, up_to: i64) -> Result<i64, Error> {
        match self {
            Server::Miniflux(c) => c.unread_count(sel, up_to).await,
            Server::GReader(c) => c.unread_count(sel, up_to).await,
        }
    }

    pub async fn mark_read_up_to(&self, sel: Selection, up_to: i64) -> Result<usize, Error> {
        match self {
            Server::Miniflux(c) => c.mark_read_up_to(sel, up_to).await,
            Server::GReader(c) => c.mark_read_up_to(sel, up_to).await,
        }
    }

    pub async fn set_status(&self, ids: &[i64], status: EntryStatus) -> Result<(), Error> {
        match self {
            Server::Miniflux(c) => c.set_status(ids, status).await,
            Server::GReader(c) => c.set_status(ids, status).await,
        }
    }

    /// Stars or unstars an entry. Call it only to change it: Miniflux can only toggle.
    pub async fn set_starred(&self, id: i64, starred: bool) -> Result<(), Error> {
        match self {
            Server::Miniflux(c) => c.set_starred(id, starred).await,
            Server::GReader(c) => c.set_starred(id, starred).await,
        }
    }

    pub async fn fetch_content(&self, id: i64) -> Result<String, Error> {
        match self {
            Server::Miniflux(c) => c.fetch_content(id).await,
            Server::GReader(c) => c.fetch_content(id).await,
        }
    }

    pub async fn refresh_all(&self) -> Result<(), Error> {
        match self {
            Server::Miniflux(c) => c.refresh_all().await,
            Server::GReader(c) => c.refresh_all().await,
        }
    }

    /// Hands the server an OPML file. The route goes by the kind of server and never by what a route answers: Miniflux
    /// answers FreshRSS's export route with 200 and `[]`.
    pub async fn import_opml(&self, opml: &[u8]) -> Result<Articles, Error> {
        self.import_within(opml, IMPORT_TIMEOUT).await
    }

    async fn import_within(&self, opml: &[u8], timeout: Duration) -> Result<Articles, Error> {
        match self {
            Server::Miniflux(c) => {
                c.import_opml(opml, timeout).await.map_err(still_working)?;
                // Without this there are no articles until its next round, up to an hour off.
                Ok(c.refresh_all().await.map_or_else(|e| Articles::NotAsked(e.message()), |()| Articles::OnTheirWay))
            }
            Server::GReader(c) if c.is_freshrss() => {
                c.import_opml(opml, timeout).await.map_err(still_working)?;
                Ok(Articles::Fetched)
            }
            Server::GReader(_) => Err(Error::Refused(feeds::IMPORT_REFUSED.into())),
        }
    }

    /// Every feed as OPML, and how many had to be left out of it. Any Google Reader server lists its feeds, so one with
    /// no export of its own still gets one, made here.
    pub async fn export_opml(&self) -> Result<(Vec<u8>, usize), Error> {
        match self {
            Server::Miniflux(c) => Ok((c.export_opml().await?, 0)),
            Server::GReader(c) if c.is_freshrss() => Ok((c.export_opml().await?, 0)),
            Server::GReader(c) => c.list_as_opml().await,
        }
    }

    /// Every feed and every category, for changing them.
    pub async fn feed_list(&self) -> Result<FeedList, Error> {
        match self {
            Server::Miniflux(c) => c.feed_list().await,
            Server::GReader(c) if c.is_freshrss() => c.feed_list().await,
            Server::GReader(_) => Err(Error::Refused(feeds::MANAGE_REFUSED.into())),
        }
    }

    /// Makes a change. On FreshRSS the answer says little: `Backend` looks at the list afterwards.
    pub async fn change(&self, change: &FeedChange) -> Result<Applied, Error> {
        match self {
            Server::Miniflux(c) => Self::change_miniflux(c, change).await,
            Server::GReader(c) if c.is_freshrss() => Self::change_freshrss(c, change).await,
            Server::GReader(_) => Err(Error::Refused(feeds::MANAGE_REFUSED.into())),
        }
    }

    async fn change_miniflux(c: &miniflux::Client, change: &FeedChange) -> Result<Applied, Error> {
        match change {
            FeedChange::Add { url, category } => {
                let id = category.as_ref().map(|category| category.id);
                let feed = Some(c.add_feed(url, id).await.map_err(may_yet_arrive)?);
                return Ok(Applied { feed, category: category.clone() });
            }
            FeedChange::RenameFeed { id, title } => c.rename_feed(*id, title).await?,
            FeedChange::MoveFeed { id, to } => {
                let category = match to {
                    Destination::Existing(category) => category.clone(),
                    Destination::New(title) => c.create_category(title).await?,
                };
                c.move_feed(*id, category.id).await?;
                return Ok(Applied { feed: None, category: Some(category) });
            }
            FeedChange::RemoveFeed { id } => c.remove_feed(*id).await?,
            FeedChange::RenameCategory { category, title } => {
                c.rename_category(category.id, title).await?;
                let category = feeds::Category { id: category.id, title: title.clone() };
                return Ok(Applied { feed: None, category: Some(category) });
            }
            FeedChange::DeleteCategory { category } => c.delete_category(category.id).await?,
        }
        Ok(Applied::default())
    }

    async fn change_freshrss(c: &greader::Client, change: &FeedChange) -> Result<Applied, Error> {
        match change {
            FeedChange::Add { url, category } => {
                let title = category.as_ref().map(|category| category.title.as_str());
                let feed = Some(c.add_feed(url, title).await.map_err(may_yet_arrive)?);
                return Ok(Applied { feed, category: category.clone() });
            }
            FeedChange::RenameFeed { id, title } => c.rename_feed(*id, title).await?,
            FeedChange::MoveFeed { id, to } => c.move_feed(*id, to.title()).await?,
            FeedChange::RemoveFeed { id } => c.remove_feed(*id).await?,
            FeedChange::RenameCategory { category, title } => {
                let category = c.rename_category(&category.title, title).await?;
                return Ok(Applied { feed: None, category: Some(category) });
            }
            FeedChange::DeleteCategory { category } => c.delete_category(&category.title).await?,
        }
        Ok(Applied::default())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::config;
    use crate::error::{Error, ErrorKind};
    use crate::feeds::Category;
    use wiremock::matchers::{body_string, header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const OPML: &str = "<opml version=\"2.0\"><body><outline xmlUrl=\"https://a.example/feed\"/></body></opml>";

    fn miniflux(server: &MockServer) -> Server {
        let cfg = config::Miniflux { url: server.uri(), api_key: "key".into() };
        Server::Miniflux(miniflux::Client::new(crate::backend::api_client(), &cfg))
    }

    fn greader_at(url: String) -> Server {
        let cfg = config::GReader { url, username: "me".into(), password: "pw".into() };
        Server::GReader(greader::Client::new(crate::backend::api_client(), &cfg))
    }

    /// At `/api/greader.php`, signed in, as FreshRSS is.
    async fn freshrss(server: &MockServer) -> Server {
        signed_in(server, "/api/greader.php").await;
        greader_at(format!("{}/api/greader.php", server.uri()))
    }

    async fn signed_in(server: &MockServer, at: &str) {
        Mock::given(method("POST"))
            .and(path(format!("{at}/accounts/ClientLogin")))
            .respond_with(ResponseTemplate::new(200).set_body_string("SID=x\nLSID=null\nAuth=token\n"))
            .mount(server)
            .await;
    }

    fn calls_to(requests: &[wiremock::Request], to: &str) -> usize {
        requests.iter().filter(|r| r.url.path().ends_with(to)).count()
    }

    #[tokio::test]
    async fn a_miniflux_import_sends_the_file_as_it_is_and_tells_it_to_fetch() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/import"))
            .and(header("X-Auth-Token", "key"))
            .and(header("Content-Type", "text/xml"))
            .and(body_string(OPML))
            .respond_with(
                ResponseTemplate::new(201)
                    .insert_header("Content-Type", "application/json")
                    .set_body_string(r#"{"message":"Feeds imported successfully"}"#),
            )
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("PUT"))
            .and(path("/v1/feeds/refresh"))
            .respond_with(ResponseTemplate::new(204))
            .expect(1)
            .mount(&server)
            .await;
        assert_eq!(miniflux(&server).import_opml(OPML.as_bytes()).await.unwrap(), Articles::OnTheirWay);
        let requests = server.received_requests().await.unwrap();
        assert!(requests[0].url.path().ends_with("/import") && requests[1].url.path().ends_with("/refresh"));
    }

    #[tokio::test]
    async fn the_file_stays_taken_when_miniflux_wont_refresh() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/import"))
            .respond_with(ResponseTemplate::new(201).set_body_string(r#"{"message":"Feeds imported successfully"}"#))
            .mount(&server)
            .await;
        Mock::given(method("PUT"))
            .and(path("/v1/feeds/refresh"))
            .respond_with(ResponseTemplate::new(500).set_body_string("busy"))
            .mount(&server)
            .await;
        let Articles::NotAsked(why) = miniflux(&server).import_opml(OPML.as_bytes()).await.unwrap() else { panic!() };
        assert_eq!(why, "Miniflux 500 Internal Server Error: busy");
    }

    #[tokio::test]
    async fn a_miniflux_that_refuses_the_file_says_why_and_isnt_told_to_fetch() {
        let server = MockServer::start().await;
        let refused = r#"{"error_message":"opml: unable to parse document: XML syntax error on line 1: expected element name after <"}"#;
        Mock::given(method("POST"))
            .and(path("/v1/import"))
            .respond_with(ResponseTemplate::new(500).set_body_string(refused))
            .mount(&server)
            .await;
        let error = miniflux(&server).import_opml(OPML.as_bytes()).await.unwrap_err();
        assert!(error.message().contains("unable to parse document"), "{}", error.message());
        assert_eq!(calls_to(&server.received_requests().await.unwrap(), "/refresh"), 0);
    }

    #[tokio::test]
    async fn a_freshrss_import_needs_only_the_login_and_its_ok_is_all_it_says() {
        let server = MockServer::start().await;
        let client = freshrss(&server).await;
        Mock::given(method("POST"))
            .and(path("/api/greader.php/reader/api/0/subscription/import"))
            .and(header("Authorization", "GoogleLogin auth=token"))
            .and(header("Content-Type", "text/xml"))
            .and(body_string(OPML))
            .respond_with(ResponseTemplate::new(200).set_body_string("OK"))
            .expect(1)
            .mount(&server)
            .await;
        assert_eq!(client.import_opml(OPML.as_bytes()).await.unwrap(), Articles::Fetched);
        assert_eq!(calls_to(&server.received_requests().await.unwrap(), "/token"), 0);
    }

    #[tokio::test]
    async fn a_freshrss_that_refuses_the_file_gets_words_for_its_bare_400() {
        let server = MockServer::start().await;
        let client = freshrss(&server).await;
        Mock::given(method("POST"))
            .and(path("/api/greader.php/reader/api/0/subscription/import"))
            .respond_with(ResponseTemplate::new(400).set_body_string("Bad Request!"))
            .expect(1)
            .mount(&server)
            .await;
        let message = client.import_opml(OPML.as_bytes()).await.unwrap_err().message();
        assert!(message.starts_with("FreshRSS turned the file down"), "{message}");
    }

    #[tokio::test]
    async fn an_answer_other_than_ok_isnt_taken_for_one() {
        let server = MockServer::start().await;
        let client = freshrss(&server).await;
        Mock::given(method("POST"))
            .and(path("/api/greader.php/reader/api/0/subscription/import"))
            .respond_with(ResponseTemplate::new(200).set_body_string("[]"))
            .mount(&server)
            .await;
        assert_eq!(client.import_opml(OPML.as_bytes()).await.unwrap_err().kind(), ErrorKind::Server);
    }

    #[tokio::test]
    async fn an_import_that_outlasts_the_wait_says_the_server_may_still_be_at_it() {
        let server = MockServer::start().await;
        let client = freshrss(&server).await;
        Mock::given(method("POST"))
            .and(path("/api/greader.php/reader/api/0/subscription/import"))
            .respond_with(ResponseTemplate::new(200).set_body_string("OK").set_delay(Duration::from_secs(5)))
            .mount(&server)
            .await;
        let error = client.import_within(OPML.as_bytes(), Duration::from_millis(50)).await.unwrap_err();
        assert_eq!(error.message(), feeds::IMPORT_STILL_WORKING);
    }

    #[tokio::test]
    async fn a_proxy_that_gave_up_says_the_same() {
        let server = MockServer::start().await;
        let client = freshrss(&server).await;
        Mock::given(method("POST"))
            .and(path("/api/greader.php/reader/api/0/subscription/import"))
            .respond_with(ResponseTemplate::new(504).set_body_string("<h1>504 Gateway Time-out</h1>"))
            .mount(&server)
            .await;
        assert_eq!(client.import_opml(OPML.as_bytes()).await.unwrap_err().message(), feeds::IMPORT_STILL_WORKING);
    }

    #[test]
    fn the_wait_is_longer_than_what_any_other_call_gets() {
        const { assert!(IMPORT_TIMEOUT.as_secs() > 300, "FreshRSS gives itself 300 s, and needs to get to answer") };
    }

    #[tokio::test]
    async fn miniflux_through_its_integration_isnt_sent_a_file_at_all() {
        let server = MockServer::start().await;
        signed_in(&server, "").await;
        let error = greader_at(server.uri()).import_opml(OPML.as_bytes()).await.unwrap_err();
        assert_eq!(error.message(), feeds::IMPORT_REFUSED);
        assert!(server.received_requests().await.unwrap().is_empty(), "the kind of server decides, not an answer");
    }

    #[tokio::test]
    async fn its_export_comes_from_the_list_rather_than_the_route_that_answers_200() {
        let server = MockServer::start().await;
        signed_in(&server, "").await;
        // As Miniflux 2.3.5 answers: 200 and an empty JSON list.
        Mock::given(method("GET"))
            .and(path("/reader/api/0/subscription/export"))
            .respond_with(ResponseTemplate::new(200).set_body_string("[]"))
            .expect(0)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/reader/api/0/subscription/list"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({ "subscriptions": [
                { "id": "feed/1", "title": "Alpha & co", "url": "http://feeds.example/alpha.xml",
                  "htmlUrl": "http://feeds.example/alpha/", "iconUrl": "",
                  "categories": [{ "id": "user/1/label/News", "label": "News", "type": "folder" }] },
                { "id": "feed/2", "title": "No address", "categories": [] },
                { "id": "feed/https://b.example/feed", "title": "In its id", "categories": [] },
            ]})))
            .mount(&server)
            .await;
        let (bytes, left_out) = greader_at(server.uri()).export_opml().await.unwrap();
        assert_eq!(left_out, 1);
        let feeds = crate::opml::feeds_in(&bytes).unwrap();
        assert_eq!(feeds, ["http://feeds.example/alpha.xml", "https://b.example/feed"]);
        assert!(String::from_utf8(bytes).unwrap().contains("<outline text=\"News\">"));
    }

    #[tokio::test]
    async fn the_two_servers_with_an_export_use_their_own() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/export"))
            .and(header("X-Auth-Token", "key"))
            .respond_with(ResponseTemplate::new(200).set_body_string(OPML))
            .expect(1)
            .mount(&server)
            .await;
        assert_eq!(miniflux(&server).export_opml().await.unwrap(), (OPML.as_bytes().to_vec(), 0));

        let client = freshrss(&server).await;
        Mock::given(method("GET"))
            .and(path("/api/greader.php/reader/api/0/subscription/export"))
            .respond_with(ResponseTemplate::new(200).set_body_string(OPML))
            .expect(1)
            .mount(&server)
            .await;
        assert_eq!(client.export_opml().await.unwrap(), (OPML.as_bytes().to_vec(), 0));
    }

    // ---- Changing feeds: each answer is what the server gave when it was run, Miniflux 2.3.5 and FreshRSS 1.30.4 ----

    const FEED: &str = "http://feeds.example/epsilon.xml";
    const FRESH: &str = "/api/greader.php/reader/api/0";

    fn category(id: i64, title: &str) -> Category {
        Category { id, title: title.into() }
    }

    async fn miniflux_says(server: &MockServer, verb: &str, at: &str, code: u16, body: &str) {
        Mock::given(method(verb))
            .and(path(at))
            .and(header("X-Auth-Token", "key"))
            .respond_with(ResponseTemplate::new(code).set_body_string(body))
            .mount(server)
            .await;
    }

    async fn sent(server: &MockServer, to: &str) -> Vec<String> {
        let requests = server.received_requests().await.unwrap();
        let bodies = requests.iter().filter(|r| r.url.path().ends_with(to));
        bodies.map(|r| String::from_utf8_lossy(&r.body).into_owned()).collect()
    }

    /// As one form field arrives, for comparing.
    fn field(name: &str, value: &str) -> String {
        format!("{name}={}", url::form_urlencoded::byte_serialize(value.as_bytes()).collect::<String>())
    }

    async fn freshrss_says(server: &MockServer, route: &str, code: u16, body: &str) {
        Mock::given(method("POST"))
            .and(path(format!("{FRESH}/{route}")))
            .respond_with(ResponseTemplate::new(code).set_body_string(body))
            .mount(server)
            .await;
    }

    /// FreshRSS, signed in, with a token for edits, and these subscriptions listed.
    async fn freshrss_with(server: &MockServer, subscriptions: serde_json::Value) -> Server {
        let client = freshrss(server).await;
        Mock::given(method("GET"))
            .and(path(format!("{FRESH}/token")))
            .respond_with(ResponseTemplate::new(200).set_body_string("token"))
            .mount(server)
            .await;
        Mock::given(method("GET"))
            .and(path(format!("{FRESH}/subscription/list")))
            .respond_with(ResponseTemplate::new(200).set_body_json(subscriptions))
            .mount(server)
            .await;
        Mock::given(method("GET"))
            .and(path(format!("{FRESH}/unread-count")))
            .respond_with(ResponseTemplate::new(200).set_body_string(r#"{"max":0,"unreadcounts":[]}"#))
            .mount(server)
            .await;
        Mock::given(method("GET"))
            .and(path(format!("{FRESH}/stream/items/ids")))
            .respond_with(ResponseTemplate::new(200).set_body_string(r#"{"itemRefs":[]}"#))
            .mount(server)
            .await;
        Mock::given(method("GET"))
            .and(path(format!("{FRESH}/tag/list")))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({ "tags": [
                { "id": "user/-/state/com.google/starred" },
                { "id": "user/-/label/Auto Created", "type": "folder" },
                { "id": "user/-/label/News", "type": "folder" },
                { "id": "user/-/label/Uncategorized", "type": "folder" },
            ]})))
            .mount(server)
            .await;
        client
    }

    fn one_feed() -> serde_json::Value {
        serde_json::json!({ "subscriptions": [{
            "id": "feed/10", "title": "Epsilon", "url": FEED, "htmlUrl": "", "iconUrl": "",
            "categories": [{ "id": "user/-/label/News", "label": "News" }],
        }]})
    }

    fn add(url: &str) -> FeedChange {
        FeedChange::Add { url: url.into(), category: None }
    }

    #[tokio::test]
    async fn miniflux_subscribes_with_the_category_and_says_which_feed() {
        let server = MockServer::start().await;
        miniflux_says(&server, "POST", "/v1/feeds", 201, r#"{"feed_id":5}"#).await;
        let change = FeedChange::Add { url: FEED.into(), category: Some(category(1, "All")) };
        let applied = miniflux(&server).change(&change).await.unwrap();
        assert_eq!(applied.feed, Some(5));
        assert_eq!(sent(&server, "/v1/feeds").await, [format!(r#"{{"category_id":1,"feed_url":"{FEED}"}}"#)]);
    }

    /// The name of the failure, the status and body Miniflux gave, and what's shown.
    #[tokio::test]
    async fn miniflux_says_why_it_wont_subscribe_in_its_own_words() {
        for (code, body, shown) in [
            (400, r#"{"error_message":"This feed already exists."}"#, "Miniflux says: This feed already exists."),
            (400, r#"{"error_message":"Invalid feed URL."}"#, "Miniflux says: Invalid feed URL."),
            (
                500,
                r#"{"error_message":"parser: unable to detect feed format"}"#,
                "Miniflux says: parser: unable to detect feed format",
            ),
            (
                500,
                r#"{"error_message":"fetcher: resource not found (404 status code)"}"#,
                "Miniflux says: fetcher: resource not found (404 status code)",
            ),
            (
                500,
                r#"{"error_message":"fetcher: Get \"http://nohost.invalid/x.xml\": dial tcp: lookup nohost.invalid on 127.0.0.11:53: no such host"}"#,
                "Miniflux says: fetcher: Get \"http://nohost.invalid/x.xml\": dial tcp: lookup nohost.invalid on 127.0.0.11:53: no such host",
            ),
        ] {
            let server = MockServer::start().await;
            miniflux_says(&server, "GET", "/v1/categories", 200, r#"[{"id":1,"title":"All"}]"#).await;
            miniflux_says(&server, "POST", "/v1/feeds", code, body).await;
            assert_eq!(miniflux(&server).change(&add(FEED)).await.unwrap_err().message(), shown);
        }
    }

    #[tokio::test]
    async fn an_answer_with_no_reason_in_it_stays_as_it_came() {
        let server = MockServer::start().await;
        miniflux_says(&server, "GET", "/v1/categories", 200, r#"[{"id":1,"title":"All"}]"#).await;
        miniflux_says(&server, "POST", "/v1/feeds", 503, "<h1>Service Unavailable</h1>").await;
        let error = miniflux(&server).change(&add(FEED)).await.unwrap_err();
        assert_eq!(error.message(), "Miniflux 503 Service Unavailable: <h1>Service Unavailable</h1>");
    }

    #[tokio::test]
    async fn with_no_category_left_miniflux_gets_one_before_the_feed() {
        // Its last category can be deleted, and then a subscribe with none is 400 "This category does not exist".
        let server = MockServer::start().await;
        miniflux_says(&server, "GET", "/v1/categories", 200, "[]").await;
        miniflux_says(&server, "POST", "/v1/categories", 201, r#"{"id":9,"title":"All"}"#).await;
        miniflux_says(&server, "POST", "/v1/feeds", 201, r#"{"feed_id":5}"#).await;
        assert_eq!(miniflux(&server).change(&add(FEED)).await.unwrap().feed, Some(5));
        assert_eq!(sent(&server, "/v1/categories").await.last().unwrap(), r#"{"title":"All"}"#);
        assert_eq!(sent(&server, "/v1/feeds").await, [format!(r#"{{"category_id":9,"feed_url":"{FEED}"}}"#)]);
    }

    #[tokio::test]
    async fn miniflux_renames_moves_removes_and_deletes_by_its_own_ids() {
        let server = MockServer::start().await;
        let answer = r#"{"id":5,"title":"Renamed"}"#;
        miniflux_says(&server, "PUT", "/v1/feeds/5", 201, answer).await;
        miniflux_says(&server, "DELETE", "/v1/feeds/5", 204, "").await;
        miniflux_says(&server, "PUT", "/v1/categories/4", 201, r#"{"id":4,"title":"Spike2"}"#).await;
        miniflux_says(&server, "DELETE", "/v1/categories/4", 204, "").await;
        miniflux_says(&server, "POST", "/v1/categories", 201, r#"{"id":7,"title":"Fresh"}"#).await;
        let client = miniflux(&server);
        client.change(&FeedChange::RenameFeed { id: 5, title: "Renamed".into() }).await.unwrap();
        let to = Destination::Existing(category(4, "Spike"));
        let moved = client.change(&FeedChange::MoveFeed { id: 5, to }).await.unwrap();
        assert_eq!(moved.category, Some(category(4, "Spike")));
        let to = Destination::New("Fresh".into());
        let moved = client.change(&FeedChange::MoveFeed { id: 5, to }).await.unwrap();
        assert_eq!(moved.category, Some(category(7, "Fresh")), "made first, as a category needs a feed nowhere");
        assert_eq!(
            sent(&server, "/v1/feeds/5").await,
            [r#"{"title":"Renamed"}"#, r#"{"category_id":4}"#, r#"{"category_id":7}"#]
        );
        let renamed = FeedChange::RenameCategory { category: category(4, "Spike"), title: "Spike2".into() };
        assert_eq!(client.change(&renamed).await.unwrap().category, Some(category(4, "Spike2")), "the same id");
        client.change(&FeedChange::RemoveFeed { id: 5 }).await.unwrap();
        client.change(&FeedChange::DeleteCategory { category: category(4, "Spike") }).await.unwrap();
        let methods: Vec<String> =
            server.received_requests().await.unwrap().iter().map(|r| r.method.to_string()).collect();
        assert_eq!(methods.iter().filter(|m| m.as_str() == "DELETE").count(), 2);
    }

    #[tokio::test]
    async fn what_miniflux_refuses_in_a_change_comes_in_its_own_words_too() {
        let server = MockServer::start().await;
        miniflux_says(&server, "PUT", "/v1/feeds/5", 400, r#"{"error_message":"The feed title cannot be empty."}"#)
            .await;
        let error =
            miniflux(&server).change(&FeedChange::RenameFeed { id: 5, title: String::new() }).await.unwrap_err();
        assert_eq!(error.message(), "Miniflux says: The feed title cannot be empty.");
        let deleted = FeedChange::RemoveFeed { id: 99999 };
        miniflux_says(&server, "DELETE", "/v1/feeds/99999", 404, r#"{"error_message":"resource not found"}"#).await;
        assert_eq!(
            miniflux(&server).change(&deleted).await.unwrap_err().message(),
            "Miniflux says: resource not found"
        );
    }

    #[tokio::test]
    async fn miniflux_lists_every_category_also_those_with_no_feeds() {
        let server = MockServer::start().await;
        miniflux_says(&server, "GET", "/v1/feeds", 200, "[]").await;
        miniflux_says(&server, "GET", "/v1/feeds/counters", 200, r#"{"reads":{},"unreads":{}}"#).await;
        miniflux_says(&server, "GET", "/v1/entries", 200, r#"{"total":0,"entries":[]}"#).await;
        let all = r#"[{"id":1,"title":"All","user_id":1,"hide_globally":false},{"id":2,"title":"News","user_id":1}]"#;
        miniflux_says(&server, "GET", "/v1/categories", 200, all).await;
        let list = miniflux(&server).feed_list().await.unwrap();
        assert_eq!(list.categories, [category(1, "All"), category(2, "News")]);
        assert_eq!(list.fixed, None);
    }

    #[tokio::test]
    async fn freshrss_lists_empty_categories_from_its_tags_and_knows_its_default() {
        let server = MockServer::start().await;
        let list = freshrss_with(&server, one_feed()).await.feed_list().await.unwrap();
        let titles: Vec<&str> = list.categories.iter().map(|c| c.title.as_str()).collect();
        assert_eq!(titles, ["Auto Created", "News", "Uncategorized"], "not the state tags");
        assert_eq!(list.counts.feeds[0].feed.category, list.categories[1], "the same id both ways");
        assert_eq!(list.fixed, Some(list.categories[2].id));
    }

    #[tokio::test]
    async fn freshrss_subscribes_with_quickadd_and_a_label_comes_after() {
        let server = MockServer::start().await;
        let client = freshrss_with(&server, one_feed()).await;
        let answer = r#"{"numResults":1,"query":"http://feeds.example/epsilon.xml","streamId":"feed/10","streamName":"Epsilon Feed"}"#;
        freshrss_says(&server, "subscription/quickadd", 200, answer).await;
        freshrss_says(&server, "subscription/edit", 200, "OK").await;
        let change = FeedChange::Add { url: FEED.into(), category: Some(category(1, "News")) };
        let applied = client.change(&change).await.unwrap();
        assert_eq!(applied.feed, Some(greader::stream_id("feed/10")));
        let requests = server.received_requests().await.unwrap();
        let quickadd = requests.iter().find(|r| r.url.path().ends_with("/quickadd")).unwrap();
        assert_eq!(quickadd.url.query_pairs().find(|(k, _)| k == "quickadd").unwrap().1, FEED);
        assert_eq!(
            sent(&server, "/subscription/edit").await,
            [format!("ac=edit&{}&{}&T=token", field("s", "feed/10"), field("a", "user/-/label/News"))]
        );
    }

    #[tokio::test]
    async fn freshrss_says_why_it_wont_subscribe_in_a_200() {
        for (body, shown) in [
            (
                r#"{"numResults":0,"error":"Already subscribed! http://feeds.example/epsilon.xml"}"#,
                "FreshRSS says: Already subscribed! http://feeds.example/epsilon.xml",
            ),
            (
                r#"{"numResults":0,"error":"`not a url` is not a valid URL"}"#,
                "FreshRSS says: `not a url` is not a valid URL",
            ),
            (
                r#"{"numResults":0,"error":"Failed to resolve domain: http://nohost.invalid/x.xml [http://nohost.invalid/x.xml]"}"#,
                "FreshRSS says: Failed to resolve domain: http://nohost.invalid/x.xml [http://nohost.invalid/x.xml]",
            ),
            (
                r#"{"numResults":0,"error":"Retrieved unsupported status code \"404\" [http://feeds.example/nope.xml]"}"#,
                "FreshRSS says: Retrieved unsupported status code \"404\" [http://feeds.example/nope.xml]",
            ),
            (
                r#"{"numResults":0,"error":"A feed could not be found at `http://feeds.example/page.html`; the status code is `200` and content-type is `text/html` [http://feeds.example/page.html]"}"#,
                "FreshRSS says: A feed could not be found at `http://feeds.example/page.html`; the status code is `200` and content-type is `text/html` [http://feeds.example/page.html]",
            ),
        ] {
            let server = MockServer::start().await;
            let client = freshrss_with(&server, one_feed()).await;
            freshrss_says(&server, "subscription/quickadd", 200, body).await;
            assert_eq!(client.change(&add(FEED)).await.unwrap_err().message(), shown);
            assert_eq!(calls_to(&server.received_requests().await.unwrap(), "/subscription/edit"), 0);
        }
    }

    #[tokio::test]
    async fn a_private_host_on_freshrss_says_how_to_allow_it() {
        let server = MockServer::start().await;
        let client = freshrss_with(&server, one_feed()).await;
        let body = r#"{"numResults":0,"error":"URL is not allowed to be resolved: http://rss.lan/f.xml [http://rss.lan/f.xml]"}"#;
        freshrss_says(&server, "subscription/quickadd", 200, body).await;
        let message = client.change(&add("http://rss.lan/f.xml")).await.unwrap_err().message();
        assert!(message.starts_with("FreshRSS says: URL is not allowed to be resolved"), "{message}");
        assert!(message.contains("INTERNAL_HOST_ALLOWLIST"), "{message}");
    }

    #[tokio::test]
    async fn freshrss_edits_go_by_stream_never_by_the_hash_and_a_category_by_its_name() {
        let server = MockServer::start().await;
        let client = freshrss_with(&server, one_feed()).await;
        freshrss_says(&server, "subscription/edit", 200, "OK").await;
        freshrss_says(&server, "rename-tag", 200, "OK").await;
        freshrss_says(&server, "disable-tag", 200, "OK").await;
        client.feed_counts().await.unwrap();
        let id = greader::stream_id("feed/10");
        let news = category(greader::stream_id("user/-/label/News"), "News");
        client.change(&FeedChange::RenameFeed { id, title: "Renamed Epsilon".into() }).await.unwrap();
        let to = Destination::New("Auto Created".into());
        client.change(&FeedChange::MoveFeed { id, to }).await.unwrap();
        client.change(&FeedChange::RemoveFeed { id }).await.unwrap();
        assert_eq!(
            sent(&server, "/subscription/edit").await,
            [
                format!("ac=edit&{}&{}&T=token", field("s", "feed/10"), field("t", "Renamed Epsilon")),
                format!("ac=edit&{}&{}&T=token", field("s", "feed/10"), field("a", "user/-/label/Auto Created")),
                format!("ac=unsubscribe&{}&T=token", field("s", "feed/10")),
            ]
        );

        let renamed = FeedChange::RenameCategory { category: news.clone(), title: "Latest".into() };
        let applied = client.change(&renamed).await.unwrap();
        let new_id = greader::stream_id("user/-/label/Latest");
        assert_eq!(applied.category, Some(category(new_id, "Latest")), "a new name is a new id");
        assert_ne!(new_id, news.id);
        assert_eq!(
            sent(&server, "/rename-tag").await,
            [format!("{}&{}&T=token", field("s", "user/-/label/News"), field("dest", "user/-/label/Latest"))]
        );
        client.change(&FeedChange::DeleteCategory { category: news }).await.unwrap();
        assert_eq!(sent(&server, "/disable-tag").await, [format!("{}&T=token", field("s", "user/-/label/News"))]);
    }

    #[tokio::test]
    async fn a_feed_thats_gone_from_freshrss_is_said_to_be_rather_than_edited() {
        let server = MockServer::start().await;
        let client = freshrss_with(&server, serde_json::json!({ "subscriptions": [] })).await;
        let error = client.change(&FeedChange::RemoveFeed { id: 12345 }).await.unwrap_err();
        assert_eq!(error.message(), feeds::GONE);
        assert_eq!(calls_to(&server.received_requests().await.unwrap(), "/subscription/edit"), 0);
    }

    #[tokio::test]
    async fn freshrss_s_bare_400_on_an_edit_gets_words() {
        let server = MockServer::start().await;
        let client = freshrss_with(&server, one_feed()).await;
        freshrss_says(&server, "rename-tag", 400, "Bad Request!").await;
        let change = FeedChange::RenameCategory { category: category(1, "Nope"), title: "X".into() };
        let message = client.change(&change).await.unwrap_err().message();
        assert!(message.starts_with("FreshRSS turned that down"), "{message}");
    }

    #[tokio::test]
    async fn a_subscribe_that_outlasts_the_wait_isnt_called_a_failure() {
        let server = MockServer::start().await;
        let client = freshrss_with(&server, one_feed()).await;
        freshrss_says(&server, "subscription/quickadd", 504, "<h1>504 Gateway Time-out</h1>").await;
        assert_eq!(client.change(&add(FEED)).await.unwrap_err().message(), feeds::ADD_STILL_WORKING);
    }

    #[tokio::test]
    async fn the_integration_is_sent_no_change_at_all_as_it_cant_be_told_from_other_servers() {
        let server = MockServer::start().await;
        signed_in(&server, "").await;
        let client = greader_at(server.uri());
        assert_eq!(client.feed_list().await.unwrap_err().message(), feeds::MANAGE_REFUSED);
        for change in
            [add(FEED), FeedChange::RemoveFeed { id: 1 }, FeedChange::DeleteCategory { category: category(1, "A") }]
        {
            assert_eq!(client.change(&change).await.unwrap_err().message(), feeds::MANAGE_REFUSED);
        }
        assert!(server.received_requests().await.unwrap().is_empty());
    }

    #[test]
    fn a_refusal_is_not_an_error_of_the_server() {
        assert!(matches!(still_working(Error::Refused("x".into())), Error::Refused(m) if m == "x"));
    }
}
