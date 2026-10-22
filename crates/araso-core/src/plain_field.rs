//! A text field in memory: committed text, an optional marked span, and a caret. For the CLI's
//! `--type` and the tests, as the upstream `FakeTextField`.

use crate::input_session::{TextClient, TextRange};
use crate::text;

#[derive(Default, Debug)]
pub struct PlainTextField {
    pub text: String,
    pub marked: Option<TextRange>,
    /// UTF-16 units.
    pub cursor: usize,
    /// False simulates apps that don't support reading back their text.
    pub unreadable: bool,
    /// Every text the input method inserted, in order: a held word must go in with its boundary in one.
    pub insertions: Vec<String>,
}

impl PlainTextField {
    pub fn new() -> Self {
        Self::default()
    }

    fn union(a: TextRange, b: TextRange) -> TextRange {
        let start = a.location.min(b.location);
        let end = a.end().max(b.end());
        TextRange::new(start, end - start)
    }

    /// The marked text, if any.
    pub fn marked_text(&self) -> Option<String> {
        self.marked.and_then(|m| text::utf16_substring(&self.text, m.location, m.length))
    }
}

impl TextClient for PlainTextField {
    fn set_marked_text(&mut self, s: &str) {
        let target = self.marked.unwrap_or(TextRange::new(self.cursor, 0));
        self.text = text::utf16_replace(&self.text, target.location, target.length, s);
        let length = text::utf16_len(s);
        self.marked = (length > 0).then(|| TextRange::new(target.location, length));
        self.cursor = target.location + length;
    }

    fn set_marked_text_replacing(&mut self, s: &str, range: TextRange) {
        let target = match self.marked {
            Some(marked) => Self::union(range, marked),
            None => range,
        };
        self.text = text::utf16_replace(&self.text, target.location, target.length, s);
        let length = text::utf16_len(s);
        self.marked = (length > 0).then(|| TextRange::new(target.location, length));
        self.cursor = target.location + length;
    }

    fn insert_text(&mut self, s: &str, range: Option<TextRange>) {
        self.insertions.push(s.to_string());
        let target = range.or(self.marked).unwrap_or(TextRange::new(self.cursor, 0));
        self.text = text::utf16_replace(&self.text, target.location, target.length, s);
        self.marked = None;
        self.cursor = target.location + text::utf16_len(s);
    }

    fn selected_range(&mut self) -> Option<TextRange> {
        Some(TextRange::new(self.cursor, 0))
    }

    fn marked_range(&mut self) -> Option<TextRange> {
        self.marked
    }

    fn text_in(&mut self, range: TextRange) -> Option<String> {
        if self.unreadable {
            return None;
        }
        text::utf16_substring(&self.text, range.location, range.length)
    }
}
