//! The class factory COM asks for the text service.

use std::ffi::c_void;

use windows::core::{implement, Interface, Ref, BOOL, GUID};
use windows::Win32::Foundation::{CLASS_E_NOAGGREGATION, E_POINTER};
use windows::Win32::System::Com::{IClassFactory, IClassFactory_Impl};
use windows::Win32::UI::TextServices::ITfTextInputProcessor;
use windows_core::IUnknown;

use crate::text_service::TextService;

#[implement(IClassFactory)]
pub struct ClassFactory;

impl IClassFactory_Impl for ClassFactory_Impl {
    fn CreateInstance(&self, outer: Ref<IUnknown>, iid: *const GUID, object: *mut *mut c_void) -> windows::core::Result<()> {
        crate::guard(|| unsafe {
            if object.is_null() || iid.is_null() {
                return Err(E_POINTER.into());
            }
            *object = std::ptr::null_mut();
            if outer.is_some() {
                return Err(CLASS_E_NOAGGREGATION.into());
            }
            let service: ITfTextInputProcessor = TextService::new().into();
            service.query(iid, object).ok()
        })
    }

    fn LockServer(&self, lock: BOOL) -> windows::core::Result<()> {
        if lock.as_bool() {
            crate::add_ref();
        } else {
            crate::release();
        }
        Ok(())
    }
}
