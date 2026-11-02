//! Paging through a selection: where the next page starts, and what a page adds to the queue.
//! Unread items come oldest first, history newest first.

use crate::dates;
use crate::miniflux::{Entry, EntryPage, EntryStatus};
use std::collections::HashSet;

pub const PAGE: usize = 50;

/// "unread": read the unread items, oldest first. "history": browse read items, newest first.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Unread,
    History,
}

impl Mode {
    /// The list a mode pages through.
    pub fn status(self) -> EntryStatus {
        match self {
            Mode::Unread => EntryStatus::Unread,
            Mode::History => EntryStatus::Read,
        }
    }

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
    use crate::miniflux::EntryFeed;

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
