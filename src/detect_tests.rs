//! Each row here is an answer seen from a real server (FreshRSS 1.30.4, Miniflux 2.3.5 and 2.2.8, CommaFeed 7.4.1, nginx
//! 1.29.8), with its status and bytes as they came, so that the fingerprints can't drift from what servers really do.

use super::*;
use crate::backend::detection_client;
use crate::miniflux::{MintFailed, mint_key};
use wiremock::matchers::{header as has_header, method, path};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

const PASS: &str = "PASS";
const FAIL: &str = "FAIL get HTTP Authorization header! Wrong Web server configuration.";
const UNAUTHORIZED: &str = r#"{"error_message":"access unauthorized"}"#;
const NOT_FOUND_HTML: &str = "<html> <head><title>404 Not Found</title></head> <body> <center><h1>404 Not Found</h1>\
                              </center> <hr><center>nginx/1.29.8</center> </body> </html> ";
const COMMAFEED: &str = r#"{"announcement":null,"version":"7.4.1","gitCommit":"6a6978c","allowRegistrations":false,
    "emailAddressRequired":false,"smtpEnabled":false,"demoAccountEnabled":false,"initialSetupRequired":true}"#;
const SIGN_IN: &str = r#"<!DOCTYPE html> <html lang="en-US"> <head> <meta charset="utf-8"> <title>Sign In - Miniflux
    </title> </head><body><form><input type="text" name="username"><input type="password" name="password"></form>
    </body></html>"#;
const NO_LOCAL_SIGN_IN: &str = r#"<!DOCTYPE html><html><head><title>Sign In - Miniflux</title></head><body>
    <a href="/oauth2/oidc/redirect">Sign in with OpenID Connect</a></body></html>"#;
const SPA: &str = "<!doctype html><html><head><title>Some app</title></head><body><div id=root></div></body></html>";

fn text(status: u16, body: &str) -> ResponseTemplate {
    ResponseTemplate::new(status).set_body_raw(body.as_bytes().to_vec(), "text/plain; charset=UTF-8")
}

fn html(status: u16, body: &str) -> ResponseTemplate {
    ResponseTemplate::new(status).set_body_raw(body.as_bytes().to_vec(), "text/html; charset=utf-8")
}

fn json(status: u16, body: &str) -> ResponseTemplate {
    ResponseTemplate::new(status).set_body_raw(body.as_bytes().to_vec(), "application/json")
}

/// A server that answers these paths, and a plain 404 page on all others.
async fn server(routes: Vec<(&str, ResponseTemplate)>) -> MockServer {
    let server = MockServer::start().await;
    for (at, response) in routes {
        Mock::given(path(at)).respond_with(response).mount(&server).await;
    }
    Mock::given(method("GET")).respond_with(html(404, NOT_FOUND_HTML)).mount(&server).await;
    server
}

async fn detect(base: &str) -> Detected {
    probe(&detection_client(), &Address::parse(base).unwrap()).await
}

const CHECK: &str = "/api/greader.php/check/compatibility";
const UNPACKED: &str = "/p/api/greader.php/check/compatibility";

fn requests(all: &[Request]) -> Vec<String> {
    all.iter().map(|r| r.url.path().to_string()).collect()
}

// ---- The address ------------------------------------------------------------------------------------------------

fn tries(text: &str) -> Vec<String> {
    Address::parse(text).unwrap().tries
}

#[test]
fn a_bare_name_is_tried_on_https_and_on_http_only_where_that_belongs() {
    assert_eq!(tries("rss.example.com"), ["https://rss.example.com"]);
    assert_eq!(tries("  rss.example.com/  "), ["https://rss.example.com"]);
    assert_eq!(tries("rss"), ["https://rss", "http://rss"]);
    assert_eq!(tries("rss:8080"), ["https://rss:8080", "http://rss:8080"]);
    assert_eq!(tries("rss.example.com:8080"), ["https://rss.example.com:8080"]);
    assert_eq!(tries("localhost:8080"), ["https://localhost:8080", "http://localhost:8080"]);
    assert_eq!(tries("192.168.1.5"), ["https://192.168.1.5", "http://192.168.1.5"]);
    assert_eq!(tries("nas.local/miniflux"), ["https://nas.local/miniflux", "http://nas.local/miniflux"]);
    assert_eq!(tries("rss.tail1234.ts.net"), ["https://rss.tail1234.ts.net", "http://rss.tail1234.ts.net"]);
}

#[test]
fn what_was_typed_as_a_scheme_is_kept_unless_it_would_go_in_the_clear() {
    assert_eq!(tries("https://rss"), ["https://rss"]);
    assert_eq!(tries("http://rss:8080"), ["http://rss:8080"]);
    assert_eq!(tries("HTTP://localhost"), ["http://localhost"]);
    let public = Address::parse("http://rss.example.com").unwrap();
    assert_eq!(public.tries, ["https://rss.example.com"]);
    assert!(public.clear_refused);
}

#[test]
fn private_hosts_are_this_machine_the_network_and_the_tailnet() {
    for host in [
        "localhost",
        "rss",
        "nas.local",
        "foo.localhost",
        "box.lan",
        "box.internal",
        "box.home.arpa",
        "rss.tail1234.ts.net",
        "127.0.0.1",
        "10.0.0.1",
        "172.16.0.1",
        "192.168.1.1",
        "169.254.0.1",
        "100.64.0.1",
        "100.127.255.1",
        "[::1]",
        "[fd7a:115c:a1e0::1]",
        "[fe80::1]",
    ] {
        assert!(is_private(host), "{host}");
    }
    for host in ["rss.example.com", "8.8.8.8", "100.128.0.1", "172.32.0.1", "[2001:db8::1]", "local.example.com"] {
        assert!(!is_private(host), "{host}");
    }
}

#[test]
fn a_pasted_page_comes_down_to_the_server_and_a_subpath_stays() {
    for (pasted, base) in [
        ("https://rss.example.com/unread", "https://rss.example.com"),
        ("https://rss.example.com/unread/entry/12", "https://rss.example.com"),
        ("https://rss.example.com/feed/3/entries?x=1#top", "https://rss.example.com"),
        ("https://rss.example.com/keys/create", "https://rss.example.com"),
        ("https://rss.example.com/v1/me", "https://rss.example.com"),
        ("https://rss.example.com/miniflux/unread", "https://rss.example.com/miniflux"),
        ("https://rss.example.com/miniflux/", "https://rss.example.com/miniflux"),
        ("https://fresh.example.com/i/?c=user&a=profile", "https://fresh.example.com"),
        ("https://fresh.example.com/i/", "https://fresh.example.com"),
        ("https://fresh.example.com/rss/i/?a=normal", "https://fresh.example.com/rss"),
        ("https://fresh.example.com/api/greader.php", "https://fresh.example.com"),
        ("https://fresh.example.com/api/greader.php/reader/api/0/token", "https://fresh.example.com"),
        ("https://fresh.example.com/rss/api/greader.php", "https://fresh.example.com/rss"),
        ("https://fresh.example.com/p/i/", "https://fresh.example.com"),
        ("https://fresh.example.com/p/api/greader.php", "https://fresh.example.com"),
        ("https://fresh.example.com/rss/p/i/?c=auth", "https://fresh.example.com/rss"),
        ("https://fresh.example.com/p", "https://fresh.example.com/p"),
        ("https://user:secret@rss.example.com/unread", "https://rss.example.com"),
        ("https://rss.example.com/reader/api/0/token", "https://rss.example.com"),
        ("https://rss.example.com/rss", "https://rss.example.com/rss"),
    ] {
        assert_eq!(tries(pasted), [base], "{pasted}");
    }
}

#[test]
fn what_isnt_an_address_is_said_to_be_one() {
    assert!(Address::parse("   ").unwrap_err().contains("Type your server's address"));
    assert!(Address::parse("ftp://rss.example.com").unwrap_err().contains("ftp://"));
    assert!(Address::parse("rss example").unwrap_err().contains("doesn't look like"));
    assert!(Address::parse("https://").unwrap_err().contains("doesn't look like"));
}

// ---- FreshRSS, by state ---------------------------------------------------------------------------------------

#[tokio::test]
async fn freshrss_with_its_api_on_passes_the_check_if_it_gets_the_dummy_header() {
    let server = MockServer::start().await;
    // A healthy server without the header says FAIL: that's the row this header is for.
    Mock::given(path(CHECK))
        .and(has_header("authorization", FRESHRSS_DUMMY))
        .respond_with(text(200, PASS))
        .mount(&server)
        .await;
    Mock::given(path(CHECK)).respond_with(text(200, FAIL)).mount(&server).await;
    let base = server.uri();
    assert_eq!(
        detect(&base).await,
        Detected::FreshRss { web: format!("{base}/i/"), api: format!("{base}/api/greader.php"), state: Api::Ready }
    );
    assert_eq!(server.received_requests().await.unwrap().len(), 1, "it stops at the first match");
}

#[tokio::test]
async fn freshrss_with_its_api_off_is_503_and_those_words() {
    let server = server(vec![(CHECK, text(503, "Service Unavailable!"))]).await;
    let found = detect(&server.uri()).await;
    assert!(matches!(found, Detected::FreshRss { state: Api::Off, .. }), "{found:?}");
    let message = found.problem().unwrap();
    assert!(message.contains("its API is off") && message.contains("Allow API access"), "{message}");
}

#[tokio::test]
async fn another_503_isnt_freshrss() {
    let server = server(vec![(CHECK, html(503, "<h1>Service Unavailable</h1>"))]).await;
    assert_eq!(detect(&server.uri()).await, Detected::Unknown { base: server.uri(), sign_in: false });
}

#[tokio::test]
async fn a_proxy_that_strips_authorization_shows_with_the_header_sent() {
    let server = server(vec![(CHECK, text(200, FAIL))]).await;
    let found = detect(&server.uri()).await;
    assert!(matches!(found, Detected::FreshRss { state: Api::Stripped, .. }), "{found:?}");
    assert!(found.problem().unwrap().contains("Authorization header"));
}

#[tokio::test]
async fn an_unpacked_freshrss_is_under_p() {
    let server = server(vec![(UNPACKED, text(200, PASS))]).await;
    let base = server.uri();
    assert_eq!(
        detect(&base).await,
        Detected::FreshRss { web: format!("{base}/p/i/"), api: format!("{base}/p/api/greader.php"), state: Api::Ready }
    );
    assert_eq!(requests(&server.received_requests().await.unwrap()), [CHECK, UNPACKED]);
}

#[tokio::test]
async fn a_subpath_is_kept_for_everything_that_follows() {
    let server = server(vec![("/rss/api/greader.php/check/compatibility", text(200, PASS))]).await;
    let base = format!("{}/rss", server.uri());
    assert_eq!(
        detect(&base).await,
        Detected::FreshRss { web: format!("{base}/i/"), api: format!("{base}/api/greader.php"), state: Api::Ready }
    );
}

// ---- Miniflux -------------------------------------------------------------------------------------------------

#[tokio::test]
async fn miniflux_is_the_401_json_and_v1_me_comes_last_and_once() {
    let server = server(vec![("/", html(200, SIGN_IN)), ("/v1/me", json(401, UNAUTHORIZED))]).await;
    assert_eq!(detect(&server.uri()).await, Detected::Miniflux { base: server.uri(), sign_in: true });
    assert_eq!(
        requests(&server.received_requests().await.unwrap()),
        [CHECK, UNPACKED, "/rest/server/get", "/", "/v1/me"],
        "one request a path, and the only one that logs a failed authentication goes last"
    );
}

#[tokio::test]
async fn miniflux_with_local_sign_in_off_shows_on_its_sign_in_page() {
    let server = server(vec![("/", html(200, NO_LOCAL_SIGN_IN)), ("/v1/me", json(401, UNAUTHORIZED))]).await;
    assert_eq!(detect(&server.uri()).await, Detected::Miniflux { base: server.uri(), sign_in: false });
}

#[tokio::test]
async fn a_home_page_that_says_nothing_leaves_sign_in_as_it_was() {
    let server = server(vec![("/", text(200, "OK")), ("/v1/me", json(401, UNAUTHORIZED))]).await;
    assert_eq!(detect(&server.uri()).await, Detected::Miniflux { base: server.uri(), sign_in: true });
}

#[tokio::test]
async fn a_401_without_that_json_isnt_miniflux() {
    let server = server(vec![("/v1/me", html(401, "<h1>Sign in</h1>"))]).await;
    let found = detect(&server.uri()).await;
    assert_eq!(found, Detected::Unknown { base: server.uri(), sign_in: true });
    assert!(found.problem().unwrap().contains("can't get past it"), "a gate answering 401 isn't called nothing");
}

// ---- Other servers -------------------------------------------------------------------------------------------

#[tokio::test]
async fn commafeed_is_recognised_and_v1_me_is_never_sent() {
    let server = server(vec![("/rest/server/get", json(200, COMMAFEED))]).await;
    let found = detect(&server.uri()).await;
    assert_eq!(found, Detected::CantRead { name: "CommaFeed", base: server.uri() });
    assert_eq!(found.problem().unwrap(), "This is CommaFeed, and Onward can't read it yet.");
    assert!(!requests(&server.received_requests().await.unwrap()).contains(&"/v1/me".to_string()));
}

#[tokio::test]
async fn servers_onward_cant_read_are_named_from_their_home_page_with_no_failed_login_on_them() {
    for (page, name) in [
        (html(200, "<html><head><title>yarr!</title></head></html>"), "yarr"),
        (html(200, "<html><head>\n    <title>selfoss</title></head></html>"), "selfoss"),
        (html(200, "<html><head><title>Tiny Tiny RSS : Login</title></head></html>"), "Tiny Tiny RSS"),
        (html(200, "<html><head><title>Tiny Tiny RSS</title></head></html>"), "Tiny Tiny RSS"),
        (
            ResponseTemplate::new(302)
                .insert_header("Location", "/login")
                .insert_header("Set-Cookie", "oc_sessionPassphrase=abc; path=/; secure; HttpOnly"),
            "Nextcloud",
        ),
        (
            ResponseTemplate::new(302)
                .insert_header("Location", "/login")
                .insert_header("Set-Cookie", "_stringer_session=abc; path=/; HttpOnly"),
            "Stringer",
        ),
    ] {
        let server = server(vec![("/", page)]).await;
        assert_eq!(detect(&server.uri()).await, Detected::CantRead { name, base: server.uri() });
        assert!(!requests(&server.received_requests().await.unwrap()).contains(&"/v1/me".to_string()), "{name}");
    }
}

#[tokio::test]
async fn a_catch_all_site_answering_200_everywhere_is_nothing() {
    let server = MockServer::start().await;
    Mock::given(method("GET")).respond_with(html(200, SPA)).mount(&server).await;
    assert_eq!(detect(&server.uri()).await, Detected::Unknown { base: server.uri(), sign_in: false });
}

#[tokio::test]
async fn a_plain_site_is_nothing() {
    let server = server(vec![]).await;
    let found = detect(&server.uri()).await;
    assert_eq!(found, Detected::Unknown { base: server.uri(), sign_in: false });
    assert!(found.problem().unwrap().contains("add it, as in rss.example.com/miniflux"));
}

#[tokio::test]
async fn a_blanket_403_ends_the_probe_at_the_first_request() {
    let server = MockServer::start().await;
    Mock::given(method("GET")).respond_with(html(403, "<h1>403 Forbidden</h1>")).mount(&server).await;
    assert_eq!(detect(&server.uri()).await, Detected::Blocked { base: server.uri() });
    assert_eq!(server.received_requests().await.unwrap().len(), 1);
}

// ---- Redirects ------------------------------------------------------------------------------------------------

#[tokio::test]
async fn a_redirect_to_another_host_is_a_sign_in_gate_and_nothing_more_goes_out() {
    let server = MockServer::start().await;
    let to = format!("{}/?rd=x", server.uri().replace("127.0.0.1", "localhost"));
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(302).insert_header("Location", to.as_str()))
        .mount(&server)
        .await;
    let found = detect(&server.uri()).await;
    assert_eq!(found, Detected::Gate { base: server.uri(), to: server.uri().replace("127.0.0.1", "localhost") });
    assert!(found.problem().unwrap().contains("sign-in page"));
    assert_eq!(server.received_requests().await.unwrap().len(), 1, "not followed, and the probe ended");
}

#[tokio::test]
async fn a_gate_that_only_covers_the_home_page_is_still_a_gate() {
    let server = MockServer::start().await;
    let to = format!("{}/login", server.uri().replace("127.0.0.1", "localhost"));
    Mock::given(path("/"))
        .respond_with(ResponseTemplate::new(302).insert_header("Location", to.as_str()))
        .mount(&server)
        .await;
    Mock::given(method("GET")).respond_with(html(404, NOT_FOUND_HTML)).mount(&server).await;
    assert!(matches!(detect(&server.uri()).await, Detected::Gate { .. }));
}

#[tokio::test]
async fn a_redirect_on_the_same_host_is_followed_by_nothing() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(302).insert_header("Location", "/somewhere"))
        .mount(&server)
        .await;
    assert_eq!(detect(&server.uri()).await, Detected::Unknown { base: server.uri(), sign_in: false });
    assert!(!requests(&server.received_requests().await.unwrap()).contains(&"/somewhere".to_string()));
}

#[tokio::test]
async fn http_that_moves_to_https_restarts_there_once() {
    // A TLS server can't be had here; nothing listens on the port it names, which shows the move was made.
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    let server = MockServer::start().await;
    let to = format!("https://127.0.0.1:{port}{CHECK}");
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(301).insert_header("Location", to.as_str()))
        .mount(&server)
        .await;
    let found = detect(&server.uri()).await;
    let Detected::NoAnswer { base, .. } = found else { panic!("{found:?}") };
    assert_eq!(base, format!("https://127.0.0.1:{port}"));
    assert_eq!(server.received_requests().await.unwrap().len(), 1);
}

#[tokio::test]
async fn plain_http_at_a_tls_port_goes_to_https_on_that_port() {
    let page = "<html> <head><title>400 The plain HTTP request was sent to HTTPS port</title></head> <body> <center>\
                <h1>400 Bad Request</h1></center> <center>The plain HTTP request was sent to HTTPS port</center> <hr>\
                <center>nginx/1.29.8</center> </body> </html> ";
    let server = MockServer::start().await;
    Mock::given(method("GET")).respond_with(html(400, page)).mount(&server).await;
    let base = server.uri();
    let found = detect(&base).await;
    let Detected::NoAnswer { base: tried, .. } = found else { panic!("{found:?}") };
    assert_eq!(tried, base.replace("http://", "https://"));
    assert_eq!(server.received_requests().await.unwrap().len(), 1);
}

// ---- No answer ------------------------------------------------------------------------------------------------

#[tokio::test]
async fn a_closed_port_is_said_to_have_no_answer_and_http_is_tried_where_it_may_be() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    let found = detect(&format!("127.0.0.1:{port}")).await;
    let Detected::NoAnswer { ref base, .. } = found else { panic!("{found:?}") };
    assert_eq!(*base, format!("https://127.0.0.1:{port}"));
    assert!(found.problem().unwrap().contains("Check the address"));
}

#[tokio::test]
async fn a_private_host_with_only_http_is_found_on_http() {
    let server = server(vec![(CHECK, text(200, PASS))]).await;
    let bare = server.uri().replace("http://", "");
    let found = detect(&bare).await;
    assert!(
        matches!(&found, Detected::FreshRss { api, state: Api::Ready, .. } if api.starts_with("http://")),
        "{found:?}"
    );
}

#[test]
fn a_public_address_typed_with_http_is_told_why_it_got_no_further() {
    let found = Detected::NoAnswer { base: "https://a.example".into(), why: "refused".into(), clear_refused: true };
    assert!(found.problem().unwrap().contains("plain http://"));
}

// ---- Making a key ---------------------------------------------------------------------------------------------

async fn mint(server: &MockServer) -> Result<String, MintFailed> {
    mint_key(&detection_client(), &server.uri(), "admin", "secret", "Onward 2026-11-05").await
}

fn answering(response: ResponseTemplate) -> Mock {
    Mock::given(method("POST")).and(path("/v1/api-keys")).respond_with(response)
}

#[tokio::test]
async fn a_right_password_gets_a_key_with_the_description_given() {
    let server = MockServer::start().await;
    let token = "a".repeat(64);
    let body = format!(r#"{{"id":1,"user_id":1,"token":"{token}","description":"Onward 2026-11-05"}}"#);
    Mock::given(method("POST"))
        .and(path("/v1/api-keys"))
        .and(has_header("authorization", "Basic YWRtaW46c2VjcmV0"))
        .respond_with(json(201, &body))
        .mount(&server)
        .await;
    assert_eq!(mint(&server).await, Ok(token));
    let sent = server.received_requests().await.unwrap();
    assert_eq!(sent.len(), 1, "once");
    assert_eq!(sent[0].body_json::<Value>().unwrap()["description"], "Onward 2026-11-05");
}

#[tokio::test]
async fn a_description_used_before_is_a_400_with_miniflux_s_words() {
    let server = MockServer::start().await;
    answering(json(400, r#"{"error_message":"This API Key already exists."}"#)).mount(&server).await;
    assert_eq!(mint(&server).await, Err(MintFailed::Other("Miniflux answered: This API Key already exists.".into())));
}

#[tokio::test]
async fn a_401_is_refused_whatever_the_reason_and_isnt_retried() {
    let server = MockServer::start().await;
    answering(json(401, UNAUTHORIZED)).mount(&server).await;
    assert_eq!(mint(&server).await, Err(MintFailed::Refused));
    assert_eq!(server.received_requests().await.unwrap().len(), 1);
}

#[tokio::test]
async fn an_older_miniflux_answers_404_before_it_looks_at_the_password() {
    let server = MockServer::start().await;
    answering(text(404, "404 page not found\n")).mount(&server).await;
    assert_eq!(mint(&server).await, Err(MintFailed::NoSuchCall));
}

#[tokio::test]
async fn a_redirect_doesnt_take_the_password_along() {
    let server = MockServer::start().await;
    let other = MockServer::start().await;
    let to = format!("{}/v1/api-keys", other.uri());
    answering(ResponseTemplate::new(307).insert_header("Location", to.as_str())).mount(&server).await;
    assert_eq!(mint(&server).await, Err(MintFailed::Redirected));
    assert!(other.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn a_server_gone_wrong_says_so() {
    let server = MockServer::start().await;
    answering(html(502, "<h1>Bad Gateway</h1>")).mount(&server).await;
    let Err(MintFailed::Other(message)) = mint(&server).await else { panic!() };
    assert!(message.contains("502"), "{message}");
}
