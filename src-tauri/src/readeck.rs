//! Saves a URL to Readeck. API docs: <readeck>/docs/api

use serde_json::json;

use crate::{config, miniflux::truncate};

/// How much of an error response body to echo in a message.
const MAX_ERROR_BODY: usize = 300;

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
        let body = resp.text().await.unwrap_or_default();
        Err(format!("Readeck {status}: {}", truncate(&body, MAX_ERROR_BODY)))
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
        .map_err(|_| {
            format!("Could not reach Readeck at {}. Check the address and that the server is running.", cfg.url)
        })?;
    match resp.status().as_u16() {
        200 | 403 => Ok(()),
        401 => Err("Readeck rejected the token. Copy the token value itself, not its ID, and give it the “Bookmarks: write” permission.".into()),
        404 => Err(format!("{} doesn't look like a Readeck server", cfg.url)),
        code => Err(format!("Readeck answered with HTTP {code}")),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use wiremock::matchers::{body_json, header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

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
        assert_eq!(save(&reqwest::Client::new(), &cfg(&server), "https://example.com/a").await, Ok(()));
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
        assert_eq!(err, "Readeck 422 Unprocessable Entity: invalid url");
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
            assert_eq!(check(&http, &cfg(&server)).await, Ok(()), "HTTP {code}");
        }
        let _guard = Mock::given(method("GET"))
            .and(path("/api/profile"))
            .respond_with(ResponseTemplate::new(401))
            .mount_as_scoped(&server)
            .await;
        assert!(check(&http, &cfg(&server)).await.unwrap_err().starts_with("Readeck rejected the token"));
        drop(_guard);
        let _guard = Mock::given(method("GET"))
            .and(path("/api/profile"))
            .respond_with(ResponseTemplate::new(404))
            .mount_as_scoped(&server)
            .await;
        assert!(check(&http, &cfg(&server)).await.unwrap_err().contains("doesn't look like a Readeck server"));
        drop(_guard);
        let _guard = Mock::given(method("GET"))
            .and(path("/api/profile"))
            .respond_with(ResponseTemplate::new(503))
            .mount_as_scoped(&server)
            .await;
        assert_eq!(check(&http, &cfg(&server)).await.unwrap_err(), "Readeck answered with HTTP 503");
    }

    #[tokio::test]
    async fn check_reports_unreachable_server() {
        // Port 1 is never listening; a freed mock port could be reused by a parallel test.
        let c = config::Readeck { url: "http://127.0.0.1:1".into(), token: "tok".into() };
        let err = check(&reqwest::Client::new(), &c).await.unwrap_err();
        assert!(err.starts_with(&format!("Could not reach Readeck at {}", c.url)), "{err}");
    }
}
