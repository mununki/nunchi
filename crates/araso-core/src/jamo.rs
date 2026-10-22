//! Hangul jamo tables and the 2-set (두벌식) QWERTY layout.
//!
//! All jamo are represented as Hangul Compatibility Jamo (U+3131…U+3163),
//! which is also what gets emitted for stray, uncombined jamo.

use crate::latin_layout::LatinLayout;
use crate::sebeolsik::{KoreanLayout, SebeolsikFinal};

pub const CHOSEONG: [char; 19] = [
    'ㄱ', 'ㄲ', 'ㄴ', 'ㄷ', 'ㄸ', 'ㄹ', 'ㅁ', 'ㅂ', 'ㅃ', 'ㅅ', 'ㅆ', 'ㅇ', 'ㅈ', 'ㅉ', 'ㅊ', 'ㅋ', 'ㅌ', 'ㅍ', 'ㅎ',
];
pub const JUNGSEONG: [char; 21] = [
    'ㅏ', 'ㅐ', 'ㅑ', 'ㅒ', 'ㅓ', 'ㅔ', 'ㅕ', 'ㅖ', 'ㅗ', 'ㅘ', 'ㅙ', 'ㅚ', 'ㅛ', 'ㅜ', 'ㅝ', 'ㅞ', 'ㅟ', 'ㅠ', 'ㅡ',
    'ㅢ', 'ㅣ',
];
/// Index 0 is "no final consonant", so a final's index is its position here plus one.
pub const JONGSEONG: [char; 27] = [
    'ㄱ', 'ㄲ', 'ㄳ', 'ㄴ', 'ㄵ', 'ㄶ', 'ㄷ', 'ㄹ', 'ㄺ', 'ㄻ', 'ㄼ', 'ㄽ', 'ㄾ', 'ㄿ', 'ㅀ', 'ㅁ', 'ㅂ', 'ㅄ', 'ㅅ',
    'ㅆ', 'ㅇ', 'ㅈ', 'ㅊ', 'ㅋ', 'ㅌ', 'ㅍ', 'ㅎ',
];

pub fn cho_index(j: char) -> Option<usize> {
    CHOSEONG.iter().position(|&c| c == j)
}

pub fn jung_index(j: char) -> Option<usize> {
    JUNGSEONG.iter().position(|&c| c == j)
}

/// 1…27 for a final consonant.
pub fn jong_index(j: char) -> Option<usize> {
    JONGSEONG.iter().position(|&c| c == j).map(|i| i + 1)
}

pub fn vowel_combo(a: char, b: char) -> Option<char> {
    Some(match (a, b) {
        ('ㅗ', 'ㅏ') => 'ㅘ',
        ('ㅗ', 'ㅐ') => 'ㅙ',
        ('ㅗ', 'ㅣ') => 'ㅚ',
        ('ㅜ', 'ㅓ') => 'ㅝ',
        ('ㅜ', 'ㅔ') => 'ㅞ',
        ('ㅜ', 'ㅣ') => 'ㅟ',
        ('ㅡ', 'ㅣ') => 'ㅢ',
        _ => return None,
    })
}

const FINAL_COMBOS: [(char, char, char); 11] = [
    ('ㄱ', 'ㅅ', 'ㄳ'),
    ('ㄴ', 'ㅈ', 'ㄵ'),
    ('ㄴ', 'ㅎ', 'ㄶ'),
    ('ㄹ', 'ㄱ', 'ㄺ'),
    ('ㄹ', 'ㅁ', 'ㄻ'),
    ('ㄹ', 'ㅂ', 'ㄼ'),
    ('ㄹ', 'ㅅ', 'ㄽ'),
    ('ㄹ', 'ㅌ', 'ㄾ'),
    ('ㄹ', 'ㅍ', 'ㄿ'),
    ('ㄹ', 'ㅎ', 'ㅀ'),
    ('ㅂ', 'ㅅ', 'ㅄ'),
];

pub fn final_combo(a: char, b: char) -> Option<char> {
    FINAL_COMBOS.iter().find(|f| f.0 == a && f.1 == b).map(|f| f.2)
}

/// Inverse of `final_combo`: compound final → (stays as final, moves to next syllable).
pub fn final_split(combined: char) -> Option<(char, char)> {
    FINAL_COMBOS.iter().find(|f| f.2 == combined).map(|f| (f.0, f.1))
}

pub fn is_consonant(j: char) -> bool {
    cho_index(j).is_some() || jong_index(j).is_some()
}

pub fn is_vowel(j: char) -> bool {
    jung_index(j).is_some()
}

pub fn syllable(cho: char, jung: char, jong: Option<char>) -> char {
    let c = cho_index(cho).expect("initial consonant") as u32;
    let v = jung_index(jung).expect("vowel") as u32;
    let f = jong.and_then(jong_index).unwrap_or(0) as u32;
    char::from_u32(0xAC00 + (c * 21 + v) * 28 + f).expect("Hangul syllable")
}

/// The 2-set layout, and where the rest of the code asks about Korean keys: with 3-set chosen
/// (`KoreanLayout`), these answer for `SebeolsikFinal` instead.
pub struct DubeolsikLayout;

impl DubeolsikLayout {
    fn lower(key: char) -> Option<char> {
        Some(match key {
            'q' => 'ㅂ',
            'w' => 'ㅈ',
            'e' => 'ㄷ',
            'r' => 'ㄱ',
            't' => 'ㅅ',
            'y' => 'ㅛ',
            'u' => 'ㅕ',
            'i' => 'ㅑ',
            'o' => 'ㅐ',
            'p' => 'ㅔ',
            'a' => 'ㅁ',
            's' => 'ㄴ',
            'd' => 'ㅇ',
            'f' => 'ㄹ',
            'g' => 'ㅎ',
            'h' => 'ㅗ',
            'j' => 'ㅓ',
            'k' => 'ㅏ',
            'l' => 'ㅣ',
            'z' => 'ㅋ',
            'x' => 'ㅌ',
            'c' => 'ㅊ',
            'v' => 'ㅍ',
            'b' => 'ㅠ',
            'n' => 'ㅜ',
            'm' => 'ㅡ',
            _ => return None,
        })
    }

    fn shifted(key: char) -> Option<char> {
        Some(match key {
            'Q' => 'ㅃ',
            'W' => 'ㅉ',
            'E' => 'ㄸ',
            'R' => 'ㄲ',
            'T' => 'ㅆ',
            'O' => 'ㅒ',
            'P' => 'ㅖ',
            _ => return None,
        })
    }

    /// Keys whose shifted form produces a different jamo. Shift on any other
    /// letter is a signal the user is typing English.
    pub fn is_korean_shift_key(key: char) -> bool {
        if KoreanLayout::current() == KoreanLayout::SebeolsikFinal {
            return SebeolsikFinal::is_korean_shift_key(key);
        }
        Self::shifted(Self::seat(key)).is_some()
    }

    /// The QWERTY key `key` was typed on (`LatinLayout`): 2-set Korean goes by key, not letter.
    fn seat(key: char) -> char {
        LatinLayout::current().seat_of(key)
    }

    /// A capital on a key with no shifted jamo, not counting a lingering Shift: Shift still down
    /// for the one key right after a Korean shift key (어떤 typed fast is djEJs, 바뀌 is qkRNl:
    /// Shift was still down for the vowel after ㄸ or ㄲ). Such keys read as Korean or English by
    /// their scores; any other capital (Hello, API, dHello) is only ever English. A Shift lingers
    /// between lowercase keys, so a word typed in capitals throughout (TM, RN, TL: acronyms) gets
    /// no such allowance, nor does ㅒ/ㅖ with no consonant before it (PDF, OAuth).
    pub fn has_english_only_capital(keys: &str) -> bool {
        if KoreanLayout::current() == KoreanLayout::SebeolsikFinal {
            return SebeolsikFinal::has_english_only_capital(keys);
        }
        let chars: Vec<char> = keys.chars().map(Self::seat).collect();
        let has_lowercase = chars.iter().any(|c| c.is_lowercase());
        for (i, &key) in chars.iter().enumerate() {
            if !(key.is_uppercase() && Self::shifted(key).is_none()) {
                continue;
            }
            if has_lowercase
                && i >= 1
                && Self::lingers(chars[i - 1], if i >= 2 { Some(chars[i - 2]) } else { None }, key)
            {
                continue;
            }
            return true;
        }
        false
    }

    /// Shift still down for `key`, right after `shift_key`, a Korean shift key that needed it. After
    /// a double consonant comes a vowel (djEJs, 어떤), or, when the double consonant is a final one
    /// (a vowel key before it), the next syllable's consonant (dlTDj, 있어). After ㅒ or ㅖ typed on a
    /// consonant comes a consonant (dPFmf, 예를); at the start of a word (PDF, OAuth) nothing lingers.
    /// The key before has to be lowercase: a capital there (RPA, TPS) was Shift on purpose.
    /// Takes seats (`has_english_only_capital` translated them).
    fn lingers(shift_key: char, two_back: Option<char>, key: char) -> bool {
        let before = two_back.and_then(|c| if c.is_lowercase() { Self::jamo_on_seat(c) } else { None });
        match Self::shifted(shift_key) {
            Some('ㅃ' | 'ㅉ' | 'ㄸ' | 'ㄲ' | 'ㅆ') => {
                if Self::is_vowel_key(key) {
                    return true;
                }
                before.map(is_vowel) == Some(true) && Self::is_consonant_key(key)
            }
            Some('ㅒ' | 'ㅖ') => before.map(is_consonant) == Some(true) && Self::is_consonant_key(key),
            _ => false,
        }
    }

    fn is_vowel_key(seat: char) -> bool {
        Self::jamo_on_seat(seat).map(is_vowel).unwrap_or(false)
    }

    fn is_consonant_key(seat: char) -> bool {
        Self::jamo_on_seat(seat).map(is_consonant).unwrap_or(false)
    }

    /// The jamo a key produces, or nil for non-letters.
    /// Uppercase letters without a shifted jamo fall back to the lowercase jamo,
    /// mirroring how the system 2-set keyboard behaves.
    pub fn jamo(key: char) -> Option<char> {
        if KoreanLayout::current() == KoreanLayout::SebeolsikFinal {
            return SebeolsikFinal::jamo_for(key).map(|k| k.jamo);
        }
        Self::jamo_on_seat(Self::seat(key))
    }

    fn jamo_on_seat(key: char) -> Option<char> {
        if let Some(j) = Self::shifted(key) {
            return Some(j);
        }
        if !key.is_ascii() {
            return None;
        }
        Self::lower(key.to_ascii_lowercase())
    }

    /// A Korean syllable has to start with a consonant key.
    pub fn starts_syllable(key: char) -> bool {
        if KoreanLayout::current() == KoreanLayout::SebeolsikFinal {
            return SebeolsikFinal::starts_syllable(key);
        }
        Self::jamo(key).map(is_consonant).unwrap_or(false)
    }
}
