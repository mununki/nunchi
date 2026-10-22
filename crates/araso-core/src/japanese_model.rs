//! Kana trigram model for judging how Japanese a romaji string reads. Counts come
//! from Japanese sentences (Tatoeba, CC-BY 2.0 FR) turned into the romaji people
//! type and back into kana with `romaji::compose` (upstream tools/build_kana_model.py).
//! The kana model is built into tables when the crate is built (`tables::japanese`).
//!
//! Clean romaji isn't enough: "hello" composes to へっぉ, which this model scores
//! as very un-Japanese (っぉ essentially never occurs).

use std::borrow::Cow;
use std::sync::{Arc, OnceLock};

use crate::embedded;
use crate::resources;
use crate::romaji::{self, Romaji};
use crate::tables::{self, Table};

#[derive(Clone, Debug)]
pub struct JapaneseScore {
    /// Average log-probability per keystroke, comparable with the English and Korean models.
    pub average_log_probability: f64,
    pub composition: Romaji,
}

pub struct JapaneseModel {
    trigrams: Cow<'static, [u8]>,
    trigram_contexts: Cow<'static, [u8]>,
    bigrams: Cow<'static, [u8]>,
    bigram_contexts: Cow<'static, [u8]>,
    unigrams: Cow<'static, [u8]>,
    total: f32,
    vocabulary: f32,
}

const STRAY_LETTER_LOG_P: f64 = -13.0;
const PENDING_LOG_P: f64 = -3.5;
const START: char = '^';
const END: char = '$';

/// Non-letters that belong inside a Japanese phrase: ん (n'), ー, and the full-width symbols
/// (、。？！「」・…).
pub fn is_in_phrase_symbol(c: char) -> bool {
    c == '\'' || romaji::symbol(c).is_some()
}

impl JapaneseModel {
    fn from_tables(t: tables::Japanese) -> JapaneseModel {
        JapaneseModel {
            total: tables::f32_of(&t.totals[0..]),
            vocabulary: tables::f32_of(&t.totals[4..]),
            trigrams: Cow::Owned(t.trigrams),
            trigram_contexts: Cow::Owned(t.trigram_contexts),
            bigrams: Cow::Owned(t.bigrams),
            bigram_contexts: Cow::Owned(t.bigram_contexts),
            unigrams: Cow::Owned(t.unigrams),
        }
    }

    /// From trigram<TAB>count lines; nil without counts.
    pub fn from_text(text: &str) -> Option<JapaneseModel> {
        tables::japanese(text).map(Self::from_tables)
    }

    /// The kana model built into the crate.
    pub fn shared() -> Option<Arc<JapaneseModel>> {
        static SHARED: OnceLock<Arc<JapaneseModel>> = OnceLock::new();
        Some(
            SHARED
                .get_or_init(|| {
                    Arc::new(JapaneseModel {
                        trigrams: Cow::Borrowed(embedded::KANA_TRIGRAMS),
                        trigram_contexts: Cow::Borrowed(embedded::KANA_TRIGRAM_CONTEXTS),
                        bigrams: Cow::Borrowed(embedded::KANA_BIGRAMS),
                        bigram_contexts: Cow::Borrowed(embedded::KANA_BIGRAM_CONTEXTS),
                        unigrams: Cow::Borrowed(embedded::KANA_UNIGRAMS),
                        total: tables::f32_of(&embedded::KANA_TOTALS[0..]),
                        vocabulary: tables::f32_of(&embedded::KANA_TOTALS[4..]),
                    })
                })
                .clone(),
        )
    }

    /// The same kind of model over written Japanese (kanji + kana), for reranking conversions:
    /// read from `text_trigrams.txt` when there is a converter to rerank.
    pub fn text_model() -> Option<Arc<JapaneseModel>> {
        static TEXT: OnceLock<Option<Arc<JapaneseModel>>> = OnceLock::new();
        TEXT.get_or_init(|| resources::read(resources::TEXT_MODEL).and_then(|t| Self::from_text(&t)).map(Arc::new))
            .clone()
    }

    fn count(table: &[u8], chars: &[char]) -> Option<f32> {
        let mut buffer = [0u8; 16];
        Table::new(table).get(tables::key_of(&mut buffer, chars)).map(tables::f32_of)
    }

    /// Interpolated trigram probability of `c` after `a b`.
    fn probability(&self, a: char, b: char, c: char) -> f64 {
        let p1 = (Self::count(&self.unigrams, &[c]).unwrap_or(0.0) + 1.0) / (self.total + self.vocabulary);
        let p2 = Self::count(&self.bigram_contexts, &[b])
            .map(|ctx| Self::count(&self.bigrams, &[b, c]).unwrap_or(0.0) / ctx)
            .unwrap_or(0.0);
        let p3 = Self::count(&self.trigram_contexts, &[a, b])
            .map(|ctx| Self::count(&self.trigrams, &[a, b, c]).unwrap_or(0.0) / ctx)
            .unwrap_or(0.0);
        (0.6 * p3 + 0.3 * p2 + 0.1 * p1) as f64
    }

    /// Sum of log-probabilities of the kana (and the end of word unless `partial`).
    pub fn log_probability(&self, kana: &str, partial: bool) -> f64 {
        let mut seq = vec![START, START];
        seq.extend(kana.chars());
        if !partial {
            seq.push(END);
        }
        let mut sum = 0.0;
        for i in 2..seq.len() {
            sum += self.probability(seq[i - 2], seq[i - 1], seq[i]).ln();
        }
        sum
    }

    pub fn score(&self, keys: &str, partial: bool) -> Option<JapaneseScore> {
        if keys.is_empty() || !keys.chars().all(|c| c.is_ascii() && (c.is_alphabetic() || is_in_phrase_symbol(c))) {
            return None;
        }
        let composition = romaji::compose(keys, partial);
        let pending = composition.pending.chars().count();
        let kana: String = crate::text::drop_last(&composition.text, pending)
            .chars()
            .filter(|&c| !c.is_ascii() && !romaji::is_punctuation(c))
            .collect();
        let mut total = self.log_probability(&kana, partial);
        total += composition.stray_letters as f64 * STRAY_LETTER_LOG_P;
        if !composition.pending.is_empty() {
            total += PENDING_LOG_P;
        }
        Some(JapaneseScore { average_log_probability: total / crate::text::len(keys) as f64, composition })
    }
}
