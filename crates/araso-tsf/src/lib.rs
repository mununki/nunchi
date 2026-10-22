//! Araso as a TSF text input processor: an in-process COM server that Windows loads into every
//! app with a text field while Araso is the chosen input method. All typing logic is
//! `araso_core::InputSession`; this crate turns key events into its keys and its text changes
//! into TSF compositions.

#![allow(non_snake_case)]

mod class_factory;
mod client;
mod display_attribute;
mod edit_session;
mod keys;
mod log;
mod register;
mod runtime;
mod text_service;

use std::ffi::c_void;
use std::sync::atomic::{AtomicIsize, AtomicU32, Ordering};

use windows::core::{Interface, GUID, HRESULT};
use windows::Win32::Foundation::{
    CLASS_E_CLASSNOTAVAILABLE, E_FAIL, E_POINTER, HINSTANCE, S_FALSE, S_OK,
};
use windows::Win32::System::Com::IClassFactory;
use windows::Win32::System::SystemServices::DLL_PROCESS_ATTACH;

/// The text service's COM class.
pub const CLSID_ARASO: GUID = GUID::from_u128(0xae988ccd_6a62_424c_a948_91d4d3b4c13a);
/// Its one profile, under Korean.
pub const GUID_PROFILE: GUID = GUID::from_u128(0xc37c500f_1136_464d_bc89_601a354fc632);
/// The underline of the word being typed.
pub const GUID_DISPLAY_ATTRIBUTE_INPUT: GUID = GUID::from_u128(0x79b7faca_ddcb_4f9a_86ae_2fdd4f6e655f);
/// The 한/영 key, kept for Araso (switches the word's reading).
pub const GUID_PRESERVED_HANGUL: GUID = GUID::from_u128(0x73d8d15a_c3dd_4499_91b1_c1b7797c32be);
/// The 한자 key, kept for Araso (pauses it in the text field).
pub const GUID_PRESERVED_HANJA: GUID = GUID::from_u128(0xbdf16c77_80d3_4611_8ff1_7b4948e64362);

/// LANGID of Korean (Korea): Araso is listed among the Korean keyboards.
pub const LANGID_KOREAN: u16 = 0x0412;

static MODULE: AtomicIsize = AtomicIsize::new(0);
/// Live COM objects and server locks: the DLL may unload when none are left.
static REFERENCES: AtomicU32 = AtomicU32::new(0);

pub fn module() -> HINSTANCE {
    HINSTANCE(MODULE.load(Ordering::Relaxed) as *mut c_void)
}

pub fn add_ref() {
    REFERENCES.fetch_add(1, Ordering::SeqCst);
}

pub fn release() {
    REFERENCES.fetch_sub(1, Ordering::SeqCst);
}

#[no_mangle]
extern "system" fn DllMain(instance: HINSTANCE, reason: u32, _reserved: *mut c_void) -> i32 {
    if reason == DLL_PROCESS_ATTACH {
        MODULE.store(instance.0 as isize, Ordering::Relaxed);
    }
    1
}

/// # Safety
/// COM calls it with valid pointers.
#[no_mangle]
pub unsafe extern "system" fn DllGetClassObject(rclsid: *const GUID, riid: *const GUID, ppv: *mut *mut c_void) -> HRESULT {
    if ppv.is_null() || rclsid.is_null() || riid.is_null() {
        return E_POINTER;
    }
    *ppv = std::ptr::null_mut();
    if *rclsid != CLSID_ARASO {
        return CLASS_E_CLASSNOTAVAILABLE;
    }
    let factory: IClassFactory = class_factory::ClassFactory.into();
    factory.query(riid, ppv)
}

#[no_mangle]
pub extern "system" fn DllCanUnloadNow() -> HRESULT {
    if REFERENCES.load(Ordering::SeqCst) == 0 {
        S_OK
    } else {
        S_FALSE
    }
}

#[no_mangle]
pub extern "system" fn DllRegisterServer() -> HRESULT {
    match register::register() {
        Ok(()) => S_OK,
        Err(e) => {
            let _ = register::unregister();
            if e.code().is_err() { e.code() } else { E_FAIL }
        }
    }
}

#[no_mangle]
pub extern "system" fn DllUnregisterServer() -> HRESULT {
    match register::unregister() {
        Ok(()) => S_OK,
        Err(e) => e.code(),
    }
}

/// Runs a COM method body, turning a panic into E_FAIL instead of unwinding into the host app.
pub fn guard<T>(f: impl FnOnce() -> windows::core::Result<T>) -> windows::core::Result<T> {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)) {
        Ok(result) => result,
        Err(panic) => {
            let message = panic
                .downcast_ref::<&str>()
                .map(|s| s.to_string())
                .or_else(|| panic.downcast_ref::<String>().cloned())
                .unwrap_or_default();
            log::write(&format!("panic: {message}"));
            Err(E_FAIL.into())
        }
    }
}
