//! What the terminal keeps of its own, around `App`: the article laid out for this width and how far it's
//! scrolled, the text fields, and the picture in the viewer. This is to the terminal what bridge.rs is to QML, and
//! like QML it only shows state and forwards input: what a key means is in app.rs. Only what a terminal needs and a
//! window doesn't is decided here, such as how to scroll without Shift+Space, which most terminals don't report.

use super::article::{self, Header, Row, Slice, Target};
use super::editor::Editor;
use super::images::{self, Answer, Job};
use crate::app::{
    App, ConnectView, Effect, KeyPress, Outcome, Overlay, Screen, SettingsValues, Status, Step, display_title,
    status_name,
};
use crate::config::Server;
use crate::content::{convert, native};
use crate::feeds::EntryStatus;
use crate::keys::Action;
use crate::prefs::{self, Prefs};
use crate::providers::{self, FieldKey};
use crate::theme::Palette;
use ratatui::crossterm::event::{
    KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use ratatui::layout::{Rect, Size};
use ratatui_image::picker::{Picker, ProtocolType};
use ratatui_image::protocol::Protocol;
use ratatui_image::sliced::SlicedProtocol;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// The top bar and the line under it.
pub const BAR: u16 = 2;
/// Rows the wheel moves per click.
const WHEEL: isize = 3;
/// Rows J and K move.
const LINE: isize = 2;
/// A column's worth of cells per unit of `Prefs::width_factor`, which counts font sizes: the same `[` and `]`.
const CELLS_PER_FONT_SIZE: u16 = 2;

/// The article on screen.
pub struct Shown {
    id: i64,
    content: u64,
    blocks: Vec<native::NativeBlock>,
    /// Its images.
    images: Vec<String>,
    /// What `rows` were laid out for: the width, the status and which pictures had arrived.
    laid: Option<(usize, EntryStatus, u64)>,
    pub rows: Vec<Row>,
}

/// A picture in an article, on its way or here.
pub enum Inline {
    Loading,
    /// It stays the line of text that stands for it.
    Failed,
    Here {
        bytes: images::Bytes,
        dimensions: images::Dimensions,
        /// Encoded for this many cells.
        sliced: Option<(Size, SlicedProtocol)>,
        /// The size being encoded, so it isn't asked for twice.
        fitting: Option<Size>,
    },
}

/// What has the keyboard on a form.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    /// The kind of server.
    Choice,
    Field(FieldKey),
    /// A row of Connect, or Settings' way there.
    Row(usize),
    /// A row of Readeck's browser way in, under its fields.
    Grant(usize),
}

/// The settings form, with an editor for every field the core has. The form starts from the saved values once, then
/// keeps what is typed, until the core has something to take back (`SettingsForm::refill`).
pub struct Form {
    pub server: Server,
    pub fields: HashMap<FieldKey, Editor>,
    pub focus: Focus,
    /// Readeck's fields are on screen: Settings has them, first run doesn't.
    pub readeck: bool,
    /// How many rows the core offers under them.
    grant_rows: usize,
    /// The refill these values came from.
    refill: u64,
    /// What Connect had on screen when the focus was last placed.
    connect: (&'static str, Vec<FieldKey>),
}

impl Form {
    /// The whole form as Settings and filling in by hand have it, top to bottom: the server choice, its fields, then
    /// Readeck's.
    fn whole(&self) -> impl Iterator<Item = Focus> + use<> {
        let own = providers::provider(self.server).fields.iter();
        let readeck = providers::READECK.iter().filter(|_| self.readeck).collect::<Vec<_>>();
        own.chain(readeck).map(|f| Focus::Field(f.key))
    }

    /// What's on screen, top to bottom.
    pub fn shown(&self, connect: Option<&ConnectView>) -> Vec<Focus> {
        let rows = |view: &ConnectView| (0..view.rows.len()).map(Focus::Row).collect::<Vec<_>>();
        match connect {
            Some(view) if view.stage == "address" => {
                let fields = view.fields.iter().map(|f| Focus::Field(f.key));
                std::iter::once(Focus::Field(FieldKey::Address)).chain(fields).chain(rows(view)).collect()
            }
            Some(view) => std::iter::once(Focus::Choice).chain(self.whole()).chain(rows(view)).collect(),
            None => {
                let grant = (0..self.grant_rows).map(Focus::Grant);
                [Focus::Choice, Focus::Row(0)].into_iter().chain(self.whole()).chain(grant).collect()
            }
        }
    }

    /// Tab and Shift+Tab, round what's on screen.
    fn step(&mut self, by: isize, connect: Option<&ConnectView>) {
        let shown = self.shown(connect);
        let at = shown.iter().position(|&f| f == self.focus).unwrap_or(0);
        let n = isize::try_from(shown.len()).unwrap_or(1);
        let next = (isize::try_from(at).unwrap_or(0) + by).rem_euclid(n);
        self.focus = shown[usize::try_from(next).unwrap_or(0)];
    }

    pub fn text(&self, key: FieldKey) -> &str {
        self.fields.get(&key).map_or("", Editor::text)
    }

    fn editor(&mut self) -> Option<&mut Editor> {
        match self.focus {
            Focus::Field(key) => self.fields.get_mut(&key),
            _ => None,
        }
    }

    fn values(&self) -> SettingsValues {
        let mut values = SettingsValues { server: self.server, ..SettingsValues::default() };
        for key in FieldKey::ALL {
            self.text(key).clone_into(values.field_mut(key));
        }
        values
    }

    fn load(&mut self, values: &SettingsValues) {
        self.server = values.server;
        self.fields = FieldKey::ALL.map(|key| (key, Editor::new(values.field(key)))).into_iter().collect();
    }

    /// The first empty field of those on screen, or the last, or the address box.
    fn place_focus(&mut self, view: &ConnectView) {
        let by_hand = view.stage == "by_hand";
        let fields = if by_hand { providers::provider(self.server).fields } else { view.fields.as_slice() };
        let empty = fields.iter().find(|f| self.text(f.key).is_empty());
        self.focus = empty.or(fields.last()).map_or(Focus::Field(FieldKey::Address), |f| Focus::Field(f.key));
    }
}

pub enum Picture {
    Loading,
    Failed(String),
    Ready { bytes: images::Bytes, protocol: Protocol, size: Size },
}

/// The picture in the viewer.
pub struct Shot {
    pub src: String,
    pub picture: Picture,
    /// The size being encoded, so it isn't asked for twice.
    fitting: Option<Size>,
}

pub struct Ui {
    pub app: App,
    demo: bool,
    palette: Palette,
    effects: Vec<Effect>,
    jobs: Vec<Job>,
    quit: bool,
    /// The screen needs painting from scratch: Ctrl+L, or a picture leaving it.
    repaint: bool,
    pub shown: Option<Shown>,
    pub scroll: usize,
    max_scroll: usize,
    /// Rows Space moves.
    page: usize,
    /// Where the article is drawn.
    pub column: Rect,
    pub help_scroll: usize,
    /// The first picker row in view, where they are and the panel around them, for clicks.
    pub picker_top: usize,
    pub picker_rows: Rect,
    pub picker_panel: Rect,
    /// The import and export panel, for clicks beside it.
    pub transfer_panel: Rect,
    /// The first row of the feeds panel in view, where they are and the panel around them, for clicks.
    pub manage_top: usize,
    pub manage_rows: Rect,
    pub manage_panel: Rect,
    /// What's typed on the feeds panel, and which of its fields that's for.
    pub manage_field: Option<(String, Editor)>,
    pub search: Option<Editor>,
    /// The path in the import and export panel, which takes the place of a window's file dialog.
    pub transfer_path: Option<Editor>,
    pub form: Option<Form>,
    pub shot: Option<Shot>,
    /// The room the viewer has for a picture.
    viewer_size: Size,
    /// What the terminal can draw.
    picker: Picker,
    /// The pictures of this article, the next and the last.
    pub pictures: HashMap<String, Inline>,
    /// Bumped when a picture's size becomes known, which changes the layout.
    pictures_seen: u64,
    /// The next article, whose pictures are fetched already, and those of the last one.
    ahead: Option<(i64, Vec<String>)>,
    behind: Vec<String>,
    /// What's on screen that a terminal can't move, to know when it must paint afresh: see `sync_stale`.
    on_screen: Vec<(String, isize, u16)>,
    covered: (Overlay, Option<u64>),
    /// When the toast with this serial goes.
    toast_until: Option<(u64, Instant)>,
}

fn hash(text: &str) -> u64 {
    let mut h = std::hash::DefaultHasher::new();
    text.hash(&mut h);
    h.finish()
}

/// The row of the `count`th row that isn't an image.
fn row_of_text(rows: &[Row], count: usize) -> usize {
    let mut seen = 0;
    for (i, row) in rows.iter().enumerate() {
        if seen == count {
            return i;
        }
        seen += usize::from(!row.image);
    }
    rows.len()
}

impl Ui {
    pub fn new(prefs: Prefs, demo: bool, picker: Picker) -> Self {
        Self {
            // Ctrl isn't ⌘ in a terminal, on a Mac either.
            app: App::new(prefs, false),
            demo,
            palette: article::palette(),
            effects: Vec::new(),
            jobs: Vec::new(),
            quit: false,
            repaint: false,
            shown: None,
            scroll: 0,
            max_scroll: 0,
            page: 1,
            column: Rect::default(),
            help_scroll: 0,
            picker_top: 0,
            picker_rows: Rect::default(),
            picker_panel: Rect::default(),
            transfer_panel: Rect::default(),
            manage_top: 0,
            manage_rows: Rect::default(),
            manage_panel: Rect::default(),
            manage_field: None,
            search: None,
            transfer_path: None,
            form: None,
            shot: None,
            viewer_size: Size::default(),
            picker,
            pictures: HashMap::new(),
            pictures_seen: 0,
            ahead: None,
            behind: Vec::new(),
            on_screen: Vec::new(),
            covered: (Overlay::None, None),
            toast_until: None,
        }
    }

    pub fn start(&mut self, status: Status) {
        let fx = self.app.start(status);
        self.run(fx);
    }

    pub fn quitting(&self) -> bool {
        self.quit
    }

    pub fn quit(&mut self) {
        self.quit = true;
    }

    pub fn can_scroll(&self) -> bool {
        self.max_scroll > 0
    }

    pub fn max_scroll(&self) -> usize {
        self.max_scroll
    }

    /// Server calls for the runner.
    pub fn take_effects(&mut self) -> Vec<Effect> {
        std::mem::take(&mut self.effects)
    }

    pub fn take_jobs(&mut self) -> Vec<Job> {
        std::mem::take(&mut self.jobs)
    }

    pub fn take_repaint(&mut self) -> bool {
        std::mem::take(&mut self.repaint)
    }

    /// The window-side effects happen here; the rest goes to the runner.
    fn run(&mut self, fx: Vec<Effect>) {
        for effect in fx {
            match effect {
                Effect::SavePrefs(p) => {
                    if !self.demo {
                        prefs::save(p);
                    }
                }
                Effect::ScrollPage { up } => {
                    let page = isize::try_from(self.page).unwrap_or(1);
                    self.scroll_by(if up { -page } else { page });
                }
                Effect::Quit => self.quit = true,
                effect => self.effects.push(effect),
            }
        }
    }

    pub fn outcome(&mut self, outcome: Outcome) {
        let fx = self.app.apply(outcome);
        self.run(fx);
    }

    fn scroll_by(&mut self, rows: isize) {
        self.scroll = self.scroll.saturating_add_signed(rows).min(self.max_scroll);
    }

    // ---- Keys ----------------------------------------------------------------------------------------------

    pub fn key(&mut self, event: KeyEvent) {
        if event.kind == KeyEventKind::Release {
            return;
        }
        let ctrl = event.modifiers.contains(KeyModifiers::CONTROL);
        match (event.code, self.app.overlay()) {
            (KeyCode::Char('c'), _) if ctrl => self.quit = true,
            (KeyCode::Char('l'), _) if ctrl => self.repaint = true,
            (_, Overlay::Search) => self.search_key(event),
            (_, Overlay::Transfer) => self.transfer_key(event),
            (_, Overlay::Manage) => self.manage_key(event),
            (_, Overlay::Settings) => self.form_key(event),
            (_, Overlay::Onboarding) if self.app.step() == Step::Connect => self.form_key(event),
            (KeyCode::Char('j') | KeyCode::Down, Overlay::Help) => self.help_scroll += 1,
            (KeyCode::Char('k') | KeyCode::Up, Overlay::Help) => self.help_scroll = self.help_scroll.saturating_sub(1),
            (_, Overlay::None) if self.app.screen() == Screen::Article && self.scroll_key(event) => {}
            _ => self.forward(event),
        }
    }

    /// Scrolling that a window does with Shift+Space, the wheel and the scrollbar.
    fn scroll_key(&mut self, event: KeyEvent) -> bool {
        let ctrl = event.modifiers.contains(KeyModifiers::CONTROL);
        let page = isize::try_from(self.page).unwrap_or(1);
        match event.code {
            KeyCode::PageDown => self.scroll_by(page),
            KeyCode::PageUp => self.scroll_by(-page),
            KeyCode::Char('d') if ctrl => self.scroll_by(page),
            KeyCode::Char('u') if ctrl => self.scroll_by(-page),
            KeyCode::Char('J') => self.scroll_by(LINE),
            KeyCode::Char('K') => self.scroll_by(-LINE),
            KeyCode::Home => self.scroll = 0,
            KeyCode::End => self.scroll = self.max_scroll,
            _ => return false,
        }
        true
    }

    /// Hands a key to the app, named as keys.rs names them. Ctrl stays here: Ctrl + and − set the window's font
    /// size, and a terminal has its own.
    fn forward(&mut self, event: KeyEvent) {
        let key = match event.code {
            KeyCode::Char(c) => c.to_string(),
            KeyCode::Enter => "Enter".into(),
            KeyCode::Tab | KeyCode::BackTab => "Tab".into(),
            KeyCode::Esc => "Escape".into(),
            KeyCode::Up => "ArrowUp".into(),
            KeyCode::Down => "ArrowDown".into(),
            KeyCode::Left => "ArrowLeft".into(),
            KeyCode::Right => "ArrowRight".into(),
            _ => return,
        };
        if event.modifiers.contains(KeyModifiers::CONTROL) {
            return;
        }
        let press = KeyPress {
            key,
            shift: event.modifiers.contains(KeyModifiers::SHIFT),
            alt: event.modifiers.contains(KeyModifiers::ALT),
            at_bottom: self.scroll >= self.max_scroll,
            ..KeyPress::default()
        };
        let (_, fx) = self.app.key(&press);
        self.run(fx);
    }

    fn search_key(&mut self, event: KeyEvent) {
        match event.code {
            KeyCode::Esc => self.forward(event),
            KeyCode::Enter => {
                let query = self.search.as_ref().map(|e| e.text().to_string()).unwrap_or_default();
                let fx = self.app.search(&query);
                self.run(fx);
            }
            _ => {
                if let Some(editor) = &mut self.search {
                    editor.key(event);
                }
            }
        }
    }

    /// The panel's field takes what a window's file dialog would; everything else is the core's.
    fn transfer_key(&mut self, event: KeyEvent) {
        let asking = self.app.transfer_view().is_some_and(|view| view.stage == "asking");
        match (event.code, &mut self.transfer_path) {
            (KeyCode::Esc, _) => self.forward(event),
            (KeyCode::Enter, Some(editor)) if asking => {
                let path = editor.text().to_string();
                let fx = self.app.feed_file(&path, false);
                self.run(fx);
            }
            (_, Some(editor)) if asking => editor.key(event),
            _ => self.forward(event),
        }
    }

    /// The panel's field, where it has one; everything else is the core's.
    fn manage_key(&mut self, event: KeyEvent) {
        self.sync_manage_field();
        match (event.code, &mut self.manage_field) {
            (KeyCode::Esc, _) => self.forward(event),
            (KeyCode::Enter, Some((_, editor))) => {
                let text = editor.text().to_string();
                let fx = self.app.submit_manage(&text);
                self.run(fx);
            }
            (_, Some((_, editor))) => editor.key(event),
            _ => self.forward(event),
        }
    }

    /// Connect as it stands, on that step.
    fn connect(&self) -> Option<ConnectView> {
        (self.app.overlay() == Overlay::Onboarding && self.app.step() == Step::Connect).then(|| self.app.connect_view())
    }

    /// Enter: on a row, that row, and anywhere else the form's main button.
    fn form_enter(&mut self) {
        let Some(form) = &self.form else { return };
        let fx = match form.focus {
            Focus::Row(_) if self.app.overlay() == Overlay::Settings => self.app.find_server(),
            Focus::Row(row) => self.app.choose_onboarding(row),
            Focus::Grant(row) => self.app.readeck_row(row, form.text(FieldKey::ReadeckUrl)),
            _ => {
                let values = form.values();
                self.app.submit_settings(values)
            }
        };
        self.run(fx);
    }

    /// Tells the core what's in the address box, or Readeck's. That's what makes an answer about the old address out of
    /// date.
    fn address_changed(&mut self) {
        let Some(form) = &self.form else { return };
        match form.focus {
            Focus::Field(FieldKey::Address) => {
                let address = form.text(FieldKey::Address).to_string();
                self.app.address_edited(&address);
            }
            Focus::Field(FieldKey::ReadeckUrl) => {
                let address = form.text(FieldKey::ReadeckUrl).to_string();
                self.app.readeck_edited(&address);
            }
            _ => {}
        }
    }

    fn form_key(&mut self, event: KeyEvent) {
        let connect = self.connect();
        let checking = self.app.settings().is_some_and(|f| f.checking);
        let Some(form) = &mut self.form else {
            return self.forward(event); // Esc, while the values are on their way
        };
        match event.code {
            KeyCode::Esc => self.forward(event),
            KeyCode::Tab | KeyCode::Down => form.step(1, connect.as_ref()),
            KeyCode::BackTab | KeyCode::Up => form.step(-1, connect.as_ref()),
            KeyCode::Enter => self.form_enter(),
            // A save or a key on its way is for what's in the fields now.
            _ if checking => {}
            // The server choice: either arrow, or Space, switches.
            KeyCode::Left | KeyCode::Right | KeyCode::Char(' ') if form.focus == Focus::Choice => {
                let kinds = Server::ALL.len();
                let at = Server::ALL.iter().position(|&kind| kind == form.server).unwrap_or(0);
                let by = if event.code == KeyCode::Left { kinds - 1 } else { 1 };
                form.server = Server::ALL[(at + by) % kinds];
            }
            _ => {
                if let Some(editor) = form.editor() {
                    editor.key(event);
                }
                self.address_changed();
            }
        }
    }

    /// Pasted text goes to the field with the cursor, in one piece: API keys are pasted.
    pub fn paste(&mut self, text: &str) {
        let checking = self.app.settings().is_some_and(|f| f.checking);
        if let Some(editor) = &mut self.search {
            editor.paste(text);
        } else if let Some(editor) = self.transfer_path.as_mut().filter(|_| self.app.overlay() == Overlay::Transfer) {
            editor.paste(text);
        } else if let Some((_, editor)) = self.manage_field.as_mut().filter(|_| self.app.overlay() == Overlay::Manage) {
            editor.paste(text);
        } else if let Some(form) = &mut self.form
            && !checking
            && (self.app.overlay() == Overlay::Settings || self.app.step() == Step::Connect)
            && let Some(editor) = form.editor()
        {
            editor.paste(text);
            self.address_changed();
        }
    }

    // ---- The mouse -----------------------------------------------------------------------------------------

    pub fn mouse(&mut self, event: MouseEvent) {
        let down = match event.kind {
            MouseEventKind::ScrollDown => true,
            MouseEventKind::ScrollUp => false,
            MouseEventKind::Down(MouseButton::Left) => return self.click(event.column, event.row),
            _ => return,
        };
        match self.app.overlay() {
            Overlay::None => self.scroll_by(if down { WHEEL } else { -WHEEL }),
            Overlay::Picker => {
                let fx = self.app.action(Action::PickerMove(if down { 1 } else { -1 }));
                self.run(fx);
            }
            Overlay::Manage => {
                let fx = self.app.action(Action::ManageCursor(if down { 1 } else { -1 }));
                self.run(fx);
            }
            Overlay::Help if down => self.help_scroll += 1,
            Overlay::Help => self.help_scroll = self.help_scroll.saturating_sub(1),
            _ => {}
        }
    }

    fn click(&mut self, column: u16, row: u16) {
        match self.app.overlay() {
            Overlay::None => match self.target_at(column, row).cloned() {
                Some(Target::Link(url)) => {
                    let fx = self.app.open_link(&url);
                    self.run(fx);
                }
                Some(Target::Image(src)) => self.app.view_image(&src),
                None => {}
            },
            Overlay::Picker => {
                let spot = ratatui::layout::Position::new(column, row);
                if self.picker_rows.contains(spot) {
                    let index = self.picker_top + usize::from(row - self.picker_rows.y);
                    if index < self.app.picker_items().len() {
                        let fx = self.app.choose_row(index);
                        self.run(fx);
                    }
                } else if !self.picker_panel.contains(spot) {
                    self.app.picker_backdrop();
                }
            }
            Overlay::Manage => {
                let spot = ratatui::layout::Position::new(column, row);
                let rows = self.app.manage_view().map_or(0, |view| view.rows.len());
                if self.manage_rows.contains(spot) {
                    let index = self.manage_top + usize::from(row - self.manage_rows.y);
                    if index < rows {
                        let fx = self.app.manage_row(index);
                        self.run(fx);
                    }
                } else if !self.manage_panel.contains(spot) {
                    let fx = self.app.cancel_manage();
                    self.run(fx);
                }
            }
            Overlay::Help => self.app.close_help_overlay(),
            Overlay::Image => self.app.close_viewer(),
            Overlay::Transfer if !self.transfer_panel.contains(ratatui::layout::Position::new(column, row)) => {
                let fx = self.app.cancel_transfer();
                self.run(fx);
            }
            _ => {}
        }
    }

    /// What's under a cell of the article.
    fn target_at(&self, column: u16, row: u16) -> Option<&Target> {
        let area = self.column;
        if !area.contains(ratatui::layout::Position::new(column, row)) {
            return None;
        }
        let x = column - area.x;
        let index = self.scroll + usize::from(row - area.y);
        let row = self.shown.as_ref()?.rows.get(index)?;
        row.targets.iter().find(|(start, end, _)| (*start..*end).contains(&x)).map(|(_, _, target)| target)
    }

    // ---- Answers and time ------------------------------------------------------------------------------------

    pub fn image(&mut self, answer: Answer) {
        match answer {
            Answer::Viewer { src, size, result } => {
                let Some(shot) = self.shot.as_mut().filter(|s| s.src == src) else { return };
                shot.fitting = None;
                shot.picture = match result {
                    Ok((bytes, protocol)) => Picture::Ready { bytes, protocol, size },
                    Err(message) => Picture::Failed(message),
                };
                self.repaint = true;
            }
            Answer::Fetched { src, result } => {
                // Unless you've moved on far enough for it to be dropped meanwhile.
                let Some(picture) = self.pictures.get_mut(&src) else { return };
                *picture = match result {
                    Ok((bytes, dimensions)) => Inline::Here { bytes, dimensions, sliced: None, fitting: None },
                    Err(_) => Inline::Failed,
                };
                self.pictures_seen += 1;
            }
            Answer::Sliced { src, size, result } => {
                let Some(picture) = self.pictures.get_mut(&src) else { return };
                let Inline::Here { sliced, fitting, .. } = picture else { return };
                if *fitting == Some(size) {
                    *fitting = None;
                }
                match result {
                    Ok(protocol) => *sliced = Some((size, protocol)),
                    // The terminal can't take it: back to the line of text, or the layout would ask again on
                    // every draw.
                    Err(_) => {
                        *picture = Inline::Failed;
                        self.pictures_seen += 1;
                    }
                }
            }
        }
    }

    /// How long until something changes by itself.
    pub fn next_wake(&self) -> Option<Duration> {
        self.toast_until.map(|(_, at)| at.saturating_duration_since(Instant::now()))
    }

    /// Time passed: the toast's may be up. Errors stay up longer, to be read.
    pub fn tick(&mut self) {
        let serial = self.app.toast().map(|t| (t.serial, t.kind));
        match (serial, self.toast_until) {
            (Some((serial, _)), Some((known, at))) if serial == known => {
                if Instant::now() >= at {
                    self.app.dismiss_toast(serial);
                    self.toast_until = None;
                }
            }
            (Some((serial, kind)), _) => {
                let error = kind == crate::app::ToastKind::Error;
                self.toast_until =
                    Some((serial, Instant::now() + Duration::from_millis(if error { 6000 } else { 2200 })));
            }
            (None, _) => self.toast_until = None,
        }
    }

    // ---- Before drawing ------------------------------------------------------------------------------------

    /// Brings what the terminal keeps in line with the app, for a screen of this size.
    pub fn prepare(&mut self, area: Rect) {
        self.viewer_size = Size::new(area.width, area.height.saturating_sub(2));
        self.tick();
        self.sync_fields();
        self.lay_out(area);
        self.sync_pictures();
        self.sync_viewer();
        self.sync_stale();
    }

    /// The field goes with the stage that has it, and starts from what the core says: after a refusal, what was typed.
    fn sync_manage_field(&mut self) {
        let view = self.app.manage_view().filter(|_| self.app.manage_shown());
        let Some((title, field)) = view.and_then(|view| Some((view.title, view.field?))) else {
            self.manage_field = None;
            return;
        };
        let which = format!("{title} {}", field.label);
        if self.manage_field.as_ref().is_none_or(|(known, _)| *known != which) {
            self.manage_field = Some((which, Editor::new(&field.value)));
        }
    }

    fn sync_fields(&mut self) {
        let searching = self.app.overlay() == Overlay::Search;
        if searching && self.search.is_none() {
            self.search = Some(Editor::selected(self.app.search_query()));
        } else if !searching {
            self.search = None;
        }
        // It stays behind the question about a file that's there, and goes with the panel.
        match self.app.transfer_view() {
            None => self.transfer_path = None,
            Some(view) if self.transfer_path.is_none() => {
                let start = if view.kind == "export" { format!("~/{}", view.suggested) } else { String::new() };
                self.transfer_path = Some(Editor::new(&start));
            }
            Some(_) => {}
        }
        self.sync_manage_field();
        if self.app.overlay() != Overlay::Help {
            self.help_scroll = 0;
        }
        let connect = self.connect();
        let readeck = self.app.overlay() == Overlay::Settings;
        match self.app.settings() {
            Some(state) if self.app.in_settings() => {
                let v = &state.values;
                let form = self.form.get_or_insert_with(|| {
                    // The first empty field of the chosen server, or its last.
                    let own = providers::provider(v.server).fields;
                    let empty = own.iter().find(|f| v.field(f.key).is_empty());
                    let focus = empty.or(own.last()).map_or(Focus::Choice, |f| Focus::Field(f.key));
                    let mut form = Form {
                        server: v.server,
                        fields: HashMap::new(),
                        focus,
                        readeck,
                        grant_rows: 0,
                        refill: state.refill,
                        connect: ("", Vec::new()),
                    };
                    form.load(v);
                    form
                });
                form.readeck = readeck;
                let grant = self.app.readeck_view();
                form.grant_rows = if readeck { grant.rows.len() } else { 0 };
                // The row under it went: a stage with fewer of them, or none. On the way to the next stage the focus
                // stays, as that's what keeps what comes in view.
                let passing = matches!(grant.stage, "asking" | "keeping");
                if let Focus::Grant(row) = form.focus
                    && row >= form.grant_rows
                    && !passing
                {
                    let last = form.grant_rows.checked_sub(1);
                    form.focus = last.map_or(Focus::Field(FieldKey::ReadeckToken), Focus::Grant);
                }
                if form.refill != state.refill {
                    form.refill = state.refill;
                    form.load(v);
                }
                if let Some(view) = connect {
                    let shown = (view.stage, view.fields.iter().map(|f| f.key).collect());
                    if form.connect != shown {
                        form.connect = shown;
                        form.place_focus(&view);
                    }
                }
            }
            _ => self.form = None,
        }
    }

    /// Where an article goes in a screen of this size: as wide as `[` and `]` say, centred.
    fn column_for(&self, area: Rect) -> Rect {
        let body = area.intersection(Rect::new(area.x, area.y + BAR, area.width, area.height.saturating_sub(BAR)));
        let gutter = if body.width < 60 { 1 } else { 2 };
        let wanted = u16::try_from(self.app.prefs().width_factor).unwrap_or(40).saturating_mul(CELLS_PER_FONT_SIZE);
        let width = wanted.min(body.width.saturating_sub(2 * gutter)).max(1);
        Rect::new(body.x + body.width.saturating_sub(width) / 2, body.y, width, body.height)
    }

    fn lay_out(&mut self, area: Rect) {
        self.column = self.column_for(area);
        let width = usize::from(self.column.width);
        let height = usize::from(self.column.height);
        self.page = (height * 85 / 100).max(1);
        let entry = self.app.current().filter(|_| self.app.screen() == Screen::Article);
        let Some(entry) = entry else {
            self.shown = None;
            self.scroll = 0;
            self.max_scroll = 0;
            return;
        };
        let content = hash(&entry.content);
        if !self.shown.as_ref().is_some_and(|s| s.id == entry.id && s.content == content) {
            let rendered = convert::convert(&entry.content, &entry.url, &self.palette);
            let blocks = native::native(&rendered.blocks);
            let images = convert::image_refs(&rendered.blocks).into_iter().map(|i| i.src).collect();
            if let Some(last) = self.shown.take().filter(|s| s.id != entry.id) {
                self.behind = last.images;
            }
            self.shown = Some(Shown { id: entry.id, content, blocks, images, laid: None, rows: Vec::new() });
            self.scroll = 0;
        }
        let upcoming = self.app.upcoming();
        if self.ahead.as_ref().map(|a| a.0) != upcoming.map(|e| e.id) {
            let images = upcoming.map(|e| convert::images(&e.content, &e.url));
            let srcs = images.unwrap_or_default().into_iter().map(|i| i.src).collect();
            self.ahead = upcoming.map(|e| (e.id, srcs));
        }
        let Some(shown) = self.shown.as_mut() else { return };
        // Only these stay: an article's pictures are a lot to keep for no reason.
        let ahead = self.ahead.iter().flat_map(|a| &a.1);
        let wanted: Vec<&String> = shown.images.iter().chain(ahead).chain(&self.behind).collect();
        self.pictures.retain(|src, _| wanted.contains(&src));
        for src in wanted {
            if !self.pictures.contains_key(src) {
                self.pictures.insert(src.clone(), Inline::Loading);
                self.jobs.push(Job::Fetch { src: src.clone() });
            }
        }
        let laid = (width, entry.status, self.pictures_seen);
        if shown.laid != Some(laid) {
            // Pictures arriving above where you are mustn't move what you're reading.
            let reading = (shown.laid.is_some_and(|l| l.0 == width))
                .then(|| shown.rows.iter().take(self.scroll).filter(|r| !r.image).count());
            let (font, pictures) = (self.picker.font_size(), &self.pictures);
            let room = self.column.height.max(1);
            let size_of = |src: &str, width: usize| match pictures.get(src) {
                Some(Inline::Here { dimensions, .. }) => {
                    let available = Size::new(u16::try_from(width).unwrap_or(u16::MAX), room);
                    Some(images::fit(*dimensions, font, available))
                }
                _ => None,
            };
            let mut rows = vec![Row::default()];
            let header = Header {
                title: display_title(&entry.title),
                url: &entry.url,
                feed: &entry.feed.title,
                author: &entry.author,
                published: &entry.published_at,
                reading_time: entry.reading_time,
                status: status_name(entry.status),
            };
            rows.extend(article::header(&header, width));
            rows.extend(article::blocks(&shown.blocks, width, &size_of));
            shown.rows = rows;
            shown.laid = Some(laid);
            if let Some(text_rows) = reading {
                self.scroll = row_of_text(&shown.rows, text_rows);
            }
        }
        // Room to scroll the last lines up, as in the window.
        self.max_scroll = (shown.rows.len() + height * 3 / 10).saturating_sub(height);
        self.scroll = self.scroll.min(self.max_scroll);
    }

    /// Asks for each picture in the article to be encoded for the rows it got.
    fn sync_pictures(&mut self) {
        let Some(shown) = &self.shown else { return };
        for slice in shown.rows.iter().filter_map(|row| row.slice.as_ref()).filter(|slice| slice.row == 0) {
            if let Some(Inline::Here { bytes, sliced, fitting, .. }) = self.pictures.get_mut(&slice.src)
                && sliced.as_ref().map(|s| s.0) != Some(slice.size)
                && *fitting != Some(slice.size)
            {
                *fitting = Some(slice.size);
                let (src, bytes) = (slice.src.clone(), Arc::clone(bytes));
                self.jobs.push(Job::Slice { src, bytes, size: slice.size });
            }
        }
    }

    /// The pictures in view: each with the row its top is on (above the article's first when scrolled past it).
    pub fn visible_pictures(&self) -> Vec<(&Slice, isize)> {
        let Some(shown) = &self.shown else { return Vec::new() };
        let visible = shown.rows.iter().skip(self.scroll).take(usize::from(self.column.height));
        let mut found: Vec<(&Slice, isize)> = Vec::new();
        let mut last: Option<&Slice> = None;
        for (i, row) in visible.enumerate() {
            let Some(slice) = &row.slice else {
                last = None;
                continue;
            };
            let goes_on = last.is_some_and(|l| l.src == slice.src && l.row + 1 == slice.row);
            if !goes_on {
                let top = isize::try_from(i).unwrap_or(0) - isize::try_from(slice.row).unwrap_or(0);
                found.push((slice, top));
            }
            last = Some(slice);
        }
        found
    }

    /// Sixel and iTerm2 pictures are pixels over the text, not cells: they stay when the text under them is
    /// redrawn or moves on, so when anything about them changes the screen is painted afresh. Kitty's are cells, and
    /// half-blocks are plain text.
    fn sync_stale(&mut self) {
        let on_screen: Vec<_> =
            self.visible_pictures().iter().map(|(s, top)| (s.src.clone(), *top, s.column)).collect();
        let covered = (self.app.overlay(), self.app.toast().map(|t| t.serial));
        let moved = on_screen != self.on_screen;
        let shown = !on_screen.is_empty() || !self.on_screen.is_empty();
        let paints = matches!(self.picker.protocol_type(), ProtocolType::Sixel | ProtocolType::Iterm2);
        if paints && (moved || (shown && covered != self.covered)) {
            self.repaint = true;
        }
        self.on_screen = on_screen;
        self.covered = covered;
    }

    /// Asks for the picture the viewer shows, for the room it has.
    fn sync_viewer(&mut self) {
        let Some(wanted) = self.app.viewer().and_then(|v| v.images.get(v.index)) else {
            self.repaint |= self.shot.take().is_some();
            return;
        };
        let size = self.viewer_size;
        if self.shot.as_ref().is_none_or(|shot| shot.src != wanted.src) {
            let src = wanted.src.clone();
            self.repaint = true;
            self.jobs.push(match self.pictures.get(&src) {
                Some(Inline::Here { bytes, .. }) => Job::Fit { src: src.clone(), bytes: Arc::clone(bytes), size },
                _ => Job::Load { src: src.clone(), size },
            });
            self.shot = Some(Shot { src, picture: Picture::Loading, fitting: Some(size) });
        }
        let Some(shot) = self.shot.as_mut() else { return };
        if let Picture::Ready { bytes, size: fitted, .. } = &shot.picture
            && *fitted != size
            && shot.fitting != Some(size)
        {
            shot.fitting = Some(size);
            self.jobs.push(Job::Fit { src: shot.src.clone(), bytes: Arc::clone(bytes), size });
        }
    }

    /// The keys, as they are here: no Ctrl + and −, and a few more ways to scroll.
    pub fn help_lines(&self) -> Vec<(String, String)> {
        let mut lines = Vec::new();
        for (keys, action) in self.app.help_lines() {
            if keys.starts_with("Ctrl ") {
                continue;
            }
            if keys.contains("Shift+Space") {
                lines.push(("Space".to_string(), "scroll; at the end = next".to_string()));
                lines.push(("PgDn / PgUp / Ctrl+D / Ctrl+U".to_string(), "scroll a page".to_string()));
                lines.push(("J / K".to_string(), "scroll a little".to_string()));
                lines.push(("Home / End".to_string(), "top / end of the article".to_string()));
                continue;
            }
            lines.push((keys, action));
        }
        lines
    }
}
