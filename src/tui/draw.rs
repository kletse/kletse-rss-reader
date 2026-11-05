//! Draws the state in `Ui`, the way the QML does: a top bar, the reader under it, and the overlays over that.

use super::article::{self, ACCENT, DIM, Row};
use super::ui::{Form, Inline, Picture, Ui};
use crate::app::{Overlay, PickerList, Screen, Step, ToastKind};
use crate::paging::Mode;
use crate::providers::{self, Field};
use ratatui::Frame;
use ratatui::layout::{Alignment, Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Clear, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState};
use ratatui_image::Image;
use ratatui_image::sliced::{SignedPosition, SlicedImage};
use unicode_width::UnicodeWidthStr;

const BOLD: Style = Style::new().add_modifier(Modifier::BOLD);
const ACCENTED: Style = Style::new().fg(ACCENT);
/// The logo, as near as text gets: the articles to come and the one you're reading.
const LOGO: &str = "· • O";
const ERROR: Style = Style::new().fg(Color::Red);

pub fn draw(ui: &mut Ui, frame: &mut Frame) {
    let area = frame.area();
    if ui.app.overlay() != Overlay::Image {
        top_bar(ui, frame, area);
        body(ui, frame, area);
        if ui.app.picker_shown() {
            picker(ui, frame, area);
        }
    }
    match ui.app.overlay() {
        Overlay::Image => viewer(ui, frame, area),
        Overlay::Search => search(ui, frame, area),
        Overlay::Help => help(ui, frame, area),
        Overlay::Confirm => confirm(ui, frame, area),
        _ => {}
    }
    toast(ui, frame, area);
}

fn row_at(y: u16, area: Rect) -> Rect {
    Rect::new(area.x, y, area.width, 1).intersection(area)
}

/// `text`, cut to `width` columns.
fn fitted(text: &str, width: usize) -> String {
    if text.width() <= width {
        return text.to_string();
    }
    let mut out = String::new();
    for c in text.chars() {
        if out.width() + 2 > width {
            break;
        }
        out.push(c);
    }
    out.push('…');
    out
}

fn top_bar(ui: &Ui, frame: &mut Frame, area: Rect) {
    let app = &ui.app;
    if app.screen() == Screen::Settings {
        return;
    }
    let inner = Rect::new(area.x + 1, area.y, area.width.saturating_sub(2), 1).intersection(area);
    let label = if app.selection_label().is_empty() { "Feeds" } else { app.selection_label() };
    let mut left =
        vec![Span::styled(LOGO, ACCENTED), Span::raw(" "), Span::raw(fitted(label, usize::from(area.width) * 2 / 5))];
    if app.searching() {
        left.extend([Span::raw("  "), Span::styled("SEARCH", ACCENTED)]);
    } else if app.starred_list() {
        left.extend([Span::raw("  "), Span::styled("STARRED", ACCENTED)]);
    } else if app.history_badge() {
        left.extend([Span::raw("  "), Span::styled("HISTORY", ACCENTED)]);
    }
    frame.render_widget(Line::from(left), inner);
    // The article on screen is starred (`*`): a star before where you are, the position itself staying put.
    let position = app.position();
    let starred = app.starred() && app.screen() == Screen::Article;
    let mut middle = vec![Span::styled(position.clone(), DIM)];
    if starred {
        middle.splice(0..0, [Span::styled("★ ", ACCENTED)]);
        middle.push(Span::raw("  "));
    }
    frame.render_widget(Line::from(middle).centered(), inner);
    // There's a key for it, and no room for both: the samples are short of neither `?` nor time.
    if app.samples() {
        let marker = vec![Span::styled("sample articles", ACCENTED), Span::styled(" · , connect", DIM)];
        frame.render_widget(Line::from(marker).right_aligned(), inner);
    } else {
        frame.render_widget(Line::styled("? keys", DIM).right_aligned(), inner);
    }
    frame.render_widget(Line::styled("─".repeat(usize::from(area.width)), DIM), row_at(area.y + 1, area));
}

fn body(ui: &mut Ui, frame: &mut Frame, area: Rect) {
    match ui.app.screen() {
        Screen::Article => reader(ui, frame, area),
        Screen::Settings if ui.app.overlay() == Overlay::Onboarding => onboarding(ui, frame, area),
        Screen::Settings => settings(ui, frame, area),
        Screen::Loading => message(ui, frame, "", "", "Loading…"),
        Screen::End => {
            let (title, hint) = (ui.app.end_title(), ui.app.end_hint());
            message(ui, frame, LOGO, title, &hint);
        }
        Screen::Empty => {}
    }
}

fn message(ui: &Ui, frame: &mut Frame, logo: &str, title: &str, text: &str) {
    let column = ui.column;
    let width = usize::from(column.width);
    let mut rows = vec![Row::default()];
    if !logo.is_empty() {
        rows.extend(article::plain(logo, ACCENTED, width));
        rows.push(Row::default());
    }
    if !title.is_empty() {
        rows.extend(article::plain(title, BOLD, width));
        rows.push(Row::default());
    }
    rows.extend(article::plain(text, DIM, width));
    draw_rows(frame, &rows, column);
}

fn draw_rows(frame: &mut Frame, rows: &[Row], area: Rect) {
    let buffer = frame.buffer_mut();
    for (row, y) in rows.iter().zip(area.y..area.bottom()) {
        buffer.set_line(area.x, y, &row.line, area.width);
    }
}

fn reader(ui: &Ui, frame: &mut Frame, area: Rect) {
    let Some(shown) = &ui.shown else { return };
    let visible = shown.rows.get(ui.scroll..).unwrap_or_default();
    draw_rows(frame, visible, ui.column);
    // A row short of the screen's end: a sixel is put in place by moving the cursor down over it and back up, and
    // the terminal stops it at the last row, so it would come back a row too high.
    let room = Rect { height: ui.column.height.saturating_sub(1), ..ui.column };
    for (slice, top) in ui.visible_pictures() {
        // Encoded for another size, after a resize: it's on its way.
        if let Some(Inline::Here { sliced: Some((size, protocol)), .. }) = ui.pictures.get(&slice.src)
            && *size == slice.size
        {
            let top = i16::try_from(top).unwrap_or(i16::MIN);
            let at = SignedPosition::from((i16::try_from(slice.column).unwrap_or(0), top));
            frame.render_widget(SlicedImage::new(protocol, at), room);
        }
    }
    if ui.can_scroll() {
        let mut state = ScrollbarState::new(ui.max_scroll()).position(ui.scroll);
        let bar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .begin_symbol(None)
            .end_symbol(None)
            .track_symbol(None)
            .thumb_symbol("▐")
            .thumb_style(DIM);
        let track = Rect::new(area.x, ui.column.y, area.width, ui.column.height);
        frame.render_stateful_widget(bar, track, &mut state);
    }
}

/// Wide enough for the longest link in `providers.rs`, indented, to stay on one line and be copied.
const FORM_WIDTH: u16 = 72;

/// Where the form and the first-run screens go. They take the whole window, with no top bar over them.
fn form_column(area: Rect) -> Rect {
    let width = area.width.saturating_sub(4).min(FORM_WIDTH);
    Rect::new(area.x + (area.width - width.min(area.width)) / 2, area.y, width, area.height)
}

/// Rows, and the place in them to keep in view.
#[derive(Default)]
struct Page {
    rows: Vec<Row>,
    /// Row and column of the text cursor.
    cursor: Option<(usize, usize)>,
    /// The rows that have to stay on screen: the field or link with the focus and what's said about it.
    focused: std::ops::Range<usize>,
}

impl Page {
    fn text(&mut self, text: &str, style: Style, width: usize) {
        self.rows.extend(article::plain(text, style, width));
    }

    fn blank(&mut self) {
        self.rows.push(Row::default());
    }

    /// A heading and a line under it, and any problem with the config, which every step has to say.
    fn head(&mut self, title: &str, text: &str, problem: Option<&str>, width: usize) {
        self.blank();
        self.text(title, BOLD, width);
        self.blank();
        self.text(text, DIM, width);
        self.blank();
        if let Some(problem) = problem {
            self.text(problem, Style::new().fg(Color::Yellow), width);
            self.blank();
        }
    }

    fn section(&mut self, title: &str, note: &str) {
        let line = Line::from(vec![Span::styled(title.to_string(), ACCENTED), Span::styled(format!(" · {note}"), DIM)]);
        self.rows.push(Row { line, ..Row::default() });
    }

    fn indented(&mut self, text: &str, style: Style, width: usize) {
        let indent = Span::raw("    ");
        self.rows.extend(article::prefixed(article::plain(text, style, width.saturating_sub(4)), &indent, &indent));
    }

    fn field(&mut self, form: &Form, field: &Field, width: usize) {
        let focused = form.focus == Some(field.key);
        let start = self.rows.len();
        let label = format!("  {}", field.label);
        self.rows
            .push(Row { line: Line::from(Span::styled(label, if focused { BOLD } else { DIM })), ..Row::default() });
        let editor = form.fields.get(&field.key);
        let (text, at) = editor.map_or((String::new(), 0), |e| e.view(width.saturating_sub(4), field.secret));
        let value = if text.is_empty() {
            Span::styled(fitted(field.placeholder, width.saturating_sub(4)), DIM)
        } else {
            Span::raw(text)
        };
        if focused {
            self.cursor = Some((self.rows.len(), 4 + at));
        }
        self.rows.push(Row { line: Line::from(vec![Span::raw("  "), mark(focused), value]), ..Row::default() });
        if !field.help.is_empty() {
            self.indented(field.help, DIM, width);
        }
        if focused {
            self.focused = start..self.rows.len();
        }
    }

    /// The server choice, the chosen server's fields and links, and Readeck when it's on the form.
    fn form(&mut self, form: &Form, width: usize) {
        self.section("Server", "← → to switch");
        let mut choice = vec![Span::raw("  "), mark(form.focus.is_none())];
        for (n, provider) in providers::PROVIDERS.iter().enumerate() {
            if n > 0 {
                choice.push(Span::raw("   "));
            }
            let on = provider.protocol == form.server;
            let style = if on { BOLD } else { DIM };
            choice.push(Span::styled(format!("{} {}", if on { "●" } else { "○" }, provider.label), style));
        }
        self.rows.push(Row { line: Line::from(choice), ..Row::default() });
        self.blank();
        let provider = providers::provider(form.server);
        self.section(provider.short_label, "required");
        for (n, field) in provider.fields.iter().enumerate() {
            if n > 0 {
                self.blank();
            }
            self.field(form, field, width);
        }
        let last = provider.fields.last().map(|f| f.key);
        for link in provider.links {
            self.blank();
            self.indented(&format!("{}: {}", link.label, link.url), DIM, width);
            // They can't take the focus, so on a small screen they come into view with the field above them.
            if form.focus == last {
                self.focused.end = self.rows.len();
            }
        }
        if form.readeck {
            self.blank();
            self.section("Readeck", "optional: leave empty to hide the bookmark button");
            for (n, field) in providers::READECK.iter().enumerate() {
                if n > 0 {
                    self.blank();
                }
                self.field(form, field, width);
            }
        }
    }

    /// The rows, cut to leave room for `foot` and keep what has the focus in view: the foot says what went wrong, so
    /// it's what the rows give way to.
    fn draw(&self, frame: &mut Frame, foot: &[Row], column: Rect, cursor: bool) {
        let foot_height = u16::try_from(foot.len()).unwrap_or(u16::MAX).min(column.height);
        let height = column.height - foot_height;
        let room = usize::from(height);
        let start = self.focused.end.saturating_sub(room).min(self.focused.start).min(self.rows.len());
        let shown = &self.rows[start..];
        let form_height = u16::try_from(shown.len()).unwrap_or(u16::MAX).min(height);
        draw_rows(frame, shown, Rect { height: form_height, ..column });
        draw_rows(frame, foot, Rect { y: column.y + form_height, height: foot_height, ..column });
        if let Some((row, x)) = self.cursor.filter(|_| cursor)
            && let (Ok(row), Ok(x)) = (u16::try_from(row.saturating_sub(start)), u16::try_from(x))
            && row < form_height
        {
            frame.set_cursor_position(Position::new(column.x + x, column.y + row));
        }
    }
}

fn mark(focused: bool) -> Span<'static> {
    if focused { Span::styled("› ", ACCENTED) } else { Span::raw("  ") }
}

/// Under the form: what went wrong, and what the keys do.
fn form_foot(error: &str, hint: &str, width: usize) -> Vec<Row> {
    let mut foot = vec![Row::default()];
    if !error.is_empty() {
        foot.extend(article::plain(error, ERROR, width));
        foot.push(Row::default());
    }
    foot.extend(article::plain(hint, DIM, width));
    foot
}

fn settings(ui: &mut Ui, frame: &mut Frame, area: Rect) {
    let Some(state) = ui.app.settings() else { return };
    let Some(form) = &ui.form else { return };
    let column = form_column(area);
    let w = usize::from(column.width);
    let mut page = Page::default();
    let intro = "Changes are checked against your servers before they are saved.";
    page.head("Settings", intro, state.problem.as_deref(), w);
    page.form(form, w);
    let hint = if state.checking { "Checking…" } else { "Enter to save · Esc to cancel · Tab for the next field" };
    page.draw(frame, &form_foot(&state.error, hint, w), column, !state.checking);
}

/// First run, on the step it's at. Each step has what it needs, so nothing here waits on another.
fn onboarding(ui: &mut Ui, frame: &mut Frame, area: Rect) {
    let column = form_column(area);
    let w = usize::from(column.width);
    let state = ui.app.settings();
    let problem = state.and_then(|s| s.problem.as_deref());
    let cursor = ui.app.step_cursor();
    let mut page = Page::default();
    match ui.app.step() {
        Step::Welcome => {
            page.head(providers::WELCOME_TITLE, providers::WELCOME_TEXT, problem, w);
            for (n, row) in providers::WELCOME.iter().enumerate() {
                let style = if row.quiet { DIM } else { Style::new() };
                let line = Line::from(vec![mark(n == cursor), Span::styled(row.label, style)]);
                page.rows.push(Row { line, ..Row::default() });
            }
            page.draw(frame, &form_foot("", "↑ ↓ and Enter · Ctrl+C to quit", w), column, false);
        }
        Step::NoServer => {
            let panel = providers::NO_SERVER;
            page.head(panel.title, panel.text, problem, w);
            for (n, offer) in panel.offers.iter().enumerate() {
                let start = page.rows.len();
                page.section(offer.name, offer.price);
                page.indented(offer.text, Style::new(), w);
                link_row(&mut page, offer.link.url, n == cursor, w);
                if n == cursor {
                    page.focused = start..page.rows.len();
                }
                page.blank();
            }
            page.text(panel.note, DIM, w);
            page.blank();
            let start = page.rows.len();
            page.text(panel.more.label, ACCENTED, w);
            link_row(&mut page, panel.more.url, cursor == panel.offers.len(), w);
            if cursor == panel.offers.len() {
                page.focused = start..page.rows.len();
            }
            page.draw(frame, &form_foot("", "↑ ↓ and Enter to open · Esc to go back", w), column, false);
        }
        Step::Connect => {
            page.head(providers::CONNECT_TITLE, providers::CONNECT_TEXT, problem, w);
            let (error, checking) = state.map_or(("", false), |s| (s.error.as_str(), s.checking));
            if let Some(form) = &ui.form {
                page.form(form, w);
            }
            let hint =
                if checking { "Checking…" } else { "Enter to connect · Esc to go back · Tab for the next field" };
            page.draw(frame, &form_foot(error, hint, w), column, !checking);
        }
    }
}

/// A URL in full: a terminal may have no browser to open, and being told one opened proves nothing.
fn link_row(page: &mut Page, url: &str, focused: bool, width: usize) {
    let style = if focused { ACCENTED } else { DIM };
    let first = if focused { Span::styled("  › ", ACCENTED) } else { Span::raw("    ") };
    let rows = article::plain(url, style, width.saturating_sub(4));
    page.rows.extend(article::prefixed(rows, &first, &Span::raw("    ")));
}

/// Makes sure every cell of `area` gets written. After a repaint ratatui leaves out blanks, as the screen is blank,
/// but a sixel picture underneath isn't: it would show through them. A hidden space is a blank it won't leave out.
fn solid(frame: &mut Frame, area: Rect) {
    let buffer = frame.buffer_mut();
    for position in area.intersection(buffer.area).positions() {
        if let Some(cell) = buffer.cell_mut(position)
            && cell.symbol() == " "
        {
            cell.modifier.insert(Modifier::HIDDEN);
        }
    }
}

/// A box with a title in the middle of `area`, on top of what's there; returns what's inside it.
fn panel(frame: &mut Frame, area: Rect, wanted: (u16, u16), block: Block<'_>) -> (Rect, Rect) {
    let width = wanted.0.min(area.width.saturating_sub(4)).max(1);
    let height = wanted.1.min(area.height.saturating_sub(2)).max(1);
    let outer = Rect::new(area.x + (area.width - width) / 2, area.y + (area.height - height) / 2, width, height)
        .intersection(area);
    frame.render_widget(Clear, outer);
    let block = block.border_type(BorderType::Rounded).border_style(DIM);
    let inner = block.inner(outer);
    frame.render_widget(block, outer);
    (outer, inner)
}

fn picker(ui: &mut Ui, frame: &mut Frame, area: Rect) {
    let history = ui.app.picker_mode() == Mode::History;
    let items = ui.app.picker_items();
    let note = match ui.app.picker_list() {
        PickerList::Loading => Some(("Loading…".to_string(), DIM)),
        PickerList::Error(message) => Some((message.clone(), ERROR)),
        PickerList::Ready(_) if !items.is_empty() => None,
        PickerList::Ready(_) if history => Some(("Nothing read yet. Press h for unread items.".to_string(), DIM)),
        PickerList::Ready(_) if ui.app.capabilities().refresh => {
            Some(("Nothing unread. Press r to refresh, or h for history.".to_string(), DIM))
        }
        PickerList::Ready(_) => Some(("Nothing unread. Press h for history.".to_string(), DIM)),
    };
    let order = if ui.app.newest_first() { "Newest first" } else { "Oldest first" };
    let swap = if history { "h unread" } else { "h history" };
    let refresh = if ui.app.capabilities().refresh { " · r refresh" } else { "" };
    let settings = if ui.app.samples() { "connect" } else { "settings" };
    let mut block = Block::bordered()
        .title(Line::styled(if history { " History " } else { " Unread " }, BOLD))
        .title_bottom(Line::styled(format!(" {swap} · / search{refresh} · , {settings} · ? keys "), DIM));
    if !history {
        block = block.title(Line::styled(format!(" {order} "), DIM).alignment(Alignment::Right));
    }
    let wanted_width = area.width.saturating_sub(4).min(72);
    let inner_width = usize::from(wanted_width.saturating_sub(4));
    let lines = match &note {
        Some((text, style)) => article::plain(text, *style, inner_width),
        None => Vec::new(),
    };
    let rows = if note.is_some() { lines.len() } else { items.len() };
    let tallest = usize::from(area.height) * 4 / 5;
    let height = u16::try_from(rows.min(tallest.saturating_sub(2)) + 2).unwrap_or(u16::MAX);
    let (outer, inner) = panel(frame, area, (wanted_width, height), block);
    let inner = Rect::new(inner.x + 1, inner.y, inner.width.saturating_sub(2), inner.height);
    ui.picker_panel = outer;
    ui.picker_rows = inner;
    if note.is_some() {
        draw_rows(frame, &lines, inner);
        solid(frame, outer);
        return;
    }
    let cursor = ui.app.picker_cursor();
    let room = usize::from(inner.height).max(1);
    // Only as far as it takes to keep the cursor in view.
    ui.picker_top = ui.picker_top.min(cursor).max((cursor + 1).saturating_sub(room));
    ui.picker_top = ui.picker_top.min(items.len().saturating_sub(room));
    let width = usize::from(inner.width);
    for (i, item) in items.iter().enumerate().skip(ui.picker_top).take(room) {
        let indent = if item.depth == 1 { "  " } else { "" };
        let count = item.count.map(|c| c.to_string()).unwrap_or_default();
        let label = fitted(&item.label, width.saturating_sub(indent.width() + count.width() + 1));
        let gap = width.saturating_sub(indent.width() + label.width() + count.width());
        let selected = i == cursor;
        let name = if item.depth == 0 || selected { BOLD } else { Style::new() };
        let mut line = Line::from(vec![
            Span::raw(indent),
            Span::styled(label, name),
            Span::raw(" ".repeat(gap)),
            Span::styled(count, DIM),
        ]);
        if selected {
            line = line.style(Style::new().add_modifier(Modifier::REVERSED));
        }
        frame.render_widget(line, row_at(inner.y + u16::try_from(i - ui.picker_top).unwrap_or(0), inner));
    }
    solid(frame, outer);
}

fn search(ui: &Ui, frame: &mut Frame, area: Rect) {
    let Some(editor) = &ui.search else { return };
    let block = Block::bordered()
        .title(Line::styled(" Search ", BOLD))
        .title_bottom(Line::styled(" Enter to search · Esc to cancel ", DIM));
    let (outer, inner) = panel(frame, area, (56, 4), block);
    let inner = Rect::new(inner.x + 1, inner.y, inner.width.saturating_sub(2), inner.height);
    frame.render_widget(Line::styled("Words to look for, in every feed", DIM), row_at(inner.y, inner));
    let (text, at) = editor.view(usize::from(inner.width), false);
    let style = if editor.is_selected() { Style::new().add_modifier(Modifier::REVERSED) } else { Style::new() };
    frame.render_widget(Line::styled(text, style), row_at(inner.y + 1, inner));
    solid(frame, outer);
    if inner.height > 1 {
        frame.set_cursor_position(Position::new(inner.x + u16::try_from(at).unwrap_or(0), inner.y + 1));
    }
}

fn help(ui: &mut Ui, frame: &mut Frame, area: Rect) {
    let lines = ui.help_lines();
    let keys_width = lines.iter().map(|(keys, _)| keys.width()).max().unwrap_or(0);
    let action_width = lines.iter().map(|(_, action)| action.width()).max().unwrap_or(0);
    let wanted = keys_width + 2 + action_width;
    let room = usize::from(area.width.saturating_sub(8)); // the margin, the box and its padding
    let mut rows = Vec::new();
    for (keys, action) in &lines {
        if wanted <= room {
            let keys = Span::styled(format!("{keys:<keys_width$}  "), ACCENTED);
            rows.push(Row { line: Line::from(vec![keys, Span::raw(action.clone())]), ..Row::default() });
        } else {
            // Too narrow for two columns: what a key does goes under it.
            rows.extend(article::plain(keys, ACCENTED, room));
            let indent = Span::raw("  ");
            rows.extend(article::prefixed(
                article::plain(action, Style::new(), room.saturating_sub(2)),
                &indent,
                &indent,
            ));
        }
    }
    let block = Block::bordered().title(Line::styled(" Keys ", BOLD)).title_bottom(Line::styled(" Esc to close ", DIM));
    let size = (u16::try_from(wanted + 4).unwrap_or(u16::MAX), u16::try_from(rows.len() + 2).unwrap_or(u16::MAX));
    let (outer, inner) = panel(frame, area, size, block);
    let inner = Rect::new(inner.x + 1, inner.y, inner.width.saturating_sub(2), inner.height);
    ui.help_scroll = ui.help_scroll.min(rows.len().saturating_sub(usize::from(inner.height)));
    draw_rows(frame, &rows[ui.help_scroll..], inner);
    solid(frame, outer);
}

fn confirm(ui: &Ui, frame: &mut Frame, area: Rect) {
    let width = area.width.saturating_sub(4).min(56);
    let lines = article::plain(ui.app.confirm_text(), Style::new(), usize::from(width.saturating_sub(4)));
    let block = Block::bordered().title_bottom(Line::styled(" Enter / y to confirm · Esc / n to cancel ", DIM));
    let height = u16::try_from(lines.len() + 2).unwrap_or(u16::MAX);
    let (outer, inner) = panel(frame, area, (width, height), block);
    draw_rows(frame, &lines, Rect::new(inner.x + 1, inner.y, inner.width.saturating_sub(2), inner.height));
    solid(frame, outer);
}

fn viewer(ui: &Ui, frame: &mut Frame, area: Rect) {
    frame.render_widget(Clear, area);
    let Some(v) = ui.app.viewer() else { return };
    let image_area = Rect::new(area.x, area.y, area.width, area.height.saturating_sub(2));
    let note = |frame: &mut Frame, text: &str, style: Style| {
        let y = image_area.y + image_area.height / 2;
        frame.render_widget(Paragraph::new(Line::styled(text.to_string(), style)).centered(), row_at(y, image_area));
    };
    match ui.shot.as_ref().map(|s| &s.picture) {
        Some(Picture::Ready { protocol, .. }) => {
            let size = protocol.size();
            let (width, height) = (size.width.min(image_area.width), size.height.min(image_area.height));
            let at = Rect::new(
                image_area.x + (image_area.width - width) / 2,
                image_area.y + (image_area.height - height) / 2,
                width,
                height,
            );
            frame.render_widget(Image::new(protocol), at);
        }
        Some(Picture::Failed(message)) => note(frame, message, ERROR),
        _ => note(frame, "Loading…", DIM),
    }
    let alt = v.images.get(v.index).map_or("", |i| i.alt.as_str());
    let position = format!("{} / {}", v.index + 1, v.images.len());
    let hints = format!("{position} · ← / → between images · o to open · Esc to close");
    let bottom = area.bottom().saturating_sub(1);
    frame.render_widget(
        Line::styled(fitted(alt, usize::from(area.width)), Style::new()).centered(),
        row_at(bottom.saturating_sub(1), area),
    );
    frame.render_widget(Line::styled(fitted(&hints, usize::from(area.width)), DIM).centered(), row_at(bottom, area));
}

fn toast(ui: &Ui, frame: &mut Frame, area: Rect) {
    let Some(toast) = ui.app.toast() else { return };
    let border = if toast.kind == ToastKind::Error { ERROR } else { ACCENTED };
    let room = usize::from(area.width.saturating_sub(8)).max(1);
    let lines = article::plain(&toast.message, Style::new(), room);
    let widest = lines.iter().map(|l| l.line.width()).max().unwrap_or(0);
    let width = u16::try_from(widest + 4).unwrap_or(u16::MAX).min(area.width);
    let height = u16::try_from(lines.len() + 2).unwrap_or(u16::MAX).min(area.height);
    let y = area.bottom().saturating_sub(height);
    let outer = Rect::new(area.x + (area.width - width) / 2, y, width, height);
    frame.render_widget(Clear, outer);
    let block = Block::bordered().border_type(BorderType::Rounded).border_style(border);
    let inner = block.inner(outer);
    frame.render_widget(block, outer);
    draw_rows(frame, &lines, Rect::new(inner.x + 1, inner.y, inner.width.saturating_sub(2), inner.height));
    solid(frame, outer);
}
