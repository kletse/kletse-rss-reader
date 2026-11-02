//! The one object QML sees. It owns the `App` (app.rs), runs the effects it asks for (server calls on a tokio
//! runtime, whose answers come back on the Qt thread), and after every change copies what the window shows into
//! properties. QML only draws those and forwards keys and clicks; nothing it does can change the state directly.

#![allow(unsafe_code)] // the generated FFI; nothing hand-written here is unsafe
#![allow(clippy::too_many_arguments)] // `key` takes one flag per modifier, as QML has them

use crate::app::{App, Effect, KeyPress, Overlay, PickerList, Screen, SettingsValues, ToastKind};
use crate::backend::Backend;
use crate::content::convert;
use crate::keys::Action;
use crate::paging::Mode;
use crate::theme::{self, Palette};
use crate::{prefs, theme::Theme};
use cxx_qt::{CxxQtType, Threading};
use cxx_qt_lib::QString;
use std::hash::{Hash, Hasher};
use std::pin::Pin;
use std::sync::{Arc, LazyLock};

#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
    }

    #[auto_cxx_name]
    extern "RustQt" {
        #[qobject]
        #[qml_element]
        // What fills the window: "settings", "loading", "article", "end" or "empty".
        #[qproperty(QString, screen)]
        #[qproperty(QString, end_title)]
        #[qproperty(QString, window_title)]
        // "none", "picker", "help", "confirm", "settings" or "onboarding".
        #[qproperty(QString, overlay)]
        // Top bar
        #[qproperty(QString, selection_label)]
        #[qproperty(bool, history)]
        #[qproperty(QString, position)]
        #[qproperty(bool, readeck)]
        // The article. The serial changes with the article or its text, not with a re-render for the theme.
        #[qproperty(i32, article_serial)]
        #[qproperty(QString, title)]
        #[qproperty(QString, url)]
        #[qproperty(QString, feed_title)]
        #[qproperty(QString, author)]
        #[qproperty(QString, published_at)]
        #[qproperty(i32, reading_time)]
        #[qproperty(QString, status)]
        // The body as JSON blocks (see content/convert.rs).
        #[qproperty(QString, blocks)]
        // Picker
        #[qproperty(QString, picker_state)]
        #[qproperty(QString, picker_error)]
        // JSON rows: `{label, count, depth}`.
        #[qproperty(QString, picker_rows)]
        #[qproperty(i32, picker_cursor)]
        #[qproperty(bool, picker_history)]
        // Help, confirm, toast
        // JSON `[[keys, action], …]`.
        #[qproperty(QString, help_lines)]
        #[qproperty(QString, confirm_text)]
        #[qproperty(QString, toast_message)]
        #[qproperty(bool, toast_error)]
        #[qproperty(i32, toast_serial)]
        // Settings form
        #[qproperty(bool, settings_ready)]
        #[qproperty(i32, settings_serial)]
        #[qproperty(QString, settings_problem)]
        #[qproperty(QString, settings_error)]
        #[qproperty(bool, settings_checking)]
        #[qproperty(QString, miniflux_url)]
        #[qproperty(QString, miniflux_api_key)]
        #[qproperty(QString, readeck_url)]
        #[qproperty(QString, readeck_token)]
        // Prefs
        #[qproperty(i32, article_size)]
        #[qproperty(i32, width_factor)]
        // Theme
        #[qproperty(QString, bg)]
        #[qproperty(QString, bg_light)]
        #[qproperty(QString, bg_dark)]
        #[qproperty(QString, fg)]
        #[qproperty(QString, fg_dim)]
        #[qproperty(QString, fg_bright)]
        #[qproperty(QString, accent)]
        #[qproperty(QString, muted)]
        #[qproperty(QString, selection)]
        #[qproperty(QString, red)]
        #[qproperty(QString, yellow)]
        #[qproperty(QString, ui_font)]
        #[qproperty(i32, rounding)]
        #[qproperty(bool, dark)]
        #[qproperty(bool, mac)]
        // `KLETSE_RSS_SNAPSHOT`: render the first article to this PNG and quit (screenshots, the CI smoke test).
        #[qproperty(QString, snapshot_path)]
        // Keys to press first, space-separated ("Enter n ?"); by default Enter opens the first row of the picker.
        #[qproperty(QString, snapshot_keys)]
        type Model = super::ModelRust;

        #[qinvokable]
        fn start(self: Pin<&mut Model>);
        /// A key press, named as the web names keys. Returns whether it was used.
        #[qinvokable]
        fn key(
            self: Pin<&mut Model>,
            key: &QString,
            shift: bool,
            ctrl: bool,
            alt: bool,
            meta: bool,
            typing: bool,
            at_bottom: bool,
        ) -> bool;
        /// A button: "next", "prev", "picker", "readeck", "mode", "settings", "help".
        #[qinvokable]
        fn press(self: Pin<&mut Model>, button: &QString);
        #[qinvokable]
        fn choose_row(self: Pin<&mut Model>, row: i32);
        #[qinvokable]
        fn picker_backdrop(self: Pin<&mut Model>);
        #[qinvokable]
        fn close_help(self: Pin<&mut Model>);
        #[qinvokable]
        fn open_link(self: Pin<&mut Model>, url: &QString);
        #[qinvokable]
        fn submit_settings(
            self: Pin<&mut Model>,
            miniflux_url: &QString,
            miniflux_api_key: &QString,
            readeck_url: &QString,
            readeck_token: &QString,
        );
        #[qinvokable]
        fn cancel_settings(self: Pin<&mut Model>);
        #[qinvokable]
        fn dismiss_toast(self: Pin<&mut Model>, serial: i32);
        /// The window regained focus or the system appearance changed (macOS follows the theme this way).
        #[qinvokable]
        fn recheck_theme(self: Pin<&mut Model>);

        #[qsignal]
        fn scroll_page(self: Pin<&mut Model>, up: bool);
        #[qsignal]
        fn quit_requested(self: Pin<&mut Model>);
    }

    impl cxx_qt::Threading for Model {}
}

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

/// What the article on screen was rendered from, to render it again only when that changes.
#[derive(PartialEq, Eq, Clone, Copy)]
struct Rendered {
    id: i64,
    content: u64,
    palette: u64,
}

pub struct ModelRust {
    screen: QString,
    end_title: QString,
    window_title: QString,
    overlay: QString,
    selection_label: QString,
    history: bool,
    position: QString,
    readeck: bool,
    article_serial: i32,
    title: QString,
    url: QString,
    feed_title: QString,
    author: QString,
    published_at: QString,
    reading_time: i32,
    status: QString,
    blocks: QString,
    picker_state: QString,
    picker_error: QString,
    picker_rows: QString,
    picker_cursor: i32,
    picker_history: bool,
    help_lines: QString,
    confirm_text: QString,
    toast_message: QString,
    toast_error: bool,
    toast_serial: i32,
    settings_ready: bool,
    settings_serial: i32,
    settings_problem: QString,
    settings_error: QString,
    settings_checking: bool,
    miniflux_url: QString,
    miniflux_api_key: QString,
    readeck_url: QString,
    readeck_token: QString,
    article_size: i32,
    width_factor: i32,
    bg: QString,
    bg_light: QString,
    bg_dark: QString,
    fg: QString,
    fg_dim: QString,
    fg_bright: QString,
    accent: QString,
    muted: QString,
    selection: QString,
    red: QString,
    yellow: QString,
    ui_font: QString,
    rounding: i32,
    dark: bool,
    mac: bool,
    snapshot_path: QString,
    snapshot_keys: QString,

    app: App,
    backend: Arc<Backend>,
    palette: Palette,
    watcher: Option<theme::Watcher>,
    rendered: Option<Rendered>,
    /// Whether the settings form on screen was filled in from the loaded values yet.
    settings_shown: bool,
}

impl std::fmt::Debug for ModelRust {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ModelRust { .. }")
    }
}

impl Default for ModelRust {
    fn default() -> Self {
        let mac = cfg!(target_os = "macos");
        let demo = std::env::var("KLETSE_RSS_DEMO").is_ok_and(|v| v == "1");
        Self {
            screen: QString::from("empty"),
            end_title: QString::default(),
            window_title: QString::from(crate::app::APP_NAME),
            overlay: QString::from("none"),
            selection_label: QString::default(),
            history: false,
            position: QString::default(),
            readeck: false,
            article_serial: 0,
            title: QString::default(),
            url: QString::default(),
            feed_title: QString::default(),
            author: QString::default(),
            published_at: QString::default(),
            reading_time: 0,
            status: QString::default(),
            blocks: QString::from("[]"),
            picker_state: QString::from("loading"),
            picker_error: QString::default(),
            picker_rows: QString::from("[]"),
            picker_cursor: 0,
            picker_history: false,
            help_lines: QString::from("[]"),
            confirm_text: QString::default(),
            toast_message: QString::default(),
            toast_error: false,
            toast_serial: 0,
            settings_ready: false,
            settings_serial: 0,
            settings_problem: QString::default(),
            settings_error: QString::default(),
            settings_checking: false,
            miniflux_url: QString::default(),
            miniflux_api_key: QString::default(),
            readeck_url: QString::default(),
            readeck_token: QString::default(),
            article_size: 18,
            width_factor: 40,
            bg: QString::default(),
            bg_light: QString::default(),
            bg_dark: QString::default(),
            fg: QString::default(),
            fg_dim: QString::default(),
            fg_bright: QString::default(),
            accent: QString::default(),
            muted: QString::default(),
            selection: QString::default(),
            red: QString::default(),
            yellow: QString::default(),
            ui_font: QString::default(),
            rounding: 0,
            dark: true,
            mac,
            snapshot_path: QString::from(&std::env::var("KLETSE_RSS_SNAPSHOT").unwrap_or_default()),
            snapshot_keys: QString::from(&std::env::var("KLETSE_RSS_SNAPSHOT_KEYS").unwrap_or_else(|_| "Enter".into())),
            app: App::new(prefs::load(), mac),
            backend: Arc::new(Backend::new(demo)),
            palette: Palette::default(),
            watcher: None,
            rendered: None,
            settings_shown: false,
        }
    }
}

fn qs(s: &str) -> QString {
    QString::from(s)
}

fn to_i32(n: impl TryInto<i32>) -> i32 {
    n.try_into().unwrap_or(i32::MAX)
}

fn hash(value: &impl Hash) -> u64 {
    let mut h = std::hash::DefaultHasher::new();
    value.hash(&mut h);
    h.finish()
}

fn palette_hash(p: &Palette) -> u64 {
    hash(&format!("{p:?}"))
}

fn button_action(button: &str) -> Option<Action> {
    Some(match button {
        "next" => Action::Next { mark_read: true },
        "prev" => Action::Prev,
        "picker" => Action::OpenPicker,
        "readeck" => Action::SaveToReadeck,
        "mode" => Action::ToggleMode,
        "settings" => Action::Settings,
        "help" => Action::Help,
        _ => return None,
    })
}

impl qobject::Model {
    fn state(self: Pin<&mut Self>) -> &mut ModelRust {
        self.rust_mut().get_mut()
    }

    pub fn start(mut self: Pin<&mut Self>) {
        let help = serde_json::to_string(&self.as_mut().state().app.help_lines()).unwrap_or_else(|_| "[]".into());
        self.as_mut().set_help_lines(qs(&help));
        self.as_mut().apply_theme(theme::load());

        let qt = self.qt_thread();
        let on_change: theme::OnChange = Arc::new(move |theme: Theme| {
            let _ = qt.queue(move |model| model.apply_theme(theme));
        });
        self.as_mut().state().watcher = Some(theme::watch(on_change));

        // Loading the grammars takes a moment; better now than on the first code block.
        std::thread::spawn(crate::content::highlight::warm_up);

        let status = self.as_mut().state().backend.status();
        let fx = self.as_mut().state().app.start(status);
        self.run(fx);
    }

    fn apply_theme(mut self: Pin<&mut Self>, theme: Theme) {
        let p = Palette::from(&theme);
        self.as_mut().set_bg(qs(&p.bg));
        self.as_mut().set_bg_light(qs(&p.bg_light));
        self.as_mut().set_bg_dark(qs(&p.bg_dark));
        self.as_mut().set_fg(qs(&p.fg));
        self.as_mut().set_fg_dim(qs(&p.fg_dim));
        self.as_mut().set_fg_bright(qs(&p.fg_bright));
        self.as_mut().set_accent(qs(&p.accent));
        self.as_mut().set_muted(qs(&p.muted));
        self.as_mut().set_selection(qs(&p.selection));
        self.as_mut().set_red(qs(&p.bright_red));
        self.as_mut().set_yellow(qs(&p.yellow));
        self.as_mut().set_ui_font(qs(&p.ui_font));
        self.as_mut().set_rounding(to_i32(p.rounding));
        self.as_mut().set_dark(p.dark);
        self.as_mut().state().palette = p;
        // Code colours and link colours are baked into the rendered article.
        self.sync();
    }

    pub fn recheck_theme(mut self: Pin<&mut Self>) {
        if let Some(watcher) = &self.as_mut().state().watcher {
            watcher.recheck();
        }
    }

    pub fn key(
        mut self: Pin<&mut Self>,
        key: &QString,
        shift: bool,
        ctrl: bool,
        alt: bool,
        meta: bool,
        typing: bool,
        at_bottom: bool,
    ) -> bool {
        let press = KeyPress { key: key.to_string(), shift, ctrl, alt, meta, typing, at_bottom };
        let (used, fx) = self.as_mut().state().app.key(&press);
        self.run(fx);
        used
    }

    pub fn press(mut self: Pin<&mut Self>, button: &QString) {
        if let Some(action) = button_action(&button.to_string()) {
            let fx = self.as_mut().state().app.action(action);
            self.run(fx);
        }
    }

    pub fn choose_row(mut self: Pin<&mut Self>, row: i32) {
        let fx = self.as_mut().state().app.choose_row(usize::try_from(row).unwrap_or(usize::MAX));
        self.run(fx);
    }

    pub fn picker_backdrop(mut self: Pin<&mut Self>) {
        self.as_mut().state().app.picker_backdrop();
        self.sync();
    }

    pub fn close_help(mut self: Pin<&mut Self>) {
        self.as_mut().state().app.close_help_overlay();
        self.sync();
    }

    pub fn open_link(mut self: Pin<&mut Self>, url: &QString) {
        let fx = self.as_mut().state().app.open_link(&url.to_string());
        self.run(fx);
    }

    pub fn submit_settings(
        mut self: Pin<&mut Self>,
        miniflux_url: &QString,
        miniflux_api_key: &QString,
        readeck_url: &QString,
        readeck_token: &QString,
    ) {
        let values = SettingsValues {
            miniflux_url: miniflux_url.to_string(),
            miniflux_api_key: miniflux_api_key.to_string(),
            readeck_url: readeck_url.to_string(),
            readeck_token: readeck_token.to_string(),
        };
        let fx = self.as_mut().state().app.submit_settings(values);
        self.run(fx);
    }

    pub fn cancel_settings(mut self: Pin<&mut Self>) {
        let fx = self.as_mut().state().app.cancel_settings();
        self.run(fx);
    }

    pub fn dismiss_toast(mut self: Pin<&mut Self>, serial: i32) {
        self.as_mut().state().app.dismiss_toast(u64::try_from(serial).unwrap_or(0));
        self.sync();
    }

    /// Runs what the app asked for, then shows the new state.
    fn run(mut self: Pin<&mut Self>, fx: Vec<Effect>) {
        for effect in fx {
            match effect {
                Effect::SavePrefs(p) => prefs::save(p),
                Effect::ScrollPage { up } => self.as_mut().scroll_page(up),
                Effect::Quit => self.as_mut().quit_requested(),
                effect => {
                    let backend = Arc::clone(&self.as_mut().state().backend);
                    let qt = self.qt_thread();
                    RUNTIME.spawn(async move {
                        if let Some(outcome) = backend.run(effect).await {
                            let _ = qt.queue(move |mut model| {
                                let fx = model.as_mut().state().app.apply(outcome);
                                model.run(fx);
                            });
                        }
                    });
                }
            }
        }
        self.sync();
    }

    /// Copies what the window shows from the app into the properties. Setters only notify on a real change.
    fn sync(mut self: Pin<&mut Self>) {
        self.as_mut().sync_chrome();
        self.as_mut().sync_article();
        self.as_mut().sync_picker();
        self.sync_settings();
    }

    fn sync_chrome(mut self: Pin<&mut Self>) {
        let s = self.as_mut().state();
        let app = &s.app;
        let screen = match app.screen() {
            Screen::Settings => "settings",
            Screen::Loading => "loading",
            Screen::Article => "article",
            Screen::End => "end",
            Screen::Empty => "empty",
        };
        let overlay = match app.overlay() {
            Overlay::None => "none",
            Overlay::Picker => "picker",
            Overlay::Help => "help",
            Overlay::Confirm => "confirm",
            Overlay::Settings => "settings",
            Overlay::Onboarding => "onboarding",
        };
        let end_title = if app.mode() == Mode::History { "End of history" } else { "All caught up" };
        let history = app.mode() == Mode::History && !app.selection_label().is_empty();
        let (window_title, label, position, readeck) =
            (app.window_title(), app.selection_label().to_string(), app.position(), app.readeck());
        let confirm = app.confirm_text().to_string();
        let toast = app.toast().cloned();
        let prefs = app.prefs();

        self.as_mut().set_screen(qs(screen));
        self.as_mut().set_overlay(qs(overlay));
        self.as_mut().set_end_title(qs(end_title));
        self.as_mut().set_window_title(qs(&window_title));
        self.as_mut().set_selection_label(qs(&label));
        self.as_mut().set_history(history);
        self.as_mut().set_position(qs(&position));
        self.as_mut().set_readeck(readeck);
        self.as_mut().set_confirm_text(qs(&confirm));
        match toast {
            Some(t) => {
                self.as_mut().set_toast_message(qs(&t.message));
                self.as_mut().set_toast_error(t.kind == ToastKind::Error);
                self.as_mut().set_toast_serial(to_i32(t.serial));
            }
            None => self.as_mut().set_toast_message(QString::default()),
        }
        self.as_mut().set_article_size(prefs.font_size);
        self.set_width_factor(prefs.width_factor);
    }

    fn sync_article(mut self: Pin<&mut Self>) {
        let s = self.as_mut().state();
        let Some(entry) = s.app.current().cloned() else { return };
        let palette = palette_hash(&s.palette);
        let key = Rendered { id: entry.id, content: hash(&entry.content), palette };
        let previous = s.rendered;
        if previous != Some(key) {
            let rendered = convert::convert(&entry.content, &entry.url, &s.palette);
            if *DEBUG {
                let lost = rendered.stats.summary();
                if !lost.is_empty() {
                    eprintln!("kletse-rss: entry {}: {lost}", entry.id);
                }
            }
            let json = serde_json::to_string(&rendered.blocks).unwrap_or_else(|_| "[]".into());
            s.rendered = Some(key);
            let new_article = previous.is_none_or(|p| p.id != key.id || p.content != key.content);
            self.as_mut().set_blocks(qs(&json));
            if new_article {
                // A new article, or new text for it (`f`): it starts from the top.
                let serial = self.as_ref().article_serial().wrapping_add(1);
                self.as_mut().set_article_serial(serial);
            }
        }
        self.as_mut().set_title(qs(crate::app::display_title(&entry.title)));
        self.as_mut().set_url(qs(&entry.url));
        self.as_mut().set_feed_title(qs(&entry.feed.title));
        self.as_mut().set_author(qs(&entry.author));
        self.as_mut().set_published_at(qs(&entry.published_at));
        self.as_mut().set_reading_time(to_i32(entry.reading_time));
        self.set_status(qs(crate::app::status_name(entry.status)));
    }

    fn sync_picker(mut self: Pin<&mut Self>) {
        let s = self.as_mut().state();
        let (state, error) = match s.app.picker_list() {
            PickerList::Loading => ("loading", String::new()),
            PickerList::Error(message) => ("error", message.clone()),
            PickerList::Ready(_) => ("ready", String::new()),
        };
        let rows: Vec<serde_json::Value> = s
            .app
            .picker_items()
            .iter()
            .map(|i| serde_json::json!({ "label": i.label, "count": i.count, "depth": i.depth }))
            .collect();
        let rows = serde_json::to_string(&rows).unwrap_or_else(|_| "[]".into());
        let (cursor, history) = (to_i32(s.app.picker_cursor()), s.app.picker_mode() == Mode::History);
        self.as_mut().set_picker_state(qs(state));
        self.as_mut().set_picker_error(qs(&error));
        self.as_mut().set_picker_rows(qs(&rows));
        self.as_mut().set_picker_cursor(cursor);
        self.set_picker_history(history);
    }

    fn sync_settings(mut self: Pin<&mut Self>) {
        let s = self.as_mut().state();
        let Some(form) = s.app.settings() else {
            s.settings_shown = false;
            self.set_settings_ready(false);
            return;
        };
        let (problem, error, checking) = (form.problem.clone().unwrap_or_default(), form.error.clone(), form.checking);
        // The form starts from the loaded values once; after that the window keeps what is typed.
        let fill = (!s.settings_shown).then(|| form.values.clone());
        s.settings_shown = true;
        if let Some(values) = fill {
            self.as_mut().set_miniflux_url(qs(&values.miniflux_url));
            self.as_mut().set_miniflux_api_key(qs(&values.miniflux_api_key));
            self.as_mut().set_readeck_url(qs(&values.readeck_url));
            self.as_mut().set_readeck_token(qs(&values.readeck_token));
            let serial = self.as_ref().settings_serial().wrapping_add(1);
            self.as_mut().set_settings_serial(serial);
        }
        self.as_mut().set_settings_problem(qs(&problem));
        self.as_mut().set_settings_error(qs(&error));
        self.as_mut().set_settings_checking(checking);
        self.set_settings_ready(true);
    }
}
