//! A one-line text field with a cursor, used by forms, the search, the command palette and the typed
//! delete confirmation.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TextInput {
    text: String,
    /// cursor position in characters, `0..=len`
    cursor: usize,
    /// the whole text is selected: typing or pasting replaces it, Backspace/Delete clear it,
    /// cursor keys keep it (like a prefilled field in VS Code that gets the focus)
    selected: bool,
}

impl TextInput {
    pub fn new(text: impl Into<String>) -> Self {
        let text = text.into();
        let cursor = text.chars().count();
        TextInput { text, cursor, selected: false }
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    pub fn set(&mut self, text: impl Into<String>) {
        *self = TextInput::new(text);
    }

    /// Selects the whole text, if there is any; called when the field gets the focus.
    pub fn select_all(&mut self) {
        self.selected = !self.text.is_empty();
        self.cursor = self.len();
    }

    pub fn is_selected(&self) -> bool {
        self.selected
    }

    /// Keeps the text, ends the selection: when the field loses the focus.
    pub fn unselect(&mut self) {
        self.selected = false;
    }

    /// Ends a selection by replacing the text: before typing or pasting.
    fn replace_selection(&mut self) {
        if self.selected {
            self.clear();
        }
    }

    pub fn clear(&mut self) {
        self.text.clear();
        self.cursor = 0;
        self.selected = false;
    }

    fn byte_at(&self, chars: usize) -> usize {
        self.text.char_indices().nth(chars).map_or(self.text.len(), |(i, _)| i)
    }

    fn len(&self) -> usize {
        self.text.chars().count()
    }

    pub fn insert(&mut self, c: char) {
        self.replace_selection();
        let at = self.byte_at(self.cursor);
        self.text.insert(at, c);
        self.cursor += 1;
    }

    /// Pasted text: line breaks become spaces, it is a one-line field.
    pub fn insert_str(&mut self, s: &str) {
        self.replace_selection();
        for c in s.chars() {
            match c {
                '\r' => {},
                '\n' | '\t' => self.insert(' '),
                c if c.is_control() => {},
                c => self.insert(c),
            }
        }
    }

    pub fn backspace(&mut self) {
        if self.selected {
            return self.clear();
        }
        if self.cursor > 0 {
            self.cursor -= 1;
            let at = self.byte_at(self.cursor);
            self.text.remove(at);
        }
    }

    pub fn delete(&mut self) {
        if self.selected {
            return self.clear();
        }
        if self.cursor < self.len() {
            let at = self.byte_at(self.cursor);
            self.text.remove(at);
        }
    }

    pub fn left(&mut self) {
        if std::mem::take(&mut self.selected) {
            return;
        }
        self.cursor = self.cursor.saturating_sub(1);
    }

    pub fn right(&mut self) {
        if std::mem::take(&mut self.selected) {
            return;
        }
        self.cursor = (self.cursor + 1).min(self.len());
    }

    pub fn home(&mut self) {
        self.selected = false;
        self.cursor = 0;
    }

    pub fn end(&mut self) {
        self.selected = false;
        self.cursor = self.len();
    }

    /// Handles an editing key. Returns `false` for keys a text field does not use (Enter, Esc, Tab, ↑, ↓ …).
    pub fn on_key(&mut self, key: &KeyEvent) -> bool {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Char('u') if ctrl => self.clear(),
            KeyCode::Char('a') if ctrl => self.home(),
            KeyCode::Char('e') if ctrl => self.end(),
            KeyCode::Char(_) if ctrl || key.modifiers.contains(KeyModifiers::ALT) => return false,
            KeyCode::Char(c) => self.insert(c),
            KeyCode::Backspace => self.backspace(),
            KeyCode::Delete => self.delete(),
            KeyCode::Left => self.left(),
            KeyCode::Right => self.right(),
            KeyCode::Home => self.home(),
            KeyCode::End => self.end(),
            _ => return false,
        }
        true
    }

    /// The text split at the cursor: before, the character under the cursor (or a space at the end), after.
    pub fn split(&self) -> (String, String, String) {
        let before: String = self.text.chars().take(self.cursor).collect();
        let at: String = self.text.chars().nth(self.cursor).map_or_else(|| String::from(" "), String::from);
        let after: String = self.text.chars().skip(self.cursor + 1).collect();
        (before, at, after)
    }

    /// Like [`split`](Self::split), but only the part that fits into `width` columns, keeping the cursor visible.
    pub fn visible(&self, width: usize) -> (String, String, String) {
        let width = width.max(2);
        let chars: Vec<char> = self.text.chars().collect();
        let start = (self.cursor + 1).saturating_sub(width);
        let end = (start + width).min(chars.len().max(self.cursor + 1));
        let before: String = chars[start.min(chars.len())..self.cursor.min(chars.len())].iter().collect();
        let at: String = chars.get(self.cursor).map_or_else(|| String::from(" "), |c| c.to_string());
        let after: String = if self.cursor + 1 < end { chars[self.cursor + 1..end.min(chars.len())].iter().collect() } else { String::new() };
        (before, at, after)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::crossterm::event::KeyEventKind;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new_with_kind(code, KeyModifiers::NONE, KeyEventKind::Press)
    }

    #[test]
    fn edits_at_the_cursor() {
        let mut input = TextInput::new("Sommerbal");
        assert_eq!(input.cursor(), 9);
        input.on_key(&key(KeyCode::Char('l')));
        assert_eq!(input.text(), "Sommerball");
        input.on_key(&key(KeyCode::Home));
        input.on_key(&key(KeyCode::Delete));
        input.on_key(&key(KeyCode::Char('S')));
        assert_eq!(input.text(), "Sommerball");
        input.on_key(&key(KeyCode::End));
        input.on_key(&key(KeyCode::Left));
        input.on_key(&key(KeyCode::Backspace));
        assert_eq!(input.text(), "Sommerbal");
        assert_eq!(input.cursor(), 8);
        assert!(!input.on_key(&key(KeyCode::Enter)));
    }

    #[test]
    fn is_unicode_safe() {
        let mut input = TextInput::new("Brötchen");
        for _ in 0..6 {
            input.left();
        }
        input.backspace();
        assert_eq!(input.text(), "Bötchen");
        input.delete();
        assert_eq!(input.text(), "Btchen");
        input.insert('ä');
        assert_eq!(input.text(), "Bätchen");
        assert_eq!(input.split(), (String::from("Bä"), String::from("t"), String::from("chen")));
    }

    #[test]
    fn ctrl_u_clears_and_paste_stays_on_one_line() {
        let mut input = TextInput::new("abc");
        input.on_key(&KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));
        assert!(input.is_empty());
        input.insert_str("Nöthnitzer\r\nStraße 46");
        assert_eq!(input.text(), "Nöthnitzer Straße 46");
    }

    #[test]
    fn typing_into_a_selected_field_replaces_it() {
        let mut input = TextInput::new("12.11.2026");
        input.select_all();
        assert!(input.is_selected());
        for c in "12.11.".chars() {
            input.on_key(&key(KeyCode::Char(c)));
        }
        assert_eq!(input.text(), "12.11.");
        assert!(!input.is_selected());
    }

    #[test]
    fn paste_replaces_and_backspace_or_delete_clear_a_selection() {
        let mut input = TextInput::new("Herr");
        input.select_all();
        input.insert_str("Frau");
        assert_eq!(input.text(), "Frau");
        input.select_all();
        input.on_key(&key(KeyCode::Backspace));
        assert!(input.is_empty());
        let mut input = TextInput::new("abc");
        input.select_all();
        input.on_key(&key(KeyCode::Delete));
        assert!(input.is_empty());
    }

    #[test]
    fn cursor_keys_drop_the_selection_and_keep_the_text() {
        let cases = [(KeyCode::Left, "20276"), (KeyCode::Right, "20267"), (KeyCode::Home, "72026"), (KeyCode::End, "20267")];
        for (code, expected) in cases {
            let mut input = TextInput::new("2026");
            input.select_all();
            input.on_key(&key(code));
            assert!(!input.is_selected(), "{:?}", code);
            assert_eq!(input.text(), "2026");
            if code == KeyCode::Left {
                // the first ← only ends the selection, the cursor stays at the end
                input.on_key(&key(KeyCode::Left));
            }
            input.on_key(&key(KeyCode::Char('7')));
            assert_eq!(input.text(), expected, "{:?}", code);
        }
        // an empty field has nothing to select
        let mut input = TextInput::new("");
        input.select_all();
        assert!(!input.is_selected());
    }

    #[test]
    fn visible_part_keeps_the_cursor() {
        let input = TextInput::new("0123456789");
        let (before, at, after) = input.visible(5);
        assert_eq!((before.as_str(), at.as_str(), after.as_str()), ("6789", " ", ""));
        let mut input = input;
        input.home();
        let (before, at, after) = input.visible(5);
        assert_eq!((before.as_str(), at.as_str(), after.as_str()), ("", "0", "1234"));
    }
}
