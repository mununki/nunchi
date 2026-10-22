//! Builds the word lists and models of `data/` into compact tables (src/tables.rs) that the crate
//! embeds: the input method then reads them in place, shared by every app it runs in.

use std::path::{Path, PathBuf};

#[allow(dead_code)]
#[path = "src/english_lists.rs"]
mod english_lists;
#[allow(dead_code)]
#[path = "src/korean_syllables.rs"]
mod korean_syllables;
#[allow(dead_code)]
#[path = "src/tables.rs"]
mod tables;

/// Where `korean_vocabulary.txt` stops being the common vocabulary (upstream's old 50k list's last count).
const COMMON_COUNT: u32 = 10;

fn main() {
    for file in ["src/tables.rs", "src/english_lists.rs", "src/korean_syllables.rs"] {
        println!("cargo:rerun-if-changed={file}");
    }
    let data = Path::new(&std::env::var("CARGO_MANIFEST_DIR").unwrap()).join("..").join("..").join("data");
    let read = |name: &str| {
        let path = data.join(name);
        println!("cargo:rerun-if-changed={}", path.display());
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e} (the data files are in data/)", path.display()))
    };
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let write = |name: &str, bytes: &[u8]| std::fs::write(out.join(format!("{name}.bin")), bytes).unwrap();

    let extra: Vec<&str> = english_lists::COMMON_WORDS.split_whitespace().collect();
    let short: Vec<&str> = english_lists::SHORT_WORDS.split_whitespace().collect();
    let english = tables::english(&read("english_words.txt"), Some(&read("spelling_words.txt")), &extra, &short);
    write("english_words", &english.words);
    write("english_trigrams", &english.trigrams);

    let korean = tables::korean(
        &korean_syllables::TIERS,
        &read("korean_syllables.txt"),
        &read("korean_vocabulary.txt"),
        &read("korean_words.txt"),
        COMMON_COUNT,
    );
    write("korean_syllables", &korean.syllables);
    write("korean_corpus_words", &korean.corpus_words);
    write("korean_vocabulary", &korean.vocabulary);
    write("korean_rare_vocabulary", &korean.rare_vocabulary);
    write("korean_stems", &korean.stems);
    write("korean_list_syllables", &korean.list_syllables);

    let kana = tables::japanese(&read("kana_trigrams.txt")).expect("kana trigram counts");
    write("kana_trigrams", &kana.trigrams);
    write("kana_trigram_contexts", &kana.trigram_contexts);
    write("kana_bigrams", &kana.bigrams);
    write("kana_bigram_contexts", &kana.bigram_contexts);
    write("kana_unigrams", &kana.unigrams);
    write("kana_totals", &kana.totals);
}
