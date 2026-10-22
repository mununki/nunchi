//! User settings from `%APPDATA%\Araso\settings.json`:
//!
//! ```json
//! { "japanese": true, "liveConversion": true }
//! ```
//!
//! A missing file or key means the default, and so does a value of the wrong type; unknown keys
//! are ignored (and kept on save). A file that isn't JSON is set aside as settings.json.unreadable.

use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

use crate::app_policy::SwitchKey;
use crate::latin_layout::LatinLayout;
use crate::sebeolsik::KoreanLayout;
use crate::support_dir;

/// The modifiers of a shortcut, written by name in settings.json.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub struct KeyModifiers {
    pub control: bool,
    pub alt: bool,
    pub shift: bool,
    pub win: bool,
}

impl KeyModifiers {
    pub const NONE: KeyModifiers = KeyModifiers { control: false, alt: false, shift: false, win: false };

    pub fn is_empty(self) -> bool {
        self == Self::NONE
    }

    /// Ctrl, Alt or Win: with none of them a key types.
    pub fn has_command(self) -> bool {
        self.control || self.alt || self.win
    }

    /// Ctrl+Alt+Shift+Win order, as Windows shows shortcuts.
    pub fn label(self) -> String {
        let mut parts = Vec::new();
        if self.control {
            parts.push("Ctrl");
        }
        if self.alt {
            parts.push("Alt");
        }
        if self.shift {
            parts.push("Shift");
        }
        if self.win {
            parts.push("Win");
        }
        parts.join("+")
    }

    fn from_json(value: &Value) -> Option<KeyModifiers> {
        let names: Vec<&str> = value.as_array()?.iter().filter_map(Value::as_str).collect();
        Some(KeyModifiers {
            control: names.contains(&"control"),
            // macOS Araso's names too: ⌥ is Alt's place, ⌘ the Windows key's.
            alt: names.contains(&"alt") || names.contains(&"option"),
            shift: names.contains(&"shift"),
            win: names.contains(&"win") || names.contains(&"command"),
        })
    }

    fn to_json(self) -> Value {
        let mut names = Vec::new();
        for (on, name) in [(self.control, "control"), (self.alt, "alt"), (self.shift, "shift"), (self.win, "win")] {
            if on {
                names.push(Value::from(name));
            }
        }
        Value::Array(names)
    }
}

/// A right-hand modifier pressed and released on its own, or the 한자 key.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Tap {
    /// The 한자 key (VK_HANJA), or right Ctrl where the keyboard has none (they share a place).
    HanjaKey,
    RightControl,
    RightAlt,
    RightShift,
}

impl Tap {
    pub const ALL: [Tap; 4] = [Tap::HanjaKey, Tap::RightControl, Tap::RightAlt, Tap::RightShift];

    pub fn raw(self) -> &'static str {
        match self {
            Tap::HanjaKey => "hanjaKey",
            Tap::RightControl => "rightControl",
            Tap::RightAlt => "rightAlt",
            Tap::RightShift => "rightShift",
        }
    }

    pub fn from_raw(raw: &str) -> Option<Tap> {
        match raw {
            "rightOption" => Some(Tap::RightAlt),
            // macOS Araso's default, right ⌘: the 한자 key sits where it does on a Mac keyboard's right.
            "rightCommand" => Some(Tap::HanjaKey),
            _ => Self::ALL.into_iter().find(|t| t.raw() == raw),
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Tap::HanjaKey => "한자",
            Tap::RightControl => "Right Ctrl",
            Tap::RightAlt => "Right Alt",
            Tap::RightShift => "Right Shift",
        }
    }

    /// The switch key that is the same key, if any.
    pub fn switch_key(self) -> Option<SwitchKey> {
        match self {
            Tap::RightAlt => Some(SwitchKey::RightAlt),
            Tap::RightShift => Some(SwitchKey::RightShift),
            Tap::HanjaKey | Tap::RightControl => None,
        }
    }
}

/// A key with modifiers (Ctrl+Alt+P), or a function key on its own (F5). `key_code` is a
/// Windows virtual-key code.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct Shortcut {
    pub key_code: u16,
    pub modifiers: KeyModifiers,
    /// The key as printed on it ("P", "F5", "Space"), for showing the shortcut.
    pub key: String,
}

pub const VK_SPACE: u16 = 0x20;
pub const VK_F1: u16 = 0x70;
pub const VK_F6: u16 = 0x75;
pub const VK_F7: u16 = 0x76;
pub const VK_F24: u16 = 0x87;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ShortcutProblem {
    /// It would type a character.
    Types,
    /// Ctrl+J, Ctrl+K, F6, F7: hiragana and katakana.
    Arasos,
}

impl Shortcut {
    pub fn label(&self) -> String {
        let mods = self.modifiers.label();
        if mods.is_empty() {
            self.key.clone()
        } else {
            format!("{mods}+{}", self.key)
        }
    }

    pub fn matches(&self, key_code: u16, modifiers: KeyModifiers) -> bool {
        key_code == self.key_code && modifiers == self.modifiers
    }

    /// Why it can't pause, or nil when it can. Without Ctrl, Alt or Win a key types (only a
    /// function key may go alone), and Araso's own keys stay theirs.
    pub fn problem(&self) -> Option<ShortcutProblem> {
        if !self.modifiers.has_command() {
            if !(VK_F1..=VK_F24).contains(&self.key_code) {
                return Some(ShortcutProblem::Types);
            }
            if [VK_F6, VK_F7].contains(&self.key_code) {
                return Some(ShortcutProblem::Arasos);
            }
        }
        let control_only = KeyModifiers { control: true, ..KeyModifiers::NONE };
        if self.modifiers == control_only && [b'J' as u16, b'K' as u16].contains(&self.key_code) {
            return Some(ShortcutProblem::Arasos);
        }
        None
    }
}

/// The key that pauses Araso in the text field and resumes it (settings.json's `pauseKey`):
///
/// ```json
/// "pauseKey": "hanjaKey"                                             // a key tapped alone
/// "pauseKey": { "keyCode": 80, "modifiers": ["control", "alt"], "key": "P" }   // Ctrl+Alt+P
/// "pauseKey": "off"
/// ```
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum PauseKey {
    Off,
    Tap(Tap),
    Shortcut(Shortcut),
}

impl PauseKey {
    pub const STANDARD: PauseKey = PauseKey::Tap(Tap::HanjaKey);

    pub fn label(&self) -> String {
        match self {
            PauseKey::Off => "Off".into(),
            PauseKey::Tap(tap) => tap.label().into(),
            PauseKey::Shortcut(shortcut) => shortcut.label(),
        }
    }

    /// The switch key this pause key takes, so the two are never chosen together.
    pub fn switch_key(&self) -> Option<SwitchKey> {
        match self {
            PauseKey::Tap(tap) => tap.switch_key(),
            _ => None,
        }
    }

    fn from_json(value: &Value) -> Option<PauseKey> {
        if let Some(name) = value.as_str() {
            if name == "off" {
                return Some(PauseKey::Off);
            }
            return Tap::from_raw(name).map(PauseKey::Tap);
        }
        let object = value.as_object()?;
        let key_code = u16::try_from(object.get("keyCode")?.as_u64()?).ok()?;
        let modifiers = KeyModifiers::from_json(object.get("modifiers")?)?;
        let key = object.get("key")?.as_str()?.to_string();
        Some(PauseKey::Shortcut(Shortcut { key_code, modifiers, key }))
    }

    fn to_json(&self) -> Value {
        match self {
            PauseKey::Off => "off".into(),
            PauseKey::Tap(tap) => tap.raw().into(),
            PauseKey::Shortcut(s) => {
                let mut object = Map::new();
                object.insert("key".into(), s.key.clone().into());
                object.insert("keyCode".into(), s.key_code.into());
                object.insert("modifiers".into(), s.modifiers.to_json());
                Value::Object(object)
            }
        }
    }
}

#[derive(Clone, PartialEq, Debug)]
pub struct Settings {
    /// Detect romaji Japanese and convert it (kana preview, Space for kanji).
    pub japanese: bool,
    /// With Japanese on: show (and commit) the top kanji candidate while typing.
    pub live_conversion: bool,
    /// The key that pauses Araso in the text field (keys go through as typed) and resumes it.
    pub pause_key: PauseKey,
    /// Return on an underlined word only confirms it, like a Japanese IME; off, the app gets the
    /// Return too (a Japanese phrase is still only confirmed).
    pub return_confirms_only: bool,
    /// Once a day, ask GitHub for the latest release's version (nothing else is sent).
    pub check_for_updates: bool,
    /// Kanji conversion engine.
    pub japanese_engine: String,
    /// With Japanese on: digits after Japanese text are part of it, full-width (１２３); Space offers 123.
    pub full_width_digits: bool,
    /// With Japanese on: a space after Japanese text is full-width (Shift+Space types the other width).
    pub full_width_space: bool,
    /// The layout English and romaji are typed on. Korean 2-set stays on its keys either way.
    pub latin_layout: LatinLayout,
    /// The layout Korean is typed on.
    pub korean_layout: KoreanLayout,
    /// The layout romaji is typed on, when not English's (`latin_layout`): absent follows it.
    pub japanese_layout: Option<LatinLayout>,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            japanese: false,
            live_conversion: false,
            pause_key: PauseKey::STANDARD,
            return_confirms_only: true,
            check_for_updates: true,
            japanese_engine: "auto".into(),
            full_width_digits: false,
            full_width_space: false,
            latin_layout: LatinLayout::Qwerty,
            korean_layout: KoreanLayout::Dubeolsik,
            japanese_layout: None,
        }
    }
}

pub fn store_url() -> Option<PathBuf> {
    support_dir::file("settings.json")
}

impl Settings {
    /// One value of the wrong type ("true" for true) is that key's default, not the whole file's.
    pub fn from_json(object: &Map<String, Value>) -> Settings {
        let d = Settings::default();
        let bool_of = |key: &str, fallback: bool| object.get(key).and_then(Value::as_bool).unwrap_or(fallback);
        let str_of = |key: &str| object.get(key).and_then(Value::as_str);
        Settings {
            japanese: bool_of("japanese", d.japanese),
            live_conversion: bool_of("liveConversion", d.live_conversion),
            pause_key: object.get("pauseKey").and_then(PauseKey::from_json).unwrap_or(d.pause_key),
            return_confirms_only: bool_of("returnConfirmsOnly", d.return_confirms_only),
            check_for_updates: bool_of("checkForUpdates", d.check_for_updates),
            japanese_engine: str_of("japaneseEngine").map(str::to_string).unwrap_or(d.japanese_engine),
            full_width_digits: bool_of("fullWidthDigits", d.full_width_digits),
            full_width_space: bool_of("fullWidthSpace", d.full_width_space),
            latin_layout: str_of("latinLayout").and_then(LatinLayout::from_raw).unwrap_or(d.latin_layout),
            korean_layout: str_of("koreanLayout").and_then(KoreanLayout::from_raw).unwrap_or(d.korean_layout),
            japanese_layout: str_of("japaneseLayout").and_then(LatinLayout::from_raw),
        }
    }

    pub fn to_json(&self) -> Map<String, Value> {
        let mut o = Map::new();
        o.insert("japanese".into(), self.japanese.into());
        o.insert("liveConversion".into(), self.live_conversion.into());
        o.insert("pauseKey".into(), self.pause_key.to_json());
        o.insert("returnConfirmsOnly".into(), self.return_confirms_only.into());
        o.insert("checkForUpdates".into(), self.check_for_updates.into());
        o.insert("japaneseEngine".into(), self.japanese_engine.clone().into());
        o.insert("fullWidthDigits".into(), self.full_width_digits.into());
        o.insert("fullWidthSpace".into(), self.full_width_space.into());
        o.insert("latinLayout".into(), self.latin_layout.raw().into());
        o.insert("koreanLayout".into(), self.korean_layout.raw().into());
        if let Some(layout) = self.japanese_layout {
            o.insert("japaneseLayout".into(), layout.raw().into());
        }
        o
    }

    pub fn load(url: Option<&Path>) -> Settings {
        let Some(url) = url else { return Settings::default() };
        let Ok(data) = std::fs::read(url) else { return Settings::default() };
        match serde_json::from_slice::<Value>(&data) {
            Ok(Value::Object(object)) => Settings::from_json(&object),
            _ => {
                support_dir::set_aside(url);
                Settings::default()
            }
        }
    }

    pub fn load_default() -> Settings {
        Self::load(store_url().as_deref())
    }

    /// Writes these values over the file's, keeping any keys this version doesn't know.
    pub fn save(&self, url: Option<&Path>) -> std::io::Result<()> {
        let Some(url) = url else { return Ok(()) };
        let mut object = std::fs::read(url)
            .ok()
            .and_then(|d| serde_json::from_slice::<Value>(&d).ok())
            .and_then(|v| v.as_object().cloned())
            .unwrap_or_default();
        for (k, v) in self.to_json() {
            object.insert(k, v);
        }
        if self.japanese_layout.is_none() {
            object.remove("japaneseLayout"); // follows English again
        }
        let data = serde_json::to_vec_pretty(&Value::Object(object))?;
        support_dir::write_atomically(url, &data)
    }

    /// Makes the layouts the ones Araso types with.
    pub fn apply_layouts(&self) {
        LatinLayout::set_active(self.latin_layout);
        KoreanLayout::set_active(self.korean_layout);
        LatinLayout::set_active_romaji(self.japanese_layout);
    }
}
