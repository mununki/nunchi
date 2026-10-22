//! The user's dictionary, `dictionary.json`: keys typed as a whole word, and what they become.
//!
//! ```json
//! [{ "keys": "chrao", "as": "korean", "from": "learned", "strength": "weak", "last": "2026-10-17" },
//!  { "keys": "hoge", "as": "japanese", "from": "user" }]
//! ```
//!
//! An entry is a language (`as`) or the user's own text (`text`, kept but not yet typed).
//! `from: user` entries were added by hand and stay until removed. `from: learned` ones come
//! from corrections with the switch key: corrected once they are `weak` and only nudge the
//! decision (and are dropped after `WEAK_LIFETIME` days unused), corrected the same way again
//! they are `sure` and win.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::SystemTime;

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

use crate::decision::{Decision, DecisionContext, DecisionEngine, Language};
use crate::english_model::EnglishModel;
use crate::hangul;
use crate::japanese_segmenter;
use crate::jamo::DubeolsikLayout;
use crate::korean_model::KoreanModel;
use crate::mixed_word::{self, MixedWord, SPLIT_CONFIDENCE};
use crate::particle_splitter::ParticleSplitter;
use crate::romaji;
use crate::support_dir;
use crate::text;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    User,
    Learned,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Strength {
    Weak,
    Sure,
}

#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct Entry {
    /// Nil: every app. Else executable names, or prefixes ending in `.` as in apps.json.
    /// An app's own entry wins over the one for every app, for the keys as a whole only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub apps: Option<Vec<String>>,
    #[serde(rename = "as", default, skip_serializing_if = "Option::is_none")]
    pub language: Option<Language>,
    #[serde(rename = "from")]
    pub source: Source,
    pub keys: String,
    /// The day a learned entry was last corrected or typed (`yyyy-MM-dd`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last: Option<String>,
    /// Learned entries only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub strength: Option<Strength>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
}

impl Entry {
    pub fn new(
        keys: &str,
        language: Option<Language>,
        text: Option<String>,
        source: Source,
        strength: Option<Strength>,
        apps: Option<Vec<String>>,
        last: Option<String>,
    ) -> Entry {
        Entry {
            keys: keys.to_string(),
            language,
            text,
            source,
            strength: if source == Source::Learned { Some(strength.unwrap_or(Strength::Weak)) } else { None },
            apps: apps.filter(|a| !a.is_empty()),
            last,
        }
    }

    /// A word the user added by hand, in `language`.
    pub fn user(keys: &str, language: Language) -> Entry {
        Entry::new(keys, Some(language), None, Source::User, None, None, None)
    }

    /// A language entry that wins the decision: added by hand, or corrected twice.
    pub fn wins(&self) -> bool {
        self.language.is_some() && self.strength != Some(Strength::Weak)
    }

    fn for_every_app(&self) -> bool {
        self.apps.is_none()
    }

    /// One entry per keys and apps: adding the same pair again changes it.
    pub fn id(&self) -> String {
        let mut apps = self.apps.clone().unwrap_or_default();
        apps.sort();
        format!("{}\u{1F}{}", self.keys, apps.join(","))
    }

    pub fn applies_to(&self, app: Option<&str>) -> bool {
        let Some(apps) = &self.apps else { return true };
        let Some(app) = app else { return false };
        apps.iter().any(|a| a == app || (a.ends_with('.') && app.starts_with(a.as_str())))
    }
}

#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct UserDictionary {
    entries: Vec<Entry>,
}

/// Days a weak entry lasts without being corrected or typed again.
pub const WEAK_LIFETIME: i64 = 30;
/// Longer tokens aren't learned: a sentence switched as one token isn't a word.
pub const LONGEST_LEARNED: usize = 20;

pub fn day(date: NaiveDate) -> String {
    date.format("%Y-%m-%d").to_string()
}

pub fn date(day: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(day, "%Y-%m-%d").ok()
}

impl UserDictionary {
    pub fn new(entries: Vec<Entry>) -> Self {
        UserDictionary { entries }
    }

    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// The language entry for keys typed in `app`: the app's own one, else the one for every app.
    pub fn language_entry(&self, keys: &str, app: Option<&str>) -> Option<&Entry> {
        let matching: Vec<&Entry> =
            self.entries.iter().filter(|e| e.keys == keys && e.language.is_some() && e.applies_to(app)).collect();
        matching.iter().find(|e| !e.for_every_app()).or(matching.first()).copied()
    }

    /// Adds the entry, or replaces the one with the same keys and apps (a word learned for every
    /// app becomes the user's).
    pub fn add(&mut self, entry: Entry) {
        if entry.keys.is_empty() || (entry.language.is_none() && entry.text.as_deref().unwrap_or("").is_empty()) {
            return;
        }
        let id = entry.id();
        match self.entries.iter().position(|e| e.id() == id) {
            Some(index) => self.entries[index] = entry,
            None => self.entries.push(entry),
        }
    }

    pub fn remove(&mut self, id: &str) -> bool {
        let before = self.entries.len();
        self.entries.retain(|e| e.id() != id);
        self.entries.len() != before
    }

    /// A learned word becomes one the user added: it stays, and corrections no longer change it.
    pub fn pin(&mut self, id: &str) -> bool {
        let Some(entry) = self.entries.iter_mut().find(|e| e.id() == id && e.source == Source::Learned) else {
            return false;
        };
        entry.source = Source::User;
        entry.strength = None;
        entry.last = None;
        true
    }

    /// Records that `keys` were corrected to `language` while `shown` was on screen.
    /// Returns true when the correction made the entry sure.
    ///
    /// - Back to what the engine showed, or away from a weak entry's language while it was
    ///   shown (the nudge turned the word wrongly): the learned entry goes.
    /// - The same correction as the weak entry: sure.
    /// - Otherwise: a weak entry for `language`, replacing any learned one.
    ///
    /// Keys the user added by hand aren't changed by corrections.
    pub fn learn(&mut self, keys: &str, language: Language, shown: Language, today: NaiveDate) -> bool {
        if keys.is_empty() || self.entries.iter().any(|e| e.keys == keys && e.for_every_app() && e.source == Source::User) {
            return false;
        }
        let index = self.entries.iter().position(|e| e.keys == keys && e.for_every_app() && e.source == Source::Learned);
        let current = index.map(|i| self.entries[i].clone());
        if current.as_ref().is_some_and(|c| c.language == Some(language) && c.strength == Some(Strength::Sure)) {
            return false;
        }
        // Back to what the engine showed, or away from what a weak entry turned it to: the
        // nudge was wrong, and a weak entry for the reading switched back to isn't a correction.
        let rejected =
            current.as_ref().is_some_and(|c| c.strength == Some(Strength::Weak) && c.language == Some(shown));
        if language == shown || rejected {
            if let Some(index) = index {
                self.entries.remove(index);
            }
            return false;
        }
        if text::len(keys) > LONGEST_LEARNED {
            return false;
        }
        let sure = current.as_ref().is_some_and(|c| c.language == Some(language));
        let entry = Entry::new(
            keys,
            Some(language),
            None,
            Source::Learned,
            Some(if sure { Strength::Sure } else { Strength::Weak }),
            None,
            Some(day(today)),
        );
        match index {
            Some(index) => self.entries[index] = entry,
            None => self.entries.push(entry),
        }
        sure
    }

    /// A weak entry's word went in as it says: it is still in use, so it stays.
    /// Returns true when something changed.
    pub fn used(&mut self, keys: &str, language: Language, today: NaiveDate) -> bool {
        let today = day(today);
        let Some(entry) =
            self.entries.iter_mut().find(|e| e.keys == keys && e.for_every_app() && e.strength == Some(Strength::Weak))
        else {
            return false;
        };
        if entry.language != Some(language) || entry.last.as_deref() == Some(today.as_str()) {
            return false;
        }
        entry.last = Some(today);
        true
    }

    /// Removes the language entries for the keys (the engine decides them again).
    /// Returns true when something was removed.
    pub fn forget(&mut self, keys: &str) -> bool {
        let before = self.entries.len();
        self.entries.retain(|e| !(e.keys == keys && e.language.is_some() && e.for_every_app()));
        self.entries.len() != before
    }

    /// Removes every learned entry; the user's own stay.
    pub fn forget_learned(&mut self) {
        self.entries.retain(|e| e.source != Source::Learned);
    }

    /// Days until a weak entry is dropped unless corrected or typed again; nil for others.
    pub fn days_left(entry: &Entry, today: NaiveDate) -> Option<i64> {
        if entry.strength != Some(Strength::Weak) {
            return None;
        }
        let last = entry.last.as_deref().and_then(date)?;
        Some((WEAK_LIFETIME - (today - last).num_days()).max(0))
    }

    /// What the entry writes: the user's text, or the keys in its language.
    pub fn reading(keys: &str, language: Language) -> String {
        match language {
            Language::English => keys.to_string(),
            Language::Korean => hangul::text(keys).unwrap_or_else(|| keys.to_string()),
            Language::Mixed => MixedWord::split(keys).map(|m| m.text()).unwrap_or_else(|| keys.to_string()),
            Language::Japanese => romaji::compose(keys, false).text,
        }
    }

    /// Drops weak entries not corrected or typed for `WEAK_LIFETIME` days.
    pub fn drop_expired(&mut self, today: NaiveDate) -> bool {
        let before = self.entries.len();
        self.entries.retain(|entry| {
            if entry.strength != Some(Strength::Weak) {
                return true;
            }
            let Some(last) = entry.last.as_deref().and_then(date) else { return false };
            (today - last).num_days() < WEAK_LIFETIME
        });
        self.entries.len() != before
    }

    pub fn decode(data: &[u8]) -> Option<UserDictionary> {
        let entries: Vec<Entry> = serde_json::from_slice(data).ok()?;
        Some(UserDictionary::new(
            entries
                .into_iter()
                .filter(|e| !e.keys.is_empty() && (e.language.is_some() || !e.text.as_deref().unwrap_or("").is_empty()))
                .collect(),
        ))
    }

    pub fn encoded(&self) -> Option<Vec<u8>> {
        serde_json::to_vec_pretty(&self.entries).ok()
    }
}

struct Store {
    dictionary: UserDictionary,
    /// The language entries by keys, so each key typed looks its keys (and the fronts a particle
    /// may follow) up instead of going through every entry.
    by_keys: HashMap<String, Vec<Entry>>,
    /// The keys most recently made sure in this process.
    last_learned_keys: Option<String>,
    /// When dictionary.json was last read or written here: another process (another app's
    /// input method) may change it.
    loaded: Option<SystemTime>,
}

impl Store {
    fn set(&mut self, dictionary: UserDictionary) {
        self.by_keys = index(&dictionary);
        self.dictionary = dictionary;
    }
}

fn index(dictionary: &UserDictionary) -> HashMap<String, Vec<Entry>> {
    let mut map: HashMap<String, Vec<Entry>> = HashMap::new();
    for entry in dictionary.entries.iter().filter(|e| e.language.is_some()) {
        map.entry(entry.keys.clone()).or_default().push(entry.clone());
    }
    map
}

type Today = Box<dyn Fn() -> NaiveDate + Send + Sync>;
type Saved = Box<dyn Fn() + Send + Sync>;

/// Wraps an engine with the user's dictionary: a language entry that wins (added by hand,
/// or corrected the same way twice) decides the keys, and also the keys with a particle
/// typed straight after them (촉매를, github에, githubで, hogeで); a weak one (corrected once)
/// passes the base engine a nudge toward its language, so only a close call turns.
pub struct DictionaryEngine {
    pub base: Arc<dyn DecisionEngine>,
    /// For keys that are an English word as a whole: no particle split there.
    pub english: Arc<EnglishModel>,
    /// Whether Japanese particles split off (Japanese on).
    pub japanese_particles: bool,
    /// For keys that read as a Korean word, or one with a particle, as a whole (놈들을, not sha들을).
    splitter: ParticleSplitter,
    store: Mutex<Store>,
    store_url: Option<PathBuf>,
    today: Today,
    /// Called after a change is written (a correction learned, a word typed again), so an open
    /// settings window can show it. Not after `reload`, which writes nothing of its own.
    saved: Mutex<Option<Saved>>,
}

pub fn default_store_url() -> Option<PathBuf> {
    support_dir::file("dictionary.json")
}

pub fn local_today() -> NaiveDate {
    chrono::Local::now().date_naive()
}

impl DictionaryEngine {
    pub fn new(
        base: Arc<dyn DecisionEngine>,
        english: Arc<EnglishModel>,
        korean: KoreanModel,
        japanese_particles: bool,
        store_url: Option<PathBuf>,
        today: Option<Today>,
    ) -> Self {
        let today = today.unwrap_or_else(|| Box::new(local_today));
        let (dictionary, loaded) = Self::read(store_url.as_ref(), today());
        let store = Store { by_keys: index(&dictionary), dictionary, last_learned_keys: None, loaded };
        DictionaryEngine {
            base,
            splitter: ParticleSplitter::new(english.clone(), korean, None),
            english,
            japanese_particles,
            store: Mutex::new(store),
            store_url,
            today,
            saved: Mutex::new(None),
        }
    }

    fn lock(&self) -> MutexGuard<'_, Store> {
        self.store.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn set_saved(&self, saved: impl Fn() + Send + Sync + 'static) {
        *self.saved.lock().unwrap_or_else(|e| e.into_inner()) = Some(Box::new(saved));
    }

    fn read(url: Option<&PathBuf>, today: NaiveDate) -> (UserDictionary, Option<SystemTime>) {
        let Some(url) = url else { return (UserDictionary::default(), None) };
        let modified = support_dir::modified(url);
        let Ok(data) = std::fs::read(url) else { return (UserDictionary::default(), None) };
        let Some(mut saved) = UserDictionary::decode(&data) else {
            // Unreadable (hand-edited, or from a newer version): keep it aside, don't overwrite it.
            support_dir::set_aside(url);
            return (UserDictionary::default(), None);
        };
        if saved.drop_expired(today) {
            Self::write(&saved, url);
            return (saved, support_dir::modified(url));
        }
        (saved, modified)
    }

    pub fn dictionary(&self) -> UserDictionary {
        self.lock().dictionary.clone()
    }

    /// Language entries that win: keys → language.
    pub fn learned(&self) -> HashMap<String, Language> {
        Self::words(self.lock().dictionary.entries.iter().filter(|e| e.wins() && e.apps.is_none()))
    }

    /// Whether the keys have a winning entry for every app.
    pub fn has_learned(&self, keys: &str) -> bool {
        self.lock().by_keys.get(keys).is_some_and(|entries| entries.iter().any(|e| e.wins() && e.apps.is_none()))
    }

    /// Words corrected once: the same correction again makes them sure.
    pub fn pending(&self) -> HashMap<String, Language> {
        Self::words(
            self.lock().dictionary.entries.iter().filter(|e| e.strength == Some(Strength::Weak) && e.apps.is_none()),
        )
    }

    /// The keys most recently made sure in this process, while still sure.
    pub fn last_learned(&self) -> Option<String> {
        let store = self.lock();
        let keys = store.last_learned_keys.clone()?;
        (store.dictionary.language_entry(&keys, None).and_then(|e| e.strength) == Some(Strength::Sure)).then_some(keys)
    }

    fn words<'a>(entries: impl Iterator<Item = &'a Entry>) -> HashMap<String, Language> {
        let mut map = HashMap::new();
        for entry in entries {
            if let Some(language) = entry.language {
                map.entry(entry.keys.clone()).or_insert(language);
            }
        }
        map
    }

    /// Re-reads dictionary.json (after the settings window, or another app's input method,
    /// changed it).
    pub fn reload(&self) {
        let (fresh, loaded) = Self::read(self.store_url.as_ref(), (self.today)());
        let mut store = self.lock();
        store.set(fresh);
        store.loaded = loaded;
    }

    /// Re-reads dictionary.json when it changed since it was read or written here.
    pub fn reload_if_changed(&self) {
        let Some(url) = &self.store_url else { return };
        let modified = support_dir::modified(url);
        if modified != self.lock().loaded {
            self.reload();
        }
    }

    /// Forgets the keys' language, learned or added: the engine decides them again.
    pub fn forget(&self, keys: &str) {
        self.change(|d| d.forget(keys));
    }

    /// Adds a word, or changes the one with the same keys and apps.
    pub fn add(&self, entry: Entry) {
        self.change(|d| {
            d.add(entry);
            true
        });
    }

    pub fn remove(&self, id: &str) {
        self.change(|d| d.remove(id));
    }

    /// Keeps a learned word for good, as if the user had added it.
    pub fn pin(&self, id: &str) {
        self.change(|d| d.pin(id));
    }

    /// Forgets every learned word; words the user added stay.
    pub fn forget_all(&self) {
        self.change(|d| {
            d.forget_learned();
            true
        });
    }

    /// Applies `change` to the latest copy of the file (another process may have written it) and
    /// writes it when `change` says something changed.
    fn change(&self, change: impl FnOnce(&mut UserDictionary) -> bool) {
        self.reload_if_changed();
        let snapshot = {
            let mut store = self.lock();
            let mut dictionary = store.dictionary.clone();
            if !change(&mut dictionary) {
                return;
            }
            store.set(dictionary.clone());
            dictionary
        };
        self.persist(&snapshot);
    }

    /// `UserDictionary::language_entry`, from the index: the app's own entry, else the one for every app.
    fn entry(&self, keys: &str, app: Option<&str>) -> Option<Entry> {
        let store = self.lock();
        let matching: Vec<&Entry> = store.by_keys.get(keys)?.iter().filter(|e| e.applies_to(app)).collect();
        matching.iter().find(|e| e.apps.is_some()).or(matching.first()).map(|e| (*e).clone())
    }

    /// A word that wins, with a particle or ending typed straight after it: Korean as a whole
    /// (촉매를, the particle in the form the word takes), English + Korean particle as mixed
    /// (github에), English or Japanese + Japanese particle as Japanese (githubで, hogeで). The
    /// longest word first; never keys that are an English word as a whole, and not after a
    /// one-key word. An English word, as in `ParticleSplitter`, needs 3 letters unless it has
    /// capitals (so + が is そが), and doesn't split keys that read as a Korean word or one with
    /// a particle (sha + 들을 is 놈들을) or as clean romaji (toki + は is ときは). The same
    /// confidence as the engine's own splits, so a Strict app splits too.
    fn with_particle(&self, keys: &str) -> Option<Decision> {
        let chars: Vec<char> = keys.chars().collect();
        if chars.len() < 3 || self.english.is_known_word(keys) {
            return None;
        }
        let fronts: Vec<Entry> = {
            let store = self.lock();
            if store.by_keys.is_empty() {
                return None;
            }
            (2..chars.len())
                .rev()
                .filter_map(|length| {
                    let front: String = chars[..length].iter().collect();
                    store.by_keys.get(&front)?.iter().find(|e| e.wins() && e.apps.is_none()).cloned()
                })
                .collect()
        };
        for word in fronts {
            let Some(language) = word.language else { continue };
            let tail = text::drop_first(keys, text::len(&word.keys));
            let how = format!("{}{}+particle", if word.source == Source::User { "added " } else { "learned " }, word.keys);
            match language {
                Language::Korean => {
                    let Some(front) = hangul::compose(&word.keys).filter(|f| !f.has_stray_jamo()) else { continue };
                    let Some(particle) = Self::korean_particle(tail) else { continue };
                    let front_text = front.text();
                    let agrees = text::last(&front_text)
                        .and_then(MixedWord::final_sound)
                        .and_then(|f| MixedWord::particle_agrees(&particle, f));
                    if agrees == Some(false) {
                        continue;
                    }
                    return Some(Decision::new(Language::Korean, SPLIT_CONFIDENCE, front_text + &particle, how));
                }
                Language::English => {
                    if !(text::len(&word.keys) >= 3 || word.keys.chars().any(|c| c.is_uppercase())) {
                        continue;
                    }
                    if let Some(particle) = Self::korean_particle(tail) {
                        if !self.reads_as_korean_word(keys) {
                            let mixed = MixedWord { english: word.keys.clone(), korean_keys: tail.to_string(), korean: particle };
                            return Some(Decision::new(Language::Mixed, SPLIT_CONFIDENCE, mixed.text(), how));
                        }
                    }
                    if self.japanese_particles && !romaji::compose(keys, false).is_clean() {
                        if let Some(particle) = Self::japanese_particle(tail) {
                            return Some(Decision::new(
                                Language::Japanese,
                                SPLIT_CONFIDENCE,
                                format!("{}{}", word.keys, particle),
                                how,
                            ));
                        }
                    }
                }
                Language::Japanese => {
                    let front = romaji::compose(&word.keys, false);
                    if !self.japanese_particles || !front.is_clean() {
                        continue;
                    }
                    let Some(particle) = Self::japanese_particle(tail) else { continue };
                    return Some(Decision::new(Language::Japanese, SPLIT_CONFIDENCE, front.text + &particle, how));
                }
                Language::Mixed => continue,
            }
        }
        None
    }

    fn reads_as_korean_word(&self, keys: &str) -> bool {
        let Some(composed) = hangul::compose(keys) else { return false };
        !composed.has_stray_jamo() && self.splitter.is_korean_word_or_word_with_particle(&composed.text())
    }

    /// The tail as one of the particles and endings that attach to a word (에, 를, 에서, 했어).
    fn korean_particle(tail: &str) -> Option<String> {
        if !text::first(tail).is_some_and(DubeolsikLayout::starts_syllable) {
            return None;
        }
        let composed = hangul::compose(tail)?;
        let korean = composed.text();
        (!composed.has_stray_jamo() && mixed_word::is_suffix(&korean)).then_some(korean)
    }

    /// The tail as a Japanese particle (で, に, から).
    fn japanese_particle(tail: &str) -> Option<String> {
        let composed = romaji::compose(tail, false);
        (composed.is_clean() && japanese_segmenter::is_trailing_particle(&composed.text)).then_some(composed.text)
    }

    /// Records that `keys` were corrected to `language`: once is weak, the same again sure,
    /// back to the engine's choice forgets the word.
    ///
    /// Pass `engine_choice` when the caller already knows what was shown (the input method
    /// always does: a fresh decision might not match).
    pub fn learn(&self, keys: &str, language: Language, engine_choice: Option<Language>) {
        let shown = engine_choice.unwrap_or_else(|| self.decide(keys, &DecisionContext::default()).language);
        let today = (self.today)();
        self.reload_if_changed();
        let snapshot = {
            let mut store = self.lock();
            let mut dictionary = store.dictionary.clone();
            let sure = dictionary.learn(keys, language, shown, today);
            if dictionary == store.dictionary {
                return;
            }
            if sure {
                store.last_learned_keys = Some(keys.to_string());
            }
            store.set(dictionary.clone());
            dictionary
        };
        self.persist(&snapshot);
    }

    /// The word went in as `language` without a correction: a weak entry saying so stays.
    pub fn used(&self, keys: &str, language: Language) {
        let today = (self.today)();
        {
            // Most words have no entry: nothing to read or write for them.
            let store = self.lock();
            if !store.by_keys.get(keys).is_some_and(|e| e.iter().any(|e| e.strength == Some(Strength::Weak))) {
                return;
            }
        }
        self.change(|d| d.used(keys, language, today));
    }

    fn persist(&self, snapshot: &UserDictionary) {
        let Some(url) = &self.store_url else { return };
        Self::write(snapshot, url);
        self.lock().loaded = support_dir::modified(url);
        if let Some(saved) = self.saved.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
            saved();
        }
    }

    fn write(dictionary: &UserDictionary, url: &PathBuf) {
        if let Some(data) = dictionary.encoded() {
            let _ = support_dir::write_atomically(url, &data);
        }
    }
}

impl DecisionEngine for DictionaryEngine {
    fn decide(&self, keys: &str, context: &DecisionContext) -> Decision {
        let entry = self.entry(keys, context.app.as_deref());
        if let Some(entry) = &entry {
            if entry.wins() {
                if let Some(language) = entry.language {
                    // Rendered by the base engine: this engine's own render(Mixed) would ask it to
                    // decide the keys again, find them in the dictionary, and so on without end.
                    let explanation = if entry.source == Source::User { "added" } else { "learned" };
                    return Decision::new(language, 1.0, self.base.render(keys, language), explanation);
                }
            }
        }
        if let Some(split) = self.with_particle(keys) {
            return split;
        }
        let mut context = context.clone();
        if let Some(entry) = &entry {
            if entry.strength == Some(Strength::Weak) {
                context.nudge = entry.language;
            }
        }
        self.base.decide(keys, &context)
    }
}
