//! Nunchi's mode icon in the taskbar's input indicator, beside the input method's own icon: 가 while
//! it is on, 가 with a small red x while it is paused in the text field or off in the app (keys go
//! in as typed), and a tooltip saying which. A click pauses or resumes it, as the 한자 key does; a
//! right click opens its menu. Windows shows there the language bar button TSF calls
//! GUID_LBI_INPUTMODE.

use std::cell::{Cell, RefCell};

use windows::core::{implement, Interface, Ref, Result, BOOL, BSTR, GUID, PCWSTR};
use windows::Win32::Foundation::{POINT, RECT};
use windows::Win32::Graphics::Gdi::HBITMAP;
use windows::Win32::System::Ole::{CONNECT_E_ADVISELIMIT, CONNECT_E_CANNOTCONNECT, CONNECT_E_NOCONNECTION};
use windows::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD};
use windows::Win32::UI::TextServices::*;
use windows::Win32::UI::WindowsAndMessaging::*;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Mode {
    #[default]
    On,
    /// In this text field (the 한자 key, or a click on the icon).
    Paused,
    /// In this app (apps.json).
    Off,
}

/// What the icon asks of the text service.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Command {
    TogglePause,
    ToggleApp,
    OpenSettings,
}

const MENU_PAUSE: u32 = 1;
const MENU_APP: u32 = 2;
const MENU_SETTINGS: u32 = 3;
/// What the text service does with a command.
pub type Handler = Box<dyn Fn(Command)>;

/// The one sink's cookie.
const COOKIE: u32 = 0x4152_4153;

#[implement(ITfLangBarItemButton, ITfLangBarItem, ITfSource)]
pub struct ModeButton {
    mode: Cell<Mode>,
    /// On the secure desktop (UAC, sign-in): a larger icon, and no menu items that open or write
    /// the user's files.
    secure: bool,
    sink: RefCell<Option<ITfLangBarItemSink>>,
    /// The text service's handler; let go of when it deactivates.
    handler: RefCell<Option<Handler>>,
}

impl ModeButton {
    pub fn new(secure: bool) -> Self {
        ModeButton { mode: Cell::new(Mode::On), secure, sink: RefCell::new(None), handler: RefCell::new(None) }
    }

    pub fn set_handler(&self, handler: Option<Handler>) {
        *self.handler.borrow_mut() = handler;
    }

    /// Shows `mode`, telling the input indicator when it changed.
    pub fn set_mode(&self, mode: Mode) {
        if self.mode.replace(mode) == mode {
            return;
        }
        let sink = self.sink.borrow().clone();
        if let Some(sink) = sink {
            let _ = unsafe { sink.OnUpdate(TF_LBI_ICON | TF_LBI_TEXT | TF_LBI_TOOLTIP) };
        }
    }

    fn command(&self, command: Command) {
        // Out of the borrow: the handler may come back here (set_mode).
        let handler = self.handler.borrow_mut().take();
        if let Some(handler) = handler {
            handler(command);
            let mut slot = self.handler.borrow_mut();
            if slot.is_none() {
                *slot = Some(handler);
            }
        }
    }

    /// The menu: label, command id, checked.
    fn items(&self) -> Vec<(&'static str, u32, bool)> {
        let mode = self.mode.get();
        let mut items = vec![("이 입력창에서 일시정지\t한자", MENU_PAUSE, mode == Mode::Paused)];
        if !self.secure {
            items.push(("이 앱에서 끄기", MENU_APP, mode == Mode::Off));
            items.push(("설정…", MENU_SETTINGS, false));
        }
        items
    }

    fn select(&self, id: u32) {
        match id {
            MENU_PAUSE => self.command(Command::TogglePause),
            MENU_APP => self.command(Command::ToggleApp),
            MENU_SETTINGS => self.command(Command::OpenSettings),
            _ => {}
        }
    }

    /// The menu at `pt` (a right click on the icon), owned by a window of this thread.
    fn show_menu(&self, pt: POINT) {
        unsafe {
            let mut info = GUITHREADINFO { cbSize: size_of::<GUITHREADINFO>() as u32, ..Default::default() };
            let _ = GetGUIThreadInfo(windows::Win32::System::Threading::GetCurrentThreadId(), &mut info);
            let owner = [info.hwndFocus, info.hwndActive].into_iter().find(|h| !h.is_invalid());
            let Some(owner) = owner else { return };
            let Ok(menu) = CreatePopupMenu() else { return };
            for (label, id, checked) in self.items() {
                if id == MENU_SETTINGS {
                    let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
                }
                let text: Vec<u16> = label.encode_utf16().chain(Some(0)).collect();
                let flags = if checked { MF_STRING | MF_CHECKED } else { MF_STRING };
                let _ = AppendMenuW(menu, flags, id as usize, PCWSTR(text.as_ptr()));
            }
            // The owner in front, or the menu doesn't close on a click elsewhere.
            let _ = SetForegroundWindow(owner);
            let flags = TPM_RETURNCMD | TPM_NONOTIFY | TPM_RIGHTBUTTON | TPM_BOTTOMALIGN;
            let id = TrackPopupMenuEx(menu, flags.0, pt.x, pt.y, owner, None).0 as u32;
            let _ = DestroyMenu(menu);
            if id != 0 {
                self.select(id);
            }
        }
    }

    fn tooltip(&self) -> &'static str {
        match self.mode.get() {
            Mode::On => "눈치: 켜짐 (클릭하면 이 입력창에서 일시정지)",
            Mode::Paused => "눈치: 일시정지, 친 그대로 들어갑니다 (클릭하면 다시 켜기)",
            Mode::Off => "눈치: 이 앱에서 꺼짐, 친 그대로 들어갑니다 (클릭하면 다시 켜기)",
        }
    }
}

/// The icon size for the input indicator: 16 pixels (20 on the secure desktop) at 100%, and the
/// screen's scale as Windows applies it (the app the input method runs in may not know it).
fn icon_size(secure: bool) -> i32 {
    let mut dpi = 0u32;
    let mut size = size_of::<u32>() as u32;
    let read = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            windows::core::w!("Control Panel\\Desktop\\WindowMetrics"),
            windows::core::w!("AppliedDPI"),
            RRF_RT_REG_DWORD,
            None,
            Some(&mut dpi as *mut u32 as *mut _),
            Some(&mut size),
        )
    };
    let dpi = if read.is_ok() && dpi >= 96 { dpi } else { 96 };
    let base = if secure { 20 } else { 16 };
    (base * dpi as i32 + 48) / 96
}

impl ITfLangBarItem_Impl for ModeButton_Impl {
    fn GetInfo(&self, info: *mut TF_LANGBARITEMINFO) -> Result<()> {
        if info.is_null() {
            return Err(windows::Win32::Foundation::E_INVALIDARG.into());
        }
        let mut description = [0u16; 32];
        for (slot, unit) in description.iter_mut().zip(crate::NAME.encode_utf16().take(31)) {
            *slot = unit;
        }
        unsafe {
            *info = TF_LANGBARITEMINFO {
                clsidService: crate::CLSID_NUNCHI,
                guidItem: GUID_LBI_INPUTMODE,
                dwStyle: TF_LBI_STYLE_BTN_BUTTON | TF_LBI_STYLE_SHOWNINTRAY,
                ulSort: 0,
                szDescription: description,
            };
        }
        Ok(())
    }

    fn GetStatus(&self) -> Result<u32> {
        Ok(0)
    }

    fn Show(&self, _show: BOOL) -> Result<()> {
        Ok(())
    }

    fn GetTooltipString(&self) -> Result<BSTR> {
        Ok(BSTR::from(self.tooltip()))
    }
}

impl ITfLangBarItemButton_Impl for ModeButton_Impl {
    fn OnClick(&self, click: TfLBIClick, pt: &POINT, _area: *const RECT) -> Result<()> {
        crate::guard(|| {
            if click == TF_LBI_CLK_RIGHT {
                self.show_menu(*pt);
            } else if self.mode.get() == Mode::Off {
                self.command(Command::ToggleApp);
            } else {
                self.command(Command::TogglePause);
            }
            Ok(())
        })
    }

    /// The menu of the floating language bar, where Windows still shows one.
    fn InitMenu(&self, menu: Ref<ITfMenu>) -> Result<()> {
        let Some(menu) = menu.as_ref() else { return Ok(()) };
        for (label, id, checked) in self.items() {
            if id == MENU_SETTINGS {
                let _ = unsafe { menu.AddMenuItem(0, TF_LBMENUF_SEPARATOR, HBITMAP::default(), HBITMAP::default(), &[], std::ptr::null_mut()) };
            }
            let text: Vec<u16> = label.split('\t').next().unwrap_or(label).encode_utf16().collect();
            let flags = if checked { TF_LBMENUF_CHECKED } else { 0 };
            unsafe { menu.AddMenuItem(id, flags, HBITMAP::default(), HBITMAP::default(), &text, std::ptr::null_mut())? };
        }
        Ok(())
    }

    fn OnMenuSelect(&self, id: u32) -> Result<()> {
        crate::guard(|| {
            self.select(id);
            Ok(())
        })
    }

    fn GetIcon(&self) -> Result<HICON> {
        let id = if self.mode.get() == Mode::On { 2 } else { 3 };
        let size = icon_size(self.secure);
        // A new icon each time: the caller destroys it.
        let icon = unsafe {
            LoadImageW(Some(crate::module()), PCWSTR(id as usize as *const u16), IMAGE_ICON, size, size, LR_DEFAULTCOLOR)?
        };
        Ok(HICON(icon.0))
    }

    fn GetText(&self) -> Result<BSTR> {
        Ok(BSTR::from(crate::NAME))
    }
}

impl ITfSource_Impl for ModeButton_Impl {
    fn AdviseSink(&self, riid: *const GUID, sink: Ref<windows::core::IUnknown>) -> Result<u32> {
        if riid.is_null() || unsafe { *riid } != ITfLangBarItemSink::IID {
            return Err(CONNECT_E_CANNOTCONNECT.into());
        }
        if self.sink.borrow().is_some() {
            return Err(CONNECT_E_ADVISELIMIT.into());
        }
        let sink: ITfLangBarItemSink = sink.ok()?.cast()?;
        *self.sink.borrow_mut() = Some(sink);
        Ok(COOKIE)
    }

    fn UnadviseSink(&self, cookie: u32) -> Result<()> {
        if cookie != COOKIE || self.sink.borrow_mut().take().is_none() {
            return Err(CONNECT_E_NOCONNECTION.into());
        }
        Ok(())
    }
}
