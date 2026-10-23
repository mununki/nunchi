//! Character-trigram language model plus a word list with how common each word is, from
//! SCOWL (Data/english_words.txt: word<TAB>level, 10 = most common … 60). Both are built into
//! tables when the crate is built (`tables::english`): the letter trigrams trained on the
//! spelling list, as upstream trains them on macOS's `/usr/share/dict/words`.

use std::sync::{Arc, OnceLock};

use crate::embedded;
use crate::tables::{self, Table, ENGLISH_SYMBOLS, NOT_LISTED};

pub use crate::english_lists::COMMON_WORDS;
pub use crate::tables::ENGLISH_EXTRA_LEVEL as EXTRA_LEVEL;

const SMOOTHING: f32 = 0.5;

/// Words up to this level count as English words (a score floor, protection from kana).
pub const KNOWN_LEVEL: i32 = 50;
/// Words up to this level are everyday English (you, sense, machine, for, got).
pub const COMMON_LEVEL: i32 = 20;

pub struct EnglishModel {
    /// Each word (lowercase, letters only) with its level, and its level in the list itself:
    /// the level a guessed inflection may take from its stem.
    words: Table<'static>,
    /// Trigram counts (27³ f32), then context counts (27² f32).
    trigrams: &'static [u8],
}

impl EnglishModel {
    /// The model built into the crate; one instance.
    pub fn shared() -> Arc<EnglishModel> {
        static SHARED: OnceLock<Arc<EnglishModel>> = OnceLock::new();
        SHARED
            .get_or_init(|| {
                Arc::new(EnglishModel { words: Table::new(embedded::ENGLISH_WORDS), trigrams: embedded::ENGLISH_TRIGRAMS })
            })
            .clone()
    }

    pub fn word_count(&self) -> usize {
        self.words.len()
    }

    fn count(&self, index: usize) -> f32 {
        tables::f32_of(&self.trigrams[index * 4..])
    }

    /// Average natural-log probability per symbol (letters + end marker).
    /// Real English words land around -1.8…-2.4; keyboard mash around -3.5 or lower.
    pub fn average_log_probability(&self, token: &str) -> f64 {
        let n = ENGLISH_SYMBOLS;
        let codes = tables::english_codes(&token.to_lowercase());
        if codes.len() <= 3 {
            return -10.0;
        }
        let mut total = 0f64;
        for i in 2..codes.len() {
            let c = self.count((codes[i - 2] * n + codes[i - 1]) * n + codes[i]);
            let t = self.count(n * n * n + codes[i - 2] * n + codes[i - 1]);
            total += ((c + SMOOTHING) / (t + SMOOTHING * n as f32)).ln() as f64;
        }
        total / (codes.len() - 2) as f64
    }

    /// The word's level, also for regular inflections of a listed word (nil: not a word).
    pub fn level(&self, token: &str) -> Option<i32> {
        let w: String = token.to_lowercase().chars().filter(|&c| c != '\'').collect();
        if let Some(value) = self.words.get(w.as_bytes()) {
            return Some(value[0] as i32);
        }
        let count = w.chars().count();
        let mut best: Option<i32> = None;
        // SCOWL lists the inflections of its words, so a guessed one counts only for a stem the
        // list itself ranks above its rarest known level: thru (50, and 35 only as one of
        // `COMMON_WORDS`) + s is no word, and thrus is 소견.
        let mut consider = |stem: &str| {
            if let Some(value) = self.words.get(stem.as_bytes()) {
                let l = value[1];
                if l != NOT_LISTED && (l as i32) < KNOWN_LEVEL {
                    best = Some(best.map_or(l as i32, |b| b.min(l as i32)));
                }
            }
        };
        // Stems of 3+ letters: ths isn't the+s, ehs isn't eh+s.
        for suffix in ["s", "es", "ed", "d", "ing", "ly", "er", "ers", "est"] {
            if !(count > suffix.len() + 2 && w.ends_with(suffix)) {
                continue;
            }
            let stem = &w[..w.len() - suffix.len()];
            consider(stem);
            consider(&format!("{stem}e"));
            // running → run, stopped → stop
            let mut tail = stem.chars().rev();
            if let (Some(last), Some(before)) = (tail.next(), tail.next()) {
                if last == before {
                    consider(&stem[..stem.len() - last.len_utf8()]);
                }
            }
        }
        // stories → story, denied → deny, heavier → heavy
        for suffix in ["ies", "ied", "ier", "iest", "iers"] {
            if count > suffix.len() + 1 && w.ends_with(suffix) {
                consider(&format!("{}y", &w[..w.len() - suffix.len()]));
            }
        }
        best
    }

    /// A word people use (SCOWL level ≤ 50, or one of `COMMON_WORDS`).
    pub fn is_known_word(&self, token: &str) -> bool {
        self.level(token).is_some_and(|l| l <= KNOWN_LEVEL)
    }

    /// Everyday English (level ≤ 20): kept English even right after Korean or Japanese.
    pub fn is_common_word(&self, token: &str) -> bool {
        self.level(token).is_some_and(|l| l <= COMMON_LEVEL)
    }
}
