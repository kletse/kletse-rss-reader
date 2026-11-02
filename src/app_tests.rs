//! The app driven by key presses against a fake Miniflux, as the UI tests did in the webview days.
//! Effects run right away unless a test holds one back to answer it later.

#![allow(clippy::unwrap_used)]

use super::*;
use crate::dates;
use crate::miniflux::{Category, Feed};
use crate::paging::PAGE;
use crate::paging::tests::{entry, entry_at};
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
    /// in a different order on every request, as Postgres may return them.
    fn page(&mut self, status: EntryStatus, past: i64, limit: usize) -> EntryPage {
        self.requests += 1;
        let salt = self.requests;
        let dir = if status == EntryStatus::Unread { 1 } else { -1 };
        let mut list: Vec<Entry> = self
            .entries
            .iter()
            .filter(|e| e.status == status && (past == 0 || dir * (seconds(e) - past) > 0))
            .cloned()
            .collect();
        list.sort_by_key(|e| (dir * seconds(e), (e.id * salt) % 17, e.id));
        EntryPage { total: i64::try_from(list.len()).unwrap(), entries: list.into_iter().take(limit).collect() }
    }

    fn mark(&mut self, ids: &[i64], status: EntryStatus) {
        for e in self.entries.iter_mut().filter(|e| ids.contains(&e.id)) {
            e.status = status;
        }
    }

    fn unread_up_to(&self, up_to: i64) -> Vec<i64> {
        self.entries.iter().filter(|e| e.status == EntryStatus::Unread && e.id <= up_to).map(|e| e.id).collect()
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
    /// Every server effect, in the order they ran.
    calls: Vec<Effect>,
    hold_next_page: bool,
    hold_fetch: bool,
    held: Option<Effect>,
    fail_next_page: Option<String>,
    fail_next_status: Option<String>,
    full_content: String,
    problem: Option<String>,
    save_result: Result<Status, String>,
}

fn counts(unread: i64, read: i64) -> Vec<FeedCount> {
    vec![FeedCount {
        feed: Feed { id: 1, title: "Feed".into(), category: Category { id: 1, title: "Dev".into() } },
        unread,
        read,
    }]
}

impl Harness {
    fn new(entries: Vec<Entry>) -> Self {
        Self {
            app: App::new(Prefs::default(), false),
            fake: Fake { entries, requests: 0 },
            status: Status { configured: true, readeck: false },
            counts: counts(2, 3),
            calls: Vec::new(),
            hold_next_page: false,
            hold_fetch: false,
            held: None,
            fail_next_page: None,
            fail_next_status: None,
            full_content: "<p>Full text</p>".into(),
            problem: None,
            save_result: Ok(Status { configured: true, readeck: false }),
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
        let server = !matches!(effect, Effect::SavePrefs(_) | Effect::ScrollPage { .. } | Effect::Quit);
        if server {
            self.calls.push(effect.clone());
        }
        match effect {
            Effect::OpenSelection { load, mode, request, .. } => {
                let page = self.fake.page(mode.status(), request.past, request.limit);
                let newest =
                    if mode == Mode::Unread { self.fake.unread_ids().into_iter().max().unwrap_or(0) } else { 0 };
                Some(Outcome::Opened { load, result: Ok((page, newest)) })
            }
            Effect::LoadPage { load, mode, request, arrivals, .. } => {
                if std::mem::take(&mut self.hold_next_page) {
                    self.held = Some(effect);
                    return None;
                }
                let result = match self.fail_next_page.take() {
                    Some(message) => Err(message),
                    None => Ok(self.fake.page(mode.status(), request.past, request.limit)),
                };
                Some(Outcome::Page { load, arrivals, result })
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
            Effect::FetchContent { index, id } => {
                if self.hold_fetch {
                    self.held = Some(effect);
                    return None;
                }
                Some(Outcome::ContentFetched { index, id, result: Ok(self.full_content.clone()) })
            }
            Effect::FeedCounts => Some(Outcome::FeedCounts(Ok(self.counts.clone()))),
            Effect::RefreshFeeds => Some(Outcome::Refreshed(Ok(()))),
            Effect::ReloadPickerAfter { .. } | Effect::SavePrefs(_) | Effect::ScrollPage { .. } | Effect::Quit => None,
            Effect::SaveToReadeck { .. } => Some(Outcome::SavedToReadeck(Ok(()))),
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
        self.calls.iter().filter(|c| matches!(c, Effect::OpenSelection { .. } | Effect::LoadPage { .. })).collect()
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
    assert_eq!(page_request(h.entries_calls()[0]), Some(PageRequest { past: 0, limit: PAGE }));
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
    assert_eq!(page_request(h.entries_calls().last().unwrap()), Some(PageRequest { past, limit: PAGE + 1 }));
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
    assert!(h.entries_calls().iter().any(|c| page_request(c) == Some(PageRequest { past, limit: PAGE + 1 })));
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
    assert_eq!(h.app.window_title(), "First — Kletse RSS");
    h.key("Escape");
    assert_eq!(h.app.window_title(), "Kletse RSS");
    h.key("Escape");
    assert_eq!(h.app.window_title(), "First — Kletse RSS");
    h.key("n");
    assert_eq!(h.app.window_title(), "Second — Kletse RSS");
    h.key("n");
    assert!(h.caught_up());
    assert_eq!(h.app.window_title(), "Kletse RSS");
}

#[test]
fn an_untitled_article_is_untitled_in_the_window_title_too() {
    let mut h = Harness::new(vec![entry(1, "")]);
    h.open(false);
    assert_eq!(h.app.window_title(), "(untitled) — Kletse RSS");
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
    assert!(h.calls.contains(&Effect::SaveToReadeck { url: "https://example.com/1".into() }));
    assert_eq!(h.title(), Some("Second"));
    assert_eq!(h.status_calls(), [(vec![1], EntryStatus::Read)]);
}

#[test]
fn font_size_and_width_keys_change_and_save_the_prefs() {
    let mut h = first_second();
    let ctrl = |key: &str| KeyPress { key: key.into(), ctrl: true, ..KeyPress::default() };
    let (_, fx) = h.app.key(&ctrl("="));
    assert_eq!(fx, [Effect::SavePrefs(Prefs { font_size: 19, width_factor: 40 })]);
    let (_, fx) = h.app.key(&KeyPress { key: "[".into(), ..KeyPress::default() });
    assert_eq!(fx, [Effect::SavePrefs(Prefs { font_size: 19, width_factor: 36 })]);
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
        Effect::OpenSelection { mode: Mode::History, request: PageRequest { past: 0, limit: PAGE }, .. }
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
    h.status = Status { configured: false, readeck: false };
    h.save_result = Ok(Status { configured: true, readeck: true });
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
    let problem = "Couldn't use /home/me/.config/kletse-rss/config.toml. Line 3: string values must be quoted.";
    let mut h = Harness::new(items(2));
    h.status = Status { configured: false, readeck: false };
    h.problem = Some(problem.into());
    h.start();
    assert_eq!(h.app.settings().unwrap().problem.as_deref(), Some(problem));
}

#[test]
fn onboarding_shows_no_notice_when_the_config_file_is_fine() {
    let mut h = Harness::new(items(2));
    h.status = Status { configured: false, readeck: false };
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
