//! The terminal version on a fake screen: real keys and mouse events, the demo's data, and what ends up in the
//! cells. What keys mean is tested in app_tests.rs; here it's what the terminal adds.

#![allow(clippy::unwrap_used)]

use super::draw;
use super::images::{self, Job};
use super::ui::{Inline, Ui};
use crate::app::{Effect, Outcome, Overlay, Screen, Status};
use crate::backend::Backend;
use crate::prefs::Prefs;
use base64::Engine as _;
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::{Rect, Size};
use ratatui::style::Color;
use ratatui_image::picker::{Picker, ProtocolType};
use std::sync::Arc;

struct Screenful {
    ui: Ui,
    terminal: Terminal<TestBackend>,
    backend: Arc<Backend>,
    runtime: tokio::runtime::Runtime,
    /// Links that would have opened in the browser.
    opened: Vec<String>,
    /// Takes the place of the first article's text.
    content: Option<String>,
}

impl Screenful {
    fn new(width: u16, height: u16) -> Self {
        Self::starting(width, height, Status { configured: true, readeck: true, fetches: true })
    }

    fn starting(width: u16, height: u16, status: Status) -> Self {
        let mut ui = Ui::new(Prefs::default(), true, Picker::halfblocks());
        ui.start(status);
        let mut screen = Self {
            ui,
            terminal: Terminal::new(TestBackend::new(width, height)).unwrap(),
            backend: Arc::new(Backend::new(true)),
            runtime: tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap(),
            opened: Vec::new(),
            content: None,
        };
        screen.settle();
        screen
    }

    /// The article that opens next has this as its text.
    fn with_content(mut self, html: &str) -> Self {
        self.content = Some(html.to_string());
        self
    }

    /// Draws, and runs what comes of it, until nothing more does.
    fn settle(&mut self) {
        loop {
            let size = self.terminal.size().unwrap();
            self.ui.prepare(size.into());
            self.terminal.draw(|frame| draw::draw(&mut self.ui, frame)).unwrap();
            let (effects, jobs) = (self.ui.take_effects(), self.ui.take_jobs());
            if effects.is_empty() && jobs.is_empty() {
                return;
            }
            for effect in effects {
                if let Effect::OpenInBrowser { url, .. } = &effect {
                    self.opened.push(url.clone());
                } else if let Some(mut outcome) = self.runtime.block_on(self.backend.run(effect)) {
                    if let (Outcome::Opened { result: Ok((page, _)), .. }, Some(html)) = (&mut outcome, &self.content) {
                        page.entries[0].content.clone_from(html);
                    }
                    self.ui.outcome(outcome);
                }
            }
            for job in jobs {
                let answer = self.runtime.block_on(images::run(job, Arc::clone(&self.backend), Picker::halfblocks()));
                self.ui.image(answer);
            }
        }
    }

    fn key(&mut self, code: KeyCode) -> &mut Self {
        self.event(KeyEvent::from(code))
    }

    fn ctrl(&mut self, c: char) -> &mut Self {
        self.event(KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL))
    }

    fn event(&mut self, event: KeyEvent) -> &mut Self {
        self.ui.key(event);
        self.settle();
        self
    }

    fn keys(&mut self, keys: &str) -> &mut Self {
        for c in keys.chars() {
            let modifiers = if c.is_uppercase() { KeyModifiers::SHIFT } else { KeyModifiers::NONE };
            self.event(KeyEvent::new(KeyCode::Char(c), modifiers));
        }
        self
    }

    fn mouse(&mut self, kind: MouseEventKind, column: u16, row: u16) -> &mut Self {
        self.ui.mouse(MouseEvent { kind, column, row, modifiers: KeyModifiers::NONE });
        self.settle();
        self
    }

    fn text(&self) -> String {
        let buffer = self.terminal.backend().buffer();
        let rows = buffer.content.chunks(usize::from(buffer.area.width));
        rows.map(|row| row.iter().map(ratatui::buffer::Cell::symbol).collect::<String>()).collect::<Vec<_>>().join("\n")
    }

    fn shows(&self, text: &str) -> bool {
        self.text().contains(text)
    }

    /// The cell a piece of text starts at.
    fn find(&self, text: &str) -> (u16, u16) {
        let screen = self.text();
        let row = screen.lines().position(|l| l.contains(text)).unwrap_or_else(|| panic!("no {text:?} in\n{screen}"));
        let line = screen.lines().nth(row).unwrap();
        let column = line[..line.find(text).unwrap()].chars().count();
        (u16::try_from(column).unwrap(), u16::try_from(row).unwrap())
    }

    fn reading(width: u16, height: u16) -> Self {
        let mut screen = Self::new(width, height);
        screen.key(KeyCode::Enter);
        assert_eq!(screen.ui.app.screen(), Screen::Article);
        screen
    }
}

#[test]
fn the_picker_opens_on_an_article_and_j_goes_on() {
    let mut s = Screenful::new(100, 30);
    assert!(s.shows("All unread") && s.shows("Oldest first"), "{}", s.text());
    s.key(KeyCode::Enter);
    assert!(s.shows("1 / 8") && s.shows("Why I read my feeds one article at a time"), "{}", s.text());
    s.keys("j");
    assert!(s.shows("2 / 8"), "{}", s.text());
}

#[test]
fn space_pages_down_and_at_the_end_moves_on() {
    let mut s = Screenful::reading(80, 12);
    s.key(KeyCode::Char(' '));
    assert!(s.ui.scroll > 0);
    let mut presses = 0;
    while s.shows("1 / 8") {
        s.key(KeyCode::Char(' '));
        presses += 1;
        assert!(presses < 50, "Space never got to the end");
    }
    assert!(s.shows("2 / 8"));
    assert_eq!(s.ui.scroll, 0, "the next article starts at the top");
}

#[test]
fn a_page_leaves_some_of_the_last_one_in_view() {
    let mut s = Screenful::reading(80, 22);
    s.key(KeyCode::Char(' '));
    assert_eq!(s.ui.scroll, 17); // 85% of the 20 rows under the bar
}

#[test]
fn the_keys_a_terminal_has_for_scrolling() {
    let mut s = Screenful::reading(80, 12);
    s.key(KeyCode::PageDown);
    let page = s.ui.scroll;
    assert!(page > 0);
    s.key(KeyCode::PageUp);
    assert_eq!(s.ui.scroll, 0);
    s.ctrl('d');
    assert_eq!(s.ui.scroll, page);
    s.ctrl('u');
    assert_eq!(s.ui.scroll, 0);
    s.keys("J");
    assert_eq!(s.ui.scroll, 2);
    s.keys("K");
    assert_eq!(s.ui.scroll, 0);
    s.key(KeyCode::End);
    assert_eq!(s.ui.scroll, s.ui.max_scroll());
    s.key(KeyCode::Home);
    assert_eq!(s.ui.scroll, 0);
    assert!(s.shows("1 / 8"), "scrolling stays on the article");
}

#[test]
fn shift_space_scrolls_up_where_the_terminal_reports_it() {
    let mut s = Screenful::reading(80, 12);
    s.key(KeyCode::PageDown);
    s.event(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::SHIFT));
    assert_eq!(s.ui.scroll, 0);
}

/// Without mouse reporting a terminal sends the wheel as ↑ and ↓ on the alternate screen, and ↓ marks read.
#[test]
fn the_wheel_scrolls_and_never_changes_the_article() {
    let mut s = Screenful::reading(80, 12);
    for _ in 0..100 {
        s.mouse(MouseEventKind::ScrollDown, 10, 5);
    }
    assert_eq!(s.ui.scroll, s.ui.max_scroll());
    assert!(s.shows("1 / 8"));
    s.mouse(MouseEventKind::ScrollUp, 10, 5);
    assert_eq!(s.ui.scroll, s.ui.max_scroll() - 3);
}

#[test]
fn the_wheel_moves_the_picker_cursor() {
    let mut s = Screenful::new(80, 24);
    s.mouse(MouseEventKind::ScrollDown, 10, 5);
    assert_eq!(s.ui.app.picker_cursor(), 1);
}

#[test]
fn a_click_on_the_title_opens_the_article() {
    let mut s = Screenful::reading(100, 30);
    let (column, row) = s.find("Why I read my feeds");
    s.mouse(MouseEventKind::Down(MouseButton::Left), column + 2, row);
    assert_eq!(s.opened.len(), 1);
    assert!(s.opened[0].starts_with("http"), "{:?}", s.opened);
}

#[test]
fn a_click_counts_where_the_article_is_scrolled_to() {
    let mut s = Screenful::reading(100, 30);
    s.keys("J");
    let (column, row) = s.find("Why I read my feeds");
    s.mouse(MouseEventKind::Down(MouseButton::Left), column, row);
    assert_eq!(s.opened.len(), 1);
    let (blank_column, _) = s.find("example.com");
    s.mouse(MouseEventKind::Down(MouseButton::Left), blank_column, row + 1);
    assert_eq!(s.opened.len(), 1, "the site's name isn't a link");
}

#[test]
fn a_click_on_an_image_opens_the_viewer_and_another_closes_it() {
    let mut s = Screenful::reading(100, 30);
    let (column, row) = s.find("Mountains at dusk");
    s.mouse(MouseEventKind::Down(MouseButton::Left), column, row);
    assert_eq!(s.ui.app.overlay(), Overlay::Image);
    s.mouse(MouseEventKind::Down(MouseButton::Left), 1, 1);
    assert_eq!(s.ui.app.overlay(), Overlay::None);
}

#[test]
fn a_click_picks_a_picker_row_and_outside_closes_it_when_it_can() {
    let mut s = Screenful::new(100, 30);
    s.mouse(MouseEventKind::Down(MouseButton::Left), 0, 0);
    assert_eq!(s.ui.app.overlay(), Overlay::Picker, "nothing to go back to yet");
    let (column, row) = s.find("Terminal Notes");
    s.mouse(MouseEventKind::Down(MouseButton::Left), column, row);
    assert!(s.shows("Terminal Notes") && s.shows("/ 2"), "{}", s.text());
    s.keys("p");
    s.mouse(MouseEventKind::Down(MouseButton::Left), 0, 0);
    assert_eq!(s.ui.app.overlay(), Overlay::None);
}

#[test]
fn typing_in_the_search_box_is_not_shortcuts() {
    let mut s = Screenful::reading(100, 30);
    s.keys("/");
    s.keys("jk watch");
    s.ctrl('u');
    s.keys("watch");
    assert_eq!(s.ui.search.as_ref().unwrap().text(), "watch");
    assert!(s.shows("1 / 8"));
    s.key(KeyCode::Enter);
    assert!(s.shows("SEARCH") && s.shows("“watch”"), "{}", s.text());
    s.keys("/");
    assert!(s.ui.search.as_ref().unwrap().is_selected(), "it starts from the last search");
    s.key(KeyCode::Esc);
    assert_eq!(s.ui.app.overlay(), Overlay::None);
}

#[test]
fn the_settings_form_switches_between_miniflux_and_google_reader_fields() {
    let mut s = Screenful::reading(100, 40);
    s.keys(",");
    s.settle();
    assert!(s.shows("● Miniflux") && s.shows("Miniflux URL") && !s.shows("Username"), "{}", s.text());
    s.ui.form.as_mut().unwrap().focus = super::ui::CHOICE;
    s.keys("x"); // typing on the choice does nothing
    s.key(KeyCode::Right);
    s.settle();
    assert!(s.shows("● Google Reader API (FreshRSS, …)") && s.shows("Username"), "{}", s.text());
    assert!(s.shows("FreshRSS: https://your-freshrss/api/greader.php."), "the hint: {}", s.text());
    assert!(!s.shows("Miniflux URL"), "{}", s.text());
    s.key(KeyCode::Tab);
    assert_eq!(s.ui.form.as_ref().unwrap().focus, 2, "the Google Reader URL");
    s.keys("x");
    let form = s.ui.form.as_ref().unwrap();
    assert!(form.fields[2].text().ends_with('x'));
    assert_eq!(form.shown(), [super::ui::CHOICE, 2, 3, 4, 5, 6]);
    s.key(KeyCode::BackTab).key(KeyCode::Left);
    s.settle();
    assert!(s.shows("● Miniflux"), "{}", s.text());
}

#[test]
fn the_settings_form_takes_typing_and_keeps_secrets_dotted() {
    let mut s = Screenful::reading(100, 30);
    s.keys(",");
    assert_eq!(s.ui.app.screen(), Screen::Settings);
    let form = s.ui.form.as_mut().unwrap();
    form.focus = 6; // Readeck's token
    s.keys("j");
    s.ui.paste("-pasted\n");
    s.settle();
    assert!(s.ui.form.as_ref().unwrap().fields[6].text().ends_with("j-pasted"));
    assert!(!s.shows("j-pasted") && s.shows("••••"), "{}", s.text());
    s.key(KeyCode::Tab);
    assert_eq!(s.ui.form.as_ref().unwrap().focus, super::ui::CHOICE, "round to the top: the server choice");
    s.key(KeyCode::BackTab);
    assert_eq!(s.ui.form.as_ref().unwrap().focus, 6);
    s.key(KeyCode::Esc);
    assert!(s.ui.form.is_none());
    assert!(s.shows("1 / 8"), "back on the article: {}", s.text());
}

#[test]
fn onboarding_cant_be_left_but_ctrl_c_still_quits() {
    let unset = Status { configured: false, readeck: false, fetches: true };
    let mut s = Screenful::starting(100, 30, unset);
    assert!(s.shows("Welcome to Kletse RSS"), "{}", s.text());
    s.key(KeyCode::Esc);
    assert_eq!(s.ui.app.overlay(), Overlay::Onboarding);
    assert!(!s.ui.quitting());
    s.ctrl('c');
    assert!(s.ui.quitting());
}

#[test]
fn q_quits() {
    let mut s = Screenful::reading(100, 30);
    s.keys("q");
    assert!(s.ui.quitting());
}

#[test]
fn ctrl_keys_stay_away_from_the_windows_font_size() {
    let mut s = Screenful::reading(100, 30);
    let before = s.ui.app.prefs();
    s.ctrl('-').ctrl('=').ctrl('0');
    assert_eq!(s.ui.app.prefs(), before);
}

#[test]
fn brackets_change_the_column() {
    let mut s = Screenful::reading(120, 30);
    assert_eq!(s.ui.column.width, 80);
    s.keys("[");
    assert_eq!(s.ui.column.width, 72);
    s.keys("]]");
    assert_eq!(s.ui.column.width, 88);
    assert_eq!(s.ui.column.x, (120 - 88) / 2);
}

#[test]
fn the_column_stays_inside_a_small_terminal() {
    let s = Screenful::reading(30, 20);
    assert!(s.ui.column.x >= 1 && s.ui.column.right() < 30, "{:?}", s.ui.column);
    for row in &s.ui.shown.as_ref().unwrap().rows {
        assert!(row.line.width() <= usize::from(s.ui.column.width));
    }
}

#[test]
fn resizing_lays_the_article_out_again() {
    let mut s = Screenful::reading(100, 30);
    let wide = s.ui.shown.as_ref().unwrap().rows.len();
    s.terminal.backend_mut().resize(40, 30);
    s.settle();
    assert!(s.ui.shown.as_ref().unwrap().rows.len() > wide);
}

#[test]
fn m_shows_the_new_status_without_going_back_to_the_top() {
    let mut s = Screenful::reading(100, 12);
    assert!(s.shows("UNREAD"));
    s.keys("m");
    assert_eq!(s.ui.scroll, 0);
    s.keys("J");
    s.keys("K");
    assert!(s.shows("READ") && !s.shows("UNREAD"), "{}", s.text());
}

#[test]
fn the_end_of_the_list_says_where_j_goes() {
    let mut s = Screenful::reading(100, 30);
    s.keys("nnnnnnnn");
    assert!(s.shows("All caught up") && s.shows("Enter for the feeds"), "{}", s.text());
}

#[test]
fn star_puts_a_star_in_the_top_bar_and_the_starred_row_lists_them() {
    let mut s = Screenful::new(100, 30);
    // The demo starts with two starred, in a row after All unread.
    let (_, row) = s.find("Starred");
    assert!(s.text().lines().nth(usize::from(row)).unwrap().contains('2'), "{}", s.text());
    s.key(KeyCode::Enter);
    assert!(!s.text().lines().next().unwrap().contains('★'), "{}", s.text());
    s.keys("*");
    let top = s.text().lines().next().unwrap().to_string();
    assert!(top.contains("★ 1 / 8"), "{top}");
    assert!(s.shows("Starred"), "the toast: {}", s.text());
    s.keys("*");
    assert!(!s.text().lines().next().unwrap().contains('★'), "{}", s.text());
    s.keys("*");
    s.keys("p");
    let (column, row) = s.find("Starred");
    assert!(s.text().lines().nth(usize::from(row)).unwrap().contains('3'), "now three: {}", s.text());
    s.mouse(MouseEventKind::Down(MouseButton::Left), column, row);
    let top = s.text().lines().next().unwrap().to_string();
    assert!(top.contains("STARRED") && top.contains("★ 1 / 3"), "{top}");
    assert!(s.shows("Designing with only five colours"), "newest first: {}", s.text());
}

#[test]
fn history_has_its_badge() {
    let mut s = Screenful::new(100, 30);
    s.keys("h");
    assert!(s.shows("All read"), "{}", s.text());
    s.key(KeyCode::Enter);
    assert!(s.shows("HISTORY"), "{}", s.text());
}

#[test]
fn a_tall_picker_keeps_the_cursor_in_view() {
    let mut s = Screenful::new(60, 8);
    for _ in 1..s.ui.app.picker_items().len() {
        s.keys("j");
    }
    let last = s.ui.app.picker_items().last().unwrap().label.clone();
    assert!(s.shows(&last), "{}", s.text());
    // The cursor wraps round to the top.
    s.keys("j");
    assert!(s.shows("All unread"), "{}", s.text());
    assert_eq!(s.ui.picker_top, 0);
}

#[test]
fn the_help_says_how_it_works_here() {
    let s = Screenful::new(100, 30);
    let lines = s.ui.help_lines();
    let keys: Vec<&str> = lines.iter().map(|(keys, _)| keys.as_str()).collect();
    assert!(!keys.iter().any(|k| k.starts_with("Ctrl ")), "font size is the terminal's: {keys:?}");
    assert!(!keys.iter().any(|k| k.contains("Shift+Space")), "most terminals don't report it: {keys:?}");
    assert!(keys.iter().any(|k| k.contains("PgDn")));
    assert!(keys.contains(&"q"));
}

#[test]
fn the_help_fits_a_narrow_terminal_by_stacking() {
    let mut s = Screenful::new(44, 60);
    s.keys("?");
    assert!(s.shows("scroll a page") && s.shows("skip (keeps unread)"), "{}", s.text());
    s.keys("?");
    assert_eq!(s.ui.app.overlay(), Overlay::Picker);
}

#[test]
fn the_help_scrolls_on_a_short_terminal() {
    let mut s = Screenful::new(100, 10);
    s.keys("?");
    assert!(!s.shows("quit"));
    for _ in 0..30 {
        s.keys("j");
    }
    assert!(s.shows("quit"), "{}", s.text());
    assert_eq!(s.ui.app.overlay(), Overlay::Help);
}

#[test]
fn a_toast_shows_and_asks_to_be_woken_for_going_away() {
    let mut s = Screenful::reading(100, 30);
    assert!(s.ui.next_wake().is_none());
    s.keys("m");
    assert!(s.shows("Marked read"), "{}", s.text());
    assert!(s.ui.next_wake().is_some());
}

#[test]
fn an_image_the_terminal_cant_show_says_so() {
    let mut s = Screenful::reading(100, 30);
    s.keys("v");
    // The demo's image is an SVG.
    assert!(s.shows("This image can't be shown here") && s.shows("Mountains at dusk"), "{}", s.text());
    assert!(s.ui.take_repaint());
    s.key(KeyCode::Esc);
    assert!(s.ui.shot.is_none());
    assert!(s.ui.take_repaint(), "what the picture left on screen goes");
}

fn png_uri(width: u32, height: u32) -> String {
    let picture = image::RgbImage::from_pixel(width, height, image::Rgb([200, 30, 30]));
    let mut bytes = std::io::Cursor::new(Vec::new());
    picture.write_to(&mut bytes, image::ImageFormat::Png).unwrap();
    format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(bytes.into_inner()))
}

/// An article with a picture of 20×10 cells in the halfblocks picker's cells of 10×20 px, between lots of text.
fn with_picture(height: u16) -> Screenful {
    let text = "<p>Words words words.</p>".repeat(30);
    let html =
        format!("<p>Before it.</p>{text}<img src='{}' alt='The picture'>{text}<p>The end.</p>", png_uri(200, 200));
    let mut screen = Screenful::new(100, height).with_content(&html);
    screen.key(KeyCode::Enter);
    screen
}

fn picture_rows(s: &Screenful) -> usize {
    s.ui.shown.as_ref().unwrap().rows.iter().filter(|r| r.slice.is_some()).count()
}

fn coloured_cells(s: &Screenful) -> usize {
    let cells = &s.terminal.backend().buffer().content;
    cells.iter().filter(|c| c.fg == Color::Rgb(200, 30, 30) || c.bg == Color::Rgb(200, 30, 30)).count()
}

#[test]
fn a_picture_takes_its_rows_in_the_article_and_is_drawn_when_scrolled_to() {
    let mut s = with_picture(40);
    assert_eq!(picture_rows(&s), 10);
    assert!(!s.shows("The picture"), "the line that stood for it is gone");
    assert_eq!(coloured_cells(&s), 0, "not in view yet");
    while s.ui.visible_pictures().is_empty() {
        s.keys("J");
    }
    assert!(coloured_cells(&s) > 0, "{}", s.text());
    let whole = {
        while s.ui.visible_pictures()[0].1 + 10 > 38 {
            s.keys("J");
        }
        coloured_cells(&s)
    };
    // Half scrolled off the top, it's cut, not gone.
    while s.ui.visible_pictures()[0].1 > -5 {
        s.keys("J");
    }
    let cut = coloured_cells(&s);
    assert!(cut > 0 && cut < whole, "{cut} of {whole}");
}

/// A sixel touching the last row lands a row too high, in foot at least.
#[test]
fn a_picture_stops_a_row_short_of_the_end_of_the_screen() {
    let html = format!("<img src='{}'>", png_uri(400, 400));
    let mut s = Screenful::new(100, 14).with_content(&html);
    s.key(KeyCode::Enter);
    assert_eq!(picture_rows(&s), 12, "it has the height of the screen");
    let buffer = s.terminal.backend().buffer();
    let on_row = |row: u16| (0..100).filter(|&x| buffer[(x, row)].bg == Color::Rgb(200, 30, 30)).count();
    assert!(on_row(12) > 0);
    assert_eq!(on_row(13), 0);
}

/// The blanks of what goes over a picture must be written, or a sixel shows through them.
#[test]
fn panels_and_toasts_have_no_blanks_left_to_chance() {
    let hidden = |s: &Screenful, x: u16, y: u16| {
        s.terminal.backend().buffer()[(x, y)].modifier.contains(ratatui::style::Modifier::HIDDEN)
    };
    let mut s = Screenful::reading(100, 30);
    s.keys("m");
    let (column, row) = s.find("Marked read");
    assert!(hidden(&s, column + 6, row), "the space between the words");
    assert!(hidden(&s, column - 1, row), "the padding");
    assert!(s.shows("Marked read"));
    s.keys("?");
    let (column, row) = s.find("skip (keeps unread)");
    assert!(hidden(&s, column + 19, row), "the panel's padding");
    assert!(!hidden(&s, 0, 2), "what isn't covered is left alone");
}

#[test]
fn a_tall_picture_fits_the_screen() {
    let html = format!("<img src='{}'>", png_uri(100, 2000));
    let mut s = Screenful::new(100, 22).with_content(&html);
    s.key(KeyCode::Enter);
    assert_eq!(picture_rows(&s), 20);
}

#[test]
fn a_small_picture_stays_small_and_a_wide_one_stays_in_the_column() {
    let small = format!("<img src='{}'>", png_uri(30, 20));
    let mut s = Screenful::new(100, 30).with_content(&small);
    s.key(KeyCode::Enter);
    assert_eq!(picture_rows(&s), 1);
    let slice = s.ui.visible_pictures()[0].0.clone();
    assert_eq!((slice.size.width, slice.column), (3, (80 - 3) / 2));

    let wide = format!("<img src='{}'>", png_uri(4000, 200));
    let mut s = Screenful::new(100, 30).with_content(&wide);
    s.key(KeyCode::Enter);
    assert_eq!(s.ui.visible_pictures()[0].0.size.width, 80);
}

#[test]
fn a_picture_arriving_above_where_you_are_leaves_your_place_alone() {
    let text = "<p>Words words words.</p>".repeat(40);
    let html = format!("<img src='{}' alt='x'><p>Marker</p>{text}", png_uri(200, 200));
    let mut s = Screenful::new(100, 30).with_content(&html);
    s.ui.key(KeyEvent::from(KeyCode::Enter));
    // The article opens, and you've scrolled on before the picture is in.
    let size: Rect = s.terminal.size().unwrap().into();
    let opening = s.ui.take_effects();
    for effect in opening {
        let mut outcome = s.runtime.block_on(s.backend.run(effect)).unwrap();
        if let Outcome::Opened { result: Ok((page, _)), .. } = &mut outcome {
            page.entries[0].content.clone_from(&html);
        }
        s.ui.outcome(outcome);
    }
    s.ui.prepare(size);
    let jobs = s.ui.take_jobs();
    s.ui.key(KeyEvent::from(KeyCode::PageDown));
    s.ui.prepare(size);
    assert!(s.ui.scroll > 0);
    let first = |s: &Screenful| s.ui.shown.as_ref().unwrap().rows[s.ui.scroll].line.to_string();
    let reading = first(&s);
    for job in jobs {
        let answer = s.runtime.block_on(images::run(job, Arc::clone(&s.backend), Picker::halfblocks()));
        s.ui.image(answer);
    }
    s.settle();
    assert_eq!(picture_rows(&s), 10);
    assert_eq!(first(&s), reading);
}

#[test]
fn an_image_that_cant_be_shown_stays_a_line_of_text() {
    let s = Screenful::reading(100, 30);
    // The demo's image is an SVG.
    assert!(s.shows("▣ Mountains at dusk"), "{}", s.text());
    assert_eq!(picture_rows(&s), 0);
}

#[test]
fn a_click_on_a_picture_opens_the_viewer() {
    let html = format!("<img src='{}' alt='x'>", png_uri(200, 200));
    let mut s = Screenful::new(100, 30).with_content(&html);
    s.key(KeyCode::Enter);
    let (slice, top) = s.ui.visible_pictures()[0];
    let column = s.ui.column.x + slice.column + 1;
    let row = u16::try_from(top).unwrap() + s.ui.column.y + 1;
    s.mouse(MouseEventKind::Down(MouseButton::Left), column, row);
    assert_eq!(s.ui.app.overlay(), Overlay::Image);
    assert!(coloured_cells(&s) > 0, "the viewer has it from the article");
}

#[test]
fn resizing_fits_the_picture_again() {
    let html = format!("<img src='{}'>", png_uri(2000, 1000));
    let mut s = Screenful::new(100, 50).with_content(&html);
    s.key(KeyCode::Enter);
    assert_eq!(s.ui.visible_pictures()[0].0.size.width, 80);
    s.terminal.backend_mut().resize(40, 50);
    s.settle();
    assert_eq!(s.ui.visible_pictures()[0].0.size.width, 38);
    assert!(coloured_cells(&s) > 0);
}

#[test]
fn the_next_articles_pictures_are_fetched_ahead_and_old_ones_dropped() {
    let mut s = Screenful::reading(100, 30);
    let first = s.ui.pictures.keys().next().unwrap().clone();
    s.keys("nnn");
    assert!(!s.ui.pictures.contains_key(&first), "two articles on, it's gone");
    s.keys("kkk");
    assert!(s.ui.pictures.contains_key(&first), "back again, it's asked for again");
}

/// Sixel pictures are pixels the terminal can't move, so they need a fresh screen; half-blocks are just cells.
#[test]
fn only_pixels_on_screen_make_a_repaint() {
    let mut cells = with_picture(60);
    while cells.ui.visible_pictures().is_empty() {
        cells.keys("J");
    }
    cells.ui.take_repaint();
    cells.keys("J");
    assert!(!cells.ui.take_repaint());

    let mut sixel = Picker::halfblocks();
    sixel.set_protocol_type(ProtocolType::Sixel);
    let mut pixels = Ui::new(Prefs::default(), true, sixel);
    pixels.start(Status { configured: true, readeck: true, fetches: true });
    let area = Rect::new(0, 0, 100, 60);
    let settle = |ui: &mut Ui, s: &Screenful| {
        loop {
            ui.prepare(area);
            let (effects, jobs) = (ui.take_effects(), ui.take_jobs());
            if effects.is_empty() && jobs.is_empty() {
                return;
            }
            for effect in effects {
                if let Some(mut outcome) = s.runtime.block_on(s.backend.run(effect)) {
                    if let (Outcome::Opened { result: Ok((page, _)), .. }, Some(html)) = (&mut outcome, &s.content) {
                        page.entries[0].content.clone_from(html);
                    }
                    ui.outcome(outcome);
                }
            }
            for job in jobs {
                let mut picker = Picker::halfblocks();
                picker.set_protocol_type(ProtocolType::Sixel);
                let answer = s.runtime.block_on(images::run(job, Arc::clone(&s.backend), picker));
                ui.image(answer);
            }
        }
    };
    let html = format!("<img src='{}'><p>{}</p>", png_uri(200, 200), "Words words. ".repeat(2000));
    let host = Screenful::new(20, 20).with_content(&html);
    settle(&mut pixels, &host);
    pixels.key(KeyEvent::from(KeyCode::Enter));
    settle(&mut pixels, &host);
    pixels.take_repaint();
    pixels.key(KeyEvent::new(KeyCode::Char('J'), KeyModifiers::SHIFT));
    settle(&mut pixels, &host);
    assert!(pixels.take_repaint(), "the picture moved");
    pixels.key(KeyEvent::new(KeyCode::Char('p'), KeyModifiers::NONE));
    settle(&mut pixels, &host);
    assert!(pixels.take_repaint(), "the picker goes over it");
    pixels.key(KeyEvent::from(KeyCode::Esc));
    settle(&mut pixels, &host);
    assert!(pixels.take_repaint(), "and comes off again");
    settle(&mut pixels, &host);
    assert!(!pixels.take_repaint(), "nothing changed");
}

fn png() -> Vec<u8> {
    let picture = image::RgbImage::from_pixel(40, 20, image::Rgb([200, 30, 30]));
    let mut bytes = std::io::Cursor::new(Vec::new());
    picture.write_to(&mut bytes, image::ImageFormat::Png).unwrap();
    bytes.into_inner()
}

#[tokio::test]
async fn pictures_are_decoded_from_data_uris_and_fit_again_for_a_new_size() {
    let src = format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(png()));
    let backend = Arc::new(Backend::new(true));
    let picker = Picker::halfblocks();
    let job = Job::Load { src: src.clone(), size: Size::new(10, 5) };
    let images::Answer::Viewer { result, .. } = images::run(job, Arc::clone(&backend), picker.clone()).await else {
        panic!("not the viewer's answer");
    };
    let (bytes, protocol) = result.unwrap();
    assert!(protocol.size().width <= 10 && protocol.size().height <= 5);

    let job = Job::Fit { src, bytes, size: Size::new(4, 2) };
    let images::Answer::Viewer { result, .. } = images::run(job, backend, picker).await else {
        panic!("not the viewer's answer");
    };
    let (_, protocol) = result.unwrap();
    assert!(protocol.size().width <= 4 && protocol.size().height <= 2);
}

#[tokio::test]
async fn demo_mode_stays_off_the_network_for_images() {
    let job = Job::Load { src: "https://example.com/a.png".into(), size: Size::new(10, 5) };
    let images::Answer::Viewer { result, .. } =
        images::run(job, Arc::new(Backend::new(true)), Picker::halfblocks()).await
    else {
        panic!("not the viewer's answer");
    };
    assert_eq!(result.err().as_deref(), Some("Not loading https://example.com/a.png"));
}

#[test]
fn a_picture_the_terminal_cant_encode_goes_back_to_its_line_and_isnt_asked_for_again() {
    let mut s = with_picture(30);
    let src = png_uri(200, 200);
    let size = match s.ui.pictures.get(&src) {
        Some(Inline::Here { sliced: Some((size, _)), .. }) => *size,
        _ => panic!("the picture was fetched and encoded"),
    };
    s.ui.image(images::Answer::Sliced { src: src.clone(), size, result: Err("no sixel".into()) });
    // Were it asked for again, settling would encode it again and it would be Here once more.
    s.settle();
    assert!(matches!(s.ui.pictures.get(&src), Some(Inline::Failed)));
}

#[test]
fn a_picture_for_something_no_longer_on_screen_is_dropped() {
    let mut s = Screenful::reading(100, 30);
    s.keys("v");
    let answer = images::Answer::Viewer { src: "gone".into(), size: Size::new(1, 1), result: Err("late".into()) };
    s.ui.image(answer);
    s.settle();
    assert!(!s.shows("late"));
}

/// Every screen, down to a terminal too small to show anything: none of them may panic.
#[test]
fn nothing_breaks_in_a_tiny_terminal() {
    for (width, height) in [(1, 1), (2, 2), (3, 1), (1, 5), (4, 3), (10, 2), (10, 3), (12, 4), (20, 6), (30, 8)] {
        for keys in ["", "?", "/", ",", "A", "v", "nnnnnnnn", "m", "h"] {
            let mut s = Screenful::new(width, height);
            s.key(KeyCode::Enter);
            s.keys(keys);
            s.keys("J");
        }
        let mut s = Screenful::starting(width, height, Status { configured: false, readeck: false, fetches: true });
        s.keys("x");
    }
}
