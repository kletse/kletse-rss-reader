//! The C interface of the native macOS app (macos/), which does for SwiftUI what bridge.rs does for Qt. It owns
//! the `App` and runs the effects it asks for. Swift sends each command as JSON (`kr_send`) and gets back what
//! the window shows, as JSON. Server calls run on a tokio runtime; when one answers, `notify` asks Swift to send
//! `pump` from the main thread, which applies the answer there. So the app only ever changes on the main thread.
//!
//! The article body goes out only when it was converted again (a new article, new text, a new palette), with
//! `serial` changing only for a new article or new text, which is what scrolls back to the top.

#![allow(unsafe_code)] // the C interface: pointers Swift hands over

use crate::app::{App, Effect, KeyPress, Outcome, PickerList, SettingsValues, ToastKind};
use crate::backend::Backend;
use crate::content::{convert, native};
use crate::keys;
use crate::paging::Mode;
use crate::prefs;
use crate::theme::{self, Palette};
use serde::{Deserialize, Serialize};
use std::ffi::{CStr, CString, c_char, c_void};
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock, mpsc};

static RUNTIME: LazyLock<tokio::runtime::Runtime> = LazyLock::new(|| {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .thread_name("kletse-rss-io")
        .enable_all()
        .build()
        .expect("a tokio runtime starts on any supported platform")
});

/// `KLETSE_RSS_DEBUG=1`: say on stderr what an article had that the window can't show (inline SVG, MathML).
static DEBUG: LazyLock<bool> = LazyLock::new(|| std::env::var("KLETSE_RSS_DEBUG").is_ok_and(|v| v == "1"));

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

pub struct Core {
    app: App,
    backend: Arc<Backend>,
    palette: Palette,
    answers: mpsc::Sender<Outcome>,
    waiting: mpsc::Receiver<Outcome>,
    waker: Arc<Waker>,
    rendered: Option<Rendered>,
    article_serial: u64,
    /// Whether the settings form on screen was filled in from the loaded values yet.
    settings_shown: bool,
    settings_serial: u64,
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
        accent: String,
    },
    /// The system appearance or accent colour changed.
    Theme {
        dark: bool,
        accent: String,
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
        miniflux_url: String,
        miniflux_api_key: String,
        readeck_url: String,
        readeck_token: String,
    },
    CancelSettings,
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
    /// "scroll_up", "scroll_down", "quit".
    events: Vec<&'static str>,
    state: State,
}

#[derive(Serialize)]
struct State {
    screen: &'static str,
    overlay: &'static str,
    end_title: &'static str,
    window_title: String,
    selection_label: String,
    history: bool,
    position: String,
    readeck: bool,
    article: Option<Article>,
    picker: Picker,
    help: Vec<(String, String)>,
    confirm_text: String,
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
}

#[derive(Serialize)]
struct PickerRow {
    label: String,
    count: i64,
    depth: u8,
}

#[derive(Serialize)]
struct ToastState {
    message: String,
    error: bool,
    serial: u64,
}

#[derive(Serialize)]
struct Settings {
    problem: String,
    error: String,
    checking: bool,
    /// Bumped when `values` is sent: the form fills in from them once, then keeps what is typed.
    serial: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    values: Option<Values>,
}

#[derive(Serialize)]
struct Values {
    miniflux_url: String,
    miniflux_api_key: String,
    readeck_url: String,
    readeck_token: String,
}

fn hash(value: &impl Hash) -> u64 {
    let mut h = std::hash::DefaultHasher::new();
    value.hash(&mut h);
    h.finish()
}

impl Core {
    fn new(waker: Waker) -> Self {
        let demo = std::env::var("KLETSE_RSS_DEMO").is_ok_and(|v| v == "1");
        let (answers, waiting) = mpsc::channel();
        Self {
            app: App::new(prefs::load(), true),
            backend: Arc::new(Backend::new(demo)),
            palette: Palette::default(),
            answers,
            waiting,
            waker: Arc::new(waker),
            rendered: None,
            article_serial: 0,
            settings_shown: false,
            settings_serial: 0,
        }
    }

    fn set_theme(&mut self, dark: bool, accent: &str) {
        self.palette = Palette::from(&theme::macos(dark, accent));
    }

    fn handle(&mut self, command: Command) -> Reply {
        let mut events = Vec::new();
        let mut used = true;
        let fx = match command {
            Command::Start { dark, accent } => {
                self.set_theme(dark, &accent);
                // Loading the grammars takes a moment; better now than on the first code block.
                std::thread::spawn(crate::content::highlight::warm_up);
                let status = self.backend.status();
                self.app.start(status)
            }
            Command::Theme { dark, accent } => {
                self.set_theme(dark, &accent);
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
            Command::SubmitSettings { miniflux_url, miniflux_api_key, readeck_url, readeck_token } => {
                self.app.submit_settings(SettingsValues { miniflux_url, miniflux_api_key, readeck_url, readeck_token })
            }
            Command::CancelSettings => self.app.cancel_settings(),
            Command::DismissToast { serial } => {
                self.app.dismiss_toast(serial);
                Vec::new()
            }
            Command::Pump => {
                self.waker.pending.store(false, Ordering::Release);
                let mut fx = Vec::new();
                while let Ok(outcome) = self.waiting.try_recv() {
                    fx.extend(self.app.apply(outcome));
                    // What an answer asks for runs before the next answer is applied, as it would in Qt.
                    self.run(std::mem::take(&mut fx), &mut events);
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
                Effect::SavePrefs(p) => prefs::save(p),
                Effect::ScrollPage { up } => events.push(if up { "scroll_up" } else { "scroll_down" }),
                Effect::Quit => events.push("quit"),
                effect => {
                    let backend = Arc::clone(&self.backend);
                    let answers = self.answers.clone();
                    let waker = Arc::clone(&self.waker);
                    RUNTIME.spawn(async move {
                        if let Some(outcome) = backend.run(effect).await
                            && answers.send(outcome).is_ok()
                        {
                            waker.wake();
                        }
                    });
                }
            }
        }
    }

    fn state(&mut self) -> State {
        let article = self.article();
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
            end_title: app.end_title(),
            window_title: app.window_title(),
            selection_label: app.selection_label().to_string(),
            history: app.history_badge(),
            position: app.position(),
            readeck: app.readeck(),
            article,
            picker: Picker {
                state: picker_state,
                error: picker_error,
                rows,
                cursor: app.picker_cursor(),
                history: app.picker_mode() == Mode::History,
            },
            help: app.help_lines(),
            confirm_text: app.confirm_text().to_string(),
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
            let rendered = convert::convert(&entry.content, &entry.url, &self.palette);
            if *DEBUG {
                let lost = rendered.stats.summary();
                if !lost.is_empty() {
                    eprintln!("kletse-rss: entry {}: {lost}", entry.id);
                }
            }
            self.rendered = Some(key);
            if previous.is_none_or(|p| p.id != key.id || p.content != key.content) {
                // A new article, or new text for it (`f`): it starts from the top.
                self.article_serial += 1;
            }
            native::native(&rendered.blocks)
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
            blocks,
        })
    }

    fn settings(&mut self) -> Option<Settings> {
        let Some(form) = self.app.settings() else {
            self.settings_shown = false;
            return None;
        };
        // The form starts from the loaded values once; after that the window keeps what is typed.
        let values = (!self.settings_shown).then(|| Values {
            miniflux_url: form.values.miniflux_url.clone(),
            miniflux_api_key: form.values.miniflux_api_key.clone(),
            readeck_url: form.values.readeck_url.clone(),
            readeck_token: form.values.readeck_token.clone(),
        });
        let settings = Settings {
            problem: form.problem.clone().unwrap_or_default(),
            error: form.error.clone(),
            checking: form.checking,
            serial: self.settings_serial + u64::from(values.is_some()),
            values,
        };
        self.settings_shown = true;
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

    /// The demo, through the C interface: it starts, the answers come back through `pump`, and the article's
    /// blocks are sent once.
    #[test]
    fn the_demo_opens_through_the_c_interface() {
        // SAFETY: setting the variable before anything reads it; no other test in this module reads it.
        unsafe { std::env::set_var("KLETSE_RSS_DEMO", "1") };
        let core = kr_new(count, std::ptr::null_mut());
        assert!(send(core, r#"{"cmd":"nonsense"}"#)["error"].is_string());
        let started = send(core, r##"{"cmd":"start","dark":true,"accent":"#0a84ff"}"##);
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
        // SAFETY: the core is not used after this.
        drop(unsafe { Box::from_raw(core) });
    }
}
