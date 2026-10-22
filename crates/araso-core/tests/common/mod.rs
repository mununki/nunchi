//! What the ported upstream tests share: the engines, a temporary dictionary, the policies.
#![allow(dead_code)]

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock};

use araso_core::app_policy::{Override, DEFAULT_KEY};
use araso_core::plain_field::PlainTextField;
use araso_core::{
    AppPolicies, DecisionEngine, DictionaryEngine, EnglishModel, HeuristicEngine, InputSession, Key, KoreanModel,
    SplittingEngine, SwitchKey, TextClient,
};
use araso_core::japanese_model::JapaneseModel;

/// `HeuristicEngine()` upstream: English and Korean, no splitting.
pub fn shared_engine() -> Arc<HeuristicEngine> {
    static ENGINE: OnceLock<Arc<HeuristicEngine>> = OnceLock::new();
    ENGINE.get_or_init(|| Arc::new(HeuristicEngine::new(EnglishModel::shared(), KoreanModel::new(), None))).clone()
}

/// The heuristic with Japanese on.
pub fn trilingual() -> Arc<HeuristicEngine> {
    static ENGINE: OnceLock<Arc<HeuristicEngine>> = OnceLock::new();
    ENGINE
        .get_or_init(|| Arc::new(HeuristicEngine::new(EnglishModel::shared(), KoreanModel::new(), JapaneseModel::shared())))
        .clone()
}

pub fn splitting(base: Arc<HeuristicEngine>) -> Arc<SplittingEngine> {
    Arc::new(SplittingEngine::new(base.clone(), base.english.clone(), base.korean.clone(), base.segmenter.clone()))
}

pub fn temp_path(name: &str) -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("araso-{name}-{}-{n}.json", std::process::id()))
}

pub fn dictionary(base: Arc<dyn DecisionEngine>, url: Option<PathBuf>) -> Arc<DictionaryEngine> {
    let japanese = false;
    Arc::new(DictionaryEngine::new(base, EnglishModel::shared(), KoreanModel::new(), japanese, url, None))
}

/// Esc and Shift+Esc switch languages everywhere (as `.escSwitches` upstream).
pub fn esc_switches() -> AppPolicies {
    let mut policies = AppPolicies::default();
    policies.overrides.insert(
        DEFAULT_KEY.into(),
        Override { switch_keys: Some([SwitchKey::Escape, SwitchKey::ShiftEscape].into_iter().collect()), ..Default::default() },
    );
    policies
}

/// Types `script` into `field`: letters and printable keys as themselves, "⏎" Return, "⎋" Esc,
/// "⇧" a switch key (Shift+Esc), "⌫" Delete, "←"/"→". Keys the input method passes on are
/// applied the way a text field would.
pub fn type_script(session: &mut InputSession, field: &mut PlainTextField, script: &str) -> Vec<bool> {
    script
        .chars()
        .map(|ch| {
            let (key, typed) = match ch {
                '⏎' => (Key::Enter, Some("\n".to_string())),
                '⎋' => (Key::Escape, None),
                '⇧' => (Key::Cycle, None),
                '←' => (Key::Left, None),
                '→' => (Key::Right, None),
                '⌫' => (Key::Delete, None),
                c if c.is_ascii_alphabetic() => (Key::Letter(c), Some(c.to_string())),
                c => (Key::Printable(c), Some(c.to_string())),
            };
            let handled = session.handle(key, field);
            if !handled {
                if let Some(text) = typed {
                    field.insert_text(&text, None);
                }
            }
            handled
        })
        .collect()
}

/// The upstream tests' converter: a few readings with kanji, every other reading as itself.
#[derive(Default)]
pub struct FakeConverter {
    pub learned: std::cell::RefCell<Vec<String>>,
}

impl araso_core::converter::JapaneseConverter for FakeConverter {
    fn candidates(&self, kana: &str) -> Vec<araso_core::converter::ConversionCandidate> {
        use araso_core::converter::ConversionCandidate as C;
        match kana {
            "きょうはいいてんきですね" => vec![C::new("今日はいい天気ですね", 12), C::new("今日は", 4), C::new(kana, 12)],
            "いいてんきですね" => vec![C::new("いい天気ですね", 8), C::new(kana, 8)],
            "へんかん" => vec![C::new("変換", 4), C::new("返還", 4), C::new("編纂", 4)],
            "しょう" => vec![C::new(kana, 3), C::new("翔", 3), C::new("小", 3)], // the engine puts the reading first
            "３がつ" => vec![C::new("三月", 3), C::new("３月", 3), C::new("3月", 3)],
            "3じ" => vec![C::new("三時", 2), C::new("３時", 2), C::new("3時", 2), C::new("3", 1)],
            "１２３" => vec![C::new("一二三", 3), C::new("123", 3), C::new(kana, 3)],
            _ => vec![C::new(kana, kana.chars().count())],
        }
    }

    fn learn(&self, candidate: &araso_core::converter::ConversionCandidate, _kana: &str) {
        self.learned.borrow_mut().push(candidate.text.clone());
    }

    fn save(&self) {}

    fn more_candidates(&self, kana: &str) -> Vec<araso_core::converter::ConversionCandidate> {
        use araso_core::converter::ConversionCandidate as C;
        if kana == "しょう" {
            vec![C::new("小", 3), C::new("硝", 3)]
        } else {
            Vec::new()
        }
    }
}

/// A dictionary over the trilingual splitting engine, as the Japanese tests use.
pub fn trilingual_dictionary(url: Option<PathBuf>) -> Arc<DictionaryEngine> {
    Arc::new(DictionaryEngine::new(splitting(trilingual()), EnglishModel::shared(), KoreanModel::new(), true, url, None))
}

/// Collects failures of a table of cases so one run shows them all.
pub struct Failures(Vec<String>);

impl Failures {
    pub fn new() -> Self {
        Failures(Vec::new())
    }

    pub fn check(&mut self, ok: bool, message: impl FnOnce() -> String) {
        if !ok {
            self.0.push(message());
        }
    }

    pub fn assert_none(self) {
        assert!(self.0.is_empty(), "{} failed:\n  {}", self.0.len(), self.0.join("\n  "));
    }
}
