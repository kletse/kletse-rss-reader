//! Draws the state in `Ui`, the way the QML does: a top bar, the reader under it, and the overlays over that.

use super::article::{self, ACCENT, DIM, Row};
use super::ui::{CHOICE, FIELDS, Inline, Picture, SECRET, TABS, Ui};
use crate::app::{Overlay, PickerList, Screen, SettingsSection, ToastKind};
use crate::paging::Mode;
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
    let mut left = vec![Span::raw("☰ "), Span::raw(fitted(label, usize::from(area.width) * 2 / 5))];
    if app.searching() {
        left.extend([Span::raw("  "), Span::styled("SEARCH", ACCENTED)]);
    } else if app.history_badge() {
        left.extend([Span::raw("  "), Span::styled("HISTORY", ACCENTED)]);
    }
    frame.render_widget(Line::from(left), inner);
    frame.render_widget(Line::styled(app.position(), DIM).centered(), inner);
    frame.render_widget(Line::styled("? keys", DIM).right_aligned(), inner);
    frame.render_widget(Line::styled("─".repeat(usize::from(area.width)), DIM), row_at(area.y + 1, area));
}

fn body(ui: &mut Ui, frame: &mut Frame, area: Rect) {
    match ui.app.screen() {
        Screen::Article => reader(ui, frame, area),
        Screen::Settings => settings(ui, frame, area),
        Screen::Loading => message(ui, frame, "", "Loading…"),
        Screen::End => {
            let (title, hint) = (ui.app.end_title(), ui.app.end_hint());
            message(ui, frame, title, &hint);
        }
        Screen::Empty => {}
    }
}

fn message(ui: &Ui, frame: &mut Frame, title: &str, text: &str) {
    let column = ui.column;
    let width = usize::from(column.width);
    let mut rows = vec![Row::default()];
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

fn settings(ui: &mut Ui, frame: &mut Frame, area: Rect) {
    ui.settings_tabs.clear();
    let Some(form) = ui.app.settings() else { return };
    let (problem, error, checking) = (form.problem.clone(), form.error.clone(), form.checking);
    let first_run = ui.app.overlay() == Overlay::Onboarding;
    let Some(fields) = &ui.form else { return };
    let width = area.width.saturating_sub(4).min(64);
    let column = Rect::new(area.x + (area.width - width.min(area.width)) / 2, area.y, width, area.height);
    let w = usize::from(width);

    let mut rows = vec![Row::default()];
    let title = if first_run { "Welcome to Kletse RSS" } else { "Settings" };
    rows.extend(article::plain(title, BOLD, w));
    rows.push(Row::default());
    let intro = if first_run {
        "Connect your Miniflux or Google Reader API server to start reading. Readeck is optional: add it on its tab \
         to save articles for later."
    } else {
        "Changes are checked against your servers before they are saved."
    };
    rows.extend(article::plain(intro, DIM, w));
    rows.push(Row::default());
    if let Some(problem) = &problem {
        rows.extend(article::plain(problem, Style::new().fg(Color::Yellow), w));
        rows.push(Row::default());
    }

    // The tabs, a row of their own: ← → switch there, and Ctrl+Tab anywhere, where the terminal tells it from Tab.
    let tabs_row = rows.len();
    let mut spots = Vec::new();
    let mut line =
        vec![Span::raw("  "), if fields.focus == TABS { Span::styled("› ", ACCENTED) } else { Span::raw("  ") }];
    let mut x = 4;
    for section in SettingsSection::ALL {
        let on = section == fields.section;
        // The same width either way, so the tabs stay put.
        let label = if on { format!("[ {} ]", section.title()) } else { format!("  {}  ", section.title()) };
        let width = label.width();
        spots.push((x, width, section));
        line.push(Span::styled(label, if on { BOLD } else { DIM }));
        line.push(Span::raw(" "));
        x += width + 1;
    }
    line.push(Span::styled("  ← → or Ctrl+Tab", DIM));
    rows.push(Row { line: Line::from(line), ..Row::default() });
    rows.push(Row::default());
    // Under the tabs, whichever is open: a failed save opens the one it's about.
    if !error.is_empty() {
        rows.extend(article::plain(&error, ERROR, w));
        rows.push(Row::default());
    }

    let server = SettingsSection::Server;
    let readeck = SettingsSection::Readeck;
    let sections: [(usize, &str, &str); 4] = [
        (CHOICE, server.title(), server.note()),
        (0, "Miniflux", "required"),
        (2, "Google Reader API", "required"),
        (5, readeck.title(), readeck.note()),
    ];
    const LABELS: [&str; FIELDS] =
        ["Miniflux URL", "API key", "Server URL", "Username", "Password", "Readeck URL", "API token"];
    const PLACEHOLDERS: [&str; FIELDS] = [
        "https://miniflux.example.com",
        "Settings → API Keys → Create a new API key",
        "https://freshrss.example.com/api/greader.php",
        "",
        "FreshRSS: the API password in your profile",
        "https://readeck.example.com",
        "Profile → API Tokens (needs “Bookmarks: write”)",
    ];
    const GREADER_HINT: &str = "FreshRSS: https://your-freshrss/api/greader.php. Miniflux: the server URL, with the Google Reader integration enabled.";
    const READECK_HINT: &str = "Leave both empty to hide the bookmark button.";
    let mut cursor = None;
    let shown: Vec<usize> = fields.shown().into_iter().filter(|&i| i != TABS).collect();
    for (n, &i) in shown.iter().enumerate() {
        if let Some((_, title, note)) = sections.iter().find(|(first, ..)| *first == i) {
            let line = Line::from(vec![Span::styled(*title, ACCENTED), Span::styled(format!(" · {note}"), DIM)]);
            rows.push(Row { line, ..Row::default() });
        }
        let focused = i == fields.focus;
        if i == CHOICE {
            let mark = if focused { Span::styled("› ", ACCENTED) } else { Span::raw("  ") };
            let option = |on: bool, name: &'static str| {
                let style = if on { BOLD } else { DIM };
                Span::styled(format!("{} {name}", if on { "●" } else { "○" }), style)
            };
            let greader = fields.server == crate::config::Server::GReader;
            rows.push(Row {
                line: Line::from(vec![
                    Span::raw("  "),
                    mark,
                    option(!greader, "Miniflux"),
                    Span::raw("   "),
                    option(greader, "Google Reader API (FreshRSS, …)"),
                ]),
                ..Row::default()
            });
            rows.push(Row::default());
            continue;
        }
        rows.push(Row {
            line: Line::from(Span::styled(format!("  {}", LABELS[i]), if focused { BOLD } else { DIM })),
            ..Row::default()
        });
        let (text, at) = fields.fields[i].view(w.saturating_sub(4), SECRET[i]);
        let mark = if focused { Span::styled("› ", ACCENTED) } else { Span::raw("  ") };
        let value = if text.is_empty() {
            Span::styled(fitted(PLACEHOLDERS[i], w.saturating_sub(4)), DIM)
        } else {
            Span::raw(text)
        };
        if focused {
            cursor = Some((rows.len(), 4 + at));
        }
        rows.push(Row { line: Line::from(vec![Span::raw("  "), mark, value]), ..Row::default() });
        let hint = match i {
            2 => Some(GREADER_HINT),
            6 => Some(READECK_HINT),
            _ => None,
        };
        if let Some(hint) = hint {
            let indent = Span::raw("    ");
            rows.extend(article::prefixed(article::plain(hint, DIM, w.saturating_sub(4)), &indent, &indent));
        }
        if n + 1 < shown.len() {
            rows.push(Row::default());
        }
    }
    rows.push(Row::default());
    let action = if first_run { "Enter to connect" } else { "Enter to save · Esc to cancel" };
    let hint = if checking { "Checking…".to_string() } else { format!("{action} · Tab for the next field") };
    rows.extend(article::plain(&hint, DIM, w));
    draw_rows(frame, &rows, column);
    if let Ok(y) = u16::try_from(tabs_row)
        && y < column.height
    {
        ui.settings_tabs = spots
            .into_iter()
            .filter_map(|(x, width, section)| {
                let (x, width) = (u16::try_from(x).ok()?, u16::try_from(width).ok()?);
                Some((Rect::new(column.x + x, column.y + y, width, 1).intersection(column), section))
            })
            .collect();
    }
    if let Some((row, x)) = cursor
        && !checking
        && let (Ok(row), Ok(x)) = (u16::try_from(row), u16::try_from(x))
        && column.y + row < column.bottom()
    {
        frame.set_cursor_position(Position::new(column.x + x, column.y + row));
    }
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
        PickerList::Ready(_) if ui.app.fetches() => {
            Some(("Nothing unread. Press r to refresh, or h for history.".to_string(), DIM))
        }
        PickerList::Ready(_) => Some(("Nothing unread. Press h for history.".to_string(), DIM)),
    };
    let order = if ui.app.newest_first() { "Newest first" } else { "Oldest first" };
    let swap = if history { "h unread" } else { "h history" };
    let refresh = if ui.app.fetches() { " · r refresh" } else { "" };
    let mut block = Block::bordered()
        .title(Line::styled(if history { " History " } else { " Feeds " }, BOLD))
        .title_bottom(Line::styled(format!(" {swap} · / search{refresh} · , settings · ? keys "), DIM));
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
