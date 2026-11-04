//! The one object QML sees. It owns the `App` (app.rs), runs the effects it asks for (server calls on a tokio
//! runtime, whose answers come back on the Qt thread), and after every change copies what the window shows into
//! properties. QML only draws those and forwards keys and clicks; nothing it does can change the state directly.

#![allow(unsafe_code)] // the generated FFI; nothing hand-written here is unsafe
#![allow(clippy::too_many_arguments)] // `key` takes one flag per modifier, as QML has them

use crate::app::{App, Effect, KeyPress, PickerList, SettingsValues, ToastKind};
use crate::backend::Backend;
use crate::content::convert;
use crate::keys;
use crate::paging::Mode;
use crate::theme::{self, Palette};
use crate::{
    prefs::{self, Colours},
    theme::Theme,
};
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
        // Where j goes from the end of the list.
        #[qproperty(QString, end_hint)]
        #[qproperty(QString, window_title)]
        // "none", "picker", "help", "confirm", "settings", "onboarding", "search" or "image".
        #[qproperty(QString, overlay)]
        // The picker is on screen: open, or behind the question Shift+A on a row asks.
        #[qproperty(bool, picker_shown)]
        // Top bar
        #[qproperty(QString, selection_label)]
        #[qproperty(bool, history)]
        #[qproperty(bool, searching)]
        // Reading the starred items: the top bar's Starred badge.
        #[qproperty(bool, starred_list)]
        #[qproperty(QString, position)]
        #[qproperty(bool, readeck)]
        // The server refreshes feeds (r) and fetches whole articles (f): Miniflux, not a Google Reader server.
        #[qproperty(bool, fetches)]
        // The article. The serial changes with the article or its text, not with a re-render for the theme.
        #[qproperty(i32, article_serial)]
        #[qproperty(QString, title)]
        #[qproperty(QString, url)]
        #[qproperty(QString, feed_title)]
        #[qproperty(QString, author)]
        #[qproperty(QString, published_at)]
        #[qproperty(i32, reading_time)]
        #[qproperty(QString, status)]
        // The article on screen is starred (`*`).
        #[qproperty(bool, starred)]
        // The body as JSON blocks (see content/convert.rs).
        #[qproperty(QString, blocks)]
        // The next article's images as a JSON list of URLs, loading while this one is read.
        #[qproperty(QString, prefetch_images)]
        // The image viewer: the image on screen, its alt text, and "2 / 5" when there are more.
        #[qproperty(QString, viewer_src)]
        #[qproperty(QString, viewer_alt)]
        #[qproperty(QString, viewer_position)]
        // The search box starts from the last search.
        #[qproperty(QString, search_query)]
        // Picker
        #[qproperty(QString, picker_state)]
        #[qproperty(QString, picker_error)]
        // JSON rows: `{label, count, depth}`.
        #[qproperty(QString, picker_rows)]
        #[qproperty(i32, picker_cursor)]
        #[qproperty(bool, picker_history)]
        // Unread items come newest first.
        #[qproperty(bool, newest_first)]
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
        // "miniflux" or "greader".
        #[qproperty(QString, server)]
        #[qproperty(QString, miniflux_url)]
        #[qproperty(QString, miniflux_api_key)]
        #[qproperty(QString, greader_url)]
        #[qproperty(QString, greader_username)]
        #[qproperty(QString, greader_password)]
        #[qproperty(QString, readeck_url)]
        #[qproperty(QString, readeck_token)]
        // Prefs
        #[qproperty(i32, article_size)]
        #[qproperty(i32, width_factor)]
        // "system", "onward-dark" or "onward-light".
        #[qproperty(QString, colours)]
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
        // `ONWARD_SNAPSHOT`: render the first article to this PNG and quit (screenshots, the CI smoke test).
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
        /// A button: "next", "prev", "picker", "readeck", "unread", "history", "refresh", "settings", "help",
        /// "search", "order".
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
            server: &QString,
            miniflux_url: &QString,
            miniflux_api_key: &QString,
            greader_url: &QString,
            greader_username: &QString,
            greader_password: &QString,
            readeck_url: &QString,
            readeck_token: &QString,
        );
        #[qinvokable]
        fn cancel_settings(self: Pin<&mut Model>);
        /// The search box's Enter.
        #[qinvokable]
        fn search(self: Pin<&mut Model>, query: &QString);
        #[qinvokable]
        fn cancel_search(self: Pin<&mut Model>);
        /// A click on an image in the article.
        #[qinvokable]
        fn view_image(self: Pin<&mut Model>, src: &QString);
        /// A click in the image viewer.
        #[qinvokable]
        fn close_viewer(self: Pin<&mut Model>);
        #[qinvokable]
        fn dismiss_toast(self: Pin<&mut Model>, serial: i32);
        /// The window regained focus or the system appearance changed (macOS follows the theme this way).
        #[qinvokable]
        fn recheck_theme(self: Pin<&mut Model>);
        #[qinvokable]
        fn choose_colours(self: Pin<&mut Model>, name: &QString);

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
        .thread_name("onward-io")
        .enable_all()
        .build()
        .expect("a tokio runtime starts on any supported platform")
});

/// `ONWARD_DEBUG=1`: say on stderr what an article had that the window can't show (inline SVG, MathML).
static DEBUG: LazyLock<bool> = LazyLock::new(|| std::env::var("ONWARD_DEBUG").is_ok_and(|v| v == "1"));

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
    json: String,
    /// What the converter dropped, for `ONWARD_DEBUG=1`.
    lost: String,
    images: Vec<String>,
}

pub struct ModelRust {
    screen: QString,
    end_title: QString,
    end_hint: QString,
    window_title: QString,
    overlay: QString,
    picker_shown: bool,
    selection_label: QString,
    history: bool,
    searching: bool,
    starred_list: bool,
    position: QString,
    readeck: bool,
    fetches: bool,
    article_serial: i32,
    title: QString,
    url: QString,
    feed_title: QString,
    author: QString,
    published_at: QString,
    reading_time: i32,
    status: QString,
    starred: bool,
    blocks: QString,
    prefetch_images: QString,
    viewer_src: QString,
    viewer_alt: QString,
    viewer_position: QString,
    search_query: QString,
    picker_state: QString,
    picker_error: QString,
    picker_rows: QString,
    picker_cursor: i32,
    picker_history: bool,
    newest_first: bool,
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
    server: QString,
    miniflux_url: QString,
    miniflux_api_key: QString,
    greader_url: QString,
    greader_username: QString,
    greader_password: QString,
    readeck_url: QString,
    readeck_token: QString,
    article_size: i32,
    width_factor: i32,
    colours: QString,
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
    /// The desktop's theme as last heard, for switching back to it.
    system_theme: Theme,
    watcher: Option<theme::Watcher>,
    rendered: Option<Rendered>,
    prefetched: Option<Prefetched>,
    /// The article being converted ahead, so it isn't started twice.
    prefetching: Option<Rendered>,
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
        let demo = std::env::var("ONWARD_DEMO").is_ok_and(|v| v == "1");
        Self {
            screen: QString::from("empty"),
            end_title: QString::default(),
            end_hint: QString::default(),
            picker_shown: false,
            window_title: QString::from(crate::app::APP_NAME),
            overlay: QString::from("none"),
            selection_label: QString::default(),
            history: false,
            searching: false,
            starred_list: false,
            position: QString::default(),
            readeck: false,
            fetches: true,
            article_serial: 0,
            title: QString::default(),
            url: QString::default(),
            feed_title: QString::default(),
            author: QString::default(),
            published_at: QString::default(),
            reading_time: 0,
            status: QString::default(),
            starred: false,
            blocks: QString::from("[]"),
            prefetch_images: QString::from("[]"),
            viewer_src: QString::default(),
            viewer_alt: QString::default(),
            viewer_position: QString::default(),
            search_query: QString::default(),
            picker_state: QString::from("loading"),
            picker_error: QString::default(),
            picker_rows: QString::from("[]"),
            picker_cursor: 0,
            picker_history: false,
            newest_first: false,
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
            server: QString::default(),
            miniflux_url: QString::default(),
            miniflux_api_key: QString::default(),
            greader_url: QString::default(),
            greader_username: QString::default(),
            greader_password: QString::default(),
            readeck_url: QString::default(),
            readeck_token: QString::default(),
            article_size: 18,
            width_factor: 40,
            colours: QString::default(),
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
            snapshot_path: QString::from(&std::env::var("ONWARD_SNAPSHOT").unwrap_or_default()),
            snapshot_keys: QString::from(&std::env::var("ONWARD_SNAPSHOT_KEYS").unwrap_or_else(|_| "Enter".into())),
            app: App::new(prefs::load(), mac),
            backend: Arc::new(Backend::new(demo)),
            palette: Palette::default(),
            system_theme: Theme::default(),
            watcher: None,
            rendered: None,
            prefetched: None,
            prefetching: None,
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
        self.as_mut().state().system_theme = theme.clone();
        let colours = self.as_mut().state().app.prefs().colours;
        let p = Palette::from(&theme.in_colours(colours));
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

    pub fn choose_colours(mut self: Pin<&mut Self>, name: &QString) {
        let Some(colours) = Colours::from_name(&name.to_string()) else { return };
        let fx = self.as_mut().state().app.set_colours(colours);
        let theme = self.as_mut().state().system_theme.clone();
        self.as_mut().apply_theme(theme);
        self.run(fx);
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
        if let Some(action) = keys::button_action(&button.to_string()) {
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
        server: &QString,
        miniflux_url: &QString,
        miniflux_api_key: &QString,
        greader_url: &QString,
        greader_username: &QString,
        greader_password: &QString,
        readeck_url: &QString,
        readeck_token: &QString,
    ) {
        let values = SettingsValues {
            server: crate::config::Server::from_name(&server.to_string()),
            miniflux_url: miniflux_url.to_string(),
            miniflux_api_key: miniflux_api_key.to_string(),
            greader_url: greader_url.to_string(),
            greader_username: greader_username.to_string(),
            greader_password: greader_password.to_string(),
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

    pub fn search(mut self: Pin<&mut Self>, query: &QString) {
        let fx = self.as_mut().state().app.search(&query.to_string());
        self.run(fx);
    }

    pub fn cancel_search(mut self: Pin<&mut Self>) {
        self.as_mut().state().app.cancel_search();
        self.sync();
    }

    pub fn view_image(mut self: Pin<&mut Self>, src: &QString) {
        self.as_mut().state().app.view_image(&src.to_string());
        self.sync();
    }

    pub fn close_viewer(mut self: Pin<&mut Self>) {
        self.as_mut().state().app.close_viewer();
        self.sync();
    }

    pub fn dismiss_toast(mut self: Pin<&mut Self>, serial: i32) {
        self.as_mut().state().app.dismiss_toast(u64::try_from(serial).unwrap_or(0));
        self.sync();
    }

    /// Runs what the app asked for, then shows the new state.
    fn run(mut self: Pin<&mut Self>, fx: Vec<Effect>) {
        for effect in fx {
            match effect {
                Effect::SavePrefs(p) => {
                    if !self.as_mut().state().backend.demo() {
                        prefs::save(p);
                    }
                }
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
        let (screen, overlay) = (app.screen().name(), app.overlay().name());
        let (end_title, history, searching) = (app.end_title(), app.history_badge(), app.searching());
        let (starred_list, starred) = (app.starred_list(), app.starred());
        let search_query = app.search_query().to_string();
        let (end_hint, picker_shown) = (app.end_hint(), app.picker_shown());
        let viewer = app.viewer().map(|v| {
            let image = &v.images[v.index];
            let n = v.images.len();
            let position = if n > 1 { format!("{} / {n}", v.index + 1) } else { String::new() };
            (image.src.clone(), image.alt.clone(), position)
        });
        let (window_title, label, position, readeck) =
            (app.window_title(), app.selection_label().to_string(), app.position(), app.readeck());
        let fetches = app.fetches();
        let confirm = app.confirm_text().to_string();
        let toast = app.toast().cloned();
        let prefs = app.prefs();

        self.as_mut().set_screen(qs(screen));
        self.as_mut().set_overlay(qs(overlay));
        self.as_mut().set_end_title(qs(end_title));
        self.as_mut().set_end_hint(qs(&end_hint));
        self.as_mut().set_picker_shown(picker_shown);
        self.as_mut().set_window_title(qs(&window_title));
        self.as_mut().set_selection_label(qs(&label));
        self.as_mut().set_history(history);
        self.as_mut().set_searching(searching);
        self.as_mut().set_starred_list(starred_list);
        self.as_mut().set_starred(starred);
        self.as_mut().set_search_query(qs(&search_query));
        let (src, alt, position_in_viewer) = viewer.unwrap_or_default();
        self.as_mut().set_viewer_src(qs(&src));
        self.as_mut().set_viewer_alt(qs(&alt));
        self.as_mut().set_viewer_position(qs(&position_in_viewer));
        self.as_mut().set_position(qs(&position));
        self.as_mut().set_readeck(readeck);
        self.as_mut().set_fetches(fetches);
        self.as_mut().set_confirm_text(qs(&confirm));
        match toast {
            Some(t) => {
                self.as_mut().set_toast_message(qs(&t.message));
                self.as_mut().set_toast_error(t.kind == ToastKind::Error);
                self.as_mut().set_toast_serial(to_i32(t.serial));
            }
            None => self.as_mut().set_toast_message(QString::default()),
        }
        self.as_mut().set_colours(qs(prefs.colours.name()));
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
            let ready = s.prefetched.take_if(|p| p.key == key);
            let (json, lost) = match ready {
                Some(p) => (p.json, p.lost),
                None => {
                    let rendered = convert::convert(&entry.content, &entry.url, &s.palette);
                    let json = serde_json::to_string(&rendered.blocks).unwrap_or_else(|_| "[]".into());
                    (json, rendered.stats.summary())
                }
            };
            if *DEBUG && !lost.is_empty() {
                eprintln!("onward: entry {}: {lost}", entry.id);
            }
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
        self.as_mut().set_status(qs(crate::app::status_name(entry.status)));
        // After the article's own images exist: the next one's must not take their place in Qt's image cache.
        self.prefetch();
    }

    /// Converts the next article in the background, and then lists its images for the window to load, so it's
    /// ready by the time you get there.
    fn prefetch(mut self: Pin<&mut Self>) {
        let s = self.as_mut().state();
        let Some(next) = s.app.upcoming().cloned() else { return };
        let key = Rendered { id: next.id, content: hash(&next.content), palette: palette_hash(&s.palette) };
        if s.prefetched.as_ref().is_some_and(|p| p.key == key) || s.prefetching == Some(key) {
            return;
        }
        s.prefetching = Some(key);
        let palette = s.palette.clone();
        let qt = self.qt_thread();
        RUNTIME.spawn_blocking(move || {
            let rendered = convert::convert(&next.content, &next.url, &palette);
            let prefetched = Prefetched {
                key,
                json: serde_json::to_string(&rendered.blocks).unwrap_or_else(|_| "[]".into()),
                lost: rendered.stats.summary(),
                images: convert::image_refs(&rendered.blocks).into_iter().map(|i| i.src).collect(),
            };
            let _ = qt.queue(move |model| model.prefetched(prefetched));
        });
    }

    /// Keeps the converted article, unless you've moved on and another one is being converted meanwhile.
    fn prefetched(mut self: Pin<&mut Self>, prefetched: Prefetched) {
        let s = self.as_mut().state();
        if s.prefetching != Some(prefetched.key) {
            return;
        }
        s.prefetching = None;
        let images = serde_json::to_string(&prefetched.images).unwrap_or_else(|_| "[]".into());
        s.prefetched = Some(prefetched);
        self.set_prefetch_images(qs(&images));
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
        let newest_first = s.app.newest_first();
        self.as_mut().set_picker_state(qs(state));
        self.as_mut().set_picker_error(qs(&error));
        self.as_mut().set_picker_rows(qs(&rows));
        self.as_mut().set_picker_cursor(cursor);
        self.as_mut().set_newest_first(newest_first);
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
            self.as_mut().set_server(qs(values.server.name()));
            self.as_mut().set_miniflux_url(qs(&values.miniflux_url));
            self.as_mut().set_miniflux_api_key(qs(&values.miniflux_api_key));
            self.as_mut().set_greader_url(qs(&values.greader_url));
            self.as_mut().set_greader_username(qs(&values.greader_username));
            self.as_mut().set_greader_password(qs(&values.greader_password));
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
