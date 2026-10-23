//! What every text service in the process shares: the settings, the per-app policies and the
//! decision engine with the user's dictionary. The files are in `%APPDATA%\Araso`; another
//! process (the settings, or the input method in another app) may change them, so they are
//! looked at again now and then, and a change bumps `generation`: sessions built before it are
//! replaced, as upstream's `Runtime`.

use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime};

use araso_core::{support_dir, AppPolicies, DictionaryEngine, InputSession, Settings};
use windows::Win32::System::LibraryLoader::GetModuleFileNameW;

pub struct Runtime {
    pub settings: Settings,
    pub policies: AppPolicies,
    pub engine: Arc<DictionaryEngine>,
    pub generation: u64,
    stamps: (Option<SystemTime>, Option<SystemTime>),
}

impl Runtime {
    /// The word lists are there. They are built into the DLL, so only a broken build lacks them;
    /// without them every word would be guessed, so keys would go through as typed.
    pub fn has_data(&self) -> bool {
        static HAS_DATA: OnceLock<bool> = OnceLock::new();
        *HAS_DATA.get_or_init(|| {
            let ok = self.engine.english.word_count() > 10_000
                && araso_core::KoreanModel::new().is_vocabulary_word("안녕하세요");
            if !ok {
                crate::log::write("word lists missing: keys go through as typed");
            }
            ok
        })
    }
}

struct Shared {
    runtime: Option<Arc<Runtime>>,
    checked: Option<Instant>,
}

static SHARED: Mutex<Shared> = Mutex::new(Shared { runtime: None, checked: None });

/// How often the files are looked at for changes.
const CHECK_EVERY: Duration = Duration::from_secs(2);

fn stamps() -> (Option<SystemTime>, Option<SystemTime>) {
    let settings = araso_core::settings::store_url().and_then(|p| support_dir::modified(&p));
    let apps = araso_core::app_policy::store_url().and_then(|p| support_dir::modified(&p));
    (settings, apps)
}

fn load(previous: Option<&Runtime>) -> Runtime {
    point_resources();
    let settings = Settings::load_default();
    settings.apply_layouts();
    let policies = AppPolicies::load_default();
    // The engine (and its models) only changes with Japanese; the dictionary reads itself again.
    let engine = match previous {
        Some(p) if p.settings.japanese == settings.japanese => p.engine.clone(),
        _ => araso_core::make_engine(settings.japanese, araso_core::user_dictionary::default_store_url()),
    };
    let generation = previous.map_or(1, |p| p.generation + 1);
    Runtime { settings, policies, engine, generation, stamps: stamps() }
}

/// Has the next `current` look at the files at once (this text service just wrote one).
pub fn refresh() {
    SHARED.lock().unwrap_or_else(|e| e.into_inner()).checked = None;
}

/// The runtime, read again when its files changed since it was last read.
pub fn current() -> Arc<Runtime> {
    let mut shared = SHARED.lock().unwrap_or_else(|e| e.into_inner());
    let due = shared.checked.map_or(true, |t| t.elapsed() >= CHECK_EVERY);
    if let Some(runtime) = shared.runtime.clone() {
        if !due {
            return runtime;
        }
        shared.checked = Some(Instant::now());
        if stamps() == runtime.stamps {
            runtime.engine.reload_if_changed();
            return runtime;
        }
    }
    let fresh = Arc::new(load(shared.runtime.as_deref()));
    shared.checked = Some(Instant::now());
    shared.runtime = Some(fresh.clone());
    crate::log::write(&format!("runtime generation {}", fresh.generation));
    fresh
}

/// Loads the models in the background, so the first key in an app doesn't wait for them.
pub fn preload() {
    static STARTED: OnceLock<()> = OnceLock::new();
    STARTED.get_or_init(|| {
        let _ = std::thread::Builder::new().name("araso-preload".into()).spawn(|| {
            let started = Instant::now();
            let runtime = current();
            araso_core::KoreanModel::preload();
            let _ = runtime.engine.english.is_known_word("hello");
            crate::log::write(&format!("models loaded in {} ms", started.elapsed().as_millis()));
        });
    });
}

/// A session for one text service, from the runtime as it is now.
pub fn make_session(runtime: &Runtime) -> InputSession {
    let s = &runtime.settings;
    let mut session = InputSession::new(
        runtime.engine.clone(),
        runtime.policies.clone(),
        None,
        s.japanese,
        s.live_conversion,
        s.return_confirms_only,
    );
    session.full_width_digits = s.full_width_digits;
    session.full_width_space = s.full_width_space;
    session
}

/// The data files ship in `data` next to the DLL; in a development build the source tree's
/// copies are found without this.
fn point_resources() {
    static DONE: OnceLock<()> = OnceLock::new();
    DONE.get_or_init(|| {
        if let Some(dir) = module_dir().map(|d| d.join("data")).filter(|d| d.exists()) {
            araso_core::resources::set_directory(dir);
        }
    });
}

fn module_path(module: windows::Win32::Foundation::HMODULE) -> Option<PathBuf> {
    let mut buffer = vec![0u16; 1024];
    let length = unsafe { GetModuleFileNameW(Some(module), &mut buffer) } as usize;
    (length > 0 && length < buffer.len()).then(|| PathBuf::from(String::from_utf16_lossy(&buffer[..length])))
}

/// The folder this DLL is in.
pub fn module_dir() -> Option<PathBuf> {
    module_path(crate::module().into())?.parent().map(PathBuf::from)
}

pub fn dll_path() -> Option<PathBuf> {
    module_path(crate::module().into())
}

/// The app Araso types into: the executable's name in lowercase (`notepad.exe`), the key of
/// its entry in apps.json.
pub fn app_name() -> Option<String> {
    static NAME: OnceLock<Option<String>> = OnceLock::new();
    NAME.get_or_init(|| {
        let path = module_path(windows::Win32::Foundation::HMODULE::default())?;
        path.file_name().map(|n| n.to_string_lossy().to_lowercase())
    })
    .clone()
}
