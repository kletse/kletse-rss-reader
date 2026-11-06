//! The C interface of the native macOS app (macos/), which does for SwiftUI what bridge.rs does for Qt. It owns
//! the `App` and runs the effects it asks for. Swift sends each command as JSON (`kr_send`) and gets back what
//! the window shows, as JSON. Server calls run on a tokio runtime; when one answers, `notify` asks Swift to send
//! `pump` from the main thread, which applies the answer there. So the app only ever changes on the main thread.
//!
//! The article body goes out only when it was converted again (a new article, new text, a new palette), with
//! `serial` changing only for a new article or new text, which is what scrolls back to the top. The next article
//! is converted ahead on the runtime, and its images listed for Swift to load, so it's ready when you get there.

#![allow(unsafe_code)] // the C interface: pointers Swift hands over

use crate::app::{App, ConnectView, Effect, FeedFile, KeyPress, Outcome, Overlay, PickerList, ToastKind, TransferView};
use crate::backend::Backend;
use crate::content::{convert, native};
use crate::feeds::Capabilities;
use crate::grant::GrantView;
use crate::keys;
use crate::manage::ManageView;
use crate::paging::Mode;
use crate::prefs;
use crate::providers::{FIRST_RUN, Field, FieldKey, FirstRun, PROVIDERS, Provider, READECK};
use crate::theme::{self, Palette};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::ffi::{CStr, CString, c_char, c_void};
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock, mpsc};

static RUNTIME: LazyLock<tokio::runtime::Runtime> = LazyLock::new(|| {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .thread_name("onward-io")
        .enable_all()
        .build()
        .expect("a tokio runtime starts on any supported platform")
});

/// `ONWARD_DEBUG=1`: say on stderr what an article had that the window can't show (inline SVG, MathML).
static DEBUG: LazyLock<bool> = LazyLock::new(|| std::env::var("ONWARD_DEBUG").is_ok_and(|v| v == "1"));

/// Called from a background thread when answers are waiting: Swift then sends `pump` on the main thread.
pub type Notify = extern "C" fn(ctx: *mut c_void);

struct Waker {
    notify: Notify,
    ctx: *mut c_void,
    /// A `pump` is already asked for; more answers join it.
    pending: AtomicBool,
}

// SAFETY: `ctx` is only handed back to `notify`, which Swift makes safe to call from any thread (it hops to the
// main queue), and Swift keeps what it points to alive for as long as the core.
unsafe impl Send for Waker {}
unsafe impl Sync for Waker {}

impl Waker {
    fn wake(&self) {
        if !self.pending.swap(true, Ordering::AcqRel) {
            (self.notify)(self.ctx);
        }
    }
}

/// What the article on screen was rendered from, to render it again only when that changes.
#[derive(PartialEq, Eq, Clone, Copy)]
struct Rendered {
    id: i64,
    content: u64,
    palette: u64,
}

/// The next article, converted in the background while this one is read.
struct Prefetched {
    key: Rendered,
    blocks: Vec<native::NativeBlock>,
    /// What the converter dropped, for `ONWARD_DEBUG=1`.
    lost: String,
    images: Vec<String>,
}

/// What comes back from the runtime for `pump` to take in.
enum Answer {
    Outcome(Outcome),
    Prefetched(Prefetched),
}

pub struct Core {
    app: App,
    backend: Arc<Backend>,
    palette: Palette,
    answers: mpsc::Sender<Answer>,
    waiting: mpsc::Receiver<Answer>,
    waker: Arc<Waker>,
    rendered: Option<Rendered>,
    prefetched: Option<Prefetched>,
    /// The article being converted ahead, so it isn't started twice.
    prefetching: Option<Rendered>,
    prefetch_images: Vec<String>,
    article_serial: u64,
    /// Whether the settings form on screen was filled in from the loaded values yet.
    settings_shown: bool,
    settings_serial: u64,
    /// The refill it was last filled in from.
    settings_refill: u64,
}

impl std::fmt::Debug for Core {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Core { .. }")
    }
}

#[derive(Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
enum Command {
    Start {
        dark: bool,
        /// The system accent, for the original look; left out, the logo's colours.
        accent: Option<String>,
    },
    /// The system appearance or accent colour changed, or the choice of colours.
    Theme {
        dark: bool,
        accent: Option<String>,
    },
    Key {
        key: String,
        #[serde(default)]
        shift: bool,
        /// ⌘
        #[serde(default)]
        ctrl: bool,
        #[serde(default)]
        alt: bool,
        /// The Control key
        #[serde(default)]
        meta: bool,
        #[serde(default)]
        typing: bool,
        #[serde(default)]
        at_bottom: bool,
    },
    /// A button: see `keys::button_action`.
    Press {
        button: String,
    },
    ChooseRow {
        row: usize,
    },
    PickerBackdrop,
    CloseHelp,
    OpenLink {
        url: String,
    },
    SubmitSettings {
        /// An `id` from the form's `providers`.
        #[serde(default)]
        server: String,
        /// Every field's value, by its `key`.
        #[serde(default)]
        fields: HashMap<String, String>,
    },
    CancelSettings,
    /// A row of the first-run step on screen.
    ChooseOnboarding {
        row: usize,
    },
    /// The address box changed.
    AddressEdited {
        text: String,
    },
    /// A row of Readeck's way in, with what's in its URL field.
    ReadeckRow {
        row: usize,
        address: String,
    },
    /// The Readeck URL field changed.
    ReadeckEdited {
        text: String,
    },
    /// Settings' way to the address box.
    FindServer,
    /// The search box's Enter.
    Search {
        query: String,
    },
    CancelSearch,
    /// A click on an image in the article.
    ViewImage {
        src: String,
    },
    /// A click in the image viewer.
    CloseViewer,
    /// The import and export panel's main button, the same as Enter.
    TransferAction,
    /// Its Cancel or Close, or a click beside it.
    CancelTransfer,
    /// A row of the feeds panel.
    ManageRow {
        row: usize,
    },
    /// Its field's Enter, or its button.
    SubmitManage {
        text: String,
    },
    /// A step back, or a click beside it.
    CancelManage,
    /// What an open or save panel came up with. A save panel has asked about a file that's there, so it says `replace`.
    FeedFile {
        path: String,
        #[serde(default)]
        replace: bool,
    },
    DismissToast {
        serial: u64,
    },
    /// Apply the answers that came in.
    Pump,
}

#[derive(Serialize)]
struct Reply {
    /// Whether a key was used; Swift passes unused ones on (⌘Q, typing in a field).
    used: bool,
    /// "scroll_up", "scroll_down", "quit", "show_settings", "choose_import_file", "choose_export_file".
    events: Vec<&'static str>,
    state: State,
}

#[derive(Serialize)]
struct State {
    screen: &'static str,
    overlay: &'static str,
    /// The picker is on screen: open, or behind the question Shift+A on a row asks.
    picker_shown: bool,
    end_title: &'static str,
    /// Where j goes from the end of the list.
    end_hint: String,
    window_title: String,
    selection_label: String,
    history: bool,
    /// Reading search results.
    searching: bool,
    /// Reading the starred items.
    starred_list: bool,
    position: String,
    readeck: bool,
    capabilities: Capabilities,
    /// Reading the sample articles, chosen on the first screen.
    samples: bool,
    /// The last link opened from a form: a browser may not have opened.
    shown_link: Option<String>,
    /// First run, while it's on.
    onboarding: Option<Onboarding>,
    article: Option<Article>,
    picker: Picker,
    /// The search box starts from the last search.
    search_query: String,
    viewer: Option<ViewerState>,
    /// The next article's images, to load while this one is read.
    prefetch_images: Vec<String>,
    help: Vec<(String, String)>,
    confirm_text: String,
    /// How to answer it, what its button says, and whether what it does can be taken back.
    confirm_hint: &'static str,
    confirm_action: &'static str,
    confirm_lasting: bool,
    confirm_told: bool,
    /// Importing or exporting a feed list.
    transfer: Option<TransferView>,
    /// The feeds panel, while it's open or a change from it is under way.
    manage: Option<ManageView>,
    /// It's on screen: open, or behind the question before a loss.
    manage_shown: bool,
    /// Anything about the feeds can be changed here.
    manages: bool,
    toast: Option<ToastState>,
    settings: Option<Settings>,
    font_size: i32,
    width_factor: i32,
}

#[derive(Serialize)]
struct Article {
    serial: u64,
    id: i64,
    title: String,
    url: String,
    feed_title: String,
    author: String,
    published_at: String,
    reading_time: i64,
    status: &'static str,
    /// Starred on the server (`*`).
    starred: bool,
    /// Only when converted again; Swift keeps the last ones.
    #[serde(skip_serializing_if = "Option::is_none")]
    blocks: Option<Vec<native::NativeBlock>>,
}

#[derive(Serialize)]
struct Picker {
    state: &'static str,
    error: String,
    rows: Vec<PickerRow>,
    cursor: usize,
    history: bool,
    /// Unread items come newest first.
    newest_first: bool,
    /// What it says with no rows, and whether it then offers to import.
    empty_text: String,
    offers_import: bool,
    offers_add: bool,
}

#[derive(Serialize)]
struct ViewerState {
    src: String,
    alt: String,
    /// The link the image is in, if any.
    href: Option<String>,
    index: usize,
    count: usize,
}

#[derive(Serialize)]
struct PickerRow {
    label: String,
    /// `null` when the server can't count: shown without a number.
    count: Option<i64>,
    depth: u8,
}

#[derive(Serialize)]
struct ToastState {
    message: String,
    error: bool,
    serial: u64,
}

#[derive(Serialize)]
struct Onboarding {
    /// "welcome", "connect" or "no_server".
    step: &'static str,
    /// The row the arrows are on.
    cursor: usize,
    words: FirstRun,
    connect: ConnectView,
}

#[derive(Serialize)]
struct Settings {
    /// What the form offers.
    providers: &'static [Provider],
    readeck_fields: &'static [Field],
    /// The way into Readeck that needs no token.
    readeck_grant: GrantView,
    /// What the button that leads to the address box says.
    find_server: &'static str,
    problem: String,
    error: String,
    checking: bool,
    /// Bumped when `values` is sent: the form fills in from them once, then keeps what is typed, until the core has
    /// something to take back.
    serial: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    values: Option<Values>,
}

#[derive(Serialize)]
struct Values {
    server: &'static str,
    /// A list and not an object: Swift's decoder would rewrite an object's keys as camelCase.
    fields: Vec<Value>,
}

#[derive(Serialize)]
struct Value {
    key: FieldKey,
    value: String,
}

fn hash(value: &impl Hash) -> u64 {
    let mut h = std::hash::DefaultHasher::new();
    value.hash(&mut h);
    h.finish()
}

impl Core {
    fn new(waker: Waker) -> Self {
        let demo = std::env::var("ONWARD_DEMO").is_ok_and(|v| v == "1");
        let (answers, waiting) = mpsc::channel();
        let mut app = App::new(prefs::load(), true);
        app.use_settings_window();
        Self {
            app,
            backend: Arc::new(Backend::new(demo)),
            palette: Palette::default(),
            answers,
            waiting,
            waker: Arc::new(waker),
            rendered: None,
            prefetched: None,
            prefetching: None,
            prefetch_images: Vec::new(),
            article_serial: 0,
            settings_shown: false,
            settings_serial: 0,
            settings_refill: 0,
        }
    }

    fn set_theme(&mut self, dark: bool, accent: Option<&str>) {
        self.palette = Palette::from(&theme::macos(dark, accent));
    }

    fn handle(&mut self, command: Command) -> Reply {
        let mut events = Vec::new();
        let mut used = true;
        let fx = match command {
            Command::Start { dark, accent } => {
                self.set_theme(dark, accent.as_deref());
                // Loading the grammars takes a moment; better now than on the first code block.
                std::thread::spawn(crate::content::highlight::warm_up);
                let status = self.backend.status();
                self.app.start(status)
            }
            Command::Theme { dark, accent } => {
                self.set_theme(dark, accent.as_deref());
                Vec::new()
            }
            Command::Key { key, shift, ctrl, alt, meta, typing, at_bottom } => {
                let press = KeyPress { key, shift, ctrl, alt, meta, typing, at_bottom };
                let (was_used, fx) = self.app.key(&press);
                used = was_used;
                fx
            }
            Command::Press { button } => keys::button_action(&button).map(|a| self.app.action(a)).unwrap_or_default(),
            Command::ChooseRow { row } => self.app.choose_row(row),
            Command::PickerBackdrop => {
                self.app.picker_backdrop();
                Vec::new()
            }
            Command::CloseHelp => {
                self.app.close_help_overlay();
                Vec::new()
            }
            Command::OpenLink { url } => self.app.open_link(&url),
            Command::SubmitSettings { server, fields } => self.app.submit_fields(&server, &fields),
            Command::CancelSettings => self.app.cancel_settings(),
            Command::ChooseOnboarding { row } => self.app.choose_onboarding(row),
            Command::AddressEdited { text } => {
                self.app.address_edited(&text);
                Vec::new()
            }
            Command::ReadeckRow { row, address } => self.app.readeck_row(row, &address),
            Command::ReadeckEdited { text } => {
                self.app.readeck_edited(&text);
                Vec::new()
            }
            Command::FindServer => self.app.find_server(),
            Command::Search { query } => self.app.search(&query),
            Command::CancelSearch => {
                self.app.cancel_search();
                Vec::new()
            }
            Command::ViewImage { src } => {
                self.app.view_image(&src);
                Vec::new()
            }
            Command::CloseViewer => {
                self.app.close_viewer();
                Vec::new()
            }
            Command::TransferAction => self.app.transfer_action(),
            Command::CancelTransfer => self.app.cancel_transfer(),
            Command::ManageRow { row } => self.app.manage_row(row),
            Command::SubmitManage { text } => self.app.submit_manage(&text),
            Command::CancelManage => self.app.cancel_manage(),
            Command::FeedFile { path, replace } => self.app.feed_file(&path, replace),
            Command::DismissToast { serial } => {
                self.app.dismiss_toast(serial);
                Vec::new()
            }
            Command::Pump => {
                self.waker.pending.store(false, Ordering::Release);
                let mut fx = Vec::new();
                while let Ok(answer) = self.waiting.try_recv() {
                    match answer {
                        Answer::Outcome(outcome) => {
                            fx.extend(self.app.apply(outcome));
                            // What an answer asks for runs before the next answer is applied, as it would in Qt.
                            self.run(std::mem::take(&mut fx), &mut events);
                        }
                        Answer::Prefetched(prefetched) => self.prefetched(prefetched),
                    }
                }
                fx
            }
        };
        self.run(fx, &mut events);
        Reply { used, events, state: self.state() }
    }

    /// Starts what the app asked for: server calls in the background, the rest as events for the window.
    fn run(&mut self, fx: Vec<Effect>, events: &mut Vec<&'static str>) {
        for effect in fx {
            match effect {
                Effect::SavePrefs(p) => {
                    if !self.backend.demo() {
                        prefs::save(p);
                    }
                }
                Effect::ScrollPage { up } => events.push(if up { "scroll_up" } else { "scroll_down" }),
                Effect::Quit => events.push("quit"),
                Effect::ShowSettings => events.push("show_settings"),
                Effect::ChooseFeedFile(FeedFile::Import) => events.push("choose_import_file"),
                Effect::ChooseFeedFile(FeedFile::Export) => events.push("choose_export_file"),
                effect => {
                    let backend = Arc::clone(&self.backend);
                    let answers = self.answers.clone();
                    let waker = Arc::clone(&self.waker);
                    RUNTIME.spawn(async move {
                        if let Some(outcome) = backend.run(effect).await
                            && answers.send(Answer::Outcome(outcome)).is_ok()
                        {
                            waker.wake();
                        }
                    });
                }
            }
        }
    }

    /// Converts the next article on the runtime; `pump` takes it in.
    fn prefetch(&mut self) {
        let Some(next) = self.app.upcoming().cloned() else { return };
        let key = Rendered { id: next.id, content: hash(&next.content), palette: hash(&format!("{:?}", self.palette)) };
        if self.prefetched.as_ref().is_some_and(|p| p.key == key) || self.prefetching == Some(key) {
            return;
        }
        self.prefetching = Some(key);
        let palette = self.palette.clone();
        let (answers, waker) = (self.answers.clone(), Arc::clone(&self.waker));
        RUNTIME.spawn_blocking(move || {
            let rendered = convert::convert(&next.content, &next.url, &palette);
            let prefetched = Prefetched {
                key,
                blocks: native::native(&rendered.blocks),
                lost: rendered.stats.summary(),
                images: convert::image_refs(&rendered.blocks).into_iter().map(|i| i.src).collect(),
            };
            if answers.send(Answer::Prefetched(prefetched)).is_ok() {
                waker.wake();
            }
        });
    }

    /// Keeps the converted article, unless you've moved on and another one is being converted meanwhile.
    fn prefetched(&mut self, prefetched: Prefetched) {
        if self.prefetching != Some(prefetched.key) {
            return;
        }
        self.prefetching = None;
        self.prefetch_images.clone_from(&prefetched.images);
        self.prefetched = Some(prefetched);
    }

    fn state(&mut self) -> State {
        let article = self.article();
        if article.is_some() {
            self.prefetch();
        }
        let settings = self.settings();
        let app = &self.app;
        let (picker_state, picker_error) = match app.picker_list() {
            PickerList::Loading => ("loading", String::new()),
            PickerList::Error(message) => ("error", message.clone()),
            PickerList::Ready(_) => ("ready", String::new()),
        };
        let rows = app
            .picker_items()
            .into_iter()
            .map(|i| PickerRow { label: i.label, count: i.count, depth: i.depth })
            .collect();
        let prefs = app.prefs();
        State {
            screen: app.screen().name(),
            overlay: app.overlay().name(),
            picker_shown: app.picker_shown(),
            end_title: app.end_title(),
            end_hint: app.end_hint(),
            window_title: app.window_title(),
            selection_label: app.selection_label().to_string(),
            history: app.history_badge(),
            searching: app.searching(),
            starred_list: app.starred_list(),
            position: app.position(),
            readeck: app.readeck(),
            capabilities: app.capabilities(),
            samples: app.samples(),
            shown_link: app.shown_link().map(str::to_string),
            onboarding: (app.overlay() == Overlay::Onboarding).then(|| Onboarding {
                step: app.step().name(),
                cursor: app.step_cursor(),
                words: FIRST_RUN,
                connect: app.connect_view(),
            }),
            article,
            picker: Picker {
                state: picker_state,
                error: picker_error,
                rows,
                cursor: app.picker_cursor(),
                history: app.picker_mode() == Mode::History,
                newest_first: app.newest_first(),
                empty_text: app.picker_empty_text(),
                offers_import: app.offers_import(),
                offers_add: app.offers_add(),
            },
            search_query: app.search_query().to_string(),
            viewer: app.viewer().map(|v| {
                let image = &v.images[v.index];
                ViewerState {
                    src: image.src.clone(),
                    alt: image.alt.clone(),
                    href: image.href.clone(),
                    index: v.index,
                    count: v.images.len(),
                }
            }),
            prefetch_images: self.prefetch_images.clone(),
            help: app.help_lines(),
            confirm_text: app.confirm_text().to_string(),
            confirm_hint: app.confirm_hint(),
            confirm_action: app.confirm_action(),
            confirm_lasting: app.confirm_lasting(),
            confirm_told: app.confirm_told(),
            transfer: app.transfer_view(),
            manage: app.manage_view(),
            manage_shown: app.manage_shown(),
            manages: app.capabilities().feeds.any(),
            toast: app.toast().map(|t| ToastState {
                message: t.message.clone(),
                error: t.kind == ToastKind::Error,
                serial: t.serial,
            }),
            settings,
            font_size: prefs.font_size,
            width_factor: prefs.width_factor,
        }
    }

    fn article(&mut self) -> Option<Article> {
        let entry = self.app.current()?.clone();
        let key =
            Rendered { id: entry.id, content: hash(&entry.content), palette: hash(&format!("{:?}", self.palette)) };
        let previous = self.rendered;
        let blocks = (previous != Some(key)).then(|| {
            let (blocks, lost) = match self.prefetched.take_if(|p| p.key == key) {
                Some(p) => (p.blocks, p.lost),
                None => {
                    let rendered = convert::convert(&entry.content, &entry.url, &self.palette);
                    (native::native(&rendered.blocks), rendered.stats.summary())
                }
            };
            if *DEBUG && !lost.is_empty() {
                eprintln!("onward: entry {}: {lost}", entry.id);
            }
            self.rendered = Some(key);
            if previous.is_none_or(|p| p.id != key.id || p.content != key.content) {
                // A new article, or new text for it (`f`): it starts from the top.
                self.article_serial += 1;
            }
            blocks
        });
        Some(Article {
            serial: self.article_serial,
            id: entry.id,
            title: crate::app::display_title(&entry.title).to_string(),
            url: entry.url,
            feed_title: entry.feed.title,
            author: entry.author,
            published_at: entry.published_at,
            reading_time: entry.reading_time,
            status: crate::app::status_name(entry.status),
            starred: entry.starred,
            blocks,
        })
    }

    fn settings(&mut self) -> Option<Settings> {
        let Some(form) = self.app.settings() else {
            self.settings_shown = false;
            return None;
        };
        // The form starts from the loaded values once; after that the window keeps what is typed, until the core has
        // something to take back.
        let fill = !self.settings_shown || self.settings_refill != form.refill;
        let refill = form.refill;
        let values = fill.then(|| Values {
            server: form.values.server.name(),
            fields: FieldKey::ALL.map(|key| Value { key, value: form.values.field(key).to_string() }).into(),
        });
        let settings = Settings {
            providers: &PROVIDERS,
            readeck_fields: &READECK,
            readeck_grant: self.app.readeck_view(),
            find_server: FIRST_RUN.find_server,
            problem: form.problem.clone().unwrap_or_default(),
            error: form.error.clone(),
            checking: form.checking,
            serial: self.settings_serial + u64::from(values.is_some()),
            values,
        };
        self.settings_shown = true;
        self.settings_refill = refill;
        self.settings_serial = settings.serial;
        Some(settings)
    }
}

/// A new core. `notify(ctx)` is called from background threads when `pump` has work.
#[unsafe(no_mangle)]
pub extern "C" fn kr_new(notify: Notify, ctx: *mut c_void) -> *mut Core {
    Box::into_raw(Box::new(Core::new(Waker { notify, ctx, pending: AtomicBool::new(false) })))
}

/// Runs one JSON command and returns the reply as JSON, to be freed with `kr_free_string`. A command that
/// doesn't parse gets `{"error": …}`.
///
/// # Safety
/// `core` comes from `kr_new` and is used from one thread only (the main thread); `command` is a
/// NUL-terminated UTF-8 string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn kr_send(core: *mut Core, command: *const c_char) -> *mut c_char {
    // SAFETY: as the caller promises above.
    let (core, command) = unsafe { (&mut *core, CStr::from_ptr(command)) };
    let reply = match serde_json::from_slice::<Command>(command.to_bytes()) {
        Ok(command) => serde_json::to_string(&core.handle(command)),
        Err(e) => serde_json::to_string(&serde_json::json!({ "error": e.to_string() })),
    };
    let reply = reply.unwrap_or_else(|e| format!("{{\"error\":{:?}}}", e.to_string()));
    // JSON escapes every control character, NUL included.
    CString::new(reply).unwrap_or_default().into_raw()
}

/// Frees a reply from `kr_send`.
///
/// # Safety
/// `reply` comes from `kr_send` and is freed once.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn kr_free_string(reply: *mut c_char) {
    if !reply.is_null() {
        // SAFETY: as the caller promises above.
        drop(unsafe { CString::from_raw(reply) });
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::app::ConnectRow;
    use std::sync::atomic::AtomicUsize;

    static WAKES: AtomicUsize = AtomicUsize::new(0);

    extern "C" fn count(_: *mut c_void) {
        WAKES.fetch_add(1, Ordering::SeqCst);
    }

    fn send(core: *mut Core, json: &str) -> serde_json::Value {
        let command = CString::new(json).unwrap();
        // SAFETY: a core from kr_new, a NUL-terminated command, the reply freed once.
        unsafe {
            let reply = kr_send(core, command.as_ptr());
            let value = serde_json::from_str(CStr::from_ptr(reply).to_str().unwrap()).unwrap();
            kr_free_string(reply);
            value
        }
    }

    /// What Swift reads of first run and the form, in the names `State.swift` takes them in after its decoder turns
    /// them into camelCase. An object's keys would be turned as well, hence the list of values.
    #[test]
    fn first_run_reaches_swift_as_it_expects() {
        let connect = ConnectView {
            stage: "address",
            text: "t".into(),
            note: "n".into(),
            fields: PROVIDERS[0].asks.to_vec(),
            rows: vec![ConnectRow { label: "l".into(), url: Some("u".into()) }],
            action: "Continue",
            probing: false,
        };
        let onboarding = Onboarding { step: "welcome", cursor: 0, words: FIRST_RUN, connect };
        let json = serde_json::to_value(&onboarding).unwrap();
        for path in [
            "/step",
            "/words/welcome_title",
            "/words/welcome_text",
            "/words/welcome/0/label",
            "/words/welcome/2/quiet",
            "/words/connect_title",
            "/words/connect_text",
            "/words/by_hand_text",
            "/words/address/key",
            "/words/address/label",
            "/words/no_server/title",
            "/connect/stage",
            "/connect/text",
            "/connect/note",
            "/connect/fields/1/secret",
            "/connect/rows/0/label",
            "/connect/rows/0/url",
            "/connect/action",
            "/connect/probing",
            "/words/no_server/text",
            "/words/no_server/offers/0/price",
            "/words/no_server/offers/1/link/url",
            "/words/no_server/note",
            "/words/no_server/more/label",
        ] {
            assert!(json.pointer(path).is_some(), "{path}");
        }
        let provider = serde_json::to_value(PROVIDERS[1]).unwrap();
        for path in ["/id", "/short_label", "/fields/0/key", "/fields/2/secret", "/fields/2/help", "/links/0/url"] {
            assert!(provider.pointer(path).is_some(), "{path}");
        }
        let values = Values { server: "greader", fields: vec![Value { key: FieldKey::GreaderUrl, value: "u".into() }] };
        assert_eq!(
            serde_json::to_value(values).unwrap(),
            serde_json::json!({ "server": "greader", "fields": [{ "key": "greader_url", "value": "u" }] })
        );
    }

    #[test]
    fn the_commands_swift_sends_for_first_run_parse() {
        let submit = r#"{"cmd":"submit_settings","server":"greader","fields":{"greader_url":"u"}}"#;
        let Ok(Command::SubmitSettings { server, fields }) = serde_json::from_str(submit) else { panic!("{submit}") };
        assert_eq!((server.as_str(), fields["greader_url"].as_str()), ("greader", "u"));
        let choose = r#"{"cmd":"choose_onboarding","row":2}"#;
        assert!(matches!(serde_json::from_str(choose), Ok(Command::ChooseOnboarding { row: 2 })));
        let row = r#"{"cmd":"readeck_row","row":1,"address":"https://r.example.com"}"#;
        assert!(matches!(serde_json::from_str(row), Ok(Command::ReadeckRow { row: 1, .. })));
        let readeck = r#"{"cmd":"readeck_edited","text":"r"}"#;
        assert!(matches!(serde_json::from_str(readeck), Ok(Command::ReadeckEdited { .. })));
        let edited = r#"{"cmd":"address_edited","text":"rss"}"#;
        assert!(matches!(serde_json::from_str(edited), Ok(Command::AddressEdited { text }) if text == "rss"));
        assert!(matches!(serde_json::from_str(r#"{"cmd":"find_server"}"#), Ok(Command::FindServer)));
    }

    /// The demo, through the C interface: it starts, the answers come back through `pump`, and the article's
    /// blocks are sent once.
    #[test]
    fn the_demo_opens_through_the_c_interface() {
        // SAFETY: setting the variable before anything reads it; no other test in this module reads it.
        unsafe { std::env::set_var("ONWARD_DEMO", "1") };
        let core = kr_new(count, std::ptr::null_mut());
        assert!(send(core, r#"{"cmd":"nonsense"}"#)["error"].is_string());
        let started = send(core, r#"{"cmd":"start","dark":true}"#);
        assert_eq!(started["state"]["overlay"], "picker");

        let mut state = started["state"].clone();
        for _ in 0..200 {
            if state["picker"]["state"] == "ready" {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
            state = send(core, r#"{"cmd":"pump"}"#)["state"].clone();
        }
        assert_eq!(state["picker"]["state"], "ready");
        assert!(WAKES.load(Ordering::SeqCst) > 0);

        send(core, r#"{"cmd":"key","key":"Enter"}"#);
        for _ in 0..200 {
            if state["screen"] == "article" {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
            state = send(core, r#"{"cmd":"pump"}"#)["state"].clone();
        }
        assert_eq!(state["screen"], "article");
        assert!(state["article"]["blocks"].is_array());
        assert_eq!(state["article"]["serial"], 1);
        let again = send(core, r#"{"cmd":"pump"}"#);
        assert!(again["state"]["article"]["blocks"].is_null(), "blocks go out once");

        let space = send(core, r#"{"cmd":"key","key":" "}"#);
        assert_eq!(space["events"][0], "scroll_down");
        assert_eq!(send(core, r#"{"cmd":"key","key":"x"}"#)["used"], false);

        // The first demo article has an image: the viewer, and Esc closes it.
        let viewer = send(core, r#"{"cmd":"key","key":"v"}"#)["state"].clone();
        assert_eq!((viewer["overlay"].as_str(), viewer["viewer"]["count"].as_u64()), (Some("image"), Some(1)));
        assert!(send(core, r#"{"cmd":"key","key":"Escape"}"#)["state"]["viewer"].is_null());

        // The next article was converted ahead: moving on sends its blocks without converting it again.
        // SAFETY: a look at the core between commands, on the thread that sends them.
        let prefetched = |id: i64| unsafe { (*core).prefetched.as_ref().is_some_and(|p| p.key.id == id) };
        for _ in 0..200 {
            if prefetched(2) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
            send(core, r#"{"cmd":"pump"}"#);
        }
        assert!(prefetched(2));
        let next = send(core, r#"{"cmd":"key","key":"n"}"#)["state"].clone();
        assert_eq!((next["article"]["id"].as_i64(), next["article"]["serial"].as_u64()), (Some(2), Some(2)));
        assert!(next["article"]["blocks"].is_array());
        assert!(!prefetched(2), "used up");

        // Search: the box, then the results, with the Search badge.
        assert_eq!(send(core, r#"{"cmd":"key","key":"/"}"#)["state"]["overlay"], "search");
        send(core, r#"{"cmd":"search","query":"watch"}"#);
        for _ in 0..200 {
            state = send(core, r#"{"cmd":"pump"}"#)["state"].clone();
            if state["screen"] == "article" && state["searching"] == true {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert_eq!((state["searching"].as_bool(), state["search_query"].as_str()), (Some(true), Some("watch")));
        assert_eq!(state["picker"]["newest_first"], false);
        // SAFETY: the core is not used after this.
        drop(unsafe { Box::from_raw(core) });
    }
}
