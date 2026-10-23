//! How Nunchi behaves in each app: `%APPDATA%\Nunchi\apps.json`. Apps are named by their
//! executable (`windowsterminal.exe`, `code.exe`), in lowercase.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

use crate::support_dir;

/// A key that switches the word being typed, or the one just committed, to its next reading.
/// A key not chosen for an app goes to that app.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum SwitchKey {
    /// The 한/영 key (VK_HANGUL): where Korean keyboards switch languages anyway.
    HangulKey,
    /// Right Alt pressed and released on its own. Apps ignore a lone modifier, so unlike Esc it
    /// never closes a window. (macOS Araso's right ⌥, `rightOption` there.)
    RightAlt,
    /// Right Shift pressed and released on its own.
    RightShift,
    /// Esc. While a word is being typed or was just committed; otherwise Esc goes to the app.
    Escape,
    /// Shift+Esc. Dropped when there is nothing to change, never passed to the app.
    ShiftEscape,
}

impl SwitchKey {
    pub const ALL: [SwitchKey; 5] =
        [SwitchKey::HangulKey, SwitchKey::RightAlt, SwitchKey::RightShift, SwitchKey::Escape, SwitchKey::ShiftEscape];

    pub fn raw(self) -> &'static str {
        match self {
            SwitchKey::HangulKey => "hangulKey",
            SwitchKey::RightAlt => "rightAlt",
            SwitchKey::RightShift => "rightShift",
            SwitchKey::Escape => "escape",
            SwitchKey::ShiftEscape => "shiftEscape",
        }
    }

    /// Also macOS Araso's name for the same key, so its apps.json reads here too.
    pub fn from_raw(raw: &str) -> Option<SwitchKey> {
        if raw == "rightOption" {
            return Some(SwitchKey::RightAlt);
        }
        Self::ALL.into_iter().find(|k| k.raw() == raw)
    }
}

pub type SwitchKeys = BTreeSet<SwitchKey>;

/// How eagerly Nunchi converts in a given app.
#[derive(Clone, PartialEq, Debug)]
pub struct AppPolicy {
    /// When false, Nunchi passes every key through (plain English typing).
    pub enabled: bool,
    /// Convert away from the typed English letters only when the engine is at least
    /// this confident. On the held-out set, raising it above 0.5 left more Korean
    /// words as letters without catching more wrong conversions, so it only pays
    /// off where a wrong conversion is expensive (terminals, editors).
    pub conversion_threshold: f64,
    /// The keys that switch the word's language here; the others go to the app. (Inside a vim
    /// that reports its modes, Esc is vim's whatever this says.)
    pub switch_keys: SwitchKeys,
    /// Space or a symbol doesn't end the word but goes inside the underline too. Held there, it is
    /// still the word being typed: a switch key changes it, Delete deletes (the boundary first), and
    /// the next word puts it in with that character. For terminals, which can't hand a committed
    /// word back for a change.
    pub hold_space: bool,
    /// Full-width digits and spaces after Japanese (when turned on in the settings) apply here;
    /// false keeps them half-width in this app (a terminal, a code editor).
    pub full_width: bool,
}

/// The "strict" choice: convert only when quite sure (short shell commands and identifiers
/// collide with Hangul easily).
pub const STRICT_THRESHOLD: f64 = 0.97;

pub fn standard_switch_keys() -> SwitchKeys {
    [SwitchKey::HangulKey, SwitchKey::RightAlt, SwitchKey::ShiftEscape].into_iter().collect()
}

impl Default for AppPolicy {
    fn default() -> Self {
        AppPolicy {
            enabled: true,
            conversion_threshold: 0.5,
            switch_keys: standard_switch_keys(),
            hold_space: false,
            full_width: true,
        }
    }
}

impl AppPolicy {
    pub fn standard() -> Self {
        Self::default()
    }

    pub fn is_strict(&self) -> bool {
        self.conversion_threshold >= STRICT_THRESHOLD
    }
}

/// An `apps.json` entry: only the keys it names replace the default policy, so
/// `{"conversionThreshold": 0.97}` keeps whatever the default says about the switch keys.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct Override {
    pub enabled: Option<bool>,
    pub conversion_threshold: Option<f64>,
    pub switch_keys: Option<SwitchKeys>,
    pub hold_space: Option<bool>,
    pub full_width: Option<bool>,
}

impl Override {
    /// The keys of `policy` that differ from `base`.
    pub fn difference(policy: &AppPolicy, base: &AppPolicy) -> Override {
        Override {
            enabled: (policy.enabled != base.enabled).then_some(policy.enabled),
            conversion_threshold: (policy.conversion_threshold != base.conversion_threshold)
                .then_some(policy.conversion_threshold),
            switch_keys: (policy.switch_keys != base.switch_keys).then(|| policy.switch_keys.clone()),
            hold_space: (policy.hold_space != base.hold_space).then_some(policy.hold_space),
            full_width: (policy.full_width != base.full_width).then_some(policy.full_width),
        }
    }

    pub fn applied(&self, base: &AppPolicy) -> AppPolicy {
        AppPolicy {
            enabled: self.enabled.unwrap_or(base.enabled),
            conversion_threshold: self.conversion_threshold.unwrap_or(base.conversion_threshold),
            switch_keys: self.switch_keys.clone().unwrap_or_else(|| base.switch_keys.clone()),
            hold_space: self.hold_space.unwrap_or(base.hold_space),
            full_width: self.full_width.unwrap_or(base.full_width),
        }
    }

    /// A value of the wrong type ("0.97" for 0.97) is left out, as if the entry didn't name
    /// it, rather than failing the whole file; a switch key of a later version is skipped.
    pub fn from_json(value: &Value) -> Option<Override> {
        let object = value.as_object()?;
        Some(Override {
            enabled: object.get("enabled").and_then(Value::as_bool),
            conversion_threshold: object.get("conversionThreshold").and_then(Value::as_f64),
            switch_keys: object.get("switchKeys").and_then(Value::as_array).map(|keys| {
                keys.iter().filter_map(Value::as_str).filter_map(SwitchKey::from_raw).collect()
            }),
            hold_space: object.get("holdSpace").and_then(Value::as_bool),
            full_width: object.get("fullWidth").and_then(Value::as_bool),
        })
    }

    pub fn to_json(&self) -> Value {
        let mut object = Map::new();
        if let Some(v) = self.conversion_threshold {
            object.insert("conversionThreshold".into(), v.into());
        }
        if let Some(v) = self.enabled {
            object.insert("enabled".into(), v.into());
        }
        if let Some(v) = self.full_width {
            object.insert("fullWidth".into(), v.into());
        }
        if let Some(v) = self.hold_space {
            object.insert("holdSpace".into(), v.into());
        }
        if let Some(keys) = &self.switch_keys {
            object.insert("switchKeys".into(), keys.iter().map(|k| Value::from(k.raw())).collect::<Vec<_>>().into());
        }
        Value::Object(object)
    }
}

/// Picks the policy for an app from apps.json: the `"*"` entry is the user's default, and an
/// app's entry changes only the keys it names. Keys ending in "." match as a prefix. No app is
/// listed unless the user lists it.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct AppPolicies {
    pub overrides: BTreeMap<String, Override>,
}

/// The apps.json key of the default policy.
pub const DEFAULT_KEY: &str = "*";

pub fn store_url() -> Option<PathBuf> {
    support_dir::file("apps.json")
}

impl AppPolicies {
    /// Loads user overrides; a missing file means none. An entry that isn't an object is left
    /// out, and a file that isn't a JSON object is set aside as apps.json.unreadable, so the next
    /// save doesn't write over the user's entries.
    pub fn load(url: Option<&Path>) -> AppPolicies {
        let Some(url) = url else { return AppPolicies::default() };
        let Ok(data) = std::fs::read(url) else { return AppPolicies::default() };
        let Some(object) = serde_json::from_slice::<Value>(&data).ok().and_then(|v| v.as_object().cloned()) else {
            support_dir::set_aside(url);
            return AppPolicies::default();
        };
        let overrides = object
            .iter()
            .filter_map(|(id, entry)| Override::from_json(entry).map(|o| (id.to_lowercase(), o)))
            .collect();
        AppPolicies { overrides }
    }

    pub fn load_default() -> AppPolicies {
        Self::load(store_url().as_deref())
    }

    /// What every app uses unless its own entry says otherwise.
    pub fn default_policy(&self) -> AppPolicy {
        self.overrides.get(DEFAULT_KEY).map(|o| o.applied(&AppPolicy::standard())).unwrap_or_default()
    }

    /// The apps the user listed (the default entry excluded).
    pub fn apps(&self) -> Vec<String> {
        self.overrides.keys().filter(|k| *k != DEFAULT_KEY).cloned().collect()
    }

    pub fn policy(&self, app: Option<&str>) -> AppPolicy {
        let base = self.default_policy();
        let Some(id) = app.map(str::to_lowercase).filter(|id| id != DEFAULT_KEY) else { return base };
        match self.lookup(&id) {
            Some(entry) => entry.applied(&base),
            None => base,
        }
    }

    /// Sets the default, naming only what differs from Nunchi's own.
    pub fn set_default(&mut self, policy: &AppPolicy) {
        let entry = Override::difference(policy, &AppPolicy::standard());
        if entry == Override::default() {
            self.overrides.remove(DEFAULT_KEY);
        } else {
            self.overrides.insert(DEFAULT_KEY.into(), entry);
        }
    }

    /// Lists the app (if it wasn't) and makes it behave as `policy`, naming only the keys that
    /// differ from the default, so the rest keeps following it. The entry stays even when
    /// nothing differs: the user added the app, only removing it takes it off the list.
    pub fn set(&mut self, policy: &AppPolicy, app: &str) {
        let app = app.to_lowercase();
        if app == DEFAULT_KEY {
            return self.set_default(policy);
        }
        let entry = Override::difference(policy, &self.default_policy());
        self.overrides.insert(app, entry);
    }

    /// Writes the entries to apps.json. Callers load, change and save, so entries for other
    /// apps are kept as they were.
    pub fn save(&self, url: Option<&Path>) -> std::io::Result<()> {
        let Some(url) = url else { return Ok(()) };
        let object: Map<String, Value> = self.overrides.iter().map(|(k, v)| (k.clone(), v.to_json())).collect();
        let data = serde_json::to_vec_pretty(&Value::Object(object))?;
        support_dir::write_atomically(url, &data)
    }

    /// The exact entry, else the longest prefix that matches (jetbrains. over jet.).
    fn lookup(&self, id: &str) -> Option<&Override> {
        if let Some(exact) = self.overrides.get(id).filter(|_| id != DEFAULT_KEY) {
            return Some(exact);
        }
        self.overrides
            .iter()
            .filter(|(k, _)| *k != DEFAULT_KEY && k.ends_with('.') && id.starts_with(k.as_str()))
            .max_by_key(|(k, _)| k.len())
            .map(|(_, v)| v)
    }
}
