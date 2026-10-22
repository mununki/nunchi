//! Romaji → hiragana, following the conventions of the macOS and Microsoft
//! Japanese input methods (nn/n' → ん, doubled consonants → っ, x/l small kana,
//! - → ー). Letters that can't become kana are kept and counted: like stray jamo
//! for Hangul, they are the strongest sign the user wasn't typing Japanese.

use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

use crate::latin_layout::LatinLayout;

#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Romaji {
    /// Kana with any unconvertible letters left in place (こんにc while typing).
    pub text: String,
    /// Letters in the middle or (when finished) at the end that never became kana.
    pub stray_letters: usize,
    /// Letters at the end still waiting to become kana ("k", "ky", "n"); only when partial.
    pub pending: String,
    /// Number of kana produced.
    pub kana_count: usize,
}

impl Romaji {
    pub fn is_clean(&self) -> bool {
        self.stray_letters == 0 && self.pending.is_empty()
    }

    /// Only っ and letters (ff: っf, sss: っっs): a doubled consonant at the start with no
    /// kana after it, which no Japanese word is (って has kana after its っ).
    pub fn is_only_sokuon(&self) -> bool {
        self.text.starts_with('っ') && self.text.chars().all(|c| c == 'っ' || c.is_ascii_alphabetic())
    }
}

fn is_vowel(c: char) -> bool {
    matches!(c, 'a' | 'i' | 'u' | 'e' | 'o')
}

fn is_consonant(c: char) -> bool {
    "bcdfghjklmpqrstvwxyz".contains(c)
}

/// 0–9 as ０–９, for a Japanese phrase with a number in it.
fn full_width_digit(c: char) -> Option<char> {
    c.to_digit(10).filter(|_| c.is_ascii_digit()).and_then(|d| char::from_u32(0xFF10 + d))
}

/// After a digit, - . , / belong to the number (addresses, phone numbers, dates), not ー。、・.
fn number_punctuation(c: char) -> Option<char> {
    Some(match c {
        '-' => '－',
        '.' => '．',
        ',' => '，',
        '/' => '／',
        _ => return None,
    })
}

/// Symbols as Japanese IMEs type them by default: full-width, with
/// [ ] / ~ - as 「」・〜ー. Not ' (it ends ん in kan'i), and digits and spaces stay half-width.
pub const SYMBOLS: [(char, &str); 31] = [
    (',', "、"),
    ('.', "。"),
    ('?', "？"),
    ('!', "！"),
    ('-', "ー"),
    ('/', "・"),
    ('[', "「"),
    (']', "」"),
    ('~', "〜"),
    ('_', "＿"),
    ('(', "（"),
    (')', "）"),
    (':', "："),
    (';', "；"),
    ('"', "”"),
    ('@', "＠"),
    ('#', "＃"),
    ('$', "＄"),
    ('%', "％"),
    ('&', "＆"),
    ('*', "＊"),
    ('+', "＋"),
    ('=', "＝"),
    ('<', "＜"),
    ('>', "＞"),
    ('{', "｛"),
    ('}', "｝"),
    ('|', "｜"),
    ('^', "＾"),
    ('`', "｀"),
    ('\\', "＼"),
];

/// The full-width form a Japanese IME types for `c`, if it has one.
pub fn symbol(c: char) -> Option<&'static str> {
    SYMBOLS.iter().find(|s| s.0 == c).map(|s| s.1)
}

/// What the symbols become, except ー, which is part of a word (らーめん) rather than punctuation.
pub fn is_punctuation(c: char) -> bool {
    c != 'ー' && SYMBOLS.iter().any(|s| s.1.starts_with(c))
}

fn table() -> &'static HashMap<String, String> {
    static TABLE: OnceLock<HashMap<String, String>> = OnceLock::new();
    TABLE.get_or_init(|| {
        let pairs: &[(&str, &str)] = &[
            ("a", "あ"), ("i", "い"), ("u", "う"), ("e", "え"), ("o", "お"),
            ("ka", "か"), ("ki", "き"), ("ku", "く"), ("ke", "け"), ("ko", "こ"),
            ("ga", "が"), ("gi", "ぎ"), ("gu", "ぐ"), ("ge", "げ"), ("go", "ご"),
            ("sa", "さ"), ("si", "し"), ("su", "す"), ("se", "せ"), ("so", "そ"),
            ("za", "ざ"), ("zi", "じ"), ("zu", "ず"), ("ze", "ぜ"), ("zo", "ぞ"),
            ("ta", "た"), ("ti", "ち"), ("tu", "つ"), ("te", "て"), ("to", "と"),
            ("da", "だ"), ("di", "ぢ"), ("du", "づ"), ("de", "で"), ("do", "ど"),
            ("na", "な"), ("ni", "に"), ("nu", "ぬ"), ("ne", "ね"), ("no", "の"),
            ("ha", "は"), ("hi", "ひ"), ("hu", "ふ"), ("he", "へ"), ("ho", "ほ"),
            ("ba", "ば"), ("bi", "び"), ("bu", "ぶ"), ("be", "べ"), ("bo", "ぼ"),
            ("pa", "ぱ"), ("pi", "ぴ"), ("pu", "ぷ"), ("pe", "ぺ"), ("po", "ぽ"),
            ("ma", "ま"), ("mi", "み"), ("mu", "む"), ("me", "め"), ("mo", "も"),
            ("ya", "や"), ("yi", "い"), ("yu", "ゆ"), ("ye", "いぇ"), ("yo", "よ"),
            ("ra", "ら"), ("ri", "り"), ("ru", "る"), ("re", "れ"), ("ro", "ろ"),
            ("wa", "わ"), ("wi", "うぃ"), ("wu", "う"), ("we", "うぇ"), ("wo", "を"),
            ("fa", "ふぁ"), ("fi", "ふぃ"), ("fu", "ふ"), ("fe", "ふぇ"), ("fo", "ふぉ"),
            ("va", "ゔぁ"), ("vi", "ゔぃ"), ("vu", "ゔ"), ("ve", "ゔぇ"), ("vo", "ゔぉ"),
            ("qa", "くぁ"), ("qi", "くぃ"), ("qu", "く"), ("qe", "くぇ"), ("qo", "くぉ"),
            ("shi", "し"), ("chi", "ち"), ("tsu", "つ"), ("ji", "じ"),
            ("sha", "しゃ"), ("shu", "しゅ"), ("she", "しぇ"), ("sho", "しょ"),
            ("cha", "ちゃ"), ("chu", "ちゅ"), ("che", "ちぇ"), ("cho", "ちょ"),
            ("ja", "じゃ"), ("ju", "じゅ"), ("je", "じぇ"), ("jo", "じょ"),
            ("tsa", "つぁ"), ("tsi", "つぃ"), ("tse", "つぇ"), ("tso", "つぉ"),
            ("tha", "てゃ"), ("thi", "てぃ"), ("thu", "てゅ"), ("the", "てぇ"), ("tho", "てょ"),
            ("dha", "でゃ"), ("dhi", "でぃ"), ("dhu", "でゅ"), ("dhe", "でぇ"), ("dho", "でょ"),
            ("twu", "とぅ"), ("dwu", "どぅ"),
            ("wha", "うぁ"), ("whi", "うぃ"), ("whu", "う"), ("whe", "うぇ"), ("who", "うぉ"),
            ("xa", "ぁ"), ("xi", "ぃ"), ("xu", "ぅ"), ("xe", "ぇ"), ("xo", "ぉ"),
            ("la", "ぁ"), ("li", "ぃ"), ("lu", "ぅ"), ("le", "ぇ"), ("lo", "ぉ"),
            ("xya", "ゃ"), ("xyu", "ゅ"), ("xyo", "ょ"), ("lya", "ゃ"), ("lyu", "ゅ"), ("lyo", "ょ"),
            ("xtu", "っ"), ("ltu", "っ"), ("xtsu", "っ"), ("ltsu", "っ"), ("xwa", "ゎ"), ("lwa", "ゎ"),
            // As macOS and MS-IME accept them (not ca/ci/cu: they made English words read as romaji).
            ("kwa", "くぁ"), ("gwa", "ぐぁ"), ("xn", "ん"),
        ];
        let mut t: HashMap<String, String> = pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        for (symbol, full) in SYMBOLS.iter() {
            t.insert(symbol.to_string(), full.to_string());
        }
        // Contracted sounds: きゃ, ぎゅ, みょ…
        let rows = [
            ("k", "き"), ("g", "ぎ"), ("s", "し"), ("z", "じ"), ("t", "ち"), ("d", "ぢ"), ("n", "に"), ("h", "ひ"),
            ("b", "び"), ("p", "ぴ"), ("m", "み"), ("r", "り"), ("c", "ち"), ("j", "じ"), ("f", "ふ"),
        ];
        for (consonant, i) in rows {
            t.insert(format!("{consonant}ya"), format!("{i}ゃ"));
            t.insert(format!("{consonant}yi"), format!("{i}ぃ"));
            t.insert(format!("{consonant}yu"), format!("{i}ゅ"));
            t.insert(format!("{consonant}ye"), format!("{i}ぇ"));
            t.insert(format!("{consonant}yo"), format!("{i}ょ"));
        }
        t
    })
}

/// Every proper prefix of a table entry: what a word may end in while still being typed.
fn prefixes() -> &'static HashSet<String> {
    static PREFIXES: OnceLock<HashSet<String>> = OnceLock::new();
    PREFIXES.get_or_init(|| {
        let mut p: HashSet<String> = HashSet::from(["n".to_string()]);
        for key in table().keys() {
            let chars: Vec<char> = key.chars().collect();
            for length in 1..chars.len() {
                p.insert(chars[..length].iter().collect());
            }
        }
        p
    })
}

/// The shortest table entry for each single kana (ties: alphabetical); ん as "n'" so a
/// vowel typed after it can't take it over (n'a is んあ, nna would be んな).
fn spelling() -> &'static HashMap<char, String> {
    static SPELLING: OnceLock<HashMap<char, String>> = OnceLock::new();
    SPELLING.get_or_init(|| {
        let mut s: HashMap<char, String> = HashMap::from([('ん', "n'".to_string())]);
        for (key, value) in table() {
            let mut chars = value.chars();
            let (Some(ch), None) = (chars.next(), chars.next()) else { continue };
            if let Some(old) = s.get(&ch) {
                let (o, k) = (old.chars().count(), key.chars().count());
                if o < k || (o == k && old < key) {
                    continue;
                }
            }
            if ch != 'ん' {
                s.insert(ch, key.clone());
            }
        }
        s
    })
}

/// `partial`: the word is still being typed, so a trailing "k" or "n" is waiting for its vowel
/// rather than stray.
pub fn compose(keys: &str, partial: bool) -> Romaji {
    // Matched in lowercase; letters that don't become kana stay as typed (⇧P waiting for its
    // vowel shows P, not p).
    let typed: Vec<char> = LatinLayout::romaji_keys(keys).chars().collect();
    let chars: Vec<char> = typed.iter().map(|c| if c.is_ascii() { c.to_ascii_lowercase() } else { *c }).collect();
    let table = table();
    let mut out = String::new();
    let mut stray = 0;
    let mut kana = 0;
    let mut pending = String::new();
    let mut i = 0;
    let n = chars.len();

    let mut emit = |out: &mut String, s: &str| {
        out.push_str(s);
        kana += s.chars().count();
    };

    let mut buf = String::with_capacity(16);
    while i < n {
        let c = chars[i];
        let next = chars.get(i + 1).copied();

        // Digits are full-width, and so is the punctuation inside a number (０３－１２３４, ３．５).
        if let Some(full) = full_width_digit(c) {
            emit(&mut out, full.encode_utf8(&mut [0; 4]));
            i += 1;
            continue;
        }
        if i > 0 && chars[i - 1].is_ascii_digit() {
            if let Some(full) = number_punctuation(c) {
                emit(&mut out, full.encode_utf8(&mut [0; 4]));
                i += 1;
                continue;
            }
        }
        if c == 'n' {
            if let Some(next) = next {
                if next == '\'' {
                    emit(&mut out, "ん");
                    i += 2;
                    continue;
                }
                if next == 'n' {
                    // "nni" → んに, "nn" → ん
                    let after = chars.get(i + 2).copied();
                    emit(&mut out, "ん");
                    i += if after.is_some_and(|a| is_vowel(a) || a == 'y') { 1 } else { 2 };
                    continue;
                }
                if !is_vowel(next) && next != 'y' {
                    emit(&mut out, "ん");
                    i += 1;
                    continue;
                }
            } else {
                if partial {
                    pending = typed[i].to_string();
                } else {
                    emit(&mut out, "ん");
                }
                i += 1;
                continue;
            }
        } else if next == Some(c) && is_consonant(c) {
            emit(&mut out, "っ");
            i += 1;
            continue;
        } else if c == 't' && next == Some('c') && i + 2 < n && chars[i + 2] == 'h' {
            emit(&mut out, "っ");
            i += 1;
            continue; // matcha: "tch" → っch
        }

        let mut matched = false;
        for length in (1..=4.min(n - i)).rev() {
            buf.clear();
            buf.extend(&chars[i..i + length]);
            if let Some(k) = table.get(&buf) {
                emit(&mut out, k);
                i += length;
                matched = true;
                break;
            }
        }
        if matched {
            continue;
        }

        if partial {
            buf.clear();
            buf.extend(&chars[i..]);
            if prefixes().contains(&buf) {
                pending = typed[i..].iter().collect();
                break;
            }
        }
        out.push(typed[i]);
        stray += 1;
        i += 1;
    }
    let text = out + &pending;
    Romaji { text, stray_letters: stray, pending, kana_count: kana }
}

/// Hiragana → katakana (for F7-style katakana output and loanwords).
pub fn katakana(hiragana: &str) -> String {
    hiragana
        .chars()
        .map(|c| if ('\u{3041}'..='\u{3096}').contains(&c) { char::from_u32(c as u32 + 0x60).unwrap_or(c) } else { c })
        .collect()
}

/// A spelling that composes back to `kana` (し → "si", ん → "n'", っ → "ltu"), or nil if
/// some character has none. For keys rewritten after Delete took off a whole kana.
pub fn romaji(kana: &str) -> Option<String> {
    let spelling = spelling();
    let mut out = String::new();
    for ch in kana.chars() {
        out.push_str(spelling.get(&ch)?);
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn composes_common_romaji() {
        assert_eq!(compose("konnichiha", false).text, "こんにちは");
        assert_eq!(compose("kitte", false).text, "きって");
        assert_eq!(compose("matcha", false).text, "まっちゃ");
        assert_eq!(compose("kan'i", false).text, "かんい");
        assert_eq!(compose("nihon", true).pending, "n");
        assert_eq!(compose("hello", false).text, "へっぉ");
        let k = compose("watashihag", true);
        assert_eq!(k.text, "わたしはg");
        assert_eq!(k.pending, "g");
        assert_eq!(romaji("し").as_deref(), Some("si"));
        assert_eq!(romaji("っ").as_deref(), Some("ltu"));
    }
}
