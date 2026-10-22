//! The tables build.rs made from `data/`, as part of the binary (`tables::Table` reads them).

macro_rules! table {
    ($name:literal) => {
        include_bytes!(concat!(env!("OUT_DIR"), "/", $name, ".bin"))
    };
}

pub static ENGLISH_WORDS: &[u8] = table!("english_words");
pub static ENGLISH_TRIGRAMS: &[u8] = table!("english_trigrams");

pub static KOREAN_SYLLABLES: &[u8] = table!("korean_syllables");
pub static KOREAN_CORPUS_WORDS: &[u8] = table!("korean_corpus_words");
pub static KOREAN_VOCABULARY: &[u8] = table!("korean_vocabulary");
pub static KOREAN_RARE_VOCABULARY: &[u8] = table!("korean_rare_vocabulary");
pub static KOREAN_STEMS: &[u8] = table!("korean_stems");
pub static KOREAN_LIST_SYLLABLES: &[u8] = table!("korean_list_syllables");

pub static KANA_TRIGRAMS: &[u8] = table!("kana_trigrams");
pub static KANA_TRIGRAM_CONTEXTS: &[u8] = table!("kana_trigram_contexts");
pub static KANA_BIGRAMS: &[u8] = table!("kana_bigrams");
pub static KANA_BIGRAM_CONTEXTS: &[u8] = table!("kana_bigram_contexts");
pub static KANA_UNIGRAMS: &[u8] = table!("kana_unigrams");
pub static KANA_TOTALS: &[u8] = table!("kana_totals");
