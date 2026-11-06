//! Everything the app does, without Qt and without a network: the reading queue and its paging, the feeds
//! picker, overlays, toasts, the confirm dialog, settings and the keyboard.
//!
//! It never waits. An action changes the state and returns the `Effect`s it needs (a server call, a scroll);
//! whoever runs them (`bridge.rs`, or the tests) hands each result back to `apply` as an `Outcome`, whenever it
//! arrives. Loads remember which selection they were for, so an answer that comes back after you moved on to
//! another selection changes nothing.
//!
//! Unread mode: unread items, oldest first (or newest first, `s`); going back shows items read this session.
//! History mode: read items, newest first.
//! Search: read and unread items matching some words, best match first.
//! Starred: the items starred on the server (`*`), read and unread, newest first.
//! All of them load a page at a time as you get near the end. At the end of the unread list, it looks once more
//! for items that arrived meanwhile.

use crate::config::Server;
use crate::content::convert::{self, ImageRef};
use crate::detect::{Address, Api, Detected};
use crate::feeds::{self, Capabilities, Counts, Entry, EntryPage, EntryStatus, Selection};
use crate::keys::{self, Action};
use crate::miniflux::MintFailed;
use crate::paging::{self, Mode, Order, PAGE, PageRequest};
use crate::picker::{self, PickerItem};
use crate::prefs::{Colours, Prefs};
use crate::providers::{self, Choice, Field, FieldKey, Provider};
use serde::Serialize;
use std::collections::HashMap;

pub const APP_NAME: &str = "Onward";
/// Load the next page when this few loaded items are left.
const PREFETCH: usize = 10;
/// Miniflux refreshes feeds in the background; the counts are reloaded a few times while it works.
const REFRESH_RELOAD_MS: [u64; 3] = [2000, 6000, 15_000];
/// Shift+A on the starred list, or on its row in the picker. As with search results: what's starred is there to
/// be kept, and some of it is read already, so "the whole list" means little.
const STARRED_NOT_ALL_READ: &str = "Starred items can't be marked read all at once";

/// Only one thing has the keyboard at a time; the settings form replaces the article.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Overlay {
    None,
    Picker,
    Help,
    Confirm,
    Settings,
    Onboarding,
    /// The search box.
    Search,
    /// An article's images, one at a time, over the whole window.
    Image,
}

impl Overlay {
    /// As the windows name it.
    pub fn name(self) -> &'static str {
        match self {
            Overlay::None => "none",
            Overlay::Picker => "picker",
            Overlay::Help => "help",
            Overlay::Confirm => "confirm",
            Overlay::Settings => "settings",
            Overlay::Onboarding => "onboarding",
            Overlay::Search => "search",
            Overlay::Image => "image",
        }
    }
}

/// How far Connect has got, from the address box to what that server needs.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
enum Stage {
    /// The box, and what became of the last look, if that left nothing to ask for.
    #[default]
    Address,
    /// Miniflux, and how it gets a key.
    Miniflux {
        base: String,
        key: KeyRoute,
    },
    FreshRss {
        web: String,
        api: String,
    },
    /// The whole form, as it was before anything was detected.
    ByHand,
}

/// How a Miniflux user comes to a key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum KeyRoute {
    /// A username and password, traded for one.
    Password,
    /// Pasted, by choice: signing in with a password is still on offer.
    Chosen,
    /// Pasted, as this server has no password sign-in.
    NoPasswords,
    /// Pasted, as this server couldn't make one. That says nothing about the password.
    NoMinting,
}

/// A row of Connect, under the fields.
#[derive(Debug, Clone, PartialEq, Eq)]
enum RowDoes {
    /// Open a page.
    Open(String),
    /// Go looking at a hosted service's address.
    Hosted(&'static str),
    /// Miniflux's key page, and the box to paste what it gives.
    KeyPage(String),
    UsePassword,
    ByHand,
    /// Back to the address box, from filling in by hand.
    ToAddress,
}

/// One thing to press on Connect. `url` is what it opens, for a terminal to print in full.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ConnectRow {
    pub label: String,
    pub url: Option<String>,
}

/// Connect as the windows draw it. They keep what's typed; the core says what's on screen and what it's for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ConnectView {
    /// "address" for the address box with `fields` under it, or "by_hand" for the whole form.
    pub stage: &'static str,
    /// What was found.
    pub text: String,
    /// About the connection itself: it's plain http.
    pub note: String,
    pub fields: Vec<Field>,
    pub rows: Vec<ConnectRow>,
    /// What the main button says. It runs whatever `submit_settings` is on this stage.
    pub action: &'static str,
    /// Looking at an address. Fields stay open: typing makes the answer a no-op.
    pub probing: bool,
}

/// Why a save didn't go through.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SaveFailed {
    pub message: String,
    /// The server turned down the login, which on FreshRSS Connect can say more about.
    pub login_rejected: bool,
}

#[derive(Debug, Default)]
struct Connect {
    stage: Stage,
    /// What's in the box, and what `stage` is about.
    address: String,
    probing: bool,
    /// Rows a look left behind, with no fields to go with them.
    found_rows: Vec<(String, RowDoes)>,
    /// A look found a server, and it can't go further yet: the button looks again.
    look_again: bool,
    /// Plain http is in use, on an address where that's expected.
    plain: bool,
    /// Opened from Settings, which is where Esc goes back to, with what was on screen behind it.
    from_settings: Option<Overlay>,
    /// The server each secret in the form is for, once a look has found one. The form's own address can't say: only
    /// a save changes it, and the window's copy of it.
    secret_is_for: HashMap<FieldKey, String>,
}

/// Where first run is. It opens on `Welcome`, and Esc comes back there from the others.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    Welcome,
    Connect,
    NoServer,
}

impl Step {
    /// As the windows name it.
    pub fn name(self) -> &'static str {
        match self {
            Step::Welcome => "welcome",
            Step::Connect => "connect",
            Step::NoServer => "no_server",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PickerList {
    Loading,
    Error(String),
    Ready(Counts),
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
    /// The feed server's settings are present; otherwise show onboarding.
    pub configured: bool,
    /// Readeck is set up; otherwise hide the bookmark button.
    pub readeck: bool,
    /// What the server can do: what it can't, the keys only say so and the windows leave out their hints.
    pub capabilities: Capabilities,
}

/// What a save of the settings comes to. The feed server is saved; Readeck may not be.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Saved {
    pub status: Status,
    /// Who the feed server says you are, when it was asked.
    pub username: Option<String>,
    /// Why the Readeck values weren't saved: the Readeck saved before them stays.
    pub readeck_problem: Option<String>,
}

/// The settings form, flat: which server, the fields of both kinds (only the chosen one's are used, the other's
/// are kept for switching back), and Readeck, whose empty fields mean "not set up".
/// Its `Debug` shows nothing on purpose: it holds the API key, password and token.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct SettingsValues {
    pub server: Server,
    /// What's in the address box. It's turned into one of the URLs below, never saved as it is.
    pub address: String,
    pub miniflux_url: String,
    pub miniflux_api_key: String,
    /// Only on the way to a key: `App` hands these on in an effect and keeps neither the password nor a copy of it.
    pub miniflux_username: String,
    pub miniflux_password: String,
    pub greader_url: String,
    pub greader_username: String,
    pub greader_password: String,
    pub readeck_url: String,
    pub readeck_token: String,
}

impl SettingsValues {
    pub fn field(&self, key: FieldKey) -> &str {
        match key {
            FieldKey::Address => &self.address,
            FieldKey::MinifluxUrl => &self.miniflux_url,
            FieldKey::MinifluxApiKey => &self.miniflux_api_key,
            FieldKey::MinifluxUsername => &self.miniflux_username,
            FieldKey::MinifluxPassword => &self.miniflux_password,
            FieldKey::GreaderUrl => &self.greader_url,
            FieldKey::GreaderUsername => &self.greader_username,
            FieldKey::GreaderPassword => &self.greader_password,
            FieldKey::ReadeckUrl => &self.readeck_url,
            FieldKey::ReadeckToken => &self.readeck_token,
        }
    }

    pub fn field_mut(&mut self, key: FieldKey) -> &mut String {
        match key {
            FieldKey::Address => &mut self.address,
            FieldKey::MinifluxUrl => &mut self.miniflux_url,
            FieldKey::MinifluxApiKey => &mut self.miniflux_api_key,
            FieldKey::MinifluxUsername => &mut self.miniflux_username,
            FieldKey::MinifluxPassword => &mut self.miniflux_password,
            FieldKey::GreaderUrl => &mut self.greader_url,
            FieldKey::GreaderUsername => &mut self.greader_username,
            FieldKey::GreaderPassword => &mut self.greader_password,
            FieldKey::ReadeckUrl => &mut self.readeck_url,
            FieldKey::ReadeckToken => &mut self.readeck_token,
        }
    }
}

impl std::fmt::Debug for SettingsValues {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SettingsValues { .. }")
    }
}

#[derive(Debug)]
pub struct SettingsForm {
    /// What the form starts from; the window keeps what is typed until it submits.
    pub values: SettingsValues,
    /// What was wrong with the config file at startup (it didn't parse, say).
    pub problem: Option<String>,
    pub error: String,
    pub checking: bool,
    /// Bumped when `values` should replace what the window holds, after it started from them: the password, gone
    /// the moment it has been used.
    pub refill: u64,
}

/// Puts an optimistic status change back if the server turns it down.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Revert {
    pub index: usize,
    pub id: i64,
    pub before: EntryStatus,
}

/// What the open queue holds.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Opened {
    Feeds(Selection),
    /// Search results for these words.
    Search(String),
    /// The starred items, newest first (`Selection::Starred`).
    Starred,
}

/// A search whose first answer hasn't come. Whether a server can search shows only in that answer, so the open list
/// stays until then, and a search that fails leaves it as it was.
#[derive(Debug, Clone, PartialEq, Eq)]
struct AwaitedSearch {
    query: String,
    /// Where the box was opened from.
    from: Overlay,
    /// The "Searching…" toast, which the results make out of date.
    toast: u64,
}

/// The image viewer: the article's images, and the one on screen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Viewer {
    pub images: Vec<ImageRef>,
    pub index: usize,
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
    /// A page of search results, `offset` into them; the first is answered as `Opened`, the rest as `Page`.
    Search {
        load: u64,
        query: String,
        offset: usize,
        limit: usize,
        first: bool,
    },
    SetStatus {
        ids: Vec<i64>,
        status: EntryStatus,
        revert: Option<Revert>,
        announce: bool,
    },
    /// `*`: stars or unstars the article at `index`, already shown that way; put back if the server says no.
    SetStarred {
        index: usize,
        id: i64,
        starred: bool,
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
    /// `b`: saves the article at `index`; marks it read when done, and moves on if it's still on screen.
    SaveToReadeck {
        index: usize,
        id: i64,
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
    /// Shift+A in the picker: the row's newest unread id, and how many are unread up to it.
    CountRow {
        selection: Selection,
        label: String,
    },
    /// Marks the row's unread items read, up to the newest one there was when it was counted.
    MarkRowRead {
        selection: Selection,
        label: String,
        up_to: i64,
    },
    /// Bring up the settings window (`App::use_settings_window`), or to the front if it's up.
    ShowSettings,
    /// Serves the sample articles, or stops. It's an answered effect so that nothing is asked of the backend before
    /// it has switched: the backend's answer is what opens the picker.
    UseSamples(bool),
    LoadSettings,
    SaveSettings(SettingsValuesBox),
    /// Looks at an address, with no credentials. `probe` is compared when the answer comes, as tasks can't be stopped.
    Probe {
        probe: u64,
        address: Address,
    },
    /// Trades a Miniflux password for a key, once.
    MintKey(Login),
    SavePrefs(Prefs),
    ScrollPage {
        up: bool,
    },
    /// A few lines, for ↓ and ↑.
    ScrollStep {
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

/// A username and password on their way to the one request that uses them. `Debug` shows nothing.
#[derive(Clone, PartialEq, Eq)]
pub struct Login {
    pub probe: u64,
    pub base: String,
    pub username: String,
    pub password: String,
}

impl std::fmt::Debug for Login {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Login { .. }")
    }
}

/// The answer to an `Effect`. Errors are already the message to show.
pub enum Outcome {
    Opened {
        load: u64,
        result: Result<(EntryPage, i64), String>,
    },
    Page {
        load: u64,
        arrivals: bool,
        result: Result<EntryPage, String>,
    },
    StatusSet {
        status: EntryStatus,
        revert: Option<Revert>,
        announce: bool,
        result: Result<(), String>,
    },
    StarredSet {
        index: usize,
        id: i64,
        starred: bool,
        result: Result<(), String>,
    },
    ContentFetched {
        index: usize,
        id: i64,
        result: Result<String, String>,
    },
    FeedCounts(Result<Counts, String>),
    Refreshed(Result<(), String>),
    PickerReloadDue,
    SavedToReadeck {
        index: usize,
        id: i64,
        result: Result<(), String>,
    },
    OpenedInBrowser {
        then: AfterOpen,
        result: Result<(), String>,
    },
    UnreadCounted {
        load: u64,
        selection: Selection,
        up_to: i64,
        later: Vec<i64>,
        result: Result<i64, String>,
    },
    MarkedAllRead {
        load: u64,
        result: Result<(), String>,
    },
    /// The newest unread id, and the unread count up to it.
    RowCounted {
        selection: Selection,
        label: String,
        result: Result<(i64, i64), String>,
    },
    RowMarkedRead {
        selection: Selection,
        label: String,
        up_to: i64,
        result: Result<(), String>,
    },
    SettingsLoaded {
        values: SettingsValues,
        problem: Option<String>,
    },
    SettingsSaved(Result<Saved, SaveFailed>),
    Probed {
        probe: u64,
        found: Detected,
    },
    KeyMinted {
        probe: u64,
        result: Result<String, MintFailed>,
    },
    /// The backend switched, and this is what it has now.
    SamplesSwitched {
        on: bool,
        status: Status,
    },
}

impl std::fmt::Debug for Outcome {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Outcome { .. }")
    }
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
    /// The article is scrolled to its end (Space and ↓ then move on).
    pub at_bottom: bool,
    /// The article is scrolled to its top (↑ then goes back).
    pub at_top: bool,
    /// The key is held down and this is it repeating: ↓ and ↑ stop at the end of the article until it's let go.
    pub held: bool,
}

/// What a confirmed dialog goes on to do.
#[derive(Debug, Clone)]
enum Pending {
    MarkAll { load: u64, selection: Selection, up_to: i64, later: Vec<i64> },
    MarkRow { selection: Selection, label: String, up_to: i64 },
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
    /// The order the open queue was loaded in.
    order: Order,
    /// The whole selection, including pages not loaded yet.
    total: i64,
    /// More pages on the server.
    has_more: bool,
    /// Past the end of the unread list, looking for new arrivals.
    checking: bool,
    /// Opening another selection makes the loads still in flight no-ops: each remembers this and compares.
    latest_load: u64,
    opened: Option<Opened>,
    awaited_search: Option<AwaitedSearch>,
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
    before_search: Overlay,
    /// The last search, to start the box from.
    search_query: String,
    viewer: Option<Viewer>,
    confirm_text: String,
    pending: Option<Pending>,
    toast: Option<Toast>,
    toast_serial: u64,
    readeck: bool,
    /// See `Status::capabilities`.
    capabilities: Capabilities,
    settings: Option<SettingsForm>,
    /// The form lives in a window of its own: it takes no keys and doesn't replace the article.
    settings_in_window: bool,
    settings_window_open: bool,
    /// See `Step`; means nothing outside `Overlay::Onboarding`.
    step: Step,
    /// How far `Step::Connect` has got.
    connect: Connect,
    /// A new address, or stepping back, makes the probes still in flight no-ops: each remembers this and compares.
    latest_probe: u64,
    /// The row the arrows are on, in `Welcome` and `NoServer`.
    step_cursor: usize,
    /// The last link opened from first run or the form. Whether a browser really opened is more than `xdg-open`
    /// can say, so the window shows the address to take by hand.
    shown_link: Option<String>,
    /// Trying the sample articles from first run, and the prefs from before, which nothing of it may change.
    /// `ONWARD_DEMO=1` is not this: it's there from the start, for UI work, and never connects anywhere.
    samples: Option<Prefs>,

    prefs: Prefs,
    /// Whether the help says ⌘ or Ctrl.
    mac: bool,
}

impl std::fmt::Debug for App {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("App { .. }")
    }
}

impl App {
    pub fn new(prefs: Prefs, mac: bool) -> Self {
        Self {
            queue: Vec::new(),
            index: 0,
            selection_label: String::new(),
            loading: false,
            mode: Mode::Unread,
            order: Order::OldestFirst,
            total: 0,
            has_more: false,
            checking: false,
            latest_load: 0,
            opened: None,
            awaited_search: None,
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
            before_search: Overlay::None,
            search_query: String::new(),
            viewer: None,
            confirm_text: String::new(),
            pending: None,
            toast: None,
            toast_serial: 0,
            readeck: false,
            capabilities: Capabilities::EVERYTHING,
            settings: None,
            settings_in_window: false,
            settings_window_open: false,
            step: Step::Welcome,
            connect: Connect::default(),
            latest_probe: 0,
            step_cursor: 0,
            shown_link: None,
            samples: None,
            prefs,
            mac,
        }
    }

    /// The first screen: the picker, or onboarding when Miniflux isn't set up yet.
    pub fn start(&mut self, status: Status) -> Vec<Effect> {
        let mut fx = Vec::new();
        if status.configured {
            self.readeck = status.readeck;
            self.capabilities = status.capabilities;
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

    /// The article after this one, to get ready while this one is read.
    pub fn upcoming(&self) -> Option<&Entry> {
        self.queue.get(self.index + 1)
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

    /// For the Mac app, where Settings is a window beside the reader. Onboarding stays in the main window.
    pub fn use_settings_window(&mut self) {
        self.settings_in_window = true;
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

    pub fn capabilities(&self) -> Capabilities {
        self.capabilities
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

    /// Where first run is.
    pub fn step(&self) -> Step {
        self.step
    }

    /// The row the arrows are on, on the first-run steps that have rows.
    pub fn step_cursor(&self) -> usize {
        self.step_cursor
    }

    /// See the field.
    pub fn shown_link(&self) -> Option<&str> {
        self.shown_link.as_deref()
    }

    /// Reading the sample articles, chosen on the first screen: the top bar says so, and offers to connect.
    pub fn samples(&self) -> bool {
        self.samples.is_some()
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

    /// Unread items come newest first (`s`).
    pub fn newest_first(&self) -> bool {
        self.prefs.newest_first
    }

    /// What the search box starts with: the last search.
    pub fn search_query(&self) -> &str {
        &self.search_query
    }

    /// The image viewer, while it's open.
    pub fn viewer(&self) -> Option<&Viewer> {
        self.viewer.as_ref().filter(|_| self.overlay == Overlay::Image)
    }

    pub fn picker_items(&self) -> Vec<PickerItem> {
        match &self.picker_list {
            PickerList::Ready(counts) => picker::build(counts, self.picker_mode),
            _ => Vec::new(),
        }
    }

    pub fn help_lines(&self) -> Vec<(String, String)> {
        let mut lines = keys::help_lines(self.mac, self.capabilities);
        if self.samples() {
            lines.iter_mut().filter(|(keys, _)| keys == ",").for_each(|(_, action)| *action = "connect".into());
        }
        lines
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

    /// The heading of the screen at the end of the list.
    pub fn end_title(&self) -> &'static str {
        if self.searching() {
            "End of results"
        } else if self.starred_list() {
            "End of starred items"
        } else if self.mode == Mode::History {
            "End of history"
        } else {
            "All caught up"
        }
    }

    /// What the end screen says: where j goes next.
    pub fn end_hint(&self) -> String {
        match self.next_list() {
            Some(PickerItem { label, count: Some(count), .. }) => {
                format!("Press j for {label} ({count}), Enter for the feeds, k to go back.")
            }
            Some(next) => format!("Press j for {}, Enter for the feeds, k to go back.", next.label),
            None => "Press j or Enter for the feeds, k to go back.".into(),
        }
    }

    /// The picker is on screen: open, or behind the question Shift+A on a row asks.
    pub fn picker_shown(&self) -> bool {
        self.overlay == Overlay::Picker || (self.overlay == Overlay::Confirm && self.before_confirm == Overlay::Picker)
    }

    /// The top bar's History badge: reading read items.
    pub fn history_badge(&self) -> bool {
        self.mode == Mode::History && !self.searching() && !self.starred_list() && !self.selection_label().is_empty()
    }

    /// The top bar's Search badge: reading search results.
    pub fn searching(&self) -> bool {
        matches!(self.opened, Some(Opened::Search(_)))
    }

    /// The top bar's Starred badge: reading the starred items.
    pub fn starred_list(&self) -> bool {
        matches!(self.opened, Some(Opened::Starred))
    }

    /// The article on screen is starred: the windows show a star.
    pub fn starred(&self) -> bool {
        self.current().is_some_and(|e| e.starred)
    }

    /// A feed, a category or all of them is open, rather than search results or the starred items: a list that
    /// new arrivals join, and that j goes on from to the next one.
    fn reading_feeds(&self) -> bool {
        matches!(self.opened, Some(Opened::Feeds(_)))
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

    /// Forgets the loads in flight: their answers change nothing now, and they no longer hold up new ones.
    fn new_load(&mut self) -> u64 {
        self.latest_load += 1;
        self.loading_more = false;
        self.checking = false;
        self.awaited_search = None;
        self.latest_load
    }

    /// Empties the queue and cancels a selection that is still loading (after the server changed).
    fn clear_queue(&mut self) {
        self.new_load();
        self.versions.clear();
        self.loading = false;
        self.opened = None;
        self.reset_queue(String::new());
    }

    /// Opens a row of the picker. The starred items come newest first in either mode, whatever `s` says; the
    /// mode stays the picker's, so h goes on to the other one as it does from search results.
    fn open_selection(&mut self, selection: Selection, label: String, mode: Mode, fx: &mut Vec<Effect>) {
        self.new_load();
        self.mode = mode;
        let starred = selection == Selection::Starred;
        self.order = match mode {
            Mode::Unread if !self.prefs.newest_first && !starred => Order::OldestFirst,
            _ => Order::NewestFirst,
        };
        self.opened = Some(if starred { Opened::Starred } else { Opened::Feeds(selection) });
        self.newest_at_open = 0;
        self.reset_queue(label);
        self.loading = true;
        let request = paging::next_page(&self.queue, self.order);
        fx.push(Effect::OpenSelection { load: self.latest_load, selection, mode, request });
    }

    fn opened(&mut self, load: u64, result: Result<(EntryPage, i64), String>) {
        if load != self.latest_load {
            return;
        }
        self.loading = false;
        let search = self.awaited_search.take();
        match result {
            Ok((page, newest)) => {
                if let Some(search) = search {
                    self.open_results(search);
                }
                self.newest_at_open = newest;
                self.append_page(page);
            }
            Err(message) => {
                // The search box went on Enter. Back to the picker it came from, or that has something to open.
                if let Some(search) = search
                    && self.overlay == Overlay::None
                    && (search.from == Overlay::Picker || self.queue.is_empty())
                {
                    self.overlay = Overlay::Picker;
                }
                self.toast_error(message);
            }
        }
    }

    // ---- Paging (see paging.rs) -----------------------------------------

    fn append_page(&mut self, page: EntryPage) {
        // Search results page by offset, and each page asks for the one after everything loaded.
        let skipped = if self.searching() { self.queue.len() } else { 0 };
        let merged = paging::merge_page(&self.queue, page, skipped);
        self.queue.extend(merged.fresh);
        self.has_more = merged.more;
        self.total = merged.total;
    }

    /// Asks for the page after what's loaded; one at a time. Returns whether it did. New arrivals are always
    /// looked for after the newest item loaded, also when the list runs newest first.
    fn load_page(&mut self, arrivals: bool, fx: &mut Vec<Effect>) -> bool {
        // With a search on its way the list may be about to go, and this page would land in the results.
        if self.loading_more || self.awaited_search.is_some() {
            return false;
        }
        let load = self.latest_load;
        let effect = match &self.opened {
            None => return false,
            Some(Opened::Feeds(selection)) => {
                let order = if arrivals { Order::OldestFirst } else { self.order };
                let request = paging::next_page(&self.queue, order);
                Effect::LoadPage { load, selection: *selection, mode: self.mode, request, arrivals }
            }
            Some(Opened::Search(query)) => {
                let (query, offset) = (query.clone(), self.queue.len());
                Effect::Search { load, query, offset, limit: PAGE, first: false }
            }
            // Pages by time, like a feed, but no new arrivals join it: starring isn't arriving.
            Some(Opened::Starred) => {
                let request = paging::next_page(&self.queue, self.order);
                Effect::LoadPage { load, selection: Selection::Starred, mode: self.mode, request, arrivals: false }
            }
        };
        self.loading_more = true;
        fx.push(effect);
        true
    }

    fn page_loaded(&mut self, load: u64, arrivals: bool, result: Result<EntryPage, String>) {
        // An answer for an earlier selection: `new_load` already let go of it, and a page of this one may be on
        // its way, so leave the flags alone.
        if load != self.latest_load {
            return;
        }
        self.loading_more = false;
        if arrivals {
            self.checking = false;
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
    /// At the end, fresh counts too, for the end screen to say which list j goes on to.
    fn load_ahead(&mut self, fx: &mut Vec<Effect>) {
        let at_end = self.current().is_none() && !self.has_more;
        if at_end && self.reading_feeds() {
            fx.push(Effect::FeedCounts);
        }
        if at_end && self.mode == Mode::Unread && self.reading_feeds() {
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

    /// j, Space at the end, the ▼ button: the next article; once the list is done, the next feed (or category)
    /// with something in it, or the picker when there is none.
    fn advance(&mut self, mark_read: bool, fx: &mut Vec<Effect>) {
        if self.next(mark_read, fx) {
            return;
        }
        match self.next_list() {
            Some(next) => {
                // The picker's cursor follows, so p shows where you are.
                if let Some(row) = self.picker_items().iter().position(|i| i.selection == next.selection) {
                    self.picker_cursor = row;
                }
                self.open_selection(next.selection, next.label, self.mode, fx);
            }
            None => self.open_picker(None, fx),
        }
    }

    /// The list j goes on to from the end of this one (see `picker::next_after`); none after a search or the
    /// starred items.
    fn next_list(&self) -> Option<PickerItem> {
        match (&self.opened, &self.picker_list) {
            (Some(Opened::Feeds(selection)), PickerList::Ready(counts)) => {
                picker::next_after(&counts.feeds, self.mode, *selection)
            }
            _ => None,
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

    /// `*`: stars or unstars the article on screen, at once; the server is told, and if it says no, it's put back.
    /// It stays where it is, also in the starred list, as read items stay in a list for the rest of the visit.
    fn toggle_star(&mut self, fx: &mut Vec<Effect>) {
        let index = self.index;
        let Some(entry) = self.queue.get_mut(index) else { return };
        entry.starred = !entry.starred; // optimistic
        fx.push(Effect::SetStarred { index, id: entry.id, starred: entry.starred });
    }

    fn starred_set(&mut self, index: usize, id: i64, starred: bool, result: Result<(), String>) {
        match result {
            Ok(()) => self.toast_info(if starred { "Starred" } else { "Unstarred" }),
            Err(message) => {
                self.toast_error(message);
                // Put it back, unless another selection was opened meanwhile.
                if let Some(entry) = self.queue.get_mut(index).filter(|e| e.id == id) {
                    entry.starred = !starred;
                }
            }
        }
    }

    fn open_original(&mut self, fx: &mut Vec<Effect>) {
        if let Some(entry) = self.current() {
            let url = entry.url.clone();
            fx.push(Effect::OpenInBrowser { url, then: AfterOpen::MarkRead { index: self.index } });
        }
    }

    /// A link in an article or its title, or on a form: always the browser.
    pub fn open_link(&mut self, url: &str) -> Vec<Effect> {
        if self.in_settings() || self.settings_window_open {
            self.shown_link = Some(url.to_string());
        }
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
        let Some((id, url)) = self.current().map(|e| (e.id, e.url.clone())) else { return };
        if !self.readeck {
            self.toast_info("Readeck isn't set up. Add it in Settings (press ,).");
            return;
        }
        if self.busy {
            return;
        }
        self.busy = true;
        fx.push(Effect::SaveToReadeck { index: self.index, id, url });
    }

    /// The saved article is read now. If it's still on screen, move on; if you've gone on meanwhile (or opened
    /// another selection), it's marked where it is, and nothing else moves.
    fn saved_to_readeck(&mut self, index: usize, id: i64, result: Result<(), String>, fx: &mut Vec<Effect>) {
        self.busy = false;
        match result {
            Ok(()) => {
                self.toast_info("Saved to Readeck");
                if !self.queue.get(index).is_some_and(|e| e.id == id) {
                    return;
                }
                if self.index == index {
                    self.next(true, fx);
                } else {
                    self.set_status(index, EntryStatus::Read, false, fx);
                }
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
        if !self.capabilities.fetch_full {
            self.toast_info(feeds::FETCH_CONTENT_REFUSED);
            return;
        }
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
        // The dialog would still be up over the search results, and go on to mark the list before them.
        if self.awaited_search.is_some() {
            return;
        }
        let selection = match &self.opened {
            Some(Opened::Feeds(selection)) => *selection,
            Some(Opened::Search(_)) => {
                self.toast_info("Search results can't be marked read all at once");
                return;
            }
            Some(Opened::Starred) => {
                self.toast_info(STARRED_NOT_ALL_READ);
                return;
            }
            None => return,
        };
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
        self.new_load();
        self.has_more = false;
        self.total = i64::try_from(self.queue.len()).unwrap_or(i64::MAX);
    }

    /// Shift+A in the picker: the row under the cursor, without opening it. As in the reader, only what was
    /// unread when it was counted gets marked; the server counts, then the dialog asks.
    fn mark_row_read(&mut self, fx: &mut Vec<Effect>) {
        let Some(item) = self.picker_items().into_iter().nth(self.picker_cursor) else { return };
        if item.selection == Selection::Starred {
            self.toast_info(STARRED_NOT_ALL_READ);
        } else if self.picker_mode == Mode::History {
            self.toast_info("These are read already. Press h for unread items.");
        } else {
            fx.push(Effect::CountRow { selection: item.selection, label: item.label });
        }
    }

    fn row_counted(&mut self, selection: Selection, label: String, result: Result<(i64, i64), String>) {
        match result {
            Err(message) => self.toast_error(message),
            Ok((_, 0)) => self.toast_info(format!("Nothing unread in “{label}”")),
            // Unless you've left the picker meanwhile.
            Ok((up_to, count)) if self.overlay == Overlay::Picker => {
                let question = format!("Mark all {count} unread items in “{label}” as read?");
                self.confirm(question, Pending::MarkRow { selection, label, up_to });
            }
            Ok(_) => {}
        }
    }

    fn row_marked_read(
        &mut self,
        selection: Selection,
        label: &str,
        up_to: i64,
        result: Result<(), String>,
        fx: &mut Vec<Effect>,
    ) {
        if let Err(message) = result {
            self.toast_error(message);
            return;
        }
        self.toast_info(format!("Marked “{label}” read"));
        // The open queue may hold some of them: show them read too.
        let feeds = self.feeds_in(selection);
        for entry in self.queue.iter_mut() {
            if entry.status == EntryStatus::Unread && entry.id <= up_to && feeds.contains(entry) {
                entry.status = EntryStatus::Read;
            }
        }
        self.load_picker(fx);
    }

    /// Which feeds a selection takes in, by the picker's counts (entries only say their feed).
    fn feeds_in(&self, selection: Selection) -> FeedSet {
        match selection {
            Selection::All => FeedSet::All,
            Selection::Starred => FeedSet::Starred,
            Selection::Feed { id } => FeedSet::Some(vec![id]),
            Selection::Category { id } => FeedSet::Some(match &self.picker_list {
                PickerList::Ready(counts) => {
                    counts.feeds.iter().filter(|fc| fc.feed.category.id == id).map(|fc| fc.feed.id).collect()
                }
                _ => Vec::new(),
            }),
        }
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

    fn feed_counts(&mut self, result: Result<Counts, String>) {
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
            self.open_selection(item.selection, item.label, self.picker_mode, fx);
        }
    }

    fn refreshed(&mut self, result: Result<(), String>, fx: &mut Vec<Effect>) {
        match result {
            Ok(()) => fx.extend(REFRESH_RELOAD_MS.map(|millis| Effect::ReloadPickerAfter { millis })),
            Err(message) => self.toast_error(message),
        }
    }

    /// `s`: unread items newest first, or oldest first again (remembered). An unread list being read opens
    /// again in the new order; history is always newest first.
    fn flip_order(&mut self, fx: &mut Vec<Effect>) {
        self.change_prefs(Prefs::flip_order, fx);
        let order = if self.prefs.newest_first { "newest" } else { "oldest" };
        self.toast_info(format!("Unread items {order} first"));
        if self.overlay == Overlay::None
            && self.mode == Mode::Unread
            && let Some(Opened::Feeds(selection)) = self.opened
        {
            let label = self.selection_label.clone();
            self.open_selection(selection, label, Mode::Unread, fx);
        }
    }

    // ---- Search ----------------------------------------------------------

    /// `/`: the search box, from the reader or the picker.
    fn open_search(&mut self) {
        if self.capabilities.search == Some(false) {
            self.toast_info(feeds::NO_SEARCH);
        } else if matches!(self.overlay, Overlay::None | Overlay::Picker) {
            self.before_search = self.overlay;
            self.overlay = Overlay::Search;
        }
    }

    fn close_search(&mut self) {
        if self.overlay == Overlay::Search {
            self.overlay = self.before_search;
        }
    }

    /// The search box's Enter: the results open like any list, best match first, once they're in; if none come,
    /// everything stays as it was (`opened`). Until then an article stays on screen and works as usual: the windows
    /// scroll to the top on any change of screen, so "Loading…" would lose your place in it. The words stay in the
    /// box either way, to change or to try again. Nothing typed closes the box.
    pub fn search(&mut self, query: &str) -> Vec<Effect> {
        let mut fx = Vec::new();
        if self.overlay != Overlay::Search {
            return fx;
        }
        let query = query.split_whitespace().collect::<Vec<_>>().join(" ");
        if query.is_empty() {
            self.close_search();
            return fx;
        }
        self.overlay = Overlay::None;
        self.search_query.clone_from(&query);
        // A search replacing one on its way goes back where that one would have.
        let from = self.awaited_search.as_ref().map_or(self.before_search, |search| search.from);
        self.new_load();
        self.loading = self.current().is_none();
        self.toast_info("Searching…");
        let toast = self.toast_serial;
        self.awaited_search = Some(AwaitedSearch { query: query.clone(), from, toast });
        fx.push(Effect::Search { load: self.latest_load, query, offset: 0, limit: PAGE, first: true });
        fx
    }

    /// The first answer is in, so the results take over from the open list.
    fn open_results(&mut self, search: AwaitedSearch) {
        self.dismiss_toast(search.toast);
        self.reset_queue(format!("“{}”", search.query));
        self.opened = Some(Opened::Search(search.query));
        self.newest_at_open = 0;
    }

    /// The search box's Cancel: Esc.
    pub fn cancel_search(&mut self) {
        self.close_search();
    }

    // ---- The image viewer ----------------------------------------------------

    /// `v`, or a click on an image (`src`): the article's images, from that one or the first.
    fn open_viewer(&mut self, src: Option<&str>) {
        if self.overlay != Overlay::None {
            return;
        }
        let Some(entry) = self.current() else { return };
        let mut images = convert::images(&entry.content, &entry.url);
        let index = match src {
            None => 0,
            Some(src) => images.iter().position(|i| i.src == src).unwrap_or_else(|| {
                images = vec![ImageRef { src: src.to_string(), alt: String::new(), href: None }];
                0
            }),
        };
        if images.is_empty() {
            self.toast_info("This article has no images");
            return;
        }
        self.viewer = Some(Viewer { images, index });
        self.overlay = Overlay::Image;
    }

    /// A click on an image in the article.
    pub fn view_image(&mut self, src: &str) {
        self.open_viewer(Some(src));
    }

    /// A click in the viewer.
    pub fn close_viewer(&mut self) {
        if self.overlay == Overlay::Image {
            self.overlay = Overlay::None;
            self.viewer = None;
        }
    }

    fn viewer_key(&mut self, key: &str, fx: &mut Vec<Effect>) -> bool {
        let Some(viewer) = self.viewer.as_mut() else { return false };
        match key {
            "Escape" | "q" | "v" | "Enter" | " " => self.close_viewer(),
            "ArrowRight" | "l" => viewer.index = (viewer.index + 1).min(viewer.images.len().saturating_sub(1)),
            "ArrowLeft" | "h" => viewer.index = viewer.index.saturating_sub(1),
            "o" => {
                if let Some(image) = viewer.images.get(viewer.index) {
                    let url = image.href.clone().unwrap_or_else(|| image.src.clone());
                    fx.push(Effect::OpenInBrowser { url, then: AfterOpen::Nothing });
                }
            }
            _ => return false,
        }
        true
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
            Some(Pending::MarkRow { selection, label, up_to }) if yes => {
                fx.push(Effect::MarkRowRead { selection, label, up_to });
            }
            _ => {}
        }
    }

    // ---- Settings ------------------------------------------------------

    /// Opens the settings form, or on first run the steps that lead up to it.
    fn open_settings(&mut self, first_run: bool, fx: &mut Vec<Effect>) {
        if self.in_settings() {
            return;
        }
        if self.settings_in_window && !first_run {
            fx.push(Effect::ShowSettings);
            if self.settings_window_open {
                return;
            }
            self.settings_window_open = true;
        } else {
            self.overlay = if first_run { Overlay::Onboarding } else { Overlay::Settings };
        }
        self.settings = None;
        fx.push(Effect::LoadSettings);
    }

    /// Called with what was saved after a successful save, or `None` when cancelled.
    fn close_settings(&mut self, saved: Option<Saved>, fx: &mut Vec<Effect>) {
        let first_run = self.overlay == Overlay::Onboarding;
        let in_window = std::mem::take(&mut self.settings_window_open);
        self.shown_link = None;
        if let Some(saved) = saved {
            self.readeck = saved.status.readeck;
            self.capabilities = saved.status.capabilities;
            self.announce_saved(&saved, first_run);
            // The server may have changed: start from the picker with a fresh queue.
            self.settings = None;
            self.clear_queue();
            self.picker_list = PickerList::Loading;
            self.open_picker(None, fx);
        } else if in_window {
            // The reader behind it never stopped.
            self.settings = None;
        } else if !first_run {
            self.settings = None;
            self.overlay = Overlay::None;
            if self.queue.is_empty() {
                self.open_picker(None, fx);
            }
        }
    }

    /// A toast, as the form that could have said so is closing, and on the Mac it's a window of its own.
    fn announce_saved(&mut self, saved: &Saved, first_run: bool) {
        let name = saved.username.as_deref().map(str::trim).filter(|name| !name.is_empty());
        let done = name.map_or_else(|| "Settings saved".to_string(), |name| format!("Signed in as {name}"));
        match &saved.readeck_problem {
            Some(problem) => self.toast_error(format!("{done}. Readeck wasn't changed: {problem}")),
            None if name.is_some() || !first_run => self.toast_info(done),
            None => {}
        }
    }

    /// The kind of server a window's form names. One the core doesn't know stops the save, and the form says so:
    /// it would otherwise be saved as some other kind.
    fn server_named(&mut self, name: &str) -> Option<Server> {
        let server = Server::from_name(name);
        if server.is_none()
            && let Some(form) = self.settings.as_mut()
        {
            form.error = format!("Onward doesn't know a server of the kind \"{name}\"");
        }
        server
    }

    /// A window's form, which knows its fields by the names in `providers.rs`.
    pub fn submit_fields(&mut self, server: &str, fields: &HashMap<String, String>) -> Vec<Effect> {
        let Some(server) = self.server_named(server) else { return Vec::new() };
        let mut values = SettingsValues { server, ..SettingsValues::default() };
        for key in FieldKey::ALL {
            if let Some(text) = fields.get(key.name()) {
                values.field_mut(key).clone_from(text);
            }
        }
        self.submit_settings(values)
    }

    /// The form's Save, or the main button of Connect, which does what its stage is for.
    pub fn submit_settings(&mut self, values: SettingsValues) -> Vec<Effect> {
        if self.overlay == Overlay::Onboarding { self.submit_connect(values) } else { self.save_values(values) }
    }

    /// The backend checks both servers before anything is written.
    fn save_values(&mut self, mut values: SettingsValues) -> Vec<Effect> {
        let Some(form) = self.settings.as_mut() else { return Vec::new() };
        if form.checking {
            return Vec::new();
        }
        form.error.clear();
        form.checking = true;
        values.miniflux_password.clear();
        form.values = values.clone();
        vec![Effect::SaveSettings(SettingsValuesBox(values))]
    }

    /// The form's Cancel button: Esc.
    pub fn cancel_settings(&mut self) -> Vec<Effect> {
        let mut fx = Vec::new();
        if self.overlay == Overlay::Settings || self.settings_window_open {
            self.close_settings(None, &mut fx);
        }
        fx
    }

    // ---- First run -------------------------------------------------------

    fn go_to_step(&mut self, step: Step, cursor: usize) {
        self.step = step;
        self.step_cursor = cursor;
        self.shown_link = None;
    }

    /// Connect as it stands.
    pub fn connect_view(&self) -> ConnectView {
        let c = &self.connect;
        let (stage, text, action) = match &c.stage {
            Stage::Address if c.look_again => ("address", "", "Check again"),
            Stage::Address => ("address", "", "Continue"),
            Stage::ByHand => ("by_hand", "", "Connect"),
            Stage::FreshRss { .. } => ("address", providers::FRESHRSS_FOUND, "Connect"),
            Stage::Miniflux { key: KeyRoute::Password, .. } => ("address", providers::MINIFLUX_FOUND, "Sign in"),
            Stage::Miniflux { key: KeyRoute::Chosen, .. } => ("address", providers::MINIFLUX_PASTE, "Connect"),
            Stage::Miniflux { key: KeyRoute::NoPasswords, .. } => {
                ("address", providers::MINIFLUX_NO_PASSWORDS, "Connect")
            }
            Stage::Miniflux { key: KeyRoute::NoMinting, .. } => ("address", providers::MINIFLUX_NO_MINTING, "Connect"),
        };
        ConnectView {
            stage,
            text: text.to_string(),
            note: if c.plain { providers::PLAIN_HTTP.to_string() } else { String::new() },
            fields: self.connect_fields(),
            rows: self
                .connect_rows()
                .into_iter()
                .map(|(label, does)| {
                    let url = match does {
                        RowDoes::Open(url) | RowDoes::KeyPage(url) => Some(url),
                        _ => None,
                    };
                    ConnectRow { label, url }
                })
                .collect(),
            action,
            probing: c.probing,
        }
    }

    /// What this stage needs typed, under the address box.
    fn connect_fields(&self) -> Vec<Field> {
        let miniflux = providers::provider(Server::Miniflux);
        match &self.connect.stage {
            Stage::Address | Stage::ByHand => Vec::new(),
            Stage::Miniflux { key: KeyRoute::Password, .. } => miniflux.asks.to_vec(),
            Stage::Miniflux { .. } => {
                miniflux.fields.iter().filter(|f| f.key == FieldKey::MinifluxApiKey).copied().collect()
            }
            Stage::FreshRss { .. } => providers::provider(Server::GReader).asks.to_vec(),
        }
    }

    fn connect_rows(&self) -> Vec<(String, RowDoes)> {
        let row = |label: &str, does| (label.to_string(), does);
        match &self.connect.stage {
            Stage::Address => {
                let mut rows = self.connect.found_rows.clone();
                rows.extend(hosted_rows(providers::HOSTED));
                rows.push(row(providers::BY_HAND_ROW, RowDoes::ByHand));
                rows
            }
            // The page and the paste box are always there: a 401 can't tell a wrong password from an account with none.
            Stage::Miniflux { base, key: KeyRoute::Password } => {
                vec![row(providers::NO_PASSWORD_ROW, RowDoes::KeyPage(format!("{base}/keys/create")))]
            }
            Stage::Miniflux { base, key } => {
                let mut rows = vec![row(providers::KEY_PAGE_ROW, RowDoes::Open(format!("{base}/keys/create")))];
                if *key == KeyRoute::Chosen {
                    rows.push(row(providers::USE_PASSWORD_ROW, RowDoes::UsePassword));
                }
                rows
            }
            Stage::FreshRss { web, .. } => vec![row(providers::OPEN_FRESHRSS_ROW, RowDoes::Open(web.clone()))],
            Stage::ByHand => vec![row(providers::TO_ADDRESS_ROW, RowDoes::ToAddress)],
        }
    }

    /// A row of Connect: Enter on it, or a click.
    fn choose_connect_row(&mut self, row: usize) -> Vec<Effect> {
        let checking = self.settings.as_ref().is_some_and(|f| f.checking);
        let Some((_, does)) = self.connect_rows().into_iter().nth(row).filter(|_| !checking) else {
            return Vec::new();
        };
        match does {
            RowDoes::Open(url) => self.open_link(&url),
            RowDoes::Hosted(url) => {
                let Some(form) = self.settings.as_mut() else { return Vec::new() };
                url.clone_into(&mut form.values.address);
                form.refill += 1;
                url.clone_into(&mut self.connect.address);
                self.look(url)
            }
            RowDoes::KeyPage(url) => {
                self.use_route(KeyRoute::Chosen);
                self.open_link(&url)
            }
            RowDoes::UsePassword => {
                self.use_route(KeyRoute::Password);
                Vec::new()
            }
            RowDoes::ByHand => {
                self.back_to_address();
                self.connect.stage = Stage::ByHand;
                Vec::new()
            }
            RowDoes::ToAddress => {
                self.back_to_address();
                Vec::new()
            }
        }
    }

    /// Asks the window to take `form.values` again, which have no password in them.
    fn forget_password(&mut self) {
        let address = self.connect.address.clone();
        if let Some(form) = self.settings.as_mut() {
            form.values.address = address;
            form.refill += 1;
        }
    }

    fn use_route(&mut self, key: KeyRoute) {
        if let Stage::Miniflux { base, .. } = &self.connect.stage {
            self.connect.stage = Stage::Miniflux { base: base.clone(), key };
            self.forget_password();
            self.clear_connect_error();
        }
    }

    fn clear_connect_error(&mut self) {
        if let Some(form) = self.settings.as_mut() {
            form.error.clear();
        }
    }

    /// Back to the box. What's still being looked at is forgotten, as is what was found.
    fn back_to_address(&mut self) {
        self.latest_probe += 1;
        let password = matches!(self.connect.stage, Stage::Miniflux { key: KeyRoute::Password, .. });
        let Connect { address, from_settings, secret_is_for, .. } = std::mem::take(&mut self.connect);
        self.connect = Connect { address, from_settings, secret_is_for, ..Connect::default() };
        self.shown_link = None;
        if password {
            self.forget_password();
        }
        self.clear_connect_error();
    }

    /// The address box changed. No request goes out for typing: this only makes what was found, or is on its way, out
    /// of date. While a key or a save is being made, the box is read-only.
    pub fn address_edited(&mut self, text: &str) {
        let checking = self.settings.as_ref().is_some_and(|f| f.checking);
        if self.overlay == Overlay::Onboarding && self.connect.address != text && !checking {
            text.clone_into(&mut self.connect.address);
            self.back_to_address();
        }
    }

    fn submit_connect(&mut self, mut values: SettingsValues) -> Vec<Effect> {
        let fields = self.connect_fields();
        let Some(form) = self.settings.as_mut() else { return Vec::new() };
        if form.checking {
            return Vec::new();
        }
        if let Some(field) = fields.iter().find(|f| values.field(f.key).trim().is_empty()) {
            form.error = format!("{} is still empty.", field.label);
            return Vec::new();
        }
        match self.connect.stage.clone() {
            Stage::Address => {
                values.miniflux_password.clear();
                let address = values.address.clone();
                form.values = values;
                self.connect.address.clone_from(&address);
                self.look(&address)
            }
            Stage::ByHand => self.save_values(values),
            Stage::FreshRss { api, .. } => {
                values.server = Server::GReader;
                values.greader_url = api;
                self.save_values(values)
            }
            Stage::Miniflux { base, key: KeyRoute::Password } => {
                let password = std::mem::take(&mut values.miniflux_password);
                let username = values.miniflux_username.trim().to_string();
                form.error.clear();
                form.checking = true;
                form.values = values;
                vec![Effect::MintKey(Login { probe: self.latest_probe, base, username, password })]
            }
            Stage::Miniflux { base, .. } => {
                values.server = Server::Miniflux;
                values.miniflux_url = base;
                self.save_values(values)
            }
        }
    }

    /// A look at what's in the box, by Enter or the button and never by typing.
    fn look(&mut self, text: &str) -> Vec<Effect> {
        if self.connect.probing {
            return Vec::new();
        }
        let Some(form) = self.settings.as_mut() else { return Vec::new() };
        match Address::parse(text) {
            Err(message) => {
                form.error = message;
                Vec::new()
            }
            Ok(address) => {
                form.error.clear();
                self.latest_probe += 1;
                self.connect.probing = true;
                self.connect.found_rows.clear();
                self.connect.look_again = false;
                vec![Effect::Probe { probe: self.latest_probe, address }]
            }
        }
    }

    fn probed(&mut self, found: Detected) {
        self.connect.probing = false;
        let message = found.problem();
        match found {
            Detected::Miniflux { base, sign_in } => {
                let key = if sign_in { KeyRoute::Password } else { KeyRoute::NoPasswords };
                self.connect.plain = base.starts_with("http://");
                self.forget_secret_of_another_server(FieldKey::MinifluxUrl, &base, FieldKey::MinifluxApiKey);
                self.connect.stage = Stage::Miniflux { base, key };
            }
            Detected::FreshRss { web, api, state: Api::Ready } => {
                self.connect.plain = api.starts_with("http://");
                self.forget_secret_of_another_server(FieldKey::GreaderUrl, &api, FieldKey::GreaderPassword);
                self.connect.stage = Stage::FreshRss { web, api };
            }
            Detected::FreshRss { web, state: Api::Off, .. } => {
                self.connect.found_rows = vec![(providers::OPEN_FRESHRSS_ROW.to_string(), RowDoes::Open(web))];
                self.connect.look_again = true;
            }
            Detected::FreshRss { state: Api::Stripped, .. } => {
                let docs = providers::DOCS_MOBILE;
                self.connect.found_rows = vec![(docs.label.to_string(), RowDoes::Open(docs.url.to_string()))];
                self.connect.look_again = true;
            }
            _ => {}
        }
        if let Some(form) = self.settings.as_mut() {
            form.error = message.unwrap_or_default();
        }
    }

    /// The form may hold what was saved for the server before, from Settings. That's not to be sent to this one.
    fn forget_secret_of_another_server(&mut self, url: FieldKey, here: &str, secret: FieldKey) {
        let Some(form) = self.settings.as_mut() else { return };
        let owner = self.connect.secret_is_for.entry(secret).or_insert_with(|| form.values.field(url).to_string());
        if owner == here {
            return;
        }
        here.clone_into(owner);
        if !form.values.field(secret).is_empty() {
            form.values.field_mut(secret).clear();
            form.refill += 1;
        }
    }

    fn key_minted(&mut self, result: Result<String, MintFailed>, fx: &mut Vec<Effect>) {
        let Stage::Miniflux { base, .. } = self.connect.stage.clone() else { return };
        let Some(form) = self.settings.as_mut() else { return };
        form.checking = false;
        form.refill += 1;
        match result {
            Ok(key) => {
                let mut values = form.values.clone();
                values.server = Server::Miniflux;
                values.miniflux_url = base;
                values.miniflux_api_key = key;
                fx.extend(self.save_values(values));
            }
            Err(MintFailed::Refused) => providers::MINT_REFUSED.clone_into(&mut form.error),
            Err(MintFailed::Redirected) => providers::MINT_REDIRECTED.clone_into(&mut form.error),
            Err(MintFailed::Other(message)) => form.error = message,
            Err(MintFailed::NoSuchCall) => self.use_route(KeyRoute::NoMinting),
        }
    }

    /// A save that failed, on the step that made it. What Connect knows makes it a better message, and a key already
    /// made isn't made again: it's in the field now, for another try.
    fn save_failed(&mut self, failed: SaveFailed) {
        let web = match &self.connect.stage {
            Stage::FreshRss { web, .. } if self.overlay == Overlay::Onboarding => Some(web.clone()),
            _ => None,
        };
        if matches!(self.connect.stage, Stage::Miniflux { key: KeyRoute::Password, .. }) {
            self.use_route(KeyRoute::Chosen);
        }
        if let Some(form) = self.settings.as_mut() {
            form.checking = false;
            form.error = match web {
                Some(web) if failed.login_rejected => providers::freshrss_refused(&web),
                _ => failed.message,
            };
        }
    }

    /// The rows of the step on screen, if it has rows.
    fn step_rows(&self) -> usize {
        match self.step {
            Step::Welcome => providers::WELCOME.len(),
            Step::NoServer => providers::NO_SERVER.links().len(),
            Step::Connect => 0,
        }
    }

    /// Onboarding has no reader behind it to fall back on, so what isn't for this step goes on to the window: a
    /// field, or the system (⌘Q).
    fn onboarding_key(&mut self, press: &KeyPress, fx: &mut Vec<Effect>) -> bool {
        if press.ctrl || press.alt || press.meta {
            return false;
        }
        let rows = self.step_rows();
        match press.key.as_str() {
            "Escape" => return self.step_back(fx),
            "ArrowDown" | "j" if rows > 0 && !press.typing => self.step_cursor = (self.step_cursor + 1) % rows,
            "ArrowUp" | "k" if rows > 0 && !press.typing => self.step_cursor = (self.step_cursor + rows - 1) % rows,
            "Enter" if rows > 0 && !press.typing => {
                fx.extend(self.choose_onboarding(self.step_cursor));
            }
            _ => return false,
        }
        true
    }

    /// Settings' way into the address box: everything first run does to find a server, for a change of server.
    pub fn find_server(&mut self) -> Vec<Effect> {
        let checking = self.settings.as_ref().is_some_and(|f| f.checking);
        if (self.overlay != Overlay::Settings && !self.settings_window_open) || checking {
            return Vec::new();
        }
        // With Settings in a window of its own, the overlay is whatever the reader has open.
        let behind = if self.overlay == Overlay::Settings { Overlay::None } else { self.overlay };
        self.settings_window_open = false;
        self.settings = None;
        self.overlay = Overlay::Onboarding;
        self.connect = Connect { from_settings: Some(behind), ..Connect::default() };
        self.go_to_step(Step::Connect, 0);
        vec![Effect::LoadSettings]
    }

    /// Esc: back through Connect's stages, then to where it was opened from. A save that's being checked is seen
    /// through, and so is a key being made.
    fn step_back(&mut self, fx: &mut Vec<Effect>) -> bool {
        let row = |wanted| providers::WELCOME.iter().position(|r| r.choice == wanted).unwrap_or(0);
        let checking = self.settings.as_ref().is_some_and(|f| f.checking);
        match self.step {
            Step::Welcome => return false,
            Step::Connect if checking => return true,
            Step::Connect if self.connect.stage != Stage::Address => self.back_to_address(),
            Step::Connect => {
                let from_settings = self.connect.from_settings.take();
                self.back_to_address();
                self.go_to_step(Step::Welcome, row(Choice::Connect));
                if let Some(behind) = from_settings {
                    self.overlay = behind;
                    self.settings = None;
                    self.open_settings(false, fx);
                }
            }
            Step::NoServer => self.go_to_step(Step::Welcome, row(Choice::NoServer)),
        }
        true
    }

    /// A row of the step on screen: Enter on it, or a click.
    pub fn choose_onboarding(&mut self, row: usize) -> Vec<Effect> {
        if self.overlay != Overlay::Onboarding {
            return Vec::new();
        }
        match self.step {
            Step::Welcome => match providers::WELCOME.get(row).map(|r| r.choice) {
                Some(Choice::Samples) => vec![Effect::UseSamples(true)],
                Some(Choice::Connect) => {
                    self.go_to_step(Step::Connect, 0);
                    Vec::new()
                }
                Some(Choice::NoServer) => {
                    self.go_to_step(Step::NoServer, 0);
                    Vec::new()
                }
                None => Vec::new(),
            },
            Step::NoServer => match providers::NO_SERVER.links().get(row) {
                Some(link) => {
                    self.step_cursor = row;
                    self.open_link(link.url)
                }
                None => Vec::new(),
            },
            Step::Connect => self.choose_connect_row(row),
        }
    }

    /// The backend serves the samples now, so they can be asked for.
    fn samples_on(&mut self, status: Status, fx: &mut Vec<Effect>) {
        if self.overlay != Overlay::Onboarding {
            return;
        }
        self.samples = Some(self.prefs);
        self.readeck = status.readeck;
        self.capabilities = status.capabilities;
        // Connecting later loads the form afresh, with what's really in the config.
        self.settings = None;
        self.shown_link = None;
        self.open_picker(Some(Mode::Unread), fx);
    }

    /// The way out of the samples, into Connect. It starts from a new `App`, so that what the next person adds to
    /// this struct is left behind too. Only what answers in flight are matched against carries over.
    fn leave_samples(&mut self, fx: &mut Vec<Effect>) {
        let Some(prefs) = self.samples else { return };
        let mut fresh = App::new(prefs, self.mac);
        fresh.settings_in_window = self.settings_in_window;
        fresh.latest_load = self.latest_load + 1;
        fresh.latest_probe = self.latest_probe + 1;
        fresh.toast_serial = self.toast_serial;
        *self = fresh;
        self.overlay = Overlay::Onboarding;
        self.go_to_step(Step::Connect, 0);
        fx.extend([Effect::UseSamples(false), Effect::LoadSettings]);
    }

    // ---- Prefs -----------------------------------------------------------

    /// The window's colours, from Settings. Unlike the servers there's nothing to check, so it applies at once.
    pub fn set_colours(&mut self, colours: Colours) -> Vec<Effect> {
        let mut fx = Vec::new();
        self.change_prefs(|p| p.colours = colours, &mut fx);
        fx
    }

    fn change_prefs(&mut self, change: impl FnOnce(&mut Prefs), fx: &mut Vec<Effect>) {
        let before = self.prefs;
        change(&mut self.prefs);
        if self.prefs != before && !self.samples() {
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
            Overlay::Onboarding => return self.onboarding_key(press, fx),
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
            // Typing goes to the box; Enter there searches (`search`).
            Overlay::Search => {
                if key == "Escape" {
                    self.close_search();
                    return true;
                }
                return false;
            }
            Overlay::Image => {
                return !(press.ctrl || press.alt || press.meta) && self.viewer_key(key, fx);
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
            Some(Action::Arrow { up }) => self.arrow(up, press, fx),
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

    /// ↓ and ↑ scroll, and where there's no further to go, a new press goes on to the next article, or the top of the
    /// previous one. A held key stops there: it would otherwise run through the list, marking it read.
    fn arrow(&mut self, up: bool, press: &KeyPress, fx: &mut Vec<Effect>) -> bool {
        // Only an article scrolls: on the other screens what the window says about it means nothing.
        let article = self.current().is_some();
        let at_the_edge = !article || if up { press.at_top } else { press.at_bottom };
        if !at_the_edge {
            fx.push(Effect::ScrollStep { up });
        } else if press.held {
            // There.
        } else if up {
            self.prev();
        } else {
            self.advance(true, fx);
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
        // The Mac's menus are there on first run too, with nothing behind it for them to work on.
        if self.overlay == Overlay::Onboarding {
            return fx;
        }
        // The Mac's toolbar and menus stay at hand while the viewer is up; they're about the article.
        self.close_viewer();
        match action {
            Action::Space { up } => {
                fx.push(Effect::ScrollPage { up });
            }
            Action::Arrow { up } => {
                fx.push(Effect::ScrollStep { up });
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
            Action::ToggleStar => self.toggle_star(fx),
            Action::OpenOriginal => self.open_original(fx),
            Action::SaveToReadeck => self.save_to_readeck(fx),
            Action::FetchFull => self.fetch_full(fx),
            Action::MarkAllRead if self.overlay == Overlay::Picker => self.mark_row_read(fx),
            Action::MarkAllRead => self.mark_all_read(fx),
            Action::Search => self.open_search(),
            Action::FlipOrder => self.flip_order(fx),
            Action::ViewImages => self.open_viewer(None),
            Action::ToggleMode => self.toggle_mode(fx),
            Action::PickerMode(mode) if mode == self.picker_mode => {}
            Action::PickerMode(mode) => self.open_picker(Some(mode), fx),
            Action::OpenPicker => self.open_picker(None, fx),
            Action::Narrower => self.change_prefs(Prefs::narrower, fx),
            Action::Wider => self.change_prefs(Prefs::wider, fx),
            Action::BiggerFont => self.change_prefs(Prefs::bigger, fx),
            Action::SmallerFont => self.change_prefs(Prefs::smaller, fx),
            Action::ResetSize => self.change_prefs(Prefs::reset, fx),
            Action::Help => self.open_help(),
            Action::Quit => fx.push(Effect::Quit),
            // The form's saves would go nowhere in the samples, and its other half isn't kept: Connect is what's left.
            Action::Settings if self.samples() => self.leave_samples(fx),
            Action::Settings => self.open_settings(false, fx),
            Action::PickerMove(delta) => self.move_picker(delta),
            Action::PickerChoose => self.choose(None, fx),
            Action::PickerRefresh if !self.capabilities.refresh => self.toast_info(feeds::REFRESH_REFUSED),
            Action::PickerRefresh => {
                self.toast_info("Refreshing feeds…");
                fx.push(Effect::RefreshFeeds);
            }
            Action::PickerClose => {
                if self.picker_can_close() {
                    self.close_picker();
                }
            }
            Action::Space { .. } | Action::Arrow { .. } | Action::Enter => {}
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
            Outcome::StarredSet { index, id, starred, result } => self.starred_set(index, id, starred, result),
            Outcome::ContentFetched { index, id, result } => self.content_fetched(index, id, result),
            Outcome::FeedCounts(result) => self.feed_counts(result),
            Outcome::Refreshed(result) => self.refreshed(result, &mut fx),
            Outcome::PickerReloadDue => {
                if self.overlay == Overlay::Picker {
                    self.load_picker(&mut fx);
                }
            }
            Outcome::SavedToReadeck { index, id, result } => self.saved_to_readeck(index, id, result, &mut fx),
            Outcome::OpenedInBrowser { then, result } => self.opened_in_browser(then, result, &mut fx),
            Outcome::UnreadCounted { load, selection, up_to, later, result } => {
                self.unread_counted(load, selection, up_to, later, result);
            }
            Outcome::MarkedAllRead { load, result } => self.marked_all_read(load, result, &mut fx),
            Outcome::RowCounted { selection, label, result } => self.row_counted(selection, label, result),
            Outcome::RowMarkedRead { selection, label, up_to, result } => {
                self.row_marked_read(selection, &label, up_to, result, &mut fx);
            }
            Outcome::SettingsLoaded { values, problem } => {
                if self.in_settings() || self.settings_window_open {
                    let refill = 0;
                    self.settings =
                        Some(SettingsForm { values, problem, error: String::new(), checking: false, refill });
                }
            }
            Outcome::SettingsSaved(result) => match result {
                Ok(saved) => self.close_settings(Some(saved), &mut fx),
                Err(message) => self.save_failed(message),
            },
            Outcome::Probed { probe, found } => {
                if probe == self.latest_probe {
                    self.probed(found);
                }
            }
            Outcome::KeyMinted { probe, result } => {
                if probe == self.latest_probe {
                    self.key_minted(result, &mut fx);
                }
            }
            Outcome::SamplesSwitched { on, status } => {
                if on && !self.samples() {
                    self.samples_on(status, &mut fx);
                }
            }
        }
        fx
    }
}

/// The rows of hosted services. Each goes to the same probe as a typed address.
fn hosted_rows(table: &[Provider]) -> Vec<(String, RowDoes)> {
    let hosted = table.iter().filter_map(|p| p.fixed_url.map(|url| (p.label, url)));
    hosted.map(|(label, url)| (label.to_string(), RowDoes::Hosted(url))).collect()
}

/// Feeds a selection takes in.
enum FeedSet {
    All,
    /// Whichever feed, if starred.
    Starred,
    Some(Vec<i64>),
}

impl FeedSet {
    fn contains(&self, entry: &Entry) -> bool {
        match self {
            FeedSet::All => true,
            FeedSet::Starred => entry.starred,
            FeedSet::Some(ids) => ids.contains(&entry.feed.id),
        }
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

impl Screen {
    /// As the windows name it.
    pub fn name(self) -> &'static str {
        match self {
            Screen::Settings => "settings",
            Screen::Loading => "loading",
            Screen::Article => "article",
            Screen::End => "end",
            Screen::Empty => "empty",
        }
    }
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
