//! A one-line text field: the search box and the settings form.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use unicode_width::UnicodeWidthStr;

#[derive(Debug, Default, Clone)]
pub struct Editor {
    text: String,
    /// In bytes, on a character boundary.
    cursor: usize,
    /// All of it is selected, as the search box starts from the last search: typing replaces it.
    selected: bool,
}

impl Editor {
    pub fn new(text: &str) -> Self {
        Self { text: text.to_string(), cursor: text.len(), selected: false }
    }

    /// Starts with everything selected.
    pub fn selected(text: &str) -> Self {
        Self { selected: !text.is_empty(), ..Self::new(text) }
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn is_selected(&self) -> bool {
        self.selected
    }

    fn before(&self) -> usize {
        self.text[..self.cursor].char_indices().next_back().map_or(0, |(i, _)| i)
    }

    fn after(&self) -> usize {
        self.text[self.cursor..].chars().next().map_or(self.cursor, |c| self.cursor + c.len_utf8())
    }

    /// Where the word before the cursor starts.
    fn word_before(&self) -> usize {
        let trimmed = self.text[..self.cursor].trim_end();
        trimmed.rfind(char::is_whitespace).map_or(0, |i| i + trimmed[i..].chars().next().map_or(1, char::len_utf8))
    }

    fn take_selection(&mut self) {
        if std::mem::take(&mut self.selected) {
            self.text.clear();
            self.cursor = 0;
        }
    }

    fn insert(&mut self, c: char) {
        self.take_selection();
        self.text.insert(self.cursor, c);
        self.cursor += c.len_utf8();
    }

    fn delete(&mut self, from: usize, to: usize) {
        self.text.replace_range(from..to, "");
        self.cursor = from;
    }

    /// What was pasted, on one line.
    pub fn paste(&mut self, text: &str) {
        text.chars().filter(|c| !c.is_control()).for_each(|c| self.insert(c));
    }

    /// Takes a key; emacs keys as in a shell.
    pub fn key(&mut self, event: KeyEvent) {
        let ctrl = event.modifiers.contains(KeyModifiers::CONTROL);
        let was_selected = self.selected;
        match event.code {
            KeyCode::Char('a') if ctrl => self.cursor = 0,
            KeyCode::Char('e') if ctrl => self.cursor = self.text.len(),
            KeyCode::Char('u') if ctrl => self.delete(0, self.cursor),
            KeyCode::Char('k') if ctrl => self.delete(self.cursor, self.text.len()),
            KeyCode::Char('w') if ctrl => self.delete(self.word_before(), self.cursor),
            KeyCode::Char(_) if ctrl || event.modifiers.contains(KeyModifiers::ALT) => return,
            KeyCode::Char(c) => return self.insert(c),
            KeyCode::Backspace | KeyCode::Delete if was_selected => self.take_selection(),
            KeyCode::Backspace => self.delete(self.before(), self.cursor),
            KeyCode::Delete => self.delete(self.cursor, self.after()),
            KeyCode::Left => self.cursor = self.before(),
            KeyCode::Right => self.cursor = self.after(),
            KeyCode::Home => self.cursor = 0,
            KeyCode::End => self.cursor = self.text.len(),
            _ => return,
        }
        self.selected = false;
    }

    /// The part of the text that fits `width` columns with the cursor in view, and the cursor's column in it.
    /// A secret shows as dots.
    pub fn view(&self, width: usize, secret: bool) -> (String, usize) {
        let shown = |s: &str| if secret { "•".repeat(s.chars().count()) } else { s.to_string() };
        let (head, tail) = (shown(&self.text[..self.cursor]), shown(&self.text[self.cursor..]));
        let room = width.saturating_sub(1); // the cursor needs a cell of its own at the end
        let mut start = 0;
        while head[start..].width() > room {
            start += head[start..].chars().next().map_or(1, char::len_utf8);
        }
        let column = head[start..].width();
        let mut visible = head[start..].to_string();
        for c in tail.chars() {
            let wider = format!("{visible}{c}");
            if wider.width() > width {
                break;
            }
            visible = wider;
        }
        (visible, column)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn press(editor: &mut Editor, keys: &str) {
        keys.chars().for_each(|c| editor.key(KeyEvent::from(KeyCode::Char(c))));
    }

    fn ctrl(editor: &mut Editor, c: char) {
        editor.key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL));
    }

    #[test]
    fn edits_at_the_cursor_across_multibyte_text() {
        let mut e = Editor::new("héé");
        e.key(KeyCode::Left.into());
        e.key(KeyCode::Backspace.into());
        press(&mut e, "ó");
        assert_eq!(e.text(), "hóé");
        e.key(KeyCode::Home.into());
        e.key(KeyCode::Delete.into());
        assert_eq!(e.text(), "óé");
    }

    #[test]
    fn a_selected_field_is_replaced_by_typing_and_kept_by_moving() {
        let mut typed = Editor::selected("old words");
        assert!(typed.is_selected());
        press(&mut typed, "n");
        assert_eq!(typed.text(), "n");

        let mut moved = Editor::selected("old");
        moved.key(KeyCode::Home.into());
        press(&mut moved, "x");
        assert_eq!(moved.text(), "xold");

        let mut cleared = Editor::selected("old");
        cleared.key(KeyCode::Backspace.into());
        assert_eq!(cleared.text(), "");
    }

    #[test]
    fn shell_keys_cut_words_and_lines() {
        let mut e = Editor::new("one two  ");
        ctrl(&mut e, 'w');
        assert_eq!(e.text(), "one ");
        ctrl(&mut e, 'u');
        assert_eq!(e.text(), "");
        let mut e = Editor::new("héllo");
        ctrl(&mut e, 'a');
        e.key(KeyCode::Right.into());
        ctrl(&mut e, 'k');
        assert_eq!(e.text(), "h");
    }

    #[test]
    fn pasting_keeps_it_on_one_line() {
        let mut e = Editor::default();
        e.paste("key\r\n");
        assert_eq!(e.text(), "key");
    }

    #[test]
    fn the_view_follows_the_cursor_and_hides_secrets() {
        let mut e = Editor::new("abcdefghij");
        assert_eq!(e.view(5, false), ("ghij".to_string(), 4));
        for _ in 0..8 {
            e.key(KeyCode::Left.into());
        }
        assert_eq!(e.view(5, false), ("abcde".to_string(), 2));
        assert_eq!(Editor::new("abc").view(10, true), ("•••".to_string(), 3));
    }
}
