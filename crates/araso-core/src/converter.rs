//! Kana → kanji conversion, as the session sees it. Windows has no converter yet; tests use fakes.

/// One conversion candidate for (a prefix of) the hiragana being converted.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ConversionCandidate {
    pub text: String,
    /// How many kana from the start this candidate covers. Less than the whole
    /// reading means choosing it converts the first clause and keeps converting the rest.
    pub kana_count: usize,
    /// A completion of the reading (ゆーちゅ → YouTube) rather than a conversion of it: offered
    /// in the candidate list, never shown by live conversion.
    pub is_prediction: bool,
}

impl ConversionCandidate {
    pub fn new(text: impl Into<String>, kana_count: usize) -> Self {
        ConversionCandidate { text: text.into(), kana_count, is_prediction: false }
    }
}

pub trait JapaneseConverter {
    fn candidates(&self, kana: &str) -> Vec<ConversionCandidate>;
    /// The user committed `candidate`: remember it so it ranks higher next time.
    fn learn(&self, candidate: &ConversionCandidate, kana: &str);
    /// Persist anything learned (called on deactivation).
    fn save(&self);
    /// Candidates for all of `kana` that `candidates` leaves out, in the engine's order: the
    /// candidate window lists them after it.
    fn more_candidates(&self, _kana: &str) -> Vec<ConversionCandidate> {
        Vec::new()
    }
}
