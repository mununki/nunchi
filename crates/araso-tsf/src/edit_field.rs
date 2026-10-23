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

/// The word being typed in a control, and where it is in the control's text.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct EditWord {
    pub field: Field,
    pub range: TextRange,
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
pub fn underline(word: &EditWord, on: bool) {
    if word.field.kind != Kind::RichEdit {
        return;
    }
    let range = word.range;
    let result = document(word.field.hwnd).and_then(|document| unsafe {
        let font = document.Range(range.location as i32, range.end() as i32)?.GetFont()?;
        font.Reset(tomApplyTmp)?;
        font.SetUnderline(if on { tomSingle.0 } else { tomNone.0 })?;
        font.Reset(tomApplyNow)
    });
    if let Err(e) = result {
        crate::log::write(&format!("underline in the rich edit control failed: {e}"));
    }
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
    pub word: &'a mut Option<TextRange>,
}

impl EditClient<'_> {
    /// Puts `text` in place of `range`; the caret goes after it. It goes in as typing does, through
    /// the control's messages: the app hears of it (KakaoTalk clears its placeholder, text it
    /// keeps in the field, only then).
    fn replace(&self, range: TextRange, text: &str) {
        let hwnd = self.field.hwnd;
        let wide: Vec<u16> = text.encode_utf16().chain(Some(0)).collect();
        unsafe {
            SendMessageW(hwnd, EM_SETSEL, Some(WPARAM(range.location)), Some(LPARAM(range.end() as isize)));
            // Undoable, as typing is.
            SendMessageW(hwnd, EM_REPLACESEL, Some(WPARAM(1)), Some(LPARAM(PCWSTR(wide.as_ptr()).0 as isize)));
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
        self.word.or_else(|| selection(self.field.hwnd)).unwrap_or(TextRange::new(0, 0))
    }

    /// The word, `text` in place of `range`, underlined as it changes.
    fn mark(&mut self, range: TextRange, text: &str) {
        self.quietly(|client| {
            client.unmark();
            client.replace(range, text);
            *client.word = (!text.is_empty()).then(|| TextRange::new(range.location, utf16_len(text)));
            if let Some(range) = *client.word {
                underline(&EditWord { field: client.field, range }, true);
            }
        });
    }

    fn unmark(&self) {
        if let Some(range) = *self.word {
            underline(&EditWord { field: self.field, range }, false);
        }
    }
}

impl TextClient for EditClient<'_> {
    fn set_marked_text(&mut self, text: &str) {
        let range = self.target();
        self.mark(range, text);
    }

    fn set_marked_text_replacing(&mut self, text: &str, range: TextRange) {
        let end = self.word.map_or(range.end(), |w| w.end().max(range.end()));
        self.mark(TextRange::new(range.location, end.saturating_sub(range.location)), text);
    }

    fn insert_text(&mut self, text: &str, range: Option<TextRange>) {
        let range = match range {
            Some(range) => range,
            None => self.target(),
        };
        self.quietly(|client| {
            client.unmark();
            client.replace(range, text);
            *client.word = None;
        });
    }

    fn selected_range(&mut self) -> Option<TextRange> {
        selection(self.field.hwnd)
    }

    fn marked_range(&mut self) -> Option<TextRange> {
        *self.word
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
