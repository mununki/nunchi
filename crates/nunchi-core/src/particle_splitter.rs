//! An English word with a particle typed straight after it (`githubdp` → `github에`,
//! `githubde` → `githubで`) is one more reading of the keys, scored like the whole-token
//! readings it competes with: average log-probability per keystroke.
//!
//! - The front scores as English: a known word gets the dictionary floors `HeuristicEngine`
//!   uses, capitals typed on purpose (API, GitHub) a higher one, anything else its spelling.
//! - The particle scores by how common it is, and switching language costs a little.
//! - The whole token scores as Korean (a Korean word, or one with a particle, gets a floor),
//!   as unknown English, and with Japanese on as kana.
//!
//! The split wins only by `margin`. `form에서` splits because 래그 is no Korean word and form
//! is everyday English; `핸들을` doesn't, because 핸들 is a word.

use std::sync::Arc;

use crate::english_model::{self, EnglishModel};
use crate::hangul;
use crate::jamo::DubeolsikLayout;
use crate::japanese_model::JapaneseModel;
use crate::japanese_segmenter;
use crate::korean_model::{self, KoreanModel};
use crate::mixed_word::{self, MixedWord};
use crate::romaji;
use crate::text;

/// Log-probabilities for a whole word or particle, not per key, so a longer front or
/// particle isn't favored for its length.
#[derive(Clone, Debug)]
pub struct Weights {
    /// A known English word: `known_base` minus `per_level` per SCOWL level (10 = everyday).
    pub known_base: f64,
    pub per_level: f64,
    /// An acronym (API, UI, PR) or a name with a capital inside (GitHub, VSCode, macOS).
    pub acronym: f64,
    pub name: f64,
    /// Taken off once from the spelling score of a front (or token) that isn't a known word.
    pub unknown_word: f64,
    /// Taken off once more when the front isn't a known word: an unknown word with a
    /// particle after it is rarer than an unknown word (names: adriano, porto, chandi).
    pub unknown_front: f64,
    /// A common particle (에, 를, で…) and the other endings (처럼, 했어, まで…).
    pub core_particle: f64,
    pub other_particle: f64,
    /// Once per split, for switching language inside a token.
    pub switch_cost: f64,
    /// Instead of `switch_cost` for an ordinary English word and a Korean word (hello월드): rarer
    /// than a particle or an acronym or name with a word after it (github에, PC카톡).
    pub word_switch_cost: f64,
    /// Added to a particle of a pair (은/는, 을/를) after an acronym when it is the form the
    /// acronym's last letter takes (TM은, TV는): after another word either form may follow,
    /// after an acronym only that one does, so twice as likely (ln 2).
    pub agreement: f64,
    /// Per key, the least a Korean word (or a word with a particle) scores.
    pub korean_word_floor: f64,
    /// Added per key to the kana reading's score, as `HeuristicEngine::japanese_offset`.
    pub japanese_offset: f64,
    /// How far (per key) the split has to lead the best whole-token reading.
    pub margin: f64,
}

impl Default for Weights {
    fn default() -> Self {
        Weights {
            known_base: -4.0,
            per_level: 0.15,
            acronym: -6.0,
            name: -7.0,
            unknown_word: 3.0,
            unknown_front: 3.0,
            core_particle: -2.5,
            other_particle: -4.5,
            switch_cost: 1.0,
            word_switch_cost: 9.0,
            agreement: 0.7,
            korean_word_floor: -1.4,
            japanese_offset: 1.4,
            margin: 0.3,
        }
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Split {
    Korean(MixedWord),
    Japanese { english: String, particle: String },
}

impl Split {
    pub fn text(&self) -> String {
        match self {
            Split::Korean(mixed) => mixed.text(),
            Split::Japanese { english, particle } => format!("{english}{particle}"),
        }
    }
}

/// Japanese particles that score `core_particle` (から, まで, より score `other_particle`).
const CORE_JAPANESE: [&str; 8] = ["で", "に", "を", "は", "が", "の", "と", "も"];
/// Particles that follow a noun most of the time: they score `core_particle`.
const CORE_KOREAN: [&str; 23] = [
    "에", "에서", "에게", "을", "를", "은", "는", "이", "가", "도", "로", "으로", "와", "과", "의", "만", "까지", "부터",
    "랑", "이랑", "한테", "처럼", "보다",
];

fn is_core_korean(particle: &str) -> bool {
    CORE_KOREAN.contains(&particle)
}

pub struct ParticleSplitter {
    pub english: Arc<EnglishModel>,
    pub korean: KoreanModel,
    pub japanese: Option<Arc<JapaneseModel>>,
    pub weights: Weights,
}

impl ParticleSplitter {
    pub fn new(english: Arc<EnglishModel>, korean: KoreanModel, japanese: Option<Arc<JapaneseModel>>) -> Self {
        ParticleSplitter { english, korean, japanese, weights: Weights::default() }
    }

    /// The best split of `keys`, if it beats reading the whole token as one language.
    /// Japanese particles only with `japanese_on`.
    pub fn split(&self, keys: &str, japanese_on: bool) -> Option<Split> {
        self.scored(keys, japanese_on).and_then(|(split, lead, _)| (lead >= self.weights.margin).then_some(split))
    }

    /// The best split and how far it leads the best whole-token reading (for tuning).
    pub fn scored(&self, keys: &str, japanese_on: bool) -> Option<(Split, f64, String)> {
        let chars: Vec<char> = keys.chars().collect();
        let n = chars.len();
        if n < 3 || self.english.is_known_word(keys) {
            return None;
        }
        let w8 = &self.weights;
        let mut best: Option<(Split, f64)> = None;
        let offer = |best: &mut Option<(Split, f64)>, split: Split, front: &str, particle: f64, korean: Option<&str>| {
            let Some(w) = self.front_score(front) else { return };
            let mut particle = particle;
            if let Some(korean) = korean {
                if w == w8.acronym
                    && MixedWord::particle_agrees(korean, MixedWord::final_sound_of_acronym(front)) == Some(true)
                {
                    particle += w8.agreement;
                }
            }
            let score = (w + particle - w8.switch_cost) / n as f64;
            if best.as_ref().map_or(true, |b| score > b.1) {
                *best = Some((split, score));
            }
        };
        for start in 2..n {
            if !DubeolsikLayout::starts_syllable(chars[start]) {
                continue;
            }
            let tail: String = chars[start..].iter().collect();
            let Some(composed) = hangul::compose(&tail) else { continue };
            let korean = composed.text();
            if composed.has_stray_jamo() || !mixed_word::is_suffix(&korean) {
                continue;
            }
            let front: String = chars[..start].iter().collect();
            let particle = if is_core_korean(&korean) { w8.core_particle } else { w8.other_particle };
            let mixed = MixedWord { english: front.clone(), korean_keys: tail, korean: korean.clone() };
            offer(&mut best, Split::Korean(mixed), &front, particle, Some(&korean));
        }
        if japanese_on && self.japanese.is_some() {
            for start in 2.max(n.saturating_sub(4))..n {
                let tail = romaji::compose(&chars[start..].iter().collect::<String>(), false);
                if !tail.is_clean() || !japanese_segmenter::is_trailing_particle(&tail.text) {
                    continue;
                }
                let front: String = chars[..start].iter().collect();
                let particle = if CORE_JAPANESE.contains(&tail.text.as_str()) { w8.core_particle } else { w8.other_particle };
                let split = Split::Japanese { english: front.clone(), particle: tail.text.clone() };
                offer(&mut best, split, &front, particle, None);
            }
        }
        let (w, word) = self.english_then_korean_word(&chars);
        if best.is_none() && word.is_none() {
            return None;
        }
        let k = self.korean_score(keys);
        let e = self.english.average_log_probability(keys) - w8.unknown_word / n as f64;
        let j = if japanese_on { self.japanese_score(keys) } else { f64::NEG_INFINITY };
        let whole = k.max(e).max(j);
        // English + Korean word is a split too, and the particle split has to beat it, but not by
        // `margin`, which is for splitting at all (funeral로 over funer + 미로).
        let detail = format!(
            "split={:.2} ko={:.2} en={:.2} ja={:.2} en+ko={:.2}",
            best.as_ref().map_or(f64::NEG_INFINITY, |b| b.1),
            k,
            e,
            j,
            w
        );
        if let Some((split, score)) = best {
            if score >= w {
                let lead = (score - whole).min(score - w + w8.margin);
                return Some((split, lead, detail));
            }
        }
        let (mixed, score) = word?;
        Some((Split::Korean(mixed), score - whole, detail))
    }

    /// The keys as an English front and a Korean word of two syllables or more, per key
    /// (PC카톡에서, not PCzkxhr에서): the front as `front_score`, the word as the whole token is
    /// scored as Korean. A word that is only a particle is the particle split, and one with a
    /// syllable nobody writes is no word (mac + 뇃에서 doesn't stop macshot에서).
    ///
    /// `best` is the best cut, the reading a particle split has to beat. `split` is the best cut
    /// that may stand as a split itself (`may_stand_as_word`): an acronym or name and a word (PC카톡,
    /// TV광고) switch language at `switch_cost`, as a particle does; an ordinary word and a Korean
    /// word (hello월드) are rarer and pay `word_switch_cost`, so a name that spells like English
    /// stays whole (cartman isn't car스무).
    pub fn english_then_korean_word(&self, chars: &[char]) -> (f64, Option<(MixedWord, f64)>) {
        let keys: String = chars.iter().collect();
        let english_only_capital = DubeolsikLayout::has_english_only_capital(&keys);
        // Clean romaji (hasansowo, はさんそを) is Japanese, and keys that read as Korean throughout
        // are Korean, unless a capital no lingering Shift explains says otherwise (ThkT is 쐈).
        let may_stand = !romaji::compose(&keys, false).is_clean() && (english_only_capital || !self.reads_as_korean(&keys));
        let mut best = f64::NEG_INFINITY;
        let mut split: Option<(MixedWord, f64)> = None;
        if chars.len() < 3 {
            return (best, split);
        }
        for start in 2..(chars.len() - 1) {
            if !DubeolsikLayout::starts_syllable(chars[start]) {
                continue;
            }
            let front: String = chars[..start].iter().collect();
            let back: String = chars[start..].iter().collect();
            let Some(composed) = hangul::compose(&back) else { continue };
            let korean = composed.text();
            if composed.has_stray_jamo() || composed.syllable_count() < 2 || mixed_word::is_suffix(&korean) {
                continue;
            }
            let known_korean = self.is_korean_word_or_word_with_particle(&korean);
            if !(known_korean || self.reads_as_korean(&back)) {
                continue;
            }
            let Some((w, capitals)) = self.front_reading(&front) else { continue };
            let k = self.korean_score(&back) * text::len(&back) as f64;
            best = best.max((w + k - self.weights.switch_cost) / chars.len() as f64);
            if !may_stand || (capitals && !self.may_stand_as_word(&front, english_only_capital, known_korean)) {
                continue;
            }
            let cost = if capitals { self.weights.switch_cost } else { self.weights.word_switch_cost };
            let score = (w + k - cost) / chars.len() as f64;
            if split.as_ref().map_or(true, |s| score > s.1) {
                split = Some((MixedWord { english: front, korean_keys: back, korean }, score));
            }
        }
        (best, split)
    }

    /// Whether an acronym or name may be the front of a split, not only compete with one: when
    /// a capital no lingering Shift explains is in the keys, or a known Korean word follows and
    /// its own keys spell a syllable no list or corpus has (EU as ㄸ+ㅕ: 뗘). Korean typed fast
    /// keeps its rare syllables (삐졌어 as QLwuTdj: 삐 is rare, but seen) and 쓰정책 (TMwjdcor)
    /// stays Korean. Only the front is judged: the word after may well hold a rare syllable
    /// (또공룡이 as EHrhdfyddl).
    fn may_stand_as_word(&self, front: &str, english_only_capital: bool, known_korean: bool) -> bool {
        english_only_capital || !Self::is_acronym(front) || (known_korean && !self.spells_seen_korean(front))
    }

    /// Whole Hangul of syllables seen in the corpus or a word list, however rarely (삐, not 뗘).
    fn spells_seen_korean(&self, keys: &str) -> bool {
        let Some(composed) = hangul::compose(keys) else { return false };
        !composed.has_stray_jamo() && composed.syllables().into_iter().all(|s| self.korean.knows_syllable(s))
    }

    /// Whole Hangul made of syllables people use.
    pub fn reads_as_korean(&self, keys: &str) -> bool {
        let Some(composed) = hangul::compose(keys) else { return false };
        !composed.has_stray_jamo()
            && composed
                .syllables()
                .into_iter()
                .all(|s| self.korean.syllable_log_probability(s) > korean_model::UNKNOWN_SYLLABLE_LOG_P)
    }

    /// The front as English (log-probability of the word); nil when it can't be one.
    pub fn front_score(&self, front: &str) -> Option<f64> {
        self.front_reading(front).map(|r| r.0)
    }

    /// `front_score`, and whether the front is an acronym or a name typed with its capitals.
    pub fn front_reading(&self, front: &str) -> Option<(f64, bool)> {
        let count = text::len(front);
        if count < 2 || !front.chars().all(|c| c.is_ascii_alphabetic()) {
            return None;
        }
        let w8 = &self.weights;
        // Capitals that are all shifted jamo in keys that spell clean Hangul are Korean typed
        // with shift (akffkRod: 말라깽, tkrlRns: 사기꾼), not an acronym or a name.
        let korean = front.chars().all(|c| !c.is_uppercase() || DubeolsikLayout::is_korean_shift_key(c))
            && hangul::compose(front).is_some_and(|c| !c.has_stray_jamo());
        if !korean && Self::is_acronym(front) {
            return Some((w8.acronym, true));
        }
        // A name scores lower the rarer its last part: macOS + 문서에서, not macOSans (Sans) + 서에서.
        if !korean {
            if let Some(level) = self.name_level(front) {
                return Some((w8.name - w8.per_level * (level - 10).max(0) as f64, true));
            }
        }
        // Lowercase (or a capital at the start: a name, or the start of a sentence) needs
        // 3 letters: so + が, me + で are Japanese.
        if count < 3 {
            return None;
        }
        let spelling = self.english.average_log_probability(front) * count as f64;
        if let Some(level) = self.english.level(front).filter(|&l| l <= english_model::KNOWN_LEVEL) {
            return Some((spelling.max(w8.known_base - w8.per_level * level as f64), false));
        }
        Some((spelling - w8.unknown_word - w8.unknown_front, false))
    }

    /// GitHub, VSCode, iPhone, macOS: a capital inside, and the part from the last capital a
    /// known word (Hub, Code) or capitals (OS). Not APIdm. The SCOWL level of that last part
    /// (Code: 10, Hub: 35), 10 for capitals; nil when `front` isn't a name.
    pub fn name_level(&self, front: &str) -> Option<i32> {
        let chars: Vec<char> = front.chars().collect();
        if !chars.iter().skip(1).any(|c| c.is_uppercase()) || !chars.iter().any(|c| c.is_lowercase()) {
            return None;
        }
        let last = chars.iter().rposition(|c| c.is_uppercase())?;
        if last == chars.len() - 1 {
            // Ends in capitals (macOS): the run of them is the last part.
            let mut start = last;
            while start > 0 && chars[start - 1].is_uppercase() {
                start -= 1;
            }
            return (chars.len() - start >= 2).then_some(10);
        }
        // The last part starts at the last capital (Code of VSCode, Idm of APIdm).
        let part: String = chars[last..].iter().collect();
        if chars.len() - last < 3 || !self.english.is_known_word(&part) {
            return None;
        }
        self.english.level(&part)
    }

    /// API, UI, PR: 2 to 6 capitals.
    pub fn is_acronym(front: &str) -> bool {
        (2..=6).contains(&text::len(front)) && front.chars().all(|c| c.is_uppercase())
    }

    /// The whole token as Korean, per key. A capital with no shifted jamo (the S of Slack)
    /// can't be Korean.
    pub fn korean_score(&self, keys: &str) -> f64 {
        if DubeolsikLayout::has_english_only_capital(keys) {
            return f64::NEG_INFINITY;
        }
        let Some(score) = self.korean.score(keys, false) else { return f64::NEG_INFINITY };
        let composition = &score.composition;
        if !composition.has_stray_jamo() && self.is_korean_word_or_word_with_particle(&composition.text()) {
            return score.average_log_probability.max(self.weights.korean_word_floor);
        }
        score.average_log_probability
    }

    /// The whole token as kana, per key: only what keeps it from splitting, as the decision
    /// engine reads a capital as English. Capitals after the first letter aren't romaji
    /// (UIde is UI + で); one at the start may begin a sentence (Kyouha stays whole).
    pub fn japanese_score(&self, keys: &str) -> f64 {
        if text::drop_first(keys, 1).chars().any(|c| c.is_uppercase()) {
            return f64::NEG_INFINITY;
        }
        match self.japanese.as_ref().and_then(|j| j.score(keys, false)) {
            Some(score) => score.average_log_probability + self.weights.japanese_offset,
            None => f64::NEG_INFINITY,
        }
    }

    /// 핸들, 소문은, 행하는: a word of the wider vocabulary, or one with a particle or ending in
    /// the form the word's last syllable takes (씨를, not 씨이 or 씨을).
    pub fn is_korean_word_or_word_with_particle(&self, hangul: &str) -> bool {
        if self.korean.is_vocabulary_word(hangul) {
            return true;
        }
        let chars: Vec<char> = hangul.chars().collect();
        chars.len() > 1
            && (1..chars.len()).any(|cut| {
                let particle: String = chars[cut..].iter().collect();
                let stem: String = chars[..cut].iter().collect();
                self.korean.is_vocabulary_word(&stem)
                    && mixed_word::is_suffix(&particle)
                    && MixedWord::final_sound(chars[cut - 1]).and_then(|f| MixedWord::particle_agrees(&particle, f))
                        != Some(false)
            })
    }
}
