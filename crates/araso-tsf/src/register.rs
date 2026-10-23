//! `regsvr32`: the COM class, Araso's profile among the Korean keyboards, and the TSF
//! categories that say what it can do (a keyboard, with display attributes, in Store apps and
//! the taskbar's input indicator). Needs administrator rights, as every input method's does.

use windows::core::{w, Result, GUID, PCWSTR};
use windows::Win32::Foundation::ERROR_SUCCESS;
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED};
use windows::Win32::System::Registry::{
    RegCloseKey, RegCreateKeyExW, RegDeleteTreeW, RegSetValueExW, HKEY, HKEY_LOCAL_MACHINE, KEY_WRITE,
    REG_OPTION_NON_VOLATILE, REG_SZ,
};
use windows::Win32::UI::Input::KeyboardAndMouse::HKL;
use windows::Win32::UI::TextServices::*;

use crate::{CLSID_ARASO, GUID_PROFILE, LANGID_KOREAN};

const CATEGORIES: [GUID; 7] = [
    GUID_TFCAT_TIP_KEYBOARD,
    GUID_TFCAT_DISPLAYATTRIBUTEPROVIDER,
    GUID_TFCAT_TIPCAP_UIELEMENTENABLED,
    GUID_TFCAT_TIPCAP_SECUREMODE,
    GUID_TFCAT_TIPCAP_COMLESS,
    GUID_TFCAT_TIPCAP_IMMERSIVESUPPORT,
    GUID_TFCAT_TIPCAP_SYSTRAYSUPPORT,
];

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn guid_string(guid: &GUID) -> String {
    format!("{{{guid:?}}}")
}

fn clsid_key() -> String {
    format!("SOFTWARE\\Classes\\CLSID\\{}", guid_string(&CLSID_ARASO))
}

/// COM is initialized for the calls, as regsvr32 may not have done it.
struct Com(bool);

impl Com {
    fn init() -> Com {
        Com(unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }.is_ok())
    }
}

impl Drop for Com {
    fn drop(&mut self) {
        if self.0 {
            unsafe { CoUninitialize() };
        }
    }
}

fn set_string(key: HKEY, name: PCWSTR, value: &str) -> Result<()> {
    let data = wide(value);
    let bytes = unsafe { std::slice::from_raw_parts(data.as_ptr() as *const u8, data.len() * 2) };
    unsafe { RegSetValueExW(key, name, None, REG_SZ, Some(bytes)).ok() }
}

fn create_key(path: &str) -> Result<HKEY> {
    let mut key = HKEY::default();
    let path = wide(path);
    unsafe {
        RegCreateKeyExW(
            HKEY_LOCAL_MACHINE,
            PCWSTR(path.as_ptr()),
            None,
            PCWSTR::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_WRITE,
            None,
            &mut key,
            None,
        )
        .ok()?;
    }
    Ok(key)
}

fn register_class(dll: &str) -> Result<()> {
    let class = create_key(&clsid_key())?;
    let result = set_string(class, PCWSTR::null(), crate::NAME);
    unsafe {
        let _ = RegCloseKey(class);
    }
    result?;
    let server = create_key(&format!("{}\\InprocServer32", clsid_key()))?;
    let result = set_string(server, PCWSTR::null(), dll).and_then(|_| set_string(server, w!("ThreadingModel"), "Apartment"));
    unsafe {
        let _ = RegCloseKey(server);
    }
    result
}

pub fn register() -> Result<()> {
    let dll = crate::runtime::dll_path().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default();
    register_class(&dll)?;
    let _com = Com::init();
    unsafe {
        let profiles: ITfInputProcessorProfileMgr =
            CoCreateInstance(&CLSID_TF_InputProcessorProfiles, None, CLSCTX_INPROC_SERVER)?;
        let description: Vec<u16> = crate::NAME.encode_utf16().collect();
        let icon: Vec<u16> = dll.encode_utf16().collect();
        profiles.RegisterProfile(
            &CLSID_ARASO,
            LANGID_KOREAN,
            &GUID_PROFILE,
            &description,
            &icon,
            0,
            HKL::default(),
            0,
            true,
            0,
        )?;
        let categories: ITfCategoryMgr = CoCreateInstance(&CLSID_TF_CategoryMgr, None, CLSCTX_INPROC_SERVER)?;
        for category in CATEGORIES {
            categories.RegisterCategory(&CLSID_ARASO, &category, &CLSID_ARASO)?;
        }
    }
    Ok(())
}

pub fn unregister() -> Result<()> {
    let _com = Com::init();
    unsafe {
        if let Ok(categories) = CoCreateInstance::<_, ITfCategoryMgr>(&CLSID_TF_CategoryMgr, None, CLSCTX_INPROC_SERVER) {
            for category in CATEGORIES {
                let _ = categories.UnregisterCategory(&CLSID_ARASO, &category, &CLSID_ARASO);
            }
        }
        if let Ok(profiles) =
            CoCreateInstance::<_, ITfInputProcessorProfileMgr>(&CLSID_TF_InputProcessorProfiles, None, CLSCTX_INPROC_SERVER)
        {
            let _ = profiles.UnregisterProfile(&CLSID_ARASO, LANGID_KOREAN, &GUID_PROFILE, 0);
        }
        let path = wide(&clsid_key());
        let status = RegDeleteTreeW(HKEY_LOCAL_MACHINE, PCWSTR(path.as_ptr()));
        if status != ERROR_SUCCESS && status.0 != 2 {
            // Not there (2: ERROR_FILE_NOT_FOUND) is fine.
            return status.ok();
        }
    }
    Ok(())
}
