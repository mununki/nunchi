//! The word being typed, where the field can't show it. An IMM32 app (TSF gives it a transitory
//! context) shows a Korean composition as the one syllable a Korean IME composes at a time: of a
//! longer one only the first character appears. There the word stays out of the field until it
//! ends, and this window shows it over the caret instead, in the field's font, underlined.

use std::ffi::c_void;

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM};
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::WindowsAndMessaging::*;

/// The window class. A class belongs to the module that registers it, so the installed Nunchi and
/// the development build loaded in one app each have their own.
const CLASS: PCWSTR = w!("NunchiComposition");
/// Space between the border and the text.
const PAD: i32 = 1;

/// The window of one text service, made when first needed, on its thread.
#[derive(Default)]
pub struct CompositionWindow {
    hwnd: Option<HWND>,
    owner: Option<HWND>,
    /// The underlined font, and the field and caret height it was made for.
    font: Option<(HWND, i32, HFONT)>,
    visible: bool,
}

impl CompositionWindow {
    pub fn is_visible(&self) -> bool {
        self.visible
    }

    /// Shows `text` over `caret` (screen coordinates), the caret of `field`.
    pub fn show(&mut self, text: &str, caret: RECT, field: HWND) {
        unsafe {
            // Owned by the field's window, it stays above that window and goes behind other apps
            // with it.
            let root = GetAncestor(field, GA_ROOT);
            let owner = (!root.is_invalid() && GetWindowThreadProcessId(root, None) == GetCurrentThreadId()).then_some(root);
            if self.hwnd.is_some() && self.owner != owner {
                self.destroy();
            }
            let hwnd = match self.hwnd {
                Some(hwnd) => hwnd,
                None => match self.create(owner) {
                    Some(hwnd) => hwnd,
                    None => return,
                },
            };
            let font = self.font_for(field, caret.bottom - caret.top);
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, font.0 as _); // isize, or i32 in 32-bit apps
            let wide: Vec<u16> = text.encode_utf16().chain(Some(0)).collect();
            let _ = SetWindowTextW(hwnd, PCWSTR(wide.as_ptr()));
            let size = text_size(hwnd, font, &wide[..wide.len() - 1]);
            let (width, height) = (size.cx + 2 * (PAD + 1), size.cy + 2 * (PAD + 1));
            // The text where the field would draw it: from the caret, centred on its height.
            let mut x = caret.left - PAD - 1;
            let y = caret.top + (caret.bottom - caret.top - size.cy) / 2 - PAD - 1;
            let mut monitor = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
            let screen = MonitorFromPoint(POINT { x: caret.left, y: caret.top }, MONITOR_DEFAULTTONEAREST);
            if GetMonitorInfoW(screen, &mut monitor).as_bool() {
                x = x.min(monitor.rcWork.right - width).max(monitor.rcWork.left);
            }
            let above = if owner.is_some() { HWND_TOP } else { HWND_TOPMOST };
            let _ = SetWindowPos(hwnd, Some(above), x, y, width, height, SWP_NOACTIVATE | SWP_SHOWWINDOW);
            let _ = InvalidateRect(Some(hwnd), None, false);
            let _ = UpdateWindow(hwnd);
            self.visible = true;
        }
    }

    pub fn hide(&mut self) {
        if let (Some(hwnd), true) = (self.hwnd, self.visible) {
            unsafe {
                let _ = ShowWindow(hwnd, SW_HIDE);
            }
        }
        self.visible = false;
    }

    /// Destroys the window, on the thread that made it, before the DLL may unload.
    pub fn destroy(&mut self) {
        self.visible = false;
        self.owner = None;
        unsafe {
            if let Some((_, _, font)) = self.font.take() {
                let _ = DeleteObject(font.into());
            }
            if let Some(hwnd) = self.hwnd.take() {
                let _ = DestroyWindow(hwnd);
                // A class outlives the DLL that registered it: with no window of it left in the
                // app, it goes too, and the next window registers it again.
                let _ = UnregisterClassW(CLASS, Some(crate::module()));
            }
        }
    }

    unsafe fn create(&mut self, owner: Option<HWND>) -> Option<HWND> {
        let class = WNDCLASSW {
            lpfnWndProc: Some(window_proc),
            hInstance: crate::module(),
            lpszClassName: CLASS,
            ..Default::default()
        };
        RegisterClassW(&class); // fails when another thread's window has it registered already
        let topmost = if owner.is_none() { WS_EX_TOPMOST } else { WINDOW_EX_STYLE(0) };
        let style = WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW | topmost;
        match CreateWindowExW(style, CLASS, PCWSTR::null(), WS_POPUP, 0, 0, 0, 0, owner, None, Some(crate::module()), None) {
            Ok(hwnd) => {
                self.hwnd = Some(hwnd);
                self.owner = owner;
                Some(hwnd)
            }
            Err(e) => {
                crate::log::write(&format!("composition window failed: {e}"));
                None
            }
        }
    }

    unsafe fn font_for(&mut self, field: HWND, caret_height: i32) -> HFONT {
        if let Some((made_for, height, font)) = self.font {
            if made_for == field && height == caret_height {
                return font;
            }
        }
        if let Some((_, _, old)) = self.font.take() {
            let _ = DeleteObject(old.into());
        }
        let font = underlined_font(field, caret_height);
        self.font = Some((field, caret_height, font));
        font
    }
}

impl Drop for CompositionWindow {
    fn drop(&mut self) {
        self.destroy();
    }
}

/// This thread's caret, in screen coordinates, and the field it is in.
pub fn system_caret() -> Option<(RECT, HWND)> {
    unsafe {
        let mut info = GUITHREADINFO { cbSize: size_of::<GUITHREADINFO>() as u32, ..Default::default() };
        GetGUIThreadInfo(GetCurrentThreadId(), &mut info).ok()?;
        if info.hwndCaret.is_invalid() {
            return None;
        }
        let r = info.rcCaret;
        let mut corners = [POINT { x: r.left, y: r.top }, POINT { x: r.right, y: r.bottom }];
        for corner in &mut corners {
            let _ = ClientToScreen(info.hwndCaret, corner);
        }
        let rect = RECT { left: corners[0].x, top: corners[0].y, right: corners[1].x, bottom: corners[1].y };
        Some((rect, info.hwndCaret))
    }
}

/// The field's font, underlined. A field that doesn't say (one that draws its own text, or an
/// EDIT left with the system font) gets the system's font for messages, as tall as its caret.
unsafe fn underlined_font(field: HWND, caret_height: i32) -> HFONT {
    let mut font = LOGFONTW::default();
    let current = HFONT(SendMessageW(field, WM_GETFONT, None, None).0 as *mut c_void);
    let size = size_of::<LOGFONTW>() as i32;
    if current.is_invalid() || GetObjectW(current.into(), size, Some(&mut font as *mut _ as *mut c_void)) == 0 {
        let mut metrics = NONCLIENTMETRICSW { cbSize: size_of::<NONCLIENTMETRICSW>() as u32, ..Default::default() };
        let pointer = Some(&mut metrics as *mut _ as *mut c_void);
        let _ = SystemParametersInfoW(SPI_GETNONCLIENTMETRICS, metrics.cbSize, pointer, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0));
        font = metrics.lfMessageFont;
        if caret_height >= 8 {
            font.lfHeight = caret_height; // positive: the height of a line, as a caret is
        }
    }
    font.lfUnderline = 1;
    CreateFontIndirectW(&font)
}

unsafe fn text_size(hwnd: HWND, font: HFONT, text: &[u16]) -> SIZE {
    let dc = GetDC(Some(hwnd));
    let old = SelectObject(dc, font.into());
    let mut size = SIZE::default();
    let _ = GetTextExtentPoint32W(dc, text, &mut size);
    SelectObject(dc, old);
    ReleaseDC(Some(hwnd), dc);
    size
}

unsafe fn paint(hwnd: HWND) {
    let mut paint = PAINTSTRUCT::default();
    let dc = BeginPaint(hwnd, &mut paint);
    let mut rect = RECT::default();
    let _ = GetClientRect(hwnd, &mut rect);
    FillRect(dc, &rect, GetSysColorBrush(COLOR_WINDOW));
    FrameRect(dc, &rect, GetSysColorBrush(COLOR_HIGHLIGHT));
    let mut text = vec![0u16; GetWindowTextLengthW(hwnd) as usize + 1];
    let n = GetWindowTextW(hwnd, &mut text) as usize;
    let font = HFONT(GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut c_void);
    let old = SelectObject(dc, font.into());
    SetBkMode(dc, TRANSPARENT);
    SetTextColor(dc, COLORREF(GetSysColor(COLOR_WINDOWTEXT)));
    let _ = TextOutW(dc, PAD + 1, PAD + 1, &text[..n]);
    SelectObject(dc, old);
    let _ = EndPaint(hwnd, &paint);
}

extern "system" fn window_proc(hwnd: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe {
        match message {
            WM_PAINT => {
                paint(hwnd);
                LRESULT(0)
            }
            WM_ERASEBKGND => LRESULT(1), // all of it is painted in WM_PAINT
            // Clicks go through to the field under it, and never activate it.
            WM_NCHITTEST => LRESULT(HTTRANSPARENT as isize),
            WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
            _ => DefWindowProcW(hwnd, message, wparam, lparam),
        }
    }
}
