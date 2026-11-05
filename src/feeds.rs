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

/// What deleting a category does to the feeds in it, which is what the question before it has to say.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DeleteCategory {
    /// The server can't, or Onward doesn't offer it there.
    No,
    /// They stay, in the server's default category: FreshRSS.
    KeepsFeeds,
    /// They go with it, articles and all, with nothing to bring them back: Miniflux.
    TakesFeeds,
}

/// What can be changed about the feeds on this server, each by itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct FeedOps {
    pub add: bool,
    pub rename: bool,
    pub move_feed: bool,
    pub remove: bool,
    pub rename_category: bool,
    pub delete_category: DeleteCategory,
}

impl FeedOps {
    pub const NONE: Self = Self {
        add: false,
        rename: false,
        move_feed: false,
        remove: false,
        rename_category: false,
        delete_category: DeleteCategory::No,
    };
    pub const MINIFLUX: Self = Self {
        add: true,
        rename: true,
        move_feed: true,
        remove: true,
        rename_category: true,
        delete_category: DeleteCategory::TakesFeeds,
    };
    pub const FRESHRSS: Self = Self { delete_category: DeleteCategory::KeepsFeeds, ..Self::MINIFLUX };

    pub fn any(self) -> bool {
        self != Self::NONE
    }
}

/// What the configured server can do, one operation at a time: a kind of server isn't on or off as a whole.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Capabilities {
    /// Fetches the whole article on request (`f`).
    pub fetch_full: bool,
    /// Refreshes the feeds on request (`r`).
    pub refresh: bool,
    /// Searches entries (`/`). `None` when only trying will tell: Google Reader API servers differ, and the protocol
    /// has no way to ask.
    pub search: Option<bool>,
    /// Takes an OPML file and subscribes to what's in it (`i`).
    pub import_feeds: bool,
    /// Gives its feeds as an OPML file (`e`).
    pub export_feeds: bool,
    /// What can be changed about the feeds themselves (`m`, `a`).
    pub feeds: FeedOps,
    /// These are the sample articles, with no server behind them to import into or export from.
    pub samples: bool,
}

impl Capabilities {
    pub const EVERYTHING: Self = Self {
        fetch_full: true,
        refresh: true,
        search: Some(true),
        import_feeds: true,
        export_feeds: true,
        feeds: FeedOps::MINIFLUX,
        samples: false,
    };
    /// The built-in articles: all the keys work, and there's no server to take a feed list or keep a change.
    pub const SAMPLES: Self =
        Self { import_feeds: false, export_feeds: false, feeds: FeedOps::NONE, samples: true, ..Self::EVERYTHING };
}

/// Where a feed goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Destination {
    Existing(Category),
    /// Moving a feed to a new name is how a category comes to be: FreshRSS has no other way.
    New(String),
}

impl Destination {
    pub fn title(&self) -> &str {
        match self {
            Destination::Existing(category) => &category.title,
            Destination::New(title) => title,
        }
    }
}

/// A change to the feeds on the server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FeedChange {
    /// With no category, it goes wherever the server puts new feeds.
    Add {
        url: String,
        category: Option<Category>,
    },
    RenameFeed {
        id: i64,
        title: String,
    },
    MoveFeed {
        id: i64,
        to: Destination,
    },
    RemoveFeed {
        id: i64,
    },
    RenameCategory {
        category: Category,
        title: String,
    },
    DeleteCategory {
        category: Category,
    },
}

/// What a server's answer to a change names.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Applied {
    /// The feed that was added.
    pub feed: Option<i64>,
    /// A category as it is now. On a Google Reader API server a category's name is its id, so a rename changes that.
    pub category: Option<Category>,
}

/// Every feed, and every category, also those with no feed in them, which `Counts` can't show.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FeedList {
    pub counts: Counts,
    pub categories: Vec<Category>,
    /// The category the server won't rename or delete: FreshRSS's default.
    pub fixed: Option<i64>,
}

impl FeedList {
    /// `listed` by the server, and any it left out that a feed is in.
    pub fn new(counts: Counts, listed: Vec<Category>, fixed: Option<i64>) -> Self {
        let mut categories = listed;
        for feed in &counts.feeds {
            if !categories.iter().any(|c| c.id == feed.feed.category.id) {
                categories.push(feed.feed.category.clone());
            }
        }
        Self { counts, categories, fixed }
    }

    pub fn feed(&self, id: i64) -> Option<&FeedCount> {
        self.counts.feeds.iter().find(|f| f.feed.id == id)
    }

    pub fn in_category(&self, id: i64) -> impl Iterator<Item = &FeedCount> {
        self.counts.feeds.iter().filter(move |f| f.feed.category.id == id)
    }
}

/// How a change went, once the server has been asked what it did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Changed {
    pub applied: Applied,
    /// The list afterwards, `None` if that couldn't be had.
    pub list: Option<FeedList>,
    /// The server said yes and the list says otherwise: FreshRSS answers `OK` to edits it didn't make.
    pub problem: Option<String>,
}

/// What became of the articles of feeds a server just took.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Articles {
    /// The server fetched them before it answered: FreshRSS. What it has now is all there is.
    Fetched,
    /// It was told to fetch them, which it does in the background: Miniflux.
    OnTheirWay,
    /// Telling it to failed, for this reason: they come when it next refreshes by itself.
    NotAsked(String),
}

// What the app says when a server can't do something: `Error::Refused`'s words, and the toasts of keys the app
// already knows won't work.

/// What `f` says on a server that can't fetch the full article.
pub const FETCH_CONTENT_REFUSED: &str = "Fetching the full article isn't available with a Google Reader API server";
/// What `r` says on a server that can't be told to refresh.
pub const REFRESH_REFUSED: &str =
    "Refreshing feeds isn't available with a Google Reader API server; it refreshes them on its own";
/// What a search says on a server that turns out to have none.
pub const NO_SEARCH: &str = "This server has no search";
/// What `i` says where a server can't take an OPML file, and the picker of an empty account there.
pub const IMPORT_REFUSED: &str = "This server can't take a feed list from Onward. That works with FreshRSS, and with \
                                  Miniflux when you connect to it directly rather than through its Google Reader \
                                  integration.";
/// What `e` says where a server can't give its feeds.
pub const EXPORT_REFUSED: &str = "This server can't give Onward its feed list";
/// What `i` and `e` say in the sample articles.
pub const SAMPLES_HAVE_NO_FEED_LIST: &str =
    "These are sample articles, with no server behind them: nothing was imported or exported. Connect a server first.";
/// What `m` and `a` say where nothing about the feeds can be changed.
pub const MANAGE_REFUSED: &str = "Onward can only change feeds on Miniflux, connected to directly, and on FreshRSS. \
                                  This connection goes through a Google Reader API, where Onward can't tell which \
                                  server it is or check what it did.";
/// The picker of an empty account where neither importing nor adding works.
pub const NOTHING_CAN_BE_ADDED: &str = "This connection can't add feeds or take a feed list. That works with FreshRSS, \
                                        and with Miniflux when you connect to it by its address rather than through \
                                        its Google Reader integration.";
/// What `m` and `a` say in the sample articles. Nothing is pretended: a change here would be to nothing.
pub const SAMPLES_HAVE_NO_FEEDS: &str =
    "These are sample articles, with no server behind them, so their feeds can't be changed. Connect a server first.";
/// A subscribe that outlasted the wait. The server may well get there, so this mustn't read as a failure.
pub const ADD_STILL_WORKING: &str = "The server took too long to answer, and it may still be fetching the feed. Have \
                                     a look at your feeds in a minute before you add it again.";
/// A server was left working on an import. It may finish, so this mustn't read as a failure of the file.
pub const IMPORT_STILL_WORKING: &str = "The server took too long to answer, and it may still be working through the \
                                        list. Have a look at your feeds in a few minutes: whatever it managed will \
                                        be there, and importing the file again adds no duplicates.";
/// A category the server can't list on its own.
pub const NO_CATEGORY_ITEMS: &str = "This server can't list a category's items. Open its feeds one at a time.";
/// The row for feeds in no category, on a server with no stream for it.
pub const NO_FOLDER: &str = "These feeds are in no folder: open them one at a time";
/// A row that was on the server when the list was made.
pub const GONE: &str = "That feed or category isn't on the server any more";
