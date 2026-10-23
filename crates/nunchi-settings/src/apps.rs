//! The apps open now (to pick one for a rule), and bringing an open settings window forward.

use std::collections::BTreeSet;

use windows::core::{BOOL, HSTRING, PWSTR};
use windows::Win32::Foundation::{CloseHandle, HWND, LPARAM};
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::WindowsAndMessaging::*;

/// The executables (lowercase names, as apps.json keys them) of the windows on screen.
pub fn running() -> Vec<String> {
    extern "system" fn visit(hwnd: HWND, found: LPARAM) -> BOOL {
        unsafe {
            if IsWindowVisible(hwnd).as_bool() && GetWindowTextLengthW(hwnd) > 0 {
                let mut pid = 0u32;
                GetWindowThreadProcessId(hwnd, Some(&mut pid));
                if let Some(name) = executable(pid) {
                    (*(found.0 as *mut BTreeSet<String>)).insert(name);
                }
            }
        }
        true.into()
    }
    let mut found = BTreeSet::new();
    unsafe {
        let _ = EnumWindows(Some(visit), LPARAM(&mut found as *mut BTreeSet<String> as isize));
    }
    found.remove("nunchi-settings.exe");
    found.into_iter().collect()
}

fn executable(pid: u32) -> Option<String> {
    unsafe {
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buffer = [0u16; 1024];
        let mut length = buffer.len() as u32;
        let named = QueryFullProcessImageNameW(process, PROCESS_NAME_WIN32, PWSTR(buffer.as_mut_ptr()), &mut length);
        let _ = CloseHandle(process);
        named.ok()?;
        let path = String::from_utf16_lossy(&buffer[..length as usize]);
        path.rsplit('\\').next().map(str::to_lowercase)
    }
}

/// A message box saying what went wrong.
pub fn tell_failure(title: &str, message: &str) {
    unsafe {
        let _ = MessageBoxW(None, &HSTRING::from(message), &HSTRING::from(title), MB_OK | MB_ICONWARNING);
    }
}

/// The settings window already open comes to the front. Whether there was one.
pub fn bring_forward(title: &str) -> bool {
    unsafe {
        let Ok(window) = FindWindowW(None, &HSTRING::from(title)) else { return false };
        if window.is_invalid() {
            return false;
        }
        if IsIconic(window).as_bool() {
            let _ = ShowWindow(window, SW_RESTORE);
        }
        let _ = SetForegroundWindow(window);
        true
    }
}
