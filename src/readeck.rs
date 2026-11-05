//! Saves a URL to Readeck, and gets a token by the device grant (RFC 8628), which Readeck has from 0.21. API docs:
//! <readeck>/docs/api
//!
//! What a grant goes through is in `grant.rs`. Here are the calls, and what's said to them. All of it is unsigned, and on
//! `api_client()`: a redirect may stay on the server and nothing more, as `device_code` and the token come through here.

use serde::Deserialize;
use serde_json::{Value, json};
use std::fmt;

use crate::config;
use crate::error::{Error, Service, ok_status};

/// Readeck looks this up when a client registers, from where it runs, and won't have a name it can't find on the public
/// internet: see `Asked::NoPublicNames`.
const CLIENT_URI: &str = "https://github.com/kletse/onward-rss-reader";
/// The same for every copy of Onward: it says what's asking, not who.
const SOFTWARE_ID: &str = "0f6d3f0a-5b1e-4a8e-9c55-7b1f4d2e8a31";
const DEVICE_GRANT: &str = "urn:ietf:params:oauth:grant-type:device_code";
/// All that `b` needs. `check` allows for what this can't read.
const SCOPE: &str = "bookmarks:write";
/// Used when an answer leaves these out, as RFC 8628 says, and the bounds on what one says: a server shouldn't be able
/// to make the app poll in a tight loop, or sit there for a day.
const DEFAULT_INTERVAL: u64 = 5;
const INTERVALS: (u64, u64) = (1, 60);
const DEFAULT_EXPIRY: u64 = 300;
const EXPIRIES: (u64, u64) = (1, 3600);
/// Plenty for a code or an address. What's longer than this is no use to read.
const SHOWN_CHARS: usize = 300;

/// What a Readeck token is while it's on its way to being kept. `Debug` shows nothing.
#[derive(Clone, PartialEq, Eq)]
pub struct Token(String);

impl Token {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Token {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Token { .. }")
    }
}

/// What a poll is sent with. `Debug` shows nothing: `device_code` is as good as the token until it's been used.
#[derive(Clone, PartialEq, Eq)]
pub struct Ticket {
    client_id: String,
    device_code: String,
}

impl fmt::Debug for Ticket {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Ticket { .. }")
    }
}

/// A code, and what's to be done with it.
#[derive(Debug)]
pub struct Code {
    pub ticket: Ticket,
    /// Meant to be seen. Came from the server, like everything here, with control characters taken out.
    pub user_code: String,
    /// Where to enter it.
    pub address: String,
    /// The same with the code filled in, if there is one.
    pub address_with_code: Option<String>,
    pub expires_in_secs: u64,
    pub interval_secs: u64,
}

/// What became of asking for a code.
#[derive(Debug)]
pub enum Asked {
    Code(Box<Code>),
    /// No `oauth` in `/api/info`, or no such thing: this is a server to paste a token for.
    NoGrant,
    /// It wouldn't have Onward as a client as it can't look up `CLIENT_URI`.
    NoPublicNames,
    /// What to say about it.
    Failed(String),
}

/// What a poll came to.
#[derive(Debug)]
pub enum Poll {
    Pending,
    /// Asking too often: wait 5 s more from now on.
    SlowDown,
    Token(Token),
    Denied,
    Expired,
    /// The client is gone: Readeck keeps one for 10 minutes.
    ClientGone,
    /// A final no that isn't one of those, in words to show.
    Refused(String),
    /// No answer to go by, this time. In words to show.
    Failed(String),
}

#[derive(Deserialize)]
struct Device {
    device_code: String,
    user_code: String,
    verification_uri: String,
    #[serde(default)]
    verification_uri_complete: Option<String>,
    #[serde(default)]
    expires_in: Option<u64>,
    #[serde(default)]
    interval: Option<u64>,
}

/// A server's text, safe to put on a screen: a terminal would obey an escape sequence.
fn shown(text: &str) -> String {
    text.chars().filter(|c| !c.is_control()).take(SHOWN_CHARS).collect()
}

async fn json_of(resp: reqwest::Response) -> Value {
    let bytes = resp.bytes().await.unwrap_or_default();
    serde_json::from_slice(&bytes).unwrap_or(Value::Null)
}

/// What an error answer says for itself, or its HTTP code.
fn reason(body: &Value, code: u16) -> String {
    let said = ["error_description", "error"].into_iter().find_map(|key| body[key].as_str());
    said.map_or_else(|| format!("HTTP {code}"), shown)
}

fn unreachable(url: &str, e: reqwest::Error) -> String {
    Error::unreachable(Service::Readeck, url, e).message()
}

/// Does this server have the device grant? `Err` is that it couldn't be asked, and says nothing either way.
async fn has_grant(http: &reqwest::Client, url: &str) -> Result<bool, String> {
    let resp = http.get(format!("{url}/api/info")).send().await.map_err(|e| unreachable(url, e))?;
    if !resp.status().is_success() {
        return Ok(false);
    }
    let features = json_of(resp).await;
    let mut features = features["features"].as_array().into_iter().flatten();
    Ok(features.any(|feature| feature == "oauth"))
}

/// Registers a client for this attempt. There's nothing to keep about it: it has no secret and lasts 10 minutes.
async fn register(http: &reqwest::Client, url: &str) -> Result<String, Asked> {
    let body = json!({
        "client_name": crate::app::APP_NAME,
        "client_uri": CLIENT_URI,
        "software_id": SOFTWARE_ID,
        "software_version": env!("CARGO_PKG_VERSION"),
        "grant_types": [DEVICE_GRANT],
    });
    let sent = http.post(format!("{url}/api/oauth/client")).json(&body).send().await;
    let resp = sent.map_err(|e| Asked::Failed(unreachable(url, e)))?;
    let code = resp.status();
    let answer = json_of(resp).await;
    if !code.is_success() {
        let why = reason(&answer, code.as_u16());
        return Err(if why.contains("invalid client URI") {
            Asked::NoPublicNames
        } else {
            Asked::Failed(format!("Readeck wouldn't register Onward: {why}"))
        });
    }
    let id = answer["client_id"].as_str().filter(|id| !id.is_empty());
    id.map(str::to_string).ok_or_else(|| Asked::Failed("Readeck's answer had no client in it.".into()))
}

/// A code for this server, if it can give one: all that comes before the wait.
pub async fn ask(http: &reqwest::Client, url: &str) -> Asked {
    match has_grant(http, url).await {
        Ok(true) => {}
        Ok(false) => return Asked::NoGrant,
        Err(why) => return Asked::Failed(why),
    }
    let client_id = match register(http, url).await {
        Ok(client_id) => client_id,
        Err(asked) => return asked,
    };
    let body = json!({ "client_id": client_id, "scope": SCOPE });
    let resp = match http.post(format!("{url}/api/oauth/device")).json(&body).send().await {
        Ok(resp) => resp,
        Err(e) => return Asked::Failed(unreachable(url, e)),
    };
    let status = resp.status();
    let answer = json_of(resp).await;
    if !status.is_success() {
        return Asked::Failed(format!("Readeck wouldn't give Onward a code: {}", reason(&answer, status.as_u16())));
    }
    let Ok(device) = serde_json::from_value::<Device>(answer) else {
        return Asked::Failed("Readeck's answer had no code in it.".into());
    };
    let (user_code, address) = (shown(&device.user_code), shown(&device.verification_uri));
    if device.device_code.is_empty() || user_code.is_empty() || address.is_empty() {
        return Asked::Failed("Readeck's answer had no code in it.".into());
    }
    Asked::Code(Box::new(Code {
        ticket: Ticket { client_id, device_code: device.device_code },
        user_code,
        address,
        address_with_code: device.verification_uri_complete.as_deref().map(shown),
        expires_in_secs: device.expires_in.unwrap_or(DEFAULT_EXPIRY).clamp(EXPIRIES.0, EXPIRIES.1),
        interval_secs: device.interval.unwrap_or(DEFAULT_INTERVAL).clamp(INTERVALS.0, INTERVALS.1),
    }))
}

/// One go at getting the token. An error's body is read for its one word and no further.
pub async fn poll(http: &reqwest::Client, url: &str, ticket: &Ticket) -> Poll {
    let body = json!({
        "grant_type": DEVICE_GRANT,
        "client_id": ticket.client_id,
        "device_code": ticket.device_code,
    });
    let resp = match http.post(format!("{url}/api/oauth/token")).json(&body).send().await {
        Ok(resp) => resp,
        Err(e) => return Poll::Failed(unreachable(url, e)),
    };
    let code = resp.status();
    let answer = json_of(resp).await;
    if code.is_success() {
        return match answer["access_token"].as_str().filter(|token| !token.is_empty()) {
            Some(token) => Poll::Token(Token(token.to_string())),
            None => Poll::Failed("Readeck's answer had no token in it.".into()),
        };
    }
    match answer["error"].as_str() {
        Some("authorization_pending") => Poll::Pending,
        Some("slow_down") => Poll::SlowDown,
        Some("access_denied") => Poll::Denied,
        Some("expired_token") => Poll::Expired,
        Some("invalid_client") => Poll::ClientGone,
        Some(other) => Poll::Refused(format!("Readeck said no: {}.", shown(other))),
        None => Poll::Failed(format!("Readeck answered with HTTP {}", code.as_u16())),
    }
}

pub async fn save(http: &reqwest::Client, cfg: &config::Readeck, url: &str) -> Result<(), Error> {
    let resp = http
        .post(format!("{}/api/bookmarks", cfg.url))
        .bearer_auth(&cfg.token)
        .json(&json!({ "url": url }))
        .send()
        .await
        .map_err(|e| Error::unreachable(Service::Readeck, &cfg.url, e))?;
    ok_status(Service::Readeck, resp).await?;
    Ok(())
}

/// Checks that Readeck is reachable and accepts the token, without creating anything.
/// A token that only has `bookmarks:write` gets 403 on the profile endpoint, which still
/// proves the token itself is valid; only 401 means it is wrong.
pub async fn check(http: &reqwest::Client, cfg: &config::Readeck) -> Result<(), Error> {
    let resp = http
        .get(format!("{}/api/profile", cfg.url))
        .bearer_auth(&cfg.token)
        .header("Accept", "application/json")
        .send()
        .await
        .map_err(|e| Error::unreachable(Service::Readeck, &cfg.url, e))?;
    match resp.status().as_u16() {
        200 | 403 => Ok(()),
        401 => Err(Error::Rejected(
            "Readeck rejected the token. Copy the token value itself, not its ID, and give it the “Bookmarks: write” \
             permission."
                .into(),
        )),
        404 => Err(Error::Setup(format!("{} doesn't look like a Readeck server", cfg.url))),
        code => Err(Error::unexpected(format!("Readeck answered with HTTP {code}"))),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
pub(crate) mod tests {
    use super::*;
    use crate::error::ErrorKind;
    use wiremock::matchers::{body_json, body_partial_json, header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    pub(crate) fn token(text: &str) -> Token {
        Token(text.to_string())
    }

    fn cfg(server: &MockServer) -> config::Readeck {
        config::Readeck { url: server.uri(), token: "tok".into() }
    }

    #[tokio::test]
    async fn save_posts_bookmark_with_bearer_token() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/bookmarks"))
            .and(header("Authorization", "Bearer tok"))
            .and(body_json(json!({ "url": "https://example.com/a" })))
            .respond_with(ResponseTemplate::new(202))
            .expect(1)
            .mount(&server)
            .await;
        save(&reqwest::Client::new(), &cfg(&server), "https://example.com/a").await.unwrap();
    }

    #[tokio::test]
    async fn save_reports_server_errors() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/bookmarks"))
            .respond_with(ResponseTemplate::new(422).set_body_string("invalid url"))
            .mount(&server)
            .await;
        let err = save(&reqwest::Client::new(), &cfg(&server), "nope").await.unwrap_err();
        assert_eq!(err.message(), "Readeck 422 Unprocessable Entity: invalid url");
    }

    #[tokio::test]
    async fn save_with_a_token_lacking_write_permission_is_rejected() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/bookmarks"))
            .respond_with(ResponseTemplate::new(403))
            .mount(&server)
            .await;
        let err = save(&reqwest::Client::new(), &cfg(&server), "https://example.com/a").await.unwrap_err();
        assert_eq!(err.kind(), ErrorKind::Rejected);
        assert_eq!(err.message(), "Readeck turned down the token (403 Forbidden). Check it in Settings.");
    }

    #[tokio::test]
    async fn check_accepts_200_and_403_but_not_401() {
        let server = MockServer::start().await;
        let http = reqwest::Client::new();
        for code in [200, 403] {
            let _guard = Mock::given(method("GET"))
                .and(path("/api/profile"))
                .and(header("Authorization", "Bearer tok"))
                .respond_with(ResponseTemplate::new(code))
                .mount_as_scoped(&server)
                .await;
            assert!(check(&http, &cfg(&server)).await.is_ok(), "HTTP {code}");
        }
        let guard = Mock::given(method("GET"))
            .and(path("/api/profile"))
            .respond_with(ResponseTemplate::new(401))
            .mount_as_scoped(&server)
            .await;
        let err = check(&http, &cfg(&server)).await.unwrap_err();
        assert_eq!(err.kind(), ErrorKind::Rejected);
        assert!(err.message().starts_with("Readeck rejected the token. Copy the token value itself"));
        assert!(err.message().ends_with("“Bookmarks: write” permission."), "{}", err.message());
        drop(guard);
        let guard = Mock::given(method("GET"))
            .and(path("/api/profile"))
            .respond_with(ResponseTemplate::new(404))
            .mount_as_scoped(&server)
            .await;
        assert!(
            check(&http, &cfg(&server)).await.unwrap_err().message().contains("doesn't look like a Readeck server")
        );
        drop(guard);
        let _guard = Mock::given(method("GET"))
            .and(path("/api/profile"))
            .respond_with(ResponseTemplate::new(503))
            .mount_as_scoped(&server)
            .await;
        assert_eq!(check(&http, &cfg(&server)).await.unwrap_err().message(), "Readeck answered with HTTP 503");
    }

    #[tokio::test]
    async fn the_token_and_the_saved_link_dont_follow_a_redirect_to_another_host() {
        let elsewhere = MockServer::start().await;
        Mock::given(wiremock::matchers::any())
            .respond_with(ResponseTemplate::new(200))
            .expect(0)
            .mount(&elsewhere)
            .await;
        let to = format!("{}/login", elsewhere.uri().replace("127.0.0.1", "localhost"));
        let server = MockServer::start().await;
        Mock::given(wiremock::matchers::any())
            .respond_with(ResponseTemplate::new(307).insert_header("Location", to.as_str()))
            .mount(&server)
            .await;
        let http = crate::backend::api_client();
        for err in [
            check(&http, &cfg(&server)).await.unwrap_err(),
            save(&http, &cfg(&server), "https://example.com/a").await.unwrap_err(),
        ] {
            assert_eq!(err.kind(), ErrorKind::Setup);
            assert!(err.message().contains("token"), "{}", err.message());
        }
    }

    #[tokio::test]
    async fn check_reports_unreachable_server() {
        // Port 1 is never listening; a freed mock port could be reused by a parallel test.
        let c = config::Readeck { url: "http://127.0.0.1:1".into(), token: "tok".into() };
        let err = check(&reqwest::Client::new(), &c).await.unwrap_err();
        assert_eq!(err.kind(), ErrorKind::Unreachable);
        let prefix = format!("Could not reach Readeck at {}: ", c.url);
        assert!(err.message().starts_with(&prefix) && err.message().len() > prefix.len(), "{}", err.message());
    }

    const CLIENT: &str = "urn:uuid:4c1dc763-1c86-4f05-833c-ba1a4e9f28d6";
    /// As Readeck 0.23.5 answered, byte for byte.
    const INFO: &str = r#"{"version":{"canonical":"0.23.5","release":"0.23.5","build":""},"features":["oauth"]}"#;
    const NO_URI: &str = r#"{"error":"invalid_client_metadata","error_description":"error on field \"client_uri\": invalid client URI"}"#;
    const DEVICE: &str = r#"{"device_code":"pvo5Ezm1GtHbZNhNYtoXwUR0HM4s2vu24JA-z3xhZhylIxncgA0IK9GtT7sK3S75","user_code":"VSCZLVQJ","verification_uri":"http://127.0.0.1:18086/device","verification_uri_complete":"http://127.0.0.1:18086/device?user_code=VSCZLVQJ","expires_in":300,"interval":5}"#;

    fn answer(code: u16, body: &str) -> ResponseTemplate {
        ResponseTemplate::new(code).set_body_raw(body.to_string(), "application/json; charset=utf-8")
    }

    async fn on(server: &MockServer, verb: &str, at: &str, reply: ResponseTemplate) {
        Mock::given(method(verb)).and(path(at)).respond_with(reply).mount(server).await;
    }

    async fn registers(server: &MockServer) {
        on(server, "GET", "/api/info", answer(200, INFO)).await;
        let client = format!(r#"{{"client_id":"{CLIENT}","token_endpoint_auth_method":"none"}}"#);
        on(server, "POST", "/api/oauth/client", answer(201, &client)).await;
    }

    fn ticket() -> Ticket {
        Ticket { client_id: CLIENT.into(), device_code: "the-device-code".into() }
    }

    #[tokio::test]
    async fn asking_registers_a_client_and_gets_a_code_with_the_narrowest_scope() {
        let server = MockServer::start().await;
        on(&server, "GET", "/api/info", answer(200, INFO)).await;
        let client = format!(r#"{{"client_id":"{CLIENT}"}}"#);
        Mock::given(method("POST"))
            .and(path("/api/oauth/client"))
            .and(body_json(json!({
                "client_name": "Onward",
                "client_uri": "https://github.com/kletse/onward-rss-reader",
                "software_id": SOFTWARE_ID,
                "software_version": env!("CARGO_PKG_VERSION"),
                "grant_types": ["urn:ietf:params:oauth:grant-type:device_code"],
            })))
            .respond_with(answer(201, &client))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/api/oauth/device"))
            .and(body_json(json!({ "client_id": CLIENT, "scope": "bookmarks:write" })))
            .respond_with(answer(200, DEVICE))
            .expect(1)
            .mount(&server)
            .await;
        let Asked::Code(code) = ask(&reqwest::Client::new(), &server.uri()).await else { panic!("no code") };
        assert_eq!(code.user_code, "VSCZLVQJ");
        assert_eq!(code.address, "http://127.0.0.1:18086/device");
        assert_eq!(code.address_with_code.as_deref(), Some("http://127.0.0.1:18086/device?user_code=VSCZLVQJ"));
        assert_eq!((code.expires_in_secs, code.interval_secs), (300, 5));
        assert_eq!(code.ticket.client_id, CLIENT);
    }

    #[tokio::test]
    async fn no_oauth_feature_or_no_api_info_means_the_token_field_alone() {
        let server = MockServer::start().await;
        Mock::given(method("POST")).respond_with(ResponseTemplate::new(500)).expect(0).mount(&server).await;
        let http = reqwest::Client::new();
        for (code, body) in [(200, r#"{"features":[]}"#), (200, "{}"), (200, "<html>"), (404, "nope"), (401, "{}")] {
            let reply = answer(code, body);
            let guard = Mock::given(method("GET")).and(path("/api/info")).respond_with(reply);
            let _guard = guard.mount_as_scoped(&server).await;
            assert!(matches!(ask(&http, &server.uri()).await, Asked::NoGrant), "{code} {body}");
        }
    }

    #[tokio::test]
    async fn a_server_that_cant_be_reached_isnt_a_server_without_the_grant() {
        let asked = ask(&reqwest::Client::new(), "http://127.0.0.1:1").await;
        assert!(matches!(asked, Asked::Failed(why) if why.starts_with("Could not reach Readeck at")));
    }

    #[tokio::test]
    async fn a_readeck_that_cant_look_up_the_client_uri_is_told_apart() {
        let server = MockServer::start().await;
        on(&server, "GET", "/api/info", answer(200, INFO)).await;
        on(&server, "POST", "/api/oauth/client", answer(400, NO_URI)).await;
        assert!(matches!(ask(&reqwest::Client::new(), &server.uri()).await, Asked::NoPublicNames));
    }

    #[tokio::test]
    async fn another_refusal_at_either_step_says_what_the_server_said() {
        let server = MockServer::start().await;
        on(&server, "GET", "/api/info", answer(200, INFO)).await;
        on(&server, "POST", "/api/oauth/client", answer(500, r#"{"error":"server_error"}"#)).await;
        let asked = ask(&reqwest::Client::new(), &server.uri()).await;
        assert!(matches!(&asked, Asked::Failed(why) if why == "Readeck wouldn't register Onward: server_error"));

        let server = MockServer::start().await;
        registers(&server).await;
        on(&server, "POST", "/api/oauth/device", answer(400, r#"{"error":"invalid_scope"}"#)).await;
        let asked = ask(&reqwest::Client::new(), &server.uri()).await;
        assert!(matches!(&asked, Asked::Failed(why) if why == "Readeck wouldn't give Onward a code: invalid_scope"));
    }

    #[tokio::test]
    async fn what_a_server_sends_is_kept_within_bounds_and_free_of_escape_sequences() {
        let server = MockServer::start().await;
        registers(&server).await;
        let device = json!({
            "device_code": "d", "user_code": "\u{1b}[2JAB\nCD", "verification_uri": "http://r.example/\u{7}device",
            "expires_in": 0, "interval": 0,
        });
        on(&server, "POST", "/api/oauth/device", ResponseTemplate::new(200).set_body_json(device)).await;
        let Asked::Code(code) = ask(&reqwest::Client::new(), &server.uri()).await else { panic!("no code") };
        assert_eq!((code.user_code.as_str(), code.address.as_str()), ("[2JABCD", "http://r.example/device"));
        assert_eq!((code.expires_in_secs, code.interval_secs), (1, 1), "never a tight loop");

        let server = MockServer::start().await;
        registers(&server).await;
        let device = json!({ "device_code": "d", "user_code": "A", "verification_uri": "http://r/" });
        on(&server, "POST", "/api/oauth/device", ResponseTemplate::new(200).set_body_json(device)).await;
        let Asked::Code(code) = ask(&reqwest::Client::new(), &server.uri()).await else { panic!("no code") };
        assert_eq!((code.expires_in_secs, code.interval_secs), (300, 5), "RFC 8628's defaults");

        let server = MockServer::start().await;
        registers(&server).await;
        on(&server, "POST", "/api/oauth/device", answer(200, r#"{"device_code":"d"}"#)).await;
        assert!(matches!(ask(&reqwest::Client::new(), &server.uri()).await, Asked::Failed(_)));
    }

    #[tokio::test]
    async fn each_answer_to_a_poll_is_told_apart() {
        let server = MockServer::start().await;
        let http = reqwest::Client::new();
        let body = json!({
            "grant_type": "urn:ietf:params:oauth:grant-type:device_code",
            "client_id": CLIENT,
            "device_code": "the-device-code",
        });
        type Expected = fn(&Poll) -> bool;
        let cases: [(u16, &str, Expected); 8] = [
            (400, r#"{"error":"authorization_pending"}"#, |p| matches!(p, Poll::Pending)),
            (400, r#"{"error":"slow_down"}"#, |p| matches!(p, Poll::SlowDown)),
            (400, r#"{"error":"access_denied"}"#, |p| matches!(p, Poll::Denied)),
            (400, r#"{"error":"expired_token"}"#, |p| matches!(p, Poll::Expired)),
            (401, r#"{"error":"invalid_client"}"#, |p| matches!(p, Poll::ClientGone)),
            (400, r#"{"error":"invalid_grant"}"#, |p| matches!(p, Poll::Refused(w) if w.contains("invalid_grant"))),
            (502, "<html>Bad gateway</html>", |p| matches!(p, Poll::Failed(w) if w.contains("502"))),
            (201, r#"{"id":"x","token_type":"Bearer"}"#, |p| matches!(p, Poll::Failed(w) if w.contains("no token"))),
        ];
        for (code, reply, expected) in cases {
            let mock = Mock::given(method("POST")).and(path("/api/oauth/token")).and(body_json(body.clone()));
            let _guard = mock.respond_with(answer(code, reply)).mount_as_scoped(&server).await;
            let got = poll(&http, &server.uri(), &ticket()).await;
            assert!(expected(&got), "{code} {reply}: {got:?}");
        }
        let token = r#"{"id":"x","access_token":"the-token","token_type":"Bearer","scope":"bookmarks:write"}"#;
        let mock = Mock::given(method("POST")).and(body_partial_json(json!({ "client_id": CLIENT })));
        let _guard = mock.respond_with(answer(201, token)).mount_as_scoped(&server).await;
        let Poll::Token(token) = poll(&http, &server.uri(), &ticket()).await else { panic!("no token") };
        assert_eq!(token.as_str(), "the-token");
    }

    #[tokio::test]
    async fn a_poll_with_no_answer_says_so_and_goes_nowhere_else_after_a_redirect() {
        let poll_of = |url: String| async move { poll(&crate::backend::api_client(), &url, &ticket()).await };
        assert!(matches!(poll_of("http://127.0.0.1:1".into()).await, Poll::Failed(_)));

        let elsewhere = MockServer::start().await;
        Mock::given(wiremock::matchers::any())
            .respond_with(ResponseTemplate::new(200))
            .expect(0)
            .mount(&elsewhere)
            .await;
        let to = format!("{}/steal", elsewhere.uri().replace("127.0.0.1", "localhost"));
        let server = MockServer::start().await;
        let to_elsewhere = ResponseTemplate::new(307).insert_header("Location", to.as_str());
        Mock::given(wiremock::matchers::any()).respond_with(to_elsewhere).mount(&server).await;
        assert!(matches!(poll_of(server.uri()).await, Poll::Failed(_)));
    }

    #[test]
    fn nothing_that_signs_in_shows_in_debug_output() {
        let code = Code {
            ticket: ticket(),
            user_code: "CODE".into(),
            address: "http://r/device".into(),
            address_with_code: None,
            expires_in_secs: 1,
            interval_secs: 1,
        };
        let token = Token("the-token".into());
        let shown = format!("{code:?} {token:?} {:?}", Poll::Token(token.clone()));
        assert!(!shown.contains("the-device-code") && !shown.contains("the-token"), "{shown}");
        assert!(shown.contains("CODE"), "the code is meant to be seen: {shown}");
    }
}
