//! An article as rows of terminal cells: the converted blocks (content/native.rs), wrapped to a width.
//!
//! Colours are the terminal's own, so the reader looks like everything else in it and follows its theme, on
//! Omarchy and on macOS alike, on dark and light backgrounds. `Palette` only speaks hex, so `palette()` gives each
//! ANSI colour a code that `ink` turns back when it comes out of the converter in a run.

use crate::content::native::{Cell, NativeBlock, Run, Script, Style as RunStyle};
use crate::theme::Palette;
use ratatui::layout::Size;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

pub const ACCENT: Color = Color::Blue;
/// Less to look at: faint rather than a grey that may not suit the theme.
pub const DIM: Style = Style::new().add_modifier(Modifier::DIM);
const CODE: Style = Style::new().fg(Color::Cyan);
const TAB: &str = "    ";

const INK_DIM: &str = "#000001";
const INK_RED: &str = "#000002";
const INK_GREEN: &str = "#000003";
const INK_YELLOW: &str = "#000004";
const INK_BLUE: &str = "#000005";
const INK_MAGENTA: &str = "#000006";
const INK_CYAN: &str = "#000007";
const INK_ORANGE: &str = "#000008";
const INK_PLAIN: &str = "#000009";

/// The palette the converter gets: not colours, but codes for `ink`.
pub fn palette() -> Palette {
    let code = |hex: &str| hex.to_string();
    Palette {
        fg: code(INK_PLAIN),
        fg_bright: code(INK_PLAIN),
        fg_dim: code(INK_DIM),
        muted: code(INK_DIM),
        accent: code(INK_BLUE),
        blue: code(INK_BLUE),
        red: code(INK_RED),
        bright_red: code(INK_RED),
        green: code(INK_GREEN),
        yellow: code(INK_YELLOW),
        magenta: code(INK_MAGENTA),
        cyan: code(INK_CYAN),
        orange: code(INK_ORANGE),
        ..Palette::default()
    }
}

fn ink(code: &str) -> Style {
    let fg = |color| Style::new().fg(color);
    match code {
        INK_DIM => DIM,
        INK_RED => fg(Color::Red),
        INK_GREEN => fg(Color::Green),
        INK_YELLOW => fg(Color::Yellow),
        INK_BLUE => fg(ACCENT),
        INK_MAGENTA => fg(Color::Magenta),
        INK_CYAN => fg(Color::Cyan),
        INK_ORANGE => fg(Color::LightRed), // ANSI has no orange
        _ => Style::new(),
    }
}

/// What a click on part of a row does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    Link(String),
    Image(String),
}

/// The size an image gets in `width` columns, once it's known; until then it's a line of text.
pub type SizeOf<'a> = &'a dyn Fn(&str, usize) -> Option<Size>;

/// A row of a picture in the article.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Slice {
    pub src: String,
    /// Where the picture starts in the row.
    pub column: u16,
    /// Which of its rows this is.
    pub row: u16,
    pub size: Size,
}

/// One row on screen, with where you can click in it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Row {
    pub line: Line<'static>,
    pub slice: Option<Slice>,
    /// Stands for an image, as a picture or as the line before it loads: these come and go as images arrive.
    pub image: bool,
    /// From column `.0` up to `.1`, a click does `.2`.
    pub targets: Vec<(u16, u16, Target)>,
}

impl Row {
    fn styled(text: impl Into<String>, style: Style) -> Self {
        Self { line: Line::from(Span::styled(text.into(), style)), ..Self::default() }
    }

    fn width(&self) -> usize {
        self.line.width()
    }

    /// `text` in front of the row.
    fn after(mut self, prefix: &Span<'static>) -> Self {
        let shift = u16::try_from(prefix.width()).unwrap_or(0);
        self.line.spans.insert(0, prefix.clone());
        if let Some(slice) = &mut self.slice {
            slice.column += shift;
        }
        for (start, end, _) in &mut self.targets {
            *start += shift;
            *end += shift;
        }
        self
    }

    /// `other` at the end of the row.
    fn extend(&mut self, other: Row) {
        let shift = u16::try_from(self.width()).unwrap_or(0);
        self.line.spans.extend(other.line.spans);
        self.targets.extend(other.targets.into_iter().map(|(start, end, t)| (start + shift, end + shift, t)));
    }

    fn pad_to(&mut self, width: usize) {
        let missing = width.saturating_sub(self.width());
        if missing > 0 {
            self.line.spans.push(Span::raw(" ".repeat(missing)));
        }
    }

    /// Adds text at the end of the row, in one span with what's before it if the style is the same.
    fn push(&mut self, text: &str, style: Style, target: Option<&Target>) {
        let start = u16::try_from(self.width()).unwrap_or(u16::MAX);
        let width = u16::try_from(text.width()).unwrap_or(0);
        match self.line.spans.last_mut() {
            Some(span) if span.style == style => span.content.to_mut().push_str(text),
            _ => self.line.spans.push(Span::styled(text.to_string(), style)),
        }
        let Some(target) = target else { return };
        match self.targets.last_mut() {
            Some((_, end, last)) if *end == start && last == target => *end += width,
            _ => self.targets.push((start, start + width, target.clone())),
        }
    }
}

/// Some text in one style, maybe clickable.
struct Piece {
    text: String,
    style: Style,
    target: Option<Target>,
}

fn superscript(c: char) -> char {
    match c {
        '0' => '⁰',
        '1' => '¹',
        '2' => '²',
        '3' => '³',
        '4'..='9' => char::from_u32(0x2070 + u32::from(c) - u32::from('0')).unwrap_or(c),
        '+' => '⁺',
        '-' => '⁻',
        c => c,
    }
}

fn subscript(c: char) -> char {
    match c {
        '0'..='9' => char::from_u32(0x2080 + u32::from(c) - u32::from('0')).unwrap_or(c),
        '+' => '₊',
        '-' => '₋',
        c => c,
    }
}

fn run_style(style: &RunStyle, base: Style) -> Style {
    let mut s = base;
    if let Some(color) = &style.color {
        s = s.patch(ink(color));
    }
    for (on, modifier) in [
        (style.bold, Modifier::BOLD),
        (style.italic, Modifier::ITALIC),
        (style.underline, Modifier::UNDERLINED),
        (style.strike, Modifier::CROSSED_OUT),
        (style.small, Modifier::DIM),
        (style.link.is_some(), Modifier::UNDERLINED),
        // Marked text; a cell has no tint that suits every theme.
        (style.background.is_some() && !style.code, Modifier::REVERSED),
    ] {
        if on {
            s = s.add_modifier(modifier);
        }
    }
    if style.code { s.patch(CODE) } else { s }
}

fn pieces(runs: &[Run], base: Style) -> Vec<Piece> {
    runs.iter()
        .map(|run| {
            let text = match run.style.script {
                Some(Script::Sup) => run.text.chars().map(superscript).collect(),
                Some(Script::Sub) => run.text.chars().map(subscript).collect(),
                // The converter pads inline code for the tint behind it, which isn't here.
                None if run.style.code => run.text.trim_matches('\u{a0}').to_string(),
                None => run.text.clone(),
            };
            let target = run.style.link.clone().map(Target::Link);
            Piece { text, style: run_style(&run.style, base), target }
        })
        .collect()
}

/// Rows of text being filled, breaking at spaces, or anywhere in a word too long for a row.
struct Wrap {
    width: usize,
    rows: Vec<Row>,
    row: Row,
    /// The word being collected, kept whole across style changes.
    word: Vec<Piece>,
    /// A space before that word, in the style it was typed in.
    space: Option<(Style, Option<Target>)>,
}

impl Wrap {
    fn new(width: usize) -> Self {
        Self { width: width.max(1), rows: Vec::new(), row: Row::default(), word: Vec::new(), space: None }
    }

    fn line_break(&mut self) {
        self.rows.push(std::mem::take(&mut self.row));
        self.space = None;
    }

    fn word_char(&mut self, c: char, piece: &Piece) {
        match self.word.last_mut() {
            Some(last) if last.style == piece.style && last.target == piece.target => last.text.push(c),
            _ => self.word.push(Piece { text: c.to_string(), style: piece.style, target: piece.target.clone() }),
        }
    }

    /// Puts the word on this row, or the next, or over several if it's longer than a row.
    fn end_word(&mut self) {
        let word = std::mem::take(&mut self.word);
        if word.is_empty() {
            return;
        }
        let width: usize = word.iter().map(|p| p.text.width()).sum();
        let used = self.row.width();
        if used > 0 && used + usize::from(self.space.is_some()) + width > self.width {
            self.line_break();
        }
        if let Some((style, target)) = self.space.take()
            && self.row.width() > 0
        {
            self.row.push(" ", style, target.as_ref());
        }
        for piece in word {
            self.hard_wrapped(&piece.text, piece.style, piece.target.as_ref());
        }
    }

    /// Text that may be cut anywhere.
    fn hard_wrapped(&mut self, text: &str, style: Style, target: Option<&Target>) {
        let mut buf = [0; 4];
        for c in text.chars() {
            if self.row.width() + c.width().unwrap_or(0) > self.width {
                self.line_break();
            }
            self.row.push(c.encode_utf8(&mut buf), style, target);
        }
    }

    fn text(&mut self, piece: &Piece) {
        for c in piece.text.chars() {
            match c {
                '\n' => {
                    self.end_word();
                    self.line_break();
                }
                ' ' | '\t' => {
                    self.end_word();
                    self.space = Some((piece.style, piece.target.clone()));
                }
                c if c.is_control() => {}
                c => self.word_char(c, piece),
            }
        }
    }

    /// Code: every space stays, and lines are only cut where they don't fit.
    fn preformatted(&mut self, piece: &Piece) {
        for line in piece.text.split_inclusive('\n') {
            let (text, ends) = line.strip_suffix('\n').map_or((line, false), |text| (text, true));
            let text = text.replace('\t', TAB).replace(char::is_control, "");
            self.hard_wrapped(&text, piece.style, piece.target.as_ref());
            if ends {
                self.line_break();
            }
        }
    }

    fn finish(mut self) -> Vec<Row> {
        self.end_word();
        if !self.row.line.spans.is_empty() {
            self.rows.push(self.row);
        }
        self.rows
    }
}

fn wrap(runs: &[Run], base: Style, width: usize) -> Vec<Row> {
    let mut w = Wrap::new(width);
    pieces(runs, base).iter().for_each(|p| w.text(p));
    w.finish()
}

/// Plain text wrapped to `width`, for what isn't an article: messages, the help.
pub fn plain(text: &str, style: Style, width: usize) -> Vec<Row> {
    let mut w = Wrap::new(width);
    w.text(&Piece { text: text.to_string(), style, target: None });
    w.finish()
}

pub fn prefixed(rows: Vec<Row>, first: &Span<'static>, rest: &Span<'static>) -> Vec<Row> {
    rows.into_iter().enumerate().map(|(i, row)| row.after(if i == 0 { first } else { rest })).collect()
}

/// Rows set aside for a picture, centred as in the window; `draw` puts it there.
fn picture(src: &str, size: Size, width: usize) -> Vec<Row> {
    let column = u16::try_from(width).unwrap_or(u16::MAX).saturating_sub(size.width) / 2;
    let clickable = (column, column + size.width, Target::Image(src.to_string()));
    (0..size.height)
        .map(|row| Row {
            slice: Some(Slice { src: src.to_string(), column, row, size }),
            image: true,
            targets: vec![clickable.clone()],
            ..Row::default()
        })
        .collect()
}

/// The line that stands for an image that isn't here (yet).
fn placeholder(src: &str, alt: &str, width: usize) -> Vec<Row> {
    let mark = Span::styled("▣ ", Style::new().fg(ACCENT));
    let target = Target::Image(src.to_string());
    let label = if alt.trim().is_empty() { "image" } else { alt.trim() };
    let piece = Piece { text: label.to_string(), style: DIM, target: Some(target) };
    let mut w = Wrap::new(width.saturating_sub(2));
    w.text(&piece);
    let rows = w.finish().into_iter().map(|row| Row { image: true, ..row }).collect();
    prefixed(rows, &mark, &Span::raw("  "))
}

fn image(src: &str, alt: &str, caption: Option<&[Run]>, width: usize, size_of: SizeOf<'_>) -> Vec<Row> {
    let mut rows = match size_of(src, width).filter(|size| size.height > 0 && size.width > 0) {
        Some(size) => picture(src, size, width),
        None => placeholder(src, alt, width),
    };
    if let Some(caption) = caption {
        rows.extend(wrap(caption, DIM.add_modifier(Modifier::ITALIC), width));
    }
    rows
}

fn code(runs: &[Run], lang: Option<&str>, width: usize) -> Vec<Row> {
    let bar = Span::styled("┃ ", DIM);
    let mut w = Wrap::new(width.saturating_sub(2));
    pieces(runs, Style::new()).iter().for_each(|p| w.preformatted(p));
    let mut rows = prefixed(w.finish(), &bar, &bar);
    while rows.last().is_some_and(|row| row.line.width() <= bar.width()) {
        rows.pop();
    }
    if let Some(lang) = lang.filter(|l| !l.is_empty()) {
        rows.insert(0, Row::styled(lang, DIM).after(&bar));
    }
    rows
}

fn list(ordered: bool, start: i64, items: &[Vec<NativeBlock>], width: usize, size_of: SizeOf<'_>) -> Vec<Row> {
    let number = |i: usize| start.saturating_add(i64::try_from(i).unwrap_or(0));
    let marker = |i: usize| if ordered { format!("{}. ", number(i)) } else { "• ".to_string() };
    let indent = (0..items.len()).map(|i| marker(i).width()).max().unwrap_or(2);
    let inner = width.saturating_sub(indent);
    let mut rows = Vec::new();
    for (i, item) in items.iter().enumerate() {
        let first = Span::styled(format!("{:>indent$}", marker(i)), DIM);
        rows.extend(prefixed(stacked(item, inner, true, size_of), &first, &Span::raw(" ".repeat(indent))));
    }
    rows
}

/// A cell's text on one line, as wide as it would like to be.
fn natural_width(cell: &Cell) -> usize {
    cell.runs.iter().map(|r| r.text.width()).sum()
}

/// How wide each column gets: what it wants if they all fit; if not, the narrow ones keep theirs and the wide ones
/// share what's left.
fn fit_columns(natural: &[usize], width: usize) -> Vec<usize> {
    let mut left = width;
    let mut columns = vec![0; natural.len()];
    let mut by_width: Vec<usize> = (0..natural.len()).collect();
    by_width.sort_by_key(|&i| natural[i]);
    for (done, &i) in by_width.iter().enumerate() {
        let share = left / (natural.len() - done);
        columns[i] = natural[i].min(share).max(1);
        left = left.saturating_sub(columns[i]);
    }
    columns
}

fn table(rows: &[Vec<Cell>], width: usize) -> Vec<Row> {
    const GAP: &str = "  ";
    let count = rows.iter().map(|r| r.iter().map(|c| c.colspan.max(1) as usize).sum()).max().unwrap_or(0);
    let room = width.saturating_sub((count.max(1) - 1) * GAP.len());
    if count == 0 || room < count * 3 {
        // Too narrow for columns: each row as a line of its own.
        return rows
            .iter()
            .flat_map(|row| {
                let cells: Vec<Run> = row
                    .iter()
                    .flat_map(|c| {
                        c.runs.iter().cloned().chain(std::iter::once(Run { text: " · ".into(), ..Run::default() }))
                    })
                    .collect();
                wrap(&cells[..cells.len().saturating_sub(1)], Style::new(), width)
            })
            .collect();
    }
    let mut natural = vec![1; count];
    for row in rows {
        let mut column = 0;
        for cell in row {
            let span = cell.colspan.max(1) as usize;
            if span == 1 && column < count {
                natural[column] = natural[column].max(natural_width(cell));
            }
            column += span;
        }
    }
    // A cell over several columns: the last of them makes up for what it lacks.
    for row in rows {
        let mut column = 0;
        for cell in row {
            let span = (cell.colspan.max(1) as usize).min(count - column.min(count));
            if span > 1 {
                let spanned = &natural[column..column + span];
                let room = spanned.iter().sum::<usize>() + (span - 1) * GAP.len();
                natural[column + span - 1] += natural_width(cell).saturating_sub(room);
            }
            column += span;
        }
    }
    let columns = fit_columns(&natural, room);
    let mut out = Vec::new();
    for row in rows {
        let mut laid: Vec<(usize, Vec<Row>)> = Vec::new();
        let mut column = 0;
        for cell in row {
            let span = (cell.colspan.max(1) as usize).min(count - column.min(count));
            let spanned = columns[column.min(count)..(column + span).min(count)].iter().sum::<usize>()
                + span.saturating_sub(1) * GAP.len();
            let base = if cell.header { Style::new().add_modifier(Modifier::BOLD) } else { Style::new() };
            laid.push((spanned, wrap(&cell.runs, base, spanned)));
            column += span;
        }
        let height = laid.iter().map(|(_, rows)| rows.len()).max().unwrap_or(0);
        for i in 0..height {
            let mut line = Row::default();
            for (n, (cell_width, rows)) in laid.iter().enumerate() {
                if n > 0 {
                    line.push(GAP, Style::new(), None);
                }
                let mut part = rows.get(i).cloned().unwrap_or_default();
                part.pad_to(*cell_width);
                line.extend(part);
            }
            out.push(line);
        }
        if !row.is_empty() && row.iter().all(|c| c.header) {
            out.push(Row::styled("─".repeat(width.min(columns.iter().sum::<usize>() + (count - 1) * GAP.len())), DIM));
        }
    }
    out
}

fn block(b: &NativeBlock, width: usize, size_of: SizeOf<'_>) -> Vec<Row> {
    match b {
        NativeBlock::Paragraph { runs } => wrap(runs, Style::new(), width),
        NativeBlock::Heading { level, runs } => {
            let style = Style::new().add_modifier(Modifier::BOLD);
            wrap(runs, if *level <= 2 { style.fg(ACCENT) } else { style }, width)
        }
        NativeBlock::Image { src, alt, caption, .. } => image(src, alt, caption.as_deref(), width, size_of),
        NativeBlock::Caption { runs } => wrap(runs, DIM.add_modifier(Modifier::ITALIC), width),
        NativeBlock::Code { runs, lang } => code(runs, lang.as_deref(), width),
        NativeBlock::Quote { blocks: inner } => {
            let bar = Span::styled("│ ", Style::new().fg(ACCENT));
            prefixed(stacked(inner, width.saturating_sub(2), false, size_of), &bar, &bar)
        }
        NativeBlock::List { ordered, start, items } => list(*ordered, *start, items, width, size_of),
        NativeBlock::Table { rows } => table(rows, width),
        NativeBlock::Rule => vec![Row::styled("─".repeat(width), DIM)],
        NativeBlock::Embed { href, label } => {
            let mut w = Wrap::new(width.saturating_sub(2));
            let style = Style::new().fg(ACCENT).add_modifier(Modifier::UNDERLINED);
            w.text(&Piece { text: label.clone(), style, target: Some(Target::Link(href.clone())) });
            prefixed(w.finish(), &Span::styled("▶ ", Style::new().fg(ACCENT)), &Span::raw("  "))
        }
    }
}

/// `blocks` one under the other, an empty row between them. In a list item, `tight`, a list follows on directly.
fn stacked(blocks: &[NativeBlock], width: usize, tight: bool, size_of: SizeOf<'_>) -> Vec<Row> {
    let mut rows = Vec::new();
    for b in blocks {
        let laid = block(b, width, size_of);
        let follows_on = tight && matches!(b, NativeBlock::List { .. });
        if !rows.is_empty() && !laid.is_empty() && !follows_on {
            rows.push(Row::default());
        }
        rows.extend(laid);
    }
    rows
}

pub fn blocks(blocks: &[NativeBlock], width: usize, size_of: SizeOf<'_>) -> Vec<Row> {
    stacked(blocks, width, false, size_of)
}

/// What the article's header shows.
#[derive(Debug)]
pub struct Header<'a> {
    pub title: &'a str,
    pub url: &'a str,
    pub feed: &'a str,
    pub author: &'a str,
    pub published: &'a str,
    pub reading_time: i64,
    /// "read" or "unread".
    pub status: &'a str,
}

/// example.com, from `https://www.example.com/a`.
fn host(url: &str) -> String {
    let parsed = url::Url::parse(url).ok();
    let host = parsed.as_ref().and_then(url::Url::host_str).unwrap_or(url);
    host.trim_start_matches("www.").to_string()
}

/// The article's header: feed, author, date and status; the title, which opens the original; the site.
pub fn header(h: &Header<'_>, width: usize) -> Vec<Row> {
    let minutes = if h.reading_time > 0 { format!("{} min", h.reading_time) } else { String::new() };
    let date = crate::dates::display(h.published).unwrap_or_default();
    let meta: Vec<&str> = [h.feed, h.author, &date, &minutes].into_iter().filter(|s| !s.is_empty()).collect();
    let status = h.status.to_uppercase();
    let unread = h.status == "unread";
    let mark = Row::styled(status.clone(), if unread { Style::new().fg(ACCENT) } else { DIM });

    let mut rows = plain(&meta.join(" · "), DIM, width.saturating_sub(status.width() + 1));
    match rows.first_mut() {
        Some(first) => {
            first.pad_to(width.saturating_sub(status.width()));
            first.extend(mark);
        }
        None => rows.push(mark),
    }
    rows.push(Row::default());
    let title = Piece {
        text: h.title.to_string(),
        style: Style::new().add_modifier(Modifier::BOLD),
        target: Some(Target::Link(h.url.to_string())),
    };
    let mut w = Wrap::new(width);
    w.text(&title);
    rows.extend(w.finish());
    rows.extend(plain(&host(h.url), DIM, width));
    rows.push(Row::default());
    rows
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::content::{convert, native};

    fn text(rows: &[Row]) -> Vec<String> {
        rows.iter().map(|r| r.line.to_string()).collect()
    }

    fn laid(html: &str, width: usize) -> Vec<Row> {
        let rendered = convert::convert(html, "https://example.com/a/", &palette());
        blocks(&native::native(&rendered.blocks), width, &|_, _| None)
    }

    fn lines(html: &str, width: usize) -> Vec<String> {
        text(&laid(html, width))
    }

    #[test]
    fn words_wrap_at_spaces_and_long_ones_are_cut() {
        assert_eq!(lines("<p>one two three four</p>", 9), ["one two", "three", "four"]);
        assert_eq!(lines("<p>abcdefghij kl</p>", 4), ["abcd", "efgh", "ij", "kl"]);
    }

    #[test]
    fn a_word_stays_whole_across_a_style_change() {
        assert_eq!(lines("<p>one two<b>three</b></p>", 9), ["one", "twothree"]);
    }

    #[test]
    fn wide_characters_count_double() {
        assert_eq!(lines("<p>日本語の文章</p>", 6), ["日本語", "の文章"]);
    }

    #[test]
    fn line_breaks_and_blocks_keep_their_rows() {
        assert_eq!(lines("<p>one<br>two</p><p>three</p>", 20), ["one", "two", "", "three"]);
    }

    #[test]
    fn links_are_styled_and_clickable_across_rows() {
        let rows = laid("<p>go <a href='/x'>read this one</a> now</p>", 11);
        assert_eq!(text(&rows), ["go read", "this one", "now"]);
        let link = Target::Link("https://example.com/x".into());
        assert_eq!(rows[0].targets, vec![(3, 7, link.clone())]);
        assert_eq!(rows[1].targets, vec![(0, 8, link)]);
        let span = rows[1].line.spans.first().unwrap();
        assert!(span.style.add_modifier.contains(Modifier::UNDERLINED));
    }

    #[test]
    fn inline_code_loses_its_padding() {
        let rows = laid("<p>call <code>f()</code></p>", 20);
        assert_eq!(text(&rows), ["call f()"]);
        assert_eq!(rows[0].line.spans[1].style.fg, CODE.fg);
    }

    #[test]
    fn code_keeps_its_indentation_and_is_coloured_from_the_terminal() {
        let html = "<pre><code class='language-rust'>fn main() {\n    let x = 1;\n}\n</code></pre>";
        let rows = laid(html, 40);
        assert_eq!(text(&rows), ["┃ Rust", "┃ fn main() {", "┃     let x = 1;", "┃ }"]);
        let colours: Vec<_> = rows[1].line.spans.iter().filter_map(|s| s.style.fg).collect();
        assert!(colours.contains(&Color::Magenta), "{colours:?}");
    }

    #[test]
    fn long_code_lines_are_cut_rather_than_lost() {
        assert_eq!(lines("<pre>abcdefgh</pre>", 7), ["┃ abcde", "┃ fgh"]);
    }

    #[test]
    fn quotes_and_lists_nest() {
        assert_eq!(lines("<blockquote><p>one two</p></blockquote>", 6), ["│ one", "│ two"]);
        assert_eq!(
            lines("<ul><li>one two</li><li>three</li></ul>", 6),
            ["• one", "  two", "• thre", "  e"].map(String::from)
        );
        let nested = "<ul><li>a<ul><li>b</li></ul></li></ul>";
        assert_eq!(lines(nested, 20), ["• a", "  • b"]);
    }

    #[test]
    fn ordered_lists_start_where_they_say_and_line_up() {
        let items: String = (0..2).map(|i| format!("<li>item {i}</li>")).collect();
        assert_eq!(lines(&format!("<ol start='9'>{items}</ol>"), 20), [" 9. item 0", "10. item 1"]);
    }

    #[test]
    fn images_are_a_line_to_click() {
        let rows = laid("<p><img src='pic.png' alt='A cat'></p>", 20);
        assert_eq!(text(&rows), ["▣ A cat"]);
        assert_eq!(rows[0].targets, vec![(2, 7, Target::Image("https://example.com/a/pic.png".into()))]);
        assert_eq!(lines("<p><img src='pic.png'></p>", 20), ["▣ image"]);
    }

    #[test]
    fn a_known_image_gets_rows_centred_in_the_column_and_stays_clickable() {
        let rendered = convert::convert("<p>a</p><img src='p.png' alt='A cat'><p>b</p>", "https://e.com/", &palette());
        let blocks = native::native(&rendered.blocks);
        let rows = super::blocks(&blocks, 20, &|src, width| {
            assert_eq!((src, width), ("https://e.com/p.png", 20));
            Some(Size::new(10, 3))
        });
        assert_eq!(text(&rows), ["a", "", "", "", "", "", "b"]);
        let slices: Vec<_> = rows.iter().filter_map(|r| r.slice.as_ref()).collect();
        assert_eq!(slices.iter().map(|s| (s.column, s.row)).collect::<Vec<_>>(), [(5, 0), (5, 1), (5, 2)]);
        assert_eq!(rows[3].targets, vec![(5, 15, Target::Image("https://e.com/p.png".into()))]);
    }

    #[test]
    fn an_image_in_a_quote_starts_after_the_bar() {
        let rendered = convert::convert("<blockquote><img src='p.png'></blockquote>", "https://e.com/", &palette());
        let rows = blocks(&native::native(&rendered.blocks), 12, &|_, width| {
            Some(Size::new(u16::try_from(width).unwrap(), 2))
        });
        assert_eq!(rows[0].slice.as_ref().unwrap().column, 2);
        assert_eq!(rows[1].line.to_string(), "│ ");
    }

    #[test]
    fn embeds_are_links() {
        let rows = laid("<iframe src='https://video.example.com/1'></iframe>", 40);
        assert!(text(&rows)[0].starts_with("▶ "));
        assert_eq!(rows[0].targets[0].2, Target::Link("https://video.example.com/1".into()));
    }

    #[test]
    fn tables_line_up_and_rule_off_their_header() {
        let html = "<table><tr><th>Name</th><th>N</th></tr><tr><td>a</td><td>12</td></tr></table>";
        assert_eq!(lines(html, 40), ["Name  N ", "────────", "a     12"]);
    }

    #[test]
    fn table_cells_wrap_when_the_table_is_too_wide() {
        let html = "<table><tr><td>one two three</td><td>x</td></tr></table>";
        assert_eq!(lines(html, 10), ["one two  x", "three     "]);
    }

    #[test]
    fn a_cell_can_span_columns() {
        let html = "<table><tr><td colspan='2'>wide cell</td></tr><tr><td>a</td><td>b</td></tr></table>";
        assert_eq!(lines(html, 20), ["wide cell", "a  b     "]);
    }

    #[test]
    fn a_table_with_no_room_for_columns_becomes_lines() {
        let html = "<table><tr><td>a</td><td>b</td><td>c</td><td>d</td></tr></table>";
        assert_eq!(lines(html, 8), ["a · b ·", "c · d"]);
    }

    #[test]
    fn columns_share_what_there_is() {
        assert_eq!(fit_columns(&[2, 3], 10), [2, 3]);
        assert_eq!(fit_columns(&[2, 30, 30], 20), [2, 9, 9]);
        assert_eq!(fit_columns(&[9, 9], 2), [1, 1]);
    }

    #[test]
    fn scripts_use_the_small_digits() {
        assert_eq!(lines("<p>x<sup>23</sup> H<sub>2</sub>O</p>", 20), ["x²³ H₂O"]);
    }

    #[test]
    fn nothing_is_wider_than_asked() {
        let html = crate::demo::content(3);
        for width in [12, 20, 33, 80] {
            for row in laid(&html, width) {
                assert!(row.width() <= width, "{width}: {:?}", row.line.to_string());
            }
        }
    }

    #[test]
    fn the_header_has_the_status_on_the_right_and_the_title_opens_the_article() {
        let h = Header {
            title: "A title",
            url: "https://www.example.com/post",
            feed: "Feed",
            author: "Someone",
            published: "2026-10-24T08:12:00+02:00",
            reading_time: 4,
            status: "unread",
        };
        let rows = header(&h, 64);
        assert_eq!(rows[0].line.to_string(), format!("{:<58}UNREAD", "Feed · Someone · 24 Oct 2026, 08:12 · 4 min"));
        assert_eq!(rows[2].line.to_string(), "A title");
        assert_eq!(rows[2].targets[0].2, Target::Link(h.url.into()));
        assert_eq!(rows[3].line.to_string(), "example.com");
    }
}
