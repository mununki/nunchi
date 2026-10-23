//! Putting Nunchi in the user's keyboard list (Settings → Language → Korean → Keyboards) and
//! taking it out, for the installer and the development scripts:
//!
//!   rundll32 nunchi_tsf.dll,AddToKeyboardList          add it
//!   rundll32 nunchi_tsf.dll,AddToKeyboardList default  add it and make it the default input method
//!   rundll32 nunchi_tsf.dll,RemoveFromKeyboardList     take it out
//!
//! Run as the user whose list it is (not elevated as another account). The list is changed with
//! input.dll's `InstallLayoutOrTip`, the documented way for an input method to add itself.

use std::ffi::CStr;

use windows::core::{s, w, BOOL, PCSTR, PCWSTR};
use windows::Win32::Foundation::{HINSTANCE, HWND};
use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};

use crate::{CLSID_NUNCHI, GUID_PROFILE, LANGID_KOREAN};

/// `ILOT_UNINSTALL`: take the input method out of the list.
const ILOT_UNINSTALL: u32 = 0x1;
/// `ILOT_DEFPROFILE`: make it the user's default input method too.
const ILOT_DEFPROFILE: u32 = 0x2;

type InstallLayoutOrTip = unsafe extern "system" fn(PCWSTR, u32) -> BOOL;

/// "0412:{class}{profile}", as `InstallLayoutOrTip` names an input method.
fn tip() -> Vec<u16> {
    let name = format!("{:04X}:{{{:?}}}{{{:?}}}", LANGID_KOREAN, CLSID_NUNCHI, GUID_PROFILE);
    name.encode_utf16().chain(std::iter::once(0)).collect()
}

fn install_layout_or_tip(flags: u32) -> bool {
    unsafe {
        let Ok(input) = LoadLibraryW(w!("input.dll")) else { return false };
        let Some(function) = GetProcAddress(input, s!("InstallLayoutOrTip")) else { return false };
        let function: InstallLayoutOrTip = std::mem::transmute(function);
        let tip = tip();
        let ok = function(PCWSTR(tip.as_ptr()), flags).as_bool();
        crate::log::write(&format!("keyboard list flags={flags:#x} ok={ok}"));
        ok
    }
}

fn arguments(command_line: PCSTR) -> String {
    if command_line.is_null() {
        return String::new();
    }
    unsafe { CStr::from_ptr(command_line.0 as *const _) }.to_string_lossy().to_lowercase()
}

#[no_mangle]
pub extern "system" fn AddToKeyboardList(_window: HWND, _instance: HINSTANCE, command_line: PCSTR, _show: i32) {
    let default = arguments(command_line).split_whitespace().any(|a| a == "default");
    install_layout_or_tip(if default { ILOT_DEFPROFILE } else { 0 });
}

#[no_mangle]
pub extern "system" fn RemoveFromKeyboardList(_window: HWND, _instance: HINSTANCE, _command_line: PCSTR, _show: i32) {
    install_layout_or_tip(ILOT_UNINSTALL);
}
