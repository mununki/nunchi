//! Languages, decisions and the heuristic engine that scores keys as English, Korean and Japanese.

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::english_model::{self, EnglishModel};
use crate::hangul;
use crate::japanese_model::JapaneseModel;
use crate::japanese_segmenter::{JapaneseSegmenter, Segment};
use crate::jamo::DubeolsikLayout;
use crate::korean_model::{KoreanModel, KoreanScore};
use crate::mixed_word::MixedWord;
use crate::romaji;
use crate::text;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    English,
    Korean,
    /// English word + Korean particle/ending, e.g. `github에` (see `MixedWord`).
    Mixed,
    /// Romaji typed as Japanese (rendered as hiragana; kanji conversion happens on Space).
    Japanese,
}

impl Language {
    pub fn raw(self) -> &'static str {
        match self {
            Language::English => "english",
            Language::Korean => "korean",
            Language::Mixed => "mixed",
            Language::Japanese => "japanese",
        }
    }

    pub fn from_raw(raw: &str) -> Option<Language> {
        [Language::English, Language::Korean, Language::Mixed, Language::Japanese].into_iter().find(|l| l.raw() == raw)
    }

    /// The language a sentence continues in after this word.
    pub fn sentence_language(self) -> Language {
        match self {
            Language::English => Language::English,
            Language::Korean | Language::Mixed => Language::Korean,
            Language::Japanese => Language::Japanese,
        }
    }

    /// What Esc switches to: english → mixed (when the keys split) → korean →
    /// japanese (when enabled) → english. Readings that can't exist for these keys
    /// are skipped (no Korean for "ra-men"), and so are garbled ones (dkssud as
    /// Japanese: dkっすd; loose consonants and ㅜ/ㅠ such as ㅁㅊ or ㅜㅜ aren't garbled) as long
    /// as a clean alternative exists — a typo can still be forced into its language when nothing
    /// reads cleanly. `splits`: whether the engine splits the keys (x에, hello월드 as well as
    /// github에); left out, the particle split decides.
    pub fn next(self, keys: &str, japanese: bool, splits: Option<bool>) -> Language {
        let order = [Language::English, Language::Mixed, Language::Korean, Language::Japanese];
        let composed = hangul::compose(keys);
        let available: Vec<Language> = order
            .into_iter()
            .filter(|language| match language {
                Language::English => true,
                Language::Mixed => splits.unwrap_or_else(|| MixedWord::split(keys).is_some()),
                Language::Korean => composed.is_some(),
                Language::Japanese => japanese,
            })
            .collect();
        let clean: Vec<Language> = available
            .iter()
            .copied()
            .filter(|language| match language {
                Language::English | Language::Mixed => true,
                Language::Korean => {
                    composed.as_ref().is_some_and(|c| !c.has_stray_jamo() || c.is_jamo_abbreviation())
                }
                Language::Japanese => romaji::compose(keys, false).stray_letters == 0,
            })
            .collect();
        let cycle: Vec<Language> = if clean.len() >= 2 {
            available.into_iter().filter(|l| clean.contains(l) || *l == self).collect()
        } else {
            available
        };
        match cycle.iter().position(|&l| l == self) {
            Some(index) => cycle[(index + 1) % cycle.len()],
            None => cycle.first().copied().unwrap_or(Language::English),
        }
    }
}

/// What the engine knows besides the token itself.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DecisionContext {
    /// Language of the previously committed word, if it is recent enough to matter.
    pub previous_language: Option<Language>,
    /// The token is still being typed (used for the live preview).
    pub partial: bool,
    /// The language the user corrected these keys to once (a weak dictionary entry).
    pub nudge: Option<Language>,
    /// The app typed into, for the user's dictionary entries limited to apps.
    pub app: Option<String>,
}

impl DecisionContext {
    pub fn new(previous_language: Option<Language>, partial: bool, app: Option<&str>) -> Self {
        DecisionContext { previous_language, partial, nudge: None, app: app.map(str::to_string) }
    }

    pub fn previous(previous_language: Option<Language>) -> Self {
        DecisionContext { previous_language, ..Default::default() }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Decision {
    pub language: Language,
    /// 0.5 = coin flip, 1.0 = certain.
    pub confidence: f64,
    /// What should be committed for this decision.
    pub output: String,
    /// Human-readable reasoning, for the CLI and debugging.
    pub explanation: String,
    /// The second most likely language, when the engine compared several.
    pub runner_up: Option<Language>,
}

impl Decision {
    pub fn new(language: Language, confidence: f64, output: impl Into<String>, explanation: impl Into<String>) -> Self {
        Decision { language, confidence, output: output.into(), explanation: explanation.into(), runner_up: None }
    }
}

/// A local decider that maps raw QWERTY keystrokes of one word to a language.
/// The heuristic engine below is the default; a model-backed engine can implement the same trait.
pub trait DecisionEngine: Send + Sync {
    fn decide(&self, keys: &str, context: &DecisionContext) -> Decision;

    /// Renders `keys` in a given language regardless of what the engine would pick.
    fn render(&self, keys: &str, language: Language) -> String {
        match language {
            Language::English => keys.to_string(),
            Language::Korean => hangul::text(keys).unwrap_or_else(|| keys.to_string()),
            Language::Mixed => {
                // The engine's own split (hello월드, github에); the particle split for engines without one.
                let decision = self.decide(keys, &DecisionContext::default());
                if decision.language == Language::Mixed {
                    decision.output
                } else {
                    MixedWord::split(keys).map(|m| m.text()).unwrap_or_else(|| keys.to_string())
                }
            }
            Language::Japanese => romaji::compose(keys, false).text,
        }
    }
}

/// Scores the same keystrokes as English (character trigrams + word list), Korean
/// (2-set syllables) and, when enabled, Japanese (romaji → kana trigrams), each as
/// average log-probability per keystroke, nudges the previous word's language, and
/// picks the best. Confidence comes from the gap to the runner-up.
pub struct HeuristicEngine {
    pub english: Arc<EnglishModel>,
    pub korean: KoreanModel,
    /// Nil when Japanese is off or its model isn't available.
    pub japanese: Option<Arc<JapaneseModel>>,
    pub segmenter: Option<Arc<JapaneseSegmenter>>,

    /// Added to the previous word's language score: people mostly stay in one language.
    pub context_bias: f64,
    /// Bias after Japanese text. Raising it to 1.2 recovered 1 of 2,590 held-out phrases
    /// but tripled common English words turning into kana right after Japanese (0.5% → 1.6%).
    pub japanese_context_bias: f64,
    /// Shifts Japanese scores to the English/Korean scale (kana trigrams over ~80 symbols
    /// per ~2 keys read differently from letter trigrams). Tuned on held-out data so that
    /// English words almost never turn into kana.
    pub japanese_offset: f64,
    /// Shifts Korean scores (used when the syllable model changes scale).
    pub korean_offset: f64,
    /// Taken off the dictionary floor for rarer English words (SCOWL levels 40–50: dork,
    /// ahem), so a common Korean word with the same keys (애가, 모든) wins the tie.
    pub rare_word_floor_penalty: f64,
    /// The previous word's pull is scaled by this when the keys are a known word of another
    /// language and not of the previous one (내가 after English, `for` after Korean): that's
    /// someone switching language, not a stray reading.
    pub switch_bias_factor: f64,
    /// Score floors for a Korean word seen in real text (two or more syllables, one), the
    /// counterpart of the English dictionary floor: 내가, 무슨 stay Korean after English.
    /// A word only in the wider vocabulary (닥터, 했죠: too rare for Tatoeba's sentences), or a
    /// noun the lists have only with a particle (혜택), gets the long floor too, unless the keys
    /// are an English word: that list comes from subtitles, and work (재가) or ahem (모드) must
    /// stay English.
    pub korean_known_floor: (f64, f64),
    /// How far Japanese must lead before a known English word is turned into kana
    /// outside Japanese text.
    pub english_word_protection: f64,
    /// Added to the language the user corrected the keys to once (`DecisionContext::nudge`):
    /// as much as the previous word's pull, so a close call turns and a clear one doesn't.
    pub correction_bias: f64,
}

impl HeuristicEngine {
    pub fn new(english: Arc<EnglishModel>, korean: KoreanModel, japanese: Option<Arc<JapaneseModel>>) -> Self {
        let segmenter = japanese.as_ref().map(|j| Arc::new(JapaneseSegmenter::new(english.clone(), j.clone())));
        HeuristicEngine {
            english,
            korean,
            japanese,
            segmenter,
            context_bias: 0.8,
            japanese_context_bias: 0.8,
            japanese_offset: 0.5,
            korean_offset: 0.0,
            rare_word_floor_penalty: 0.3,
            switch_bias_factor: 0.25,
            korean_known_floor: (-1.4, -2.0),
            english_word_protection: 1.0,
            correction_bias: 0.8,
        }
    }

    /// With the shared English and Korean models, and Japanese when `japanese` and its model is there.
    pub fn standard(japanese: bool) -> Self {
        Self::new(EnglishModel::shared(), KoreanModel::new(), if japanese { JapaneseModel::shared() } else { None })
    }
}

fn short(language: Language) -> &'static str {
    &language.raw()[..2]
}

impl DecisionEngine for HeuristicEngine {
    fn decide(&self, keys: &str, context: &DecisionContext) -> Decision {
        // Shift on a key that has no shifted jamo (e.g. "Hello", "API") is only ever English,
        // unless it lingers from a Korean shift key onto the next key (djEJs 어떤, dPFmf 예를):
        // then the scores decide.
        if DubeolsikLayout::has_english_only_capital(keys) {
            return Decision::new(Language::English, 0.99, keys, "uppercase outside QWERTOP");
        }
        let count = text::len(keys);

        let letters: String = keys.chars().filter(|c| c.is_alphabetic()).collect();
        let mut e = self.english.average_log_probability(&letters);
        // Also while typing: the preview must show what Space/Return will commit.
        let known = self.english.is_known_word(keys);
        if known {
            // Short dictionary words collide with real Korean (go=해, so=내, do=애),
            // so they get a weaker boost and leave room for context to decide.
            let mut floor: f64 = if count >= 4 {
                -1.4
            } else if count == 3 {
                -2.0
            } else {
                -2.6
            };
            // to, so, go, do: everyday English just above a one-syllable Korean word (새, 내,
            // 해, 애), so they stay English unless the previous word was Korean.
            if count == 2 && self.english.is_common_word(keys) {
                floor = self.korean_known_floor.1 + 0.1;
            }
            if self.english.level(keys).unwrap_or(0) > english_model::EXTRA_LEVEL {
                floor -= self.rare_word_floor_penalty;
            }
            e = e.max(floor);
        }
        if keys.chars().any(|c| !c.is_alphabetic()) {
            e -= 3.0; // "ra-men", "kan'i" aren't English
        }

        let mut k: Option<KoreanScore> = self.korean.score(keys, context.partial);
        let known_korean = k.as_ref().is_some_and(|score| {
            let text = score.composition.text();
            !score.composition.has_stray_jamo()
                && (self.korean.is_known_word(&text)
                    || (!known
                        && (self.korean.is_vocabulary_word(&text)
                            || self.korean.is_rare_vocabulary_word(&text)
                            || self.korean.is_known_stem(&text))))
        });
        if known_korean {
            if let Some(score) = k.as_mut() {
                let floor = if score.composition.syllable_count() >= 2 {
                    self.korean_known_floor.0
                } else {
                    self.korean_known_floor.1
                };
                score.average_log_probability = score.average_log_probability.max(floor);
            }
        }
        // A dictionary English word may already be complete, and so are chat jamo (ㅜㅜ): score
        // the Japanese reading as finished too, so the preview shows what Space will commit
        // ("to" stays to, not と; nn shows ㅜㅜ, not ん that Space turned into ㅜㅜ). So is a
        // doubled consonant with nothing after its っ (ff, ww): no Japanese word is only っ, so
        // it isn't one on its way (ff showed っf and went in as ff); って (tte) still is.
        let chat = k.as_ref().is_some_and(|k| k.composition.is_chat());
        let sokuon_first = romaji::compose(keys, true).is_only_sokuon();
        // A capital is never romaji: Japanese IMEs type ⇧ letters as themselves (⇧O is O, not お).
        let j = if keys.chars().any(|c| c.is_uppercase()) {
            None
        } else {
            self.japanese
                .as_ref()
                .and_then(|m| m.score(keys, context.partial && !known && !chat && !sokuon_first))
        };
        // English words inside a Japanese phrase (googlede…) score as segmented Japanese,
        // not as kana with stray letters.
        let mut segmented: Option<(Vec<Segment>, f64)> = None;
        if let (Some(j), Some(segmenter)) = (&j, &self.segmenter) {
            if j.composition.stray_letters > 0 {
                segmented = segmenter.scored_segments(keys);
            }
        }
        let japanese_score = j.as_ref().map(|j| {
            let seg = segmented.as_ref().map(|s| s.1 / count as f64).unwrap_or(f64::NEG_INFINITY);
            j.average_log_probability.max(seg)
        });

        let mut scores: Vec<(Language, f64)> = vec![(Language::English, e)];
        if let Some(k) = &k {
            scores.push((Language::Korean, k.average_log_probability + self.korean_offset));
        }
        if let Some(js) = japanese_score {
            scores.push((Language::Japanese, js + self.japanese_offset));
        }
        if let Some(previous) = context.previous_language.map(Language::sentence_language) {
            if let Some(i) = scores.iter().position(|s| s.0 == previous) {
                let mut bias = if previous == Language::Japanese { self.japanese_context_bias } else { self.context_bias };
                let known_in = |l: Language| match l {
                    Language::English => known,
                    Language::Korean => known_korean,
                    _ => false,
                };
                let others_known = [Language::English, Language::Korean, Language::Japanese]
                    .into_iter()
                    .any(|l| l != previous && known_in(l));
                if !known_in(previous) && matches!(previous, Language::English | Language::Korean | Language::Japanese) && others_known {
                    bias *= self.switch_bias_factor;
                }
                scores[i].1 += bias;
            }
        }
        if let Some(nudge) = context.nudge.filter(|&n| n != Language::Mixed) {
            if let Some(i) = scores.iter().position(|s| s.0 == nudge) {
                scores[i].1 += self.correction_bias;
            }
        }
        scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        // A dictionary English word only becomes kana on a clear win, unless we're already
        // in Japanese text or the user corrected it to Japanese once (where "same", "sake", "kite" are Japanese words).
        if known
            && scores[0].0 == Language::Japanese
            && context.previous_language.map(Language::sentence_language) != Some(Language::Japanese)
            && context.nudge != Some(Language::Japanese)
        {
            if let Some(english_index) = scores.iter().position(|s| s.0 == Language::English) {
                if scores[0].1 - scores[english_index].1 < self.english_word_protection {
                    scores.swap(0, english_index);
                }
            }
        }

        let (language, best) = scores[0];
        let runner_up = scores.get(1).copied();
        let margin = best - runner_up.map(|r| r.1).unwrap_or(best - 5.0);
        let confidence = 1.0 / (1.0 + (-margin * 2.0).exp());
        let output = match language {
            Language::Korean => k.as_ref().map(|k| k.composition.text()).unwrap_or_else(|| keys.to_string()),
            Language::Japanese => match (&segmented, &j) {
                (Some(s), _) => JapaneseSegmenter::text(&s.0),
                (None, Some(j)) => j.composition.text.clone(),
                _ => keys.to_string(),
            },
            _ => keys.to_string(),
        };
        let detail: Vec<String> = scores.iter().map(|(l, s)| format!("{}={:.2}", short(*l), s)).collect();
        let explanation =
            format!("{}{} margin={:+.2}", detail.join(" "), if known { " (dict)" } else { "" }, margin);
        Decision { language, confidence, output, explanation, runner_up: runner_up.map(|r| r.0) }
    }
}
