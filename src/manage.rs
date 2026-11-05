//! The feed list, for changing it: what the rows are, what each stage of the panel shows, what the questions before a
//! loss say, and what a change came to. `app.rs` holds a `Manage` and runs it, and the windows draw `ManageView`.
//!
//! The servers differ where it matters: deleting a category takes its feeds with it on Miniflux, for good, and leaves
//! them on FreshRSS. So the words go by `FeedOps::delete_category`, and nothing else decides. FreshRSS also answers `OK`
//! to edits it didn't make, so `check` looks at the list afterwards, on every server, and it's the list that's reported.

use crate::feeds::{
    Applied, Capabilities, Category, Changed, DeleteCategory, Destination, FeedChange, FeedCount, FeedList, FeedOps,
};
use crate::picker::by_title;
use serde::Serialize;

pub const TITLE: &str = "Manage feeds";
const ADD_LABEL: &str = "Feed address";
const ADD_HELP: &str = "The address of the feed itself. Your server fetches it, and nothing else sees it.";
const NEW_CATEGORY_ROW: &str = "New category…";
const NEW_CATEGORY_LABEL: &str = "Name of the new category";
const NOTHING_TEXT: &str = "Nothing here yet. Press a to add a feed by its address.";
const FIXED_CATEGORY: &str = "That's FreshRSS's default category, which it keeps as it is.";
/// A row the server has no stream for: nothing can be done to it.
const NO_FOLDER: &str = "These feeds are in no category, so there's nothing to rename or delete.";
/// What goes with a feed. Both servers delete its articles, and with them what's read and starred.
const LOSES: &str = "Its articles go too, read and unread, starred or not, and that can't be undone.";

/// What's under the cursor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Row {
    Category(Category),
    Feed(FeedCount),
}

/// Every category in order, each with its feeds.
pub fn rows(list: &FeedList) -> Vec<Row> {
    let mut categories: Vec<&Category> = list.categories.iter().collect();
    categories.sort_by(|a, b| by_title(&a.title, &b.title));
    let mut rows = Vec::new();
    for category in categories {
        rows.push(Row::Category(category.clone()));
        let mut feeds: Vec<&FeedCount> = list.in_category(category.id).collect();
        feeds.sort_by(|a, b| by_title(&a.feed.title, &b.feed.title));
        rows.extend(feeds.into_iter().cloned().map(Row::Feed));
    }
    rows
}

/// What a name is for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    Feed { id: i64, title: String },
    Category(Category),
}

impl Target {
    fn title(&self) -> &str {
        match self {
            Target::Feed { title, .. } => title,
            Target::Category(category) => &category.title,
        }
    }
}

/// Where the panel is.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum Stage {
    #[default]
    List,
    /// A feed's address, on its way into `into`, or wherever the server puts new ones.
    Add {
        into: Option<Category>,
        typed: String,
        error: String,
    },
    Rename {
        target: Target,
        typed: String,
        error: String,
    },
    /// Which category a feed goes to.
    Move {
        id: i64,
        title: String,
        from: i64,
        cursor: usize,
    },
    NewCategory {
        id: i64,
        title: String,
        from: i64,
        typed: String,
        error: String,
    },
    /// A task has it. It can't be stopped, so closing the panel leaves it running and its answer still counts.
    Working {
        words: String,
        /// What a failure goes back to.
        back: Box<Stage>,
    },
}

/// What the last change came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notice {
    pub text: String,
    pub worrying: bool,
}

/// A row as the windows draw it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ManageRow {
    pub label: String,
    /// Beside it: a feed's unread items, a category's feeds.
    pub note: String,
    /// 0 for a category, 1 for a feed.
    pub depth: u8,
}

/// Something that can be done here: the key, and what a button calls it and sends to `press`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ManageKey {
    pub key: &'static str,
    pub label: &'static str,
    pub button: &'static str,
}

/// The text field, on the stages that have one. `value` is where it starts: what was typed, after a failure.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ManageField {
    pub label: &'static str,
    pub value: String,
    pub action: &'static str,
}

/// The panel as the windows draw it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ManageView {
    /// "loading", "list", "field", "choose" or "working".
    pub stage: &'static str,
    pub title: String,
    pub text: String,
    /// What the last change came to, and whether that needs reading.
    pub notice: String,
    pub worrying: bool,
    pub error: String,
    pub rows: Vec<ManageRow>,
    pub cursor: usize,
    pub keys: Vec<ManageKey>,
    pub field: Option<ManageField>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Manage {
    /// `None` until it's in.
    pub list: Option<FeedList>,
    pub list_error: String,
    pub stage: Stage,
    pub cursor: usize,
    pub notice: Option<Notice>,
}

fn feeds_word(count: usize) -> String {
    format!("{count} feed{}", if count == 1 { "" } else { "s" })
}

fn unread_note(feed: &FeedCount) -> String {
    feed.unread.map_or_else(String::new, |unread| format!("{unread} unread"))
}

impl Manage {
    pub fn rows(&self) -> Vec<Row> {
        self.list.as_ref().map(rows).unwrap_or_default()
    }

    pub fn row(&self) -> Option<Row> {
        self.rows().into_iter().nth(self.cursor)
    }

    /// The category a new feed goes to: the one the cursor is in.
    pub fn category_here(&self) -> Option<Category> {
        let (list, row) = (self.list.as_ref()?, self.row()?);
        let category = match row {
            Row::Category(category) => category,
            Row::Feed(feed) => feed.feed.category,
        };
        (category.id != 0 && list.fixed != Some(category.id)).then_some(category)
    }

    /// Where a feed can go from `from`: every other category. The row after them is for a new one.
    fn destinations(&self, from: i64) -> Vec<Category> {
        let mut categories: Vec<Category> = self.list.iter().flat_map(|l| l.categories.iter().cloned()).collect();
        categories.retain(|c| c.id != from && c.id != 0);
        categories.sort_by(|a, b| by_title(&a.title, &b.title));
        categories
    }

    /// The category under the cursor of `Stage::Move`, or `None` on the row for a new one.
    pub fn destination(&self) -> Option<Category> {
        let Stage::Move { from, cursor, .. } = self.stage else { return None };
        self.destinations(from).into_iter().nth(cursor)
    }

    /// A category of this name, whatever its capitals: a new one mustn't be a second of those.
    pub fn named(&self, title: &str) -> Option<&Category> {
        let wanted = title.to_lowercase();
        self.list.as_ref()?.categories.iter().find(|c| c.title.to_lowercase() == wanted)
    }

    pub fn move_cursor(&mut self, delta: i32) {
        let n = match &self.stage {
            Stage::List => self.rows().len(),
            Stage::Move { from, .. } => self.destinations(*from).len() + 1,
            _ => 0,
        };
        let Ok(n) = i64::try_from(n) else { return };
        if n == 0 {
            return;
        }
        let step = |at: usize| usize::try_from((i64::try_from(at).unwrap_or(0) + i64::from(delta)).rem_euclid(n));
        match &mut self.stage {
            Stage::Move { cursor, .. } => *cursor = step(*cursor).unwrap_or(0),
            _ => self.cursor = step(self.cursor).unwrap_or(0),
        }
    }

    /// Picks a row by itself, for a click.
    pub fn place_cursor(&mut self, row: usize) {
        match &mut self.stage {
            Stage::Move { cursor, .. } => *cursor = row,
            _ => self.cursor = row,
        }
    }

    /// Puts the cursor on what's just been changed, wherever the new order has it.
    pub fn follow(&mut self, wanted: impl Fn(&Row) -> bool) {
        let rows = self.rows();
        self.cursor = rows.iter().position(wanted).unwrap_or(self.cursor.min(rows.len().saturating_sub(1)));
    }

    /// A list that's come in. The cursor stays on the same row.
    pub fn take_list(&mut self, list: FeedList) {
        let here = self.row();
        self.list = Some(list);
        self.list_error.clear();
        self.follow(|row| match (row, &here) {
            (Row::Category(a), Some(Row::Category(b))) => a.id == b.id,
            (Row::Feed(a), Some(Row::Feed(b))) => a.feed.id == b.feed.id,
            _ => false,
        });
    }

    /// Why the cursor's row can't have a name of its own or be deleted, if it's a category that can't.
    pub fn category_refusal(&self, category: &Category) -> Option<&'static str> {
        if category.id == 0 {
            Some(NO_FOLDER)
        } else if self.list.as_ref().is_some_and(|l| l.fixed == Some(category.id)) {
            Some(FIXED_CATEGORY)
        } else {
            None
        }
    }

    /// What can be done on the cursor's row. What the server can't do isn't there at all.
    fn list_keys(&self, can: Capabilities) -> Vec<ManageKey> {
        let key = |key, label, button| ManageKey { key, label, button };
        let ops = can.feeds;
        let mut keys = Vec::new();
        if ops.add {
            keys.push(key("a", "Add", "manage_add"));
        }
        match self.row() {
            Some(Row::Feed(_)) => {
                if ops.rename {
                    keys.push(key("r", "Rename", "manage_rename"));
                }
                if ops.move_feed {
                    keys.push(key("m", "Move", "manage_move"));
                }
                if ops.remove {
                    keys.push(key("d", "Remove", "manage_delete"));
                }
            }
            Some(Row::Category(category)) if self.category_refusal(&category).is_none() => {
                if ops.rename_category {
                    keys.push(key("r", "Rename", "manage_rename"));
                }
                if ops.delete_category != DeleteCategory::No {
                    keys.push(key("d", "Delete", "manage_delete"));
                }
            }
            _ => {}
        }
        if can.refresh {
            keys.push(key("f", "Fetch now", "manage_fetch"));
        }
        keys
    }

    fn list_rows(&self) -> Vec<ManageRow> {
        let list = self.list.as_ref();
        let in_category = |id| list.map_or(0, |l| l.in_category(id).count());
        self.rows()
            .iter()
            .map(|row| match row {
                Row::Category(category) => {
                    ManageRow { label: category.title.clone(), note: feeds_word(in_category(category.id)), depth: 0 }
                }
                Row::Feed(feed) => ManageRow { label: feed.feed.title.clone(), note: unread_note(feed), depth: 1 },
            })
            .collect()
    }

    pub fn view(&self, can: Capabilities) -> ManageView {
        let (notice, worrying) = self.notice.as_ref().map_or((String::new(), false), |n| (n.text.clone(), n.worrying));
        let mut view = ManageView {
            stage: "list",
            title: TITLE.to_string(),
            text: String::new(),
            notice,
            worrying,
            error: String::new(),
            rows: Vec::new(),
            cursor: self.cursor,
            keys: Vec::new(),
            field: None,
        };
        let field = |label, value: &str, action| Some(ManageField { label, value: value.to_string(), action });
        match &self.stage {
            Stage::List if self.list.is_none() => {
                view.stage = "loading";
                view.error.clone_from(&self.list_error);
            }
            Stage::List => {
                view.rows = self.list_rows();
                view.keys = self.list_keys(can);
                if view.rows.is_empty() {
                    NOTHING_TEXT.clone_into(&mut view.text);
                }
            }
            Stage::Add { into, typed, error } => {
                view.stage = "field";
                "Add a feed".clone_into(&mut view.title);
                view.text = match into {
                    Some(category) => format!("{ADD_HELP} It goes in “{}”.", category.title),
                    None => ADD_HELP.to_string(),
                };
                view.error.clone_from(error);
                view.field = field(ADD_LABEL, typed, "Add");
            }
            Stage::Rename { target, typed, error } => {
                view.stage = "field";
                view.title = format!("Rename “{}”", target.title());
                view.error.clone_from(error);
                view.field = field("New name", typed, "Rename");
            }
            Stage::Move { title, from, cursor, .. } => {
                view.stage = "choose";
                view.title = format!("Move “{title}”");
                view.rows = self
                    .destinations(*from)
                    .into_iter()
                    .map(|c| ManageRow { label: c.title, note: String::new(), depth: 0 })
                    .collect();
                view.rows.push(ManageRow { label: NEW_CATEGORY_ROW.into(), note: String::new(), depth: 0 });
                view.cursor = *cursor;
            }
            Stage::NewCategory { title, typed, error, .. } => {
                view.stage = "field";
                view.title = format!("Move “{title}”");
                view.error.clone_from(error);
                view.field = field(NEW_CATEGORY_LABEL, typed, "Move");
            }
            Stage::Working { words, .. } => {
                view.stage = "working";
                view.text.clone_from(words);
            }
        }
        view
    }

    /// Back where a failure leaves things, with why. Back to a list, that goes where the last change's report did.
    pub fn failed(&mut self, message: String) {
        let Stage::Working { back, .. } = std::mem::take(&mut self.stage) else { return };
        self.stage = *back;
        match &mut self.stage {
            Stage::Add { error, .. } | Stage::Rename { error, .. } | Stage::NewCategory { error, .. } => {
                *error = message
            }
            _ => self.notice = Some(Notice { text: message, worrying: true }),
        }
    }

    /// The list didn't come. With one on screen that stays, and this says why it may be out of date.
    pub fn failed_list(&mut self, message: String) {
        if self.list.is_some() {
            self.notice = Some(Notice { text: message, worrying: true });
        } else {
            self.list_error = message;
        }
    }

    /// The question before a feed goes.
    pub fn removing(&self, id: i64) -> String {
        let feed = self.list.as_ref().and_then(|l| l.feed(id));
        let title = feed.map_or("this feed", |f| f.feed.title.as_str());
        format!("Remove “{title}” from your server? {LOSES}")
    }

    /// What a window says while a change is on its way.
    pub fn saying(&self, change: &FeedChange) -> String {
        match change {
            FeedChange::Add { .. } => "Asking your server to fetch the feed…".into(),
            FeedChange::RemoveFeed { .. } => "Removing the feed…".into(),
            FeedChange::DeleteCategory { .. } => "Deleting the category…".into(),
            FeedChange::MoveFeed { .. } => "Moving the feed…".into(),
            FeedChange::RenameFeed { .. } | FeedChange::RenameCategory { .. } => "Renaming…".into(),
        }
    }

    /// In plain words, claiming only what the list shows.
    pub fn report(&self, change: &FeedChange, changed: &Changed, ops: FeedOps, refreshes: bool) -> Notice {
        if let Some(text) = &changed.problem {
            return Notice { text: text.clone(), worrying: true };
        }
        let before = self.list.as_ref();
        let title = |id: i64| before.and_then(|l| l.feed(id)).map_or("The feed", |f| f.feed.title.as_str());
        let text = match change {
            FeedChange::Add { .. } => return added(changed, refreshes),
            FeedChange::RenameFeed { title, .. } => format!("Renamed to “{title}”."),
            FeedChange::MoveFeed { id, to } => match to {
                Destination::Existing(category) => format!("Moved “{}” to “{}”.", title(*id), category.title),
                Destination::New(name) => format!("Moved “{}” to a new category, “{name}”.", title(*id)),
            },
            FeedChange::RemoveFeed { id } => format!("Removed “{}” and its articles.", title(*id)),
            FeedChange::RenameCategory { category, title } => format!("Renamed “{}” to “{title}”.", category.title),
            FeedChange::DeleteCategory { category } => {
                let count = before.map_or(0, |l| l.in_category(category.id).count());
                match (ops.delete_category, count) {
                    (_, 0) => format!("Deleted “{}”.", category.title),
                    (DeleteCategory::TakesFeeds, _) => {
                        format!("Deleted “{}”, with its {} and their articles.", category.title, feeds_word(count))
                    }
                    _ => format!("Deleted “{}”. Its {} kept.", category.title, kept(count)),
                }
            }
        };
        let unchecked = if changed.list.is_none() { " Onward couldn't get the list back to check." } else { "" };
        Notice { text: format!("{text}{unchecked}"), worrying: changed.list.is_none() }
    }
}

fn kept(count: usize) -> String {
    if count == 1 { "1 feed is".to_string() } else { format!("{count} feeds are") }
}

/// The question before a category goes, which is where the servers differ. `count` is what the server just said.
pub fn deleting(category: &Category, count: usize, ops: FeedOps) -> String {
    let title = &category.title;
    match (ops.delete_category, count) {
        (_, 0) => format!("Delete “{title}”? There are no feeds in it."),
        (DeleteCategory::TakesFeeds, _) => format!(
            "Delete “{title}” and the {} in it? Miniflux deletes the feeds with the category, and all their articles, \
             read and unread, starred or not. That can't be undone. To keep them, say no and move them to another \
             category first.",
            feeds_word(count)
        ),
        _ => format!(
            "Delete the category “{title}”? Its {} and their articles stay: FreshRSS moves them to its default category.",
            feeds_word(count)
        ),
    }
}

/// What came of adding a feed, by what's there: the server's yes doesn't say whether there's anything to read yet.
fn added(changed: &Changed, refreshes: bool) -> Notice {
    let feed = changed.list.as_ref().zip(changed.applied.feed).and_then(|(list, id)| list.feed(id));
    let Some(feed) = feed else {
        let text = "The server took it. Onward couldn't get the list back to see it.".to_string();
        return Notice { text, worrying: true };
    };
    let (title, category) = (&feed.feed.title, &feed.feed.category.title);
    let some = feed.unread.unwrap_or(0) + feed.read.unwrap_or(0);
    let text = if some > 0 {
        format!("Added “{title}” to “{category}”, with {some} article{}.", if some == 1 { "" } else { "s" })
    } else if refreshes {
        format!("Added “{title}” to “{category}”. No articles yet: press f to have the server fetch them now.")
    } else {
        format!("Added “{title}” to “{category}”. No articles yet: they come when the server next fetches it.")
    };
    Notice { text, worrying: false }
}

/// Whether there's still something to wait for after adding a feed.
pub fn articles_to_come(changed: &Changed) -> bool {
    let feed = changed.list.as_ref().zip(changed.applied.feed).and_then(|(list, id)| list.feed(id));
    feed.is_some_and(|feed| feed.unread.unwrap_or(0) + feed.read.unwrap_or(0) == 0)
}

/// What the list says the server didn't do, if anything: FreshRSS answers `OK` to a rename onto a name that's taken and
/// to an edit of a feed that isn't there.
pub fn check(change: &FeedChange, applied: &Applied, list: &FeedList) -> Option<String> {
    let said = "The server said yes, but";
    let category_named = |title: &str| list.categories.iter().find(|c| c.title.eq_ignore_ascii_case(title));
    let feed_here = |id: i64| list.feed(id);
    match change {
        FeedChange::Add { category, .. } => {
            let feed = applied.feed.and_then(feed_here);
            match (feed, category) {
                (None, _) => Some(format!("{said} the feed isn't in the list.")),
                (Some(feed), Some(category)) if feed.feed.category.title != category.title => Some(format!(
                    "Added “{}”, but it's in “{}” and not in “{}”. Move it with m.",
                    feed.feed.title, feed.feed.category.title, category.title
                )),
                _ => None,
            }
        }
        FeedChange::RenameFeed { id, title } => match feed_here(*id) {
            None => Some(crate::feeds::GONE.to_string()),
            Some(feed) if feed.feed.title != *title => {
                Some(format!("{said} the feed is still called “{}”. Nothing changed.", feed.feed.title))
            }
            Some(_) => None,
        },
        FeedChange::MoveFeed { id, to } => match feed_here(*id) {
            None => Some(crate::feeds::GONE.to_string()),
            Some(feed) if !feed.feed.category.title.eq_ignore_ascii_case(to.title()) => {
                Some(format!("{said} the feed is still in “{}”. Nothing changed.", feed.feed.category.title))
            }
            Some(_) => None,
        },
        FeedChange::RemoveFeed { id } => {
            feed_here(*id).map(|feed| format!("{said} “{}” is still there. Nothing changed.", feed.feed.title))
        }
        FeedChange::RenameCategory { category, title } => {
            let renamed = category_named(title).is_some();
            let old_gone = title.eq_ignore_ascii_case(&category.title) || category_named(&category.title).is_none();
            (!renamed || !old_gone).then(|| format!("{said} “{}” kept its name. Nothing changed.", category.title))
        }
        FeedChange::DeleteCategory { category } => list
            .categories
            .iter()
            .any(|c| c.id == category.id)
            .then(|| format!("{said} “{}” is still there. Nothing changed.", category.title)),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::feeds::{Counts, Feed};

    fn category(id: i64, title: &str) -> Category {
        Category { id, title: title.into() }
    }

    fn feed(id: i64, title: &str, in_category: &Category) -> FeedCount {
        let feed = Feed { id, title: title.into(), category: in_category.clone() };
        FeedCount { feed, unread: Some(3), read: Some(1) }
    }

    fn list() -> FeedList {
        let (news, tech) = (category(1, "News"), category(2, "Tech"));
        let feeds = vec![feed(10, "Zed", &news), feed(11, "Alpha", &news), feed(12, "Beta", &tech)];
        let categories = vec![tech.clone(), news.clone(), category(3, "Empty")];
        FeedList::new(Counts { feeds, starred: Some(0) }, categories, None)
    }

    #[test]
    fn rows_are_in_order_with_empty_categories_kept() {
        let labels: Vec<String> = Manage { list: Some(list()), ..Manage::default() }
            .view(Capabilities::EVERYTHING)
            .rows
            .into_iter()
            .map(|r| format!("{} [{}]", r.label, r.note))
            .collect();
        assert_eq!(
            labels,
            [
                "Empty [0 feeds]",
                "News [2 feeds]",
                "Alpha [3 unread]",
                "Zed [3 unread]",
                "Tech [1 feed]",
                "Beta [3 unread]"
            ]
        );
    }

    #[test]
    fn a_category_only_a_feed_names_is_listed_too() {
        let (news, listed) = (category(1, "News"), vec![category(2, "Tech")]);
        let counts = Counts { feeds: vec![feed(10, "A", &news)], starred: None };
        let list = FeedList::new(counts, listed, None);
        assert_eq!(list.categories.len(), 2);
    }

    #[test]
    fn the_keys_on_offer_are_the_ones_the_row_and_the_server_have() {
        let keys = |can, cursor| {
            let manage = Manage { list: Some(list()), cursor, ..Manage::default() };
            manage.view(can).keys.iter().map(|k| k.key).collect::<String>()
        };
        let everything = Capabilities::EVERYTHING;
        assert_eq!(keys(everything, 1), "ardf", "a category");
        assert_eq!(keys(everything, 2), "armdf", "a feed");
        let no_moving = FeedOps { move_feed: false, ..FeedOps::MINIFLUX };
        assert_eq!(keys(Capabilities { feeds: no_moving, ..everything }, 2), "ardf", "each by itself");
        assert_eq!(keys(Capabilities { refresh: false, ..everything }, 2), "armd");
        assert_eq!(keys(Capabilities { feeds: FeedOps::NONE, refresh: false, ..everything }, 2), "");
    }

    #[test]
    fn the_default_category_of_freshrss_offers_nothing_of_its_own() {
        let mut list = list();
        list.fixed = Some(1);
        let manage = Manage { list: Some(list), cursor: 1, ..Manage::default() };
        let keys: String = manage.view(Capabilities::EVERYTHING).keys.iter().map(|k| k.key).collect();
        assert_eq!(keys, "af");
        assert_eq!(manage.category_here(), None, "a new feed lands there anyway");
    }

    #[test]
    fn the_question_on_miniflux_names_the_feeds_and_their_articles_and_on_freshrss_says_they_stay() {
        let news = category(1, "News");
        let miniflux = deleting(&news, 3, FeedOps::MINIFLUX);
        for words in ["3 feeds", "all their articles", "can't be undone", "move them"] {
            assert!(miniflux.contains(words), "{words}: {miniflux}");
        }
        let freshrss = deleting(&news, 3, FeedOps::FRESHRSS);
        assert!(freshrss.contains("3 feeds and their articles stay"), "{freshrss}");
        assert!(!freshrss.contains("undone"));
        assert_eq!(deleting(&news, 0, FeedOps::MINIFLUX), deleting(&news, 0, FeedOps::FRESHRSS));
        assert!(deleting(&news, 1, FeedOps::MINIFLUX).contains("the 1 feed in it"));
    }

    #[test]
    fn what_the_list_shows_decides_not_the_servers_ok() {
        let no = Applied::default();
        let list = list();
        let rename = |id, title: &str| FeedChange::RenameFeed { id, title: title.into() };
        assert_eq!(check(&rename(10, "Zed"), &no, &list), None);
        assert!(check(&rename(10, "Other"), &no, &list).unwrap().contains("still called “Zed”"));
        assert_eq!(check(&rename(99, "Other"), &no, &list).unwrap(), crate::feeds::GONE);

        let moved = |to: &str| FeedChange::MoveFeed { id: 10, to: Destination::New(to.into()) };
        assert_eq!(check(&moved("News"), &no, &list), None);
        assert!(check(&moved("Tech"), &no, &list).unwrap().contains("still in “News”"));

        assert!(check(&FeedChange::RemoveFeed { id: 10 }, &no, &list).is_some());
        assert_eq!(check(&FeedChange::RemoveFeed { id: 99 }, &no, &list), None);

        // FreshRSS's rename onto a name that's taken: `OK`, and an SQL error in its log.
        let renamed = |title: &str| FeedChange::RenameCategory { category: category(2, "Tech"), title: title.into() };
        assert!(check(&renamed("News"), &no, &list).unwrap().contains("kept its name"));
        assert!(check(&renamed("Gone"), &no, &list).unwrap().contains("kept its name"));
        let done = FeedChange::RenameCategory { category: category(9, "Old"), title: "Tech".into() };
        assert_eq!(check(&done, &no, &list), None);

        let delete = |id| FeedChange::DeleteCategory { category: category(id, "X") };
        assert!(check(&delete(1), &no, &list).is_some());
        assert_eq!(check(&delete(9), &no, &list), None);

        let add = |into| FeedChange::Add { url: "u".into(), category: into };
        let found = Applied { feed: Some(10), category: None };
        assert_eq!(check(&add(None), &found, &list), None);
        assert!(check(&add(None), &no, &list).is_some());
        assert!(check(&add(Some(category(2, "Tech"))), &found, &list).unwrap().contains("it's in “News”"));
    }

    #[test]
    fn a_report_of_a_new_feed_is_honest_about_its_articles() {
        let manage = Manage::default();
        let (ops, url) = (FeedOps::MINIFLUX, "http://a.example/feed".to_string());
        let change = FeedChange::Add { url, category: None };
        let mut list = list();
        let applied = Applied { feed: Some(10), category: None };
        let changed = |list: &FeedList| Changed { applied: applied.clone(), list: Some(list.clone()), problem: None };
        let text = manage.report(&change, &changed(&list), ops, true).text;
        assert_eq!(text, "Added “Zed” to “News”, with 4 articles.");
        assert!(!articles_to_come(&changed(&list)));

        list.counts.feeds[0].unread = Some(0);
        list.counts.feeds[0].read = Some(0);
        assert!(articles_to_come(&changed(&list)));
        let text = manage.report(&change, &changed(&list), ops, true).text;
        assert!(text.contains("press f"), "{text}");
        let text = manage.report(&change, &changed(&list), ops, false).text;
        assert!(text.contains("when the server next fetches it"), "{text}");
    }
}
