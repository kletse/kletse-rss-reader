//! Minimal Google Reader API client: only what the reader needs, as FreshRSS (`https://host/api/greader.php`),
//! Miniflux's Google Reader integration (`https://host`), Inoreader, BazQux and The Old Reader speak it.
//! The best description is FreshRSS's: https://freshrss.github.io/FreshRSS/en/developers/06_GoogleReader_API.html
//!
//! It gives the app the same `Entry`, `FeedCount` and `Selection` the Miniflux client does, so app.rs doesn't
//! know which server it reads from. What the protocol has no way to do comes back as `Error::Refused`, which the
//! app shows as a toast: fetching the full article, refreshing the feeds, and search on servers without it.
//!
//! What the servers do differently, and how this copes:
//!
//! - Entries are listed with `stream/items/ids` and then fetched with `stream/items/contents`, two requests per
//!   page rather than one `stream/contents`, because Miniflux has only those two.
//! - A page is ordered and bounded (`ot`, `nt`, whole seconds) by the server's own time: when an item arrived on
//!   FreshRSS, when it was published on Miniflux. Either way that is the item's `timestampUsec`, which becomes
//!   `Entry::order_ms` so that `paging::next_page` starts the next page from the right time.
//! - Item ids are 64-bit numbers. In `stream/items/ids` they're decimal, in contents a long form with the number in
//!   hex (`tag:google.com,2005:reader/item/00065f684d0ec886`); both become the same i64 (`as` from u64: an id
//!   with the top bit set would come out negative, which no server here hands out).
//! - Feeds and categories are named by strings (`feed/12`, `feed/http://…`, `user/-/label/Dev`); `Selection`
//!   carries i64s, so each is a 63-bit hash of its string (`stream_id`), and the client remembers which string
//!   each hash stands for.
//! - There are no read counts, and Miniflux has no unread counts or search either: those come back as unknown.
//!   Miniflux can't list a category's items (labels) either, nor history per feed.

use serde::Deserialize;
use std::collections::HashMap;
use std::sync::Mutex;

use crate::config;
use crate::dates;
use crate::error::{Error, Service, ok_status};
use crate::miniflux::{Category, Entry, EntryFeed, EntryPage, EntryStatus, Feed, FeedCount, Selection};
use crate::paging::Order;

const READING_LIST: &str = "user/-/state/com.google/reading-list";
const READ: &str = "user/-/state/com.google/read";
const ITEM_PREFIX: &str = "tag:google.com,2005:reader/item/";
/// How many item ids one listing asks for: enough to count most lists whole, and what servers allow.
const ID_LIST: usize = 1000;
/// How many listings `unread_ids` follows, at most: 50 000 unread items.
const MAX_ID_PAGES: usize = 50;
/// Items per `edit-tag` or `stream/items/contents` request.
const BATCH: usize = 100;
/// Upper bound on one page requested by the UI.
const MAX_PAGE: usize = 1000;

const NO_SEARCH: &str = "This server has no search";

/// `Debug` leaves out the password and the tokens.
pub struct Client {
    http: reqwest::Client,
    base: String,
    username: String,
    password: String,
    /// The `Auth` token from logging in, sent with every request; logging in again when the server turns it down.
    auth: Mutex<Option<String>>,
    /// The short-lived `T` token every POST needs; fetched again when it goes stale.
    edit_token: Mutex<Option<String>>,
    /// Which stream (`feed/12`, `user/-/label/Dev`) each hashed feed and category id stands for.
    streams: Mutex<HashMap<i64, String>>,
}

impl std::fmt::Debug for Client {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Client")
            .field("base", &self.base)
            .field("username", &self.username)
            .field("password", &config::Redacted(&self.password))
            .finish_non_exhaustive()
    }
}

/// The 63-bit id a feed or category stream gets in a `Selection`: FNV-1a of the stream's name, sign bit cleared,
/// never 0. Stable across runs and servers, so the same feed keeps its place in the picker.
pub fn stream_id(stream: &str) -> i64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in stream.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    i64::try_from(hash & i64::MAX as u64).unwrap_or(1).max(1)
}

/// An item id as either form gives it: decimal, or `tag:google.com,2005:reader/item/<16 hex digits>`.
fn parse_item_id(id: &str) -> Option<i64> {
    match id.strip_prefix(ITEM_PREFIX) {
        // The cast keeps the bits: ids are 64-bit numbers, and no server here uses the top bit.
        Some(hex) => u64::from_str_radix(hex, 16).ok().map(|n| n as i64),
        None => id.parse::<u64>().ok().map(|n| n as i64),
    }
}

/// The long form, which every server takes in `i=`.
fn long_item_id(id: i64) -> String {
    format!("{ITEM_PREFIX}{:016x}", id as u64)
}

#[derive(Deserialize)]
struct Subscriptions {
    subscriptions: Vec<Subscription>,
}

#[derive(Deserialize)]
struct Subscription {
    id: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    categories: Vec<SubscriptionCategory>,
}

#[derive(Deserialize)]
struct SubscriptionCategory {
    id: String,
    #[serde(default)]
    label: String,
}

#[derive(Deserialize)]
struct ItemRefs {
    #[serde(default, rename = "itemRefs")]
    item_refs: Vec<ItemRef>,
    /// FreshRSS sends a string, Miniflux a number (0 for none).
    #[serde(default)]
    continuation: Option<serde_json::Value>,
}

impl ItemRefs {
    fn ids(&self) -> Vec<i64> {
        self.item_refs.iter().filter_map(|r| parse_item_id(&r.id)).collect()
    }

    /// Where the next listing starts, if there is one.
    fn next(&self) -> Option<String> {
        match self.continuation.as_ref()? {
            serde_json::Value::String(s) if !s.is_empty() => Some(s.clone()),
            serde_json::Value::Number(n) if n.as_i64() != Some(0) => Some(n.to_string()),
            _ => None,
        }
    }
}

#[derive(Deserialize)]
struct ItemRef {
    id: String,
}

#[derive(Deserialize)]
struct Contents {
    #[serde(default)]
    items: Vec<Item>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Item {
    id: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    author: String,
    #[serde(default)]
    published: i64,
    #[serde(default)]
    timestamp_usec: Option<String>,
    #[serde(default)]
    crawl_time_msec: Option<String>,
    #[serde(default)]
    alternate: Vec<Link>,
    #[serde(default)]
    canonical: Vec<Link>,
    #[serde(default)]
    categories: Vec<String>,
    #[serde(default)]
    summary: Option<Text>,
    #[serde(default)]
    content: Option<Text>,
    #[serde(default)]
    origin: Option<Origin>,
}

#[derive(Deserialize)]
struct Link {
    href: String,
}

#[derive(Deserialize)]
struct Text {
    #[serde(default)]
    content: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Origin {
    #[serde(default)]
    stream_id: String,
    #[serde(default)]
    title: String,
}

impl Item {
    fn into_entry(self) -> Option<Entry> {
        let id = parse_item_id(&self.id)?;
        // Miniflux names the user (`user/1/state/…`) where the others write `user/-/state/…`.
        let read = self.categories.iter().any(|c| c.ends_with("/state/com.google/read"));
        let order_ms = self
            .timestamp_usec
            .and_then(|t| t.parse::<i64>().ok().map(|usec| usec / 1000))
            .or_else(|| self.crawl_time_msec.and_then(|t| t.parse().ok()))
            .unwrap_or(self.published * 1000);
        let url = self.alternate.into_iter().chain(self.canonical).map(|l| l.href).next().unwrap_or_default();
        let content = [self.content, self.summary].into_iter().flatten().map(|t| t.content).find(|c| !c.is_empty());
        let origin = self.origin.unwrap_or(Origin { stream_id: String::new(), title: String::new() });
        let content = content.unwrap_or_default();
        Some(Entry {
            id,
            title: self.title,
            url,
            author: self.author,
            published_at: dates::rfc3339_utc(self.published * 1000),
            status: if read { EntryStatus::Read } else { EntryStatus::Unread },
            reading_time: reading_minutes(&content),
            content,
            feed: EntryFeed { id: stream_id(&origin.stream_id), title: origin.title },
            order_ms,
        })
    }
}

/// Minutes to read some HTML, as Miniflux reckons them (265 words a minute), since this API doesn't say.
fn reading_minutes(html: &str) -> i64 {
    let mut text = String::with_capacity(html.len());
    let mut in_tag = false;
    for c in html.chars() {
        match c {
            '<' => in_tag = true,
            '>' => {
                in_tag = false;
                text.push(' ');
            }
            c if !in_tag => text.push(c),
            _ => {}
        }
    }
    i64::try_from(text.split_whitespace().count().div_ceil(265)).unwrap_or(0)
}

/// What a request carries besides the token.
enum Body<'a> {
    None,
    /// A form; every POST also gets the `T` token.
    Form(&'a [(&'a str, String)]),
}

impl Client {
    pub fn new(http: reqwest::Client, cfg: &config::GReader) -> Self {
        Self {
            http,
            base: cfg.url.clone(),
            username: cfg.username.clone(),
            password: cfg.password.clone(),
            auth: Mutex::new(None),
            edit_token: Mutex::new(None),
            streams: Mutex::new(HashMap::new()),
        }
    }

    fn not_greader(&self) -> Error {
        Error::Setup(format!(
            "{} doesn't look like a Google Reader API server. For FreshRSS the address ends in /api/greader.php.",
            self.base
        ))
    }

    fn unreachable(&self, e: reqwest::Error) -> Error {
        Error::unreachable(Service::GReader, &self.base, e)
    }

    /// Logs in with the username and password: `Auth=<token>` comes back.
    async fn login(&self) -> Result<String, Error> {
        let form = [("Email", self.username.as_str()), ("Passwd", self.password.as_str())];
        let resp = self
            .http
            .post(format!("{}/accounts/ClientLogin", self.base))
            .form(&form)
            .send()
            .await
            .map_err(|e| self.unreachable(e))?;
        match resp.status().as_u16() {
            200 => {
                let text = resp.text().await.map_err(|e| self.unreachable(e))?;
                let token = text.lines().find_map(|line| line.trim().strip_prefix("Auth=")).map(str::to_string);
                let token = token.filter(|t| !t.is_empty()).ok_or_else(|| self.not_greader())?;
                *lock(&self.auth) = Some(token.clone());
                Ok(token)
            }
            401 | 403 => {
                Err(Error::Rejected("The Google Reader API server turned down the username or password".into()))
            }
            404 | 405 => Err(self.not_greader()),
            code => Err(Error::unexpected(format!("The Google Reader API server answered with HTTP {code}"))),
        }
    }

    async fn auth(&self) -> Result<String, Error> {
        let known = lock(&self.auth).clone();
        match known {
            Some(token) => Ok(token),
            None => self.login().await,
        }
    }

    /// The `T` token edits need. It expires (FreshRSS: after a while; others: with the login), so a POST that
    /// is turned down fetches a new one and tries again.
    async fn edit_token(&self, auth: &str) -> Result<String, Error> {
        if let Some(token) = lock(&self.edit_token).clone() {
            return Ok(token);
        }
        let resp = self
            .http
            .get(format!("{}/reader/api/0/token", self.base))
            .header(reqwest::header::AUTHORIZATION, format!("GoogleLogin auth={auth}"))
            .send()
            .await
            .map_err(|e| self.unreachable(e))?;
        let resp = ok_status(Service::GReader, resp).await?;
        let token = resp.text().await.map_err(|e| self.unreachable(e))?.trim().to_string();
        *lock(&self.edit_token) = Some(token.clone());
        Ok(token)
    }

    /// Sends a request to `/reader/api/0/<path>`. A 401 means the login (or, for a POST, the `T` token) went stale:
    /// both are got again and the request sent once more. A POST turned down with a 400 gets a new `T` too.
    async fn send(
        &self,
        method: reqwest::Method,
        path: &str,
        query: &[(&str, String)],
        body: Body<'_>,
    ) -> Result<reqwest::Response, Error> {
        let mut retried = false;
        loop {
            let auth = self.auth().await?;
            let mut rb = self
                .http
                .request(method.clone(), format!("{}/reader/api/0/{path}", self.base))
                .header(reqwest::header::AUTHORIZATION, format!("GoogleLogin auth={auth}"))
                .query(query);
            let posting = matches!(body, Body::Form(_));
            if let Body::Form(form) = body {
                let token = self.edit_token(&auth).await?;
                let mut fields: Vec<(&str, &str)> = form.iter().map(|(k, v)| (*k, v.as_str())).collect();
                fields.push(("T", &token));
                rb = rb.form(&fields);
            }
            let resp = rb.send().await.map_err(|e| self.unreachable(e))?;
            let status = resp.status().as_u16();
            if !retried && (status == 401 || (posting && status == 400)) {
                retried = true;
                *lock(&self.edit_token) = None;
                if status == 401 {
                    *lock(&self.auth) = None;
                }
                continue;
            }
            return ok_status(Service::GReader, resp).await;
        }
    }

    async fn get_json<T: serde::de::DeserializeOwned>(&self, path: &str, query: &[(&str, String)]) -> Result<T, Error> {
        let resp = self.send(reqwest::Method::GET, path, query, Body::None).await?;
        resp.json().await.map_err(|e| Error::reading(Service::GReader, &self.base, e))
    }

    /// Checks the URL, username and password; returns the username.
    pub async fn check(&self) -> Result<String, Error> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct UserInfo {
            #[serde(default)]
            user_name: String,
        }
        *lock(&self.auth) = None;
        *lock(&self.edit_token) = None;
        self.login().await?;
        let resp = self
            .send(reqwest::Method::GET, "user-info", &[("output", "json".into())], Body::None)
            .await
            .map_err(|e| match e {
                Error::Http { status, .. } if status == reqwest::StatusCode::NOT_FOUND => self.not_greader(),
                e => e,
            })?;
        let info: UserInfo = resp.json().await.map_err(|_| self.not_greader())?;
        Ok(if info.user_name.is_empty() { self.username.clone() } else { info.user_name })
    }

    async fn subscriptions(&self) -> Result<Vec<Subscription>, Error> {
        let list: Subscriptions = self.get_json("subscription/list", &[("output", "json".into())]).await?;
        let mut streams = HashMap::new();
        for sub in &list.subscriptions {
            streams.insert(stream_id(&sub.id), sub.id.clone());
            for category in &sub.categories {
                streams.insert(stream_id(&category.id), category.id.clone());
            }
        }
        *lock(&self.streams) = streams;
        Ok(list.subscriptions)
    }

    /// Unread counts by stream, or `None` from a server without them (Miniflux answers `[]`).
    async fn unread_counts(&self) -> Result<Option<HashMap<String, i64>>, Error> {
        let value: serde_json::Value = self.get_json("unread-count", &[("output", "json".into())]).await?;
        let Some(counts) = value.get("unreadcounts").and_then(|c| c.as_array()) else { return Ok(None) };
        let count = |v: &serde_json::Value| v.as_i64().or_else(|| v.as_str().and_then(|s| s.parse().ok()));
        Ok(Some(
            counts.iter().filter_map(|c| Some((c.get("id")?.as_str()?.to_string(), count(c.get("count")?)?))).collect(),
        ))
    }

    /// Every feed with its category (its first label) and unread count. Read counts are unknown in this API.
    pub async fn feed_counts(&self) -> Result<Vec<FeedCount>, Error> {
        let (subscriptions, unread) = tokio::try_join!(self.subscriptions(), self.unread_counts())?;
        Ok(subscriptions
            .into_iter()
            .map(|sub| {
                let category = match sub.categories.into_iter().next() {
                    Some(c) => {
                        let title = if c.label.is_empty() {
                            c.id.rsplit_once("/label/").map_or(c.id.clone(), |(_, name)| name.to_string())
                        } else {
                            c.label
                        };
                        Category { id: stream_id(&c.id), title }
                    }
                    // A feed in no folder (Inoreader has those); its category row can't be opened.
                    None => Category { id: 0, title: "Uncategorized".into() },
                };
                let count = unread.as_ref().map(|counts| counts.get(&sub.id).copied().unwrap_or(0));
                let feed = Feed { id: stream_id(&sub.id), title: sub.title, category };
                FeedCount { feed, unread: count, read: None }
            })
            .collect())
    }

    /// The stream a selection reads from. A feed or category the client hasn't seen yet (the picker counted
    /// before a restart of the client, say) is looked up once more on the server.
    async fn stream(&self, sel: Selection) -> Result<String, Error> {
        let id = match sel {
            Selection::All => return Ok(READING_LIST.into()),
            // The "Uncategorized" row `feed_counts` makes for feeds in no folder: there is no stream for it.
            Selection::Category { id: 0 } => {
                return Err(Error::Refused("These feeds are in no folder: open them one at a time".into()));
            }
            Selection::Category { id } | Selection::Feed { id } => id,
        };
        if let Some(stream) = lock(&self.streams).get(&id).cloned() {
            return Ok(stream);
        }
        self.subscriptions().await?;
        lock(&self.streams)
            .get(&id)
            .cloned()
            .ok_or_else(|| Error::Refused("That feed or category isn't on the server any more".into()))
    }

    /// One listing of item ids.
    async fn item_ids(&self, query: &[(&str, String)]) -> Result<ItemRefs, Error> {
        let mut query = query.to_vec();
        query.push(("output", "json".into()));
        self.get_json("stream/items/ids", &query).await
    }

    /// The items with these ids, in this order.
    async fn contents(&self, ids: &[i64]) -> Result<Vec<Entry>, Error> {
        let mut entries: HashMap<i64, Entry> = HashMap::new();
        for chunk in ids.chunks(BATCH) {
            let form: Vec<(&str, String)> = chunk.iter().map(|&id| ("i", long_item_id(id))).collect();
            let resp = self
                .send(reqwest::Method::POST, "stream/items/contents", &[("output", "json".into())], Body::Form(&form))
                .await?;
            let contents: Contents = resp.json().await.map_err(|e| Error::reading(Service::GReader, &self.base, e))?;
            entries.extend(contents.items.into_iter().filter_map(Item::into_entry).map(|e| (e.id, e)));
        }
        Ok(ids.iter().filter_map(|id| entries.remove(id)).collect())
    }

    /// One page of the selection's unread or read entries, oldest or newest first, loaded as you go. `past` is
    /// where the page starts, in unix seconds of the server's order (see `Entry::order_millis`): `ot` oldest
    /// first, `nt` newest first; 0 starts at the beginning. FreshRSS takes `ot` as inclusive and Miniflux as
    /// strict; `paging::next_page` starts a second early either way, and drops what comes back twice.
    ///
    /// The total is how many ids one listing found past `past`, up to `ID_LIST` (one more if there are more), as
    /// this API counts nothing itself.
    pub async fn entries(
        &self,
        sel: Selection,
        status: EntryStatus,
        order: Order,
        past: i64,
        limit: usize,
    ) -> Result<EntryPage, Error> {
        let limit = limit.clamp(1, MAX_PAGE);
        let mut query = vec![("n", ID_LIST.max(limit).to_string())];
        match (status, sel) {
            // Miniflux has no `it` on the reading list, but every server has the read items as a stream.
            (EntryStatus::Read, Selection::All) => query.push(("s", READ.into())),
            (EntryStatus::Read, _) => query.extend([("s", self.stream(sel).await?), ("it", READ.into())]),
            (EntryStatus::Unread, _) => query.extend([("s", self.stream(sel).await?), ("xt", READ.into())]),
        }
        if order == Order::OldestFirst {
            query.push(("r", "o".into()));
        }
        if past > 0 {
            query.push((if order == Order::OldestFirst { "ot" } else { "nt" }, past.to_string()));
        }
        let refs = self.item_ids(&query).await.map_err(|e| no_labels(sel, e))?;
        let ids = refs.ids();
        let total = i64::try_from(ids.len()).unwrap_or(i64::MAX) + i64::from(refs.next().is_some());
        let mut entries = self.contents(&ids[..ids.len().min(limit)]).await?;
        // Miniflux ignores `it` on a feed and lists its unread items too.
        entries.retain(|e| e.status == status);
        Ok(EntryPage { total, entries })
    }

    /// A page of the items matching `query`, read and unread, in every feed, in the server's order. FreshRSS
    /// answers 400 when its search isn't there, Miniflux `[]`: both become "This server has no search".
    pub async fn search(&self, query: &str, offset: usize, limit: usize) -> Result<EntryPage, Error> {
        let limit = limit.clamp(1, MAX_PAGE);
        let params = [
            ("q", query.to_string()),
            ("s", READING_LIST.to_string()),
            ("n", (offset + limit).max(ID_LIST).to_string()),
            ("output", "json".to_string()),
        ];
        let value: serde_json::Value = self.get_json("search/items/ids", &params).await.map_err(|e| match e {
            Error::Http { status, .. } if matches!(status.as_u16(), 400 | 404 | 405 | 501) => {
                Error::Refused(NO_SEARCH.into())
            }
            e => e,
        })?;
        if value.get("itemRefs").is_none() {
            return Err(Error::Refused(NO_SEARCH.into()));
        }
        let refs: ItemRefs =
            serde_json::from_value(value).map_err(|_| Error::unexpected("Unexpected search results"))?;
        let ids = refs.ids();
        let total = i64::try_from(ids.len()).unwrap_or(i64::MAX) + i64::from(refs.next().is_some());
        let page: Vec<i64> = ids.iter().copied().skip(offset).take(limit).collect();
        Ok(EntryPage { total, entries: self.contents(&page).await? })
    }

    /// Every unread item id in the selection, a listing of `ID_LIST` at a time.
    async fn unread_ids(&self, sel: Selection) -> Result<Vec<i64>, Error> {
        let base = vec![("s", self.stream(sel).await?), ("xt", READ.into()), ("n", ID_LIST.to_string())];
        let mut ids = Vec::new();
        let mut from: Option<String> = None;
        for _ in 0..MAX_ID_PAGES {
            let mut query = base.clone();
            if let Some(c) = &from {
                query.push(("c", c.clone()));
            }
            let refs = self.item_ids(&query).await.map_err(|e| no_labels(sel, e))?;
            ids.extend(refs.ids());
            let next = refs.next();
            // A server that ignored `c` would hand out the same listing forever.
            if next.is_none() || next == from || refs.item_refs.is_empty() {
                break;
            }
            from = next;
        }
        Ok(ids)
    }

    /// The id of the selection's newest unread item, 0 if there is none. "Newest" means the highest id, not the
    /// latest `timestampUsec`: on FreshRSS an item's id is the microsecond it arrived, on Miniflux an id that
    /// only grows, so on both a higher id arrived later, whereas Miniflux's timestamps are publication times,
    /// which a feed can backdate. That is what app.rs compares too (`id > up_to`), so "mark all read" leaves
    /// alone what arrived after the list was counted.
    pub async fn newest_unread_id(&self, sel: Selection) -> Result<i64, Error> {
        Ok(self.unread_ids(sel).await?.into_iter().max().unwrap_or(0))
    }

    /// How many of the selection's unread items have an id up to `up_to`.
    pub async fn unread_count(&self, sel: Selection, up_to: i64) -> Result<i64, Error> {
        if up_to < 1 {
            return Ok(0);
        }
        let ids = self.unread_ids(sel).await?;
        Ok(i64::try_from(ids.into_iter().filter(|&id| id <= up_to).count()).unwrap_or(i64::MAX))
    }

    /// Marks the selection's unread items with an id up to `up_to` read; returns how many. (Not with
    /// `mark-all-as-read`, whose `ts` is a time, and on Miniflux a publication time.)
    pub async fn mark_read_up_to(&self, sel: Selection, up_to: i64) -> Result<usize, Error> {
        if up_to < 1 {
            return Ok(0);
        }
        let ids: Vec<i64> = self.unread_ids(sel).await?.into_iter().filter(|&id| id <= up_to).collect();
        self.set_status(&ids, EntryStatus::Read).await?;
        Ok(ids.len())
    }

    /// Adds or removes the read state, `BATCH` items per request.
    pub async fn set_status(&self, ids: &[i64], status: EntryStatus) -> Result<(), Error> {
        let change = if status == EntryStatus::Read { "a" } else { "r" };
        for chunk in ids.chunks(BATCH) {
            let mut form: Vec<(&str, String)> = chunk.iter().map(|&id| ("i", long_item_id(id))).collect();
            form.push((change, READ.into()));
            self.send(reqwest::Method::POST, "edit-tag", &[], Body::Form(&form)).await?;
        }
        Ok(())
    }

    /// The protocol can't ask the server to scrape the original page.
    pub async fn fetch_content(&self, _id: i64) -> Result<String, Error> {
        Err(Error::Refused(FETCH_CONTENT_REFUSED.into()))
    }

    /// Nor to refresh the feeds now: the server does that on its own schedule.
    pub async fn refresh_all(&self) -> Result<(), Error> {
        Err(Error::Refused(REFRESH_REFUSED.into()))
    }
}

/// What `f` says on a Google Reader API server.
pub const FETCH_CONTENT_REFUSED: &str = "Fetching the full article isn't available with a Google Reader API server";
/// What `r` says on a Google Reader API server.
pub const REFRESH_REFUSED: &str =
    "Refreshing feeds isn't available with a Google Reader API server; it refreshes them on its own";

/// Miniflux answers a category (a label stream) with a 500: say what that means.
fn no_labels(sel: Selection, e: Error) -> Error {
    match (sel, &e) {
        (Selection::Category { .. }, Error::Http { status, .. })
            if matches!(status.as_u16(), 400 | 404 | 500 | 501) =>
        {
            Error::Refused("This server can't list a category's items. Open its feeds one at a time.".into())
        }
        _ => e,
    }
}

/// A poisoned lock only means another task panicked mid-write; what it holds is still plain data.
fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::error::ErrorKind;
    use serde_json::json;
    use wiremock::matchers::{body_string_contains, header, method, path, query_param, query_param_is_missing};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const AUTH: &str = "GoogleLogin auth=ward/abc";

    fn client(server: &MockServer) -> Client {
        let cfg = config::GReader { url: server.uri(), username: "ward".into(), password: "pw".into() };
        Client::new(reqwest::Client::new(), &cfg)
    }

    /// Logging in, and the edit token: what every test needs.
    async fn logged_in(server: &MockServer) {
        Mock::given(method("POST"))
            .and(path("/accounts/ClientLogin"))
            .and(body_string_contains("Email=ward"))
            .and(body_string_contains("Passwd=pw"))
            .respond_with(ResponseTemplate::new(200).set_body_string("SID=ward/abc\nLSID=null\nAuth=ward/abc\n"))
            .mount(server)
            .await;
        Mock::given(method("GET"))
            .and(path("/reader/api/0/token"))
            .and(header("Authorization", AUTH))
            .respond_with(ResponseTemplate::new(200).set_body_string("tok123\n"))
            .mount(server)
            .await;
    }

    fn item(id: i64, usec: i64, read: bool) -> serde_json::Value {
        let mut categories = vec![READING_LIST.to_string(), "user/-/label/Dev".into()];
        if read {
            categories.push(READ.into());
        }
        json!({
            "id": long_item_id(id), "crawlTimeMsec": (usec / 1000).to_string(), "timestampUsec": usec.to_string(),
            "published": 1_788_739_200, "title": format!("Item {id}"),
            "alternate": [{ "href": format!("https://example.com/{id}") }],
            "categories": categories, "origin": { "streamId": "feed/2", "title": "Rust Blog" },
            "summary": { "content": "<p>Hi</p>" }, "author": "Sam"
        })
    }

    fn refs(ids: &[i64], continuation: Option<&str>) -> serde_json::Value {
        let refs: Vec<_> = ids.iter().map(|id| json!({ "id": id.to_string() })).collect();
        match continuation {
            Some(c) => json!({ "itemRefs": refs, "continuation": c }),
            None => json!({ "itemRefs": refs }),
        }
    }

    #[test]
    fn stream_ids_are_stable_positive_and_distinct() {
        assert_eq!(stream_id("feed/1"), stream_id("feed/1"));
        assert_ne!(stream_id("feed/1"), stream_id("feed/2"));
        for s in ["", "feed/1", "user/-/label/Dev", "feed/http://example.com/rss"] {
            assert!(stream_id(s) > 0, "{s}");
        }
        // FNV-1a's offset basis, sign bit cleared: pins the hash, so ids survive an update.
        assert_eq!(stream_id(""), 0x4bf2_9ce4_8422_2325);
    }

    #[test]
    fn item_ids_read_both_forms_and_write_the_long_one() {
        assert_eq!(parse_item_id("1793751434315910"), Some(1_793_751_434_315_910));
        assert_eq!(parse_item_id("tag:google.com,2005:reader/item/00065f684d0ec886"), Some(1_793_751_434_315_910));
        assert_eq!(parse_item_id("nonsense"), None);
        assert_eq!(long_item_id(11), "tag:google.com,2005:reader/item/000000000000000b");
        assert_eq!(parse_item_id(&long_item_id(1_793_751_434_315_910)), Some(1_793_751_434_315_910));
    }

    #[test]
    fn an_item_becomes_an_entry_ordered_by_its_timestamp() {
        let parsed: Item = serde_json::from_value(item(7, 1_793_751_434_315_910, true)).unwrap();
        let e = parsed.into_entry().unwrap();
        assert_eq!((e.id, e.title.as_str(), e.url.as_str()), (7, "Item 7", "https://example.com/7"));
        assert_eq!((e.author.as_str(), e.content.as_str()), ("Sam", "<p>Hi</p>"));
        assert_eq!(e.published_at, "2026-09-07T00:00:00.000Z");
        assert_eq!(e.status, EntryStatus::Read);
        assert_eq!((e.feed.id, e.feed.title.as_str()), (stream_id("feed/2"), "Rust Blog"));
        assert_eq!(e.order_ms, 1_793_751_434_315);
        assert_eq!(e.order_millis(), Some(1_793_751_434_315), "not the publication time");
        assert_eq!(e.reading_time, 1);
        assert_eq!(reading_minutes(&format!("<p>{}</p>", "word ".repeat(600))), 3);
        assert_eq!(reading_minutes(""), 0);
        // Miniflux names the user in the read state.
        let mut miniflux = item(8, 1_000_000, false);
        miniflux["categories"] = json!(["user/1/state/com.google/reading-list", "user/1/state/com.google/read"]);
        let e = serde_json::from_value::<Item>(miniflux).unwrap().into_entry().unwrap();
        assert_eq!(e.status, EntryStatus::Read);
    }

    #[test]
    fn debug_output_leaves_out_the_password() {
        let cfg = config::GReader {
            url: "https://r.example.com".into(),
            username: "ward".into(),
            password: "gr-secret".into(),
        };
        let shown = format!("{:?}", Client::new(reqwest::Client::new(), &cfg));
        assert!(!shown.contains("gr-secret") && shown.contains("<redacted>"), "{shown}");
    }

    #[tokio::test]
    async fn check_logs_in_and_asks_who_it_is() {
        let server = MockServer::start().await;
        logged_in(&server).await;
        Mock::given(method("GET"))
            .and(path("/reader/api/0/user-info"))
            .and(header("Authorization", AUTH))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "userId": "1", "userName": "ward" })))
            .expect(1)
            .mount(&server)
            .await;
        assert_eq!(client(&server).check().await.unwrap(), "ward");
    }

    #[tokio::test]
    async fn check_maps_a_wrong_password_and_a_wrong_address() {
        let server = MockServer::start().await;
        let c = client(&server);
        let guard = Mock::given(path("/accounts/ClientLogin"))
            .respond_with(ResponseTemplate::new(401).set_body_string("Unauthorized!"))
            .mount_as_scoped(&server)
            .await;
        let err = c.check().await.unwrap_err();
        assert_eq!(err.kind(), ErrorKind::Rejected);
        assert_eq!(err.message(), "The Google Reader API server turned down the username or password");
        drop(guard);

        // FreshRSS's address without /api/greader.php: its web page, which has no ClientLogin.
        let guard = Mock::given(path("/accounts/ClientLogin"))
            .respond_with(ResponseTemplate::new(404).set_body_string("<html>"))
            .mount_as_scoped(&server)
            .await;
        let err = c.check().await.unwrap_err();
        assert_eq!(err.kind(), ErrorKind::Setup);
        assert!(err.message().contains("doesn't look like a Google Reader API server"), "{}", err.message());
        drop(guard);

        // Some web page that answers anything with 200.
        let guard = Mock::given(path("/accounts/ClientLogin"))
            .respond_with(ResponseTemplate::new(200).set_body_string("<html>hello</html>"))
            .mount_as_scoped(&server)
            .await;
        assert!(c.check().await.unwrap_err().message().contains("doesn't look like"));
        drop(guard);
    }

    #[tokio::test]
    async fn check_reports_an_unreachable_server() {
        let cfg = config::GReader { url: "http://127.0.0.1:1".into(), username: "u".into(), password: "p".into() };
        let err = Client::new(reqwest::Client::new(), &cfg).check().await.unwrap_err();
        assert_eq!(err.kind(), ErrorKind::Unreachable);
        assert!(err.message().starts_with("Could not reach Google Reader API at http://127.0.0.1:1: "));
    }

    async fn subscriptions(server: &MockServer) {
        Mock::given(method("GET"))
            .and(path("/reader/api/0/subscription/list"))
            .and(query_param("output", "json"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "subscriptions": [
                { "id": "feed/1", "title": "cxx-qt", "categories": [{ "id": "user/-/label/Dev", "label": "Dev" }] },
                { "id": "feed/2", "title": "Rust Blog", "categories": [{ "id": "user/-/label/Dev", "label": "Dev" }] },
                { "id": "feed/3", "title": "TWiR", "categories": [{ "id": "user/1/label/News" }] },
                { "id": "feed/http://x.example/rss", "title": "Loose", "categories": [] }
            ]})))
            .mount(server)
            .await;
    }

    #[tokio::test]
    async fn feed_counts_are_the_subscriptions_with_unread_counts_and_no_read_ones() {
        let server = MockServer::start().await;
        logged_in(&server).await;
        subscriptions(&server).await;
        Mock::given(method("GET"))
            .and(path("/reader/api/0/unread-count"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "max": 24, "unreadcounts": [
                { "id": "feed/1", "count": 10 }, { "id": "feed/2", "count": "4" },
                { "id": "user/-/label/Dev", "count": 14 }, { "id": READING_LIST, "count": 14 }
            ]})))
            .mount(&server)
            .await;
        let counts = client(&server).feed_counts().await.unwrap();
        let shown: Vec<_> =
            counts.iter().map(|c| (c.feed.title.as_str(), c.feed.category.title.as_str(), c.unread, c.read)).collect();
        assert_eq!(
            shown,
            [
                ("cxx-qt", "Dev", Some(10), None),
                ("Rust Blog", "Dev", Some(4), None),
                ("TWiR", "News", Some(0), None),
                ("Loose", "Uncategorized", Some(0), None)
            ]
        );
        assert_eq!(counts[0].feed.id, stream_id("feed/1"));
        assert_eq!(counts[0].feed.category.id, stream_id("user/-/label/Dev"));
    }

    #[tokio::test]
    async fn a_server_without_unread_counts_leaves_them_unknown() {
        let server = MockServer::start().await;
        logged_in(&server).await;
        subscriptions(&server).await;
        // What Miniflux answers to an endpoint it doesn't have.
        Mock::given(path("/reader/api/0/unread-count"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
            .mount(&server)
            .await;
        let counts = client(&server).feed_counts().await.unwrap();
        assert!(counts.iter().all(|c| c.unread.is_none() && c.read.is_none()));
    }

    #[tokio::test]
    async fn unread_entries_are_listed_oldest_first_from_past_then_fetched_in_that_order() {
        let server = MockServer::start().await;
        logged_in(&server).await;
        subscriptions(&server).await;
        Mock::given(method("GET"))
            .and(path("/reader/api/0/stream/items/ids"))
            .and(query_param("s", "feed/2"))
            .and(query_param("xt", READ))
            .and(query_param("r", "o"))
            .and(query_param("ot", "1793751434"))
            .and(query_param("n", ID_LIST.to_string()))
            .and(query_param_is_missing("nt"))
            .and(header("Authorization", AUTH))
            .respond_with(ResponseTemplate::new(200).set_body_json(refs(&[11, 12, 13], Some("13"))))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/reader/api/0/stream/items/contents"))
            .and(body_string_contains("i=tag%3Agoogle.com%2C2005%3Areader%2Fitem%2F000000000000000b"))
            .and(body_string_contains("T=tok123"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "items": [
                item(12, 2_000_000, false), item(11, 1_000_000, false)
            ]})))
            .expect(1)
            .mount(&server)
            .await;
        let c = client(&server);
        c.subscriptions().await.unwrap(); // what the picker's counts leave behind: the streams
        let sel = Selection::Feed { id: stream_id("feed/2") };
        let page = c.entries(sel, EntryStatus::Unread, Order::OldestFirst, 1_793_751_434, 2).await.unwrap();
        assert_eq!(page.entries.iter().map(|e| e.id).collect::<Vec<_>>(), [11, 12], "in the listing's order");
        assert_eq!(page.total, 4, "three ids and more after them");
        assert_eq!(page.entries[1].order_ms, 2_000);
    }

    #[tokio::test]
    async fn a_feed_it_doesnt_know_yet_is_looked_up_once() {
        let server = MockServer::start().await;
        logged_in(&server).await;
        subscriptions(&server).await;
        Mock::given(path("/reader/api/0/stream/items/ids"))
            .and(query_param("s", "feed/http://x.example/rss"))
            .respond_with(ResponseTemplate::new(200).set_body_json(refs(&[], None)))
            .expect(1)
            .mount(&server)
            .await;
        let c = client(&server);
        let sel = Selection::Feed { id: stream_id("feed/http://x.example/rss") };
        let page = c.entries(sel, EntryStatus::Unread, Order::NewestFirst, 0, 50).await.unwrap();
        assert_eq!((page.total, page.entries.len()), (0, 0));
        let err = c.entries(Selection::Feed { id: 42 }, EntryStatus::Unread, Order::NewestFirst, 0, 50).await;
        assert_eq!(err.unwrap_err().message(), "That feed or category isn't on the server any more");
    }

    #[tokio::test]
    async fn history_is_the_read_stream_newest_first_before_past() {
        let server = MockServer::start().await;
        logged_in(&server).await;
        Mock::given(method("GET"))
            .and(path("/reader/api/0/stream/items/ids"))
            .and(query_param("s", READ))
            .and(query_param("nt", "1793751435"))
            .and(query_param_is_missing("r"))
            .and(query_param_is_missing("it"))
            .respond_with(ResponseTemplate::new(200).set_body_json(refs(&[5, 4], None)))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/reader/api/0/stream/items/contents"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "items": [
                item(4, 4_000_000, true), item(5, 5_000_000, false)
            ]})))
            .mount(&server)
            .await;
        let c = client(&server);
        let page = c.entries(Selection::All, EntryStatus::Read, Order::NewestFirst, 1_793_751_435, 50).await.unwrap();
        // Item 5 isn't read after all (Miniflux ignores `it` on a feed): left out.
        assert_eq!(page.entries.iter().map(|e| e.id).collect::<Vec<_>>(), [4]);
        assert_eq!(page.total, 2);
    }

    #[tokio::test]
    async fn a_category_on_a_server_without_labels_says_so() {
        let server = MockServer::start().await;
        logged_in(&server).await;
        subscriptions(&server).await;
        Mock::given(path("/reader/api/0/stream/items/ids"))
            .respond_with(
                ResponseTemplate::new(500)
                    .set_body_json(json!({ "error_message": "googlereader: unknown stream type LabelStream" })),
            )
            .mount(&server)
            .await;
        let c = client(&server);
        let sel = Selection::Category { id: stream_id("user/-/label/Dev") };
        let err = c.entries(sel, EntryStatus::Unread, Order::OldestFirst, 0, 50).await.unwrap_err();
        assert_eq!(err.message(), "This server can't list a category's items. Open its feeds one at a time.");
        let err = c.newest_unread_id(sel).await.unwrap_err();
        assert_eq!(err.kind(), ErrorKind::Local);
    }

    #[tokio::test]
    async fn a_stale_login_logs_in_again_once() {
        let server = MockServer::start().await;
        logged_in(&server).await;
        Mock::given(path("/reader/api/0/stream/items/ids"))
            .and(header("Authorization", "GoogleLogin auth=old"))
            .respond_with(ResponseTemplate::new(401))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(path("/reader/api/0/stream/items/ids"))
            .and(header("Authorization", AUTH))
            .respond_with(ResponseTemplate::new(200).set_body_json(refs(&[], None)))
            .expect(1)
            .mount(&server)
            .await;
        let c = client(&server);
        *lock(&c.auth) = Some("old".into());
        assert_eq!(c.newest_unread_id(Selection::All).await.unwrap(), 0);
    }

    #[tokio::test]
    async fn a_password_turned_down_while_reading_is_rejected() {
        let server = MockServer::start().await;
        logged_in(&server).await;
        Mock::given(path("/reader/api/0/stream/items/ids"))
            .respond_with(ResponseTemplate::new(401))
            .expect(2)
            .mount(&server)
            .await;
        let err = client(&server).newest_unread_id(Selection::All).await.unwrap_err();
        assert_eq!(err.kind(), ErrorKind::Rejected);
        assert_eq!(
            err.message(),
            "Google Reader API turned down the password (401 Unauthorized). Check it in Settings."
        );
    }

    #[tokio::test]
    async fn a_stale_edit_token_is_fetched_again() {
        let server = MockServer::start().await;
        logged_in(&server).await;
        Mock::given(method("POST"))
            .and(path("/reader/api/0/edit-tag"))
            .and(body_string_contains("T=stale"))
            .respond_with(ResponseTemplate::new(401).set_body_string("Unauthorized!"))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/reader/api/0/edit-tag"))
            .and(body_string_contains("T=tok123"))
            .and(body_string_contains("r=user%2F-%2Fstate%2Fcom.google%2Fread"))
            .respond_with(ResponseTemplate::new(200).set_body_string("OK"))
            .expect(1)
            .mount(&server)
            .await;
        let c = client(&server);
        *lock(&c.edit_token) = Some("stale".into());
        c.set_status(&[5], EntryStatus::Unread).await.unwrap();
    }

    #[tokio::test]
    async fn set_status_sends_batches_and_skips_empty() {
        let server = MockServer::start().await;
        logged_in(&server).await;
        Mock::given(method("POST"))
            .and(path("/reader/api/0/edit-tag"))
            .and(body_string_contains("a=user%2F-%2Fstate%2Fcom.google%2Fread"))
            .respond_with(ResponseTemplate::new(200).set_body_string("OK"))
            .expect(2)
            .mount(&server)
            .await;
        let c = client(&server);
        let ids: Vec<i64> = (1..=BATCH as i64 + 1).collect();
        c.set_status(&ids, EntryStatus::Read).await.unwrap();
        c.set_status(&[], EntryStatus::Read).await.unwrap();
        let requests = server.received_requests().await.unwrap();
        let edits: Vec<_> = requests.iter().filter(|r| r.url.path().ends_with("edit-tag")).collect();
        let count = |body: &[u8]| String::from_utf8_lossy(body).matches("i=").count();
        assert_eq!(edits.iter().map(|r| count(&r.body)).collect::<Vec<_>>(), [BATCH, 1]);
    }

    #[tokio::test]
    async fn mark_read_up_to_marks_the_unread_ids_up_to_it_across_listings() {
        let server = MockServer::start().await;
        logged_in(&server).await;
        Mock::given(path("/reader/api/0/stream/items/ids"))
            .and(query_param("s", READING_LIST))
            .and(query_param("xt", READ))
            .and(query_param_is_missing("c"))
            .respond_with(ResponseTemplate::new(200).set_body_json(refs(&[30, 10], Some("2"))))
            .mount(&server)
            .await;
        Mock::given(path("/reader/api/0/stream/items/ids"))
            .and(query_param("c", "2"))
            .respond_with(ResponseTemplate::new(200).set_body_json(refs(&[20, 40], None)))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/reader/api/0/edit-tag"))
            .respond_with(ResponseTemplate::new(200).set_body_string("OK"))
            .expect(1)
            .mount(&server)
            .await;
        let c = client(&server);
        assert_eq!(c.newest_unread_id(Selection::All).await.unwrap(), 40);
        assert_eq!(c.unread_count(Selection::All, 30).await.unwrap(), 3);
        assert_eq!(c.unread_count(Selection::All, 0).await.unwrap(), 0);
        assert_eq!(c.mark_read_up_to(Selection::All, 20).await.unwrap(), 2);
        assert_eq!(c.mark_read_up_to(Selection::All, 0).await.unwrap(), 0);
        let requests = server.received_requests().await.unwrap();
        let edit = requests.iter().find(|r| r.url.path().ends_with("edit-tag")).unwrap();
        let body = String::from_utf8_lossy(&edit.body);
        assert!(body.contains("item%2F000000000000000a") && body.contains("item%2F0000000000000014"), "{body}");
        assert!(!body.contains("item%2F000000000000001e"), "30 is past up_to: {body}");
    }

    #[tokio::test]
    async fn search_lists_matches_then_fetches_the_page() {
        let server = MockServer::start().await;
        logged_in(&server).await;
        Mock::given(method("GET"))
            .and(path("/reader/api/0/search/items/ids"))
            .and(query_param("q", "rust async"))
            .and(query_param("s", READING_LIST))
            .respond_with(ResponseTemplate::new(200).set_body_json(refs(&[9, 8, 7], None)))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/reader/api/0/stream/items/contents"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "items": [item(8, 8_000_000, true)] })))
            .mount(&server)
            .await;
        let page = client(&server).search("rust async", 1, 1).await.unwrap();
        assert_eq!((page.total, page.entries[0].id), (3, 8));
    }

    #[tokio::test]
    async fn a_server_without_search_says_so() {
        for answer in [
            ResponseTemplate::new(400).set_body_string("Bad Request!"),
            ResponseTemplate::new(200).set_body_json(json!([])),
        ] {
            let server = MockServer::start().await;
            logged_in(&server).await;
            Mock::given(path("/reader/api/0/search/items/ids")).respond_with(answer).mount(&server).await;
            let err = client(&server).search("rust", 0, 50).await.unwrap_err();
            assert_eq!(err.message(), NO_SEARCH);
            assert_eq!(err.kind(), ErrorKind::Local);
        }
    }

    #[tokio::test]
    async fn fetching_and_refreshing_are_turned_down_without_a_request() {
        let server = MockServer::start().await;
        let c = client(&server);
        assert_eq!(c.fetch_content(1).await.unwrap_err().message(), FETCH_CONTENT_REFUSED);
        assert_eq!(c.refresh_all().await.unwrap_err().message(), REFRESH_REFUSED);
        assert!(server.received_requests().await.unwrap().is_empty());
    }
}
