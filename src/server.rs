//! The feed server the app reads from: Miniflux's own API, or the Google Reader API. backend.rs calls this and
//! doesn't care which; each method hands the call to the client for the configured kind.

use crate::error::Error;
use crate::feeds::{self, Articles, Counts, EntryPage, EntryStatus, Selection};
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
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::config;
    use crate::error::{Error, ErrorKind};
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

    #[test]
    fn a_refusal_is_not_an_error_of_the_server() {
        assert!(matches!(still_working(Error::Refused("x".into())), Error::Refused(m) if m == "x"));
    }
}
