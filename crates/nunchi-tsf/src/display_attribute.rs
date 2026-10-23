//! How the word being typed looks: underlined, in the text's own color, as macOS shows marked text.

use std::cell::Cell;

use windows::core::{implement, BSTR, GUID};
use windows::Win32::Foundation::{E_INVALIDARG, S_FALSE};
use windows::Win32::UI::TextServices::{
    IEnumTfDisplayAttributeInfo, IEnumTfDisplayAttributeInfo_Impl, ITfDisplayAttributeInfo,
    ITfDisplayAttributeInfo_Impl, TF_ATTR_INPUT, TF_CT_NONE, TF_DA_COLOR, TF_DISPLAYATTRIBUTE, TF_LS_SOLID,
};

use crate::GUID_DISPLAY_ATTRIBUTE_INPUT;

pub fn input() -> TF_DISPLAYATTRIBUTE {
    TF_DISPLAYATTRIBUTE {
        crText: TF_DA_COLOR { r#type: TF_CT_NONE, ..Default::default() },
        crBk: TF_DA_COLOR { r#type: TF_CT_NONE, ..Default::default() },
        lsStyle: TF_LS_SOLID,
        fBoldLine: false.into(),
        crLine: TF_DA_COLOR { r#type: TF_CT_NONE, ..Default::default() },
        bAttr: TF_ATTR_INPUT,
    }
}

#[implement(ITfDisplayAttributeInfo)]
pub struct DisplayAttributeInfo;

impl ITfDisplayAttributeInfo_Impl for DisplayAttributeInfo_Impl {
    fn GetGUID(&self) -> windows::core::Result<GUID> {
        Ok(GUID_DISPLAY_ATTRIBUTE_INPUT)
    }

    fn GetDescription(&self) -> windows::core::Result<BSTR> {
        Ok(BSTR::from("Nunchi: the word being typed"))
    }

    fn GetAttributeInfo(&self, attribute: *mut TF_DISPLAYATTRIBUTE) -> windows::core::Result<()> {
        if attribute.is_null() {
            return Err(E_INVALIDARG.into());
        }
        unsafe { *attribute = input() };
        Ok(())
    }

    fn SetAttributeInfo(&self, _attribute: *const TF_DISPLAYATTRIBUTE) -> windows::core::Result<()> {
        Ok(())
    }

    fn Reset(&self) -> windows::core::Result<()> {
        Ok(())
    }
}

/// Lists the one attribute.
#[implement(IEnumTfDisplayAttributeInfo)]
pub struct EnumDisplayAttributeInfo {
    pub index: Cell<u32>,
}

impl IEnumTfDisplayAttributeInfo_Impl for EnumDisplayAttributeInfo_Impl {
    fn Clone(&self) -> windows::core::Result<IEnumTfDisplayAttributeInfo> {
        Ok(EnumDisplayAttributeInfo { index: Cell::new(self.index.get()) }.into())
    }

    fn Next(&self, count: u32, infos: *mut Option<ITfDisplayAttributeInfo>, fetched: *mut u32) -> windows::core::Result<()> {
        let mut n = 0u32;
        if count > 0 && self.index.get() == 0 && !infos.is_null() {
            unsafe { *infos = Some(DisplayAttributeInfo.into()) };
            self.index.set(1);
            n = 1;
        }
        if !fetched.is_null() {
            unsafe { *fetched = n };
        }
        if n == count {
            Ok(())
        } else {
            Err(S_FALSE.into())
        }
    }

    fn Reset(&self) -> windows::core::Result<()> {
        self.index.set(0);
        Ok(())
    }

    fn Skip(&self, count: u32) -> windows::core::Result<()> {
        if count > 0 && self.index.get() == 0 {
            self.index.set(1);
            if count == 1 {
                return Ok(());
            }
        }
        Err(S_FALSE.into())
    }
}
