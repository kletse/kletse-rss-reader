//! What a feed server deals in, whichever kind it is: the entries, feeds and counts every client hands the app.

use serde::{Deserialize, Serialize};

use crate::dates;

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct Category {
    pub id: i64,
    pub title: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct Feed {
    pub id: i64,
    pub title: String,
    pub category: Category,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct EntryFeed {
    pub id: i64,
    pub title: String,
}

/// Read state of an entry, as Miniflux spells it. (Miniflux also has `removed`, which we never see.)
#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum EntryStatus {
    Read,
    Unread,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
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
    /// Where the server has the entry in its order, in unix milliseconds, when that isn't `published_at`: a
    /// Google Reader API server pages by its own timestamp (FreshRSS: when the entry arrived). 0 for Miniflux,
    /// which pages by `published_at`. See `order_millis`.
    #[serde(default)]
    pub order_ms: i64,
    /// Bookmarked on the server (`*`): Miniflux calls it `starred`, the Google Reader API a state tag.
    #[serde(default)]
    pub starred: bool,
}

impl Entry {
    /// The time the server orders and pages the entry by, in unix milliseconds: what `paging::next_page` starts
    /// the next page from. `published_at` stays the article's date, for the header.
    pub fn order_millis(&self) -> Option<i64> {
        if self.order_ms > 0 { Some(self.order_ms) } else { dates::unix_millis(&self.published_at) }
    }
}

/// One page of entries, plus how many the server has in total for the query.
#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct EntryPage {
    pub total: i64,
    pub entries: Vec<Entry>,
}

/// A page of entries when only their ids matter; serde skips the rest (content included).
/// What the picker shows: every feed with its category and its unread and read counts. `None` is a count the
/// server can't give: the Google Reader API has no read counts, and Miniflux's Google Reader integration no
/// unread ones either. The picker then lists the feed without a number.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct FeedCount {
    pub feed: Feed,
    pub unread: Option<i64>,
    pub read: Option<i64>,
}

/// What the picker shows: every feed with its counts, and how many entries are starred.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Counts {
    pub feeds: Vec<FeedCount>,
    /// `None` when the server can't count them; the picker then shows the row without a number.
    pub starred: Option<i64>,
}

/// Which set of entries to read.
#[derive(Debug, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Selection {
    All,
    Category {
        id: i64,
    },
    Feed {
        id: i64,
    },
    /// The starred entries of every feed, read and unread alike: a status doesn't narrow it (see `entries`).
    Starred,
}
