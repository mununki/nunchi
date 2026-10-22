//! Finds data files: next to the installed input method (`<install dir>\data`, set by the host
//! with `set_directory`), under `ARASO_RESOURCES`, or in the source tree's `data` during
//! development and tests.

use std::path::{Path, PathBuf};
use std::sync::RwLock;

static DIRECTORY: RwLock<Option<PathBuf>> = RwLock::new(None);

/// Where the host keeps the data files (the folder of the TSF DLL, for one).
pub fn set_directory(dir: impl Into<PathBuf>) {
    *DIRECTORY.write().unwrap_or_else(|e| e.into_inner()) = Some(dir.into());
}

fn candidates(name: &str) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(dir) = DIRECTORY.read().unwrap_or_else(|e| e.into_inner()).as_ref() {
        out.push(dir.join(name));
    }
    if let Ok(dir) = std::env::var("ARASO_RESOURCES") {
        if !dir.is_empty() {
            out.push(Path::new(&dir).join(name));
        }
    }
    out.push(Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..").join("data").join(name));
    out
}

/// The first place `name` exists.
pub fn path(name: &str) -> Option<PathBuf> {
    candidates(name).into_iter().find(|p| p.exists())
}

pub fn read(name: &str) -> Option<String> {
    std::fs::read_to_string(path(name)?).ok()
}

/// Kana trigram counts (upstream tools/build_kana_model.py).
pub const KANA_MODEL: &str = "kana_trigrams.txt";
/// Character trigrams of written Japanese, for reranking conversions (build_text_model.py).
pub const TEXT_MODEL: &str = "text_trigrams.txt";
/// English words with their SCOWL level, 10 (most common) to 60 (build_english_words.py).
pub const ENGLISH_WORDS: &str = "english_words.txt";
/// Korean words seen in sentences, with counts (Tatoeba; build_korean_words.py).
pub const KOREAN_WORDS: &str = "korean_words.txt";
/// Korean words of two or more syllables, loanwords included (FrequencyWords;
/// build_korean_vocabulary.py): splitting `hello월드`, and the heuristic's Korean word floor.
pub const KOREAN_VOCABULARY: &str = "korean_vocabulary.txt";
/// Korean syllable counts from sentences (Tatoeba).
pub const KOREAN_SYLLABLES: &str = "korean_syllables.txt";
/// A long English spelling list the letter trigrams are trained on when it's there. On macOS it is
/// `/usr/share/dict/words` (Webster's Second, public domain); on Windows it has to be shipped.
pub const SPELLING_LIST: &str = "spelling_words.txt";
