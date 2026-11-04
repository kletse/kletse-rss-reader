//! The app driven by key presses against a fake Miniflux, as the UI tests did in the webview days.
//! Effects run right away unless a test holds one back to answer it later.

#![allow(clippy::unwrap_used)]

use super::*;
use crate::dates;
use crate::feeds::{Category, Feed, FeedCount};
use crate::paging::tests::{entry, entry_at};
use crate::paging::{Order, PAGE};
use std::collections::VecDeque;

/// What the fake Miniflux holds (every selection gets all of it). Tests change statuses here as "elsewhere".
#[derive(Default)]
struct Fake {
    entries: Vec<Entry>,
    requests: i64,
}

fn seconds(e: &Entry) -> i64 {
    dates::unix_millis(&e.published_at).unwrap() / 1000
}

impl Fake {
    /// Pages like Miniflux: `past` is a strict bound in whole seconds, and items published in the same second come
    /// in a different order on every request, as Postgres may return them. The starred list has no status.
    fn page(&mut self, selection: Selection, status: EntryStatus, request: PageRequest) -> EntryPage {
        let PageRequest { past, limit, order } = request;
        self.requests += 1;
        let salt = self.requests;
        let dir = if order == Order::OldestFirst { 1 } else { -1 };
        let mut list: Vec<Entry> = self
            .entries
            .iter()
            .filter(|e| if selection == Selection::Starred { e.starred } else { e.status == status })
            .filter(|e| past == 0 || dir * (seconds(e) - past) > 0)
            .cloned()
            .collect();
        list.sort_by_key(|e| (dir * seconds(e), (e.id * salt) % 17, e.id));
        EntryPage { total: i64::try_from(list.len()).unwrap(), entries: list.into_iter().take(limit).collect() }
    }

    /// Every entry whose title has `query`, read or unread, highest id first, from `offset`.
    fn search(&mut self, query: &str, offset: usize, limit: usize) -> EntryPage {
        self.requests += 1;
        let mut found: Vec<Entry> = self.entries.iter().filter(|e| e.title.contains(query)).cloned().collect();
        found.sort_by_key(|e| -e.id);
        let total = i64::try_from(found.len()).unwrap();
        EntryPage { total, entries: found.into_iter().skip(offset).take(limit).collect() }
    }

    fn mark(&mut self, ids: &[i64], status: EntryStatus) {
        for e in self.entries.iter_mut().filter(|e| ids.contains(&e.id)) {
            e.status = status;
        }
    }

    fn unread_up_to(&self, up_to: i64) -> Vec<i64> {
        self.entries.iter().filter(|e| e.status == EntryStatus::Unread && e.id <= up_to).map(|e| e.id).collect()
    }

    fn star(&mut self, id: i64, starred: bool) {
        for e in self.entries.iter_mut().filter(|e| e.id == id) {
            e.starred = starred;
        }
    }

    fn unread_ids(&self) -> Vec<i64> {
        self.entries.iter().filter(|e| e.status == EntryStatus::Unread).map(|e| e.id).collect()
    }
}

struct Harness {
    app: App,
    fake: Fake,
    status: Status,
    counts: Vec<FeedCount>,
    /// The picker's count of starred items.
    starred: Option<i64>,
    /// Every server effect, in the order they ran.
    calls: Vec<Effect>,
    hold_next_page: bool,
    hold_fetch: bool,
    hold_save: bool,
    held: Option<Effect>,
    fail_next_page: Option<String>,
    /// What the server says to the next search's first page, instead of answering.
    fail_next_search: Option<String>,
    /// Keeps the next search's first page back, in `held`.
    hold_next_search: bool,
    fail_next_status: Option<String>,
    fail_next_star: Option<String>,
    /// What the server says to `f` and `r`, instead of doing it.
    fail_fetch: Option<String>,
    full_content: String,
    problem: Option<String>,
    save_result: Result<Saved, String>,
}

fn counts(unread: i64, read: i64) -> Vec<FeedCount> {
    vec![FeedCount {
        feed: Feed { id: 1, title: "Feed".into(), category: Category { id: 1, title: "Dev".into() } },
        unread: Some(unread),
        read: Some(read),
    }]
}

fn saved(status: Status) -> Saved {
    Saved { status, username: None, readeck_problem: None }
}

impl Harness {
    fn new(entries: Vec<Entry>) -> Self {
        Self {
            app: App::new(Prefs::default(), false),
            fake: Fake { entries, requests: 0 },
            status: Status { configured: true, readeck: false, fetches: true },
            counts: counts(2, 3),
            starred: Some(0),
            calls: Vec::new(),
            hold_next_page: false,
            hold_fetch: false,
            hold_save: false,
            held: None,
            fail_next_page: None,
            fail_next_search: None,
            hold_next_search: false,
            fail_next_status: None,
            fail_next_star: None,
            fail_fetch: None,
            full_content: "<p>Full text</p>".into(),
            problem: None,
            save_result: Ok(saved(Status { configured: true, readeck: false, fetches: true })),
        }
    }

    fn start(&mut self) {
        let fx = self.app.start(self.status);
        self.run(fx);
    }

    /// Starts the app and opens "All unread" (or "All read", in history) from the picker.
    fn open(&mut self, history: bool) {
        self.start();
        assert!(self.picker_loaded());
        if history {
            self.key("h");
            assert_eq!(self.app.picker_items()[0].label, "All read");
        }
        self.key("Enter");
        assert!(self.app.current().is_some(), "nothing to read after opening");
    }

    fn run(&mut self, fx: Vec<Effect>) {
        let mut queue: VecDeque<Effect> = fx.into();
        while let Some(effect) = queue.pop_front() {
            if let Some(outcome) = self.exec(effect) {
                queue.extend(self.app.apply(outcome));
            }
        }
    }

    fn exec(&mut self, effect: Effect) -> Option<Outcome> {
        let server =
            !matches!(effect, Effect::SavePrefs(_) | Effect::ScrollPage { .. } | Effect::Quit | Effect::ShowSettings);
        if server {
            self.calls.push(effect.clone());
        }
        match effect {
            Effect::OpenSelection { load, selection, mode, request } => {
                let page = self.fake.page(selection, mode.status(), request);
                let unread = mode == Mode::Unread && selection != Selection::Starred;
                let newest = if unread { self.fake.unread_ids().into_iter().max().unwrap_or(0) } else { 0 };
                Some(Outcome::Opened { load, result: Ok((page, newest)) })
            }
            Effect::LoadPage { load, selection, mode, request, arrivals } => {
                if std::mem::take(&mut self.hold_next_page) {
                    self.held = Some(effect);
                    return None;
                }
                let result = match self.fail_next_page.take() {
                    Some(message) => Err(message),
                    None => Ok(self.fake.page(selection, mode.status(), request)),
                };
                Some(Outcome::Page { load, arrivals, result })
            }
            Effect::Search { load, ref query, offset, limit, first } => {
                let hold = if first { &mut self.hold_next_search } else { &mut self.hold_next_page };
                if std::mem::take(hold) {
                    self.held = Some(effect);
                    return None;
                }
                let failure = if first { self.fail_next_search.take() } else { self.fail_next_page.take() };
                let result = match failure {
                    Some(message) => Err(message),
                    None => Ok(self.fake.search(query, offset, limit)),
                };
                Some(if first {
                    Outcome::Opened { load, result: result.map(|page| (page, 0)) }
                } else {
                    Outcome::Page { load, arrivals: false, result }
                })
            }
            Effect::CountRow { selection, label } => {
                let newest = self.fake.unread_ids().into_iter().max().unwrap_or(0);
                let count = i64::try_from(self.fake.unread_up_to(newest).len()).unwrap();
                Some(Outcome::RowCounted { selection, label, result: Ok((newest, count)) })
            }
            Effect::MarkRowRead { selection, label, up_to } => {
                let ids = self.fake.unread_up_to(up_to);
                self.fake.mark(&ids, EntryStatus::Read);
                Some(Outcome::RowMarkedRead { selection, label, up_to, result: Ok(()) })
            }
            Effect::SetStatus { ids, status, revert, announce } => {
                let result = match self.fail_next_status.take() {
                    Some(message) => Err(message),
                    None => {
                        self.fake.mark(&ids, status);
                        Ok(())
                    }
                };
                Some(Outcome::StatusSet { status, revert, announce, result })
            }
            Effect::SetStarred { index, id, starred } => {
                let result = match self.fail_next_star.take() {
                    Some(message) => Err(message),
                    None => {
                        self.fake.star(id, starred);
                        Ok(())
                    }
                };
                Some(Outcome::StarredSet { index, id, starred, result })
            }
            Effect::FetchContent { index, id } => {
                if self.hold_fetch {
                    self.held = Some(effect);
                    return None;
                }
                let result = self.fail_fetch.clone().map_or_else(|| Ok(self.full_content.clone()), Err);
                Some(Outcome::ContentFetched { index, id, result })
            }
            Effect::FeedCounts => {
                Some(Outcome::FeedCounts(Ok(Counts { feeds: self.counts.clone(), starred: self.starred })))
            }
            Effect::RefreshFeeds => Some(Outcome::Refreshed(self.fail_fetch.clone().map_or(Ok(()), Err))),
            Effect::ReloadPickerAfter { .. }
            | Effect::SavePrefs(_)
            | Effect::ScrollPage { .. }
            | Effect::Quit
            | Effect::ShowSettings => None,
            Effect::SaveToReadeck { index, id, .. } => {
                if self.hold_save {
                    self.held = Some(effect);
                    return None;
                }
                Some(Outcome::SavedToReadeck { index, id, result: Ok(()) })
            }
            Effect::OpenInBrowser { then, .. } => Some(Outcome::OpenedInBrowser { then, result: Ok(()) }),
            Effect::CountUnread { load, selection, up_to, later } => {
                let count = i64::try_from(self.fake.unread_up_to(up_to).len()).unwrap();
                Some(Outcome::UnreadCounted { load, selection, up_to, later, result: Ok(count) })
            }
            Effect::MarkAllRead { load, up_to, later, .. } => {
                let ids = self.fake.unread_up_to(up_to);
                self.fake.mark(&ids, EntryStatus::Read);
                self.fake.mark(&later, EntryStatus::Read);
                Some(Outcome::MarkedAllRead { load, result: Ok(()) })
            }
            Effect::LoadSettings => {
                Some(Outcome::SettingsLoaded { values: SettingsValues::default(), problem: self.problem.clone() })
            }
            Effect::SaveSettings(_) => Some(Outcome::SettingsSaved(self.save_result.clone())),
        }
    }

    /// Lets the held-back request answer now, with what the fake Miniflux holds by then.
    fn answer(&mut self) {
        self.hold_fetch = false;
        self.hold_save = false;
        let effect = self.held.take().expect("a request was held");
        if let Some(pos) = self.calls.iter().rposition(|c| *c == effect) {
            self.calls.remove(pos); // `exec` records it again
        }
        if let Some(outcome) = self.exec(effect) {
            let fx = self.app.apply(outcome);
            self.run(fx);
        }
    }

    fn press(&mut self, press: KeyPress) {
        let (_, fx) = self.app.key(&press);
        self.run(fx);
    }

    fn key(&mut self, key: &str) {
        self.press(KeyPress { key: key.into(), ..KeyPress::default() });
    }

    fn shift(&mut self, key: &str) {
        self.press(KeyPress { key: key.into(), shift: true, ..KeyPress::default() });
    }

    fn picker_loaded(&self) -> bool {
        self.app.overlay() == Overlay::Picker && matches!(self.app.picker_list(), PickerList::Ready(_))
    }

    fn title(&self) -> Option<&str> {
        self.app.current().map(|e| e.title.as_str())
    }

    /// Presses `keys` on every article until the end; returns the ids it showed, in order.
    fn press_through(&mut self, keys: &[&str]) -> Vec<i64> {
        let mut ids = Vec::new();
        while let Some(entry) = self.app.current() {
            ids.push(entry.id);
            for key in keys {
                self.key(key);
            }
        }
        ids
    }

    fn entries_calls(&self) -> Vec<&Effect> {
        self.calls
            .iter()
            .filter(|c| matches!(c, Effect::OpenSelection { .. } | Effect::LoadPage { .. } | Effect::Search { .. }))
            .collect()
    }

    fn status_calls(&self) -> Vec<(Vec<i64>, EntryStatus)> {
        self.calls
            .iter()
            .filter_map(|c| match c {
                Effect::SetStatus { ids, status, .. } => Some((ids.clone(), *status)),
                Effect::MarkAllRead { later, .. } if !later.is_empty() => Some((later.clone(), EntryStatus::Read)),
                _ => None,
            })
            .collect()
    }

    fn toast(&self) -> Option<&str> {
        self.app.toast().map(|t| t.message.as_str())
    }

    fn caught_up(&self) -> bool {
        self.app.screen() == Screen::End
    }

    fn mark_elsewhere(&mut self, ids: &[i64]) {
        self.fake.mark(ids, EntryStatus::Read);
    }
}

fn range(from: i64, to: i64) -> Vec<i64> {
    (from..=to).collect()
}

/// `n` unread entries titled "Item 1" … "Item n", oldest first.
fn items(n: i64) -> Vec<Entry> {
    range(1, n).into_iter().map(|id| entry(id, &format!("Item {id}"))).collect()
}

/// Read entries titled "Read 1" … "Read n", oldest first.
/// One unread item per title, ids from 1.
fn titled(titles: &[&str]) -> Vec<Entry> {
    titles.iter().zip(1..).map(|(title, id)| entry(id, title)).collect()
}

fn read_items(n: i64) -> Vec<Entry> {
    range(1, n).into_iter().map(|id| entry_at(id, &format!("Read {id}"), EntryStatus::Read, id)).collect()
}

fn page_request(e: &Effect) -> Option<PageRequest> {
    match e {
        Effect::OpenSelection { request, .. } | Effect::LoadPage { request, .. } => Some(*request),
        _ => None,
    }
}

const PAGE_I64: i64 = PAGE as i64;

// ---- Unread paging ------------------------------------------------------------

#[test]
fn loads_a_page_at_a_time_and_j_marking_items_read_doesnt_make_it_skip_any() {
    let mut h = Harness::new(items(120));
    h.open(false);
    assert_eq!(
        page_request(h.entries_calls()[0]),
        Some(PageRequest { past: 0, limit: PAGE, order: Order::OldestFirst })
    );
    assert_eq!(h.app.queue().len(), PAGE);
    assert_eq!(h.app.position(), "1 / 120"); // the whole selection
    for _ in 0..PAGE - 10 {
        h.key("j");
    }
    assert_eq!(h.app.queue().len(), 2 * PAGE);
    assert_eq!(h.app.position(), format!("{} / 120", PAGE - 9));
    assert_eq!(h.press_through(&["j"]), range(PAGE_I64 - 9, 120));
    assert_eq!(h.app.queue().iter().map(|e| e.id).collect::<Vec<_>>(), range(1, 120));
    assert!(h.caught_up());
    assert!(h.fake.unread_ids().is_empty());
    h.key("j");
    assert!(h.picker_loaded()); // the true end goes back to the feeds
}

#[test]
fn a_page_still_loading_for_the_last_selection_doesnt_hold_up_the_new_one() {
    let mut h = Harness::new(items(120));
    h.open(false);
    h.hold_next_page = true;
    for _ in 0..PAGE - 10 {
        h.key("j");
    }
    assert!(h.held.is_some(), "the second page is on its way");
    h.key("p");
    h.key("j"); // the category: the 80 items still unread
    h.key("Enter");
    assert_eq!(h.app.queue().len(), PAGE);
    for _ in 0..PAGE {
        h.key("j");
    }
    // Its second page was asked for and came, without waiting for the old one.
    assert_eq!(h.app.screen(), Screen::Article);
    assert_eq!(h.title(), Some("Item 91"));
    assert_eq!(h.app.queue().len(), 80);
    // The old one, when it does come, changes nothing.
    h.answer();
    assert_eq!(h.app.queue().iter().map(|e| e.id).collect::<Vec<_>>(), range(41, 120));
}

#[test]
fn shows_everything_once_with_ties_in_publication_time_and_items_read_elsewhere_meanwhile() {
    // Items 30–110 were all published in the same second, which Miniflux returns in any order.
    let entries = items(130)
        .into_iter()
        .map(|e| if (30..=110).contains(&e.id) { entry_at(e.id, &e.title, EntryStatus::Unread, 30) } else { e })
        .collect();
    let mut h = Harness::new(entries);
    h.open(false);
    let loaded: Vec<i64> = h.app.queue().iter().map(|e| e.id).collect();
    let mut not_loaded: Vec<i64> = range(30, 110).into_iter().filter(|id| !loaded.contains(id)).take(6).collect();
    not_loaded.extend([125, 130]);
    let mut elsewhere = range(5, 20);
    elsewhere.extend(&not_loaded);
    h.mark_elsewhere(&elsewhere);
    let ids = h.press_through(&["n"]); // skipping keeps them unread, so they stay in the server's list
    let mut sorted = ids.clone();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(sorted.len(), ids.len(), "something was shown twice");
    assert_eq!(sorted, range(1, 130).into_iter().filter(|id| !not_loaded.contains(id)).collect::<Vec<_>>());
    assert!(h.caught_up());
}

#[test]
fn marking_all_read_marks_what_was_unread_when_the_selection_was_opened_not_what_arrived_since() {
    let mut h = Harness::new(items(120));
    h.open(false);
    h.key("j");
    h.key("j");
    assert_eq!(h.title(), Some("Item 3"));
    h.fake.entries.push(entry(121, "Arrived later"));
    h.shift("A");
    assert_eq!(h.app.overlay(), Overlay::Confirm);
    assert_eq!(h.app.confirm_text(), "Mark all 118 unread items in “All unread” as read?");
    h.key("y");
    assert!(h.caught_up());
    assert!(h.calls.iter().any(|c| matches!(c, Effect::MarkAllRead { selection: Selection::All, up_to: 120, .. })));
    assert_eq!(h.fake.unread_ids(), [121]);
    assert!(h.app.queue().iter().all(|e| e.status == EntryStatus::Read));
    assert!(!h.app.has_more());
}

#[test]
fn marking_all_read_also_marks_a_later_item_that_a_page_did_load() {
    let mut h = Harness::new(items(PAGE_I64 + 5));
    h.open(false);
    h.fake.entries.push(entry(PAGE_I64 + 6, "Arrived later"));
    for _ in 0..PAGE - 10 {
        h.key("n");
    }
    assert_eq!(h.app.queue().len(), PAGE + 6);
    h.shift("A");
    assert_eq!(h.app.confirm_text(), format!("Mark all {} unread items in “All unread” as read?", PAGE + 6));
    h.key("y");
    assert!(h.caught_up());
    assert!(h.status_calls().contains(&(vec![PAGE_I64 + 6], EntryStatus::Read)));
    assert!(h.fake.unread_ids().is_empty());
}

// ---- New arrivals at the end of the unread list -----------------------------

#[test]
fn shows_items_that_arrived_while_you_were_reading_instead_of_all_caught_up() {
    let mut h = Harness::new(items(3));
    h.open(false);
    h.key("j");
    h.key("j");
    assert_eq!(h.title(), Some("Item 3"));
    h.fake.entries.push(entry(4, "Arrived later"));
    h.fake.entries.push(entry(5, "Also later"));
    // No flash of "All caught up", nor of "Loading…", while it looks.
    h.hold_next_page = true;
    h.key("j");
    assert_eq!(h.app.screen(), Screen::Empty);
    h.answer();
    assert_eq!(h.title(), Some("Arrived later"));
    assert_eq!(h.app.position(), "4 / 5");
    // It asked for what came after the newest item loaded.
    let past = seconds(&entry(3, "")) - 1;
    assert_eq!(
        page_request(h.entries_calls().last().unwrap()),
        Some(PageRequest { past, limit: PAGE + 1, order: Order::OldestFirst })
    );
    assert_eq!(h.press_through(&["j"]), [4, 5]);
    assert!(h.caught_up());
    assert!(h.fake.unread_ids().is_empty());
}

#[test]
fn with_nothing_new_says_all_caught_up_and_then_goes_to_the_feeds_as_before() {
    let mut h = Harness::new(items(2));
    h.open(false);
    h.key("j");
    assert_eq!(h.title(), Some("Item 2"));
    h.key("j");
    assert!(h.caught_up());
    assert_eq!(h.entries_calls().len(), 2); // the first page, and the one look for new arrivals
    h.key("j");
    assert!(h.picker_loaded());
    assert_eq!(h.entries_calls().len(), 2);
}

#[test]
fn a_quick_second_j_while_it_looks_neither_loads_twice_nor_skips_the_new_item() {
    let mut h = Harness::new(items(2));
    h.open(false);
    h.key("j");
    assert_eq!(h.title(), Some("Item 2"));
    h.fake.entries.push(entry(3, "Arrived later"));
    h.hold_next_page = true;
    h.key("j");
    h.key("j");
    h.key("Enter");
    assert_eq!(h.app.overlay(), Overlay::None);
    h.answer();
    assert_eq!(h.title(), Some("Arrived later"));
    assert_eq!(h.entries_calls().len(), 2);
    assert_eq!(h.fake.unread_ids(), [3]);
}

#[test]
fn if_looking_fails_it_says_so_and_is_all_caught_up() {
    let mut h = Harness::new(items(1));
    h.open(false);
    h.fail_next_page = Some("Miniflux is down".into());
    h.key("j");
    assert!(h.caught_up());
    assert_eq!(h.toast(), Some("Miniflux is down"));
    h.key("j");
    assert!(h.picker_loaded());
}

#[test]
fn ignores_what_it_finds_once_another_selection_is_opened() {
    let mut h = Harness::new(items(2));
    h.open(false);
    h.key("n");
    assert_eq!(h.title(), Some("Item 2"));
    h.hold_next_page = true;
    h.key("n");
    h.key("g");
    assert!(h.picker_loaded());
    h.key("Enter");
    assert_eq!(h.app.current().map(|e| e.id), Some(1));
    h.fake.entries.push(entry(3, "Arrived later"));
    h.answer();
    assert_eq!(h.entries_calls().len(), 3);
    assert_eq!(h.app.queue().iter().map(|e| e.id).collect::<Vec<_>>(), [1, 2]);
}

#[test]
fn marking_all_read_also_marks_a_new_arrival_it_loaded() {
    let mut h = Harness::new(items(2));
    h.open(false);
    h.fake.entries.push(entry(3, "Arrived later"));
    h.key("n");
    h.key("n");
    assert_eq!(h.title(), Some("Arrived later"));
    h.shift("A");
    assert_eq!(h.app.confirm_text(), "Mark all 3 unread items in “All unread” as read?");
    h.key("y");
    assert!(h.caught_up());
    assert!(h.status_calls().contains(&(vec![3], EntryStatus::Read)));
    assert!(h.fake.unread_ids().is_empty());
}

#[test]
fn history_doesnt_look_the_end_is_the_end() {
    let mut h = Harness::new(read_items(1));
    h.open(true);
    h.fake.entries.push(entry_at(2, "Read 2", EntryStatus::Read, 2));
    h.key("j");
    assert!(h.caught_up());
    assert_eq!(h.app.mode(), Mode::History);
    assert_eq!(h.entries_calls().len(), 1);
}

// ---- History paging --------------------------------------------------------------

#[test]
fn history_loads_the_next_page_as_you_get_near_the_end() {
    let mut h = Harness::new(read_items(PAGE_I64 + 5));
    h.open(true);
    assert_eq!(h.title(), Some(format!("Read {}", PAGE + 5).as_str()));
    assert_eq!(h.app.position(), format!("1 / {}", PAGE + 5));
    for _ in 0..PAGE - 10 {
        h.key("n");
    }
    // Page two starts before the oldest item loaded: Read 6.
    let past = seconds(&entry(6, "")) + 1;
    assert!(
        h.entries_calls()
            .iter()
            .any(|c| page_request(c) == Some(PageRequest { past, limit: PAGE + 1, order: Order::NewestFirst }))
    );
    assert_eq!(h.app.queue().len(), PAGE + 5);
    assert!(!h.app.has_more());
}

#[test]
fn m_marking_items_unread_doesnt_make_history_skip_any() {
    let mut h = Harness::new(read_items(2 * PAGE_I64));
    h.open(true);
    let mut expected = range(1, 2 * PAGE_I64);
    expected.reverse();
    assert_eq!(h.press_through(&["m", "n"]), expected);
    assert_eq!(h.fake.unread_ids().len(), 2 * PAGE);
    assert!(h.caught_up());
}

#[test]
fn marking_all_read_in_history_marks_what_was_marked_unread_here() {
    let mut h = Harness::new(read_items(2 * PAGE_I64));
    h.open(true);
    h.key("m");
    h.key("n");
    h.key("m");
    h.shift("A");
    assert_eq!(h.app.confirm_text(), "Mark all 2 unread items in “All read” as read?");
    h.key("y");
    assert_eq!(h.status_calls().last(), Some(&(vec![2 * PAGE_I64, 2 * PAGE_I64 - 1], EntryStatus::Read)));
    assert!(h.fake.unread_ids().is_empty());
}

// ---- The reader --------------------------------------------------------------------

fn first_second() -> Harness {
    let mut h = Harness::new(vec![entry(1, "First"), entry(2, "Second")]);
    h.open(false);
    assert_eq!(h.title(), Some("First"));
    h
}

#[test]
fn j_marks_the_article_read_and_shows_the_next_k_goes_back() {
    let mut h = first_second();
    assert_eq!(h.app.position(), "1 / 2");
    h.key("j");
    assert_eq!(h.title(), Some("Second"));
    assert_eq!(h.status_calls(), [(vec![1], EntryStatus::Read)]);
    h.key("k");
    assert_eq!(h.title(), Some("First"));
    assert_eq!(h.app.current().unwrap().status, EntryStatus::Read);
}

#[test]
fn n_skips_without_marking_read_and_the_end_says_all_caught_up() {
    let mut h = first_second();
    h.key("n");
    h.key("n");
    assert!(h.caught_up());
    assert!(h.status_calls().is_empty());
}

#[test]
fn m_says_what_it_did_and_rolls_the_status_back_when_the_server_fails() {
    let mut h = first_second();
    h.key("m");
    assert_eq!(h.toast(), Some("Marked read"));
    h.key("m");
    assert_eq!(h.toast(), Some("Marked unread"));
    h.fail_next_status = Some("offline".into());
    h.key("m");
    assert_eq!(h.toast(), Some("offline"));
    assert_eq!(h.app.toast().unwrap().kind, ToastKind::Error);
    assert_eq!(h.app.current().unwrap().status, EntryStatus::Unread);
}

#[test]
fn f_fetches_the_full_article_once_then_switches_between_it_and_the_feeds_version() {
    let mut h = first_second();
    let content = |h: &Harness| h.app.current().unwrap().content.clone();
    h.key("f");
    assert_eq!(content(&h), "<p>Full text</p>");
    assert!(h.calls.contains(&Effect::FetchContent { index: 0, id: 1 }));
    assert_eq!(h.toast(), Some("Full article"));
    h.key("f");
    assert_eq!(content(&h), "<p>First body</p>");
    assert_eq!(h.toast(), Some("Feed version"));
    h.key("f");
    assert_eq!(content(&h), "<p>Full text</p>");
    assert_eq!(h.toast(), Some("Full article"));
    assert_eq!(h.calls.iter().filter(|c| matches!(c, Effect::FetchContent { .. })).count(), 1);
}

#[test]
fn f_finishing_after_moving_on_leaves_the_article_on_screen_alone() {
    let mut h = first_second();
    h.hold_fetch = true;
    h.key("f");
    h.key("n");
    assert_eq!(h.title(), Some("Second"));
    h.answer();
    assert_eq!(h.app.queue()[0].content, "<p>Full text</p>");
    assert_ne!(h.toast(), Some("Full article"));
    h.key("k");
    assert_eq!(h.app.current().unwrap().content, "<p>Full text</p>");
    h.key("f");
    assert_eq!(h.app.current().unwrap().content, "<p>First body</p>");
    assert_eq!(h.calls.iter().filter(|c| matches!(c, Effect::FetchContent { .. })).count(), 1);
}

#[test]
fn left_opens_the_picker_and_right_opens_the_chosen_row() {
    let mut h = first_second();
    h.key("ArrowLeft");
    assert!(h.picker_loaded());
    h.key("ArrowRight");
    assert_eq!(h.app.overlay(), Overlay::None);
    assert_eq!(h.title(), Some("First"));
}

#[test]
fn esc_opens_the_picker_and_closes_it_again_once_there_is_a_queue() {
    let mut h = first_second();
    h.key("Escape");
    assert!(h.picker_loaded());
    h.key("Escape");
    assert_eq!(h.app.overlay(), Overlay::None);
}

#[test]
fn the_window_title_shows_the_article_and_just_the_app_name_elsewhere() {
    let mut h = first_second();
    assert_eq!(h.app.window_title(), "First — Onward");
    h.key("Escape");
    assert_eq!(h.app.window_title(), "Onward");
    h.key("Escape");
    assert_eq!(h.app.window_title(), "First — Onward");
    h.key("n");
    assert_eq!(h.app.window_title(), "Second — Onward");
    h.key("n");
    assert!(h.caught_up());
    assert_eq!(h.app.window_title(), "Onward");
}

#[test]
fn an_untitled_article_is_untitled_in_the_window_title_too() {
    let mut h = Harness::new(vec![entry(1, "")]);
    h.open(false);
    assert_eq!(h.app.window_title(), "(untitled) — Onward");
}

#[test]
fn space_scrolls_and_at_the_end_moves_on() {
    let mut h = first_second();
    let (used, fx) = h.app.key(&KeyPress { key: " ".into(), ..KeyPress::default() });
    assert!(used);
    assert_eq!(fx, [Effect::ScrollPage { up: false }]);
    let (_, fx) = h.app.key(&KeyPress { key: " ".into(), shift: true, at_bottom: true, ..KeyPress::default() });
    assert_eq!(fx, [Effect::ScrollPage { up: true }]);
    h.press(KeyPress { key: " ".into(), at_bottom: true, ..KeyPress::default() });
    assert_eq!(h.title(), Some("Second"));
    assert_eq!(h.status_calls(), [(vec![1], EntryStatus::Read)]);
}

#[test]
fn o_opens_the_original_and_marks_it_read() {
    let mut h = first_second();
    h.key("o");
    assert!(h.calls.contains(&Effect::OpenInBrowser {
        url: "https://example.com/1".into(),
        then: AfterOpen::MarkRead { index: 0 }
    }));
    assert_eq!(h.toast(), Some("Opened in browser"));
    assert_eq!(h.app.current().unwrap().status, EntryStatus::Read);
}

#[test]
fn b_needs_readeck_and_then_saves_and_moves_on() {
    let mut h = first_second();
    h.key("b");
    assert_eq!(h.toast(), Some("Readeck isn't set up. Add it in Settings (press ,)."));
    assert_eq!(h.title(), Some("First"));

    let mut h = Harness::new(vec![entry(1, "First"), entry(2, "Second")]);
    h.status.readeck = true;
    h.open(false);
    h.key("b");
    assert!(h.calls.contains(&Effect::SaveToReadeck { index: 0, id: 1, url: "https://example.com/1".into() }));
    assert_eq!(h.title(), Some("Second"));
    assert_eq!(h.status_calls(), [(vec![1], EntryStatus::Read)]);
}

#[test]
fn a_save_that_lands_after_moving_on_marks_the_saved_article_read_and_nothing_else() {
    let mut h = Harness::new(items(3));
    h.status.readeck = true;
    h.open(false);
    h.hold_save = true;
    h.key("b");
    assert_eq!(h.title(), Some("Item 1"), "waiting for Readeck");
    h.key("n"); // moved on meanwhile, keeping Item 1 unread
    assert_eq!(h.title(), Some("Item 2"));
    h.answer();
    assert_eq!(h.toast(), Some("Saved to Readeck"));
    assert_eq!(h.title(), Some("Item 2"), "stays where you are");
    assert_eq!(h.status_calls(), [(vec![1], EntryStatus::Read)]);
    assert_eq!(h.app.queue()[1].status, EntryStatus::Unread);

    // Another list opened meanwhile: nothing here to mark.
    let mut entries = items(3);
    entries.push(entry_at(10, "Old", EntryStatus::Read, 10));
    let mut h = Harness::new(entries);
    h.status.readeck = true;
    h.open(false);
    h.hold_save = true;
    h.key("b");
    h.key("h");
    h.key("Enter");
    assert_eq!(h.title(), Some("Old"));
    h.answer();
    assert_eq!(h.toast(), Some("Saved to Readeck"));
    assert_eq!(h.title(), Some("Old"));
    assert_eq!(h.status_calls(), []);
}

#[test]
fn font_size_and_width_keys_change_and_save_the_prefs() {
    let mut h = first_second();
    let ctrl = |key: &str| KeyPress { key: key.into(), ctrl: true, ..KeyPress::default() };
    let (_, fx) = h.app.key(&ctrl("="));
    assert_eq!(fx, [Effect::SavePrefs(Prefs { font_size: 19, ..Prefs::default() })]);
    let (_, fx) = h.app.key(&KeyPress { key: "[".into(), ..KeyPress::default() });
    assert_eq!(fx, [Effect::SavePrefs(Prefs { font_size: 19, width_factor: 36, ..Prefs::default() })]);
    let (_, fx) = h.app.key(&ctrl("0"));
    assert_eq!(fx, [Effect::SavePrefs(Prefs::default())]);
    let (used, fx) = h.app.key(&ctrl("0"));
    assert!(used && fx.is_empty(), "nothing changed, nothing to save");
}

#[test]
fn keys_with_alt_or_super_are_left_alone() {
    let mut h = first_second();
    let (used, _) = h.app.key(&KeyPress { key: "j".into(), alt: true, ..KeyPress::default() });
    assert!(!used);
    assert_eq!(h.title(), Some("First"));
}

// ---- History ------------------------------------------------------------------------

#[test]
fn h_in_the_picker_switches_to_read_items_newest_first_with_the_same_keys() {
    let mut h = Harness::new(read_items(3));
    h.open(true);
    assert_eq!(h.title(), Some("Read 3"));
    assert!(matches!(
        h.entries_calls()[0],
        Effect::OpenSelection {
            mode: Mode::History,
            request: PageRequest { past: 0, limit: PAGE, order: Order::NewestFirst },
            ..
        }
    ));
    assert_eq!(h.app.mode(), Mode::History);
    h.key("j");
    assert_eq!(h.title(), Some("Read 2"));
    assert!(h.status_calls().is_empty()); // already read
    h.key("m");
    assert_eq!(h.status_calls(), [(vec![2], EntryStatus::Unread)]);
    assert_eq!(h.app.current().unwrap().status, EntryStatus::Unread);
    h.key("k");
    assert_eq!(h.title(), Some("Read 3"));
    h.key("n");
    h.key("n");
    h.key("n");
    assert!(h.caught_up());
}

#[test]
fn h_in_the_reader_opens_the_picker_in_the_other_mode_and_esc_goes_back_to_the_open_queue() {
    let mut h = first_second();
    h.key("h");
    assert_eq!(h.app.picker_mode(), Mode::History);
    h.key("Escape");
    assert_eq!(h.app.overlay(), Overlay::None);
    assert_eq!(h.title(), Some("First"));
    assert_eq!(h.app.mode(), Mode::Unread);
}

#[test]
fn the_pickers_switch_goes_to_the_mode_it_names_and_stays_there_when_pressed_again() {
    let mut h = Harness::new(items(2));
    h.start();
    h.key("j");
    let cursor = h.app.picker_cursor();

    let fx = h.app.action(keys::button_action("unread").unwrap());
    assert!(fx.is_empty());
    assert_eq!(h.app.picker_mode(), Mode::Unread);
    assert_eq!(h.app.picker_cursor(), cursor);

    let fx = h.app.action(keys::button_action("history").unwrap());
    h.run(fx);
    assert_eq!(h.app.picker_mode(), Mode::History);
    assert_eq!(h.app.picker_cursor(), 0);
    let fx = h.app.action(keys::button_action("history").unwrap());
    assert!(fx.is_empty());
    assert_eq!(h.app.picker_mode(), Mode::History);
}

#[test]
fn the_pickers_refresh_button_refreshes_as_r_does() {
    let mut h = Harness::new(items(2));
    h.start();
    let fx = h.app.action(keys::button_action("refresh").unwrap());
    assert_eq!(fx, [Effect::RefreshFeeds]);
    assert_eq!(h.toast(), Some("Refreshing feeds…"));
}

// ---- Overlays -----------------------------------------------------------------------

#[test]
fn the_picker_cant_be_dismissed_before_anything_was_opened() {
    let mut h = Harness::new(items(2));
    h.start();
    assert!(h.picker_loaded());
    h.key("Escape");
    assert!(h.picker_loaded());
}

#[test]
fn the_picker_moves_round_and_shows_errors() {
    let mut h = Harness::new(items(2));
    h.start();
    assert_eq!(h.app.picker_items().len(), 3); // All unread, Dev, Feed
    h.key("k");
    assert_eq!(h.app.picker_cursor(), 2);
    h.key("j");
    assert_eq!(h.app.picker_cursor(), 0);
    let fx = h.app.apply(Outcome::FeedCounts(Err("Miniflux is down".into())));
    assert!(fx.is_empty());
    assert_eq!(h.app.picker_list(), &PickerList::Error("Miniflux is down".into()));
}

#[test]
fn r_refreshes_and_reloads_the_counts_a_few_times() {
    let mut h = Harness::new(items(2));
    h.start();
    h.key("r");
    assert_eq!(h.toast(), Some("Refreshing feeds…"));
    let reloads: Vec<_> = h.calls.iter().filter(|c| matches!(c, Effect::ReloadPickerAfter { .. })).collect();
    assert_eq!(reloads.len(), 3);
    let fx = h.app.apply(Outcome::PickerReloadDue);
    assert_eq!(fx, [Effect::FeedCounts]);
}

#[test]
fn the_confirm_dialog_swallows_reader_keys() {
    let mut h = first_second();
    h.shift("A");
    assert!(h.app.confirm_text().starts_with("Mark all 2 unread items"));
    h.key("j");
    assert_eq!(h.title(), Some("First"));
    h.key("n");
    assert_eq!(h.app.overlay(), Overlay::None);
    assert!(h.status_calls().is_empty());
}

#[test]
fn question_mark_in_the_picker_shows_the_keys_and_closing_them_goes_back_to_the_picker() {
    let mut h = Harness::new(items(2));
    h.start();
    h.key("?");
    assert_eq!(h.app.overlay(), Overlay::Help);
    h.key("Escape");
    assert!(h.picker_loaded());
}

#[test]
fn help_opens_with_question_mark_and_closes_with_esc() {
    let mut h = first_second();
    h.key("?");
    assert_eq!(h.app.overlay(), Overlay::Help);
    h.key("j");
    assert_eq!(h.app.overlay(), Overlay::Help);
    h.key("Escape");
    assert_eq!(h.app.overlay(), Overlay::None);
}

// ---- Settings ----------------------------------------------------------------------

#[test]
fn esc_cancels_settings() {
    let mut h = first_second();
    h.key(",");
    assert_eq!(h.app.overlay(), Overlay::Settings);
    assert_eq!(h.app.screen(), Screen::Settings);
    h.key("Escape");
    assert_eq!(h.app.overlay(), Overlay::None);
    assert_eq!(h.title(), Some("First"));
}

#[test]
fn keys_typed_into_the_settings_form_are_not_shortcuts() {
    let mut h = first_second();
    h.key(",");
    let (used, _) = h.app.key(&KeyPress { key: "j".into(), typing: true, ..KeyPress::default() });
    assert!(!used);
    assert_eq!(h.app.overlay(), Overlay::Settings);
}

#[test]
fn onboarding_cant_be_cancelled_and_connects_on_submit() {
    let mut h = Harness::new(items(2));
    h.status = Status { configured: false, readeck: false, fetches: true };
    h.save_result = Ok(saved(Status { configured: true, readeck: true, fetches: true }));
    h.start();
    assert_eq!(h.app.overlay(), Overlay::Onboarding);
    h.key("Escape");
    assert_eq!(h.app.overlay(), Overlay::Onboarding);
    assert!(h.app.cancel_settings().is_empty());
    assert_eq!(h.app.overlay(), Overlay::Onboarding);
    let fx = h.app.submit_settings(SettingsValues::default());
    assert!(h.app.settings().unwrap().checking);
    h.run(fx);
    assert!(h.picker_loaded());
    assert!(h.app.readeck());
}

#[test]
fn a_failed_save_shows_why_and_lets_you_try_again() {
    let mut h = first_second();
    h.key(",");
    h.save_result = Err("Miniflux rejected the API key".into());
    let fx = h.app.submit_settings(SettingsValues::default());
    h.run(fx);
    let form = h.app.settings().unwrap();
    assert_eq!(form.error, "Miniflux rejected the API key");
    assert!(!form.checking);
    assert_eq!(h.app.overlay(), Overlay::Settings);
}

#[test]
fn a_settings_window_leaves_the_reader_as_it_was() {
    let mut h = first_second();
    h.app.use_settings_window();
    h.key(",");
    assert!(h.app.settings().is_some());
    assert_eq!(h.app.overlay(), Overlay::None);
    assert_eq!(h.title(), Some("First"));
    h.key("j");
    assert_eq!(h.title(), Some("Second"), "the keys still read");

    let fx = h.app.cancel_settings();
    h.run(fx);
    assert!(h.app.settings().is_none());
    assert_eq!(h.title(), Some("Second"));
}

#[test]
fn asking_for_the_open_settings_window_only_brings_it_up() {
    let mut h = first_second();
    h.app.use_settings_window();
    h.key(",");
    let values = SettingsValues { miniflux_url: "typed".into(), ..SettingsValues::default() };
    h.app.settings.as_mut().unwrap().values = values;
    let (_, fx) = h.app.key(&KeyPress { key: ",".into(), ..KeyPress::default() });
    assert!(matches!(fx.as_slice(), [Effect::ShowSettings]), "the form isn't loaded again");
}

#[test]
fn saving_in_the_settings_window_starts_over_from_the_picker() {
    let mut h = first_second();
    h.app.use_settings_window();
    h.key(",");
    let fx = h.app.submit_settings(SettingsValues::default());
    h.run(fx);
    assert!(h.app.settings().is_none());
    assert_eq!(h.toast(), Some("Settings saved"));
    assert!(h.picker_loaded());
}

#[test]
fn readeck_values_that_failed_are_reported_after_the_form_closes() {
    let mut h = first_second();
    h.key(",");
    let status = Status { configured: true, readeck: true, fetches: true };
    h.save_result = Ok(Saved { status, username: None, readeck_problem: Some("Readeck rejected the token".into()) });
    let fx = h.app.submit_settings(SettingsValues::default());
    h.run(fx);
    assert!(h.app.settings().is_none(), "the feed server is saved, so the form is done");
    assert_eq!(h.app.toast().map(|t| t.kind), Some(ToastKind::Error));
    assert_eq!(h.toast(), Some("Settings saved. Readeck wasn't changed: Readeck rejected the token"));
    assert!(h.app.readeck(), "the Readeck saved before stays");
    assert!(h.picker_loaded());
}

#[test]
fn readeck_values_that_failed_are_reported_in_a_settings_window_and_on_first_run_too() {
    let problem = Some("Readeck rejected the token".to_string());
    let status = Status { configured: true, readeck: false, fetches: true };

    let mut h = first_second();
    h.app.use_settings_window();
    h.key(",");
    h.save_result = Ok(Saved { status, username: None, readeck_problem: problem.clone() });
    let fx = h.app.submit_settings(SettingsValues::default());
    h.run(fx);
    assert!(h.app.settings().is_none());
    assert_eq!(h.toast(), Some("Settings saved. Readeck wasn't changed: Readeck rejected the token"));

    let mut h = Harness::new(items(2));
    h.status = Status { configured: false, readeck: false, fetches: true };
    h.save_result = Ok(Saved { status, username: None, readeck_problem: problem });
    h.start();
    let fx = h.app.submit_settings(SettingsValues::default());
    h.run(fx);
    assert!(h.picker_loaded());
    assert_eq!(h.toast(), Some("Settings saved. Readeck wasn't changed: Readeck rejected the token"));
}

#[test]
fn saving_says_who_you_are_signed_in_as() {
    let status = Status { configured: true, readeck: false, fetches: true };
    let mut h = first_second();
    h.key(",");
    h.save_result = Ok(Saved { status, username: Some("ward".into()), readeck_problem: None });
    let fx = h.app.submit_settings(SettingsValues::default());
    h.run(fx);
    assert_eq!(h.toast(), Some("Signed in as ward"));
    assert_eq!(h.app.toast().map(|t| t.kind), Some(ToastKind::Info));

    // On first run too, where a plain "Settings saved" says nothing new.
    let mut h = Harness::new(items(2));
    h.status = Status { configured: false, readeck: false, fetches: true };
    h.save_result = Ok(Saved { status, username: Some("ward".into()), readeck_problem: None });
    h.start();
    let fx = h.app.submit_settings(SettingsValues::default());
    h.run(fx);
    assert_eq!(h.toast(), Some("Signed in as ward"));
}

#[test]
fn a_nameless_sign_in_shows_no_empty_name() {
    let status = Status { configured: true, readeck: false, fetches: true };
    for username in [None, Some(String::new()), Some("  ".to_string())] {
        let mut h = first_second();
        h.key(",");
        h.save_result = Ok(Saved { status, username: username.clone(), readeck_problem: None });
        let fx = h.app.submit_settings(SettingsValues::default());
        h.run(fx);
        assert_eq!(h.toast(), Some("Settings saved"), "{username:?}");

        let mut h = Harness::new(items(2));
        h.status = Status { configured: false, readeck: false, fetches: true };
        h.save_result = Ok(Saved { status, username, readeck_problem: None });
        h.start();
        let fx = h.app.submit_settings(SettingsValues::default());
        h.run(fx);
        assert_eq!(h.toast(), None, "the first run says nothing");
    }
}

#[test]
fn the_name_and_a_readeck_problem_are_both_in_the_toast() {
    let status = Status { configured: true, readeck: false, fetches: true };
    let mut h = first_second();
    h.key(",");
    let readeck_problem = Some("Readeck rejected the token".to_string());
    h.save_result = Ok(Saved { status, username: Some("ward".into()), readeck_problem });
    let fx = h.app.submit_settings(SettingsValues::default());
    h.run(fx);
    assert_eq!(h.toast(), Some("Signed in as ward. Readeck wasn't changed: Readeck rejected the token"));
    assert_eq!(h.app.toast().map(|t| t.kind), Some(ToastKind::Error));
}

#[test]
fn a_settings_window_doesnt_replace_onboarding() {
    let mut h = Harness::new(items(2));
    h.app.use_settings_window();
    h.status = Status { configured: false, readeck: false, fetches: true };
    h.start();
    assert_eq!(h.app.overlay(), Overlay::Onboarding);
    h.press(KeyPress { key: ",".into(), ..KeyPress::default() });
    assert_eq!(h.app.overlay(), Overlay::Onboarding);
}

#[test]
fn q_quits_from_the_picker() {
    let mut h = first_second();
    h.key("p");
    assert_eq!(h.app.overlay(), Overlay::Picker);
    let (used, fx) = h.app.key(&KeyPress { key: "q".into(), ..KeyPress::default() });
    assert!(used);
    assert!(matches!(fx.as_slice(), [Effect::Quit]));
}

#[test]
fn saving_settings_starts_over_from_the_picker() {
    let mut h = first_second();
    h.key(",");
    let fx = h.app.submit_settings(SettingsValues::default());
    h.run(fx);
    assert_eq!(h.toast(), Some("Settings saved"));
    assert!(h.picker_loaded());
    assert!(h.app.queue().is_empty());
    h.key("Escape");
    assert!(h.picker_loaded(), "nothing to go back to");
}

#[test]
fn onboarding_says_why_the_config_file_wasnt_used() {
    let problem = "Couldn't use /home/me/.config/onward/config.toml. Line 3: string values must be quoted.";
    let mut h = Harness::new(items(2));
    h.status = Status { configured: false, readeck: false, fetches: true };
    h.problem = Some(problem.into());
    h.start();
    assert_eq!(h.app.settings().unwrap().problem.as_deref(), Some(problem));
}

#[test]
fn onboarding_shows_no_notice_when_the_config_file_is_fine() {
    let mut h = Harness::new(items(2));
    h.status = Status { configured: false, readeck: false, fetches: true };
    h.start();
    assert_eq!(h.app.settings().unwrap().problem, None);
}

#[test]
fn a_toast_goes_when_its_time_is_up_but_not_a_newer_one() {
    let mut h = first_second();
    h.key("m");
    let first = h.app.toast().unwrap().serial;
    h.key("m");
    h.app.dismiss_toast(first);
    assert_eq!(h.toast(), Some("Marked unread"));
    let second = h.app.toast().unwrap().serial;
    h.app.dismiss_toast(second);
    assert_eq!(h.toast(), None);
}

// ---- Newest first (s) ----------------------------------------------------------

fn newest_first(entries: Vec<Entry>) -> Harness {
    let mut h = Harness::new(entries);
    h.app = App::new(Prefs { newest_first: true, ..Prefs::default() }, false);
    h
}

#[test]
fn s_reopens_the_unread_list_newest_first_and_remembers_it() {
    let mut h = Harness::new(items(5));
    h.open(false);
    let (_, fx) = h.app.key(&KeyPress { key: "s".into(), ..KeyPress::default() });
    assert!(fx.contains(&Effect::SavePrefs(Prefs { newest_first: true, ..Prefs::default() })));
    h.run(fx);
    assert_eq!(h.toast(), Some("Unread items newest first"));
    assert_eq!(h.app.selection_label(), "All unread");
    assert_eq!(h.press_through(&["j"]), [5, 4, 3, 2, 1]);
    assert!(h.caught_up());
    h.key("s");
    assert_eq!(h.toast(), Some("Unread items oldest first"));
    assert!(!h.app.newest_first());
}

#[test]
fn newest_first_pages_back_through_everything_once() {
    let mut h = newest_first(items(120));
    h.open(false);
    assert_eq!(page_request(h.entries_calls()[0]).map(|r| r.order), Some(Order::NewestFirst));
    assert_eq!(h.app.position(), "1 / 120");
    let mut expected = range(1, 120);
    expected.reverse();
    assert_eq!(h.press_through(&["j"]), expected);
    assert!(h.fake.unread_ids().is_empty());
}

#[test]
fn newest_first_still_shows_what_arrived_while_you_were_reading() {
    let mut h = newest_first(items(3));
    h.open(false);
    h.key("j");
    h.key("j");
    assert_eq!(h.title(), Some("Item 1"));
    h.fake.entries.push(entry(4, "Arrived later"));
    h.fake.entries.push(entry(5, "Also later"));
    h.key("j");
    // It looked after the newest item loaded, which here was the first one.
    let last = page_request(h.entries_calls().last().unwrap()).unwrap();
    assert_eq!((last.order, last.past), (Order::OldestFirst, seconds(&entry(3, "")) - 1));
    assert_eq!(h.press_through(&["j"]), [4, 5]);
    assert!(h.caught_up());
}

#[test]
fn s_in_the_picker_only_changes_the_next_list_and_history_stays_newest_first() {
    let mut h = Harness::new(items(3));
    h.start();
    h.key("s");
    assert!(h.entries_calls().is_empty());
    assert_eq!(h.app.overlay(), Overlay::Picker);
    h.key("Enter");
    assert_eq!(h.title(), Some("Item 3"));
    h.key("s");
    assert_eq!(h.title(), Some("Item 1"));

    let mut h = newest_first(read_items(3));
    h.open(true);
    assert_eq!(h.title(), Some("Read 3"));
    h.key("s");
    assert_eq!(h.title(), Some("Read 3"), "history doesn't open again");
}

// ---- Search (/) -------------------------------------------------------------------

/// Unread and read items, some with “Rust” in the title.
fn mixed() -> Vec<Entry> {
    vec![
        entry(1, "Rust one"),
        entry_at(2, "Go two", EntryStatus::Read, 2),
        entry_at(3, "Rust three", EntryStatus::Read, 3),
        entry(4, "Rust four"),
        entry(5, "Zig five"),
    ]
}

fn typing(key: &str) -> KeyPress {
    KeyPress { key: key.into(), typing: true, ..KeyPress::default() }
}

#[test]
fn slash_searches_read_and_unread_items_and_they_read_like_any_list() {
    let mut h = Harness::new(mixed());
    h.open(false);
    h.key("/");
    assert_eq!(h.app.overlay(), Overlay::Search);
    assert!(!h.app.key(&typing("j")).0, "typing in the box isn't a shortcut");
    assert_eq!(h.title(), Some("Rust one"));
    let fx = h.app.search("  Rust \n");
    h.run(fx);
    assert_eq!(h.app.overlay(), Overlay::None);
    assert_eq!(h.app.selection_label(), "“Rust”");
    assert!(h.app.searching() && !h.app.history_badge());
    assert_eq!(h.app.position(), "1 / 3");
    assert_eq!(h.title(), Some("Rust four"));
    assert_eq!(h.press_through(&["j"]), [4, 3, 1]);
    assert!(h.caught_up());
    assert_eq!(h.app.end_title(), "End of results");
    // No looking for arrivals at the end of a search.
    assert_eq!(h.entries_calls().iter().filter(|c| matches!(c, Effect::Search { .. })).count(), 1);
    assert!(h.status_calls().contains(&(vec![4], EntryStatus::Read)));
    assert!(!h.status_calls().iter().any(|(ids, _)| ids == &[3]), "already read");
    h.key("j");
    assert!(h.picker_loaded());
    h.key("/");
    assert_eq!(h.app.search_query(), "Rust");
}

#[test]
fn esc_closes_the_search_box_back_where_it_was_and_so_does_searching_nothing() {
    let mut h = Harness::new(mixed());
    h.start();
    h.key("/");
    h.key("Escape");
    assert_eq!(h.app.overlay(), Overlay::Picker);
    h.key("Enter");
    h.key("/");
    let fx = h.app.search("   ");
    assert!(fx.is_empty());
    assert_eq!(h.app.overlay(), Overlay::None);
    assert_eq!(h.app.selection_label(), "All unread");
    h.key("/");
    h.app.cancel_search();
    assert_eq!(h.app.overlay(), Overlay::None);
    assert!(h.app.search("Rust").is_empty(), "the box is closed");
}

#[test]
fn search_results_load_a_page_at_a_time_by_offset() {
    let mut h = Harness::new(items(120));
    h.open(false);
    h.key("/");
    let fx = h.app.search("Item");
    h.run(fx);
    assert_eq!(h.app.position(), "1 / 120");
    assert_eq!(h.app.queue().len(), PAGE);
    for _ in 0..PAGE - 10 {
        h.key("n");
    }
    assert!(matches!(h.entries_calls().last(), Some(Effect::Search { offset: PAGE, first: false, .. })));
    let mut expected = range(1, 120);
    expected.reverse();
    let shown = h.press_through(&["n"]);
    assert_eq!(shown, expected[PAGE - 10..]);
    assert!(h.caught_up());
}

#[test]
fn a_failed_search_leaves_the_list_and_the_place_in_it() {
    let mut h = Harness::new(items(5));
    h.open(false);
    h.key("j");
    let id = h.app.current().unwrap().id;
    let (label, position, shift_a_before) =
        (h.app.selection_label().to_string(), h.app.position(), h.app.newest_at_open);
    let requests = h.entries_calls().len();

    h.key("/");
    h.fail_next_search = Some("This server has no search".into());
    let fx = h.app.search("Rust");
    h.run(fx);

    assert_eq!(h.toast(), Some("This server has no search"));
    assert_eq!(h.app.overlay(), Overlay::None);
    assert_eq!(h.app.screen(), Screen::Article);
    assert_eq!(h.app.current().unwrap().id, id);
    assert_eq!(h.app.queue().len(), 5);
    assert_eq!(h.app.selection_label(), label);
    assert_eq!(h.app.position(), position);
    assert_eq!(h.app.newest_at_open, shift_a_before);
    assert!(!h.app.searching());
    assert_eq!(h.app.search_query(), "Rust", "the box offers it again, to change or to try once more");
    assert_eq!(h.entries_calls().len(), requests + 1, "the list isn't loaded again");

    // It carries on as if nothing happened: j moves on and marks read, Shift+A still counts this list.
    h.key("j");
    assert_eq!(h.app.position(), "3 / 5");
    h.key("A");
    assert!(h.app.confirm_text().contains(&label), "{}", h.app.confirm_text());
}

#[test]
fn a_failed_search_from_another_search_keeps_its_results() {
    let mut h = Harness::new(titled(&["Rust one", "Rust two", "Rust three", "Zig"]));
    h.open(false);
    h.key("/");
    let fx = h.app.search("Rust");
    h.run(fx);
    h.key("j");
    let id = h.app.current().unwrap().id;

    h.key("/");
    assert_eq!(h.app.search_query(), "Rust");
    h.fail_next_search = Some("Could not reach the server".into());
    let fx = h.app.search("Zig");
    h.run(fx);

    assert!(h.app.searching());
    assert_eq!(h.app.selection_label(), "“Rust”");
    assert_eq!(h.app.current().unwrap().id, id);
    assert_eq!(h.app.position(), "2 / 3");
}

#[test]
fn a_failed_search_from_the_picker_goes_back_to_the_picker() {
    let mut h = Harness::new(items(3));
    h.start();
    assert!(h.picker_loaded());
    h.key("/");
    h.hold_next_search = true;
    let fx = h.app.search("Rust");
    h.run(fx);
    assert_eq!((h.app.overlay(), h.app.screen()), (Overlay::None, Screen::Loading), "nothing to show meanwhile");

    let Some(Effect::Search { load, .. }) = h.held.take() else { panic!("no search held") };
    let fx = h.app.apply(Outcome::Opened { load, result: Err("This server has no search".into()) });
    h.run(fx);
    assert_eq!(h.app.overlay(), Overlay::Picker);
    assert_eq!(h.toast(), Some("This server has no search"));
    assert!(h.app.queue().is_empty() && !h.app.searching());
    assert_eq!(h.app.selection_label(), "");
}

#[test]
fn a_failed_search_over_a_list_still_opening_goes_to_the_picker() {
    let mut h = Harness::new(items(3));
    h.start();
    let (_, opening) = h.app.key(&KeyPress { key: "Enter".into(), ..KeyPress::default() });
    assert!(matches!(opening.as_slice(), [Effect::OpenSelection { .. }]), "left unanswered");
    h.key("/");
    h.fail_next_search = Some("This server has no search".into());
    let fx = h.app.search("Rust");
    h.run(fx);
    assert_eq!(h.app.overlay(), Overlay::Picker, "that answer is lost, so there's nothing to read");
    assert_eq!(h.toast(), Some("This server has no search"));
}

#[test]
fn the_article_stays_on_screen_until_the_search_answers() {
    let mut h = Harness::new(titled(&["Rust one", "Plain", "Rust two"]));
    h.open(false);
    h.key("j");
    let id = h.app.current().unwrap().id;
    h.key("/");
    h.hold_next_search = true;
    let fx = h.app.search("Rust");
    h.run(fx);

    // Every window scrolls back to the top on another screen, so "Loading…" would lose the place in the article.
    assert_eq!(h.app.screen(), Screen::Article);
    assert_eq!(h.app.current().unwrap().id, id);
    assert!(!h.app.searching());
    assert_eq!(h.toast(), Some("Searching…"));

    let Some(Effect::Search { load, .. }) = h.held.take() else { panic!("no search held") };
    let page = h.fake.search("Rust", 0, PAGE);
    let fx = h.app.apply(Outcome::Opened { load, result: Ok((page, 0)) });
    h.run(fx);
    assert!(h.app.searching());
    assert_eq!(h.app.selection_label(), "“Rust”");
    assert_eq!(h.app.position(), "1 / 2");
    assert_eq!(h.toast(), None, "it's there, so \"Searching…\" is over");
}

#[test]
fn a_search_that_finds_nothing_is_an_answer_and_replaces_the_list() {
    let mut h = Harness::new(items(3));
    h.open(false);
    h.key("/");
    let fx = h.app.search("Nothing like it");
    h.run(fx);
    assert!(h.app.searching());
    assert!(h.app.queue().is_empty());
    assert_eq!(h.app.selection_label(), "“Nothing like it”");
    assert_eq!(h.app.screen(), Screen::Empty);
}

#[test]
fn a_page_of_the_old_list_doesnt_join_search_results() {
    let mut h = Harness::new(items(120));
    h.open(false);
    // Near the end of the first page, with the next one on its way.
    h.hold_next_page = true;
    for _ in 0..PAGE - 10 {
        h.key("j");
    }
    let old_page = h.held.take().expect("the next page is loading");
    let Effect::LoadPage { load, .. } = old_page else { panic!("not a page") };
    h.key("/");
    h.hold_next_search = true;
    let fx = h.app.search("Item");
    h.run(fx);
    let Some(Effect::Search { load: search, .. }) = h.held.take() else { panic!("no search held") };

    // The old list's page comes in late, and no new one goes out in the meantime.
    let late = h.fake.page(
        Selection::All,
        EntryStatus::Unread,
        PageRequest { past: 0, limit: PAGE, order: Order::OldestFirst },
    );
    let fx = h.app.apply(Outcome::Page { load, arrivals: false, result: Ok(late) });
    assert!(fx.is_empty());
    let calls = h.entries_calls().len();
    h.key("j");
    assert_eq!(h.entries_calls().len(), calls, "a page asked for now would land in the search results");

    let page = h.fake.search("Item", 0, 10);
    let fx = h.app.apply(Outcome::Opened { load: search, result: Ok((page, 0)) });
    h.run(fx);
    assert!(h.app.searching());
    assert_eq!(h.app.queue().len(), 10);
}

#[test]
fn a_failed_search_lets_the_old_list_page_on() {
    let mut h = Harness::new(items(120));
    h.open(false);
    h.hold_next_page = true;
    for _ in 0..PAGE - 10 {
        h.key("j");
    }
    assert!(h.held.take().is_some(), "the next page was on its way");
    h.key("/");
    h.fail_next_search = Some("No".into());
    let fx = h.app.search("Rust");
    h.run(fx);
    assert_eq!(h.app.queue().len(), PAGE);

    h.key("j");
    assert!(h.app.queue().len() > PAGE, "the page that was dropped is asked for again");
}

#[test]
fn a_stale_search_changes_nothing_good_or_bad() {
    let mut h = Harness::new(titled(&["Rust one", "Rust two", "Zig"]));
    h.open(false);
    h.key("/");
    h.hold_next_search = true;
    let fx = h.app.search("Rust");
    h.run(fx);
    let Some(Effect::Search { load: first, .. }) = h.held.take() else { panic!("no search held") };

    // `/` while it's on its way: the newer search is the one that counts.
    h.key("/");
    assert_eq!(h.app.overlay(), Overlay::Search);
    h.hold_next_search = true;
    let fx = h.app.search("Zig");
    h.run(fx);
    let Some(Effect::Search { load: second, .. }) = h.held.take() else { panic!("no search held") };

    h.app.apply(Outcome::Opened { load: first, result: Err("late".into()) });
    assert_ne!(h.toast(), Some("late"));
    let page = h.fake.search("Rust", 0, PAGE);
    h.app.apply(Outcome::Opened { load: first, result: Ok((page, 0)) });
    assert!(!h.app.searching(), "the first answer is out of date");

    let page = h.fake.search("Zig", 0, PAGE);
    h.app.apply(Outcome::Opened { load: second, result: Ok((page, 0)) });
    assert_eq!(h.app.selection_label(), "“Zig”");
    assert_eq!(h.app.position(), "1 / 1");
}

#[test]
fn opening_something_else_drops_a_search_on_its_way() {
    let mut h = Harness::new(titled(&["Rust one", "Plain"]));
    h.open(false);
    h.key("/");
    h.hold_next_search = true;
    let fx = h.app.search("Rust");
    h.run(fx);
    let Some(Effect::Search { load, .. }) = h.held.take() else { panic!("no search held") };

    h.key("p");
    h.key("Enter");
    assert_eq!(h.app.selection_label(), "All unread");
    h.app.apply(Outcome::Opened { load, result: Err("late".into()) });
    assert_eq!(h.app.overlay(), Overlay::None);
    assert_ne!(h.toast(), Some("late"));
}

#[test]
fn shift_a_waits_for_the_search_on_its_way() {
    let mut h = Harness::new(items(3));
    h.open(false);
    h.key("/");
    h.hold_next_search = true;
    let fx = h.app.search("Item");
    h.run(fx);
    let calls = h.calls.len();
    h.key("A");
    assert_eq!(h.calls.len(), calls, "the list it would mark is about to go");
}

#[test]
fn a_failed_second_page_of_a_search_keeps_the_results_and_tries_again() {
    let mut h = Harness::new(items(120));
    h.open(false);
    h.key("/");
    let fx = h.app.search("Item");
    h.run(fx);
    assert_eq!(h.app.queue().len(), PAGE);
    h.fail_next_page = Some("Could not reach the server".into());
    for _ in 0..PAGE - 10 {
        h.key("j");
    }
    assert_eq!(h.toast(), Some("Could not reach the server"));
    assert!(h.app.searching());
    assert_eq!(h.app.queue().len(), PAGE);
    assert_eq!(h.app.position(), format!("{} / 120", PAGE - 9));
    h.key("j");
    assert!(h.app.queue().len() > PAGE, "the next move asks again");
}

#[test]
fn shift_a_doesnt_mark_search_results_read_all_at_once() {
    let mut h = Harness::new(mixed());
    h.open(false);
    h.key("/");
    let fx = h.app.search("Rust");
    h.run(fx);
    h.shift("A");
    assert_eq!(h.app.overlay(), Overlay::None);
    assert_eq!(h.toast(), Some("Search results can't be marked read all at once"));
}

#[test]
fn a_search_answer_after_opening_something_else_changes_nothing() {
    let mut h = Harness::new(items(60));
    h.open(false);
    h.key("/");
    let fx = h.app.search("Item");
    h.run(fx);
    h.hold_next_page = true;
    for _ in 0..PAGE - 10 {
        h.key("n");
    }
    h.key("p");
    h.key("Enter");
    h.answer();
    assert!(!h.app.searching());
    assert_eq!(h.app.queue().len(), PAGE);
    assert_eq!(h.title(), Some("Item 1"));
}

// ---- Shift+A in the picker ----------------------------------------------------------

#[test]
fn shift_a_in_the_picker_asks_then_marks_the_row_read() {
    let mut h = Harness::new(items(3));
    h.start();
    h.shift("A");
    assert_eq!(h.app.overlay(), Overlay::Confirm);
    assert_eq!(h.app.confirm_text(), "Mark all 3 unread items in “All unread” as read?");
    let counts_before = h.calls.iter().filter(|c| **c == Effect::FeedCounts).count();
    h.key("y");
    assert!(h.fake.unread_ids().is_empty());
    assert_eq!(h.toast(), Some("Marked “All unread” read"));
    assert_eq!(h.app.overlay(), Overlay::Picker);
    assert_eq!(h.calls.iter().filter(|c| **c == Effect::FeedCounts).count(), counts_before + 1);
}

#[test]
fn shift_a_in_the_picker_can_be_cancelled_and_says_when_theres_nothing() {
    let mut h = Harness::new(items(2));
    h.start();
    h.shift("A");
    h.key("n");
    assert_eq!(h.fake.unread_ids(), [1, 2]);
    assert_eq!(h.app.overlay(), Overlay::Picker);
    h.mark_elsewhere(&[1, 2]);
    h.shift("A");
    assert_eq!(h.app.overlay(), Overlay::Picker);
    assert_eq!(h.toast(), Some("Nothing unread in “All unread”"));
    h.key("h");
    h.shift("A");
    assert_eq!(h.toast(), Some("These are read already. Press h for unread items."));
}

#[test]
fn shift_a_in_the_picker_shows_the_open_queue_read_too() {
    let mut h = Harness::new(items(3));
    h.open(false);
    h.key("p");
    h.key("ArrowDown"); // the category
    h.shift("A");
    assert_eq!(h.app.confirm_text(), "Mark all 3 unread items in “Dev” as read?");
    h.key("y");
    h.key("Escape");
    assert_eq!(h.app.current().map(|e| e.status), Some(EntryStatus::Read));
    assert!(h.app.queue().iter().all(|e| e.status == EntryStatus::Read));
}

// ---- The image viewer (v) ----------------------------------------------------------

fn with_images() -> Harness {
    let mut article = entry(1, "Pictures");
    article.content = "<p>a</p><img src='/a.png' alt='First'>\
                       <p><a href='https://example.com/b-big'><img src='https://example.com/b.png'></a></p>"
        .into();
    let mut h = Harness::new(vec![article, entry(2, "Words")]);
    h.open(false);
    h
}

fn viewing(h: &Harness) -> Option<(&str, usize)> {
    h.app.viewer().map(|v| (v.images[v.index].src.as_str(), v.index))
}

#[test]
fn v_steps_through_the_articles_images_and_esc_closes() {
    let mut h = with_images();
    h.key("v");
    assert_eq!(h.app.overlay(), Overlay::Image);
    assert_eq!(viewing(&h), Some(("https://example.com/a.png", 0)));
    assert_eq!(h.app.viewer().unwrap().images[0].alt, "First");
    h.key("ArrowRight");
    h.key("ArrowRight");
    assert_eq!(viewing(&h), Some(("https://example.com/b.png", 1)));
    h.key("o");
    assert!(
        h.calls.contains(&Effect::OpenInBrowser { url: "https://example.com/b-big".into(), then: AfterOpen::Nothing })
    );
    h.key("j");
    assert_eq!(h.title(), Some("Pictures"), "the viewer keeps the reader's keys");
    h.key("ArrowLeft");
    assert_eq!(viewing(&h).map(|v| v.1), Some(0));
    h.key("Escape");
    assert_eq!(h.app.overlay(), Overlay::None);
    assert!(h.app.viewer().is_none());
    // A toolbar button closes it too (on the Mac the toolbar is still there).
    h.key("v");
    let fx = h.app.action(Action::Prev);
    h.run(fx);
    assert_eq!(h.app.overlay(), Overlay::None);
    h.key("j");
    assert_eq!(h.title(), Some("Words"));
    h.key("v");
    assert_eq!(h.app.overlay(), Overlay::None);
    assert_eq!(h.toast(), Some("This article has no images"));
}

#[test]
fn clicking_an_image_opens_the_viewer_on_it() {
    let mut h = with_images();
    h.app.view_image("https://example.com/b.png");
    assert_eq!(viewing(&h), Some(("https://example.com/b.png", 1)));
    h.app.close_viewer();
    assert_eq!(h.app.overlay(), Overlay::None);
    h.app.view_image("https://elsewhere.example/c.png");
    assert_eq!(viewing(&h), Some(("https://elsewhere.example/c.png", 0)));
    assert_eq!(h.app.viewer().unwrap().images.len(), 1);
}

#[test]
fn the_article_after_this_one_is_known_ahead() {
    let mut h = Harness::new(items(2));
    h.open(false);
    assert_eq!(h.app.upcoming().map(|e| e.id), Some(2));
    h.key("j");
    assert!(h.app.upcoming().is_none());
}

// ---- j at the end goes on to the next feed ----------------------------------------------

/// Feeds Alpha and Beta in Dev, Gamma in Blogs. Rows: All unread, Blogs, Gamma, Dev, Alpha, Beta.
fn three_feeds(beta_unread: i64) -> Vec<FeedCount> {
    let feed = |id, title: &str, category: (i64, &str), unread| FeedCount {
        feed: Feed { id, title: title.into(), category: Category { id: category.0, title: category.1.into() } },
        unread: Some(unread),
        read: Some(1),
    };
    vec![feed(1, "Alpha", (1, "Dev"), 2), feed(2, "Beta", (1, "Dev"), beta_unread), feed(3, "Gamma", (2, "Blogs"), 5)]
}

/// Opens the picker row called `label`.
fn open_row(h: &mut Harness, label: &str) {
    h.start();
    let row = h.app.picker_items().iter().position(|i| i.label == label).unwrap();
    for _ in 0..row {
        h.key("j");
    }
    h.key("Enter");
    assert_eq!(h.app.selection_label(), label);
}

#[test]
fn j_at_the_end_of_a_feed_goes_on_to_the_next_feed_with_unread_items() {
    let mut h = Harness::new(items(2));
    h.counts = three_feeds(3);
    open_row(&mut h, "Alpha");
    h.key("j");
    h.key("j");
    assert!(h.caught_up());
    assert_eq!(h.app.end_hint(), "Press j for Beta (3), Enter for the feeds, k to go back.");
    h.key("j");
    assert_eq!(h.app.selection_label(), "Beta");
    assert!(matches!(
        h.calls.iter().rfind(|c| matches!(c, Effect::OpenSelection { .. })),
        Some(Effect::OpenSelection { selection: Selection::Feed { id: 2 }, mode: Mode::Unread, .. })
    ));
    assert_eq!(h.app.picker_items()[h.app.picker_cursor()].label, "Beta");
}

#[test]
fn j_at_the_end_skips_feeds_with_nothing_and_starts_over_at_the_top() {
    let mut h = Harness::new(items(1));
    h.counts = three_feeds(0);
    open_row(&mut h, "Alpha");
    h.key("j");
    assert_eq!(h.app.end_hint(), "Press j for Gamma (5), Enter for the feeds, k to go back.");
    h.key("j");
    assert_eq!(h.app.selection_label(), "Gamma");
}

#[test]
fn j_at_the_end_of_all_unread_or_a_search_still_goes_to_the_feeds() {
    let mut h = Harness::new(items(1));
    h.counts = three_feeds(3);
    h.open(false);
    h.key("j");
    assert_eq!(h.app.end_hint(), "Press j or Enter for the feeds, k to go back.");
    h.key("j");
    assert!(h.picker_loaded());

    let mut h = Harness::new(mixed());
    h.counts = three_feeds(3);
    open_row(&mut h, "Alpha");
    h.key("/");
    let fx = h.app.search("Zig");
    h.run(fx);
    h.key("j");
    assert!(h.caught_up());
    h.key("j");
    assert!(h.picker_loaded());
}

#[test]
fn enter_at_the_end_still_opens_the_picker() {
    let mut h = Harness::new(items(1));
    h.counts = three_feeds(3);
    open_row(&mut h, "Alpha");
    h.key("j");
    h.key("Enter");
    assert!(h.picker_loaded());
}

#[test]
fn the_picker_stays_on_screen_behind_the_question_shift_a_asks() {
    let mut h = Harness::new(items(2));
    h.start();
    assert!(h.app.picker_shown());
    h.shift("A");
    assert_eq!(h.app.overlay(), Overlay::Confirm);
    assert!(h.app.picker_shown());
    h.key("n");
    h.key("Escape"); // nothing open yet: the picker stays
    h.key("Enter");
    h.shift("A");
    assert_eq!(h.app.overlay(), Overlay::Confirm);
    assert!(!h.app.picker_shown(), "asked from the reader");
}

// ---- A server that can't fetch (the Google Reader API) ------------------------------------------------------

#[test]
fn a_full_article_the_server_wont_fetch_is_a_toast_and_f_works_again_after() {
    let mut h = Harness::new(items(3));
    h.fail_fetch = Some("Fetching the full article isn't available".into());
    h.open(false);
    h.key("f");
    assert_eq!(h.toast(), Some("Fetching the full article isn't available"));
    assert_eq!(h.app.toast().map(|t| t.kind), Some(ToastKind::Error));
    // Nothing is left waiting: f asks again, and the article is as it was.
    h.fail_fetch = None;
    h.key("f");
    assert_eq!(h.toast(), Some("Full article"));
    assert_eq!(h.calls.iter().filter(|c| matches!(c, Effect::FetchContent { .. })).count(), 2);
}

#[test]
fn a_refresh_the_server_turns_down_is_a_toast() {
    let mut h = Harness::new(items(3));
    h.fail_fetch = Some("Refreshing feeds isn't available".into());
    h.start();
    h.key("r");
    assert_eq!(h.toast(), Some("Refreshing feeds isn't available"));
    assert!(!h.calls.iter().any(|c| matches!(c, Effect::ReloadPickerAfter { .. })));
}

#[test]
fn on_a_server_that_doesnt_fetch_f_and_r_only_say_so() {
    let mut h = Harness::new(items(3));
    h.status = Status { configured: true, readeck: false, fetches: false };
    h.open(false);
    assert!(!h.app.fetches());
    h.key("f");
    assert_eq!(h.toast(), Some(crate::feeds::FETCH_CONTENT_REFUSED));
    h.key("p");
    h.key("r");
    assert_eq!(h.toast(), Some(crate::feeds::REFRESH_REFUSED));
    assert!(!h.calls.iter().any(|c| matches!(c, Effect::FetchContent { .. } | Effect::RefreshFeeds)));
    // Saved settings for a Miniflux server bring them back.
    h.save_result = Ok(saved(Status { configured: true, readeck: false, fetches: true }));
    h.key(",");
    let fx = h.app.submit_settings(SettingsValues::default());
    h.run(fx);
    assert!(h.app.fetches());
}

#[test]
fn history_with_counts_the_server_doesnt_know_lists_every_feed_and_says_where_j_goes() {
    let mut h = Harness::new(read_items(2));
    h.counts = three_feeds(0).into_iter().map(|c| FeedCount { read: None, ..c }).collect();
    h.start();
    h.key("h");
    let rows: Vec<_> = h.app.picker_items().into_iter().map(|i| (i.label, i.count)).collect();
    assert_eq!(rows.len(), 6, "{rows:?}");
    assert!(rows.iter().all(|(_, count)| count.is_none()), "{rows:?}");
    let row = h.app.picker_items().iter().position(|i| i.label == "Alpha").unwrap();
    for _ in 0..row {
        h.key("j");
    }
    h.key("Enter");
    assert_eq!(h.app.selection_label(), "Alpha");
    h.press_through(&["j"]);
    assert!(h.caught_up());
    assert_eq!(h.app.end_hint(), "Press j for Beta, Enter for the feeds, k to go back.");
}

#[test]
fn choosing_colours_saves_them_once() {
    let mut app = App::new(Prefs::default(), false);
    let chosen = app.set_colours(Colours::OnwardLight);
    assert!(matches!(chosen.as_slice(), [Effect::SavePrefs(p)] if p.colours == Colours::OnwardLight));
    assert!(app.set_colours(Colours::OnwardLight).is_empty(), "nothing changed, nothing to save");
}

// ---- Starring ------------------------------------------------------------------------------------------------------

/// Items 1 … 6, oldest first; 2, 4 and 5 are starred, and 4 is read.
fn some_starred() -> Vec<Entry> {
    range(1, 6)
        .into_iter()
        .map(|id| {
            let status = if id == 4 { EntryStatus::Read } else { EntryStatus::Unread };
            Entry { starred: [2, 4, 5].contains(&id), ..entry_at(id, &format!("Item {id}"), status, id) }
        })
        .collect()
}

fn star_calls(h: &Harness) -> Vec<(i64, bool)> {
    h.calls
        .iter()
        .filter_map(|c| match c {
            Effect::SetStarred { id, starred, .. } => Some((*id, *starred)),
            _ => None,
        })
        .collect()
}

/// Opens the Starred row from the picker, in history first if asked.
fn open_starred(h: &mut Harness, history: bool) {
    h.start();
    if history {
        h.key("h");
    }
    let row = h.app.picker_items().iter().position(|i| i.selection == Selection::Starred).unwrap();
    for _ in 0..row {
        h.key("j");
    }
    h.key("Enter");
    assert_eq!(h.app.selection_label(), "Starred");
}

#[test]
fn star_shows_at_once_says_so_and_stays_on_the_article() {
    let mut h = Harness::new(items(3));
    h.open(false);
    assert!(!h.app.starred());
    h.key("*");
    assert!(h.app.starred());
    assert_eq!(h.toast(), Some("Starred"));
    assert_eq!(h.title(), Some("Item 1"), "doesn't move on");
    assert_eq!(h.app.current().map(|e| e.status), Some(EntryStatus::Unread), "nor mark it read");
    assert!(h.fake.entries[0].starred);
    h.key("*");
    assert!(!h.app.starred());
    assert_eq!(h.toast(), Some("Unstarred"));
    assert_eq!(star_calls(&h), [(1, true), (1, false)]);
    assert!(!h.fake.entries[0].starred);
}

#[test]
fn a_star_the_server_turns_down_is_put_back_with_the_reason() {
    let mut h = Harness::new(items(3));
    h.open(false);
    h.fail_next_star = Some("Miniflux 500 Internal Server Error".into());
    h.key("*");
    assert!(!h.app.starred(), "put back");
    assert_eq!(h.toast(), Some("Miniflux 500 Internal Server Error"));
    assert_eq!(h.app.toast().map(|t| t.kind), Some(ToastKind::Error));
    // Unstarring that fails stars it again.
    h.key("*");
    assert!(h.app.starred());
    h.fail_next_star = Some("Nope".into());
    h.key("*");
    assert!(h.app.starred());
}

#[test]
fn a_star_turned_down_after_moving_on_is_put_back_where_it_was() {
    let mut h = Harness::new(items(3));
    h.open(false);
    let (_, fx) = h.app.key(&KeyPress { key: "*".into(), ..KeyPress::default() });
    assert!(h.app.starred());
    h.key("n");
    let Some(Effect::SetStarred { index, id, starred }) = fx.into_iter().next() else { panic!("no star") };
    let fx = h.app.apply(Outcome::StarredSet { index, id, starred, result: Err("Nope".into()) });
    h.run(fx);
    assert!(!h.app.queue()[0].starred);
    assert_eq!(h.title(), Some("Item 2"), "you stay where you are");
}

#[test]
fn star_does_nothing_in_the_picker_or_at_the_end() {
    let mut h = Harness::new(items(1));
    h.start();
    h.key("*");
    assert!(star_calls(&h).is_empty());
    h.key("Enter");
    h.key("j");
    assert!(h.caught_up());
    h.key("*");
    assert!(star_calls(&h).is_empty());
}

#[test]
fn the_picker_has_a_starred_row_with_its_count_after_all_unless_nothing_is_starred() {
    let mut h = Harness::new(items(2));
    h.starred = Some(3);
    h.start();
    let rows: Vec<_> = h.app.picker_items().into_iter().map(|i| (i.label, i.count, i.depth)).collect();
    assert_eq!(rows[..2], [("All unread".into(), Some(2), 0), ("Starred".into(), Some(3), 0)]);
    h.key("h");
    let rows: Vec<_> = h.app.picker_items().into_iter().map(|i| (i.label, i.count)).collect();
    assert_eq!(rows[..2], [("All read".into(), Some(3)), ("Starred".into(), Some(3))]);

    let mut h = Harness::new(items(2));
    h.starred = Some(0);
    h.start();
    assert!(h.app.picker_items().iter().all(|i| i.selection != Selection::Starred));

    let mut h = Harness::new(items(2));
    h.starred = None; // a server that can't count them
    h.start();
    assert_eq!(h.app.picker_items()[1].count, None);
    assert_eq!(h.app.picker_items()[1].label, "Starred");
}

#[test]
fn the_starred_list_is_newest_first_read_and_unread_whatever_the_order() {
    let mut h = Harness::new(some_starred());
    h.starred = Some(3);
    assert!(!h.app.newest_first(), "unread items oldest first");
    open_starred(&mut h, false);
    assert!(matches!(
        h.calls.iter().rfind(|c| matches!(c, Effect::OpenSelection { .. })),
        Some(Effect::OpenSelection {
            selection: Selection::Starred,
            request: PageRequest { order: Order::NewestFirst, .. },
            ..
        })
    ));
    assert_eq!(h.app.queue().iter().map(|e| e.id).collect::<Vec<_>>(), [5, 4, 2]);
    assert!(h.app.starred());
    assert!(h.app.starred_list() && !h.app.searching() && !h.app.history_badge());
    assert_eq!(h.app.position(), "1 / 3");
    // j marks read and moves on, as anywhere.
    h.key("j");
    assert_eq!(h.title(), Some("Item 4"));
    assert_eq!(h.status_calls(), [(vec![5], EntryStatus::Read)]);
    // m toggles: item 4 is read, so it's unread again.
    h.key("m");
    assert_eq!(h.app.current().map(|e| e.status), Some(EntryStatus::Unread));
    // s doesn't change the starred list's order, nor open it again.
    let opened = h.entries_calls().len();
    h.key("s");
    assert_eq!(h.entries_calls().len(), opened);
    assert_eq!(h.title(), Some("Item 4"));
}

#[test]
fn the_starred_list_from_history_has_no_history_badge_either() {
    let mut h = Harness::new(some_starred());
    h.starred = Some(3);
    open_starred(&mut h, true);
    assert_eq!(h.app.queue().iter().map(|e| e.id).collect::<Vec<_>>(), [5, 4, 2]);
    assert!(h.app.starred_list() && !h.app.history_badge());
    // h goes to the other picker, as from search results.
    h.key("h");
    assert!(h.picker_loaded());
    assert_eq!(h.app.picker_mode(), Mode::Unread);
}

#[test]
fn unstarring_in_the_starred_list_keeps_the_item_there() {
    let mut h = Harness::new(some_starred());
    h.starred = Some(3);
    open_starred(&mut h, false);
    h.key("*");
    assert_eq!(h.toast(), Some("Unstarred"));
    assert!(!h.app.starred());
    assert_eq!(h.title(), Some("Item 5"));
    h.key("n");
    h.key("k");
    assert_eq!(h.title(), Some("Item 5"), "still in the list");
    assert!(!h.app.starred());
    h.key("*");
    assert!(h.app.starred());
    assert_eq!(star_calls(&h), [(5, false), (5, true)]);
}

#[test]
fn the_end_of_the_starred_list_goes_back_to_the_feeds_and_h_to_history() {
    let mut h = Harness::new(some_starred());
    h.counts = three_feeds(3);
    h.starred = Some(3);
    open_starred(&mut h, false);
    let counts_before = h.calls.iter().filter(|c| matches!(c, Effect::FeedCounts)).count();
    h.press_through(&["j"]);
    assert!(h.caught_up());
    assert_eq!(h.app.end_title(), "End of starred items");
    assert_eq!(h.app.end_hint(), "Press j or Enter for the feeds, k to go back.");
    assert_eq!(h.app.position(), "3 / 3");
    // No looking for new arrivals, as at the end of search results.
    assert!(!h.calls.iter().any(|c| matches!(c, Effect::LoadPage { arrivals: true, .. })));
    assert_eq!(h.calls.iter().filter(|c| matches!(c, Effect::FeedCounts)).count(), counts_before);
    h.key("j");
    assert!(h.picker_loaded());
    assert_eq!(h.app.picker_mode(), Mode::Unread);
    h.key("Escape");
    h.key("h");
    assert!(h.picker_loaded());
    assert_eq!(h.app.picker_mode(), Mode::History);
}

#[test]
fn the_starred_list_loads_a_page_at_a_time_by_time() {
    let entries: Vec<Entry> = items(PAGE as i64 + 5).into_iter().map(|e| Entry { starred: true, ..e }).collect();
    let mut h = Harness::new(entries);
    h.starred = Some(PAGE as i64 + 5);
    open_starred(&mut h, false);
    assert!(h.app.has_more());
    assert_eq!(h.app.position(), format!("1 / {}", PAGE + 5));
    let ids = h.press_through(&["n"]);
    assert_eq!(ids, (1..=PAGE as i64 + 5).rev().collect::<Vec<_>>(), "every one, newest first, once");
    assert!(matches!(
        h.calls.iter().rfind(|c| matches!(c, Effect::LoadPage { .. })),
        Some(Effect::LoadPage { selection: Selection::Starred, arrivals: false, .. })
    ));
}

#[test]
fn shift_a_on_the_starred_list_or_its_row_says_it_cant() {
    let mut h = Harness::new(some_starred());
    h.starred = Some(3);
    open_starred(&mut h, false);
    h.shift("A");
    assert_eq!(h.toast(), Some("Starred items can't be marked read all at once"));
    assert_eq!(h.app.overlay(), Overlay::None);
    h.key("p");
    assert_eq!(h.app.picker_items()[h.app.picker_cursor()].selection, Selection::Starred);
    h.shift("A");
    assert_eq!(h.toast(), Some("Starred items can't be marked read all at once"));
    assert_eq!(h.app.overlay(), Overlay::Picker);
    // Also in the history picker, where other rows say they're read already.
    h.key("h");
    h.key("j");
    h.shift("A");
    assert_eq!(h.toast(), Some("Starred items can't be marked read all at once"));
    assert!(!h.calls.iter().any(|c| matches!(
        c,
        Effect::CountUnread { .. } | Effect::CountRow { .. } | Effect::MarkAllRead { .. } | Effect::MarkRowRead { .. }
    )));
}

#[test]
fn all_unread_marked_read_from_the_picker_shows_read_in_the_starred_list_behind_it() {
    let mut h = Harness::new(some_starred());
    h.starred = Some(3);
    open_starred(&mut h, false);
    h.key("p");
    let row = h.app.picker_items().iter().position(|i| i.selection == Selection::All).unwrap();
    while h.app.picker_cursor() != row {
        h.key("j");
    }
    h.shift("A");
    h.key("y");
    assert!(h.app.queue().iter().all(|e| e.status == EntryStatus::Read));
}
