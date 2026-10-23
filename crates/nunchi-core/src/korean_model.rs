//! Syllable-unigram model for Korean: enough to tell "안녕하세요" from the accidental
//! Hangul that English produces on a 2-set layout (e.g. "the" → "솓"). The syllables and word
//! lists are built into tables when the crate is built (`tables::korean`).

use crate::embedded;
use crate::hangul::{self, Composition, Unit};
use crate::jamo::{self, DubeolsikLayout};
use crate::sebeolsik::KoreanLayout;
use crate::tables::{self, Table};
use crate::text;

pub use crate::korean_syllables::{TIER1_LOG_P, TIER2_LOG_P, TIER3_LOG_P};

/// ㅋㅋ, ㅎㅎ, ㅠㅠ and friends.
pub const CHAT_JAMO: [char; 10] = ['ㅋ', 'ㅎ', 'ㅠ', 'ㅜ', 'ㄷ', 'ㅇ', 'ㄱ', 'ㅅ', 'ㅂ', 'ㄴ'];

#[derive(Clone, Debug)]
pub struct KoreanScore {
    /// Average log-probability per keystroke.
    pub average_log_probability: f64,
    pub composition: Composition,
}

/// The syllables' log-probabilities (the hand-made tiers and the corpus counts, whichever rates a
/// syllable higher) and the word lists, all in the crate's tables.
#[derive(Clone, Copy, Default)]
pub struct KoreanModel;

pub const UNKNOWN_SYLLABLE_LOG_P: f64 = -11.0;
pub const STRAY_JAMO_LOG_P: f64 = -13.0;
/// Typing the first consonant of the next syllable is normal mid-word.
pub const PENDING_CONSONANT_LOG_P: f64 = -3.5;
/// The same for a consonant that joined the syllable before as its final (갷 of 소개ㅎ),
/// a little less sure: alr is 미국 on its way and also already (tuned on preview traces of
/// the top 10,000 English and Korean words).
pub const PENDING_FINAL_PENALTY: f64 = 1.0;
pub const CHAT_JAMO_LOG_P: f64 = -4.0;

const SYLLABLES: Table<'static> = Table::new(embedded::KOREAN_SYLLABLES);
/// Words seen in Korean sentences (Data/korean_words.txt, Tatoeba): 나는, 무슨, 내가…
const CORPUS_WORDS: Table<'static> = Table::new(embedded::KOREAN_CORPUS_WORDS);
const VOCABULARY: Table<'static> = Table::new(embedded::KOREAN_VOCABULARY);
const RARE_VOCABULARY: Table<'static> = Table::new(embedded::KOREAN_RARE_VOCABULARY);
const STEMS: Table<'static> = Table::new(embedded::KOREAN_STEMS);
const LIST_SYLLABLES: Table<'static> = Table::new(embedded::KOREAN_LIST_SYLLABLES);

fn syllable_key(syllable: char, buffer: &mut [u8; 16]) -> &[u8] {
    tables::key_of(buffer, &[syllable])
}

impl KoreanModel {
    pub fn new() -> Self {
        KoreanModel
    }

    /// Nothing to load: the tables are part of the binary.
    pub fn preload() {}

    /// A word that occurs in real Korean text (the composed Hangul, not the keys).
    pub fn is_known_word(&self, hangul: &str) -> bool {
        CORPUS_WORDS.contains(hangul.as_bytes())
    }

    /// A word in the wider vocabulary (loanwords like 월드, 서버; 혜택), for English + Korean splits
    /// and the heuristic's Korean word floor.
    pub fn is_vocabulary_word(&self, hangul: &str) -> bool {
        CORPUS_WORDS.contains(hangul.as_bytes()) || VOCABULARY.contains(hangul.as_bytes())
    }

    /// A word the subtitles have fewer than ten times (촉매, twice): for the heuristic's Korean
    /// word floor only. The tail holds dialect and typos too (어뗘, 쓔벌, 꺼이), so it stays out
    /// of the splits and of `knows_syllable`, which `EU정책` and `RJ이` rely on.
    pub fn is_rare_vocabulary_word(&self, hangul: &str) -> bool {
        RARE_VOCABULARY.contains(hangul.as_bytes())
    }

    /// A noun the lists only have with a case particle (혜택 from 혜택을, 혜택으로): they
    /// count words with their particles, so a bare noun can be missing from both.
    pub fn is_known_stem(&self, hangul: &str) -> bool {
        STEMS.contains(hangul.as_bytes())
    }

    pub fn syllable_log_probability(&self, syllable: char) -> f64 {
        let mut buffer = [0u8; 16];
        SYLLABLES.get(syllable_key(syllable, &mut buffer)).map_or(UNKNOWN_SYLLABLE_LOG_P, tables::f64_of)
    }

    /// A syllable seen at all: in the tiers, the corpus or a word of the lists, however rarely
    /// (삐, 쪄, 뾰, 쭈 are; 뗘, 쎠, 쓔 aren't). Rarer than `UNKNOWN_SYLLABLE_LOG_P` still means
    /// someone typed it.
    pub fn knows_syllable(&self, syllable: char) -> bool {
        let mut buffer = [0u8; 16];
        let key = syllable_key(syllable, &mut buffer);
        SYLLABLES.contains(key) || LIST_SYLLABLES.contains(key)
    }

    /// `partial`: the token is still being typed, so a trailing lone consonant is expected
    /// rather than penalized.
    pub fn score(&self, keys: &str, partial: bool) -> Option<KoreanScore> {
        let composition = hangul::compose(keys)?;
        if keys.is_empty() {
            return None;
        }
        let mut total = self.log_probability(&composition, partial);
        // The consonant typed last may have joined the syllable before as its final (개 + ㅎ
        // shows 갷) while it is the next syllable's first (소개하는): score that reading too. Only
        // on 2-set: 3-set has its own keys for finals.
        if partial && KoreanLayout::current() == KoreanLayout::Dubeolsik && !composition.is_chat() {
            if let Some(last) = text::last(keys) {
                if DubeolsikLayout::jamo(last).map(jamo::is_consonant) == Some(true)
                    && matches!(composition.units.last(), Some(Unit::Syllable(_)))
                {
                    if let Some(before) = hangul::compose(text::drop_last(keys, 1)) {
                        if before.units.len() == composition.units.len()
                            && matches!(before.units.last(), Some(Unit::Syllable(_)))
                        {
                            total = total.max(
                                self.log_probability(&before, false) + PENDING_CONSONANT_LOG_P - PENDING_FINAL_PENALTY,
                            );
                        }
                    }
                }
            }
        }
        Some(KoreanScore { average_log_probability: total / text::len(keys) as f64, composition })
    }

    fn log_probability(&self, composition: &Composition, partial: bool) -> f64 {
        let units = &composition.units;
        let all_chat = composition.is_chat();
        let mut total = 0.0;
        for (i, unit) in units.iter().enumerate() {
            match *unit {
                Unit::Syllable(s) => total += self.syllable_log_probability(s),
                Unit::Jamo(j) => {
                    if all_chat {
                        total += CHAT_JAMO_LOG_P;
                    } else if partial && i == units.len() - 1 && jamo::is_consonant(j) {
                        total += PENDING_CONSONANT_LOG_P;
                    } else {
                        total += STRAY_JAMO_LOG_P;
                    }
                }
            }
        }
        total
    }
}
