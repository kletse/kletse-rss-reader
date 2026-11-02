//! Paging through a selection: where the next page starts, and what a page adds to the queue.
//! Unread items come oldest first (by publication time), history most recently read first.

use crate::dates;
use crate::miniflux::{Entry, EntryPage, HistoryPage};
use std::collections::HashSet;

pub const PAGE: usize = 50;

/// "unread": read the unread items, oldest first. "history": browse read items, the most recently read first.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Unread,
    History,
}

impl Mode {
    pub fn other(self) -> Mode {
        match self {
            Mode::Unread => Mode::History,
            Mode::History => Mode::Unread,
        }
    }
}

/// `past`: unix seconds the page starts after (unread) or before (history); 0 is the start of the list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PageRequest {
    pub past: i64,
    pub limit: usize,
}

/// Where the page after `queue` starts, and how long it is.
///
/// Not an offset: reading moves items out of the server's unread list (j here, or a phone meanwhile) and m moves
/// them back, so a count of loaded items drifts and skips some, and bookkeeping to correct it can't see what happens
/// elsewhere. Instead the page starts at the newest loaded item's publication time (the oldest's, in history), which
/// no status change moves. Miniflux filters on whole seconds and doesn't order items published in the same second,
/// so the page starts a second early and is longer by the loaded items in that overlap; `merge_page` drops them
/// again. Nothing is skipped, whatever was read or marked unread in the meantime, here or elsewhere.
pub fn next_page(queue: &[Entry], mode: Mode) -> PageRequest {
    let times: Vec<i64> = queue.iter().filter_map(|e| dates::unix_millis(&e.published_at)).collect();
    let history = mode == Mode::History;
    let edge = if history { times.iter().min() } else { times.iter().max() };
    let Some(&edge) = edge else { return PageRequest { past: 0, limit: PAGE } };
    let past = if history { edge.div_euclid(1000) + 1 } else { (edge + 999).div_euclid(1000) - 1 };
    let bound = past * 1000;
    let overlap = times.iter().filter(|&&t| if history { t < bound } else { t > bound }).count();
    PageRequest { past, limit: PAGE + overlap }
}

/// A page to load: unread ones by publication time, history by when items were read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Request {
    Unread(PageRequest),
    History(HistoryPage),
}

/// Where the history goes on. Pages come by `changed_at`, newest first. A full page may end in the middle of
/// entries read in the same second, which Miniflux returns in no particular order (marking a list read gives
/// them all the same moment), so that second's entries are put back and fetched by id instead. After that it goes
/// on below the second. Nothing is skipped or shown twice, and items read meanwhile (here or elsewhere) just move
/// to the top, which the queue doesn't come back to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HistoryCursor {
    /// The most recently read.
    Start,
    /// Read before this second.
    Below(i64),
    /// The rest of what was read in this second, with ids below `before_id` (0: all of it).
    Within {
        second: i64,
        before_id: i64,
    },
    End,
}

impl HistoryCursor {
    pub fn request(self) -> Option<HistoryPage> {
        Some(match self {
            HistoryCursor::Start => HistoryPage::ChangedBefore { before: 0, limit: PAGE },
            HistoryCursor::Below(second) => HistoryPage::ChangedBefore { before: second, limit: PAGE },
            HistoryCursor::Within { second, before_id } => HistoryPage::AroundSecond { second, before_id, limit: PAGE },
            HistoryCursor::End => return None,
        })
    }
}

/// The second an entry was read in.
fn read_second(entry: &Entry) -> Option<i64> {
    dates::unix_millis(&entry.changed_at).map(|ms| ms.div_euclid(1000))
}

/// What a history page adds to the queue, and where the history goes on.
pub fn merge_history(queue: &[Entry], request: HistoryPage, page: EntryPage) -> (Vec<Entry>, HistoryCursor) {
    let have: HashSet<i64> = queue.iter().map(|e| e.id).collect();
    let new = |e: &Entry| !have.contains(&e.id);
    match request {
        HistoryPage::ChangedBefore { limit, .. } => {
            let full = page.entries.len() >= limit;
            match page.entries.last().and_then(read_second).filter(|_| full) {
                // The page may have cut that second in two: keep what was read after it, fetch it by id next.
                Some(last) => {
                    let fresh = page.entries.into_iter().filter(|e| new(e) && read_second(e) != Some(last)).collect();
                    (fresh, HistoryCursor::Within { second: last, before_id: 0 })
                }
                // The end of the history (or a server without read times, which can only be shown as it comes).
                None => (page.entries.into_iter().filter(new).collect(), HistoryCursor::End),
            }
        }
        HistoryPage::AroundSecond { second, limit, .. } => {
            let next = match page.entries.last() {
                Some(last) if page.entries.len() >= limit => HistoryCursor::Within { second, before_id: last.id },
                _ => HistoryCursor::Below(second),
            };
            // The window holds the second before too; that comes later, below this one.
            let fresh = page.entries.into_iter().filter(|e| new(e) && read_second(e) == Some(second)).collect();
            (fresh, next)
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct Merged {
    /// The page's items that aren't in the queue yet.
    pub fresh: Vec<Entry>,
    /// Whether the server has more beyond this page.
    pub more: bool,
    /// The whole selection: the queue with `fresh`, plus what's beyond.
    pub total: i64,
}

pub fn merge_page(queue: &[Entry], page: EntryPage) -> Merged {
    // The overlap, and items marked unread elsewhere meanwhile, come back again; skip what we already have.
    let have: HashSet<i64> = queue.iter().map(|e| e.id).collect();
    let returned = i64::try_from(page.entries.len()).unwrap_or(i64::MAX);
    let fresh: Vec<Entry> = page.entries.into_iter().filter(|e| !have.contains(&e.id)).collect();
    let beyond = (page.total - returned).max(0);
    // A page with nothing new can't lead anywhere new either.
    let more = !fresh.is_empty() && beyond > 0;
    let loaded = i64::try_from(queue.len() + fresh.len()).unwrap_or(i64::MAX);
    Merged { total: loaded + if more { beyond } else { 0 }, fresh, more }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::miniflux::{EntryFeed, EntryStatus};

    /// An entry published `minute` minutes after the start of October 2026, like the TS tests had.
    pub fn entry_at(id: i64, title: &str, status: EntryStatus, minute: i64) -> Entry {
        const OCTOBER_2026: i64 = 1_790_812_800_000;
        Entry {
            id,
            title: title.into(),
            url: format!("https://example.com/{id}"),
            author: String::new(),
            content: format!("<p>{title} body</p>"),
            published_at: dates::rfc3339_utc(OCTOBER_2026 + minute * 60_000),
            changed_at: dates::rfc3339_utc(OCTOBER_2026 + minute * 60_000),
            status,
            reading_time: 1,
            feed: EntryFeed { id: 1, title: "Feed".into() },
        }
    }

    pub fn entry(id: i64, title: &str) -> Entry {
        entry_at(id, title, EntryStatus::Unread, id)
    }

    fn second(e: &Entry) -> i64 {
        dates::unix_millis(&e.published_at).unwrap_or_default() / 1000
    }

    #[test]
    fn test_entries_have_the_dates_they_say() {
        assert_eq!(entry(0, "").published_at, "2026-10-01T00:00:00.000Z");
        assert_eq!(entry(61, "").published_at, "2026-10-01T01:01:00.000Z");
        assert_eq!(entry_at(1, "", EntryStatus::Unread, 31 * 24 * 60).published_at, "2026-11-01T00:00:00.000Z");
    }

    #[test]
    fn starts_at_the_beginning_of_the_list() {
        assert_eq!(next_page(&[], Mode::Unread), PageRequest { past: 0, limit: PAGE });
    }

    #[test]
    fn starts_a_second_before_the_newest_unread_item_and_is_longer_by_what_that_second_overlaps() {
        let tied = [entry(1, "A"), entry_at(2, "B", EntryStatus::Read, 5), entry_at(3, "C", EntryStatus::Unread, 5)];
        assert_eq!(next_page(&tied, Mode::Unread), PageRequest { past: second(&tied[2]) - 1, limit: PAGE + 2 });
    }

    #[test]
    fn in_history_starts_a_second_after_the_oldest_item() {
        let read = [entry_at(9, "A", EntryStatus::Read, 9), entry_at(8, "B", EntryStatus::Read, 8)];
        assert_eq!(next_page(&read, Mode::History), PageRequest { past: second(&read[1]) + 1, limit: PAGE + 1 });
    }

    #[test]
    fn drops_whats_loaded_already_and_stops_at_a_page_with_nothing_new() {
        let loaded = [entry(1, "A"), entry(2, "B")];
        let page = EntryPage { total: 10, entries: vec![entry(2, "B"), entry(3, "C")] };
        assert_eq!(merge_page(&loaded, page), Merged { fresh: vec![entry(3, "C")], more: true, total: 11 });
        let page = EntryPage { total: 10, entries: loaded.to_vec() };
        assert_eq!(merge_page(&loaded, page), Merged { fresh: vec![], more: false, total: 2 });
    }
}
