//! `TextClient` over a TSF context inside an edit session: the marked text is Araso's
//! composition (underlined with its display attribute), and ranges are ACP offsets, UTF-16 units
//! as `TextRange` counts them. A field that can't give ACP offsets answers "not known". In an
//! IMM32 app the marked text is shown in a window of its own instead (`CompositionWindow`).

use std::mem::ManuallyDrop;

use araso_core::{TextClient, TextRange};
use windows::core::{Interface, Result, BOOL};
use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::System::Variant::{VARIANT, VT_I4};
use windows::Win32::UI::Input::KeyboardAndMouse::GetFocus;
use windows::Win32::UI::TextServices::{
    ITfComposition, ITfCompositionSink, ITfContext, ITfContextComposition, ITfInsertAtSelection, ITfRange,
    ITfRangeACP, GUID_PROP_ATTRIBUTE, INSERT_TEXT_AT_SELECTION_FLAGS, TF_AE_NONE, TF_ANCHOR_END,
    TF_DEFAULT_SELECTION, TF_IAS_QUERYONLY, TF_SELECTION, TF_SELECTIONSTYLE,
};

use crate::composition_window::{self, CompositionWindow};

pub struct TsfClient<'a> {
    pub ec: u32,
    pub context: &'a ITfContext,
    pub composition: &'a mut Option<ITfComposition>,
    pub sink: &'a ITfCompositionSink,
    /// The display attribute's atom (0: none registered).
    pub atom: u32,
    /// Where the marked text goes when the context stands in for an IMM32 app (`is_imm32`). The
    /// compatibility layer shows a Korean composition as the one syllable Korean IMEs compose,
    /// and only its first character: there the word is shown in this window, and goes into the
    /// field when it ends.
    pub window: Option<&'a mut CompositionWindow>,
}

/// `TS_STATUS::dwStaticFlags`.
const TS_SS_TRANSITORY: u32 = 0x4;
const TS_SS_NOHIDDENTEXT: u32 = 0x8;

/// The context is TSF's stand-in for an IMM32 app (its compatibility layer, CUAS). Those are
/// transitory contexts; Chrome marks its own text store transitory too, but it shows a whole
/// composition itself, and it says it has no hidden text, which the stand-ins don't.
pub fn is_imm32(context: &ITfContext) -> bool {
    unsafe { context.GetStatus() }
        .is_ok_and(|s| s.dwStaticFlags & (TS_SS_TRANSITORY | TS_SS_NOHIDDENTEXT) == TS_SS_TRANSITORY)
}

/// What the context is, for the log: its status flags, the window it is for, whether that window
/// has an IMM32 context, and the transitory extension compartment of its document.
pub fn describe(context: &ITfContext) -> String {
    use windows::Win32::UI::Input::Ime::{ImmGetContext, ImmReleaseContext};
    use windows::Win32::UI::TextServices::{ITfCompartmentMgr, GUID_COMPARTMENT_TRANSITORYEXTENSION};
    use windows::Win32::UI::WindowsAndMessaging::RealGetWindowClassW;
    unsafe {
        let (static_flags, dynamic_flags) = context.GetStatus().map_or((0, 0), |s| (s.dwStaticFlags, s.dwDynamicFlags));
        let hwnd = context.GetActiveView().and_then(|v| v.GetWnd()).unwrap_or_default();
        let mut class = [0u16; 64];
        let n = RealGetWindowClassW(hwnd, &mut class) as usize;
        let imc = ImmGetContext(hwnd);
        let has_imc = !imc.is_invalid();
        if has_imc {
            let _ = ImmReleaseContext(hwnd, imc);
        }
        let extension = context
            .GetDocumentMgr()
            .and_then(|d| d.cast::<ITfCompartmentMgr>())
            .and_then(|c| c.GetCompartment(&GUID_COMPARTMENT_TRANSITORYEXTENSION))
            .and_then(|c| c.GetValue())
            .map(|v| format!("{:?}", v.Anonymous.Anonymous.vt))
            .unwrap_or_else(|e| format!("{e}"));
        format!(
            "context: static {static_flags:#x} dynamic {dynamic_flags:#x}, window {}, IMM32 context {has_imc}, transitory extension {extension}",
            String::from_utf16_lossy(&class[..n])
        )
    }
}

fn utf16(text: &str) -> Vec<u16> {
    text.encode_utf16().collect()
}

impl TsfClient<'_> {
    fn range_at(&self, location: usize, length: usize) -> Result<ITfRange> {
        let range = unsafe { self.context.GetStart(self.ec)? };
        let acp: ITfRangeACP = range.cast()?;
        unsafe { acp.SetExtent(location as i32, length as i32)? };
        Ok(range)
    }

    fn extent(range: &ITfRange) -> Result<TextRange> {
        let acp: ITfRangeACP = range.cast()?;
        let (mut start, mut length) = (0i32, 0i32);
        unsafe { acp.GetExtent(&mut start, &mut length)? };
        Ok(TextRange::new(start.max(0) as usize, length.max(0) as usize))
    }

    fn caret_after(&self, range: &ITfRange) -> Result<()> {
        unsafe {
            let caret = range.Clone()?;
            caret.Collapse(self.ec, TF_ANCHOR_END)?;
            let mut selection = TF_SELECTION {
                range: ManuallyDrop::new(Some(caret)),
                style: TF_SELECTIONSTYLE { ase: TF_AE_NONE, fInterimChar: false.into() },
            };
            let result = self.context.SetSelection(self.ec, std::slice::from_ref(&selection));
            ManuallyDrop::drop(&mut selection.range);
            result
        }
    }

    fn underline(&self, range: &ITfRange) -> Result<()> {
        if self.atom == 0 {
            return Ok(());
        }
        unsafe {
            let property = self.context.GetProperty(&GUID_PROP_ATTRIBUTE)?;
            let mut value = VARIANT::default();
            (*value.Anonymous.Anonymous).vt = VT_I4;
            (*value.Anonymous.Anonymous).Anonymous.lVal = self.atom as i32;
            property.SetValue(self.ec, range, &value)
        }
    }

    fn clear_underline(&self, range: &ITfRange) {
        unsafe {
            if let Ok(property) = self.context.GetProperty(&GUID_PROP_ATTRIBUTE) {
                let _ = property.Clear(self.ec, range);
            }
        }
    }

    fn try_set_marked(&mut self, text: &str, replacing: Option<TextRange>) -> Result<()> {
        if self.window.is_some() {
            // Nothing before the caret can be read there, so nothing is replaced either.
            let caret = if text.is_empty() { None } else { self.caret() };
            let window = self.window.as_deref_mut().unwrap();
            match caret {
                Some((caret, field)) => window.show(text, caret, field),
                None => window.hide(),
            }
            return Ok(());
        }
        unsafe {
            if text.is_empty() && replacing.is_none() {
                if let Some(composition) = self.composition.take() {
                    let range = composition.GetRange()?;
                    range.SetText(self.ec, 0, &[])?;
                    composition.EndComposition(self.ec)?;
                }
                return Ok(());
            }
            let range = match self.composition.as_ref() {
                Some(composition) => {
                    if let Some(replacing) = replacing {
                        let start = self.range_at(replacing.location, 0)?;
                        composition.ShiftStart(self.ec, &start)?;
                    }
                    composition.GetRange()?
                }
                None => {
                    let start = match replacing {
                        Some(r) => self.range_at(r.location, r.length)?,
                        None => {
                            let insert: ITfInsertAtSelection = self.context.cast()?;
                            insert.InsertTextAtSelection(self.ec, TF_IAS_QUERYONLY, &[])?
                        }
                    };
                    let contexts: ITfContextComposition = self.context.cast()?;
                    let composition = contexts.StartComposition(self.ec, &start, self.sink)?;
                    let range = composition.GetRange()?;
                    *self.composition = Some(composition);
                    range
                }
            };
            range.SetText(self.ec, 0, &utf16(text))?;
            let _ = self.underline(&range);
            self.caret_after(&range)
        }
    }

    /// Where the composition window goes: the field's caret, or where TSF says the selection is.
    fn caret(&self) -> Option<(RECT, HWND)> {
        if let Some(caret) = composition_window::system_caret() {
            return Some(caret);
        }
        let field = unsafe { GetFocus() };
        let selection = self.selection().ok()??;
        let mut rect = RECT::default();
        let mut clipped = BOOL(0);
        unsafe {
            let view = self.context.GetActiveView().ok()?;
            view.GetTextExt(self.ec, &selection, &mut rect, &mut clipped).ok()?;
        }
        (!field.is_invalid()).then_some((rect, field))
    }

    fn try_insert(&mut self, text: &str, replacing: Option<TextRange>) -> Result<()> {
        if let Some(window) = self.window.as_deref_mut() {
            window.hide();
        }
        unsafe {
            match (replacing, self.composition.take()) {
                (None, Some(composition)) => {
                    let range = composition.GetRange()?;
                    range.SetText(self.ec, 0, &utf16(text))?;
                    self.clear_underline(&range);
                    let _ = self.caret_after(&range);
                    composition.EndComposition(self.ec)
                }
                (None, None) => {
                    let insert: ITfInsertAtSelection = self.context.cast()?;
                    let range = insert.InsertTextAtSelection(self.ec, INSERT_TEXT_AT_SELECTION_FLAGS(0), &utf16(text))?;
                    self.caret_after(&range)
                }
                (Some(replacing), composition) => {
                    // A committed word changed in place: nothing is being composed then, but if
                    // something is, it goes in first.
                    if let Some(composition) = composition {
                        let _ = composition.EndComposition(self.ec);
                    }
                    let range = self.range_at(replacing.location, replacing.length)?;
                    range.SetText(self.ec, 0, &utf16(text))?;
                    self.caret_after(&range)
                }
            }
        }
    }

    fn selection(&self) -> Result<Option<ITfRange>> {
        unsafe {
            let mut selection = [TF_SELECTION { range: ManuallyDrop::new(None), style: TF_SELECTIONSTYLE::default() }];
            let mut fetched = 0u32;
            self.context.GetSelection(self.ec, TF_DEFAULT_SELECTION, &mut selection, &mut fetched)?;
            let range = ManuallyDrop::take(&mut selection[0].range);
            Ok(range.filter(|_| fetched == 1))
        }
    }

    fn try_selection(&self) -> Result<Option<TextRange>> {
        self.selection()?.map(|range| Self::extent(&range)).transpose()
    }

    fn try_text(&self, range: TextRange) -> Result<String> {
        let r = self.range_at(range.location, range.length)?;
        let mut buffer = vec![0u16; range.length + 1];
        let mut fetched = 0u32;
        unsafe { r.GetText(self.ec, 0, &mut buffer, &mut fetched)? };
        Ok(String::from_utf16_lossy(&buffer[..(fetched as usize).min(buffer.len())]))
    }
}

impl TextClient for TsfClient<'_> {
    fn set_marked_text(&mut self, text: &str) {
        if let Err(e) = self.try_set_marked(text, None) {
            crate::log::write(&format!("set marked text failed: {e}"));
        }
    }

    fn set_marked_text_replacing(&mut self, text: &str, range: TextRange) {
        if let Err(e) = self.try_set_marked(text, Some(range)) {
            crate::log::write(&format!("set marked text (replacing) failed: {e}"));
        }
    }

    fn insert_text(&mut self, text: &str, range: Option<TextRange>) {
        if let Err(e) = self.try_insert(text, range) {
            crate::log::write(&format!("insert text failed: {e}"));
        }
    }

    fn selected_range(&mut self) -> Option<TextRange> {
        self.try_selection().ok().flatten()
    }

    fn marked_range(&mut self) -> Option<TextRange> {
        let composition = self.composition.as_ref()?;
        let range = unsafe { composition.GetRange().ok()? };
        Self::extent(&range).ok()
    }

    fn text_in(&mut self, range: TextRange) -> Option<String> {
        let text = self.try_text(range).ok()?;
        // Shorter than asked: the range ran past the text, which a field that can't be read
        // also looks like. Only the whole range is an answer.
        (araso_core::text::utf16_len(&text) == range.length).then_some(text)
    }
}
