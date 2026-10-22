//! An English word with a Korean particle or ending typed straight after it, and the engine
//! that decides such splits.

use std::collections::HashSet;
use std::sync::{Arc, OnceLock};

use crate::decision::{Decision, DecisionContext, DecisionEngine, Language};
use crate::english_model::EnglishModel;
use crate::hangul;
use crate::jamo::DubeolsikLayout;
use crate::japanese_segmenter::JapaneseSegmenter;
use crate::korean_model::KoreanModel;
use crate::particle_splitter::{ParticleSplitter, Split};
use crate::text;

/// `githubdp` → `github에`, `APIfmf` → `API를`, `commitgotj` → `commit해서`.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct MixedWord {
    pub english: String,
    pub korean_keys: String,
    pub korean: String,
}

/// Particles and endings that commonly attach directly to a (loan)word.
const SUFFIXES: &str = "
에 에서 에게 에는 에서는 에도 에서도 의 을 를 은 는 이 가 도 로 으로 와 과 랑 이랑 하고 한테 까지 부터 만 보다
처럼 이나 나 이야 야 이에요 예요 이다 입니다 인데 이라 이라고 라고 으로는 로는 으로도 로도 이고 이지 이랑은
했어 했다 했어요 했는데 했고 했지 했음 해서 해야 하는 하면 하고 하기 하게 하다 해 해요 합니다 할 한 함 하자 해줘 해봐
됐어 됐다 된 되는 되면 돼 돼요 됩니다 같은 같아 적 적인 적으로 스럽게 같이 들 들이 들을 들은 에요 이요 를요 을요
같은건 같은거 같은게 같은걸 같은데 같다 같네 같아서 같은지 같고 같아요 인가 인지 인데요 인건 인게 인거
이라는 라는 이란 에선 으론 로서 으로서
";

/// Particles that follow a single letter (x에, n이, a를, X축의 X): case particles only, not
/// endings like 해/한, so short English (ago) doesn't split.
const SINGLE_LETTER_PARTICLES: &str = "
에 에서 에게 에는 에서는 에도 의 을 를 은 는 이 가 도 로 으로 와 과 랑 이랑 하고 한테 까지 부터 만 보다
처럼 이나 나 이라 이라고 라고 이랑은 으로는 로는 같은 같이
";

/// Particles with a form after a final consonant and one after a vowel (책을, 사과를): the
/// consonant form first. A final ㄹ takes the vowel form of 으로 (서울로).
const PAIRED_PARTICLES: [(&str, &str); 18] = [
    ("을", "를"),
    ("은", "는"),
    ("이", "가"),
    ("과", "와"),
    ("이랑", "랑"),
    ("이랑은", "랑은"),
    ("으로", "로"),
    ("으로는", "로는"),
    ("으로도", "로도"),
    ("으론", "론"),
    ("으로서", "로서"),
    ("이나", "나"),
    ("이야", "야"),
    ("이라", "라"),
    ("이라고", "라고"),
    ("이라는", "라는"),
    ("이란", "란"),
    ("이에요", "예요"),
];

pub fn is_suffix(korean: &str) -> bool {
    static SET: OnceLock<HashSet<&'static str>> = OnceLock::new();
    SET.get_or_init(|| SUFFIXES.split_whitespace().collect()).contains(korean)
}

fn is_single_letter_particle(korean: &str) -> bool {
    static SET: OnceLock<HashSet<&'static str>> = OnceLock::new();
    SET.get_or_init(|| SINGLE_LETTER_PARTICLES.split_whitespace().collect()).contains(korean)
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FinalSound {
    Vowel,
    Consonant,
    Rieul,
}

impl MixedWord {
    pub fn text(&self) -> String {
        format!("{}{}", self.english, self.korean)
    }

    /// Whether `particle` is the form that follows a word ending as `final_sound` describes:
    /// nil for a particle that has one form (에, 도, 의), else true or false (씨이, 쓰은 are wrong).
    pub fn particle_agrees(particle: &str, final_sound: FinalSound) -> Option<bool> {
        for (consonant, vowel) in PAIRED_PARTICLES {
            if particle == consonant {
                return Some(
                    final_sound == FinalSound::Consonant || (final_sound == FinalSound::Rieul && !consonant.starts_with('으')),
                );
            }
            if particle == vowel {
                return Some(
                    final_sound == FinalSound::Vowel || (final_sound == FinalSound::Rieul && consonant.starts_with('으')),
                );
            }
        }
        None
    }

    /// How a Hangul syllable ends; nil for anything else.
    pub fn final_sound(syllable: char) -> Option<FinalSound> {
        if !text::is_hangul_syllable(syllable) {
            return None;
        }
        Some(match (syllable as u32 - 0xAC00) % 28 {
            0 => FinalSound::Vowel,
            8 => FinalSound::Rieul,
            _ => FinalSound::Consonant,
        })
    }

    /// How an acronym ends when read letter by letter: L, M, N and R (엘, 엠, 엔, 알) on a final
    /// consonant, every other letter's name on a vowel (TM은, TV는).
    pub fn final_sound_of_acronym(acronym: &str) -> FinalSound {
        match text::last(acronym).map(|c| c.to_ascii_uppercase()) {
            Some('L' | 'R') => FinalSound::Rieul,
            Some('M' | 'N') => FinalSound::Consonant,
            _ => FinalSound::Vowel,
        }
    }

    /// A single letter and a particle: `xdp` → `x에`, `Xfmf` → `X를`. One key on its own can't
    /// make a Hangul syllable before another consonant, so the keys never read as Korean.
    pub fn split_single_letter(keys: &str) -> Option<MixedWord> {
        let first = text::first(keys)?;
        if text::len(keys) < 2 || !first.is_ascii_alphabetic() {
            return None;
        }
        let suffix_keys = text::drop_first(keys, 1);
        if !text::first(suffix_keys).is_some_and(DubeolsikLayout::starts_syllable) {
            return None;
        }
        let composed = hangul::compose(suffix_keys)?;
        let korean = composed.text();
        if composed.has_stray_jamo() || !is_single_letter_particle(&korean) {
            return None;
        }
        // 도 and 나 are the keys eh and sk, common English endings (heh, meh, yeh, tsk): after a
        // lowercase letter they stay English. A capital (X도) was typed on purpose.
        if first.is_lowercase() && (korean == "도" || korean == "나") {
            return None;
        }
        Some(MixedWord { english: first.to_string(), korean_keys: suffix_keys.to_string(), korean })
    }

    /// Splits off the longest Korean particle/ending at the end of `keys`.
    /// Only the shape is checked here; whether the front really is English is
    /// up to the caller (see `SplittingEngine`).
    pub fn split(keys: &str) -> Option<MixedWord> {
        let chars: Vec<char> = keys.chars().collect();
        // Front needs at least 2 letters; try the longest suffix first.
        if chars.len() < 3 {
            return None;
        }
        for start in 2..chars.len() {
            if !DubeolsikLayout::starts_syllable(chars[start]) {
                continue;
            }
            let suffix_keys: String = chars[start..].iter().collect();
            let Some(composed) = hangul::compose(&suffix_keys) else { continue };
            let korean = composed.text();
            if composed.has_stray_jamo() || !is_suffix(&korean) {
                continue;
            }
            return Some(MixedWord { english: chars[..start].iter().collect(), korean_keys: suffix_keys, korean });
        }
        None
    }
}

/// Decides `mixed` for English and Korean typed as one token: English + particle (github에),
/// a single letter + particle (x에), English + Korean word (hello월드), and with Japanese on
/// English + Japanese particle (githubで). Never when the whole token is a known English word or
/// reads as ordinary Korean (every syllable is one people actually use).
pub struct SplittingEngine {
    pub base: Arc<dyn DecisionEngine>,
    pub english: Arc<EnglishModel>,
    pub korean: KoreanModel,
    /// With Japanese on: English word + Japanese particle (githubで), decided the same way.
    pub japanese: Option<Arc<JapaneseSegmenter>>,
    /// Scores English word + particle, and English + Korean word, against the whole token.
    pub particles: ParticleSplitter,
}

/// How sure a split is: above Strict (`AppPolicy::STRICT_THRESHOLD`), since each rule already
/// asks for an English front it can stand on, so github에 splits in a terminal too.
pub const SPLIT_CONFIDENCE: f64 = 0.98;

impl SplittingEngine {
    pub fn new(
        base: Arc<dyn DecisionEngine>,
        english: Arc<EnglishModel>,
        korean: KoreanModel,
        japanese: Option<Arc<JapaneseSegmenter>>,
    ) -> Self {
        let particles =
            ParticleSplitter::new(english.clone(), korean.clone(), japanese.as_ref().map(|s| s.japanese.clone()));
        SplittingEngine { base, english, korean, japanese, particles }
    }
}

impl DecisionEngine for SplittingEngine {
    fn decide(&self, keys: &str, context: &DecisionContext) -> Decision {
        // English word + particle, Korean (github에) or with Japanese on Japanese (githubで),
        // when that reading beats the whole token in one language. Before English + Korean
        // word: beautywo is beauty + を, not beau + 쇼재.
        match self.particles.split(keys, self.japanese.is_some()) {
            Some(Split::Korean(mixed)) => {
                let explanation = format!("split {}+{}", mixed.english, mixed.korean);
                return Decision::new(Language::Mixed, SPLIT_CONFIDENCE, mixed.text(), explanation);
            }
            Some(Split::Japanese { english, particle }) => {
                return Decision::new(
                    Language::Japanese,
                    SPLIT_CONFIDENCE,
                    format!("{english}{particle}"),
                    format!("split {english}+{particle}"),
                );
            }
            None => {}
        }
        if let Some(mixed) = MixedWord::split_single_letter(keys) {
            if !self.english.is_known_word(keys) && !self.particles.reads_as_korean(keys) {
                let explanation = format!("split {}+{}", mixed.english, mixed.korean);
                return Decision::new(Language::Mixed, SPLIT_CONFIDENCE, mixed.text(), explanation);
            }
        }
        self.base.decide(keys, context)
    }
}
