//! Works out what's at an address, with no credentials: the forgiving address box and the probe behind it. What it
//! finds goes to app.rs, which asks for what that server needs.
//!
//! The rules come from running these servers rather than reading about them. Nothing is decided on a status alone: a
//! single-page site answers 200 on every path. No redirect is followed, as one to another host means a sign-in gate,
//! and that mustn't get anything of ours. `/v1/me` goes last and isn't repeated, because on Miniflux even an empty
//! request logs a failed authentication, which fail2ban and the like count. The Google Reader header isn't a secret.

use crate::error;
use reqwest::header::{self, HeaderMap};
use reqwest::{StatusCode, Url};
use serde_json::Value;
use std::net::IpAddr;

/// `Authorization` that FreshRSS's compatibility check looks for. Without it, a healthy server reports FAIL.
const FRESHRSS_DUMMY: &str = "GoogleLogin auth=x";
const COMPATIBILITY: &str = "/api/greader.php/check/compatibility";
/// What it answers with the API turned off.
const API_OFF: &str = "Service Unavailable!";
/// What it answers when `Authorization` never reached it.
const NO_AUTHORIZATION: &str = "FAIL get HTTP Authorization";
/// nginx, and others in other words, when plain http reaches a TLS port.
const PLAIN_TO_TLS: &str = "plain http request";
/// More of an answer than any page probed here needs.
const MAX_BODY: usize = 256 * 1024;

/// Segments of a pasted URL that are pages of a server rather than where it lives. The part before the first of them
/// is the server, so a subpath stays.
const MINIFLUX_PAGES: [&str; 25] = [
    "unread",
    "starred",
    "history",
    "feeds",
    "feed",
    "categories",
    "category",
    "entry",
    "settings",
    "integrations",
    "keys",
    "sessions",
    "users",
    "about",
    "search",
    "shared",
    "subscribe",
    "bookmarklet",
    "import",
    "export",
    "login",
    "oauth2",
    "webauthn",
    "healthcheck",
    "v1",
];

/// Servers Onward can't read, by the page a plain `GET <base>/` gives: the name, and how the page's title starts.
const TITLES: [(&str, &str); 3] = [("yarr", "yarr!"), ("selfoss", "selfoss"), ("Tiny Tiny RSS", "tiny tiny rss")];
/// The same for what can't be told by its title: the cookie it sets, on the page or on the redirect to its sign-in.
/// Stringer isn't here: it redirects and sets nothing, so one request can't tell it from anything else.
const COOKIES: [(&str, &str); 1] = [("Nextcloud", "oc_sessionPassphrase")];

/// Where to look for a server: the URLs to try, in order, each without a trailing slash.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Address {
    tries: Vec<String>,
    /// `http://` was typed for a host that isn't private, so https is all that's tried.
    clear_refused: bool,
}

impl Address {
    /// What was typed, however it was typed: `rss`, `rss.example.com`, `host:8080`, or a page copied from the
    /// browser. The error is the message to show.
    pub fn parse(text: &str) -> Result<Self, String> {
        let text = text.trim();
        if text.is_empty() {
            return Err("Type your server's address first.".into());
        }
        let (scheme, rest) = match text.split_once("://") {
            Some((scheme, rest)) => (Some(scheme.to_ascii_lowercase()), rest),
            None => (None, text),
        };
        if let Some(scheme) = scheme.as_deref().filter(|s| !matches!(*s, "http" | "https")) {
            return Err(format!("{scheme}:// isn't a web address. Try the address you open in your browser."));
        }
        let scheme = scheme.unwrap_or_default();
        let as_typed = if scheme.is_empty() { "https" } else { scheme.as_str() };
        let not_an_address = || format!("\"{text}\" doesn't look like a server address.");
        let mut url = Url::parse(&format!("{as_typed}://{rest}")).map_err(|_| not_an_address())?;
        let host = url.host_str().filter(|h| !h.is_empty()).ok_or_else(not_an_address)?.to_string();
        let path = server_path(url.path());
        url.set_path(&path);
        url.set_query(None);
        url.set_fragment(None);
        // A login in the URL is nothing to send around, and what's asked next asks for it properly.
        let _ = url.set_username("");
        let _ = url.set_password(None);
        let plain = |mut url: Url| {
            let _ = url.set_scheme("http");
            bare(&url)
        };
        let private = is_private(&host);
        let tries = match scheme.as_str() {
            "https" => vec![bare(&url)],
            "http" if private => vec![bare(&url)],
            "http" => {
                let _ = url.set_scheme("https");
                vec![bare(&url)]
            }
            _ if private => vec![bare(&url), plain(url)],
            _ => vec![bare(&url)],
        };
        Ok(Self { tries, clear_refused: scheme == "http" && !private })
    }
}

/// A URL as a base to add paths to.
fn bare(url: &Url) -> String {
    url.as_str().trim_end_matches('/').to_string()
}

/// The part of a pasted path that's the server's, without a page of Miniflux or FreshRSS after it.
fn server_path(path: &str) -> String {
    let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    let page = |at: usize| {
        let (this, next) = (segments[at], segments.get(at + 1).copied());
        let lower = this.to_ascii_lowercase();
        MINIFLUX_PAGES.contains(&lower.as_str())
            || lower == "i"
            || lower.ends_with(".php")
            || (lower == "api" && next.is_none_or(|n| n.ends_with(".php")))
            || (lower == "reader" && next == Some("api"))
    };
    let mut keep = (0..segments.len()).find(|&at| page(at)).unwrap_or(segments.len());
    // FreshRSS unpacked as it comes has its pages under p/, which is found by looking.
    if keep > 0 && keep < segments.len() && segments[keep - 1] == "p" {
        keep -= 1;
    }
    format!("/{}", segments[..keep].join("/"))
}

/// Hosts where plain http is expected: this machine, the local network, and a tailnet, whose traffic is encrypted
/// anyway. A name with no dots is a local one or a Tailscale short name, as nothing public has one.
pub fn is_private(host: &str) -> bool {
    let host = host.trim_start_matches('[').trim_end_matches(']').trim_end_matches('.').to_ascii_lowercase();
    if let Ok(ip) = host.parse::<IpAddr>() {
        return match ip {
            IpAddr::V4(ip) => {
                let [a, b, ..] = ip.octets();
                ip.is_loopback() || ip.is_private() || ip.is_link_local() || (a == 100 && (64..128).contains(&b))
            }
            IpAddr::V6(ip) => {
                let first = ip.segments()[0];
                ip.is_loopback() || (first & 0xfe00) == 0xfc00 || (first & 0xffc0) == 0xfe80
            }
        };
    }
    !host.contains('.')
        || [".local", ".localhost", ".lan", ".internal", ".home.arpa", ".ts.net"].iter().any(|s| host.ends_with(s))
}

/// How FreshRSS's API stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Api {
    Ready,
    /// Off, as it is until someone turns it on.
    Off,
    /// Something in front of it removes `Authorization`.
    Stripped,
}

/// What was found at an address.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Detected {
    Miniflux {
        base: String,
        /// False when its sign-in page has no password field, so local accounts are off.
        sign_in: bool,
    },
    FreshRss {
        /// Its page, to send people to. Logged out, the deeper pages are a 403.
        web: String,
        /// Its Google Reader API.
        api: String,
        state: Api,
    },
    /// A server that was recognised, and that Onward has no way to read.
    CantRead {
        name: &'static str,
        base: String,
    },
    /// It redirects to another host. Nothing more was sent.
    Gate {
        base: String,
        to: String,
    },
    /// A 403 before anything else.
    Blocked {
        base: String,
    },
    /// Something answers, and it's nothing we know. `sign_in` says it asked for one on some path, which a gate may do.
    Unknown {
        base: String,
        sign_in: bool,
    },
    Certificate {
        base: String,
    },
    NoAnswer {
        base: String,
        why: String,
        clear_refused: bool,
    },
    /// The samples are on, and those don't go near a network.
    Samples,
}

impl Detected {
    /// What to tell someone when there's nothing to ask them for. `None` when there is.
    pub fn problem(&self) -> Option<String> {
        Some(match self {
            Detected::Miniflux { .. } | Detected::FreshRss { state: Api::Ready, .. } => return None,
            Detected::FreshRss { state: Api::Off, .. } => {
                "This is FreshRSS, but its API is off, as it is until someone turns it on. Open FreshRSS and sign in as \
                 the admin. Then go to Settings → Authentication, tick “Allow API access”, and save."
                    .into()
            }
            Detected::FreshRss { state: Api::Stripped, .. } => {
                "This is FreshRSS, but something in front of it removes the Authorization header, so signing in can't \
                 work, whatever the password. A proxy or web server has to pass that header on to FreshRSS."
                    .into()
            }
            Detected::CantRead { name, .. } => format!("This is {name}, and Onward can't read it yet."),
            Detected::Gate { base, to } => format!(
                "{base} sends Onward on to {to}, which usually means a sign-in page sits in front of the server. \
                 Onward can't get past one. Use an address that reaches the server directly, such as its local or \
                 Tailscale address."
            ),
            Detected::Blocked { base } => format!(
                "{base} refused to answer (403). A firewall rule or a sign-in page in front of the server would do \
                 this. Use an address that reaches the server directly."
            ),
            Detected::Unknown { base, sign_in: true } => format!(
                "{base} asked for a sign-in Onward doesn't recognise, so it can't tell what's there. If a sign-in \
                 page or proxy sits in front of your server, Onward can't get past it: use an address that reaches \
                 the server directly."
            ),
            Detected::Unknown { base, sign_in: false } => format!(
                "Onward reached {base}, but found neither Miniflux nor FreshRSS. If your server lives under a path, \
                 add it, as in rss.example.com/miniflux."
            ),
            Detected::Certificate { base } => {
                format!("The certificate at {base} isn't trusted, so Onward stopped there.")
            }
            Detected::NoAnswer { base, why, clear_refused: true } => format!(
                "No answer from {base}: {why}. Onward doesn't use plain http:// for a public address when it finds a \
                 server. To use it anyway, choose “Enter the details yourself”."
            ),
            Detected::NoAnswer { base, why, clear_refused: false } => {
                format!("No answer from {base}: {why}. Check the address and that the server is running.")
            }
            Detected::Samples => "The sample articles are on, and they don't connect to anything.".into(),
        })
    }
}

/// An answer to one request.
struct Page {
    status: StatusCode,
    headers: HeaderMap,
    body: String,
    /// Where it redirects to, made absolute.
    redirect: Option<Url>,
}

impl Page {
    fn is(&self, kind: &str) -> bool {
        let kind_of = self.headers.get(header::CONTENT_TYPE).and_then(|v| v.to_str().ok()).unwrap_or_default();
        kind_of.to_ascii_lowercase().starts_with(kind)
    }

    fn json(&self) -> Option<Value> {
        self.is("application/json").then(|| serde_json::from_str(&self.body).ok()).flatten()
    }

    fn sets_cookie(&self, name: &str) -> bool {
        let cookies = self.headers.get_all(header::SET_COOKIE).iter();
        cookies.filter_map(|v| v.to_str().ok()).any(|cookie| {
            let name_of = cookie.split('=').next().unwrap_or_default();
            name_of.trim_start_matches("__Host-").trim_start_matches("__Secure-") == name
        })
    }

    fn title(&self) -> Option<String> {
        let lower = self.body.to_ascii_lowercase();
        let open = lower.find("<title")?;
        let start = open + lower[open..].find('>')? + 1;
        let end = start + lower[start..].find("</title")?;
        Some(lower[start..end].trim().to_string())
    }
}

/// One request, read up to `MAX_BODY`. Redirects aren't followed, by `backend::detection_client`.
async fn fetch(http: &reqwest::Client, url: &str, freshrss: bool) -> Result<Page, reqwest::Error> {
    let mut request = http.get(url);
    if freshrss {
        request = request.header(header::AUTHORIZATION, FRESHRSS_DUMMY);
    }
    let mut response = request.send().await?;
    let status = response.status();
    let location = response.headers().get(header::LOCATION).and_then(|v| v.to_str().ok());
    let redirect = location.filter(|_| status.is_redirection()).and_then(|to| response.url().join(to).ok());
    let headers = response.headers().clone();
    let mut body = Vec::new();
    while body.len() < MAX_BODY {
        match response.chunk().await? {
            Some(chunk) => body.extend_from_slice(&chunk),
            None => break,
        }
    }
    Ok(Page { status, headers, body: String::from_utf8_lossy(&body).into_owned(), redirect })
}

/// Where a walk ended.
enum Walked {
    Found(Detected),
    /// Start again on https, at this base.
    Secure(String),
}

/// What a redirect means: to another host it's a gate; to https on the same one, the server's real address. Any other
/// is just not what was asked for, and the walk goes on.
fn redirected(base: &str, page: &Page) -> Option<Walked> {
    let (to, from) = (page.redirect.as_ref()?, Url::parse(base).ok()?);
    if from.host_str() != to.host_str() {
        let to = to.origin().ascii_serialization();
        return Some(Walked::Found(Detected::Gate { base: base.to_string(), to }));
    }
    (from.scheme() == "http" && to.scheme() == "https").then(|| secure(&from, to.port())).flatten()
}

/// The base on https, on `port`.
fn secure(from: &Url, port: Option<u16>) -> Option<Walked> {
    let mut base = from.clone();
    base.set_scheme("https").ok()?;
    base.set_port(port).ok()?;
    Some(Walked::Secure(bare(&base)))
}

/// FreshRSS's compatibility check at `root`, by the body: an empty reply or a catch-all page is no match.
fn freshrss(base: &str, root: &str, page: &Page) -> Option<Detected> {
    let body = page.body.trim();
    let state = if page.is("text/plain") && body == "PASS" {
        Api::Ready
    } else if page.is("text/plain") && body.starts_with(NO_AUTHORIZATION) {
        Api::Stripped
    } else if page.status == StatusCode::SERVICE_UNAVAILABLE && body == API_OFF {
        Api::Off
    } else {
        return None;
    };
    Some(Detected::FreshRss { web: format!("{base}{root}/i/"), api: format!("{base}{root}/api/greader.php"), state })
}

fn commafeed(base: &str, page: &Page) -> Option<Detected> {
    let json = page.json().filter(|_| page.status.is_success())?;
    let is_it = json.get("version").is_some() && json.get("allowRegistrations").is_some();
    is_it.then(|| Detected::CantRead { name: "CommaFeed", base: base.to_string() })
}

fn unreadable_home(base: &str, page: &Page) -> Option<Detected> {
    let title = page.title().unwrap_or_default();
    let by_title = TITLES.iter().find(|(_, start)| title.starts_with(start)).map(|(name, _)| *name);
    let by_cookie = COOKIES.iter().find(|(_, cookie)| page.sets_cookie(cookie)).map(|(name, _)| *name);
    by_title.or(by_cookie).map(|name| Detected::CantRead { name, base: base.to_string() })
}

fn miniflux(page: &Page) -> bool {
    page.status == StatusCode::UNAUTHORIZED && page.json().is_some_and(|json| json.get("error_message").is_some())
}

/// The sign-in page shows whether local accounts are on. Anything but a page says nothing, and that leaves them on.
fn has_password_field(page: &Page) -> bool {
    !(page.status.is_success() && page.is("text/html")) || page.body.contains("name=\"password\"")
}

/// The walk at one base, in the order of its paths. `Err` when the first request got no answer, which the next base
/// may do better with; a later path that fails is only a path with nothing on it.
async fn walk(http: &reqwest::Client, base: &str) -> Result<Walked, reqwest::Error> {
    let found = |detected| Ok(Walked::Found(detected));
    let mut sign_in = false;
    for (n, root) in ["", "/p"].iter().enumerate() {
        let page = match fetch(http, &format!("{base}{root}{COMPATIBILITY}"), true).await {
            Ok(page) => page,
            Err(e) if n == 0 => return Err(e),
            Err(_) => continue,
        };
        if let Some(moved) = redirected(base, &page) {
            return Ok(moved);
        }
        if page.status == StatusCode::BAD_REQUEST
            && page.body.to_ascii_lowercase().contains(PLAIN_TO_TLS)
            && let Some(moved) = Url::parse(base).ok().and_then(|from| secure(&from, from.port()))
        {
            return Ok(moved);
        }
        if let Some(detected) = freshrss(base, root, &page) {
            return found(detected);
        }
        if page.status == StatusCode::FORBIDDEN && n == 0 {
            return found(Detected::Blocked { base: base.to_string() });
        }
        sign_in |= page.status == StatusCode::UNAUTHORIZED;
    }
    if let Ok(page) = fetch(http, &format!("{base}/rest/server/get"), false).await {
        if let Some(moved) = redirected(base, &page) {
            return Ok(moved);
        }
        if let Some(detected) = commafeed(base, &page) {
            return found(detected);
        }
        sign_in |= page.status == StatusCode::UNAUTHORIZED;
    }
    // Asks nothing of a real server, and spares Miniflux's log if this is something else.
    let mut has_password = true;
    if let Ok(page) = fetch(http, &format!("{base}/"), false).await {
        if let Some(detected) = unreadable_home(base, &page) {
            return found(detected);
        }
        if let Some(moved) = redirected(base, &page) {
            return Ok(moved);
        }
        has_password = has_password_field(&page);
        sign_in |= page.status == StatusCode::UNAUTHORIZED;
    }
    if let Ok(page) = fetch(http, &format!("{base}/v1/me"), false).await {
        if let Some(moved) = redirected(base, &page) {
            return Ok(moved);
        }
        if miniflux(&page) {
            return found(Detected::Miniflux { base: base.to_string(), sign_in: has_password });
        }
        sign_in |= page.status == StatusCode::UNAUTHORIZED;
    }
    found(Detected::Unknown { base: base.to_string(), sign_in })
}

/// Why a request got no answer, as someone at the address box can use it.
fn failed_because(e: &reqwest::Error) -> String {
    let why = error::root_cause(e);
    // What rustls makes of a server that doesn't speak TLS at all.
    if why.contains("InvalidContentType") { "it doesn't speak https on that port".to_string() } else { why }
}

/// Tries the address, https first, and says what's there. It's at most five requests, and none of them repeats.
pub async fn probe(http: &reqwest::Client, address: &Address) -> Detected {
    let mut failure = None;
    for base in &address.tries {
        let mut at = base.clone();
        // One move to https, no more: a server can't keep this going.
        for moved in [false, true] {
            match walk(http, &at).await {
                Ok(Walked::Found(found)) => return found,
                Ok(Walked::Secure(next)) if !moved => at = next,
                Ok(Walked::Secure(_)) => return Detected::Unknown { base: at, sign_in: false },
                Err(e) if error::is_certificate(&e) => return Detected::Certificate { base: at },
                Err(e) => {
                    failure.get_or_insert((at.clone(), failed_because(&e)));
                    break;
                }
            }
        }
    }
    let (base, why) = failure.unwrap_or_default();
    Detected::NoAnswer { base, why, clear_refused: address.clear_refused }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
#[path = "detect_tests.rs"]
mod tests;
