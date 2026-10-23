//! A classic EDIT or RichEdit control (Explorer's rename box, KakaoTalk's chat input, the fields
//! of most dialogs) sees Araso through TSF's compatibility layer, which shows only the first
//! character of a Korean composition. The control is in this thread, though, and its text is one
//! message away: there the word being typed goes straight into the text and changes in place as
//! it is typed, as it does in a TSF field, and the text before it can be read (a word just
//! committed can still be switched).

use araso_core::text::utf16_len;
use araso_core::{TextClient, TextRange};
use windows::core::{Interface, PCWSTR};
use windows::Win32::Foundation::{E_NOINTERFACE, HWND, LPARAM, WPARAM};
use windows::Win32::Graphics::Gdi::{RedrawWindow, RDW_INVALIDATE, RDW_UPDATENOW};
use windows::Win32::UI::Controls::RichEdit::{
    tomApplyNow, tomApplyTmp, tomNone, tomSingle, IRichEditOle, ITextDocument, EM_GETOLEINTERFACE,
};
use windows::Win32::UI::Input::KeyboardAndMouse::GetFocus;
use windows::Win32::UI::WindowsAndMessaging::*;

const EM_GETSEL: u32 = 0x00B0;
const EM_SETSEL: u32 = 0x00B1;
const EM_REPLACESEL: u32 = 0x00C2;
const EM_SCROLLCARET: u32 = 0x00B7;
const EM_GETTEXTRANGE: u32 = 0x0400 + 75;

/// `CHARRANGE` and `TEXTRANGEW` of a RichEdit control.
#[repr(C)]
struct TextRangeW {
    min: i32,
    max: i32,
    text: *mut u16,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Edit,
    /// Counts a paragraph break as one character, as its text comes with `EM_GETTEXTRANGE` (the
    /// window text has "\r\n").
    RichEdit,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Field {
    pub hwnd: HWND,
    pub kind: Kind,
}

/// The word being typed in a control: where it is in the control's text, and what it took the
/// place of. Ctrl+Z takes a whole word back, as typing it: not the changes on the way.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Word {
    pub range: TextRange,
    /// The text the word was typed over (the selection, or a committed word being switched). An
    /// EDIT control keeps one undo step: the changes on the way aren't recorded, and the word goes
    /// in as one replacement of this when it ends.
    pub original: String,
}

/// The word being typed in a control, kept between keys.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct EditWord {
    pub field: Field,
    pub word: Word,
}

/// The focused control when it is an EDIT or RichEdit (or one built on them) whose text is
/// UTF-16, as `TextRange` counts. A password box is left to the system.
pub fn focused() -> Option<Field> {
    unsafe {
        let hwnd = GetFocus();
        if hwnd.is_invalid() || !IsWindowUnicode(hwnd).as_bool() {
            return None;
        }
        let mut class = [0u16; 32];
        let n = RealGetWindowClassW(hwnd, &mut class) as usize;
        let class = String::from_utf16_lossy(&class[..n]).to_ascii_lowercase();
        let kind = match class.as_str() {
            "edit" => Kind::Edit,
            c if c.starts_with("richedit") => Kind::RichEdit,
            _ => return None,
        };
        let style = GetWindowLongW(hwnd, GWL_STYLE);
        (style & ES_PASSWORD == 0).then_some(Field { hwnd, kind })
    }
}

/// Underlines the word in a RichEdit control, or with `false` takes the underline off, the way an
/// IME marks what it composes: RichEdit's temporary formatting, shown but not part of the text
/// (not saved, not undone, not read back). An EDIT control has no formatting: there the word
/// shows as typed.
fn underline(field: Field, range: TextRange, on: bool) {
    if field.kind != Kind::RichEdit {
        return;
    }
    let result = document(field.hwnd).and_then(|document| unsafe {
        let font = document.Range(range.location as i32, range.end() as i32)?.GetFont()?;
        font.Reset(tomApplyTmp)?;
        font.SetUnderline(if on { tomSingle.0 } else { tomNone.0 })?;
        font.Reset(tomApplyNow)
    });
    if let Err(e) = result {
        crate::log::write(&format!("underline in the rich edit control failed: {e}"));
    }
}

/// Starts (or ends) one undo step for a word in a RichEdit control: every change in between, the
/// app's own too (KakaoTalk taking its placeholder away), is undone together.
fn group(field: Field, start: bool) {
    let result = document(field.hwnd).and_then(|document| unsafe {
        if start {
            document.BeginEditCollection()
        } else {
            document.EndEditCollection()
        }
    });
    if let Err(e) = result {
        crate::log::write(&format!("undo step in the rich edit control failed: {e}"));
    }
}

/// The caret is no longer right after the word: a click (or the app) moved it.
pub fn caret_left(word: &EditWord) -> bool {
    selection(word.field.hwnd) != Some(TextRange::new(word.word.range.end(), 0))
}

/// The caret left the word (a click): the word stays as it is and becomes one undo step, as a
/// committed word does, and its underline goes. The caret stays where it was put.
pub fn settle(word: EditWord) {
    let field = word.field;
    let caret = selection(field.hwnd);
    let mut slot = Some(word.word);
    let mut client = EditClient { field, word: &mut slot };
    client.quietly(|client| {
        client.unmark();
        if let Some(word) = client.word.take() {
            let text = client.text_in(word.range).unwrap_or_default();
            client.finish(word, &text);
        }
        if let Some(caret) = caret {
            unsafe { SendMessageW(field.hwnd, EM_SETSEL, Some(WPARAM(caret.location)), Some(LPARAM(caret.end() as isize))) };
        }
    });
}

/// A RichEdit control's text object model.
fn document(hwnd: HWND) -> windows::core::Result<ITextDocument> {
    let mut ole: Option<IRichEditOle> = None;
    unsafe { SendMessageW(hwnd, EM_GETOLEINTERFACE, None, Some(LPARAM(&mut ole as *mut _ as isize))) };
    ole.ok_or_else(|| E_NOINTERFACE.into()).and_then(|ole| ole.cast())
}

/// The control's selection.
pub fn selection(hwnd: HWND) -> Option<TextRange> {
    let (mut start, mut end) = (0u32, 0u32);
    unsafe {
        if !IsWindow(Some(hwnd)).as_bool() {
            return None;
        }
        let start_at = WPARAM(&mut start as *mut u32 as usize);
        let end_at = LPARAM(&mut end as *mut u32 as isize);
        SendMessageW(hwnd, EM_GETSEL, Some(start_at), Some(end_at));
    }
    Some(TextRange::new(start as usize, end.saturating_sub(start) as usize))
}

/// `TextClient` over an EDIT or RichEdit control's messages. `word` is the word being typed,
/// kept between keys.
pub struct EditClient<'a> {
    pub field: Field,
    pub word: &'a mut Option<Word>,
}

impl EditClient<'_> {
    /// Puts `text` in place of `range`; the caret goes after it. It goes in as typing does, through
    /// the control's messages: the app hears of it (KakaoTalk clears its placeholder, text it
    /// keeps in the field, only then). `undoable`: recorded for Ctrl+Z.
    fn replace(&self, range: TextRange, text: &str, undoable: bool) {
        let hwnd = self.field.hwnd;
        let wide: Vec<u16> = text.encode_utf16().chain(Some(0)).collect();
        unsafe {
            SendMessageW(hwnd, EM_SETSEL, Some(WPARAM(range.location)), Some(LPARAM(range.end() as isize)));
            let text = LPARAM(PCWSTR(wide.as_ptr()).0 as isize);
            SendMessageW(hwnd, EM_REPLACESEL, Some(WPARAM(undoable as usize)), Some(text));
        }
    }

    /// The word ends as `text`, one undo step from what it was typed over.
    fn finish(&mut self, word: Word, text: &str) {
        match self.field.kind {
            Kind::RichEdit => {
                if self.text_in(word.range).as_deref() != Some(text) {
                    self.replace(word.range, text, true);
                }
                group(self.field, false);
            }
            Kind::Edit => {
                // Back to what it was typed over, unrecorded, then the word in one recorded step.
                self.replace(word.range, &word.original, false);
                if !(word.original.is_empty() && text.is_empty()) {
                    self.replace(TextRange::new(word.range.location, utf16_len(&word.original)), text, true);
                }
            }
        }
    }

    /// Runs `change` with the control's drawing off and draws the result once, so the word never
    /// shows half-changed: selected on the way, or without its underline.
    fn quietly<R>(&mut self, change: impl FnOnce(&mut Self) -> R) -> R {
        let hwnd = self.field.hwnd;
        unsafe { SendMessageW(hwnd, WM_SETREDRAW, Some(WPARAM(0)), None) };
        let r = change(self);
        unsafe {
            SendMessageW(hwnd, WM_SETREDRAW, Some(WPARAM(1)), None);
            let _ = RedrawWindow(Some(hwnd), None, None, RDW_INVALIDATE | RDW_UPDATENOW);
            SendMessageW(hwnd, EM_SCROLLCARET, None, None);
        }
        r
    }

    fn edit_text(&self) -> Vec<u16> {
        unsafe {
            let length = GetWindowTextLengthW(self.field.hwnd).max(0) as usize;
            let mut buffer = vec![0u16; length + 1];
            let n = GetWindowTextW(self.field.hwnd, &mut buffer) as usize;
            buffer.truncate(n);
            buffer
        }
    }

    fn rich_text(&self, range: TextRange) -> Vec<u16> {
        let mut buffer = vec![0u16; range.length + 1];
        let mut request = TextRangeW { min: range.location as i32, max: range.end() as i32, text: buffer.as_mut_ptr() };
        let n = unsafe {
            let request = LPARAM(&mut request as *mut TextRangeW as isize);
            SendMessageW(self.field.hwnd, EM_GETTEXTRANGE, None, Some(request)).0.max(0) as usize
        };
        buffer.truncate(n.min(range.length));
        buffer
    }

    /// The word if there is one, else the selection.
    fn target(&mut self) -> TextRange {
        self.word.as_ref().map(|w| w.range).or_else(|| selection(self.field.hwnd)).unwrap_or(TextRange::new(0, 0))
    }

    /// The word, `text` in place of `range`, underlined as it changes.
    fn mark(&mut self, range: TextRange, text: &str) {
        if self.word.is_none() && text.is_empty() && range.length == 0 {
            return;
        }
        self.quietly(|client| {
            client.unmark();
            let mut word = match client.word.take() {
                Some(mut word) => {
                    // Committed text the word now takes in (a word being switched back) was there
                    // before it, as what it was typed over was.
                    let before = TextRange::new(range.location, word.range.location.saturating_sub(range.location));
                    let after = TextRange::new(word.range.end(), range.end().saturating_sub(word.range.end()));
                    if before.length > 0 {
                        word.original = client.text_in(before).unwrap_or_default() + &word.original;
                    }
                    if after.length > 0 {
                        word.original += &client.text_in(after).unwrap_or_default();
                    }
                    word
                }
                None => {
                    if client.field.kind == Kind::RichEdit {
                        group(client.field, true);
                    }
                    Word { range, original: client.text_in(range).unwrap_or_default() }
                }
            };
            // A RichEdit control records the changes inside the word's undo step; an EDIT control
            // records the word only when it ends.
            client.replace(range, text, client.field.kind == Kind::RichEdit);
            word.range = TextRange::new(range.location, utf16_len(text));
            if text.is_empty() {
                client.finish(word, "");
            } else {
                underline(client.field, word.range, true);
                *client.word = Some(word);
            }
        });
    }

    fn unmark(&self) {
        if let Some(word) = self.word.as_ref() {
            underline(self.field, word.range, false);
        }
    }
}

impl TextClient for EditClient<'_> {
    fn set_marked_text(&mut self, text: &str) {
        let range = self.target();
        self.mark(range, text);
    }

    fn set_marked_text_replacing(&mut self, text: &str, range: TextRange) {
        let end = self.word.as_ref().map_or(range.end(), |w| w.range.end().max(range.end()));
        self.mark(TextRange::new(range.location, end.saturating_sub(range.location)), text);
    }

    fn insert_text(&mut self, text: &str, range: Option<TextRange>) {
        self.quietly(|client| {
            client.unmark();
            match (range, client.word.take()) {
                (None, Some(word)) => client.finish(word, text),
                (None, None) => {
                    let range = client.target();
                    client.replace(range, text, true);
                }
                (Some(range), word) => {
                    // A committed word changed in place; a word being typed, if any, goes in first.
                    if let Some(word) = word {
                        let current = client.text_in(word.range).unwrap_or_default();
                        client.finish(word, &current);
                    }
                    client.replace(range, text, true);
                }
            }
        });
    }

    fn selected_range(&mut self) -> Option<TextRange> {
        selection(self.field.hwnd)
    }

    fn marked_range(&mut self) -> Option<TextRange> {
        self.word.as_ref().map(|w| w.range)
    }

    fn text_in(&mut self, range: TextRange) -> Option<String> {
        let units = match self.field.kind {
            Kind::Edit => self.edit_text().get(range.location..range.end())?.to_vec(),
            Kind::RichEdit => self.rich_text(range),
        };
        // Only the whole range is an answer: shorter, it ran past the text.
        (units.len() == range.length).then(|| String::from_utf16_lossy(&units))
    }
}

#[cfg(test)]
#[path = "edit_field_tests.rs"]
mod tests;
