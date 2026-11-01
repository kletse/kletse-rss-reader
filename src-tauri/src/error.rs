//! The error every command returns. The webview receives it as a `CommandError`: `message` to show,
//! and `kind` for a UI that wants to react differently to, say, a server that is down and a rejected key.

use serde::Serialize;
use std::{error::Error as _, fmt};

/// The server a request went to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Service {
    Miniflux,
    Readeck,
}

impl Service {
    /// What the app signs in to it with.
    fn credential(self) -> &'static str {
        match self {
            Service::Miniflux => "API key",
            Service::Readeck => "token",
        }
    }
}

impl fmt::Display for Service {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Service::Miniflux => "Miniflux",
            Service::Readeck => "Readeck",
        })
    }
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The settings are missing or wrong; the message says what to fill in.
    #[error("{0}")]
    Setup(String),
    /// No answer at all: DNS, a refused connection, TLS, a timeout. The cause is in `source`.
    #[error("Could not reach {service} at {url}")]
    Unreachable { service: Service, url: String, source: reqwest::Error },
    /// The server answered, but turned down the API key or token.
    #[error("{0}")]
    Rejected(String),
    /// The server answered with an HTTP error.
    #[error("{service} {status}{}", body_suffix(.body))]
    Http { service: Service, status: reqwest::StatusCode, body: String },
    /// The server answered something we can't use.
    #[error("{context}")]
    Unexpected { context: String, source: Option<reqwest::Error> },
    /// Something on this machine: writing the config, starting the browser.
    #[error("{context}")]
    Io { context: String, source: std::io::Error },
    /// A request the app turns down by itself, such as opening a `file:` link.
    #[error("{0}")]
    Refused(String),
}

fn body_suffix(body: &str) -> String {
    if body.is_empty() {
        String::new()
    } else {
        format!(": {body}")
    }
}

/// What kind of problem a `CommandError` is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "lowercase")]
pub enum ErrorKind {
    /// Settings are missing or invalid.
    Setup,
    /// The server could not be reached.
    Unreachable,
    /// The server turned down the API key or token.
    Rejected,
    /// The server answered with an error or something unusable.
    Server,
    /// A problem on this machine, or a request the app refuses.
    Local,
}

/// How a command error reaches the webview: the rejected value of `invoke`.
#[derive(Debug, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct CommandError {
    pub kind: ErrorKind,
    /// Ready to show as is.
    pub message: String,
}

impl Error {
    pub fn kind(&self) -> ErrorKind {
        match self {
            Error::Setup(_) => ErrorKind::Setup,
            Error::Unreachable { .. } => ErrorKind::Unreachable,
            Error::Rejected(_) => ErrorKind::Rejected,
            Error::Http { .. } | Error::Unexpected { .. } => ErrorKind::Server,
            Error::Io { .. } | Error::Refused(_) => ErrorKind::Local,
        }
    }

    /// What the user reads: the error plus its root cause, if it has one ("Could not reach Miniflux at
    /// https://…: Connection refused (os error 111). Check the address and that the server is running.").
    pub fn message(&self) -> String {
        let message = match std::iter::successors(self.source(), |&e| e.source()).last() {
            Some(root) => format!("{self}: {root}"),
            None => self.to_string(),
        };
        match self {
            Error::Unreachable { .. } => format!("{message}. Check the address and that the server is running."),
            _ => message,
        }
    }

    /// Sending a request failed: no answer came back.
    pub fn unreachable(service: Service, url: &str, source: reqwest::Error) -> Self {
        Error::Unreachable { service, url: url.to_string(), source }
    }

    /// Reading an answer failed: either it isn't what we expected, or the connection broke off halfway.
    pub fn reading(service: Service, url: &str, source: reqwest::Error) -> Self {
        if source.is_decode() {
            Error::Unexpected { context: format!("Unexpected answer from {service}"), source: Some(source) }
        } else {
            Error::unreachable(service, url, source)
        }
    }

    pub fn unexpected(context: impl Into<String>) -> Self {
        Error::Unexpected { context: context.into(), source: None }
    }
}

impl Serialize for Error {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        CommandError { kind: self.kind(), message: self.message() }.serialize(serializer)
    }
}

/// How much of an error response body to echo in a message.
const MAX_ERROR_BODY: usize = 300;

/// Passes a successful response through. A 401 or 403 means the key or token was turned down (revoked, say, or
/// lacking a permission); any other failure becomes `Error::Http`, with the start of its body.
pub async fn ok_status(service: Service, resp: reqwest::Response) -> Result<reqwest::Response, Error> {
    let status = resp.status();
    if status.is_success() {
        return Ok(resp);
    }
    if matches!(status, reqwest::StatusCode::UNAUTHORIZED | reqwest::StatusCode::FORBIDDEN) {
        let credential = service.credential();
        return Err(Error::Rejected(format!(
            "{service} turned down the {credential} ({status}). Check it in Settings."
        )));
    }
    let body = resp.text().await.unwrap_or_default();
    Err(Error::Http { service, status, body: truncate(&body, MAX_ERROR_BODY) })
}

/// Cuts `text` to at most `max` characters, marking the cut.
fn truncate(text: &str, max: usize) -> String {
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

    #[test]
    fn truncate_cuts_on_char_boundaries() {
        assert_eq!(truncate("short", 10), "short");
        assert_eq!(truncate("  padded  ", 10), "padded");
        assert_eq!(truncate("héllo wörld", 5), "héllo…");
        assert_eq!(truncate("exact", 5), "exact");
    }

    #[test]
    fn serializes_as_kind_and_message() {
        let err = Error::Setup("Miniflux is not set up yet".into());
        assert_eq!(serde_json::to_string(&err).unwrap(), r#"{"kind":"setup","message":"Miniflux is not set up yet"}"#);
    }

    #[test]
    fn message_appends_the_root_cause() {
        let io = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "Permission denied");
        let err = Error::Io { context: "Could not write /x/config.toml".into(), source: io };
        assert_eq!(err.to_string(), "Could not write /x/config.toml");
        assert_eq!(err.message(), "Could not write /x/config.toml: Permission denied");
        assert_eq!(err.kind(), ErrorKind::Local);
    }

    #[test]
    fn http_errors_leave_out_an_empty_body() {
        let err = Error::Http { service: Service::Readeck, status: reqwest::StatusCode::BAD_GATEWAY, body: "".into() };
        assert_eq!(err.message(), "Readeck 502 Bad Gateway");
        let err =
            Error::Http { service: Service::Miniflux, status: reqwest::StatusCode::NOT_FOUND, body: "gone".into() };
        assert_eq!(err.message(), "Miniflux 404 Not Found: gone");
        assert_eq!(err.kind(), ErrorKind::Server);
    }
}
