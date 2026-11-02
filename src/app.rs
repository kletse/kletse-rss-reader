//! Everything the app does, without Qt and without a network: the reading queue and its paging, the feeds
//! picker, overlays, toasts, the confirm dialog, settings and the keyboard.
//!
//! It never waits. An action changes the state and returns the `Effect`s it needs (a server call, a scroll);
//! whoever runs them (`bridge.rs`, or the tests) hands each result back to `apply` as an `Outcome`, whenever it
//! arrives. Loads remember which selection they were for, so an answer that comes back after you moved on to
//! another selection changes nothing.
//!
//! Unread mode: unread items, oldest first; going back shows items read this session.
//! History mode: read items, newest first.
//! Both load a page at a time as you get near the end. At the end of the unread list, it looks once more for
//! items that arrived meanwhile.

use crate::keys::{self, Action};
use crate::miniflux::{Entry, EntryPage, EntryStatus, FeedCount, Selection};
use crate::paging::{self, Mode, PageRequest};
use crate::picker::{self, PickerItem};
use crate::prefs::Prefs;
use std::collections::HashMap;

pub const APP_NAME: &str = "Kletse RSS";
/// Load the next page when this few loaded items are left.
const PREFETCH: usize = 10;
/// Miniflux refreshes feeds in the background; the counts are reloaded a few times while it works.
const REFRESH_RELOAD_MS: [u64; 3] = [2000, 6000, 15_000];

/// Only one thing has the keyboard at a time; the settings form replaces the article.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Overlay {
    None,
    Picker,
    Help,
    Confirm,
    Settings,
    Onboarding,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PickerList {
    Loading,
    Error(String),
    Ready(Vec<FeedCount>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToastKind {
    Info,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Toast {
    pub message: String,
    pub kind: ToastKind,
    /// Bumped by every toast, so the window knows to restart its timer and can't dismiss a newer one.
    pub serial: u64,
}

/// What the backend reports about the configured servers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Status {
    /// Miniflux URL and key are present; otherwise show onboarding.
    pub configured: bool,
    /// Readeck is set up; otherwise hide the bookmark button.
    pub readeck: bool,
}

/// The settings form, flat. Empty Readeck fields mean "not set up".
/// No `Debug` on purpose: it holds the API key and token.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct SettingsValues {
    pub miniflux_url: String,
    pub miniflux_api_key: String,
    pub readeck_url: String,
    pub readeck_token: String,
}

pub struct SettingsForm {
    /// What the form starts from; the window keeps what is typed until it submits.
    pub values: SettingsValues,
    /// What was wrong with the config file at startup (it didn't parse, say).
    pub problem: Option<String>,
    pub error: String,
    pub checking: bool,
}

/// Puts an optimistic status change back if the server turns it down.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Revert {
    pub index: usize,
    pub id: i64,
    pub before: EntryStatus,
}

/// What follows opening a link in the browser.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AfterOpen {
    Nothing,
    /// `o`: say so, and mark the article read.
    MarkRead {
        index: usize,
    },
}

/// Work for whoever runs the app: server calls (each answered by an `Outcome`), and things the window does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    /// The first page of a selection, and (unread only) its newest unread id, together.
    OpenSelection {
        load: u64,
        selection: Selection,
        mode: Mode,
        request: PageRequest,
    },
    LoadPage {
        load: u64,
        selection: Selection,
        mode: Mode,
        request: PageRequest,
        arrivals: bool,
    },
    SetStatus {
        ids: Vec<i64>,
        status: EntryStatus,
        revert: Option<Revert>,
        announce: bool,
    },
    FetchContent {
        index: usize,
        id: i64,
    },
    FeedCounts,
    RefreshFeeds,
    ReloadPickerAfter {
        millis: u64,
    },
    SaveToReadeck {
        url: String,
    },
    OpenInBrowser {
        url: String,
        then: AfterOpen,
    },
    CountUnread {
        load: u64,
        selection: Selection,
        up_to: i64,
        later: Vec<i64>,
    },
    MarkAllRead {
        load: u64,
        selection: Selection,
        up_to: i64,
        later: Vec<i64>,
    },
    LoadSettings,
    SaveSettings(SettingsValuesBox),
    SavePrefs(Prefs),
    ScrollPage {
        up: bool,
    },
    Quit,
}

/// Settings travel in effects; this keeps them out of `Debug` output.
#[derive(Clone, PartialEq, Eq)]
pub struct SettingsValuesBox(pub SettingsValues);

impl std::fmt::Debug for SettingsValuesBox {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SettingsValues { .. }")
    }
}

/// The answer to an `Effect`. Errors are already the message to show.
pub enum Outcome {
    Opened { load: u64, result: Result<(EntryPage, i64), String> },
    Page { load: u64, arrivals: bool, result: Result<EntryPage, String> },
    StatusSet { status: EntryStatus, revert: Option<Revert>, announce: bool, result: Result<(), String> },
    ContentFetched { index: usize, id: i64, result: Result<String, String> },
    FeedCounts(Result<Vec<FeedCount>, String>),
    Refreshed(Result<(), String>),
    PickerReloadDue,
    SavedToReadeck(Result<(), String>),
    OpenedInBrowser { then: AfterOpen, result: Result<(), String> },
    UnreadCounted { load: u64, selection: Selection, up_to: i64, later: Vec<i64>, result: Result<i64, String> },
    MarkedAllRead { load: u64, result: Result<(), String> },
    SettingsLoaded { values: SettingsValues, problem: Option<String> },
    SettingsSaved(Result<Status, String>),
}

/// A key press, named as the web names keys (see `keys.rs`), plus what only the window knows.
#[derive(Debug, Clone, Default)]
pub struct KeyPress {
    pub key: String,
    pub shift: bool,
    /// Ctrl on Linux, ⌘ on macOS (Qt reports ⌘ as Ctrl there).
    pub ctrl: bool,
    pub alt: bool,
    /// The Super key on Linux, Control on macOS.
    pub meta: bool,
    /// A text field has the keyboard: never treat typing as shortcuts.
    pub typing: bool,
    /// The article is scrolled to its end (Space then moves on).
    pub at_bottom: bool,
}

/// What a confirmed dialog goes on to do.
#[derive(Debug, Clone)]
enum Pending {
    MarkAll { load: u64, selection: Selection, up_to: i64, later: Vec<i64> },
}

/// Both versions of an article whose full text was fetched this session, so `f` can switch back.
struct Versions {
    feed: String,
    full: String,
}

pub struct App {
    // ---- The queue ----
    queue: Vec<Entry>,
    /// `index == queue.len()` means "all caught up".
    index: usize,
    selection_label: String,
    loading: bool,
    mode: Mode,
    /// The whole selection, including pages not loaded yet.
    total: i64,
    /// More pages on the server.
    has_more: bool,
    /// Past the end of the unread list, looking for new arrivals.
    checking: bool,
    /// Opening another selection makes the loads still in flight no-ops: each remembers this and compares.
    latest_load: u64,
    opened: Option<Selection>,
    /// The newest unread item's id when the selection was opened (0 in history). Shift+A marks up to there.
    newest_at_open: i64,
    /// Saving to Readeck or fetching the full article: one at a time.
    busy: bool,
    loading_more: bool,
    /// Kept across selections: the server stores the fetched text, so a reloaded entry no longer has the feed's.
    versions: HashMap<i64, Versions>,

    // ---- The picker ----
    picker_list: PickerList,
    picker_cursor: usize,
    picker_mode: Mode,

    // ---- Overlays and messages ----
    overlay: Overlay,
    before_help: Overlay,
    before_confirm: Overlay,
    confirm_text: String,
    pending: Option<Pending>,
    toast: Option<Toast>,
    toast_serial: u64,
    readeck: bool,
    settings: Option<SettingsForm>,

    prefs: Prefs,
    /// Whether the help says ⌘ or Ctrl.
    mac: bool,
}

impl App {
    pub fn new(prefs: Prefs, mac: bool) -> Self {
        Self {
            queue: Vec::new(),
            index: 0,
            selection_label: String::new(),
            loading: false,
            mode: Mode::Unread,
            total: 0,
            has_more: false,
            checking: false,
            latest_load: 0,
            opened: None,
            newest_at_open: 0,
            busy: false,
            loading_more: false,
            versions: HashMap::new(),
            picker_list: PickerList::Loading,
            picker_cursor: 0,
            picker_mode: Mode::Unread,
            overlay: Overlay::None,
            before_help: Overlay::None,
            before_confirm: Overlay::None,
            confirm_text: String::new(),
            pending: None,
            toast: None,
            toast_serial: 0,
            readeck: false,
            settings: None,
            prefs,
            mac,
        }
    }

    /// The first screen: the picker, or onboarding when Miniflux isn't set up yet.
    pub fn start(&mut self, status: Status) -> Vec<Effect> {
        let mut fx = Vec::new();
        if status.configured {
            self.readeck = status.readeck;
            self.open_picker(Some(Mode::Unread), &mut fx);
        } else {
            self.open_settings(true, &mut fx);
        }
        fx
    }

    // ---- Reading the state (for the window, and the tests) --------------

    pub fn current(&self) -> Option<&Entry> {
        self.queue.get(self.index)
    }

    #[cfg(test)]
    pub fn queue(&self) -> &[Entry] {
        &self.queue
    }

    #[cfg(test)]
    pub fn has_more(&self) -> bool {
        self.has_more
    }

    pub fn mode(&self) -> Mode {
        self.mode
    }

    pub fn overlay(&self) -> Overlay {
        self.overlay
    }

    pub fn in_settings(&self) -> bool {
        matches!(self.overlay, Overlay::Settings | Overlay::Onboarding)
    }

    pub fn selection_label(&self) -> &str {
        &self.selection_label
    }

    pub fn readeck(&self) -> bool {
        self.readeck
    }

    pub fn prefs(&self) -> Prefs {
        self.prefs
    }

    pub fn toast(&self) -> Option<&Toast> {
        self.toast.as_ref()
    }

    pub fn confirm_text(&self) -> &str {
        &self.confirm_text
    }

    pub fn settings(&self) -> Option<&SettingsForm> {
        self.settings.as_ref()
    }

    pub fn picker_list(&self) -> &PickerList {
        &self.picker_list
    }

    pub fn picker_cursor(&self) -> usize {
        self.picker_cursor
    }

    pub fn picker_mode(&self) -> Mode {
        self.picker_mode
    }

    pub fn picker_items(&self) -> Vec<PickerItem> {
        match &self.picker_list {
            PickerList::Ready(counts) => picker::build(counts, self.picker_mode),
            _ => Vec::new(),
        }
    }

    pub fn help_lines(&self) -> Vec<(String, String)> {
        keys::help_lines(self.mac)
    }

    /// "3 / 120" while reading, counting the pages not loaded yet.
    pub fn position(&self) -> String {
        let n = self.queue.len();
        if n == 0 {
            return String::new();
        }
        let of = if self.has_more { self.total } else { i64::try_from(n).unwrap_or(i64::MAX) };
        format!("{} / {of}", (self.index + 1).min(n))
    }

    /// The window title: the article while one is on screen, just the app name otherwise.
    pub fn window_title(&self) -> String {
        match self.current() {
            Some(entry) if !self.loading && !self.in_settings() && self.overlay != Overlay::Picker => {
                format!("{} — {APP_NAME}", display_title(&entry.title))
            }
            _ => APP_NAME.to_string(),
        }
    }

    /// What fills the window under the top bar.
    pub fn screen(&self) -> Screen {
        if self.in_settings() {
            Screen::Settings
        } else if self.loading {
            Screen::Loading
        } else if self.current().is_some() {
            Screen::Article
        } else if self.has_more {
            Screen::Loading
        } else if !self.queue.is_empty() && !self.checking {
            // While it looks for new arrivals, nothing: no "Loading…" if there are none, no "All caught up" if
            // there are.
            Screen::End
        } else {
            Screen::Empty
        }
    }

    // ---- Toasts -------------------------------------------------------

    fn toast_info(&mut self, message: impl Into<String>) {
        self.show_toast(message.into(), ToastKind::Info);
    }

    fn toast_error(&mut self, message: impl Into<String>) {
        self.show_toast(message.into(), ToastKind::Error);
    }

    fn show_toast(&mut self, message: String, kind: ToastKind) {
        self.toast_serial += 1;
        self.toast = Some(Toast { message, kind, serial: self.toast_serial });
    }

    /// The window's timer ran out for toast `serial`; a newer toast stays.
    pub fn dismiss_toast(&mut self, serial: u64) {
        if self.toast.as_ref().is_some_and(|t| t.serial == serial) {
            self.toast = None;
        }
    }

    // ---- Opening a selection --------------------------------------------

    fn reset_queue(&mut self, label: String) {
        self.queue.clear();
        self.index = 0;
        self.selection_label = label;
        self.total = 0;
        self.has_more = false;
        self.checking = false;
    }

    /// Empties the queue and cancels a selection that is still loading (after the server changed).
    fn clear_queue(&mut self) {
        self.latest_load += 1;
        self.versions.clear();
        self.loading = false;
        self.opened = None;
        self.reset_queue(String::new());
    }

    fn open_selection(&mut self, item: PickerItem, mode: Mode, fx: &mut Vec<Effect>) {
        self.latest_load += 1;
        self.mode = mode;
        self.opened = Some(item.selection);
        self.newest_at_open = 0;
        self.reset_queue(item.label);
        self.loading = true;
        let request = paging::next_page(&self.queue, mode);
        fx.push(Effect::OpenSelection { load: self.latest_load, selection: item.selection, mode, request });
    }

    fn opened(&mut self, load: u64, result: Result<(EntryPage, i64), String>) {
        if load != self.latest_load {
            return;
        }
        self.loading = false;
        match result {
            Ok((page, newest)) => {
                self.newest_at_open = newest;
                self.append_page(page);
            }
            Err(message) => self.toast_error(message),
        }
    }

    // ---- Paging (see paging.rs) -----------------------------------------

    fn append_page(&mut self, page: EntryPage) {
        let merged = paging::merge_page(&self.queue, page);
        self.queue.extend(merged.fresh);
        self.has_more = merged.more;
        self.total = merged.total;
    }

    /// Asks for the page after what's loaded; one at a time. Returns whether it did.
    fn load_page(&mut self, arrivals: bool, fx: &mut Vec<Effect>) -> bool {
        let Some(selection) = self.opened else { return false };
        if self.loading_more {
            return false;
        }
        self.loading_more = true;
        let request = paging::next_page(&self.queue, self.mode);
        fx.push(Effect::LoadPage { load: self.latest_load, selection, mode: self.mode, request, arrivals });
        true
    }

    fn page_loaded(&mut self, load: u64, arrivals: bool, result: Result<EntryPage, String>) {
        self.loading_more = false;
        if arrivals {
            self.checking = false;
        }
        if load != self.latest_load {
            return;
        }
        match result {
            Ok(page) => self.append_page(page),
            Err(message) => self.toast_error(message),
        }
    }

    fn load_more(&mut self, fx: &mut Vec<Effect>) {
        if self.has_more {
            self.load_page(false, fx);
        }
    }

    /// Just past the last unread item: the page after it is whatever arrived since, so those follow on as if
    /// they'd been there all along, instead of "All caught up". (It starts at the newest loaded item's publication
    /// time, so an item a feed backdates before that shows up only when the selection is opened again.)
    fn check_for_arrivals(&mut self, fx: &mut Vec<Effect>) {
        if self.load_page(true, fx) {
            self.checking = true;
        }
    }

    fn prefetch(&mut self, fx: &mut Vec<Effect>) {
        if self.has_more && self.index + PREFETCH >= self.queue.len() {
            self.load_more(fx);
        }
    }

    /// After moving on: the next page when it's near, or, just past the end of the unread list, new arrivals.
    fn load_ahead(&mut self, fx: &mut Vec<Effect>) {
        if self.current().is_none() && !self.has_more && self.mode == Mode::Unread {
            self.check_for_arrivals(fx);
        } else {
            self.prefetch(fx);
        }
    }

    // ---- Actions on the queue ---------------------------------------------

    fn set_status(&mut self, index: usize, status: EntryStatus, announce: bool, fx: &mut Vec<Effect>) {
        let Some(entry) = self.queue.get_mut(index) else { return };
        if entry.status == status {
            return;
        }
        let revert = Revert { index, id: entry.id, before: entry.status };
        entry.status = status; // optimistic
        fx.push(Effect::SetStatus { ids: vec![revert.id], status, revert: Some(revert), announce });
    }

    fn status_set(&mut self, status: EntryStatus, revert: Option<Revert>, announce: bool, result: Result<(), String>) {
        match result {
            Ok(()) if announce => self.toast_info(format!("Marked {}", status_name(status))),
            Ok(()) => {}
            Err(message) => {
                self.toast_error(message);
                // Roll back, unless another selection was opened meanwhile.
                if let Some(r) = revert
                    && let Some(entry) = self.queue.get_mut(r.index)
                    && entry.id == r.id
                {
                    entry.status = r.before;
                }
            }
        }
    }

    /// Moves past the current article, marking it read if asked.
    /// Returns false at the end of the queue, when there is nothing more to load.
    fn next(&mut self, mark_read: bool, fx: &mut Vec<Effect>) -> bool {
        if self.current().is_none() {
            if self.has_more {
                self.load_more(fx); // the next page is still loading, or failed: try (again)
            }
            return self.has_more || self.checking;
        }
        if mark_read {
            self.set_status(self.index, EntryStatus::Read, false, fx);
        }
        self.index += 1;
        self.load_ahead(fx);
        true
    }

    /// j, Space at the end, the ▼ button: the next article, or the picker once the queue is done.
    fn advance(&mut self, mark_read: bool, fx: &mut Vec<Effect>) {
        if !self.next(mark_read, fx) {
            self.open_picker(None, fx);
        }
    }

    fn prev(&mut self) {
        self.index = self.index.saturating_sub(1);
    }

    fn toggle_read(&mut self, fx: &mut Vec<Effect>) {
        let Some(entry) = self.current() else { return };
        let status = if entry.status == EntryStatus::Read { EntryStatus::Unread } else { EntryStatus::Read };
        self.set_status(self.index, status, true, fx);
    }

    fn open_original(&mut self, fx: &mut Vec<Effect>) {
        if let Some(entry) = self.current() {
            let url = entry.url.clone();
            fx.push(Effect::OpenInBrowser { url, then: AfterOpen::MarkRead { index: self.index } });
        }
    }

    /// A link in an article or its title: always the browser.
    pub fn open_link(&mut self, url: &str) -> Vec<Effect> {
        vec![Effect::OpenInBrowser { url: url.to_string(), then: AfterOpen::Nothing }]
    }

    fn opened_in_browser(&mut self, then: AfterOpen, result: Result<(), String>, fx: &mut Vec<Effect>) {
        match (result, then) {
            (Err(message), _) => self.toast_error(message),
            (Ok(()), AfterOpen::Nothing) => {}
            (Ok(()), AfterOpen::MarkRead { index }) => {
                self.toast_info("Opened in browser");
                self.set_status(index, EntryStatus::Read, false, fx);
            }
        }
    }

    fn save_to_readeck(&mut self, fx: &mut Vec<Effect>) {
        let Some(url) = self.current().map(|e| e.url.clone()) else { return };
        if !self.readeck {
            self.toast_info("Readeck isn't set up. Add it in Settings (press ,).");
            return;
        }
        if self.busy {
            return;
        }
        self.busy = true;
        fx.push(Effect::SaveToReadeck { url });
    }

    fn saved_to_readeck(&mut self, result: Result<(), String>, fx: &mut Vec<Effect>) {
        self.busy = false;
        match result {
            Ok(()) => {
                self.toast_info("Saved to Readeck");
                self.next(true, fx);
            }
            Err(message) => self.toast_error(message),
        }
    }

    /// Swaps an article's text; if it's the one on screen, says which version it is.
    fn show_content(&mut self, index: usize, content: String, version: &str) {
        if let Some(entry) = self.queue.get_mut(index) {
            entry.content = content;
        }
        if self.index == index {
            self.toast_info(version);
        }
    }

    /// Fetches the full article the first time, then switches between it and the feed's version.
    fn fetch_full(&mut self, fx: &mut Vec<Effect>) {
        let Some(entry) = self.current() else { return };
        let (id, index) = (entry.id, self.index);
        let switch = self.versions.get(&id).map(|known| {
            if entry.content == known.full {
                (known.feed.clone(), "Feed version")
            } else {
                (known.full.clone(), "Full article")
            }
        });
        match switch {
            Some((content, version)) => self.show_content(index, content, version),
            None => {
                if self.busy {
                    return;
                }
                self.busy = true;
                self.toast_info("Fetching full article…");
                fx.push(Effect::FetchContent { index, id });
            }
        }
    }

    /// Shows the fetched article, unless another selection was opened meanwhile.
    fn content_fetched(&mut self, index: usize, id: i64, result: Result<String, String>) {
        self.busy = false;
        match result {
            Ok(full) => {
                let Some(entry) = self.queue.get(index).filter(|e| e.id == id) else { return };
                self.versions.insert(id, Versions { feed: entry.content.clone(), full: full.clone() });
                self.show_content(index, full, "Full article");
            }
            Err(message) => self.toast_error(message),
        }
    }

    /// Shift+A. Most of the unread list isn't loaded, so Miniflux marks what it has unread up to the newest item
    /// there was when the selection was opened: nothing that arrived since gets marked read unseen, as its "mark
    /// all as read" endpoints would. Later items that a page did load are marked by id. (In history
    /// `newest_at_open` is 0, so that's just the items marked unread here.) The server counts, so the question
    /// stays true even after reading elsewhere.
    fn mark_all_read(&mut self, fx: &mut Vec<Effect>) {
        let Some(selection) = self.opened else { return };
        if self.loading {
            return;
        }
        let up_to = self.newest_at_open;
        let later =
            self.queue.iter().filter(|e| e.status == EntryStatus::Unread && e.id > up_to).map(|e| e.id).collect();
        fx.push(Effect::CountUnread { load: self.latest_load, selection, up_to, later });
    }

    fn unread_counted(
        &mut self,
        load: u64,
        selection: Selection,
        up_to: i64,
        later: Vec<i64>,
        result: Result<i64, String>,
    ) {
        let count = match result {
            Ok(count) => count + i64::try_from(later.len()).unwrap_or(0),
            Err(message) => {
                if load == self.latest_load {
                    self.toast_error(message);
                }
                return;
            }
        };
        if count == 0 || load != self.latest_load {
            return;
        }
        let question = format!("Mark all {count} unread items in “{}” as read?", self.selection_label);
        self.confirm(question, Pending::MarkAll { load, selection, up_to, later });
    }

    fn marked_all_read(&mut self, load: u64, result: Result<(), String>, fx: &mut Vec<Effect>) {
        if load != self.latest_load {
            return;
        }
        if let Err(message) = result {
            self.toast_error(message);
            return;
        }
        // Show it, and go to the end of the queue.
        for entry in self.queue.iter_mut().filter(|e| e.status == EntryStatus::Unread) {
            entry.status = EntryStatus::Read;
        }
        self.index = self.queue.len();
        if self.mode == Mode::History {
            self.prefetch(fx);
            return;
        }
        // Nothing unread is left up to where the selection was opened. Drop a page still loading: it would bring
        // back items just marked read.
        self.latest_load += 1;
        self.has_more = false;
        self.total = i64::try_from(self.queue.len()).unwrap_or(i64::MAX);
    }

    // ---- The picker ---------------------------------------------------------

    /// Opens the picker, by default in the mode of the open queue.
    fn open_picker(&mut self, mode: Option<Mode>, fx: &mut Vec<Effect>) {
        let mode = mode.unwrap_or(self.mode);
        if mode != self.picker_mode {
            self.picker_mode = mode;
            self.picker_cursor = 0;
        }
        self.overlay = Overlay::Picker;
        self.load_picker(fx);
    }

    fn load_picker(&mut self, fx: &mut Vec<Effect>) {
        if !matches!(self.picker_list, PickerList::Ready(_)) {
            self.picker_list = PickerList::Loading;
        }
        fx.push(Effect::FeedCounts);
    }

    fn feed_counts(&mut self, result: Result<Vec<FeedCount>, String>) {
        match result {
            Ok(counts) => {
                self.picker_list = PickerList::Ready(counts);
                let n = self.picker_items().len();
                self.picker_cursor = self.picker_cursor.min(n.saturating_sub(1));
            }
            Err(message) => self.picker_list = PickerList::Error(message),
        }
    }

    fn close_picker(&mut self) {
        if self.overlay == Overlay::Picker {
            self.overlay = Overlay::None;
        }
    }

    /// The picker can only be dismissed once there is something to go back to.
    fn picker_can_close(&self) -> bool {
        !self.queue.is_empty() || !self.selection_label.is_empty()
    }

    /// h: switch between unread and history (read items, newest first).
    fn toggle_mode(&mut self, fx: &mut Vec<Effect>) {
        let mode = if self.overlay == Overlay::Picker { self.picker_mode } else { self.mode };
        self.open_picker(Some(mode.other()), fx);
    }

    fn move_picker(&mut self, delta: i32) {
        let n = self.picker_items().len();
        if n > 0 {
            let n = i64::try_from(n).unwrap_or(i64::MAX);
            let cursor = i64::try_from(self.picker_cursor).unwrap_or(0);
            self.picker_cursor = usize::try_from((cursor + i64::from(delta)).rem_euclid(n)).unwrap_or(0);
        }
    }

    fn choose(&mut self, row: Option<usize>, fx: &mut Vec<Effect>) {
        let items = self.picker_items();
        if let Some(item) = items.into_iter().nth(row.unwrap_or(self.picker_cursor)) {
            self.close_picker();
            self.open_selection(item, self.picker_mode, fx);
        }
    }

    fn refreshed(&mut self, result: Result<(), String>, fx: &mut Vec<Effect>) {
        match result {
            Ok(()) => fx.extend(REFRESH_RELOAD_MS.map(|millis| Effect::ReloadPickerAfter { millis })),
            Err(message) => self.toast_error(message),
        }
    }

    // ---- Overlays ------------------------------------------------------

    fn open_help(&mut self) {
        self.before_help = self.overlay;
        self.overlay = Overlay::Help;
    }

    fn close_help(&mut self) {
        self.overlay = self.before_help;
    }

    fn confirm(&mut self, text: String, pending: Pending) {
        self.confirm_text = text;
        self.before_confirm = self.overlay;
        self.overlay = Overlay::Confirm;
        self.pending = Some(pending);
    }

    fn answer_confirm(&mut self, yes: bool, fx: &mut Vec<Effect>) {
        self.overlay = self.before_confirm;
        match self.pending.take() {
            Some(Pending::MarkAll { load, selection, up_to, later }) if yes => {
                fx.push(Effect::MarkAllRead { load, selection, up_to, later });
            }
            _ => {}
        }
    }

    // ---- Settings ------------------------------------------------------

    /// Opens the settings form. On first run (onboarding) it can't be cancelled.
    fn open_settings(&mut self, first_run: bool, fx: &mut Vec<Effect>) {
        if !self.in_settings() {
            self.overlay = if first_run { Overlay::Onboarding } else { Overlay::Settings };
            self.settings = None;
            fx.push(Effect::LoadSettings);
        }
    }

    /// Called with the new status after a successful save, or `None` when cancelled.
    fn close_settings(&mut self, status: Option<Status>, fx: &mut Vec<Effect>) {
        let first_run = self.overlay == Overlay::Onboarding;
        if let Some(status) = status {
            self.readeck = status.readeck;
            if !first_run {
                self.toast_info("Settings saved");
            }
            // The server may have changed: start from the picker with a fresh queue.
            self.settings = None;
            self.clear_queue();
            self.picker_list = PickerList::Loading;
            self.open_picker(None, fx);
        } else if !first_run {
            self.settings = None;
            self.overlay = Overlay::None;
            if self.queue.is_empty() {
                self.open_picker(None, fx);
            }
        }
    }

    /// The form's Save / Connect: the backend checks both servers before anything is written.
    pub fn submit_settings(&mut self, values: SettingsValues) -> Vec<Effect> {
        let Some(form) = self.settings.as_mut() else { return Vec::new() };
        if form.checking {
            return Vec::new();
        }
        form.error.clear();
        form.checking = true;
        form.values = values.clone();
        vec![Effect::SaveSettings(SettingsValuesBox(values))]
    }

    /// The form's Cancel button: Esc.
    pub fn cancel_settings(&mut self) -> Vec<Effect> {
        let mut fx = Vec::new();
        if self.overlay == Overlay::Settings {
            self.close_settings(None, &mut fx);
        }
        fx
    }

    // ---- Prefs -----------------------------------------------------------

    fn change_prefs(&mut self, change: impl FnOnce(&mut Prefs), fx: &mut Vec<Effect>) {
        let before = self.prefs;
        change(&mut self.prefs);
        if self.prefs != before {
            fx.push(Effect::SavePrefs(self.prefs));
        }
    }

    // ---- Input ---------------------------------------------------------

    /// A key press; returns whether it was used, and what to do.
    pub fn key(&mut self, press: &KeyPress) -> (bool, Vec<Effect>) {
        let mut fx = Vec::new();
        let used = self.handle_key(press, &mut fx);
        (used, fx)
    }

    fn handle_key(&mut self, press: &KeyPress, fx: &mut Vec<Effect>) -> bool {
        let key = press.key.as_str();
        // Overlays that take every key.
        match self.overlay {
            Overlay::Confirm => {
                let yes = key == "Enter" || key == "y";
                let no = key == "Escape" || key == "n";
                if yes || no {
                    self.answer_confirm(yes, fx);
                }
                return yes || no;
            }
            Overlay::Onboarding => return false, // can't be cancelled; all keys go to the form
            Overlay::Settings => {
                if key == "Escape" {
                    self.close_settings(None, fx);
                    return true;
                }
                return false;
            }
            Overlay::Help => {
                if matches!(key, "Escape" | "?" | "q") {
                    self.close_help();
                    return true;
                }
                return false;
            }
            Overlay::Picker | Overlay::None => {}
        }
        if press.typing {
            return false;
        }
        let action = keys::lookup(keys::GLOBAL_KEYS, key).or_else(|| {
            if self.overlay == Overlay::Picker {
                keys::lookup(keys::PICKER_KEYS, key)
            } else if press.ctrl {
                keys::lookup(keys::MODIFIER_KEYS, key)
            } else if press.alt || press.meta {
                None
            } else {
                keys::lookup(keys::READER_KEYS, key)
            }
        });
        match action {
            Some(Action::Space { .. }) => self.space(press.shift, press.at_bottom, fx),
            Some(Action::Enter) => self.enter(fx),
            Some(action) => {
                self.act(action, fx);
                true
            }
            None => false,
        }
    }

    /// Space pages down, and at the end of an article goes to the next one; Shift+Space pages up.
    fn space(&mut self, shift: bool, at_bottom: bool, fx: &mut Vec<Effect>) -> bool {
        if shift {
            fx.push(Effect::ScrollPage { up: true });
        } else if at_bottom {
            self.advance(true, fx);
        } else {
            fx.push(Effect::ScrollPage { up: false });
        }
        true
    }

    /// Enter only means something on the "all caught up" screen: back to the feeds.
    fn enter(&mut self, fx: &mut Vec<Effect>) -> bool {
        let caught_up = self.current().is_none() && !self.queue.is_empty() && !self.checking;
        if caught_up {
            self.open_picker(None, fx);
        }
        caught_up
    }

    /// An action from a key or a button.
    pub fn action(&mut self, action: Action) -> Vec<Effect> {
        let mut fx = Vec::new();
        match action {
            Action::Space { up } => {
                fx.push(Effect::ScrollPage { up });
            }
            Action::Enter => {
                self.enter(&mut fx);
            }
            action => self.act(action, &mut fx),
        }
        fx
    }

    fn act(&mut self, action: Action, fx: &mut Vec<Effect>) {
        match action {
            Action::Next { mark_read } => self.advance(mark_read, fx),
            Action::Prev => self.prev(),
            Action::ToggleRead => self.toggle_read(fx),
            Action::OpenOriginal => self.open_original(fx),
            Action::SaveToReadeck => self.save_to_readeck(fx),
            Action::FetchFull => self.fetch_full(fx),
            Action::MarkAllRead => self.mark_all_read(fx),
            Action::ToggleMode => self.toggle_mode(fx),
            Action::OpenPicker => self.open_picker(None, fx),
            Action::Narrower => self.change_prefs(Prefs::narrower, fx),
            Action::Wider => self.change_prefs(Prefs::wider, fx),
            Action::BiggerFont => self.change_prefs(Prefs::bigger, fx),
            Action::SmallerFont => self.change_prefs(Prefs::smaller, fx),
            Action::ResetSize => self.change_prefs(Prefs::reset, fx),
            Action::Help => self.open_help(),
            Action::Quit => fx.push(Effect::Quit),
            Action::Settings => self.open_settings(false, fx),
            Action::PickerMove(delta) => self.move_picker(delta),
            Action::PickerChoose => self.choose(None, fx),
            Action::PickerRefresh => {
                self.toast_info("Refreshing feeds…");
                fx.push(Effect::RefreshFeeds);
            }
            Action::PickerClose => {
                if self.picker_can_close() {
                    self.close_picker();
                }
            }
            Action::Space { .. } | Action::Enter => {}
        }
    }

    /// A click on a picker row.
    pub fn choose_row(&mut self, row: usize) -> Vec<Effect> {
        let mut fx = Vec::new();
        self.choose(Some(row), &mut fx);
        fx
    }

    /// A click on the dimmed area around the picker.
    pub fn picker_backdrop(&mut self) {
        if self.picker_can_close() {
            self.close_picker();
        }
    }

    pub fn close_help_overlay(&mut self) {
        if self.overlay == Overlay::Help {
            self.close_help();
        }
    }

    // ---- Answers -------------------------------------------------------------

    pub fn apply(&mut self, outcome: Outcome) -> Vec<Effect> {
        let mut fx = Vec::new();
        match outcome {
            Outcome::Opened { load, result } => self.opened(load, result),
            Outcome::Page { load, arrivals, result } => self.page_loaded(load, arrivals, result),
            Outcome::StatusSet { status, revert, announce, result } => {
                self.status_set(status, revert, announce, result);
            }
            Outcome::ContentFetched { index, id, result } => self.content_fetched(index, id, result),
            Outcome::FeedCounts(result) => self.feed_counts(result),
            Outcome::Refreshed(result) => self.refreshed(result, &mut fx),
            Outcome::PickerReloadDue => {
                if self.overlay == Overlay::Picker {
                    self.load_picker(&mut fx);
                }
            }
            Outcome::SavedToReadeck(result) => self.saved_to_readeck(result, &mut fx),
            Outcome::OpenedInBrowser { then, result } => self.opened_in_browser(then, result, &mut fx),
            Outcome::UnreadCounted { load, selection, up_to, later, result } => {
                self.unread_counted(load, selection, up_to, later, result);
            }
            Outcome::MarkedAllRead { load, result } => self.marked_all_read(load, result, &mut fx),
            Outcome::SettingsLoaded { values, problem } => {
                if self.in_settings() {
                    self.settings = Some(SettingsForm { values, problem, error: String::new(), checking: false });
                }
            }
            Outcome::SettingsSaved(result) => match result {
                Ok(status) => self.close_settings(Some(status), &mut fx),
                Err(message) => {
                    if let Some(form) = self.settings.as_mut() {
                        form.error = message;
                        form.checking = false;
                    }
                }
            },
        }
        fx
    }
}

/// What fills the window under the top bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    Settings,
    Loading,
    Article,
    /// "All caught up" / "End of history".
    End,
    Empty,
}

/// An article title as shown, in the article and the window title.
pub fn display_title(title: &str) -> &str {
    if title.is_empty() { "(untitled)" } else { title }
}

pub fn status_name(status: EntryStatus) -> &'static str {
    match status {
        EntryStatus::Read => "read",
        EntryStatus::Unread => "unread",
    }
}

#[cfg(test)]
#[path = "app_tests.rs"]
mod tests;
