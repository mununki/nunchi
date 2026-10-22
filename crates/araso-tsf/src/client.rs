//! `TextClient` over a TSF context inside an edit session: the marked text is Araso's
//! composition (underlined with its display attribute), and ranges are ACP offsets, UTF-16 units
//! as `TextRange` counts them. A field that can't give ACP offsets answers "not known".

use std::mem::ManuallyDrop;

use araso_core::{TextClient, TextRange};
use windows::core::{Interface, Result};
use windows::Win32::System::Variant::{VARIANT, VT_I4};
use windows::Win32::UI::TextServices::{
    ITfComposition, ITfCompositionSink, ITfContext, ITfContextComposition, ITfInsertAtSelection, ITfRange,
    ITfRangeACP, GUID_PROP_ATTRIBUTE, INSERT_TEXT_AT_SELECTION_FLAGS, TF_AE_NONE, TF_ANCHOR_END,
    TF_DEFAULT_SELECTION, TF_IAS_QUERYONLY, TF_SELECTION, TF_SELECTIONSTYLE,
};

pub struct TsfClient<'a> {
    pub ec: u32,
    pub context: &'a ITfContext,
    pub composition: &'a mut Option<ITfComposition>,
    pub sink: &'a ITfCompositionSink,
    /// The display attribute's atom (0: none registered).
    pub atom: u32,
    /// The context stands in for an IMM32 app (TSF's transitory contexts): there a Korean
    /// composition has to be selected as an interim character, as Korean IMEs mark the syllable
    /// they compose, or the compatibility layer ends it after every key.
    pub transitory: bool,
}

/// `TS_SS_TRANSITORY` of `TS_STATUS::dwStaticFlags`.
const TS_SS_TRANSITORY: u32 = 0x4;

pub fn is_transitory(context: &ITfContext) -> bool {
    unsafe { context.GetStatus() }.is_ok_and(|s| s.dwStaticFlags & TS_SS_TRANSITORY != 0)
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

    /// The caret after `range`, or with `interim` the range selected as the interim character.
    fn caret_after(&self, range: &ITfRange, interim: bool) -> Result<()> {
        unsafe {
            let caret = range.Clone()?;
            if !interim {
                caret.Collapse(self.ec, TF_ANCHOR_END)?;
            }
            let mut selection = TF_SELECTION {
                range: ManuallyDrop::new(Some(caret)),
                style: TF_SELECTIONSTYLE { ase: TF_AE_NONE, fInterimChar: interim.into() },
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
        unsafe {
            if text.is_empty() && replacing.is_none() {
                if let Some(composition) = self.composition.take() {
                    let range = composition.GetRange()?;
                    range.SetText(self.ec, 0, &[])?;
                    composition.EndComposition(self.ec)?;
                }
                return Ok(());
            }
            let (range, started) = match self.composition.as_ref() {
                Some(composition) => {
                    if let Some(replacing) = replacing {
                        let start = self.range_at(replacing.location, 0)?;
                        composition.ShiftStart(self.ec, &start)?;
                    }
                    (composition.GetRange()?, false)
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
                    (range, true)
                }
            };
            let text = utf16(text);
            if !self.transitory {
                range.SetText(self.ec, 0, &text)?;
                let _ = self.underline(&range);
                return self.caret_after(&range, false);
            }
            // An IMM32 app sees a Korean composition through the compatibility layer, which ends
            // it after every key unless it is selected as an interim character. An interim
            // selection must start as one character; it then grows and shrinks with the word.
            if started {
                range.SetText(self.ec, 0, &text[..1])?;
                let _ = self.caret_after(&range, true);
                if text.len() > 1 {
                    range.SetText(self.ec, 0, &text)?;
                }
            } else {
                range.SetText(self.ec, 0, &text)?;
            }
            let _ = self.underline(&range);
            Ok(())
        }
    }

    fn try_insert(&mut self, text: &str, replacing: Option<TextRange>) -> Result<()> {
        unsafe {
            match (replacing, self.composition.take()) {
                (None, Some(composition)) => {
                    let range = composition.GetRange()?;
                    range.SetText(self.ec, 0, &utf16(text))?;
                    self.clear_underline(&range);
                    let _ = self.caret_after(&range, false);
                    composition.EndComposition(self.ec)
                }
                (None, None) => {
                    let insert: ITfInsertAtSelection = self.context.cast()?;
                    let range = insert.InsertTextAtSelection(self.ec, INSERT_TEXT_AT_SELECTION_FLAGS(0), &utf16(text))?;
                    self.caret_after(&range, false)
                }
                (Some(replacing), composition) => {
                    // A committed word changed in place: nothing is being composed then, but if
                    // something is, it goes in first.
                    if let Some(composition) = composition {
                        let _ = composition.EndComposition(self.ec);
                    }
                    let range = self.range_at(replacing.location, replacing.length)?;
                    range.SetText(self.ec, 0, &utf16(text))?;
                    self.caret_after(&range, false)
                }
            }
        }
    }

    fn try_selection(&self) -> Result<Option<TextRange>> {
        unsafe {
            let mut selection = [TF_SELECTION { range: ManuallyDrop::new(None), style: TF_SELECTIONSTYLE::default() }];
            let mut fetched = 0u32;
            self.context.GetSelection(self.ec, TF_DEFAULT_SELECTION, &mut selection, &mut fetched)?;
            let range = ManuallyDrop::take(&mut selection[0].range);
            match (fetched, range) {
                (1, Some(range)) => Ok(Some(Self::extent(&range)?)),
                _ => Ok(None),
            }
        }
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
