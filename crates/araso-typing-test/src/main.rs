//! Types each case of a script as real key events (SendInput) into an edit control of its own,
//! with Araso active in this process only, and compares the control's text with what the case
//! expects. Araso must be registered (scripts\dev-register.ps1).
//!
//!   araso-typing-test                 the cases in cases/ (built in) that are for the field
//!   araso-typing-test cases.tsv …     these scripts instead: "keys<TAB>expected<TAB>fields" per line
//!   araso-typing-test --richedit      a RichEdit (TSF-aware) control instead of a plain EDIT (CUAS)
//!   araso-typing-test --richedit-imm  a RichEdit left to IMM32 (KakaoTalk's chat input)
//!   araso-typing-test --imm           a field that does its own IMM32 handling (Qt, Java, games)
//!   araso-typing-test --dev           the development build, "Araso (개발)", not the installed Araso
//!   araso-typing-test --appcontainer  the same, inside the sandbox Store apps run in
//!   araso-typing-test --log           with Araso's diagnostic log at the end
//!   araso-typing-test --browser       into a textarea in Chrome (or Edge), with the input method chosen there
//!
//! In the keys: "⎋" Esc, "⇧" Shift+Esc, "⏎" Return, "⌫" Backspace, "⇥" Tab, "↶" Ctrl+Z, "⇤" the caret
//! to the start of the field (as a click there).
//! It takes the keyboard focus for a few seconds; it stops at once if its window loses it, so
//! no key goes anywhere else.

use std::time::{Duration, Instant};

use windows::core::{w, GUID, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED};
use windows::Win32::System::LibraryLoader::{GetModuleHandleW, LoadLibraryW};
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::TextServices::{
    ITfInputProcessorProfileMgr, CLSID_TF_InputProcessorProfiles, TF_IPPMF_FORPROCESS, TF_PROFILETYPE_INPUTPROCESSOR,
};
use windows::Win32::UI::WindowsAndMessaging::*;

/// The installed Araso's class and profile, and those of the development build ("Araso (개발)",
/// `--dev`), as crates/araso-tsf/src/lib.rs has them.
const CLSID_ARASO: GUID = GUID::from_u128(0xae988ccd_6a62_424c_a948_91d4d3b4c13a);
const GUID_PROFILE: GUID = GUID::from_u128(0xc37c500f_1136_464d_bc89_601a354fc632);
const CLSID_ARASO_DEV: GUID = GUID::from_u128(0xaeca5114_3633_4707_bc7d_67ca80de068c);
const GUID_PROFILE_DEV: GUID = GUID::from_u128(0x71fe9eda_947f_4439_818b_bdd4d8c0af97);

/// The scripts in cases/, built in: run when no script is named.
const SCRIPTS: &[&str] = &[
    include_str!("../cases/basic.tsv"),
    include_str!("../cases/mid-word.tsv"),
    include_str!("../cases/undo.tsv"),
    // Last: a word switched is learned, and that changes the cases after it.
    include_str!("../cases/switch.tsv"),
];

/// A case of a script: the keys, what the field shows after them, and the kinds of field it is
/// for ("-imm": all but that one; "edit,richedit-imm": only those; empty: all).
struct Case {
    keys: String,
    expected: String,
    fields: String,
}

impl Case {
    fn applies_to(&self, field: &str) -> bool {
        let names: Vec<&str> = self.fields.split(',').map(str::trim).filter(|n| !n.is_empty()).collect();
        if names.iter().any(|n| n.strip_prefix('-') == Some(field)) {
            return false;
        }
        let only: Vec<&&str> = names.iter().filter(|n| !n.starts_with('-')).collect();
        only.is_empty() || only.iter().any(|n| **n == field)
    }
}

/// "keys<TAB>expected<TAB>fields" per line ("\n" in the expected text is a line break); lines
/// starting with "#" are comments.
fn parse_cases(script: &str) -> Vec<Case> {
    script
        .lines()
        .filter(|line| !line.starts_with('#') && !line.trim().is_empty())
        .filter_map(|line| {
            let mut columns = line.splitn(3, '\t');
            let keys = columns.next()?.to_string();
            let expected = columns.next()?.replace("\\n", "\n");
            let fields = columns.next().unwrap_or("").trim().to_string();
            Some(Case { keys, expected, fields })
        })
        .collect()
}

thread_local! {
    /// The text of the `--imm` field: what went in, and the composition shown at its end.
    static IMM_TEXT: std::cell::RefCell<(String, String)> = Default::default();
}

/// A field that does its own IMM32 handling, as Qt, Java and games do: the result string goes in,
/// the composition string is shown after it, characters are typed.
extern "system" fn imm_field_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    use windows::Win32::UI::Input::Ime::{ImmGetCompositionStringW, ImmGetContext, ImmReleaseContext, GCS_COMPSTR, GCS_RESULTSTR};
    unsafe {
        let read = |kind| {
            let imc = ImmGetContext(hwnd);
            let bytes = ImmGetCompositionStringW(imc, kind, None, 0);
            let mut buffer = vec![0u16; (bytes.max(0) as usize) / 2];
            if bytes > 0 {
                ImmGetCompositionStringW(imc, kind, Some(buffer.as_mut_ptr() as *mut _), bytes as u32);
            }
            let _ = ImmReleaseContext(hwnd, imc);
            String::from_utf16_lossy(&buffer)
        };
        match msg {
            WM_IME_STARTCOMPOSITION | WM_IME_ENDCOMPOSITION => {
                if msg == WM_IME_ENDCOMPOSITION {
                    IMM_TEXT.with(|t| t.borrow_mut().1.clear());
                }
                return LRESULT(0);
            }
            WM_IME_COMPOSITION => {
                let flags = lparam.0 as u32;
                if flags & GCS_RESULTSTR.0 != 0 {
                    let result = read(GCS_RESULTSTR);
                    IMM_TEXT.with(|t| t.borrow_mut().0.push_str(&result));
                }
                let composition = if flags & GCS_COMPSTR.0 != 0 { read(GCS_COMPSTR) } else { String::new() };
                IMM_TEXT.with(|t| t.borrow_mut().1 = composition);
                return LRESULT(0);
            }
            WM_CHAR => {
                if let Some(c) = char::from_u32(wparam.0 as u32) {
                    IMM_TEXT.with(|t| {
                        let mut t = t.borrow_mut();
                        match c {
                            '\u{8}' => {
                                t.0.pop();
                            }
                            '\r' => t.0.push('\n'),
                            c if !c.is_control() => t.0.push(c),
                            _ => {}
                        }
                    });
                }
                return LRESULT(0);
            }
            WM_SETTEXT => {
                IMM_TEXT.with(|t| *t.borrow_mut() = Default::default());
                return LRESULT(1);
            }
            WM_GETTEXTLENGTH | WM_GETTEXT => {
                let text: Vec<u16> = IMM_TEXT.with(|t| {
                    let t = t.borrow();
                    format!("{}{}", t.0, t.1).encode_utf16().collect()
                });
                if msg == WM_GETTEXTLENGTH {
                    return LRESULT(text.len() as isize);
                }
                let room = wparam.0.saturating_sub(1).min(text.len());
                std::ptr::copy_nonoverlapping(text.as_ptr(), lparam.0 as *mut u16, room);
                *(lparam.0 as *mut u16).add(room) = 0;
                return LRESULT(room as isize);
            }
            _ => {}
        }
        DefWindowProcW(hwnd, msg, wparam, lparam)
    }
}

extern "system" fn window_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe {
        if msg == WM_DESTROY {
            PostQuitMessage(0);
            return LRESULT(0);
        }
        // An Alt let go on its own would start the menu's modal loop, and nothing would pump.
        if msg == WM_SYSCOMMAND && (wparam.0 & 0xFFF0) == SC_KEYMENU as usize {
            return LRESULT(0);
        }
        DefWindowProcW(hwnd, msg, wparam, lparam)
    }
}

fn pump(duration: Duration) {
    let until = Instant::now() + duration;
    loop {
        unsafe {
            let mut msg = MSG::default();
            while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
        if Instant::now() >= until {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
}

fn key_input(vk: VIRTUAL_KEY, up: bool) -> INPUT {
    let scan = unsafe { MapVirtualKeyW(vk.0 as u32, MAPVK_VK_TO_VSC) } as u16;
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT { wVk: vk, wScan: scan, dwFlags: if up { KEYEVENTF_KEYUP } else { KEYBD_EVENT_FLAGS(0) }, time: 0, dwExtraInfo: 0 },
        },
    }
}

/// The key and Shift for a character on a US keyboard.
fn key_for(ch: char) -> Option<(VIRTUAL_KEY, bool)> {
    Some(match ch {
        'a'..='z' => (VIRTUAL_KEY(ch.to_ascii_uppercase() as u16), false),
        'A'..='Z' => (VIRTUAL_KEY(ch as u16), true),
        '0'..='9' => (VIRTUAL_KEY(ch as u16), false),
        ' ' => (VK_SPACE, false),
        '⎋' => (VK_ESCAPE, false),
        '⇧' => (VK_ESCAPE, true),
        '⏎' => (VK_RETURN, false),
        '⌫' => (VK_BACK, false),
        '⇥' => (VK_TAB, false),
        _ => {
            let table = [
                (')', '0'), ('!', '1'), ('@', '2'), ('#', '3'), ('$', '4'), ('%', '5'), ('^', '6'), ('&', '7'), ('*', '8'),
                ('(', '9'),
            ];
            if let Some((_, d)) = table.iter().find(|(c, _)| *c == ch) {
                return Some((VIRTUAL_KEY(*d as u16), true));
            }
            let oem = [
                (';', ':', VK_OEM_1), ('=', '+', VK_OEM_PLUS), (',', '<', VK_OEM_COMMA), ('-', '_', VK_OEM_MINUS),
                ('.', '>', VK_OEM_PERIOD), ('/', '?', VK_OEM_2), ('`', '~', VK_OEM_3), ('[', '{', VK_OEM_4),
                ('\\', '|', VK_OEM_5), (']', '}', VK_OEM_6), ('\'', '"', VK_OEM_7),
            ];
            let (plain, _, vk) = oem.iter().find(|(p, s, _)| *p == ch || *s == ch)?;
            (*vk, *plain != ch)
        }
    })
}

fn type_keys(window: HWND, keys: &str) -> Result<(), String> {
    for ch in keys.chars() {
        if unsafe { GetForegroundWindow() } != window {
            return Err("the test window lost the keyboard focus; stopped".into());
        }
        if ch == '⇤' {
            // The caret to the start of the field, as a click there puts it.
            const EM_SETSEL: u32 = 0x00B1;
            unsafe { SendMessageW(GetFocus(), EM_SETSEL, Some(WPARAM(0)), Some(LPARAM(0))) };
            pump(Duration::from_millis(15));
            continue;
        }
        let (vk, shift, control) = match ch {
            '↶' => (VIRTUAL_KEY(b'Z' as u16), false, true),
            _ => match key_for(ch) {
                Some((vk, shift)) => (vk, shift, false),
                None => return Err(format!("no key for {ch:?}")),
            },
        };
        let mut inputs = Vec::new();
        if control {
            inputs.push(key_input(VK_LCONTROL, false));
        }
        if shift {
            inputs.push(key_input(VK_LSHIFT, false));
        }
        inputs.push(key_input(vk, false));
        inputs.push(key_input(vk, true));
        if shift {
            inputs.push(key_input(VK_LSHIFT, true));
        }
        if control {
            inputs.push(key_input(VK_LCONTROL, true));
        }
        unsafe { SendInput(&inputs, std::mem::size_of::<INPUT>() as i32) };
        pump(Duration::from_millis(15));
    }
    pump(Duration::from_millis(80));
    Ok(())
}

/// This process's private memory and working set, in KB.
fn memory() -> (usize, usize) {
    use windows::Win32::System::ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS, PROCESS_MEMORY_COUNTERS_EX};
    use windows::Win32::System::Threading::GetCurrentProcess;
    let mut counters = PROCESS_MEMORY_COUNTERS_EX { cb: std::mem::size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32, ..Default::default() };
    unsafe {
        let _ = GetProcessMemoryInfo(
            GetCurrentProcess(),
            &mut counters as *mut _ as *mut PROCESS_MEMORY_COUNTERS,
            counters.cb,
        );
    }
    (counters.PrivateUsage / 1024, counters.WorkingSetSize / 1024)
}

/// The Araso DLLs loaded in `process` (the installed one and the development build may both be).
fn araso_module(process: windows::Win32::Foundation::HANDLE) -> Option<String> {
    use windows::Win32::Foundation::HMODULE;
    use windows::Win32::System::ProcessStatus::{EnumProcessModules, GetModuleFileNameExW};
    let mut modules = vec![HMODULE::default(); 1024];
    let mut needed = 0u32;
    unsafe {
        EnumProcessModules(process, modules.as_mut_ptr(), (modules.len() * std::mem::size_of::<HMODULE>()) as u32, &mut needed).ok()?;
    }
    let count = (needed as usize / std::mem::size_of::<HMODULE>()).min(modules.len());
    let paths: Vec<String> = modules[..count]
        .iter()
        .filter_map(|&module| {
            let mut buffer = [0u16; 1024];
            let n = unsafe { GetModuleFileNameExW(Some(process), Some(module), &mut buffer) } as usize;
            let path = String::from_utf16_lossy(&buffer[..n]);
            let name = path.rsplit('\\').next().unwrap_or_default().to_lowercase();
            (name.starts_with("araso_tsf") || name.starts_with("araso_dev")).then_some(path)
        })
        .collect();
    (!paths.is_empty()).then(|| paths.join(", "))
}

/// The keyboard input method TSF has active in this thread, by name.
fn active_profile(profiles: &ITfInputProcessorProfileMgr) -> String {
    use windows::Win32::UI::TextServices::{GUID_TFCAT_TIP_KEYBOARD, TF_INPUTPROCESSORPROFILE};
    let mut profile = TF_INPUTPROCESSORPROFILE::default();
    match unsafe { profiles.GetActiveProfile(&GUID_TFCAT_TIP_KEYBOARD, &mut profile) } {
        Ok(()) if profile.clsid == CLSID_ARASO => "Araso (installed)".into(),
        Ok(()) if profile.clsid == CLSID_ARASO_DEV => "Araso (개발)".into(),
        Ok(()) => format!("another input method {:?}", profile.clsid),
        Err(e) => format!("unknown ({e})"),
    }
}

fn text_of(edit: HWND) -> String {
    unsafe {
        let length = GetWindowTextLengthW(edit) as usize;
        let mut buffer = vec![0u16; length + 1];
        let n = GetWindowTextW(edit, &mut buffer) as usize;
        String::from_utf16_lossy(&buffer[..n]).replace("\r\n", "\n").replace('\r', "\n")
    }
}

/// The word Araso shows in its composition window, where the field can't show it (IMM32 fields):
/// it stays out of the field until it ends.
fn composition_window_text() -> String {
    extern "system" fn visit(hwnd: HWND, found: LPARAM) -> windows::core::BOOL {
        unsafe {
            let mut class = [0u16; 64];
            let n = GetClassNameW(hwnd, &mut class) as usize;
            if String::from_utf16_lossy(&class[..n]) == "ArasoComposition" && IsWindowVisible(hwnd).as_bool() {
                *(found.0 as *mut String) = text_of(hwnd);
            }
        }
        true.into()
    }
    let mut found = String::new();
    unsafe {
        use windows::Win32::System::Threading::GetCurrentThreadId;
        let _ = EnumThreadWindows(GetCurrentThreadId(), Some(visit), LPARAM(&mut found as *mut String as isize));
    }
    found
}

/// Araso's mode icon in the taskbar's input indicator, as this thread's TSF has it: Araso's, with
/// an icon, saying "on", and a click pauses Araso and the next resumes it. What its tooltips said.
fn check_mode_icon(clsid: &GUID) -> Result<String, String> {
    use windows::core::Interface;
    use windows::Win32::Foundation::POINT;
    use windows::Win32::UI::TextServices::{
        ITfLangBarItemButton, ITfLangBarItemMgr, ITfThreadMgr, CLSID_TF_ThreadMgr, GUID_LBI_INPUTMODE, TF_LANGBARITEMINFO,
        TF_LBI_CLK_LEFT,
    };
    unsafe {
        let thread_mgr: ITfThreadMgr =
            CoCreateInstance(&CLSID_TF_ThreadMgr, None, CLSCTX_INPROC_SERVER).map_err(|e| format!("no TSF ({e})"))?;
        let items: ITfLangBarItemMgr = thread_mgr.cast().map_err(|e| e.to_string())?;
        let item = items.GetItem(&GUID_LBI_INPUTMODE).map_err(|e| format!("none ({e})"))?;
        let mut info = TF_LANGBARITEMINFO::default();
        item.GetInfo(&mut info).map_err(|e| e.to_string())?;
        if info.clsidService != *clsid {
            return Err(format!("another input method's ({:?})", info.clsidService));
        }
        let button: ITfLangBarItemButton = item.cast().map_err(|e| e.to_string())?;
        let icon = button.GetIcon().map_err(|e| format!("no icon ({e})"))?;
        let _ = DestroyIcon(icon);
        let tooltip = || button.GetTooltipString().map(|t| t.to_string()).unwrap_or_default();
        let click = || {
            let _ = button.OnClick(TF_LBI_CLK_LEFT, POINT::default(), std::ptr::null());
            pump(Duration::from_millis(50));
        };
        let on = tooltip();
        click();
        let paused = tooltip();
        click();
        let resumed = tooltip();
        let seen = format!("{on:?} → click → {paused:?} → click → {resumed:?}");
        let expected = on.contains("켜짐") && paused.contains("일시정지") && resumed.contains("켜짐");
        if expected { Ok(seen) } else { Err(seen) }
    }
}

/// The run's own folder in %TEMP%: what Araso keeps (learned words, the log) and, with
/// `--browser`, the browser's profile.
static SUPPORT: std::sync::OnceLock<std::path::PathBuf> = std::sync::OnceLock::new();

/// Ends the test with `code`, leaving nothing in %TEMP%.
fn finish(code: i32) -> ! {
    if let Some(support) = SUPPORT.get() {
        // A browser that just quit can hold its profile a moment longer.
        for _ in 0..50 {
            if std::fs::remove_dir_all(support).is_ok() || !support.exists() {
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }
    std::process::exit(code);
}

/// Runs the test again inside an AppContainer, the sandbox Store apps run in: there Araso loads
/// only if its DLL is readable by all app packages, and can't read %APPDATA%.
fn run_in_app_container(args: &[String]) -> i32 {
    use windows::core::PWSTR;
    use windows::Win32::Foundation::{SetHandleInformation, HANDLE_FLAG_INHERIT};
    use windows::Win32::Security::Isolation::{
        CreateAppContainerProfile, DeleteAppContainerProfile, DeriveAppContainerSidFromAppContainerName,
    };
    use windows::Win32::Security::{FreeSid, SECURITY_CAPABILITIES};
    use windows::Win32::System::Console::{GetStdHandle, STD_ERROR_HANDLE, STD_OUTPUT_HANDLE};
    use windows::Win32::System::Threading::*;

    let exe = std::env::current_exe().expect("this program");
    // The sandbox reads only what all app packages may: this program and the cases file too, then.
    let files = args.iter().filter(|a| !a.starts_with("--")).map(std::path::PathBuf::from);
    for path in std::iter::once(exe.clone()).chain(files) {
        let _ = std::process::Command::new("icacls").arg(&path).args(["/grant", "*S-1-15-2-1:(RX)"]).output();
    }
    unsafe {
        let name = w!("Araso.TypingTest");
        let sid = CreateAppContainerProfile(name, name, name, None)
            .or_else(|_| DeriveAppContainerSidFromAppContainerName(name))
            .expect("app container");
        let capabilities = SECURITY_CAPABILITIES { AppContainerSid: sid, ..Default::default() };
        let mut size = 0usize;
        let _ = InitializeProcThreadAttributeList(None, 1, None, &mut size);
        let mut buffer = vec![0u8; size];
        let list = LPPROC_THREAD_ATTRIBUTE_LIST(buffer.as_mut_ptr() as *mut _);
        InitializeProcThreadAttributeList(Some(list), 1, None, &mut size).expect("attribute list");
        UpdateProcThreadAttribute(
            list,
            0,
            PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES as usize,
            Some(&capabilities as *const _ as *const std::ffi::c_void),
            std::mem::size_of::<SECURITY_CAPABILITIES>(),
            None,
            None,
        )
        .expect("security capabilities");
        let mut startup = STARTUPINFOEXW::default();
        startup.StartupInfo.cb = std::mem::size_of::<STARTUPINFOEXW>() as u32;
        startup.lpAttributeList = list;
        // It reports through this process's output.
        startup.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
        startup.StartupInfo.hStdOutput = GetStdHandle(STD_OUTPUT_HANDLE).unwrap_or_default();
        startup.StartupInfo.hStdError = GetStdHandle(STD_ERROR_HANDLE).unwrap_or_default();
        for handle in [startup.StartupInfo.hStdOutput, startup.StartupInfo.hStdError] {
            let _ = SetHandleInformation(handle, HANDLE_FLAG_INHERIT.0, HANDLE_FLAG_INHERIT);
        }
        let quoted: Vec<String> = std::iter::once(exe.display().to_string()).chain(args.iter().cloned()).map(|a| format!("\"{a}\"")).collect();
        let mut command: Vec<u16> = quoted.join(" ").encode_utf16().chain(Some(0)).collect();
        // A folder the sandbox can read, to start in.
        let system: Vec<u16> = format!("{}\\System32", std::env::var("SystemRoot").unwrap_or("C:\\Windows".into()))
            .encode_utf16()
            .chain(Some(0))
            .collect();
        let mut process = PROCESS_INFORMATION::default();
        CreateProcessW(
            None,
            Some(PWSTR(command.as_mut_ptr())),
            None,
            None,
            true,
            EXTENDED_STARTUPINFO_PRESENT,
            None,
            PCWSTR(system.as_ptr()),
            &startup.StartupInfo,
            &mut process,
        )
        .expect("start the test in the app container");
        // A sandboxed program can't take the foreground from the app in front: it's handed over from here.
        bring_forward(process.dwProcessId);
        WaitForSingleObject(process.hProcess, INFINITE);
        let mut code = 0u32;
        let _ = GetExitCodeProcess(process.hProcess, &mut code);
        DeleteProcThreadAttributeList(list);
        FreeSid(sid);
        // The sandbox's profile goes too, its folders with it; the next run makes it again.
        let _ = DeleteAppContainerProfile(name);
        code as i32
    }
}

/// Gives the test window of process `pid` the foreground once it is there.
fn bring_forward(pid: u32) {
    for _ in 0..250 {
        if let Some(window) = find_window(w!("ArasoTypingTest"), |window, owner| owner == pid && !window.is_invalid()) {
            bring_to_front(window);
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// A top-level window of class `class` that `accept`s it and its process.
fn find_window(class: PCWSTR, accept: impl Fn(HWND, u32) -> bool) -> Option<HWND> {
    unsafe {
        let mut window = FindWindowExW(None, None, class, None).ok();
        while let Some(w) = window {
            let mut owner = 0u32;
            GetWindowThreadProcessId(w, Some(&mut owner));
            if accept(w, owner) {
                return Some(w);
            }
            window = FindWindowExW(None, Some(w), class, None).ok();
        }
        None
    }
}

/// Sharing the input state of the window in front lets `window` take the foreground.
fn bring_to_front(window: HWND) {
    use windows::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
    unsafe {
        let front = GetWindowThreadProcessId(GetForegroundWindow(), None);
        let me = GetCurrentThreadId();
        let attached = front != 0 && front != me && AttachThreadInput(me, front, true).as_bool();
        let _ = SetForegroundWindow(window);
        let _ = BringWindowToTop(window);
        if attached {
            let _ = AttachThreadInput(me, front, false);
        }
    }
}

/// `--browser`: the cases typed into a textarea in Chrome (or Edge), started with a profile of its
/// own; the page puts what the field holds in its title, where this program reads it. The browser
/// types with whichever input method is chosen (Win+Space), as any app does.
fn run_in_browser(cases: &[Case], others: &str, support: &std::path::Path) -> i32 {
    use windows::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_INFORMATION, PROCESS_VM_READ};
    let candidates = [
        format!("{}\\Google\\Chrome\\Application\\chrome.exe", std::env::var("ProgramFiles").unwrap_or_default()),
        format!("{}\\Google\\Chrome\\Application\\chrome.exe", std::env::var("LOCALAPPDATA").unwrap_or_default()),
        format!("{}\\Microsoft\\Edge\\Application\\msedge.exe", std::env::var("ProgramFiles(x86)").unwrap_or_default()),
    ];
    let Some(browser) = candidates.iter().find(|p| std::path::Path::new(p).exists()) else {
        eprintln!("neither Chrome nor Edge is installed");
        return 2;
    };
    let page = "data:text/html;charset=utf-8,<title>ARASO\"\"</title>\
        <textarea id=t autofocus style='font-size:24px;width:90%;height:150px'></textarea>\
        <script>const t=document.getElementById('t');const show=()=>document.title='ARASO'+JSON.stringify(t.value);\
        t.addEventListener('input',show);\
        document.addEventListener('keydown',e=>{if(e.key=='F9'){t.value='';show()}})</script>";
    let profile = support.join("browser-profile");
    let mut child = match std::process::Command::new(browser)
        .arg(format!("--user-data-dir={}", profile.display()))
        .args(["--no-first-run", "--no-default-browser-check", "--disable-sync", "--new-window", page])
        .spawn()
    {
        Ok(child) => child,
        Err(e) => {
            eprintln!("{browser} didn't start: {e}");
            return 2;
        }
    };
    let title = |window: HWND| {
        let text = text_of(window);
        let json = text.strip_prefix("ARASO")?;
        parse_json_string(json)
    };
    let mut window = None;
    for _ in 0..500 {
        window = find_window(w!("Chrome_WidgetWin_1"), |w, _| text_of(w).starts_with("ARASO"));
        if window.is_some() {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let Some(window) = window else {
        eprintln!("the browser window didn't show up");
        let _ = child.kill();
        return 2;
    };
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(window, Some(&mut pid)) };
    bring_to_front(window);
    std::thread::sleep(Duration::from_millis(1500));
    let mut failures = 0;
    for Case { keys, expected, .. } in cases {
        if let Err(e) = type_keys(window, keys) {
            eprintln!("{e}");
            let _ = child.kill();
            return 3;
        }
        std::thread::sleep(Duration::from_millis(150));
        let got = title(window).unwrap_or_else(|| format!("(title {:?})", text_of(window)));
        // Araso's word window must not show here: the browser shows the word itself.
        let boxed = find_window(w!("ArasoComposition"), |w, owner| owner == pid && unsafe { IsWindowVisible(w) }.as_bool())
            .map(text_of);
        let ok = got == *expected && boxed.is_none();
        if !ok {
            failures += 1;
        }
        let note = match (&boxed, got == *expected) {
            (Some(b), _) => format!(" (and in Araso's window: {b:?})"),
            (None, false) => format!(" (expected {expected:?})"),
            (None, true) => String::new(),
        };
        println!("{} {keys:?} → {got:?}{note}", if ok { "ok  " } else { "FAIL" });
        let _ = type_keys(window, "⏎");
        unsafe { SendInput(&[key_input(VK_F9, false), key_input(VK_F9, true)], std::mem::size_of::<INPUT>() as i32) };
        std::thread::sleep(Duration::from_millis(100));
    }
    let name = browser.rsplit('\\').next().unwrap_or(browser);
    println!("{} of {} cases as expected ({name}){others}", cases.len() - failures, cases.len());
    let loaded = unsafe { OpenProcess(PROCESS_QUERY_INFORMATION | PROCESS_VM_READ, false, pid) }
        .ok()
        .and_then(araso_module)
        .unwrap_or_else(|| "no Araso DLL (is Araso the chosen input method?)".into());
    println!("browser loaded: {loaded}");
    unsafe {
        let _ = PostMessageW(Some(window), WM_CLOSE, WPARAM(0), LPARAM(0));
    }
    let _ = child.wait();
    if failures > 0 { 1 } else { 0 }
}

/// The JSON string at the start of `text`.
fn parse_json_string(text: &str) -> Option<String> {
    let mut chars = text.strip_prefix('"')?.chars();
    let mut out = String::new();
    while let Some(c) = chars.next() {
        match c {
            '"' => return Some(out),
            '\\' => match chars.next()? {
                'n' => out.push('\n'),
                't' => out.push('\t'),
                'r' => out.push('\r'),
                'u' => {
                    let hex: String = chars.by_ref().take(4).collect();
                    out.push(char::from_u32(u32::from_str_radix(&hex, 16).ok()?)?);
                }
                other => out.push(other),
            },
            c => out.push(c),
        }
    }
    None
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--appcontainer") {
        let rest: Vec<String> = args.iter().filter(|a| *a != "--appcontainer").cloned().collect();
        std::process::exit(run_in_app_container(&rest));
    }
    let richedit = args.iter().any(|a| a == "--richedit");
    // A RichEdit left to IMM32, as KakaoTalk's chat input is.
    let richedit_imm = args.iter().any(|a| a == "--richedit-imm");
    let dev = args.iter().any(|a| a == "--dev");
    let (clsid, profile) = if dev { (CLSID_ARASO_DEV, GUID_PROFILE_DEV) } else { (CLSID_ARASO, GUID_PROFILE) };
    let imm = args.iter().any(|a| a == "--imm");
    let field = if args.iter().any(|a| a == "--browser") {
        "browser"
    } else if richedit {
        "richedit"
    } else if richedit_imm {
        "richedit-imm"
    } else if imm {
        "imm"
    } else {
        "edit"
    };
    let files: Vec<String> = args
        .iter()
        .filter(|a| !a.starts_with("--"))
        .map(|path| std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{path}: {e}")))
        .collect();
    let scripts: Vec<&str> = if files.is_empty() { SCRIPTS.to_vec() } else { files.iter().map(String::as_str).collect() };
    let (cases, others): (Vec<Case>, Vec<Case>) = scripts.iter().flat_map(|s| parse_cases(s)).partition(|c| c.applies_to(field));
    let others = if others.is_empty() { String::new() } else { format!(", {} not for this field", others.len()) };

    let before = memory();

    // Araso in this process keeps what it learns from the cases in a folder of its own, not in
    // the user's dictionary.
    let support = std::env::temp_dir().join(format!("araso-typing-test-{}", std::process::id()));
    unsafe { std::env::set_var("ARASO_SUPPORT_DIR", &support) };
    let _ = SUPPORT.set(support.clone());
    // `--log`: Araso's diagnostic log (kinds of keys and contexts, never text), shown at the end.
    let log = args.iter().any(|a| a == "--log");
    if log {
        let _ = std::fs::create_dir_all(&support);
        let _ = std::fs::write(support.join("logging"), "");
    }

    // Never left running: whatever happens, it ends within a minute.
    std::thread::spawn(|| {
        std::thread::sleep(Duration::from_secs(60));
        eprintln!("timed out");
        finish(4);
    });
    if args.iter().any(|a| a == "--browser") {
        let code = run_in_browser(&cases, &others, &support);
        if log {
            for entry in std::fs::read_dir(support.join("logs")).into_iter().flatten().flatten() {
                println!("--- {}", entry.file_name().to_string_lossy());
                print!("{}", std::fs::read_to_string(entry.path()).unwrap_or_default());
            }
        }
        finish(code);
    }
    let step = |s: &str| {
        use std::io::Write;
        eprintln!("[{s}]");
        let _ = std::io::stderr().flush();
    };

    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let instance = GetModuleHandleW(None).expect("module");
        let class = WNDCLASSW {
            lpfnWndProc: Some(window_proc),
            hInstance: instance.into(),
            lpszClassName: w!("ArasoTypingTest"),
            hbrBackground: windows::Win32::Graphics::Gdi::HBRUSH(6 as *mut _), // COLOR_WINDOW + 1
            ..Default::default()
        };
        RegisterClassW(&class);
        let window = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            w!("ArasoTypingTest"),
            w!("Araso typing test"),
            WS_OVERLAPPEDWINDOW | WS_VISIBLE,
            200,
            200,
            700,
            300,
            None,
            None,
            Some(instance.into()),
            None,
        )
        .expect("window");
        let edit_class: PCWSTR = if richedit || richedit_imm {
            let _ = LoadLibraryW(w!("Msftedit.dll"));
            w!("RICHEDIT50W")
        } else if imm {
            let field = WNDCLASSW {
                lpfnWndProc: Some(imm_field_proc),
                hInstance: instance.into(),
                lpszClassName: w!("ArasoImmField"),
                ..Default::default()
            };
            RegisterClassW(&field);
            w!("ArasoImmField")
        } else {
            w!("EDIT")
        };
        let edit = CreateWindowExW(
            WS_EX_CLIENTEDGE,
            edit_class,
            w!(""),
            WS_CHILD | WS_VISIBLE | WINDOW_STYLE((ES_MULTILINE | ES_AUTOVSCROLL | ES_WANTRETURN) as u32),
            10,
            10,
            660,
            230,
            Some(window),
            None,
            Some(instance.into()),
            None,
        )
        .expect("edit control");
        if richedit {
            // SES_USECTF: the control talks to TSF itself instead of through IMM32.
            const EM_SETEDITSTYLE: u32 = 0x0400 + 204;
            const SES_USECTF: usize = 0x0001_0000;
            SendMessageW(edit, EM_SETEDITSTYLE, Some(WPARAM(SES_USECTF)), Some(LPARAM(SES_USECTF as isize)));
        }

        step("window created");
        // Sharing the input state of the window in front lets this one take the foreground;
        // then Araso is activated for this process only.
        let front = GetForegroundWindow();
        let front_thread = GetWindowThreadProcessId(front, None);
        use windows::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
        let me = GetCurrentThreadId();
        let attached = front_thread != 0 && front_thread != me && AttachThreadInput(me, front_thread, true).as_bool();
        let _ = SetForegroundWindow(window);
        let _ = BringWindowToTop(window);
        let _ = SetFocus(Some(edit));
        if attached {
            let _ = AttachThreadInput(me, front_thread, false);
        }
        pump(Duration::from_millis(300));
        step(&format!("foreground: {}", GetForegroundWindow() == window));
        let profiles: ITfInputProcessorProfileMgr =
            CoCreateInstance(&CLSID_TF_InputProcessorProfiles, None, CLSCTX_INPROC_SERVER).expect("TSF profiles");

        step("activating Araso");
        if let Err(e) =
            profiles.ActivateProfile(TF_PROFILETYPE_INPUTPROCESSOR, 0x0412, &clsid, &profile, HKL::default(), TF_IPPMF_FORPROCESS)
        {
            eprintln!("Araso could not be activated ({e}); is it registered?");
            finish(2);
        }
        step("activated");
        pump(Duration::from_millis(1500)); // the models load in the background
        step("typing");
        if GetForegroundWindow() != window {
            eprintln!("the test window didn't get the keyboard focus; nothing was typed");
            finish(2);
        }

        let mut failures = 0;
        for case in &cases {
            let _ = SetWindowTextW(edit, w!(""));
            let _ = SetFocus(Some(edit));
            pump(Duration::from_millis(50));
            if let Err(e) = type_keys(window, &case.keys) {
                eprintln!("{e}");
                finish(3);
            }
            // What is on screen: the field, and the word in Araso's window after it.
            let got = text_of(edit) + &composition_window_text();
            let ok = got == case.expected;
            if !ok {
                failures += 1;
            }
            let note = if ok { String::new() } else { format!(" (expected {:?})", case.expected) };
            println!("{} {:?} → {got:?}{note}", if ok { "ok  " } else { "FAIL" }, case.keys);
            // Return commits anything left composing before the next case.
            let _ = type_keys(window, "⏎");
        }
        match check_mode_icon(&clsid) {
            Ok(seen) => println!("ok   mode icon: {seen}"),
            Err(e) => {
                failures += 1;
                println!("FAIL mode icon: {e}");
            }
        }
        let label = match field {
            "richedit" => "RichEdit, TSF",
            "richedit-imm" => "RichEdit, IMM32",
            "imm" => "own IMM32 handling",
            _ => "EDIT, IMM32",
        };
        println!("{} of {} cases as expected ({label}){others}", cases.len() - failures, cases.len());
        println!(
            "{}-bit test process, typed with {}; loaded: {}",
            std::mem::size_of::<usize>() * 8,
            active_profile(&profiles),
            araso_module(windows::Win32::System::Threading::GetCurrentProcess()).unwrap_or_else(|| "no Araso DLL".into())
        );
        let after = memory();
        println!(
            "memory with Araso: private {} → {} KB, working set {} → {} KB",
            before.0, after.0, before.1, after.1
        );
        if log {
            for entry in std::fs::read_dir(support.join("logs")).into_iter().flatten().flatten() {
                println!("--- {}", entry.file_name().to_string_lossy());
                print!("{}", std::fs::read_to_string(entry.path()).unwrap_or_default());
            }
        }
        let _ = DestroyWindow(window);
        finish(if failures > 0 { 1 } else { 0 });
    }
}
