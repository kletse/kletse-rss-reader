//! Turns feed counts into the rows of the feeds picker.

use crate::miniflux::{FeedCount, Selection};
use crate::paging::Mode;
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PickerItem {
    pub selection: Selection,
    pub label: String,
    pub count: i64,
    /// 0 for "All …" and categories, 1 for feeds.
    pub depth: u8,
}

fn by_title(a: &str, b: &str) -> std::cmp::Ordering {
    a.to_lowercase().cmp(&b.to_lowercase()).then_with(|| a.cmp(b))
}

/// Turns feed counts into the picker rows: "All unread" (or "All read" in history mode),
/// then each category (sorted) with its feeds. Anything with nothing to show is left out.
pub fn build(counts: &[FeedCount], mode: Mode) -> Vec<PickerItem> {
    rows(counts, mode, false)
}

/// The list to go on to from the end of `current`: the next row of the same kind (a feed after a feed, a category
/// after a category) with something to show, in picker order, starting over at the top. None after "All …", or
/// when nothing else has anything.
pub fn next_after(counts: &[FeedCount], mode: Mode, current: Selection) -> Option<PickerItem> {
    // With the empty rows too: the list just read through may have nothing left, and still has its place.
    let all = rows(counts, mode, true);
    let at = all.iter().position(|r| r.selection == current)?;
    let same_kind = |s: &Selection| std::mem::discriminant(s) == std::mem::discriminant(&current);
    all.iter().cycle().skip(at + 1).take(all.len() - 1).find(|r| same_kind(&r.selection) && r.count > 0).cloned()
}

fn rows(counts: &[FeedCount], mode: Mode, keep_empty: bool) -> Vec<PickerItem> {
    let count_of = |fc: &FeedCount| if mode == Mode::History { fc.read } else { fc.unread };
    let total: i64 = counts.iter().map(count_of).sum();

    // Feeds with something to show, grouped by category id.
    let mut groups: BTreeMap<i64, (String, Vec<&FeedCount>)> = BTreeMap::new();
    for fc in counts.iter().filter(|fc| keep_empty || count_of(fc) > 0) {
        let category = &fc.feed.category;
        groups.entry(category.id).or_insert_with(|| (category.title.clone(), Vec::new())).1.push(fc);
    }
    let mut groups: Vec<_> = groups.into_iter().collect();
    groups.sort_by(|a, b| by_title(&a.1.0, &b.1.0));

    let mut rows = Vec::new();
    if total > 0 || keep_empty {
        let label = if mode == Mode::History { "All read" } else { "All unread" };
        rows.push(PickerItem { selection: Selection::All, label: label.into(), count: total, depth: 0 });
    }
    for (id, (title, mut feeds)) in groups {
        feeds.sort_by(|a, b| by_title(&a.feed.title, &b.feed.title));
        let count = feeds.iter().map(|fc| count_of(fc)).sum();
        rows.push(PickerItem { selection: Selection::Category { id }, label: title, count, depth: 0 });
        rows.extend(feeds.into_iter().map(|fc| PickerItem {
            selection: Selection::Feed { id: fc.feed.id },
            label: fc.feed.title.clone(),
            count: count_of(fc),
            depth: 1,
        }));
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::miniflux::{Category, Feed};

    fn fc(id: i64, title: &str, category: (i64, &str), unread: i64, read: i64) -> FeedCount {
        FeedCount {
            feed: Feed { id, title: title.into(), category: Category { id: category.0, title: category.1.into() } },
            unread,
            read,
        }
    }

    fn rows(items: &[PickerItem]) -> Vec<(&str, i64, u8)> {
        items.iter().map(|i| (i.label.as_str(), i.count, i.depth)).collect()
    }

    #[test]
    fn is_empty_when_nothing_is_unread() {
        assert_eq!(build(&[], Mode::Unread), vec![]);
        assert_eq!(build(&[fc(1, "A", (1, "Dev"), 0, 0)], Mode::Unread), vec![]);
    }

    #[test]
    fn lists_all_unread_then_categories_with_their_feeds_hiding_read_out_feeds() {
        let items = build(
            &[
                fc(1, "Zed", (1, "Dev"), 2, 0),
                fc(2, "Alpha", (1, "Dev"), 3, 0),
                fc(3, "Quiet", (2, "Blogs"), 0, 0),
                fc(4, "Loud", (2, "Blogs"), 1, 0),
                fc(5, "Nothing", (3, "Empty"), 0, 0),
            ],
            Mode::Unread,
        );
        assert_eq!(
            rows(&items),
            [("All unread", 6, 0), ("Blogs", 1, 0), ("Loud", 1, 1), ("Dev", 5, 0), ("Alpha", 3, 1), ("Zed", 2, 1)]
        );
        let selections: Vec<_> = items.iter().take(3).map(|i| i.selection).collect();
        assert_eq!(selections, [Selection::All, Selection::Category { id: 2 }, Selection::Feed { id: 4 }]);
    }

    #[test]
    fn the_next_list_is_the_next_row_of_the_same_kind_with_something_starting_over_at_the_top() {
        let counts = [
            fc(1, "Alpha", (1, "Dev"), 0, 4),
            fc(2, "Beta", (1, "Dev"), 0, 0),
            fc(3, "Gamma", (1, "Dev"), 2, 0),
            fc(4, "Delta", (2, "Blogs"), 1, 0),
            fc(5, "Omega", (3, "Watches"), 0, 0),
        ];
        let next = |sel| next_after(&counts, Mode::Unread, sel).map(|i| i.label);
        // Rows: All unread, Blogs, Delta, Dev, Alpha, Beta, Gamma, Watches, Omega.
        assert_eq!(next(Selection::Feed { id: 1 }).as_deref(), Some("Gamma"));
        assert_eq!(next(Selection::Feed { id: 3 }).as_deref(), Some("Delta"));
        assert_eq!(next(Selection::Feed { id: 4 }).as_deref(), Some("Gamma"));
        assert_eq!(next(Selection::Category { id: 2 }).as_deref(), Some("Dev"));
        assert_eq!(next(Selection::Category { id: 1 }).as_deref(), Some("Blogs"));
        assert_eq!(next(Selection::All), None);
        assert_eq!(next(Selection::Feed { id: 99 }), None);
        let history = next_after(&counts, Mode::History, Selection::Feed { id: 3 }).map(|i| i.label);
        assert_eq!(history.as_deref(), Some("Alpha"));
        assert_eq!(next_after(&counts, Mode::History, Selection::Feed { id: 1 }), None);
    }

    #[test]
    fn history_mode_uses_read_counts_and_hides_feeds_with_nothing_read() {
        let items = build(
            &[fc(1, "Zed", (1, "Dev"), 2, 0), fc(2, "Alpha", (1, "Dev"), 0, 7), fc(3, "Loud", (2, "Blogs"), 1, 3)],
            Mode::History,
        );
        assert_eq!(
            rows(&items),
            [("All read", 10, 0), ("Blogs", 3, 0), ("Loud", 3, 1), ("Dev", 7, 0), ("Alpha", 7, 1)]
        );
    }
}
