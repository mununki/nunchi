//! Compact tables of the word lists and models, built from `data/` when the crate is built
//! (build.rs) and read in place from the binary. An input method runs inside every app, and a
//! table that is part of the DLL's image is shared by all of them, where lists and maps built
//! at run time would take tens of megabytes in each one.
//!
//! build.rs compiles this file too (`#[path]`), so it uses std only.

use std::collections::{HashMap, HashSet};

/// A sorted table of byte-string keys, each with a value of `width` bytes, looked up by binary
/// search. Layout, little endian: u32 count, u32 width, (count + 1) u32 key offsets, count × width
/// value bytes, then the keys.
#[derive(Clone, Copy)]
pub struct Table<'a> {
    bytes: &'a [u8],
}

impl<'a> Table<'a> {
    pub const fn new(bytes: &'a [u8]) -> Self {
        Table { bytes }
    }

    fn word(&self, at: usize) -> usize {
        let b = self.bytes;
        u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]]) as usize
    }

    pub fn len(&self) -> usize {
        if self.bytes.len() < 8 {
            0
        } else {
            self.word(0)
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn width(&self) -> usize {
        self.word(4)
    }

    fn values_start(&self) -> usize {
        8 + (self.len() + 1) * 4
    }

    pub fn key(&self, i: usize) -> &'a [u8] {
        let bytes: &'a [u8] = self.bytes;
        let base = self.values_start() + self.len() * self.width();
        let (start, end) = (self.word(8 + i * 4), self.word(8 + (i + 1) * 4));
        &bytes[base + start..base + end]
    }

    pub fn value(&self, i: usize) -> &'a [u8] {
        let bytes: &'a [u8] = self.bytes;
        let width = self.width();
        let at = self.values_start() + i * width;
        &bytes[at..at + width]
    }

    pub fn find(&self, key: &[u8]) -> Option<usize> {
        let (mut low, mut high) = (0, self.len());
        while low < high {
            let middle = (low + high) / 2;
            match self.key(middle).cmp(key) {
                std::cmp::Ordering::Less => low = middle + 1,
                std::cmp::Ordering::Greater => high = middle,
                std::cmp::Ordering::Equal => return Some(middle),
            }
        }
        None
    }

    pub fn contains(&self, key: &[u8]) -> bool {
        self.find(key).is_some()
    }

    pub fn get(&self, key: &[u8]) -> Option<&'a [u8]> {
        self.find(key).map(|i| self.value(i))
    }
}

pub fn f32_of(bytes: &[u8]) -> f32 {
    f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
}

pub fn f64_of(bytes: &[u8]) -> f64 {
    let mut b = [0u8; 8];
    b.copy_from_slice(&bytes[..8]);
    f64::from_le_bytes(b)
}

/// The UTF-8 of up to four characters in `buffer`, as a key.
pub fn key_of<'b>(buffer: &'b mut [u8; 16], chars: &[char]) -> &'b [u8] {
    let mut n = 0;
    for c in chars {
        n += c.encode_utf8(&mut buffer[n..]).len();
    }
    &buffer[..n]
}

fn push_u32(out: &mut Vec<u8>, n: usize) {
    out.extend_from_slice(&(n as u32).to_le_bytes());
}

/// A table of `entries` (unique keys), each value `width` bytes.
pub fn encode(mut entries: Vec<(Vec<u8>, Vec<u8>)>, width: usize) -> Vec<u8> {
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    entries.dedup_by(|a, b| a.0 == b.0);
    let mut out = Vec::new();
    push_u32(&mut out, entries.len());
    push_u32(&mut out, width);
    let mut offset = 0;
    push_u32(&mut out, 0);
    for (key, _) in &entries {
        offset += key.len();
        push_u32(&mut out, offset);
    }
    for (_, value) in &entries {
        assert_eq!(value.len(), width, "every value has the table's width");
        out.extend_from_slice(value);
    }
    for (key, _) in &entries {
        out.extend_from_slice(key);
    }
    out
}

/// A table of keys alone.
pub fn set<I: IntoIterator<Item = String>>(keys: I) -> Vec<u8> {
    encode(keys.into_iter().map(|k| (k.into_bytes(), Vec::new())).collect(), 0)
}

fn floats<I: IntoIterator<Item = (String, f32)>>(entries: I) -> Vec<u8> {
    encode(entries.into_iter().map(|(k, v)| (k.into_bytes(), v.to_le_bytes().to_vec())).collect(), 4)
}

/// The `\t`-separated fields of a line, when there are exactly two.
fn two_fields(line: &str) -> Option<(&str, &str)> {
    let mut parts = line.split('\t');
    match (parts.next(), parts.next(), parts.next()) {
        (Some(a), Some(b), None) => Some((a, b)),
        _ => None,
    }
}

// MARK: - English

/// Symbols of the letter trigrams: 0 = word boundary, 1...26 = a...z.
pub const ENGLISH_SYMBOLS: usize = 27;
/// Level given to the extra words (tech terms and slang SCOWL lacks): known, not "common".
pub const ENGLISH_EXTRA_LEVEL: i32 = 35;
/// In the words table, a word the list itself doesn't have (only one of the extra words).
pub const NOT_LISTED: u8 = u8::MAX;

/// [^, ^, letters..., $] with ^ and $ both encoded as 0.
pub fn english_codes(word: &str) -> Vec<usize> {
    let mut codes = Vec::with_capacity(word.len() + 3);
    codes.push(0);
    codes.push(0);
    for c in word.chars() {
        if c.is_ascii_lowercase() {
            codes.push(c as usize - 96);
        }
    }
    codes.push(0);
    codes
}

fn is_ascii_word(w: &str) -> bool {
    !w.is_empty() && w.chars().all(|c| c.is_ascii_alphabetic())
}

pub struct English {
    /// Each word (lowercase, letters only) with two bytes: its level, and its level in the
    /// list itself (`NOT_LISTED`), which a guessed inflection may take from its stem.
    pub words: Vec<u8>,
    /// Trigram counts, then context counts, as f32.
    pub trigrams: Vec<u8>,
}

/// From a word<TAB>level list (SCOWL), the extra words, and the spelling list the trigrams are
/// trained on (the list itself without one), as upstream's `EnglishModel(listURL:spellingPath:)`.
pub fn english(list: &str, spelling: Option<&str>, extra: &[&str], short_words: &[&str]) -> English {
    let mut levels: HashMap<String, i32> = HashMap::new();
    for line in list.lines() {
        if let Some((word, level)) = two_fields(line) {
            if let Ok(level) = level.trim().parse::<i32>() {
                levels.insert(word.to_string(), level);
            }
        }
    }
    let listed = levels.clone();
    for word in extra {
        let w = word.to_lowercase();
        let level = levels.get(&w).copied().unwrap_or(ENGLISH_EXTRA_LEVEL).min(ENGLISH_EXTRA_LEVEL);
        levels.insert(w, level);
    }
    let short: HashSet<&str> = short_words.iter().copied().collect();
    let normalized = |raw: &HashMap<String, i32>| {
        let mut out: HashMap<String, i32> = HashMap::with_capacity(raw.len());
        for (raw, &level) in raw {
            let word: String = raw.chars().filter(|&c| c != '\'').collect();
            if !is_ascii_word(&word) {
                continue;
            }
            // Many 1–2 letter list entries aren't words people type ("ra", "ko"); only real ones count.
            if word.len() > 2 || short.contains(word.as_str()) {
                let old = out.get(&word).copied().unwrap_or(level);
                out.insert(word, old.min(level));
            }
        }
        out
    };
    let levels = normalized(&levels);
    let listed = normalized(&listed);

    let n = ENGLISH_SYMBOLS;
    let mut tri = vec![0f32; n * n * n];
    let mut ctx = vec![0f32; n * n];
    let mut train = |word: &str| {
        if !is_ascii_word(word) {
            return;
        }
        let codes = english_codes(word);
        for i in 2..codes.len() {
            tri[(codes[i - 2] * n + codes[i - 1]) * n + codes[i]] += 1.0;
            ctx[codes[i - 2] * n + codes[i - 1]] += 1.0;
        }
    };
    match spelling {
        Some(text) => {
            for line in text.lines().filter(|l| !l.is_empty()) {
                train(&line.to_lowercase());
            }
            for word in extra {
                train(word);
            }
        }
        None => levels.keys().for_each(|w| train(w)),
    }

    let words = encode(
        levels
            .iter()
            .map(|(w, &l)| {
                let in_list = listed.get(w).map_or(NOT_LISTED, |&l| l as u8);
                (w.clone().into_bytes(), vec![l as u8, in_list])
            })
            .collect(),
        2,
    );
    let mut trigrams = Vec::with_capacity((tri.len() + ctx.len()) * 4);
    for v in tri.iter().chain(ctx.iter()) {
        trigrams.extend_from_slice(&v.to_le_bytes());
    }
    English { words, trigrams }
}

// MARK: - Korean

/// Case particles, by what the noun must end in: a final consonant (을), none (를), or either.
const PARTICLES: [(&str, Option<bool>); 14] = [
    ("을", Some(true)),
    ("은", Some(true)),
    ("이", Some(true)),
    ("과", Some(true)),
    ("으로", Some(true)),
    ("를", Some(false)),
    ("는", Some(false)),
    ("가", Some(false)),
    ("와", Some(false)),
    ("에", None),
    ("의", None),
    ("도", None),
    ("에서", None),
    ("만", None),
];

fn is_hangul_syllable(c: char) -> bool {
    ('\u{AC00}'..='\u{D7A3}').contains(&c)
}

pub struct Korean {
    /// Each syllable with its log-probability, as f64: the hand-made tiers and the corpus counts,
    /// whichever rates it higher.
    pub syllables: Vec<u8>,
    /// Words seen in Korean sentences (Tatoeba): 나는, 무슨, 내가…
    pub corpus_words: Vec<u8>,
    /// Words of the wider vocabulary seen `common_count` times or more, and the rest.
    pub vocabulary: Vec<u8>,
    pub rare_vocabulary: Vec<u8>,
    /// Nouns the lists only have with a case particle (혜택 from 혜택을).
    pub stems: Vec<u8>,
    /// Every syllable of a vocabulary or corpus word.
    pub list_syllables: Vec<u8>,
}

/// As upstream's `KoreanModel(source: .max)` and its word lists. `tiers`: syllables and their
/// log-probability, later tiers overwriting earlier ones.
pub fn korean(tiers: &[(&str, f64)], counts: &str, vocabulary: &str, words: &str, common_count: u32) -> Korean {
    let mut map: HashMap<char, f64> = HashMap::new();
    for (syllables, log_p) in tiers {
        for s in syllables.chars().filter(|c| is_hangul_syllable(*c)) {
            map.insert(s, *log_p);
        }
    }
    let mut corpus: HashMap<char, u64> = HashMap::new();
    for line in counts.lines() {
        if let Some((s, n)) = two_fields(line) {
            if let (Some(c), Ok(n)) = (s.chars().next(), n.trim().parse::<u64>()) {
                corpus.insert(c, n);
            }
        }
    }
    let total: f64 = corpus.values().map(|&n| n as f64).sum();
    let (smoothing, possible) = (0.5, 11172.0);
    for (s, n) in &corpus {
        let log_p = ((*n as f64 + smoothing) / (total + smoothing * possible)).ln();
        let old = map.get(s).copied().unwrap_or(f64::NEG_INFINITY);
        map.insert(*s, old.max(log_p));
    }

    let mut common = HashSet::new();
    let mut rare = HashSet::new();
    for line in vocabulary.lines() {
        let Some((word, count)) = two_fields(line) else { continue };
        let Ok(n) = count.trim().parse::<u32>() else { continue };
        if n >= common_count {
            common.insert(word.to_string());
        } else {
            rare.insert(word.to_string());
        }
    }
    let corpus_words: HashSet<String> =
        words.lines().filter(|l| !l.is_empty()).filter_map(|l| l.split('\t').next().map(str::to_string)).collect();

    let mut stems = HashSet::new();
    for word in corpus_words.iter().chain(common.iter()).chain(rare.iter()) {
        for (particle, needs_final) in PARTICLES {
            let Some(stem) = word.strip_suffix(particle) else { continue };
            if stem.chars().count() < 2 {
                continue;
            }
            let Some(last) = stem.chars().next_back().filter(|c| is_hangul_syllable(*c)) else { continue };
            let has_final = (last as u32 - 0xAC00) % 28 != 0;
            if needs_final.is_none() || needs_final == Some(has_final) {
                stems.insert(stem.to_string());
            }
        }
    }
    let mut list_syllables: HashSet<char> = HashSet::new();
    for word in common.iter().chain(corpus_words.iter()) {
        list_syllables.extend(word.chars());
    }

    Korean {
        syllables: encode(
            map.iter().map(|(s, p)| (s.to_string().into_bytes(), p.to_le_bytes().to_vec())).collect(),
            8,
        ),
        corpus_words: set(corpus_words),
        vocabulary: set(common),
        rare_vocabulary: set(rare),
        stems: set(stems),
        list_syllables: set(list_syllables.into_iter().map(String::from)),
    }
}

// MARK: - Japanese

pub struct Japanese {
    /// Counts as f32: trigrams (abc), their contexts (ab), bigrams (bc), their contexts (b), and
    /// unigrams (c); keys are the characters' UTF-8.
    pub trigrams: Vec<u8>,
    pub trigram_contexts: Vec<u8>,
    pub bigrams: Vec<u8>,
    pub bigram_contexts: Vec<u8>,
    pub unigrams: Vec<u8>,
    /// The total count and the number of symbols, as f32.
    pub totals: Vec<u8>,
}

/// From trigram<TAB>count lines, as upstream's `JapaneseModel(contentsOf:)`; nil without counts.
pub fn japanese(text: &str) -> Option<Japanese> {
    let mut trigrams: HashMap<String, f32> = HashMap::new();
    let mut trigram_contexts: HashMap<String, f32> = HashMap::new();
    let mut bigrams: HashMap<String, f32> = HashMap::new();
    let mut bigram_contexts: HashMap<String, f32> = HashMap::new();
    let mut unigrams: HashMap<String, f32> = HashMap::new();
    let mut symbols = HashSet::new();
    let mut total = 0f32;
    for line in text.lines() {
        let Some((gram, count)) = two_fields(line) else { continue };
        let Ok(count) = count.trim().parse::<f32>() else { continue };
        let chars: Vec<char> = gram.chars().collect();
        let [a, b, c] = chars[..] else { continue };
        *trigrams.entry([a, b, c].iter().collect()).or_default() += count;
        *trigram_contexts.entry([a, b].iter().collect()).or_default() += count;
        *bigrams.entry([b, c].iter().collect()).or_default() += count;
        *bigram_contexts.entry(b.to_string()).or_default() += count;
        *unigrams.entry(c.to_string()).or_default() += count;
        total += count;
        symbols.extend([a, b, c]);
    }
    if total <= 0.0 {
        return None;
    }
    let mut totals = total.to_le_bytes().to_vec();
    totals.extend_from_slice(&(symbols.len() as f32).to_le_bytes());
    Some(Japanese {
        trigrams: floats(trigrams),
        trigram_contexts: floats(trigram_contexts),
        bigrams: floats(bigrams),
        bigram_contexts: floats(bigram_contexts),
        unigrams: floats(unigrams),
        totals,
    })
}
