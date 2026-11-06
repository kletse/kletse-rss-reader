//! The app driven by key presses against a fake Miniflux, as the UI tests did in the webview days.
//! Effects run right away unless a test holds one back to answer it later.

#![allow(clippy::unwrap_used)]

use super::*;
use crate::dates;
use crate::feeds::{Applied, Feed, FeedCount, FeedOps};
use crate::keys::Action;
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
    /// What the last save was sent.
    saved_values: Option<SettingsValues>,
    /// Probes and logins, left unanswered until a test says what comes back.
    looked_at: Vec<(u64, Address)>,
    logins: Vec<Login>,
    /// What the server lists when it's asked for its feeds: changes are answered by hand, with `changed`.
    list: FeedList,
    /// What the stores answer, one per time they're asked.
    secrets_answer: VecDeque<(Status, Option<String>)>,
}

fn counts(unread: i64, read: i64) -> Vec<FeedCount> {
    vec![FeedCount {
        feed: Feed { id: 1, title: "Feed".into(), category: Category { id: 1, title: "Dev".into() } },
        unread: Some(unread),
        read: Some(read),
    }]
}

fn saved(status: Status) -> Saved {
    Saved { status, username: None, readeck_problem: None, place: None }
}

impl Harness {
    fn new(entries: Vec<Entry>) -> Self {
        Self {
            app: App::new(Prefs::default(), false),
            fake: Fake { entries, requests: 0 },
            status: Status { looking: false, configured: true, readeck: false, capabilities: Capabilities::EVERYTHING },
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
            save_result: Ok(saved(Status {
                looking: false,
                configured: true,
                readeck: false,
                capabilities: Capabilities::EVERYTHING,
            })),
            saved_values: None,
            looked_at: Vec::new(),
            logins: Vec::new(),
            list: FeedList::default(),
            secrets_answer: VecDeque::new(),
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
            Effect::ReadSecrets => {
                let (status, problem) = self.secrets_answer.pop_front().unwrap();
                Some(Outcome::SecretsRead { status, problem })
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
            Effect::SaveSettings(values) => {
                self.saved_values = Some(values.0);
                Some(Outcome::SettingsSaved(self.save_result.clone()))
            }
            // Answered by hand, as a test needs them in flight: see `found` and `minted`.
            Effect::Probe { probe, address } => {
                self.looked_at.push((probe, address));
                None
            }
            Effect::MintKey(login) => {
                self.logins.push(login);
                None
            }
            // Answered by hand, as a test needs them in flight: see `imported` and `exported`.
            Effect::ChooseFeedFile(_) | Effect::ImportFeeds { .. } | Effect::ExportFeeds { .. } => None,
            Effect::LoadFeedList { serial } => Some(Outcome::FeedList { serial, result: Ok(self.list.clone()) }),
            Effect::CountCategory { job, category } => {
                Some(Outcome::CategoryCounted { job, category, result: Ok(self.list.clone()) })
            }
            // Answered by hand: see `changed`.
            Effect::ChangeFeeds { .. } => None,
            // Run by `Connecting`, against a real Backend.
            Effect::AskReadeck { .. }
            | Effect::AwaitReadeck { .. }
            | Effect::PollReadeck { .. }
            | Effect::KeepReadeck { .. } => None,
            Effect::UseSamples(on) => {
                let status =
                    Status { looking: false, configured: on, readeck: on, capabilities: Capabilities::EVERYTHING };
                Some(Outcome::SamplesSwitched { on, status })
            }
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
        then: AfterOpen::MarkRead { index: 0, id: 1 }
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

impl Harness {
    /// From the first screen to the address box.
    fn go_to_connect(&mut self) {
        self.key("ArrowDown");
        self.key("Enter");
        assert_eq!(self.app.step(), Step::Connect);
    }

    /// From the address box to the whole form.
    fn by_hand(&mut self) {
        self.row(providers::BY_HAND_ROW);
        assert_eq!(self.app.connect_view().stage, "by_hand");
    }

    /// Connect's row with this label.
    fn row(&mut self, label: &str) {
        let rows = self.app.connect_view().rows;
        let row = rows.iter().position(|r| r.label == label).unwrap_or_else(|| panic!("{label} in {rows:?}"));
        let fx = self.app.choose_onboarding(row);
        self.run(fx);
    }

    fn submit(&mut self, values: SettingsValues) {
        let fx = self.app.submit_settings(values);
        self.run(fx);
    }

    /// Enter in the address box.
    fn look_at(&mut self, address: &str) -> u64 {
        self.app.address_edited(address);
        self.submit(SettingsValues { address: address.into(), ..SettingsValues::default() });
        self.looked_at.last().unwrap().0
    }

    /// The answer to a probe.
    fn found(&mut self, probe: u64, found: Detected) {
        let fx = self.app.apply(Outcome::Probed { probe, found });
        self.run(fx);
    }

    fn connect_to(&mut self, found: Detected) {
        self.go_to_connect();
        let probe = self.look_at("rss.example.com");
        self.found(probe, found);
    }

    fn minted(&mut self, result: Result<String, MintFailed>) {
        let probe = self.logins.last().unwrap().probe;
        let fx = self.app.apply(Outcome::KeyMinted { probe, result });
        self.run(fx);
    }

    fn form_error(&self) -> &str {
        &self.app.settings().unwrap().error
    }

    fn field_keys(&self) -> Vec<FieldKey> {
        self.app.connect_view().fields.iter().map(|f| f.key).collect()
    }

    fn opened(&self) -> Vec<&str> {
        let opened = self.calls.iter().filter_map(|c| match c {
            Effect::OpenInBrowser { url, .. } => Some(url.as_str()),
            _ => None,
        });
        opened.collect()
    }

    /// All the words Connect has on screen.
    fn connect_words(&self) -> String {
        let view = self.app.connect_view();
        let fields = view.fields.iter().flat_map(|f| [f.label, f.help, f.placeholder]);
        let rows = view.rows.iter().map(|r| r.label.as_str());
        let lines = [view.text.as_str(), view.note.as_str(), view.action, self.form_error()];
        fields.chain(rows).chain(lines).collect::<Vec<_>>().join(" ").to_lowercase()
    }
}

const BASE: &str = "https://rss.example.com";

fn miniflux_found() -> Detected {
    Detected::Miniflux { base: BASE.into(), sign_in: true }
}

fn freshrss_found(state: Api) -> Detected {
    Detected::FreshRss { web: format!("{BASE}/i/"), api: format!("{BASE}/api/greader.php"), state }
}

fn signing_in() -> SettingsValues {
    SettingsValues {
        address: "rss.example.com".into(),
        miniflux_username: "ward".into(),
        miniflux_password: "hunter2".into(),
        ..SettingsValues::default()
    }
}

/// A first run, on the first screen.
fn first_run() -> Harness {
    let mut h = Harness::new(items(2));
    h.status = Status { looking: false, configured: false, readeck: false, capabilities: Capabilities::EVERYTHING };
    h.start();
    assert_eq!(h.app.overlay(), Overlay::Onboarding);
    assert_eq!(h.app.step(), Step::Welcome);
    h
}

/// A first run, reading the first sample article.
fn in_the_samples() -> Harness {
    let mut h = first_run();
    h.key("Enter");
    h.key("Enter");
    h
}

#[test]
fn a_sample_article_is_two_key_presses_away() {
    let h = in_the_samples();
    assert_eq!(h.app.overlay(), Overlay::None);
    assert!(h.app.samples());
    assert!(h.title().is_some(), "an article is open");
}

#[test]
fn a_sample_article_is_two_clicks_away() {
    let mut h = first_run();
    let fx = h.app.choose_onboarding(0);
    h.run(fx);
    assert!(h.picker_loaded());
    let fx = h.app.choose_row(0);
    h.run(fx);
    assert!(h.title().is_some());
}

#[test]
fn nothing_is_asked_of_the_backend_before_it_has_switched() {
    let mut h = first_run();
    let (used, fx) = h.app.key(&KeyPress { key: "Enter".into(), ..KeyPress::default() });
    assert!(used);
    assert_eq!(fx, [Effect::UseSamples(true)], "the feeds are asked for once its answer is in");
    assert_eq!(h.app.overlay(), Overlay::Onboarding);
}

#[test]
fn the_first_screen_offers_three_things_and_the_arrows_go_round_them() {
    let mut h = first_run();
    assert_eq!(h.app.step_cursor(), 0);
    h.key("ArrowDown");
    h.key("j");
    assert_eq!(h.app.step_cursor(), 2);
    h.key("ArrowDown");
    assert_eq!(h.app.step_cursor(), 0);
    h.key("ArrowUp");
    assert_eq!(h.app.step_cursor(), 2);
    h.key("Enter");
    assert_eq!(h.app.step(), Step::NoServer);
}

#[test]
fn esc_steps_back_to_the_row_you_left_and_no_further() {
    let mut h = first_run();
    h.key("ArrowDown");
    h.key("Enter");
    assert_eq!(h.app.step(), Step::Connect);
    h.key("Escape");
    assert_eq!((h.app.step(), h.app.step_cursor()), (Step::Welcome, 1));
    h.key("ArrowDown");
    h.key("Enter");
    assert_eq!(h.app.step(), Step::NoServer);
    h.key("Escape");
    assert_eq!((h.app.step(), h.app.step_cursor()), (Step::Welcome, 2));
    let (used, fx) = h.app.key(&KeyPress { key: "Escape".into(), ..KeyPress::default() });
    assert!(!used && fx.is_empty());
    assert_eq!(h.app.overlay(), Overlay::Onboarding, "there's nothing behind the first screen");
    assert!(h.app.cancel_settings().is_empty());
    assert_eq!(h.app.overlay(), Overlay::Onboarding);
}

#[test]
fn esc_steps_back_from_a_field_and_the_rest_of_the_keys_are_the_fields() {
    let mut h = first_run();
    let fx = h.app.choose_onboarding(1);
    h.run(fx);
    for key in ["j", "Enter", "ArrowDown"] {
        let (used, fx) = h.app.key(&KeyPress { key: key.into(), typing: true, ..KeyPress::default() });
        assert!(!used && fx.is_empty(), "{key}");
    }
    assert_eq!(h.app.step(), Step::Connect);
    let (used, _) = h.app.key(&KeyPress { key: "Escape".into(), typing: true, ..KeyPress::default() });
    assert!(used);
    assert_eq!(h.app.step(), Step::Welcome);
}

#[test]
fn a_modifier_goes_on_to_the_system() {
    let mut h = first_run();
    let (used, _) = h.app.key(&KeyPress { key: "Enter".into(), ctrl: true, ..KeyPress::default() });
    assert!(!used);
    assert_eq!(h.app.step(), Step::Welcome);
}

#[test]
fn the_macs_menus_do_nothing_on_first_run() {
    let mut h = first_run();
    for action in [Action::OpenPicker, Action::Settings, Action::Help, Action::Next { mark_read: true }] {
        assert!(h.app.action(action).is_empty(), "{action:?}");
        assert_eq!(h.app.overlay(), Overlay::Onboarding, "{action:?}");
    }
}

#[test]
fn connect_saves_and_goes_on_to_the_picker() {
    let mut h = first_run();
    h.go_to_connect();
    h.by_hand();
    h.save_result =
        Ok(saved(Status { looking: false, configured: true, readeck: true, capabilities: Capabilities::EVERYTHING }));
    let fx = h.app.submit_settings(SettingsValues::default());
    assert!(h.app.settings().unwrap().checking);
    h.run(fx);
    assert!(h.picker_loaded());
    assert!(h.app.readeck());
    assert!(!h.app.samples());
}

#[test]
fn a_failed_connect_stays_put_and_says_why() {
    let mut h = first_run();
    h.go_to_connect();
    h.by_hand();
    h.save_result = Err("Miniflux rejected the API key".into());
    let typed = SettingsValues { miniflux_url: "https://m.example.com".into(), ..SettingsValues::default() };
    let fx = h.app.submit_settings(typed.clone());
    h.run(fx);
    assert_eq!((h.app.overlay(), h.app.step()), (Overlay::Onboarding, Step::Connect));
    let form = h.app.settings().unwrap();
    assert_eq!(form.error, "Miniflux rejected the API key");
    assert_eq!(form.values, typed);
    assert!(!form.checking);
}

#[test]
fn esc_waits_for_a_save_being_checked() {
    let mut h = first_run();
    h.go_to_connect();
    h.by_hand();
    let _still_going = h.app.submit_settings(SettingsValues::default());
    let (used, _) = h.app.key(&KeyPress { key: "Escape".into(), ..KeyPress::default() });
    assert!(used);
    assert_eq!(h.app.step(), Step::Connect);
}

#[test]
fn a_form_is_handed_in_by_field_name_and_keeps_what_isnt_on_screen() {
    let mut h = first_run();
    h.go_to_connect();
    h.by_hand();
    let fields = [("greader_url", "https://f.example.com"), ("readeck_token", "tok"), ("nonsense", "x")];
    let fields = fields.into_iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
    let fx = h.app.submit_fields("greader", &fields);
    let [Effect::SaveSettings(values)] = fx.as_slice() else { panic!("{fx:?}") };
    assert_eq!(values.0.server, Server::GReader);
    assert_eq!(values.0.greader_url, "https://f.example.com");
    assert_eq!(values.0.readeck_token, "tok", "Readeck isn't on this screen, and stays what it was");
}

#[test]
fn a_form_naming_an_unknown_kind_of_server_says_so() {
    let mut h = first_run();
    assert!(h.app.submit_fields("feedbin", &HashMap::new()).is_empty());
    assert!(h.app.settings().unwrap().error.contains("feedbin"));
}

#[test]
fn the_no_server_links_open_and_leave_their_address_to_read() {
    let mut h = first_run();
    let fx = h.app.choose_onboarding(2);
    h.run(fx);
    assert_eq!(h.app.shown_link(), None);
    h.key("ArrowDown");
    h.key("Enter");
    let url = providers::NO_SERVER.offers[1].link.url;
    assert_eq!(h.app.shown_link(), Some(url), "whatever xdg-open says");
    assert!(h.calls.contains(&Effect::OpenInBrowser { url: url.into(), then: AfterOpen::Nothing }));
    h.key("Escape");
    assert_eq!(h.app.shown_link(), None);
}

#[test]
fn an_articles_links_leave_nothing_to_read() {
    let mut h = first_second();
    let fx = h.app.open_link("https://example.com/a");
    h.run(fx);
    assert_eq!(h.app.shown_link(), None);
}

#[test]
fn the_samples_write_no_prefs_and_leave_none_behind() {
    let mut h = in_the_samples();
    let before = h.app.prefs();
    let (_, fx) = h.app.key(&KeyPress { key: "]".into(), ..KeyPress::default() });
    assert!(h.app.prefs() != before, "it still works for the look of it");
    assert!(!fx.iter().any(|e| matches!(e, Effect::SavePrefs(_))));
    h.key(",");
    assert_eq!(h.app.prefs(), before);
    let (_, fx) = h.app.key(&KeyPress { key: "]".into(), ..KeyPress::default() });
    assert!(fx.is_empty(), "a form has the keyboard");
    h.key("Escape");
    let fx = h.app.set_colours(Colours::OnwardDark);
    assert!(matches!(fx.as_slice(), [Effect::SavePrefs(_)]), "out of the samples, prefs are written again");
}

#[test]
fn connect_from_the_samples_leaves_nothing_of_them() {
    let mut h = in_the_samples();
    assert!(h.app.readeck());
    h.key("/");
    let fx = h.app.search("watch");
    h.run(fx);
    h.key(",");
    assert_eq!((h.app.overlay(), h.app.step()), (Overlay::Onboarding, Step::Connect));
    assert!(!h.app.samples());
    assert!(h.calls.contains(&Effect::UseSamples(false)));
    assert!(h.app.settings().is_some(), "the form, loaded afresh");
    assert!(h.app.queue().is_empty());
    assert_eq!(h.app.selection_label(), "");
    assert_eq!(h.app.search_query(), "");
    assert!(!h.app.readeck());
    assert_eq!(h.app.position(), "");
    assert_eq!(h.app.picker_list(), &PickerList::Loading);
}

#[test]
fn an_answer_on_its_way_when_the_samples_are_left_changes_nothing() {
    let mut h = in_the_samples();
    h.fake.entries = items(i64::try_from(2 * PAGE).unwrap());
    h.key("p");
    let fx = h.app.choose_row(0);
    h.run(fx);
    h.hold_next_page = true;
    while h.held.is_none() {
        h.key("j");
    }
    h.key(",");
    h.answer();
    assert!(h.app.queue().is_empty());
    assert_eq!(h.app.overlay(), Overlay::Onboarding);
}

#[test]
fn the_samples_can_be_tried_again_after_stepping_back() {
    let mut h = in_the_samples();
    h.key(",");
    h.key("Escape");
    assert_eq!(h.app.step(), Step::Welcome);
    assert_eq!(h.app.step_cursor(), 1);
    h.key("ArrowUp");
    h.key("Enter");
    h.key("Enter");
    assert!(h.app.samples() && h.title().is_some());
}

#[test]
fn in_the_samples_the_help_says_connect_where_it_said_settings() {
    let h = in_the_samples();
    assert!(h.app.help_lines().iter().any(|(keys, action)| keys == "," && action == "connect"));
    let h = first_second();
    assert!(h.app.help_lines().iter().any(|(keys, action)| keys == "," && action == "settings"));
}

#[test]
fn a_server_set_up_from_the_start_is_no_sample() {
    let mut h = first_second();
    assert!(!h.app.samples());
    h.key(",");
    assert_eq!(h.app.overlay(), Overlay::Settings, "ONWARD_DEMO comes this way too");
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
    let status = Status { looking: false, configured: true, readeck: true, capabilities: Capabilities::EVERYTHING };
    h.save_result =
        Ok(Saved { status, username: None, readeck_problem: Some("Readeck rejected the token".into()), place: None });
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
    let status = Status { looking: false, configured: true, readeck: false, capabilities: Capabilities::EVERYTHING };

    let mut h = first_second();
    h.app.use_settings_window();
    h.key(",");
    h.save_result = Ok(Saved { status, username: None, readeck_problem: problem.clone(), place: None });
    let fx = h.app.submit_settings(SettingsValues::default());
    h.run(fx);
    assert!(h.app.settings().is_none());
    assert_eq!(h.toast(), Some("Settings saved. Readeck wasn't changed: Readeck rejected the token"));

    let mut h = Harness::new(items(2));
    h.status = Status { looking: false, configured: false, readeck: false, capabilities: Capabilities::EVERYTHING };
    h.save_result = Ok(Saved { status, username: None, readeck_problem: problem, place: None });
    h.start();
    h.go_to_connect();
    h.by_hand();
    let fx = h.app.submit_settings(SettingsValues::default());
    h.run(fx);
    assert!(h.picker_loaded());
    assert_eq!(h.toast(), Some("Settings saved. Readeck wasn't changed: Readeck rejected the token"));
}

#[test]
fn saving_says_who_you_are_signed_in_as() {
    let status = Status { looking: false, configured: true, readeck: false, capabilities: Capabilities::EVERYTHING };
    let mut h = first_second();
    h.key(",");
    h.save_result = Ok(Saved { status, username: Some("ward".into()), readeck_problem: None, place: None });
    let fx = h.app.submit_settings(SettingsValues::default());
    h.run(fx);
    assert_eq!(h.toast(), Some("Signed in as ward"));
    assert_eq!(h.app.toast().map(|t| t.kind), Some(ToastKind::Info));

    // On first run too, where a plain "Settings saved" says nothing new.
    let mut h = Harness::new(items(2));
    h.status = Status { looking: false, configured: false, readeck: false, capabilities: Capabilities::EVERYTHING };
    h.save_result = Ok(Saved { status, username: Some("ward".into()), readeck_problem: None, place: None });
    h.start();
    h.go_to_connect();
    h.by_hand();
    let fx = h.app.submit_settings(SettingsValues::default());
    h.run(fx);
    assert_eq!(h.toast(), Some("Signed in as ward"));
}

#[test]
fn a_nameless_sign_in_shows_no_empty_name() {
    let status = Status { looking: false, configured: true, readeck: false, capabilities: Capabilities::EVERYTHING };
    for username in [None, Some(String::new()), Some("  ".to_string())] {
        let mut h = first_second();
        h.key(",");
        h.save_result = Ok(Saved { status, username: username.clone(), readeck_problem: None, place: None });
        let fx = h.app.submit_settings(SettingsValues::default());
        h.run(fx);
        assert_eq!(h.toast(), Some("Settings saved"), "{username:?}");

        let mut h = Harness::new(items(2));
        h.status = Status { looking: false, configured: false, readeck: false, capabilities: Capabilities::EVERYTHING };
        h.save_result = Ok(Saved { status, username, readeck_problem: None, place: None });
        h.start();
        let fx = h.app.submit_settings(SettingsValues::default());
        h.run(fx);
        assert_eq!(h.toast(), None, "the first run says nothing");
    }
}

#[test]
fn the_name_and_a_readeck_problem_are_both_in_the_toast() {
    let status = Status { looking: false, configured: true, readeck: false, capabilities: Capabilities::EVERYTHING };
    let mut h = first_second();
    h.key(",");
    let readeck_problem = Some("Readeck rejected the token".to_string());
    h.save_result = Ok(Saved { status, username: Some("ward".into()), readeck_problem, place: None });
    let fx = h.app.submit_settings(SettingsValues::default());
    h.run(fx);
    assert_eq!(h.toast(), Some("Signed in as ward. Readeck wasn't changed: Readeck rejected the token"));
    assert_eq!(h.app.toast().map(|t| t.kind), Some(ToastKind::Error));
}

#[test]
fn a_settings_window_doesnt_replace_onboarding() {
    let mut h = Harness::new(items(2));
    h.app.use_settings_window();
    h.status = Status { looking: false, configured: false, readeck: false, capabilities: Capabilities::EVERYTHING };
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
    h.status = Status { looking: false, configured: false, readeck: false, capabilities: Capabilities::EVERYTHING };
    h.problem = Some(problem.into());
    h.start();
    assert_eq!(h.app.step(), Step::Welcome);
    assert_eq!(h.app.settings().unwrap().problem.as_deref(), Some(problem));
    h.key("Enter");
    h.key(",");
    assert_eq!(h.app.step(), Step::Connect);
    assert_eq!(h.app.settings().unwrap().problem.as_deref(), Some(problem), "the samples don't lose it");
}

#[test]
fn onboarding_shows_no_notice_when_the_config_file_is_fine() {
    let mut h = Harness::new(items(2));
    h.status = Status { looking: false, configured: false, readeck: false, capabilities: Capabilities::EVERYTHING };
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
fn a_kind_of_server_the_core_doesnt_know_stops_the_save_and_the_form_says_so() {
    let mut h = Harness::new(items(1));
    h.start();
    h.key(",");
    assert_eq!(h.app.server_named("greader"), Some(Server::GReader));
    assert_eq!(h.app.settings().unwrap().error, "");
    assert_eq!(h.app.server_named("feedbin"), None);
    assert!(h.app.settings().unwrap().error.contains("feedbin"));
    assert!(!h.app.settings().unwrap().checking);
}

#[test]
fn on_a_server_that_doesnt_fetch_f_and_r_only_say_so() {
    let mut h = Harness::new(items(3));
    h.status =
        Status { looking: false, configured: true, readeck: false, capabilities: Server::GReader.capabilities() };
    h.open(false);
    assert!(!h.app.capabilities().fetch_full);
    h.key("f");
    assert_eq!(h.toast(), Some(crate::feeds::FETCH_CONTENT_REFUSED));
    h.key("p");
    h.key("r");
    assert_eq!(h.toast(), Some(crate::feeds::REFRESH_REFUSED));
    assert!(!h.calls.iter().any(|c| matches!(c, Effect::FetchContent { .. } | Effect::RefreshFeeds)));
    let listed = |h: &Harness, key: &str| h.app.help_lines().iter().any(|(keys, _)| keys == key);
    assert!(!listed(&h, "f") && !listed(&h, "r"), "the help leaves them out");
    assert!(listed(&h, "/"), "whether it has search only shows by trying");
    // Saved settings for a Miniflux server bring them back.
    h.save_result =
        Ok(saved(Status { looking: false, configured: true, readeck: false, capabilities: Capabilities::EVERYTHING }));
    h.key(",");
    let fx = h.app.submit_settings(SettingsValues::default());
    h.run(fx);
    assert!(h.app.capabilities().fetch_full);
    assert!(listed(&h, "f") && listed(&h, "r"));
}

#[test]
fn slash_says_so_at_once_only_on_a_server_known_to_have_no_search() {
    let mut h = Harness::new(items(3));
    h.status.capabilities.search = None;
    h.open(false);
    h.key("/");
    assert_eq!(h.app.overlay(), Overlay::Search, "only trying will tell");
    h.key("Escape");

    let mut h = Harness::new(items(3));
    h.status.capabilities.search = Some(false);
    h.open(false);
    h.key("/");
    assert_eq!(h.app.overlay(), Overlay::None);
    assert_eq!(h.toast(), Some(crate::feeds::NO_SEARCH));
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

// ---- Connect ------------------------------------------------------------------------------------------------

#[test]
fn connect_opens_on_the_address_box_alone() {
    let mut h = first_run();
    h.go_to_connect();
    let view = h.app.connect_view();
    assert_eq!((view.stage, view.action, view.probing), ("address", "Continue", false));
    assert!(view.fields.is_empty());
    assert!(!h.connect_words().contains("api key"));
}

#[test]
fn typing_looks_at_nothing_and_enter_looks_once() {
    let mut h = first_run();
    h.go_to_connect();
    for typed in ["r", "rs", "rss", "rss.example.com"] {
        h.app.address_edited(typed);
    }
    assert!(h.looked_at.is_empty());
    let values = SettingsValues { address: "rss.example.com".into(), ..SettingsValues::default() };
    h.submit(values.clone());
    assert_eq!(h.looked_at.len(), 1);
    assert!(h.app.connect_view().probing);
    h.submit(values);
    assert_eq!(h.looked_at.len(), 1, "a second Enter while it looks changes nothing");
}

#[test]
fn an_answer_about_an_address_since_changed_is_a_no_op() {
    let mut h = first_run();
    h.go_to_connect();
    let stale = h.look_at("old.example.com");
    h.app.address_edited("new.example.com");
    assert!(!h.app.connect_view().probing);
    h.found(stale, miniflux_found());
    assert!(h.app.connect_view().fields.is_empty());
    assert_eq!(h.form_error(), "");
    let fresh = h.look_at("new.example.com");
    assert_ne!(fresh, stale);
    h.found(stale, Detected::Samples);
    assert_eq!(h.form_error(), "", "still waiting for the new one");
    h.found(fresh, miniflux_found());
    assert_eq!(h.field_keys(), [FieldKey::MinifluxUsername, FieldKey::MinifluxPassword]);
}

#[test]
fn an_answer_after_stepping_back_is_a_no_op() {
    let mut h = first_run();
    h.go_to_connect();
    let stale = h.look_at("rss.example.com");
    h.key("Escape");
    assert_eq!(h.app.step(), Step::Welcome);
    h.found(stale, miniflux_found());
    h.key("Enter");
    assert_eq!(h.app.step(), Step::Connect);
    assert!(h.app.connect_view().fields.is_empty(), "the address box again, with nothing found");
}

#[test]
fn the_box_echoing_what_it_holds_changes_nothing() {
    let mut h = first_run();
    h.connect_to(miniflux_found());
    h.app.address_edited("rss.example.com");
    assert!(!h.field_keys().is_empty());
}

#[test]
fn changing_the_address_after_a_server_was_found_starts_over() {
    let mut h = first_run();
    h.connect_to(miniflux_found());
    h.app.address_edited("rss.example.org");
    assert!(h.app.connect_view().fields.is_empty());
    assert_eq!(h.app.connect_view().action, "Continue");
}

#[test]
fn an_address_that_isnt_one_is_said_at_once_with_no_request() {
    let mut h = first_run();
    h.go_to_connect();
    h.submit(SettingsValues { address: "  ".into(), ..SettingsValues::default() });
    assert!(h.looked_at.is_empty());
    assert!(h.form_error().contains("Type your server's address"));
}

#[test]
fn miniflux_asks_for_a_username_and_password_and_never_says_api_key() {
    let mut h = first_run();
    h.connect_to(miniflux_found());
    assert_eq!(h.field_keys(), [FieldKey::MinifluxUsername, FieldKey::MinifluxPassword]);
    assert_eq!(h.app.connect_view().action, "Sign in");
    let words = h.connect_words();
    assert!(words.contains("miniflux") && !words.contains("api key"), "{words}");
}

#[test]
fn a_password_becomes_a_key_and_is_never_kept() {
    let mut h = first_run();
    h.connect_to(miniflux_found());
    h.hold_save = false;
    h.submit(signing_in());
    assert_eq!(h.logins.len(), 1);
    let login = h.logins[0].clone();
    assert_eq!((login.base.as_str(), login.username.as_str(), login.password.as_str()), (BASE, "ward", "hunter2"));
    assert!(!format!("{login:?}").contains("hunter2"));
    let form = h.app.settings().unwrap();
    assert!(form.checking);
    assert_eq!(form.values.miniflux_password, "", "gone from the core as soon as it's sent");
    let before = form.refill;

    h.minted(Ok("minted-key".into()));

    let saved = h.saved_values.clone().unwrap();
    assert_eq!(saved.server, Server::Miniflux);
    assert_eq!(saved.miniflux_url, BASE);
    assert_eq!(saved.miniflux_api_key, "minted-key");
    assert_eq!(saved.miniflux_password, "");
    assert!(h.picker_loaded(), "the save went through");
    assert!(h.app.settings().is_none_or(|f| f.refill > before));
}

#[test]
fn a_refused_password_says_every_reason_clears_the_field_and_keeps_the_way_out() {
    let mut h = first_run();
    h.connect_to(miniflux_found());
    h.submit(signing_in());
    let before = h.app.settings().unwrap().refill;
    h.minted(Err(MintFailed::Refused));
    let form = h.app.settings().unwrap();
    assert!(form.error.contains("wrong") && form.error.contains("passkey"), "{}", form.error);
    assert!(!form.checking);
    assert!(form.refill > before, "the window's copy goes too");
    assert_eq!(form.values.miniflux_password, "");
    assert_eq!(form.values.miniflux_username, "ward");
    assert_eq!(h.logins.len(), 1, "no second try by itself");
    assert_eq!(h.field_keys(), [FieldKey::MinifluxUsername, FieldKey::MinifluxPassword]);
    let rows = h.app.connect_view().rows;
    assert_eq!(rows[0].url.as_deref(), Some("https://rss.example.com/keys/create"));
}

#[test]
fn the_way_to_a_key_is_there_before_any_password_fails() {
    let mut h = first_run();
    h.connect_to(miniflux_found());
    h.row(providers::NO_PASSWORD_ROW);
    assert_eq!(h.opened(), ["https://rss.example.com/keys/create"]);
    assert_eq!(h.app.shown_link(), Some("https://rss.example.com/keys/create"));
    assert_eq!(h.field_keys(), [FieldKey::MinifluxApiKey]);
    h.submit(SettingsValues { miniflux_api_key: "pasted".into(), ..signing_in() });
    assert!(h.logins.is_empty());
    let saved = h.saved_values.clone().unwrap();
    assert_eq!((saved.miniflux_url.as_str(), saved.miniflux_api_key.as_str()), (BASE, "pasted"));
    assert_eq!(saved.miniflux_password, "", "what was typed before changing route stays out of it");
}

#[test]
fn someone_who_chose_a_key_can_go_back_to_a_password() {
    let mut h = first_run();
    h.connect_to(miniflux_found());
    h.row(providers::NO_PASSWORD_ROW);
    h.row(providers::USE_PASSWORD_ROW);
    assert_eq!(h.field_keys(), [FieldKey::MinifluxUsername, FieldKey::MinifluxPassword]);
}

#[test]
fn a_miniflux_with_no_key_call_says_nothing_about_the_password_and_asks_for_a_key() {
    let mut h = first_run();
    h.connect_to(miniflux_found());
    h.submit(signing_in());
    h.minted(Err(MintFailed::NoSuchCall));
    assert_eq!(h.field_keys(), [FieldKey::MinifluxApiKey]);
    let view = h.app.connect_view();
    assert!(view.text.contains("2.2.9"), "{}", view.text);
    assert!(!view.text.to_lowercase().contains("password"), "{}", view.text);
    assert_eq!(h.form_error(), "");
    assert_eq!(h.app.settings().unwrap().values.miniflux_password, "");
    assert!(!view.rows.iter().any(|r| r.label == providers::USE_PASSWORD_ROW), "it can't work here");
    assert_eq!(view.rows[0].url.as_deref(), Some("https://rss.example.com/keys/create"));
}

#[test]
fn a_miniflux_without_password_sign_in_goes_straight_to_a_key() {
    let mut h = first_run();
    h.connect_to(Detected::Miniflux { base: BASE.into(), sign_in: false });
    assert_eq!(h.field_keys(), [FieldKey::MinifluxApiKey]);
    assert!(h.connect_words().contains("api key"));
    assert!(!h.app.connect_view().rows.iter().any(|r| r.label == providers::USE_PASSWORD_ROW));
}

#[test]
fn a_key_already_made_isnt_made_again_when_the_save_fails() {
    let mut h = first_run();
    h.connect_to(miniflux_found());
    h.submit(signing_in());
    h.save_result = Err("Could not reach Miniflux".into());
    h.minted(Ok("minted-key".into()));
    assert_eq!(h.form_error(), "Could not reach Miniflux");
    assert_eq!(h.field_keys(), [FieldKey::MinifluxApiKey]);
    let form = h.app.settings().unwrap();
    assert_eq!(form.values.miniflux_api_key, "minted-key");
    assert_eq!(h.logins.len(), 1);
}

#[test]
fn nothing_goes_to_the_server_with_a_field_empty() {
    let mut h = first_run();
    h.connect_to(miniflux_found());
    h.submit(SettingsValues { miniflux_password: String::new(), ..signing_in() });
    assert!(h.logins.is_empty());
    assert_eq!(h.form_error(), "Password is still empty.");
}

#[test]
fn esc_waits_for_a_key_being_made() {
    let mut h = first_run();
    h.connect_to(miniflux_found());
    h.submit(signing_in());
    h.key("Escape");
    assert!(!h.field_keys().is_empty());
    h.app.address_edited("elsewhere");
    assert!(!h.field_keys().is_empty(), "the box is read-only until it's done");
}

#[test]
fn esc_goes_back_to_the_box_and_takes_a_typed_password_out_of_the_window() {
    let mut h = first_run();
    h.connect_to(miniflux_found());
    let before = h.app.settings().unwrap().refill;
    h.key("Escape");
    assert_eq!(h.app.step(), Step::Connect);
    assert!(h.app.connect_view().fields.is_empty());
    let form = h.app.settings().unwrap();
    assert!(form.refill > before);
    assert_eq!(form.values.address, "rss.example.com");
    h.key("Escape");
    assert_eq!(h.app.step(), Step::Welcome);
}

#[test]
fn plain_http_is_said_when_its_used() {
    let mut h = first_run();
    h.connect_to(Detected::Miniflux { base: "http://rss:8080".into(), sign_in: true });
    assert!(h.app.connect_view().note.contains("plain http://"));
    let mut h = first_run();
    h.connect_to(miniflux_found());
    assert_eq!(h.app.connect_view().note, "");
}

#[test]
fn freshrss_asks_for_what_it_needs_and_says_where_that_is() {
    let mut h = first_run();
    h.connect_to(freshrss_found(Api::Ready));
    assert_eq!(h.field_keys(), [FieldKey::GreaderUsername, FieldKey::GreaderPassword]);
    let words = h.connect_words();
    assert!(words.contains("isn't your login password") && words.contains("couple of seconds"), "{words}");
    let view = h.app.connect_view();
    assert_eq!(view.rows[0].url.as_deref(), Some("https://rss.example.com/i/"));
    h.submit(SettingsValues {
        greader_username: "ward".into(),
        greader_password: "api-pass".into(),
        ..SettingsValues::default()
    });
    let saved = h.saved_values.clone().unwrap();
    assert_eq!(saved.server, Server::GReader);
    assert_eq!(saved.greader_url, "https://rss.example.com/api/greader.php");
}

#[test]
fn a_turned_down_freshrss_login_covers_unset_and_wrong_and_says_where_to_go() {
    let mut h = first_run();
    h.connect_to(freshrss_found(Api::Ready));
    h.save_result = Err(greader::LOGIN_REJECTED.into());
    h.submit(SettingsValues {
        greader_username: "ward".into(),
        greader_password: "api-pass".into(),
        ..SettingsValues::default()
    });
    let error = h.form_error();
    for words in ["never set", "aren't sure", "https://rss.example.com/i/", "External access", "couple of seconds"] {
        assert!(error.contains(words), "{words}: {error}");
    }
    assert!(!h.app.settings().unwrap().checking);
}

#[test]
fn a_stripped_header_after_a_good_login_isnt_called_a_wrong_password() {
    let mut h = first_run();
    h.connect_to(freshrss_found(Api::Ready));
    h.save_result = Err(greader::AUTHORIZATION_STRIPPED.into());
    h.submit(SettingsValues {
        greader_username: "ward".into(),
        greader_password: "api-pass".into(),
        ..SettingsValues::default()
    });
    assert_eq!(h.form_error(), greader::AUTHORIZATION_STRIPPED);
}

#[test]
fn freshrss_with_its_api_off_says_so_and_can_look_again() {
    let mut h = first_run();
    h.connect_to(freshrss_found(Api::Off));
    let view = h.app.connect_view();
    assert!(view.fields.is_empty());
    assert_eq!(view.action, "Check again");
    assert_eq!(view.rows[0].url.as_deref(), Some("https://rss.example.com/i/"));
    assert!(h.form_error().contains("Allow API access"));
    h.submit(SettingsValues { address: "rss.example.com".into(), ..SettingsValues::default() });
    assert_eq!(h.looked_at.len(), 2);
    let probe = h.looked_at[1].0;
    assert_eq!(h.form_error(), "");
    h.found(probe, freshrss_found(Api::Ready));
    assert_eq!(h.field_keys(), [FieldKey::GreaderUsername, FieldKey::GreaderPassword]);
}

#[test]
fn a_stripping_proxy_gets_its_own_diagnosis() {
    let mut h = first_run();
    h.connect_to(freshrss_found(Api::Stripped));
    assert!(h.form_error().contains("Authorization header"));
    assert!(h.app.connect_view().fields.is_empty());
    assert_eq!(h.app.connect_view().rows[0].url.as_deref(), Some(providers::DOCS_MOBILE.url));
}

#[test]
fn what_onward_cant_use_is_said_with_nothing_to_fill_in() {
    let base = || BASE.to_string();
    for (found, words) in [
        (Detected::CantRead { name: "CommaFeed", base: base() }, "This is CommaFeed, and Onward can't read it yet."),
        (Detected::CantRead { name: "yarr", base: base() }, "This is yarr, and Onward can't read it yet."),
        (Detected::Gate { base: base(), to: "https://auth.example.com".into() }, "sign-in page"),
        (Detected::Blocked { base: base() }, "403"),
        (Detected::Unknown { base: base(), sign_in: true }, "can't get past it"),
        (Detected::Unknown { base: base(), sign_in: false }, "found neither Miniflux nor FreshRSS"),
        (Detected::Certificate { base: base() }, "isn't trusted"),
        (Detected::NoAnswer { base: base(), why: "refused".into(), clear_refused: false }, "No answer"),
    ] {
        let mut h = first_run();
        h.connect_to(found);
        assert!(h.form_error().contains(words), "{}", h.form_error());
        assert!(h.app.connect_view().fields.is_empty());
    }
}

#[test]
fn filling_in_by_hand_is_the_form_as_it_was_and_esc_comes_back_from_it() {
    let mut h = first_run();
    h.go_to_connect();
    h.by_hand();
    h.key("Escape");
    assert_eq!((h.app.step(), h.app.connect_view().stage), (Step::Connect, "address"));
    h.by_hand();
    h.submit(SettingsValues { miniflux_url: "https://m.example.com".into(), ..SettingsValues::default() });
    assert_eq!(h.saved_values.clone().unwrap().miniflux_url, "https://m.example.com");
    assert!(h.looked_at.is_empty());
}

#[test]
fn a_hosted_row_is_one_row_of_the_table_and_goes_to_the_same_probe() {
    let hosted = Provider {
        id: "hosted-miniflux",
        label: "Hosted Miniflux",
        short_label: "Hosted Miniflux",
        fixed_url: Some("https://reader.example.net"),
        ..*providers::provider(Server::Miniflux)
    };
    assert!(hosted_rows(&[*providers::provider(Server::Miniflux)]).is_empty(), "no fixed address, no row");
    let mut h = first_run();
    h.go_to_connect();
    h.app.connect.found_rows = hosted_rows(&[hosted]);
    h.row("Hosted Miniflux");
    assert_eq!(h.looked_at.len(), 1);
    assert_eq!(h.looked_at[0].1, Address::parse("https://reader.example.net").unwrap());
    assert_eq!(h.app.settings().unwrap().values.address, "https://reader.example.net");
    let probe = h.looked_at[0].0;
    h.found(probe, miniflux_found());
    assert_eq!(h.field_keys(), [FieldKey::MinifluxUsername, FieldKey::MinifluxPassword]);
}

#[test]
fn settings_can_look_for_a_server_too_and_esc_returns_to_it() {
    let mut h = first_second();
    h.key(",");
    let fx = h.app.find_server();
    h.run(fx);
    assert_eq!((h.app.overlay(), h.app.step()), (Overlay::Onboarding, Step::Connect));
    assert!(h.app.settings().is_some());
    h.key("Escape");
    assert_eq!(h.app.overlay(), Overlay::Settings);
    assert!(h.app.settings().is_some());
}

#[test]
fn a_server_found_from_settings_replaces_the_old_one() {
    let mut h = first_second();
    h.key(",");
    let fx = h.app.find_server();
    h.run(fx);
    let probe = h.look_at("rss.example.com");
    h.found(probe, miniflux_found());
    h.submit(signing_in());
    h.minted(Ok("minted-key".into()));
    assert_eq!(h.saved_values.clone().unwrap().miniflux_api_key, "minted-key");
    assert_eq!(h.app.overlay(), Overlay::Picker);
}

#[test]
fn a_secret_saved_for_one_server_isnt_offered_to_another() {
    let mut h = first_second();
    h.key(",");
    let fx = h.app.find_server();
    h.run(fx);
    let before = h.app.settings().unwrap().refill;
    let old = SettingsValues {
        address: "elsewhere.example.com".into(),
        miniflux_url: "https://old.example.com".into(),
        miniflux_api_key: "old-key".into(),
        greader_url: "https://old.example.com/api/greader.php".into(),
        greader_password: "old-password".into(),
        ..SettingsValues::default()
    };
    h.submit(old.clone());
    let probe = h.looked_at.last().unwrap().0;
    h.found(probe, miniflux_found());
    h.row(providers::NO_PASSWORD_ROW);
    let form = h.app.settings().unwrap();
    assert_eq!(form.values.miniflux_api_key, "");
    assert!(form.refill > before, "the window's copy goes too");

    let mut h = first_second();
    h.key(",");
    let fx = h.app.find_server();
    h.run(fx);
    h.submit(old);
    let probe = h.looked_at.last().unwrap().0;
    h.found(probe, freshrss_found(Api::Ready));
    assert_eq!(h.app.settings().unwrap().values.greader_password, "");
}

#[test]
fn a_secret_stays_for_the_server_it_was_saved_for() {
    let mut h = first_second();
    h.key(",");
    let fx = h.app.find_server();
    h.run(fx);
    h.submit(SettingsValues {
        address: "rss.example.com".into(),
        miniflux_url: BASE.into(),
        miniflux_api_key: "same-key".into(),
        ..SettingsValues::default()
    });
    let probe = h.looked_at.last().unwrap().0;
    h.found(probe, miniflux_found());
    assert_eq!(h.app.settings().unwrap().values.miniflux_api_key, "same-key");
}

#[test]
fn on_the_mac_finding_a_server_leaves_the_settings_window_for_the_main_one() {
    let mut h = first_second();
    h.app.use_settings_window();
    h.key(",");
    assert!(h.app.settings().is_some());
    let fx = h.app.find_server();
    assert!(h.app.settings().is_none(), "that closes the window");
    h.run(fx);
    assert_eq!((h.app.overlay(), h.app.screen()), (Overlay::Onboarding, Screen::Settings));
    h.app.cancel_settings();
    assert_eq!(h.app.overlay(), Overlay::Onboarding, "the closing window's cancel changes nothing");
    let (_, fx) = h.app.key(&KeyPress { key: "Escape".into(), ..KeyPress::default() });
    assert!(fx.iter().any(|e| matches!(e, Effect::ShowSettings)), "it opens again");
    h.run(fx);
    assert_eq!(h.app.overlay(), Overlay::None);
    assert!(h.app.settings().is_some());
}

#[test]
fn outside_settings_there_is_no_finding_a_server() {
    let mut h = first_second();
    assert!(h.app.find_server().is_empty());
    assert_eq!(h.app.overlay(), Overlay::None);
}

#[test]
fn a_look_from_before_the_samples_is_out_of_date_after_them() {
    let mut h = first_run();
    h.go_to_connect();
    let stale = h.look_at("rss.example.com");
    h.key("Escape");
    h.key("ArrowUp");
    h.key("Enter");
    assert!(h.app.samples());
    h.key(",");
    h.found(stale, miniflux_found());
    assert!(h.app.connect_view().fields.is_empty());
}

// ---- Feed lists ------------------------------------------------------------------------------------------------

/// The picker of an account with no feeds, on a server that can do both.
fn empty_account() -> Harness {
    let mut h = Harness::new(Vec::new());
    h.counts = Vec::new();
    h.start();
    assert!(h.picker_loaded());
    h
}

fn imported(added: usize, empty: &[&str], articles: Articles) -> Imported {
    Imported {
        in_file: 4,
        added: Some(added),
        empty: empty.iter().map(ToString::to_string).collect(),
        private_hosts: Vec::new(),
        articles,
    }
}

impl Harness {
    /// The file a dialog came up with.
    fn choose(&mut self, path: &str) {
        let fx = self.app.feed_file(path, false);
        self.run(fx);
    }

    fn job(&self) -> u64 {
        self.calls
            .iter()
            .rev()
            .find_map(|c| match c {
                Effect::ImportFeeds { job, .. } | Effect::ExportFeeds { job, .. } => Some(*job),
                _ => None,
            })
            .expect("an import or export was started")
    }

    fn answer_import(&mut self, result: Result<Imported, String>) {
        let job = self.job();
        let fx = self.app.apply(Outcome::FeedsImported { job, result });
        self.run(fx);
    }

    fn answer_export(&mut self, result: Result<Exported, ExportFailed>) {
        let job = self.job();
        let fx = self.app.apply(Outcome::FeedsExported { job, result });
        self.run(fx);
    }

    fn stage(&self) -> Option<&'static str> {
        self.app.transfer_view().map(|view| view.stage)
    }

    fn feed_counts_asked(&self) -> usize {
        self.calls.iter().filter(|c| **c == Effect::FeedCounts).count()
    }

    fn reloads(&self) -> usize {
        self.calls.iter().filter(|c| matches!(c, Effect::ReloadPickerAfter { .. })).count()
    }
}

#[test]
fn an_empty_account_leads_to_importing_and_on_to_a_first_article() {
    let mut h = empty_account();
    assert!(h.app.offers_import());
    assert!(h.app.picker_empty_text().contains("press i"), "{}", h.app.picker_empty_text());

    h.key("i");
    assert_eq!(h.app.overlay(), Overlay::Transfer);
    assert_eq!(h.stage(), Some("asking"));

    h.key("Enter");
    assert!(h.calls.contains(&Effect::ChooseFeedFile(FeedFile::Import)), "the window brings up its dialog");
    h.choose("/tmp/feeds.opml");
    assert!(h.calls.iter().any(|c| matches!(c, Effect::ImportFeeds { path, .. } if path == "/tmp/feeds.opml")));
    assert_eq!(h.stage(), Some("working"));

    h.counts = counts(3, 0);
    h.fake.entries = vec![entry(1, "Entry 1")];
    h.answer_import(Ok(imported(4, &[], Articles::OnTheirWay)));
    assert_eq!(h.stage(), Some("done"));
    let report = h.app.transfer_view().unwrap().text;
    assert!(report.starts_with("4 feeds added."), "{report}");
    assert!(report.contains("told to fetch their articles"), "{report}");

    h.key("Enter");
    assert_eq!(h.app.transfer_view(), None);
    assert!(h.picker_loaded());
    assert_eq!(h.reloads(), REFRESH_RELOAD_MS.len(), "the list fills while the server fetches");
    assert!(!h.app.offers_import());
    h.key("Enter");
    assert!(h.app.current().is_some(), "a first article");
}

#[test]
fn a_file_that_fails_goes_back_to_asking_and_says_why() {
    let mut h = empty_account();
    h.key("i");
    h.choose("/tmp/nothing.opml");
    h.answer_import(Err("There's no file at /tmp/nothing.opml.".into()));
    let view = h.app.transfer_view().unwrap();
    assert_eq!((view.stage, view.error.as_str()), ("asking", "There's no file at /tmp/nothing.opml."));

    h.choose("/tmp/feeds.opml");
    let view = h.app.transfer_view().unwrap();
    assert_eq!((view.stage, view.error.as_str()), ("working", ""), "the old complaint is gone");
    assert_eq!(h.calls.iter().filter(|c| matches!(c, Effect::ImportFeeds { .. })).count(), 2);
}

#[test]
fn an_empty_path_asks_for_one_and_sends_nothing() {
    let mut h = empty_account();
    h.key("i");
    h.choose("   ");
    assert_eq!(h.stage(), Some("asking"));
    assert!(!h.app.transfer_view().unwrap().error.is_empty());
    assert!(!h.calls.iter().any(|c| matches!(c, Effect::ImportFeeds { .. })));
}

#[test]
fn the_report_claims_only_what_was_seen() {
    let says = |result: Imported| result.report();
    let report = says(imported(0, &[], Articles::OnTheirWay));
    assert!(report.starts_with("Nothing new"), "{report}");
    assert!(!report.contains("fetch"), "{report}");

    let partly = says(imported(3, &[], Articles::Fetched));
    assert_eq!(partly, "3 of the 4 feeds in the file added. The server had the rest, or left them out.");

    let not_asked = says(imported(4, &[], Articles::NotAsked("Miniflux 500".into())));
    assert!(not_asked.contains("couldn't ask the server to fetch them (Miniflux 500)"), "{not_asked}");

    let unknown = says(Imported { added: None, ..imported(0, &[], Articles::OnTheirWay) });
    assert!(unknown.starts_with("The server took the file, with 4 feeds, but"), "{unknown}");
    assert!(unknown.contains("couldn't get the list back"), "{unknown}");
}

#[test]
fn miniflux_refuses_private_hosts_later_so_the_report_says_so_now() {
    let private = Imported { private_hosts: vec!["feeds.lan".into()], ..imported(4, &[], Articles::OnTheirWay) };
    let report = private.report();
    assert!(report.contains("Feeds on feeds.lan stay empty unless"), "{report}");
    assert!(report.contains("FETCHER_ALLOW_PRIVATE_NETWORKS"), "{report}");
    assert!(!imported(4, &[], Articles::OnTheirWay).report().contains("FETCHER_ALLOW"));
}

#[test]
fn feeds_that_stayed_empty_on_freshrss_are_named_with_the_likely_reason() {
    let everything_arrived = imported(4, &[], Articles::Fetched);
    assert_eq!(everything_arrived.report(), "4 feeds added.");

    let private = Imported {
        private_hosts: vec!["feeds.lan".into()],
        ..imported(4, &["One", "Two", "Three", "Four"], Articles::Fetched)
    };
    let report = private.report();
    assert!(report.contains("No articles yet from 4 of them: One, Two, Three and 1 more."), "{report}");
    assert!(report.contains("INTERNAL_HOST_ALLOWLIST"), "{report}");
    assert!(report.contains("feeds.lan"), "{report}");

    let one = imported(4, &["Quiet"], Articles::Fetched).report();
    assert!(one.contains("No articles from Quiet yet."), "{one}");
    assert!(one.contains("may be down"), "{one}");
}

#[test]
fn closing_the_panel_leaves_the_work_going_and_its_answer_comes_as_a_toast() {
    let mut h = empty_account();
    h.key("i");
    h.choose("/tmp/feeds.opml");
    h.key("Escape");
    assert!(h.picker_loaded());
    assert_eq!(h.stage(), Some("working"), "it can't be stopped, and isn't forgotten");

    h.key("i");
    assert_eq!(h.app.overlay(), Overlay::Transfer);
    assert_eq!(h.stage(), Some("working"), "the same one, rather than a second");
    h.choose("/tmp/other.opml");
    assert_eq!(h.calls.iter().filter(|c| matches!(c, Effect::ImportFeeds { .. })).count(), 1);
    h.key("Escape");

    let asked = h.feed_counts_asked();
    h.answer_import(Ok(imported(4, &[], Articles::OnTheirWay)));
    assert_eq!(h.app.overlay(), Overlay::Picker);
    assert_eq!(h.app.transfer_view(), None);
    assert!(h.app.toast().unwrap().message.starts_with("4 feeds added."));
    assert_eq!(h.feed_counts_asked(), asked + 1);
    assert_eq!(h.reloads(), REFRESH_RELOAD_MS.len());
}

#[test]
fn a_report_with_something_to_act_on_stays_up_to_be_read() {
    let mut h = empty_account();
    h.key("i");
    h.choose("/tmp/feeds.opml");
    h.key("Escape");
    h.answer_import(Ok(imported(4, &["Quiet"], Articles::Fetched)));
    let toast = h.app.toast().unwrap();
    assert!(toast.message.contains("No articles from Quiet yet."), "{}", toast.message);
    assert_eq!(toast.kind, ToastKind::Error, "the kind of toast that lasts");
}

#[test]
fn a_failure_with_the_panel_closed_is_a_toast_too() {
    let mut h = empty_account();
    h.key("i");
    h.choose("/tmp/feeds.opml");
    h.key("Escape");
    h.answer_import(Err("It went wrong".into()));
    let toast = h.app.toast().unwrap();
    assert_eq!((toast.message.as_str(), toast.kind), ("It went wrong", ToastKind::Error));
    assert_eq!(h.app.transfer_view(), None);
}

#[test]
fn an_answer_about_a_server_you_left_changes_nothing() {
    let mut h = empty_account();
    h.key("i");
    h.choose("/tmp/feeds.opml");
    let job = h.job();
    h.key("Escape");
    // Connects to another server meanwhile.
    h.key(",");
    let fx = h.app.submit_settings(SettingsValues::default());
    h.run(fx);
    assert_eq!(h.stage(), None);
    let (toast, asked) = (h.app.toast().map(|t| t.serial), h.feed_counts_asked());

    let fx = h.app.apply(Outcome::FeedsImported { job, result: Ok(imported(4, &[], Articles::OnTheirWay)) });
    assert!(fx.is_empty());
    let fx = h.app.apply(Outcome::FeedsImported { job, result: Err("gone".into()) });
    assert!(fx.is_empty());
    assert_eq!(h.app.toast().map(|t| t.serial), toast, "nothing said");
    assert_eq!(h.feed_counts_asked(), asked, "nothing asked of the new server");
    assert_eq!(h.reloads(), 0);

    // And a new one on the new server isn't mistaken for it.
    h.key("i");
    h.choose("/tmp/feeds.opml");
    let fx = h.app.apply(Outcome::FeedsImported { job, result: Ok(imported(4, &[], Articles::Fetched)) });
    assert!(fx.is_empty());
    assert_eq!(h.stage(), Some("working"));
}

#[test]
fn leaving_the_samples_makes_an_answer_on_its_way_a_no_op() {
    let mut h = empty_account();
    h.key("e");
    h.choose("/tmp/feeds.opml");
    let job = h.job();
    h.key("Escape");
    h.app.samples = Some(Prefs::default());
    h.key(",");
    assert_eq!(h.app.overlay(), Overlay::Onboarding);
    let exported = Exported { path: "/tmp/feeds.opml".into(), feeds: 1, left_out: 0 };
    let fx = h.app.apply(Outcome::FeedsExported { job, result: Ok(exported) });
    assert!(fx.is_empty());
    assert_eq!(h.app.toast(), None);
}

#[test]
fn what_the_server_cant_do_is_left_out_and_said() {
    // Miniflux through its Google Reader integration, or some other Google Reader server.
    let mut h = empty_account();
    let cannot = Capabilities { import_feeds: false, feeds: FeedOps::NONE, ..Capabilities::EVERYTHING };
    h.status = Status { looking: false, configured: true, readeck: false, capabilities: cannot };
    h.start();
    assert!(!h.app.offers_import() && !h.app.offers_add());
    assert!(h.app.picker_empty_text().contains(feeds::NOTHING_CAN_BE_ADDED));
    assert!(!h.app.help_lines().iter().any(|(keys, _)| keys == "i"));

    h.key("i");
    assert_eq!(h.app.overlay(), Overlay::Picker);
    assert_eq!(h.app.toast().unwrap().message, feeds::IMPORT_REFUSED);

    h.key("e");
    assert_eq!(h.app.overlay(), Overlay::Transfer, "exporting works from its list");
}

#[test]
fn the_samples_say_theres_nothing_to_import_into() {
    let mut h = Harness::new(vec![entry(1, "Entry 1")]);
    h.status = Status { looking: false, configured: true, readeck: true, capabilities: Capabilities::SAMPLES };
    h.start();
    for key in ["i", "e"] {
        h.key(key);
        assert_eq!(h.app.overlay(), Overlay::Picker);
        assert_eq!(h.app.toast().unwrap().message, feeds::SAMPLES_HAVE_NO_FEED_LIST);
        assert_eq!(h.stage(), None);
    }
    assert!(!h.app.help_lines().iter().any(|(keys, _)| keys == "i" || keys == "e"));
}

#[test]
fn an_export_says_where_it_went_and_what_it_left_out() {
    let mut h = empty_account();
    h.key("e");
    assert_eq!(h.app.transfer_view().unwrap().suggested, "onward-feeds.opml");
    h.key("Enter");
    assert!(h.calls.contains(&Effect::ChooseFeedFile(FeedFile::Export)));
    h.choose("/tmp/feeds.opml");
    assert!(h.calls.iter().any(|c| matches!(c, Effect::ExportFeeds { replace: false, .. })));
    h.answer_export(Ok(Exported { path: "/tmp/feeds.opml".into(), feeds: 12, left_out: 1 }));
    let view = h.app.transfer_view().unwrap();
    assert_eq!(view.stage, "done");
    assert_eq!(
        view.text,
        "Saved 12 feeds to /tmp/feeds.opml. 1 feed left out, as the server gave no address for them."
    );
    h.key("Escape");
    assert_eq!(h.reloads(), 0, "nothing on the server changed");
}

#[test]
fn a_file_thats_there_is_replaced_only_after_asking() {
    let mut h = empty_account();
    h.key("e");
    h.choose("/tmp/feeds.opml");
    h.answer_export(Err(ExportFailed::Exists("/tmp/feeds.opml".into())));
    assert_eq!(h.app.overlay(), Overlay::Confirm);
    assert_eq!(h.app.confirm_text(), "/tmp/feeds.opml is already there. Replace it?");

    h.key("n");
    assert_eq!(h.app.overlay(), Overlay::Transfer);
    assert_eq!(h.stage(), Some("asking"), "another name, then");
    assert_eq!(h.calls.iter().filter(|c| matches!(c, Effect::ExportFeeds { .. })).count(), 1);

    h.choose("/tmp/feeds.opml");
    h.answer_export(Err(ExportFailed::Exists("/tmp/feeds.opml".into())));
    h.key("y");
    assert_eq!(h.app.overlay(), Overlay::Transfer);
    assert_eq!(h.stage(), Some("working"));
    assert!(
        h.calls
            .iter()
            .any(|c| matches!(c, Effect::ExportFeeds { replace: true, path, .. } if path == "/tmp/feeds.opml"))
    );
}

#[test]
fn a_dialog_that_asked_already_replaces_without_asking_again() {
    let mut h = empty_account();
    h.key("e");
    let fx = h.app.feed_file("/tmp/feeds.opml", true);
    assert!(matches!(fx.as_slice(), [Effect::ExportFeeds { replace: true, .. }]));
}

#[test]
fn with_the_panel_closed_a_file_thats_there_is_no_question() {
    let mut h = empty_account();
    h.key("e");
    h.choose("/tmp/feeds.opml");
    h.key("Escape");
    h.answer_export(Err(ExportFailed::Exists("/tmp/feeds.opml".into())));
    assert_eq!(h.app.overlay(), Overlay::Picker);
    assert_eq!(h.app.toast().unwrap().message, "/tmp/feeds.opml is already there.");
}

#[test]
fn the_panel_opened_from_the_reader_goes_back_to_the_reader() {
    let mut h = Harness::new(vec![entry(1, "Entry 1")]);
    h.open(false);
    let fx = h.app.action(Action::ExportFeeds);
    h.run(fx);
    assert_eq!(h.app.overlay(), Overlay::Transfer);
    assert!(h.app.current().is_some());
    h.key("Escape");
    assert_eq!(h.app.overlay(), Overlay::None);
    assert_eq!(h.title(), Some("Entry 1"));
}

#[test]
fn nothing_else_happens_behind_the_panel() {
    let mut h = empty_account();
    h.key("i");
    let calls = h.calls.len();
    for key in ["j", "A", "q", "h", ","] {
        h.key(key);
        assert_eq!(h.app.overlay(), Overlay::Transfer, "{key}");
    }
    assert_eq!(h.calls.len(), calls);
}

// ---- Changing the feeds ----------------------------------------------------------------------------------------------

/// News has Alpha and Beta, Tech has Gamma, and Empty has nothing.
fn feed_list() -> FeedList {
    let (news, tech) = (cat(1, "News"), cat(2, "Tech"));
    let feed = |id, title: &str, category: &Category| FeedCount {
        feed: Feed { id, title: title.into(), category: category.clone() },
        unread: Some(5),
        read: Some(1),
    };
    let feeds = vec![feed(10, "Alpha", &news), feed(11, "Beta", &news), feed(12, "Gamma", &tech)];
    FeedList::new(Counts { feeds, starred: Some(0) }, vec![news, tech, cat(3, "Empty")], None)
}

fn cat(id: i64, title: &str) -> Category {
    Category { id, title: title.into() }
}

/// Articles 1 to 3 of feed 10, 4 and 5 of feed 12.
fn of_two_feeds() -> Vec<Entry> {
    let mut entries = items(5);
    for entry in &mut entries {
        entry.feed.id = if entry.id <= 3 { 10 } else { 12 };
        entry.feed.title = if entry.id <= 3 { "Alpha".into() } else { "Gamma".into() };
    }
    entries
}

const FRESHRSS: Capabilities = Capabilities { refresh: false, feeds: FeedOps::FRESHRSS, ..Capabilities::EVERYTHING };

/// In the picker, on a server that can do `can`, with `feed_list` on it and the panel open.
fn managing(can: Capabilities) -> Harness {
    let mut h = Harness::new(of_two_feeds());
    h.status.capabilities = can;
    h.list = feed_list();
    h.counts = h.list.counts.feeds.clone();
    h.start();
    h.key("m");
    assert_eq!(h.app.overlay(), Overlay::Manage);
    h
}

impl Harness {
    fn changes(&self) -> Vec<FeedChange> {
        let changes = self.calls.iter().filter_map(|call| match call {
            Effect::ChangeFeeds { change, .. } => Some(change.clone()),
            _ => None,
        });
        changes.collect()
    }

    fn manage_stage(&self) -> &'static str {
        self.app.manage_view().map_or("closed", |view| view.stage)
    }

    fn cursor_on(&mut self, label: &str) {
        for _ in 0..20 {
            let view = self.app.manage_view().unwrap();
            if view.rows[view.cursor].label == label {
                return;
            }
            self.key("j");
        }
        panic!("no row called {label}");
    }

    /// Answers the change in flight, the server having done `applied` and now listing `list`.
    fn changed(&mut self, applied: Applied, list: Option<FeedList>, problem: Option<&str>) {
        let job = self.app.latest_manage;
        let change = self.changes().pop().expect("a change was sent");
        let result = Ok(Changed { applied, list, problem: problem.map(str::to_string) });
        let fx = self.app.apply(Outcome::FeedsChanged { job, change, result });
        self.run(fx);
    }

    fn refused(&mut self, message: &str) {
        let job = self.app.latest_manage;
        let change = self.changes().pop().expect("a change was sent");
        let fx = self.app.apply(Outcome::FeedsChanged { job, change, result: Err(message.to_string()) });
        self.run(fx);
    }

    fn notice(&self) -> String {
        self.app.manage_view().unwrap().notice
    }

    fn list_without(&self, feed: i64) -> FeedList {
        let mut list = self.list.clone();
        list.counts.feeds.retain(|f| f.feed.id != feed);
        list
    }
}

#[test]
fn nothing_is_removed_before_the_answer_and_enter_isnt_one() {
    let mut h = managing(Capabilities::EVERYTHING);
    h.cursor_on("Alpha");
    h.key("d");
    assert_eq!(h.app.overlay(), Overlay::Confirm);
    assert!(h.app.manage_shown());
    let question = h.app.confirm_text().to_string();
    assert!(question.contains("Alpha") && question.contains("articles go too"), "{question}");
    assert!(question.contains("can't be undone"), "{question}");
    assert!(h.changes().is_empty());

    h.key("Enter");
    assert_eq!(h.app.overlay(), Overlay::Confirm, "a key pressed once too often doesn't lose anything");
    assert!(h.changes().is_empty());
    let (used, fx) = h.app.key(&KeyPress { key: "Enter".into(), ..KeyPress::default() });
    assert!(used && fx.is_empty(), "used, or the Mac's alert would take it for its default button");
    assert_eq!(h.app.confirm_action(), "Remove");
    assert!(h.app.confirm_lasting());
    assert!(h.app.confirm_hint().starts_with("y to go ahead"));

    h.key("n");
    assert_eq!(h.app.overlay(), Overlay::Manage);
    assert!(h.changes().is_empty());

    h.key("d");
    h.key("Escape");
    assert!(h.changes().is_empty());

    h.key("d");
    h.key("y");
    assert_eq!(h.changes(), [FeedChange::RemoveFeed { id: 10 }]);
    assert_eq!(h.manage_stage(), "working");
}

#[test]
fn a_miniflux_category_goes_only_after_being_told_how_many_feeds_and_articles_go_with_it() {
    let mut h = managing(Capabilities::EVERYTHING);
    h.cursor_on("News");
    // Added elsewhere since the panel opened: the question goes by what the server says now.
    let news = cat(1, "News");
    let mut extra = h.list.counts.feeds[0].clone();
    (extra.feed.id, extra.feed.title) = (13, "Delta".into());
    h.list.counts.feeds.push(extra);
    assert_eq!(news.title, h.list.counts.feeds[3].feed.category.title);

    h.key("d");
    assert_eq!(h.app.overlay(), Overlay::Confirm);
    let question = h.app.confirm_text().to_string();
    for words in ["“News”", "3 feeds", "all their articles", "can't be undone", "move them"] {
        assert!(question.contains(words), "{words}: {question}");
    }
    let looks_only = h.calls.iter().filter(|c| matches!(c, Effect::CountCategory { .. })).count();
    assert_eq!(looks_only, 1);
    assert!(h.changes().is_empty());
    h.key("Enter");
    assert!(h.changes().is_empty(), "Enter says nothing here");
    h.key("n");
    assert!(h.changes().is_empty());
    assert_eq!(h.app.overlay(), Overlay::Manage);

    h.key("d");
    h.key("y");
    assert_eq!(h.changes(), [FeedChange::DeleteCategory { category: news }]);
}

#[test]
fn on_freshrss_the_same_key_says_the_feeds_stay_and_an_empty_category_says_so_too() {
    let mut h = managing(FRESHRSS);
    h.cursor_on("News");
    h.key("d");
    let question = h.app.confirm_text().to_string();
    assert!(question.contains("2 feeds and their articles stay"), "{question}");
    assert!(!question.contains("undone"), "{question}");
    assert!(h.app.confirm_hint().starts_with("Enter"), "nothing's lost");
    h.key("Enter");
    assert_eq!(h.changes().len(), 1);

    let mut h = managing(Capabilities::EVERYTHING);
    h.cursor_on("Empty");
    h.key("d");
    assert!(h.app.confirm_text().contains("no feeds in it"));
    assert!(h.app.confirm_hint().starts_with("Enter"), "nothing's lost on Miniflux either");
}

#[test]
fn a_panel_closed_while_the_server_counts_asks_nothing_afterwards() {
    let mut h = managing(Capabilities::EVERYTHING);
    h.cursor_on("News");
    let job = h.app.latest_manage;
    let mut fx = Vec::new();
    h.app.manage_delete(&mut fx);
    assert_eq!(h.manage_stage(), "working");
    h.key("Escape");
    assert_eq!(h.app.overlay(), Overlay::Picker);
    let answer = Outcome::CategoryCounted { job, category: cat(1, "News"), result: Ok(feed_list()) };
    assert!(h.app.apply(answer).is_empty());
    assert_eq!(h.app.overlay(), Overlay::Picker);
    assert_eq!(h.manage_stage(), "closed");
    assert_eq!(h.app.confirm_text(), "");
}

#[test]
fn a_category_someone_else_deleted_isnt_asked_about() {
    let mut h = managing(Capabilities::EVERYTHING);
    h.cursor_on("Empty");
    h.list.categories.retain(|c| c.id != 3);
    h.key("d");
    assert_eq!(h.app.overlay(), Overlay::Manage);
    assert_eq!(h.notice(), feeds::GONE);
    assert!(h.changes().is_empty());
}

#[test]
fn what_a_server_cant_do_isnt_offered_and_its_key_does_nothing() {
    let renames_only = FeedOps { rename: true, ..FeedOps::NONE };
    let mut h = managing(Capabilities { feeds: renames_only, ..Capabilities::EVERYTHING });
    h.cursor_on("Alpha");
    let keys: String = h.app.manage_view().unwrap().keys.iter().map(|k| k.key).collect();
    assert_eq!(keys, "rf");
    for key in ["a", "m", "d"] {
        h.key(key);
        assert_eq!(h.app.overlay(), Overlay::Manage);
        assert_eq!(h.manage_stage(), "list", "{key}");
    }
    assert!(h.changes().is_empty());
}

#[test]
fn a_connection_that_can_change_nothing_says_why_and_stays_in_the_picker() {
    let mut h = Harness::new(items(2));
    h.status.capabilities = Capabilities { feeds: FeedOps::NONE, ..Capabilities::EVERYTHING };
    h.start();
    for key in ["m", "a"] {
        h.key(key);
        assert_eq!(h.app.overlay(), Overlay::Picker);
        assert_eq!(h.toast(), Some(feeds::MANAGE_REFUSED));
    }
    assert!(!h.calls.iter().any(|c| matches!(c, Effect::LoadFeedList { .. })));
}

#[test]
fn the_samples_say_plainly_that_their_feeds_cant_change_and_ask_no_server() {
    let mut h = Harness::new(items(2));
    h.status.capabilities = Capabilities::SAMPLES;
    h.start();
    for key in ["m", "a"] {
        h.key(key);
        assert_eq!(h.app.overlay(), Overlay::Picker);
        assert_eq!(h.toast(), Some(feeds::SAMPLES_HAVE_NO_FEEDS));
    }
    assert!(!h.calls.iter().any(|c| matches!(c, Effect::LoadFeedList { .. } | Effect::ChangeFeeds { .. })));
}

#[test]
fn an_empty_account_is_led_to_adding_a_feed_and_lands_on_its_row() {
    let mut h = Harness::new(Vec::new());
    h.counts = Vec::new();
    h.start();
    assert!(h.app.offers_add() && h.app.offers_import());
    assert!(h.app.picker_empty_text().contains("Press a to add one by its address"));
    h.key("a");
    assert_eq!(h.manage_stage(), "field");
    assert_eq!(h.app.manage_view().unwrap().field.unwrap().action, "Add");
    // Typing in a window's field: j and the like aren't keys here.
    let (used, _) = h.app.key(&KeyPress { key: "j".into(), typing: true, ..KeyPress::default() });
    assert!(!used);
    let fx = h.app.submit_manage("  ");
    assert!(fx.is_empty());
    assert!(!h.app.manage_view().unwrap().error.is_empty());

    let fx = h.app.submit_manage(" https://a.example/feed ");
    h.run(fx);
    assert_eq!(h.manage_stage(), "working");
    assert_eq!(h.changes(), [FeedChange::Add { url: "https://a.example/feed".into(), category: None }]);

    let mut list = feed_list();
    list.counts.feeds[1].unread = Some(0);
    list.counts.feeds[1].read = Some(0);
    let applied = Applied { feed: Some(11), category: None };
    h.changed(applied, Some(list), None);
    let view = h.app.manage_view().unwrap();
    assert_eq!(view.stage, "list");
    assert_eq!(view.rows[view.cursor].label, "Beta");
    assert!(view.notice.contains("No articles yet: press f"), "{}", view.notice);
    assert!(h.calls.iter().any(|c| matches!(c, Effect::ReloadPickerAfter { .. })), "the numbers follow");
    assert!(!h.app.no_feeds(), "the picker has it too");
}

#[test]
fn a_new_feed_goes_where_the_cursor_is_and_a_refusal_brings_the_address_back_with_the_servers_words() {
    let mut h = managing(Capabilities::EVERYTHING);
    h.cursor_on("Gamma");
    h.key("a");
    assert!(h.app.manage_view().unwrap().text.contains("“Tech”"));
    let fx = h.app.submit_manage("http://feeds.example/x.xml");
    h.run(fx);
    assert_eq!(
        h.changes(),
        [FeedChange::Add { url: "http://feeds.example/x.xml".into(), category: Some(cat(2, "Tech")) }]
    );
    h.refused("Miniflux says: This feed already exists.");
    let view = h.app.manage_view().unwrap();
    assert_eq!(view.stage, "field");
    assert_eq!(view.error, "Miniflux says: This feed already exists.");
    assert_eq!(view.field.unwrap().value, "http://feeds.example/x.xml");
    h.key("Escape");
    assert_eq!(h.manage_stage(), "list");
    h.key("Escape");
    assert_eq!(h.app.overlay(), Overlay::Picker);
    assert_eq!(h.manage_stage(), "closed");
}

#[test]
fn renaming_keeps_its_field_and_a_name_thats_taken_or_unchanged_sends_nothing() {
    let mut h = managing(Capabilities::EVERYTHING);
    h.cursor_on("Tech");
    h.key("r");
    assert_eq!(h.app.manage_view().unwrap().field.unwrap().value, "Tech");
    assert!(h.app.submit_manage("Tech").is_empty());
    assert_eq!(h.manage_stage(), "list");
    h.key("r");
    assert!(h.app.submit_manage("news").is_empty(), "FreshRSS says OK to this and does nothing");
    let view = h.app.manage_view().unwrap();
    assert!(view.error.contains("already a category called “news”"), "{}", view.error);
    assert_eq!(view.field.unwrap().value, "news");
    assert!(h.changes().is_empty());
    assert!(h.app.submit_manage("").is_empty());
    assert_eq!(h.app.manage_view().unwrap().error, "It needs a name.");
    let fx = h.app.submit_manage("Science");
    h.run(fx);
    assert_eq!(h.changes(), [FeedChange::RenameCategory { category: cat(2, "Tech"), title: "Science".into() }]);
}

#[test]
fn moving_offers_the_other_categories_and_a_new_name_that_turns_out_to_exist_is_that_one() {
    let mut h = managing(Capabilities::EVERYTHING);
    h.cursor_on("Alpha");
    h.key("m");
    let view = h.app.manage_view().unwrap();
    assert_eq!(view.stage, "choose");
    let labels: Vec<&str> = view.rows.iter().map(|r| r.label.as_str()).collect();
    assert_eq!(labels, ["Empty", "Tech", "New category…"]);
    h.key("j");
    h.key("Enter");
    assert_eq!(
        h.changes(),
        [FeedChange::MoveFeed { id: 10, to: Destination::Existing(cat(2, "Tech")) }],
        "the row under the cursor"
    );
    h.changed(Applied { feed: None, category: Some(cat(2, "Tech")) }, Some(feed_list()), None);
    assert_eq!(h.notice(), "Moved “Alpha” to “Tech”.");

    h.cursor_on("Alpha");
    h.key("m");
    h.key("k");
    h.key("Enter");
    assert_eq!(h.manage_stage(), "field");
    let fx = h.app.submit_manage("  tech ");
    h.run(fx);
    assert_eq!(h.changes()[1], FeedChange::MoveFeed { id: 10, to: Destination::Existing(cat(2, "Tech")) });
    h.refused("nope");
    let fx = h.app.submit_manage("Brand New");
    h.run(fx);
    assert_eq!(h.changes()[2], FeedChange::MoveFeed { id: 10, to: Destination::New("Brand New".into()) });
}

#[test]
fn the_defaults_of_freshrss_and_a_feed_with_no_category_say_why_theres_nothing_to_rename() {
    let mut h = Harness::new(items(1));
    h.status.capabilities = FRESHRSS;
    h.list = feed_list();
    h.list.fixed = Some(1);
    h.start();
    h.key("m");
    h.cursor_on("News");
    h.key("r");
    assert_eq!(h.manage_stage(), "list");
    assert!(h.notice().contains("default category"), "{}", h.notice());
    h.key("d");
    assert_eq!(h.app.overlay(), Overlay::Manage);
    assert!(h.changes().is_empty());
    assert!(!h.calls.iter().any(|c| matches!(c, Effect::CountCategory { .. })));
}

#[test]
fn a_yes_the_list_doesnt_bear_out_is_reported_as_what_happened() {
    let mut h = managing(FRESHRSS);
    h.cursor_on("Tech");
    h.key("r");
    let fx = h.app.submit_manage("Science");
    h.run(fx);
    let said = "The server said yes, but “Tech” kept its name. Nothing changed.";
    h.changed(Applied { feed: None, category: Some(cat(99, "Science")) }, Some(feed_list()), Some(said));
    let view = h.app.manage_view().unwrap();
    assert_eq!(view.notice, said);
    assert!(view.worrying);
}

#[test]
fn closing_the_panel_leaves_a_change_running_and_its_answer_is_a_toast() {
    let mut h = managing(Capabilities::EVERYTHING);
    h.cursor_on("Alpha");
    h.key("r");
    let fx = h.app.submit_manage("Renamed");
    h.run(fx);
    h.key("Escape");
    assert_eq!(h.app.overlay(), Overlay::Picker);
    assert_eq!(h.manage_stage(), "working", "it's still going");
    h.key("m");
    assert_eq!(h.manage_stage(), "working", "and back there");
    h.key("Escape");
    h.changed(Applied::default(), Some(feed_list()), None);
    assert_eq!(h.manage_stage(), "closed");
    assert_eq!(h.toast(), Some("Renamed to “Renamed”."));
}

#[test]
fn a_change_of_server_makes_the_answer_a_no_op() {
    let mut h = managing(Capabilities::EVERYTHING);
    h.cursor_on("Alpha");
    h.key("d");
    h.key("y");
    h.key("Escape");
    let (job, list) = (h.app.latest_manage, h.app.latest_list);
    h.key(",");
    h.run(vec![Effect::LoadSettings]);
    let fx = h.app.submit_settings(SettingsValues::default());
    h.run(fx);
    assert_eq!(h.manage_stage(), "closed");
    let change = FeedChange::RemoveFeed { id: 10 };
    let changed = Changed { applied: Applied::default(), list: Some(feed_list()), problem: None };
    let fx = h.app.apply(Outcome::FeedsChanged { job, change, result: Ok(changed) });
    assert!(fx.is_empty());
    assert!(h.app.apply(Outcome::FeedList { serial: list, result: Ok(feed_list()) }).is_empty());
    assert_eq!(h.manage_stage(), "closed");
}

#[test]
fn a_list_asked_for_before_a_change_doesnt_overwrite_what_came_after() {
    let mut h = managing(Capabilities::EVERYTHING);
    let old_serial = h.app.latest_list;
    h.cursor_on("Alpha");
    h.key("r");
    let fx = h.app.submit_manage("Renamed");
    h.run(fx);
    let mut after = feed_list();
    after.counts.feeds[0].feed.title = "Renamed".into();
    let during = h.app.latest_list;
    h.changed(Applied::default(), Some(after), None);
    for serial in [old_serial, during] {
        h.app.apply(Outcome::FeedList { serial, result: Ok(feed_list()) });
    }
    assert!(h.app.manage_view().unwrap().rows.iter().any(|r| r.label == "Renamed"));
}

#[test]
fn a_renamed_feed_keeps_its_queue_and_shows_the_new_name() {
    let mut h = Harness::new(of_two_feeds());
    h.list = feed_list();
    h.start();
    h.app.opened = None;
    let fx = h.app.choose_row(0);
    h.run(fx);
    h.app.opened = Some(Opened::Feeds(Selection::Feed { id: 10 }));
    h.app.selection_label = "Alpha".into();
    h.app.overlay = Overlay::Picker;
    h.key("m");
    h.cursor_on("Alpha");
    h.key("r");
    let fx = h.app.submit_manage("Alpha One");
    h.run(fx);
    h.changed(Applied::default(), Some(feed_list()), None);
    assert_eq!(h.app.selection_label(), "Alpha One");
    assert_eq!(h.app.queue()[0].feed.title, "Alpha One");
    assert_eq!(h.app.queue()[4].feed.title, "Gamma");
    assert_eq!(h.app.queue().len(), 5);
}

/// `selection` is open with `of_two_feeds` in it, and the panel is up over it.
fn reading(selection: Selection, label: &str, can: Capabilities) -> Harness {
    let mut h = Harness::new(of_two_feeds());
    h.status.capabilities = can;
    h.list = feed_list();
    h.counts = h.list.counts.feeds.clone();
    h.start();
    let mut fx = Vec::new();
    h.app.close_picker();
    h.app.open_selection(selection, label.to_string(), Mode::Unread, &mut fx);
    h.run(fx);
    assert!(h.app.current().is_some());
    h.key("p");
    h.key("m");
    h
}

#[test]
fn a_category_thats_open_goes_on_under_its_new_id_with_its_place_kept() {
    let mut h = reading(Selection::Category { id: 2 }, "Tech", FRESHRSS);
    h.key("Escape");
    h.key("Escape");
    h.key("j");
    h.key("p");
    h.key("m");
    h.cursor_on("Tech");
    h.key("r");
    let fx = h.app.submit_manage("Science");
    h.run(fx);
    h.changed(Applied { feed: None, category: Some(cat(77, "Science")) }, Some(feed_list()), None);
    assert_eq!(h.app.selection_label(), "Science");
    assert_eq!(h.app.opened, Some(Opened::Feeds(Selection::Category { id: 77 })));
    assert_eq!(h.app.current().map(|e| e.id), Some(2), "you're where you were");
    assert_eq!(h.app.queue().len(), 5);
}

#[test]
fn a_rename_the_list_doesnt_bear_out_leaves_the_open_list_as_it_was() {
    let mut h = reading(Selection::Category { id: 2 }, "Tech", FRESHRSS);
    h.cursor_on("Tech");
    h.key("r");
    let fx = h.app.submit_manage("Science");
    h.run(fx);
    let said = "The server said yes, but “Tech” kept its name. Nothing changed.";
    h.changed(Applied { feed: None, category: Some(cat(77, "Science")) }, Some(feed_list()), Some(said));
    assert_eq!(h.app.opened, Some(Opened::Feeds(Selection::Category { id: 2 })));
    assert_eq!(h.app.selection_label(), "Tech");
}

#[test]
fn a_rename_that_keeps_the_id_leaves_a_page_on_its_way_alone_only_when_the_id_is_the_same() {
    // A page for the old id can't be answered any more: it's dropped, and the next one asks by the new id.
    let mut entries = items(PAGE as i64 + 5);
    for entry in &mut entries {
        entry.feed.id = 12;
    }
    let mut h = Harness::new(entries);
    h.status.capabilities = FRESHRSS;
    h.list = feed_list();
    h.start();
    let mut fx = Vec::new();
    h.app.close_picker();
    h.app.open_selection(Selection::Category { id: 2 }, "Tech".into(), Mode::Unread, &mut fx);
    h.run(fx);
    h.hold_next_page = true;
    for _ in 0..PAGE - PREFETCH {
        h.key("n");
    }
    assert!(h.held.is_some());
    let stale = h.held.take().unwrap();
    h.key("p");
    h.key("m");
    h.cursor_on("Tech");
    h.key("r");
    let fx = h.app.submit_manage("Science");
    h.run(fx);
    h.changed(Applied { feed: None, category: Some(cat(77, "Science")) }, Some(feed_list()), None);

    let Some(Outcome::Page { load, .. }) = h.exec(stale) else { panic!("a page") };
    let late = Outcome::Page { load, arrivals: false, result: Err("That feed or category isn't there".into()) };
    h.app.apply(late);
    assert_eq!(h.app.toast(), None, "an answer about the old id is a no-op");
    h.key("Escape");
    h.key("Escape");
    h.key("n");
    let next = h.entries_calls().last().map(|c| (*c).clone());
    assert!(matches!(next, Some(Effect::LoadPage { selection: Selection::Category { id: 77 }, .. })), "{next:?}");
}

#[test]
fn a_question_about_a_list_that_changes_id_or_goes_is_dropped_rather_than_answered() {
    let mut h = reading(Selection::Category { id: 2 }, "Tech", FRESHRSS);
    h.cursor_on("Tech");
    h.key("r");
    let fx = h.app.submit_manage("Science");
    h.run(fx);
    h.key("Escape");
    h.key("Escape");
    h.shift("A");
    assert_eq!(h.app.overlay(), Overlay::Confirm);
    h.changed(Applied { feed: None, category: Some(cat(77, "Science")) }, Some(feed_list()), None);
    assert_eq!(h.app.overlay(), Overlay::None);
    h.key("y");
    assert!(!h.calls.iter().any(|c| matches!(c, Effect::MarkAllRead { .. })));
}

#[test]
fn a_feed_thats_open_and_removed_leaves_nothing_to_read_and_the_picker_up() {
    let mut h = reading(Selection::Feed { id: 10 }, "Alpha", Capabilities::EVERYTHING);
    h.cursor_on("Alpha");
    h.key("d");
    h.key("y");
    h.hold_next_page = true;
    let list = h.list_without(10);
    h.changed(Applied::default(), Some(list), None);
    assert_eq!(h.app.queue().len(), 0);
    assert_eq!(h.app.current(), None);
    assert_eq!(h.app.selection_label(), "");
    assert_eq!(h.notice(), "Removed “Alpha” and its articles.");
    h.key("Escape");
    assert_eq!(h.app.overlay(), Overlay::Picker);
    h.key("Escape");
    assert_eq!(h.app.overlay(), Overlay::Picker, "nothing to go back to");
    assert_eq!(h.app.picker_list(), &PickerList::Ready(h.counts_as_listed()));
}

impl Harness {
    /// What the picker shows after the last answer to `FeedCounts`: the fake's counts.
    fn counts_as_listed(&self) -> Counts {
        Counts { feeds: self.counts.clone(), starred: self.starred }
    }
}

#[test]
fn a_feed_removed_while_its_list_is_being_read_with_the_panel_closed_goes_back_to_the_picker() {
    let mut h = reading(Selection::Feed { id: 10 }, "Alpha", Capabilities::EVERYTHING);
    h.cursor_on("Alpha");
    h.key("d");
    h.key("y");
    h.key("Escape");
    h.key("Escape");
    assert_eq!(h.app.overlay(), Overlay::None, "reading it still");
    let list = h.list_without(10);
    h.changed(Applied::default(), Some(list), None);
    assert_eq!(h.app.overlay(), Overlay::Picker);
    assert_eq!(h.toast(), Some("Removed “Alpha” and its articles."));
    assert_eq!(h.app.current(), None);
}

#[test]
fn a_removed_feeds_articles_leave_a_longer_list_and_your_place_stays() {
    let mut h = reading(Selection::All, "All unread", Capabilities::EVERYTHING);
    h.key("Escape");
    h.key("Escape");
    for _ in 0..3 {
        h.key("n");
    }
    assert_eq!(h.app.current().map(|e| e.id), Some(4));
    h.key("p");
    h.key("m");
    h.cursor_on("Alpha");
    h.key("d");
    h.key("y");
    let list = h.list_without(10);
    h.changed(Applied::default(), Some(list), None);
    let ids: Vec<i64> = h.app.queue().iter().map(|e| e.id).collect();
    assert_eq!(ids, [4, 5]);
    assert_eq!(h.app.current().map(|e| e.id), Some(4), "the same article");
    assert_eq!(h.app.position(), "1 / 2");
}

#[test]
fn removing_the_feed_of_the_article_on_screen_puts_the_next_one_there() {
    let mut h = reading(Selection::All, "All unread", Capabilities::EVERYTHING);
    h.cursor_on("Alpha");
    h.key("d");
    h.key("y");
    let list = h.list_without(10);
    h.changed(Applied::default(), Some(list), None);
    assert_eq!(h.app.current().map(|e| e.id), Some(4));
    h.key("Escape");
    h.key("Escape");
    assert_eq!(h.app.overlay(), Overlay::None);
    assert_eq!(h.app.screen(), Screen::Article);
}

#[test]
fn an_answer_for_an_article_that_moved_up_the_queue_changes_nothing_else() {
    let mut h = reading(Selection::All, "All unread", Capabilities::EVERYTHING);
    h.key("Escape");
    h.key("Escape");
    h.key("n");
    let then = AfterOpen::MarkRead { index: 1, id: 2 };
    h.key("p");
    h.key("m");
    h.cursor_on("Alpha");
    h.key("d");
    h.key("y");
    let list = h.list_without(10);
    h.changed(Applied::default(), Some(list), None);
    // Index 1 is article 5 now, and `o` was pressed on article 2, which is gone.
    let fx = h.app.apply(Outcome::OpenedInBrowser { then, result: Ok(()) });
    assert!(fx.is_empty());
    assert!(h.app.queue().iter().all(|e| e.status == EntryStatus::Unread));
}

#[test]
fn a_page_on_its_way_can_bring_back_nothing_of_a_removed_feed() {
    let mut h = reading(Selection::All, "All unread", Capabilities::EVERYTHING);
    let load = h.app.latest_load;
    h.cursor_on("Alpha");
    h.key("d");
    h.key("y");
    let list = h.list_without(10);
    h.changed(Applied::default(), Some(list), None);
    assert_ne!(h.app.latest_load, load);
    let stale = EntryPage { total: 9, entries: of_two_feeds() };
    h.app.apply(Outcome::Page { load, arrivals: false, result: Ok(stale) });
    assert_eq!(h.app.queue().len(), 2);
}

#[test]
fn the_last_articles_of_a_list_going_loads_what_comes_after_them() {
    let mut h = reading(Selection::Category { id: 1 }, "News", Capabilities::EVERYTHING);
    h.fake.entries.retain(|e| e.feed.id == 12);
    h.app.has_more = true;
    h.app.queue.retain(|e| e.feed.id == 10);
    h.cursor_on("Alpha");
    h.key("d");
    h.key("y");
    let before = h.entries_calls().len();
    let list = h.list_without(10);
    h.changed(Applied::default(), Some(list), None);
    assert_eq!(h.entries_calls().len(), before + 1, "it starts again, as there's nothing to page on from");
    assert!(h.app.current().is_some());
}

#[test]
fn a_miniflux_category_that_goes_takes_its_feeds_out_of_every_open_list() {
    let mut h = reading(Selection::All, "All unread", Capabilities::EVERYTHING);
    h.cursor_on("News");
    h.key("d");
    h.key("y");
    let mut after = feed_list();
    after.counts.feeds.retain(|f| f.feed.category.id != 1);
    after.categories.retain(|c| c.id != 1);
    h.changed(Applied::default(), Some(after), None);
    assert_eq!(h.app.queue().iter().map(|e| e.id).collect::<Vec<_>>(), [4, 5]);
    assert!(h.notice().contains("with its 2 feeds and their articles"), "{}", h.notice());

    let mut h = reading(Selection::Feed { id: 10 }, "Alpha", Capabilities::EVERYTHING);
    h.cursor_on("News");
    h.key("d");
    h.key("y");
    h.changed(Applied::default(), Some(FeedList::default()), None);
    assert_eq!(h.app.current(), None, "the feed being read went with it");
    assert_eq!(h.app.selection_label(), "");
}

#[test]
fn a_freshrss_category_that_goes_keeps_its_articles_and_only_its_own_open_list_closes() {
    let mut h = reading(Selection::All, "All unread", FRESHRSS);
    h.cursor_on("News");
    h.key("d");
    h.key("y");
    let mut after = feed_list();
    after.categories.retain(|c| c.id != 1);
    h.changed(Applied::default(), Some(after), None);
    assert_eq!(h.app.queue().len(), 5, "the feeds are still there");
    assert!(h.notice().contains("Its 2 feeds are kept"), "{}", h.notice());

    let mut h = reading(Selection::Feed { id: 10 }, "Alpha", FRESHRSS);
    h.cursor_on("News");
    h.key("d");
    h.key("y");
    h.changed(Applied::default(), Some(feed_list()), None);
    assert_eq!(h.app.selection_label(), "Alpha", "a feed outlives its category");

    let mut h = reading(Selection::Category { id: 1 }, "News", FRESHRSS);
    h.cursor_on("News");
    h.key("d");
    h.key("y");
    h.changed(Applied::default(), Some(feed_list()), None);
    assert_eq!(h.app.selection_label(), "", "there's no such list any more");
}

#[test]
fn a_feed_that_moves_out_of_the_open_category_takes_its_articles_with_it() {
    let mut h = reading(Selection::Category { id: 1 }, "News", Capabilities::EVERYTHING);
    h.cursor_on("Alpha");
    h.key("m");
    h.key("j");
    h.key("Enter");
    h.changed(Applied { feed: None, category: Some(cat(2, "Tech")) }, Some(feed_list()), None);
    assert_eq!(h.app.queue().iter().map(|e| e.id).collect::<Vec<_>>(), [4, 5]);

    let mut h = reading(Selection::All, "All unread", Capabilities::EVERYTHING);
    h.cursor_on("Alpha");
    h.key("m");
    h.key("j");
    h.key("Enter");
    h.changed(Applied { feed: None, category: Some(cat(2, "Tech")) }, Some(feed_list()), None);
    assert_eq!(h.app.queue().len(), 5, "still in All");
}

#[test]
fn a_list_that_wont_come_after_a_change_still_counts_as_done_and_is_asked_for_again() {
    let mut h = managing(Capabilities::EVERYTHING);
    h.cursor_on("Alpha");
    h.key("r");
    let fx = h.app.submit_manage("Renamed");
    h.run(fx);
    h.changed(Applied::default(), None, None);
    assert!(h.notice().contains("couldn't get the list back"), "{}", h.notice());
    assert!(h.app.manage_view().unwrap().worrying);
}

#[test]
fn f_asks_a_server_that_can_and_says_what_to_expect() {
    let mut h = managing(Capabilities::EVERYTHING);
    h.key("f");
    assert!(h.calls.contains(&Effect::RefreshFeeds));
    assert!(h.notice().contains("fetching"), "{}", h.notice());

    let mut h = managing(FRESHRSS);
    h.key("f");
    assert!(!h.calls.contains(&Effect::RefreshFeeds));
    assert_eq!(h.manage_stage(), "list");
}

#[test]
fn a_click_on_a_row_goes_there_and_on_a_category_to_move_to_it_moves() {
    let mut h = managing(Capabilities::EVERYTHING);
    assert!(h.app.manage_row(2).is_empty());
    assert_eq!(h.app.manage_view().unwrap().cursor, 2);
    h.cursor_on("Alpha");
    h.key("m");
    let fx = h.app.manage_row(1);
    h.run(fx);
    assert_eq!(h.changes(), [FeedChange::MoveFeed { id: 10, to: Destination::Existing(cat(2, "Tech")) }]);
}

#[test]
fn the_panel_keeps_its_keys_and_help_comes_back_to_it() {
    let mut h = managing(Capabilities::EVERYTHING);
    h.key("?");
    assert_eq!(h.app.overlay(), Overlay::Help);
    assert!(h.app.help_lines().iter().any(|(_, action)| action.contains("(managing feeds)")));
    h.key("Escape");
    assert_eq!(h.app.overlay(), Overlay::Manage);
    h.key("/");
    h.key(",");
    h.key("Enter");
    assert_eq!(h.app.overlay(), Overlay::Manage);
    assert!(h.app.settings().is_none());
}

#[test]
fn a_list_that_fails_says_so_where_the_rows_would_be_and_esc_still_closes() {
    let mut h = managing(Capabilities::EVERYTHING);
    h.key("Escape");
    h.key("m");
    let serial = h.app.latest_list;
    h.app.manage = Some(Manage::default());
    h.app.apply(Outcome::FeedList { serial, result: Err("Could not reach Miniflux".into()) });
    let view = h.app.manage_view().unwrap();
    assert_eq!((view.stage, view.error.as_str()), ("loading", "Could not reach Miniflux"));
    h.key("Escape");
    assert_eq!(h.app.overlay(), Overlay::Picker);
}

// ---- Secrets -----------------------------------------------------------------------------------------------------

const EVERYTHING: Status =
    Status { looking: false, configured: true, readeck: true, capabilities: Capabilities::EVERYTHING };
const NOTHING: Status =
    Status { looking: false, configured: false, readeck: false, capabilities: Capabilities::EVERYTHING };
const LOCKED: &str = "Your keyring is locked. Press r to try again.";

fn looking(answers: Vec<(Status, Option<String>)>) -> Harness {
    let mut h = Harness::new(items(2));
    h.status = Status { looking: true, ..NOTHING };
    h.secrets_answer = answers.into();
    h
}

fn save_went_to(place: Place) -> Harness {
    let mut h = Harness::new(items(2));
    h.save_result = Ok(Saved { status: EVERYTHING, username: None, readeck_problem: None, place: Some(place) });
    h.start();
    h.key(",");
    h
}

impl Harness {
    fn save_form(&mut self) {
        let fx = self.app.submit_settings(SettingsValues::default());
        self.run(fx);
    }
}

#[test]
fn a_secret_in_a_store_is_looked_for_before_anything_else() {
    let mut h = Harness::new(items(2));
    h.status = Status { looking: true, ..NOTHING };
    let fx = h.app.start(h.status);
    assert_eq!(fx, [Effect::ReadSecrets], "the start waits for nothing, and asks no server yet");
    assert_eq!(h.app.overlay(), Overlay::Picker);
    assert_eq!(h.app.picker_list(), &PickerList::Loading);
}

#[test]
fn the_app_stays_usable_while_a_store_takes_its_time() {
    let mut h = Harness::new(items(2));
    let fx = h.app.start(Status { looking: true, ..NOTHING });
    assert_eq!(fx, [Effect::ReadSecrets]);
    h.key("?");
    assert_eq!(h.app.overlay(), Overlay::Help, "what needs no secret works");
    h.key("Escape");
    h.key(",");
    assert_eq!(h.app.overlay(), Overlay::Picker, "the form would show what's in the store as empty");
    h.key("h");
    assert!(h.calls.is_empty(), "no server is asked while the secret is on its way");
}

#[test]
fn a_secret_that_is_found_opens_the_feeds() {
    let mut h = looking(vec![(EVERYTHING, None)]);
    h.start();
    assert!(h.picker_loaded());
    assert!(h.app.readeck());
}

#[test]
fn a_secret_nobody_has_opens_the_form_as_it_would_have() {
    let mut h = looking(vec![(NOTHING, None)]);
    h.start();
    assert_eq!(h.app.overlay(), Overlay::Onboarding);
}

#[test]
fn a_locked_keyring_never_looks_like_an_unconfigured_app() {
    let mut h = looking(vec![(NOTHING, Some(LOCKED.into()))]);
    h.start();
    assert_eq!(h.app.overlay(), Overlay::Picker, "not first run");
    assert_eq!(h.app.picker_list(), &PickerList::Error(LOCKED.into()));
}

#[test]
fn r_asks_the_stores_again() {
    let mut h = looking(vec![(NOTHING, Some(LOCKED.into())), (NOTHING, Some(LOCKED.into())), (EVERYTHING, None)]);
    h.start();
    h.key("r");
    assert_eq!(h.app.picker_list(), &PickerList::Error(LOCKED.into()), "still locked");
    h.key("r");
    assert!(h.picker_loaded());
}

#[test]
fn a_stuck_keyring_can_be_got_around_in_settings() {
    let mut h = looking(vec![(NOTHING, Some(LOCKED.into())), (NOTHING, Some(LOCKED.into()))]);
    h.save_result = Ok(saved(EVERYTHING));
    h.start();
    h.key(",");
    assert_eq!(h.app.overlay(), Overlay::Settings, "a keyring that's gone for good doesn't shut anyone out");
    h.save_form();
    assert!(h.picker_loaded(), "what was typed is used, without going back to the store");
    assert!(h.secrets_answer.len() == 1, "and it isn't asked again");
}

#[test]
fn leaving_the_form_tries_a_stuck_keyring_again() {
    let mut h = looking(vec![(NOTHING, Some(LOCKED.into())), (EVERYTHING, None)]);
    h.start();
    h.key(",");
    h.key("Escape");
    assert!(h.picker_loaded());
}

#[test]
fn a_store_that_only_held_something_else_says_so_and_stops_nothing() {
    let mut h = looking(vec![(Status { readeck: false, ..EVERYTHING }, Some(LOCKED.into()))]);
    h.start();
    assert!(h.picker_loaded());
    assert_eq!(h.app.toast().map(|t| t.message.as_str()), Some(LOCKED));
    assert!(!h.app.readeck());
}

#[test]
fn a_late_answer_changes_nothing() {
    let mut h = Harness::new(items(2));
    h.start();
    let fx = h.app.apply(Outcome::SecretsRead { status: NOTHING, problem: Some(LOCKED.into()) });
    assert!(fx.is_empty());
    assert!(h.picker_loaded());
}

#[test]
fn going_to_the_file_is_said_once_and_remembered() {
    let path = "/home/me/.local/state/onward/secrets.toml";
    let mut h = save_went_to(Place::File(path.into()));
    h.save_form();
    assert_eq!(h.app.overlay(), Overlay::Confirm);
    assert!(h.app.confirm_text().contains(path), "{}", h.app.confirm_text());
    assert!(h.app.confirm_text().contains("only you can read"), "{}", h.app.confirm_text());
    assert!(h.app.confirm_told());
    let remembered = h.app.prefs();
    assert!(remembered.told_file);
    h.key("Enter");
    assert_eq!(h.app.overlay(), Overlay::Picker);

    let mut next_run = Harness::new(items(2));
    next_run.app = App::new(remembered, false);
    next_run.save_result = h.save_result.clone();
    next_run.start();
    next_run.key(",");
    next_run.save_form();
    assert_eq!(next_run.app.overlay(), Overlay::Picker, "once, across runs");
}

#[test]
fn saying_it_saves_it() {
    let mut h = save_went_to(Place::File("/x".into()));
    let fx = h.app.submit_settings(SettingsValues::default());
    let Some(Effect::SaveSettings(_)) = fx.first() else { panic!("a save") };
    let answer = Outcome::SettingsSaved(h.save_result.clone());
    let fx = h.app.apply(answer);
    assert!(fx.iter().any(|e| matches!(e, Effect::SavePrefs(p) if p.told_file)), "{fx:?}");
}

#[test]
fn omarchys_keyring_gets_the_truth_and_only_there() {
    let mut h = save_went_to(Place::Keyring { omarchy: true });
    h.save_form();
    assert_eq!(h.app.overlay(), Overlay::Confirm);
    assert!(h.app.confirm_text().contains("unencrypted"), "{}", h.app.confirm_text());
    assert!(h.app.confirm_text().contains("until it has one"), "{}", h.app.confirm_text());
    assert!(h.app.prefs().told_omarchy);

    let mut h = save_went_to(Place::Keyring { omarchy: false });
    h.save_form();
    assert_eq!(h.app.overlay(), Overlay::Picker, "nothing to say about a keyring that's what it seems");
    assert_eq!(h.app.prefs(), Prefs::default());
}

#[test]
fn the_notice_goes_with_any_key_that_answers_a_question() {
    for key in ["Enter", "y", "n", "Escape"] {
        let mut h = save_went_to(Place::File("/x".into()));
        h.save_form();
        h.key(key);
        assert_eq!(h.app.overlay(), Overlay::Picker, "{key}");
    }
}

#[test]
fn ctrl_0_keeps_what_has_been_said() {
    let mut prefs = Prefs { told_file: true, told_omarchy: true, font_size: 30, ..Prefs::default() };
    prefs.reset();
    assert!(prefs.told_file && prefs.told_omarchy);
    assert_eq!(prefs.font_size, Prefs::default().font_size);
}

// ---- Readeck through a browser -----------------------------------------------------------------------------------
//
// The app and a real Backend against a stand-in Readeck answering as 0.23.5 did. Waits don't wait: `tick` is the time
// being up, and a poll that a test holds back is one still on its way.

use crate::backend::Backend;
use crate::secrets::tests::{Act, Scripted, id, scripted};
use crate::secrets::{Failure, Kind};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const CODE_BODY: &str = r#""device_code":"the-device-code","user_code":"VSCZLVQJ","expires_in":300,"interval":5"#;
const TOKEN_BODY: &str = r#"{"id":"x","access_token":"the-token","token_type":"Bearer","scope":"bookmarks:write"}"#;

fn json_reply(code: u16, body: &str) -> ResponseTemplate {
    ResponseTemplate::new(code).set_body_raw(body.to_string(), "application/json; charset=utf-8")
}

/// A Readeck with everything up to a code in place, at its own address, and polls to be answered.
async fn readeck() -> MockServer {
    let server = MockServer::start().await;
    device_at(&server, &format!("{}/device", server.uri())).await;
    server
}

async fn device_at(server: &MockServer, address: &str) {
    let info = r#"{"version":{"canonical":"0.23.5","release":"0.23.5","build":""},"features":["oauth"]}"#;
    Mock::given(method("GET")).and(path("/api/info")).respond_with(json_reply(200, info)).mount(server).await;
    let client = r#"{"client_id":"urn:uuid:4c1d","token_endpoint_auth_method":"none"}"#;
    let register = Mock::given(method("POST")).and(path("/api/oauth/client"));
    register.respond_with(json_reply(201, client)).mount(server).await;
    let device = format!(
        r#"{{{CODE_BODY},"verification_uri":"{address}","verification_uri_complete":"{address}?user_code=VSCZLVQJ"}}"#
    );
    let ask = Mock::given(method("POST")).and(path("/api/oauth/device"));
    ask.respond_with(json_reply(200, &device)).mount(server).await;
    let profile = Mock::given(method("GET")).and(path("/api/profile"));
    profile.respond_with(ResponseTemplate::new(403)).mount(server).await;
}

/// What the next polls are answered with, in order. The last one lasts.
async fn polls_answer(server: &MockServer, answers: &[(u16, &str)]) {
    server.reset().await;
    device_at(server, &format!("{}/device", server.uri())).await;
    for (n, (code, body)) in answers.iter().enumerate() {
        let mock = Mock::given(method("POST")).and(path("/api/oauth/token")).respond_with(json_reply(*code, body));
        let mock = if n + 1 == answers.len() { mock } else { mock.up_to_n_times(1) };
        mock.mount(server).await;
    }
}

async fn poll_count(server: &MockServer) -> usize {
    let requests = server.received_requests().await.unwrap();
    requests.iter().filter(|r| r.url.path() == "/api/oauth/token").count()
}

const PENDING: (u16, &str) = (400, r#"{"error":"authorization_pending"}"#);
const SLOW_DOWN: (u16, &str) = (400, r#"{"error":"slow_down"}"#);
const DENIED: (u16, &str) = (400, r#"{"error":"access_denied"}"#);
const EXPIRED: (u16, &str) = (400, r#"{"error":"expired_token"}"#);
const CLIENT_GONE: (u16, &str) = (401, r#"{"error":"invalid_client"}"#);
const APPROVED: (u16, &str) = (201, TOKEN_BODY);

struct Connecting {
    app: App,
    backend: Backend,
    store: Scripted,
    dir: tempfile::TempDir,
    /// The feed server the config names: nothing there answers.
    feeds: String,
    /// What each wait was for.
    waits: Vec<u64>,
    due: Option<Outcome>,
    /// Polls left on their way when `hold_polls` is set.
    held: Vec<Effect>,
    hold_polls: bool,
    held_keep: Option<Effect>,
    hold_keep: bool,
    opened: Vec<String>,
    /// What's in the Readeck URL field.
    address: String,
}

impl Connecting {
    /// With Settings open, on a config that has a working feed server and no Readeck.
    async fn new(server: &MockServer) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let feeds = "http://127.0.0.1:1/feeds".to_string();
        let config = format!("[miniflux]\nurl = \"{feeds}\"\napi_key = \"feed-key\"\n");
        std::fs::write(dir.path().join("config.toml"), config).unwrap();
        let store = Scripted::default();
        let backend = Backend::with_secrets(false, dir.path(), scripted(dir.path(), &store));
        let status = backend.status();
        let mut app = App::new(Prefs::default(), false);
        let fx = app.start(status);
        let address = server.uri();
        let mut h = Connecting {
            app,
            backend,
            store,
            dir,
            feeds,
            waits: Vec::new(),
            due: None,
            held: Vec::new(),
            hold_polls: false,
            held_keep: None,
            hold_keep: false,
            opened: Vec::new(),
            address,
        };
        h.pump(fx).await;
        h.key(",").await;
        assert_eq!(h.app.overlay(), Overlay::Settings);
        h
    }

    async fn key(&mut self, key: &str) {
        let (_, fx) = self.app.key(&KeyPress { key: key.into(), ..KeyPress::default() });
        self.pump(fx).await;
    }

    async fn pump(&mut self, fx: Vec<Effect>) {
        let mut queue = VecDeque::from(fx);
        while let Some(effect) = queue.pop_front() {
            match effect {
                Effect::AwaitReadeck { attempt, millis } => {
                    self.waits.push(millis);
                    self.due = Some(Outcome::ReadeckPollDue { attempt });
                }
                Effect::OpenInBrowser { url, .. } => self.opened.push(url),
                Effect::PollReadeck { .. } if self.hold_polls => self.held.push(effect),
                Effect::KeepReadeck { .. } if self.hold_keep => self.held_keep = Some(effect),
                Effect::ReloadPickerAfter { .. } => {}
                effect => {
                    if let Some(outcome) = self.backend.run(effect).await {
                        queue.extend(self.app.apply(outcome));
                    }
                }
            }
        }
    }

    /// The wait's over.
    async fn tick(&mut self) {
        let due = self.due.take().expect("a wait");
        let fx = self.app.apply(due);
        self.pump(fx).await;
    }

    /// The first row.
    async fn press(&mut self, row: usize) {
        let fx = self.app.readeck_row(row, &self.address.clone());
        self.pump(fx).await;
    }

    async fn run_held(&mut self) {
        self.hold_polls = false;
        for effect in std::mem::take(&mut self.held) {
            self.pump(vec![effect]).await;
        }
    }

    async fn run_held_keep(&mut self) {
        self.hold_keep = false;
        let effect = self.held_keep.take().expect("a token being kept");
        self.pump(vec![effect]).await;
    }

    fn view(&self) -> GrantView {
        self.app.readeck_view()
    }

    fn labels(&self) -> Vec<String> {
        self.view().rows.into_iter().map(|row| row.label).collect()
    }

    fn config_text(&self) -> String {
        std::fs::read_to_string(self.dir.path().join("config.toml")).unwrap()
    }

    fn stored(&self, url: &str) -> Option<String> {
        self.store.holds(&id(Kind::Readeck, url, ""))
    }

    /// Everything the windows are handed, and what `Debug` makes of the rest.
    fn everything_shown(&self) -> String {
        format!("{:?} {:?} {}", self.view(), self.app.settings(), serde_json::to_string(&self.view()).unwrap())
    }
}

#[tokio::test]
async fn approving_in_the_browser_connects_with_nothing_to_paste_or_save() {
    let server = readeck().await;
    polls_answer(&server, &[PENDING, SLOW_DOWN, PENDING, APPROVED]).await;
    let mut h = Connecting::new(&server).await;
    assert_eq!(h.view().stage, "idle");
    assert_eq!(h.labels(), ["Connect in the browser"]);

    h.press(0).await;
    assert_eq!(h.view().stage, "waiting");
    assert_eq!(h.view().code, "VSCZLVQJ");
    assert_eq!(h.view().address, format!("{}/device", server.uri()));
    assert_eq!(h.labels(), ["Open in browser", "Start again"]);

    h.press(0).await;
    assert_eq!(h.opened, [format!("{}/device?user_code=VSCZLVQJ", server.uri())], "with the code filled in");

    for _ in 0..4 {
        assert!(!h.everything_shown().contains("the-device-code"), "{}", h.everything_shown());
        h.tick().await;
    }
    assert_eq!(h.waits, [5000, 5000, 10_000, 10_000], "the interval, and 5 s more from a slow_down on");
    assert_eq!(h.view().stage, "kept");
    assert!(h.view().text.contains(&server.uri()), "{}", h.view().text);
    assert!(h.view().rows.is_empty());
    assert!(!h.app.settings().unwrap().checking);
    assert!(h.due.is_none(), "that's the end of the polling");

    assert!(h.app.readeck(), "the bookmark button is there straight away");
    assert_eq!(h.stored(&server.uri()).as_deref(), Some("the-token"));
    let config = h.config_text();
    assert!(config.contains(&server.uri()) && !config.contains("the-token"), "{config}");
    assert!(!h.everything_shown().contains("the-token"), "{}", h.everything_shown());
}

#[tokio::test]
async fn being_turned_down_is_said_and_the_polling_ends() {
    let server = readeck().await;
    polls_answer(&server, &[PENDING, DENIED]).await;
    let mut h = Connecting::new(&server).await;
    h.press(0).await;
    h.tick().await;
    h.tick().await;
    assert_eq!(h.view().stage, "failed");
    assert!(h.view().failed && h.view().text.contains("turned down"), "{:?}", h.view());
    assert_eq!(h.labels(), ["Start again"]);
    assert!(h.due.is_none());
    assert!(!h.config_text().contains("readeck"));
}

#[tokio::test]
async fn a_code_or_a_client_that_ran_out_can_be_started_again() {
    for (answer, says) in [(EXPIRED, "ran out"), (CLIENT_GONE, "10 minutes")] {
        let server = readeck().await;
        polls_answer(&server, &[answer]).await;
        let mut h = Connecting::new(&server).await;
        h.press(0).await;
        h.tick().await;
        assert!(h.view().text.contains(says), "{}", h.view().text);
        assert_eq!(h.labels(), ["Start again"]);

        polls_answer(&server, &[APPROVED]).await;
        h.press(0).await;
        assert_eq!(h.view().stage, "waiting", "a new client and a new code");
        h.tick().await;
        assert_eq!(h.view().stage, "kept");
    }
}

#[tokio::test]
async fn a_readeck_that_cant_look_up_public_addresses_points_at_the_token_field() {
    let server = MockServer::start().await;
    let info = Mock::given(method("GET")).and(path("/api/info"));
    info.respond_with(json_reply(200, r#"{"features":["oauth"]}"#)).mount(&server).await;
    let refused = r#"{"error":"invalid_client_metadata","error_description":"error on field \"client_uri\": invalid client URI"}"#;
    let register = Mock::given(method("POST")).and(path("/api/oauth/client"));
    register.respond_with(json_reply(400, refused)).mount(&server).await;
    let mut h = Connecting::new(&server).await;
    h.press(0).await;
    assert_eq!(h.view().stage, "token_only");
    let text = h.view().text;
    assert!(text.contains("look up public") && text.contains("Profile → API Tokens"), "{text}");
    assert!(h.view().rows.is_empty(), "another go would get the same answer");
}

#[tokio::test]
async fn a_server_with_no_grant_is_left_to_the_token_field_until_the_address_changes() {
    let server = MockServer::start().await;
    let info = Mock::given(method("GET")).and(path("/api/info"));
    info.respond_with(json_reply(200, r#"{"features":[]}"#)).mount(&server).await;
    let mut h = Connecting::new(&server).await;
    h.press(0).await;
    assert_eq!(h.view().stage, "token_only");
    assert!(h.view().text.contains("0.21"), "{}", h.view().text);
    assert!(h.view().rows.is_empty());
    h.app.readeck_edited("https://other.example.com");
    assert_eq!(h.view().stage, "idle");
    assert_eq!(h.labels(), ["Connect in the browser"]);
}

#[tokio::test]
async fn not_reaching_the_server_is_said_and_can_be_tried_again() {
    let server = readeck().await;
    let mut h = Connecting::new(&server).await;
    h.address = "http://127.0.0.1:1".into();
    h.press(0).await;
    assert_eq!(h.view().stage, "failed");
    assert!(h.view().text.starts_with("Could not reach Readeck"), "{}", h.view().text);
    assert_eq!(h.labels(), ["Start again"]);
}

#[tokio::test]
async fn an_address_that_wont_do_goes_no_further() {
    let server = MockServer::start().await;
    let mut h = Connecting::new(&server).await;
    for (address, says) in [("", "Enter your Readeck"), ("readeck.example.com", "not a valid URL")] {
        h.address = address.into();
        h.press(0).await;
        assert_eq!(h.view().stage, "failed");
        assert!(h.view().text.contains(says), "{}", h.view().text);
        h.app.readeck_edited("something else");
        assert_eq!(h.view().stage, "idle");
    }
    assert!(server.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn one_failed_poll_isnt_the_end_and_the_code_running_out_is() {
    let server = readeck().await;
    polls_answer(&server, &[(502, "<html>Bad gateway</html>"), PENDING]).await;
    let mut h = Connecting::new(&server).await;
    h.press(0).await;
    h.tick().await;
    assert_eq!(h.view().stage, "waiting");
    assert_eq!(h.view().note, "Readeck answered with HTTP 502. Still trying.");
    h.tick().await;
    assert_eq!(h.view().note, "", "an answer again");

    polls_answer(&server, &[(502, "<html>Bad gateway</html>")]).await;
    for _ in 2..60 {
        assert_eq!(h.view().stage, "waiting");
        h.tick().await;
    }
    assert_eq!(h.view().stage, "failed", "{:?}", h.view());
    assert!(h.view().text.contains("ran out") && h.view().text.contains("HTTP 502"), "{}", h.view().text);
    assert_eq!(h.labels(), ["Start again"]);
    assert!(h.due.is_none());
}

#[tokio::test]
async fn slow_polls_use_up_the_codes_time_too() {
    let server = readeck().await;
    let mut h = Connecting::new(&server).await;
    h.press(0).await;
    let attempt = h.app.grant.attempt();
    let answer = Poll::Failed("no".into());
    let fx = h.app.apply(Outcome::ReadeckPolled { attempt, answer, took_ms: 295_000 });
    assert!(fx.is_empty(), "{fx:?}");
    assert_eq!(h.view().stage, "failed");
}

#[tokio::test]
async fn closing_the_form_stops_the_polling_and_a_late_answer_changes_nothing() {
    let server = readeck().await;
    polls_answer(&server, &[APPROVED]).await;
    let mut h = Connecting::new(&server).await;
    h.press(0).await;
    h.hold_polls = true;
    h.tick().await;
    assert_eq!(h.held.len(), 1, "one on its way");
    h.key("Escape").await;
    assert_ne!(h.app.overlay(), Overlay::Settings);
    h.run_held().await;
    assert!(h.due.is_none());
    assert!(!h.app.readeck());
    assert_eq!(h.stored(&server.uri()), None);
    assert!(!h.config_text().contains("readeck"));
    let polls = poll_count(&server).await;

    // A wait that's over after the form closed sends nothing.
    let mut h = Connecting::new(&server).await;
    h.press(0).await;
    h.key("Escape").await;
    h.tick().await;
    assert_eq!(poll_count(&server).await, polls);
    assert_eq!(h.view().stage, "idle");
}

#[tokio::test]
async fn a_token_only_counts_for_the_address_it_was_asked_for() {
    let server = readeck().await;
    polls_answer(&server, &[APPROVED]).await;
    let mut h = Connecting::new(&server).await;
    h.press(0).await;
    h.app.readeck_edited(&format!("  {}/ ", server.uri()));
    assert_eq!(h.view().stage, "waiting", "the same address");
    h.hold_polls = true;
    h.tick().await;
    h.app.readeck_edited("https://elsewhere.example.com");
    assert_eq!(h.view().stage, "idle");
    h.run_held().await;
    assert_eq!(h.view().stage, "idle");
    assert!(h.due.is_none());
    assert_eq!(h.stored(&server.uri()), None);
    assert_eq!(h.stored("https://elsewhere.example.com"), None);
    assert!(!h.config_text().contains("readeck"));
}

#[tokio::test]
async fn starting_again_leaves_the_old_attempts_answers_behind() {
    let server = readeck().await;
    polls_answer(&server, &[DENIED]).await;
    let mut h = Connecting::new(&server).await;
    h.press(0).await;
    h.hold_polls = true;
    h.tick().await;
    h.hold_polls = false;
    h.press(1).await;
    assert_eq!(h.labels()[1], "Start again");
    h.run_held().await;
    assert_eq!(h.view().stage, "waiting", "the old attempt's no is about the old code");
    assert_eq!(h.waits, [5000, 5000]);
}

#[tokio::test]
async fn a_token_readeck_wont_take_isnt_kept() {
    let server = MockServer::start().await;
    device_at(&server, "http://x/").await;
    server.reset().await;
    let info = Mock::given(method("GET")).and(path("/api/info"));
    info.respond_with(json_reply(200, r#"{"features":["oauth"]}"#)).mount(&server).await;
    let register = Mock::given(method("POST")).and(path("/api/oauth/client"));
    register.respond_with(json_reply(201, r#"{"client_id":"c"}"#)).mount(&server).await;
    let device = format!(r#"{{{CODE_BODY},"verification_uri":"{}/device"}}"#, server.uri());
    let ask = Mock::given(method("POST")).and(path("/api/oauth/device"));
    ask.respond_with(json_reply(200, &device)).mount(&server).await;
    let poll = Mock::given(method("POST")).and(path("/api/oauth/token"));
    poll.respond_with(json_reply(201, TOKEN_BODY)).mount(&server).await;
    let profile = Mock::given(method("GET")).and(path("/api/profile"));
    profile.respond_with(ResponseTemplate::new(401)).mount(&server).await;
    let mut h = Connecting::new(&server).await;
    h.press(0).await;
    h.tick().await;
    assert_eq!(h.view().stage, "failed");
    assert!(h.view().text.contains("wouldn't take the token"), "{}", h.view().text);
    assert_eq!(h.stored(&server.uri()), None);
    assert!(!h.config_text().contains("readeck"));
}

#[tokio::test]
async fn an_address_on_another_host_is_shown_and_not_opened() {
    let server = MockServer::start().await;
    device_at(&server, "https://evil.example.com/device").await;
    let mut h = Connecting::new(&server).await;
    h.press(0).await;
    assert_eq!(h.view().stage, "waiting");
    assert_eq!(h.view().address, "https://evil.example.com/device");
    assert!(h.view().note.contains("won't open"), "{}", h.view().note);
    assert_eq!(h.labels(), ["Start again"]);
    assert!(h.opened.is_empty());
}

#[tokio::test]
async fn whats_half_typed_into_the_form_isnt_saved_along_with_the_token() {
    let server = readeck().await;
    polls_answer(&server, &[APPROVED]).await;
    let mut h = Connecting::new(&server).await;
    let queue_before = h.app.settings().unwrap().values.clone();
    h.press(0).await;
    h.tick().await;
    assert_eq!(h.view().stage, "kept");
    assert_eq!(h.app.overlay(), Overlay::Settings, "the form stays open, and the window keeps what's typed");
    assert_eq!(h.app.settings().unwrap().values, queue_before);
    assert_eq!(h.app.settings().unwrap().refill, 0, "the window's values aren't replaced");
    let config = h.config_text();
    assert!(config.contains(&h.feeds) && !config.contains("feed-key"), "the same save as any: {config}");
    assert_eq!(h.store.holds(&id(Kind::Miniflux, &h.feeds, "")).as_deref(), Some("feed-key"));
}

#[tokio::test]
async fn the_forms_save_goes_with_the_token_just_kept_unless_another_was_typed() {
    let server = readeck().await;
    polls_answer(&server, &[APPROVED]).await;
    for (typed, expected) in [("", "the-token"), ("pasted", "pasted")] {
        let mut h = Connecting::new(&server).await;
        h.press(0).await;
        h.tick().await;
        let values = SettingsValues {
            miniflux_url: h.feeds.clone(),
            miniflux_api_key: "feed-key".into(),
            readeck_url: format!("{}/", server.uri()),
            readeck_token: typed.into(),
            ..SettingsValues::default()
        };
        let fx = h.app.submit_settings(values);
        let [Effect::SaveSettings(sent)] = fx.as_slice() else { panic!("{fx:?}") };
        assert_eq!(sent.0.readeck_token, expected);
        assert_eq!(h.app.settings().unwrap().values.readeck_token, typed, "what the window has stays what it is");
    }
}

#[tokio::test]
async fn saving_the_form_ends_a_wait() {
    let server = readeck().await;
    polls_answer(&server, &[APPROVED]).await;
    let mut h = Connecting::new(&server).await;
    h.press(0).await;
    let fx = h.app.submit_settings(SettingsValues::default());
    assert!(matches!(fx.as_slice(), [Effect::SaveSettings(_)]), "{fx:?}");
    assert_eq!(h.view().stage, "idle");
    assert!(h.view().rows.is_empty(), "not while a save is on its way");
    assert!(h.app.readeck_row(0, &h.address.clone()).is_empty());
    h.tick().await;
    assert_eq!(poll_count(&server).await, 0);
}

#[tokio::test]
async fn a_save_waits_for_a_token_on_its_way_to_the_store() {
    let server = readeck().await;
    polls_answer(&server, &[APPROVED]).await;
    let mut h = Connecting::new(&server).await;
    h.press(0).await;
    h.hold_keep = true;
    h.tick().await;
    assert_eq!(h.view().stage, "keeping");
    assert!(h.app.settings().unwrap().checking);
    assert!(h.app.submit_settings(SettingsValues::default()).is_empty());
    h.app.readeck_edited("https://elsewhere.example.com");
    assert_eq!(h.view().stage, "keeping");
    h.run_held_keep().await;
    assert_eq!(h.view().stage, "kept");
    assert!(!h.app.settings().unwrap().checking);
}

#[tokio::test]
async fn a_token_kept_after_the_form_closed_still_counts() {
    let server = readeck().await;
    polls_answer(&server, &[APPROVED]).await;
    let mut h = Connecting::new(&server).await;
    h.press(0).await;
    h.hold_keep = true;
    h.tick().await;
    h.key("Escape").await;
    h.run_held_keep().await;
    assert!(h.app.readeck(), "it's in the store, whoever closed what");
    assert_eq!(h.app.toast().unwrap().message, "Readeck is connected");
    assert_eq!(h.stored(&server.uri()).as_deref(), Some("the-token"));
}

#[tokio::test]
async fn a_keyring_that_says_no_is_told_about_when_the_form_closes() {
    let server = readeck().await;
    polls_answer(&server, &[APPROVED]).await;
    let mut h = Connecting::new(&server).await;
    h.store.set(Act::Fails(Failure::Unreachable));
    h.press(0).await;
    h.tick().await;
    assert_eq!(h.view().stage, "kept");
    assert_eq!(h.app.overlay(), Overlay::Settings, "a notice now would take the form's place");
    assert!(!h.config_text().contains("the-token"));
    h.key("Escape").await;
    assert_eq!(h.app.overlay(), Overlay::Confirm);
    assert!(h.app.confirm_text().contains("a file only you can read"), "{}", h.app.confirm_text());
}

#[tokio::test]
async fn a_token_that_cant_be_stored_is_said_and_leaves_the_config_alone() {
    let server = readeck().await;
    polls_answer(&server, &[APPROVED]).await;
    let mut h = Connecting::new(&server).await;
    let before = h.config_text();
    std::fs::write(h.dir.path().join("config.toml"), "not = [toml").unwrap();
    h.press(0).await;
    h.tick().await;
    assert_eq!(h.view().stage, "failed");
    assert!(h.view().text.contains("config.toml"), "{}", h.view().text);
    assert_eq!(h.labels(), ["Start again"]);
    assert!(!h.app.settings().unwrap().checking);
    assert_eq!(h.stored(&server.uri()), None);
    assert_ne!(before, h.config_text());
    assert_eq!(h.config_text(), "not = [toml", "that file is someone's to fix, not this save's to replace");
}

#[tokio::test]
async fn going_to_the_address_box_stops_a_wait() {
    let server = readeck().await;
    let mut h = Connecting::new(&server).await;
    h.press(0).await;
    let fx = h.app.find_server();
    h.pump(fx).await;
    h.tick().await;
    assert_eq!(poll_count(&server).await, 0);
}
