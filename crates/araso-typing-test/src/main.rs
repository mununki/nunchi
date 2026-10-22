//! Types each case of a script as real key events (SendInput) into an edit control of its own,
//! with Araso active in this process only, and compares the control's text with what the case
//! expects. Araso must be registered (scripts\dev-register.ps1).
//!
//!   araso-typing-test                 the built-in cases
//!   araso-typing-test cases.tsv       "keys<TAB>expected" per line
//!   araso-typing-test --richedit      a RichEdit (TSF-aware) control instead of a plain EDIT (CUAS)
//!
//! In the keys: "⎋" Esc, "⇧" Shift+Esc, "⏎" Return, "⌫" Backspace, "⇥" Tab.
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

const CLSID_ARASO: GUID = GUID::from_u128(0xae988ccd_6a62_424c_a948_91d4d3b4c13a);
const GUID_PROFILE: GUID = GUID::from_u128(0xc37c500f_1136_464d_bc89_601a354fc632);

const CASES: &[(&str, &str)] = &[
    ("dkssudgktpdy hello ", "안녕하세요 hello "),
    ("Stay hungry, stay foolishsms wkqtmdml akfdlek.", "Stay hungry, stay foolish는 잡스의 말이다."),
    ("dlrjs githubdp commitgkf rjdi.", "이건 github에 commit할 거야."),
    ("PDFvkdlf hellodnjfem ", "PDF파일 hello월드 "),
    ("gksrnr⌫⌫ ", "한ㄱ "),
    ("hello ⇧", "ㅗ디ㅣㅐ "),
    ("dkssud⇧ ", "dkssud "),
    ("dhsmf 3tl", "오늘 3시"),
    ("dho?", "왜?"),
    ("dkssud⏎", "안녕"),
    ("https://example.com/dl ", "https://example.com/dl "),
];

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
        let Some((vk, shift)) = key_for(ch) else { return Err(format!("no key for {ch:?}")) };
        let mut inputs = Vec::new();
        if shift {
            inputs.push(key_input(VK_LSHIFT, false));
        }
        inputs.push(key_input(vk, false));
        inputs.push(key_input(vk, true));
        if shift {
            inputs.push(key_input(VK_LSHIFT, true));
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

fn text_of(edit: HWND) -> String {
    unsafe {
        let length = GetWindowTextLengthW(edit) as usize;
        let mut buffer = vec![0u16; length + 1];
        let n = GetWindowTextW(edit, &mut buffer) as usize;
        String::from_utf16_lossy(&buffer[..n]).replace("\r\n", "\n").replace('\r', "\n")
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let richedit = args.iter().any(|a| a == "--richedit");
    let imm = args.iter().any(|a| a == "--imm");
    let cases: Vec<(String, String)> = match args.iter().find(|a| !a.starts_with("--")) {
        Some(path) => std::fs::read_to_string(path)
            .expect("cases file")
            .lines()
            .filter_map(|l| l.split_once('\t'))
            .map(|(k, e)| (k.to_string(), e.replace("\\n", "\n")))
            .collect(),
        None => CASES.iter().map(|(k, e)| (k.to_string(), e.to_string())).collect(),
    };

    let before = memory();

    // Araso in this process keeps what it learns from the cases in a folder of its own, not in
    // the user's dictionary.
    let support = std::env::temp_dir().join(format!("araso-typing-test-{}", std::process::id()));
    unsafe { std::env::set_var("ARASO_SUPPORT_DIR", &support) };

    // Never left running: whatever happens, it ends within a minute.
    std::thread::spawn(|| {
        std::thread::sleep(Duration::from_secs(60));
        eprintln!("timed out");
        std::process::exit(4);
    });
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
        let edit_class: PCWSTR = if richedit {
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
            profiles.ActivateProfile(TF_PROFILETYPE_INPUTPROCESSOR, 0x0412, &CLSID_ARASO, &GUID_PROFILE, HKL::default(), TF_IPPMF_FORPROCESS)
        {
            eprintln!("Araso could not be activated ({e}); is it registered?");
            std::process::exit(2);
        }
        step("activated");
        pump(Duration::from_millis(1500)); // the models load in the background
        step("typing");
        if GetForegroundWindow() != window {
            eprintln!("the test window didn't get the keyboard focus; nothing was typed");
            std::process::exit(2);
        }

        let mut failures = 0;
        for (keys, expected) in &cases {
            let _ = SetWindowTextW(edit, w!(""));
            let _ = SetFocus(Some(edit));
            pump(Duration::from_millis(50));
            if let Err(e) = type_keys(window, keys) {
                eprintln!("{e}");
                std::process::exit(3);
            }
            // Commit whatever is still composing with a click-like focus change.
            let got = text_of(edit);
            let ok = got == *expected;
            if !ok {
                failures += 1;
            }
            println!("{} {keys:?} → {got:?}{}", if ok { "ok  " } else { "FAIL" }, if ok { String::new() } else { format!(" (expected {expected:?})") });
            // Esc commits anything left composing before the next case.
            let _ = type_keys(window, "⏎");
        }
        let field = if richedit {
            "RichEdit, TSF"
        } else if imm {
            "own IMM32 handling"
        } else {
            "EDIT, IMM32"
        };
        println!("{} of {} cases as expected ({field})", cases.len() - failures, cases.len());
        let after = memory();
        println!(
            "memory with Araso: private {} → {} KB, working set {} → {} KB",
            before.0, after.0, before.1, after.1
        );
        let _ = DestroyWindow(window);
        std::process::exit(if failures > 0 { 1 } else { 0 });
    }
}
