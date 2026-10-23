//! The layout Korean is typed on (`settings.json`'s `"koreanLayout"`), and 세벌식 최종.

use std::cell::Cell;
use std::sync::atomic::{AtomicU8, Ordering};

use crate::hangul::{Composition, Unit};
use crate::jamo;
use crate::latin_layout::LatinLayout;

/// Like `LatinLayout`, it goes by the QWERTY key (the seat), whatever English layout types the
/// letters. Callers don't ask it directly: `hangul::compose` and `DubeolsikLayout` answer for
/// whichever layout is current, so the decision code reads keys the same way for both.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum KoreanLayout {
    /// 두벌식 (KS X 5002), with 도깨비불: a final consonant moves to the next syllable.
    Dubeolsik,
    /// 세벌식 최종: separate keys for initial, medial and final jamo, on digits and punctuation too.
    SebeolsikFinal,
}

static ACTIVE: AtomicU8 = AtomicU8::new(0);
thread_local! {
    static OVERRIDE: Cell<Option<KoreanLayout>> = const { Cell::new(None) };
}

impl KoreanLayout {
    pub const ALL: [KoreanLayout; 2] = [KoreanLayout::Dubeolsik, KoreanLayout::SebeolsikFinal];

    pub fn raw(self) -> &'static str {
        match self {
            KoreanLayout::Dubeolsik => "dubeolsik",
            KoreanLayout::SebeolsikFinal => "sebeolsikFinal",
        }
    }

    pub fn from_raw(raw: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|l| l.raw() == raw)
    }

    /// What the input method types with: set from the settings when they load.
    pub fn set_active(layout: KoreanLayout) {
        ACTIVE.store(layout as u8, Ordering::Relaxed);
    }

    pub fn current() -> KoreanLayout {
        OVERRIDE.with(Cell::get).unwrap_or_else(|| {
            if ACTIVE.load(Ordering::Relaxed) == 1 {
                KoreanLayout::SebeolsikFinal
            } else {
                KoreanLayout::Dubeolsik
            }
        })
    }

    /// For a test (or a tool) that types on another layout without touching the active one.
    pub fn with_override<R>(layout: KoreanLayout, f: impl FnOnce() -> R) -> R {
        let old = OVERRIDE.with(|o| o.replace(Some(layout)));
        let result = f();
        OVERRIDE.with(|o| o.set(old));
        result
    }

    /// Whether a digit or punctuation key, which reaches the session as a symbol, goes on with the
    /// word being typed as Korean instead of ending it. Only 3-set has jamo there (ㅂ on ;, ㅆ on 2,
    /// ㅋ on 0), and only where the jamo makes a syllable people use (`knows`): 갔 (kf2) takes its
    /// 2; 안녕! (ㄲ can't follow 녕's ㅇ), 세요! (욖) and hello! don't. A word starts on one only
    /// with an initial consonant (ㅂ ; ㅌ ' ㅋ 0), and not with 0 right after a digit (100 is a number).
    pub fn continues_word(self, keys: &str, key: char, after_digit: bool, knows: impl Fn(char) -> bool) -> bool {
        if self != KoreanLayout::SebeolsikFinal || key.is_alphabetic() {
            return false;
        }
        let Some(jamo) = SebeolsikFinal::jamo_on_seat(LatinLayout::current().seat_of(key)) else { return false };
        if keys.is_empty() {
            return jamo.position == Position::Initial && !(after_digit && key.is_numeric());
        }
        // As jamo, digits too: 0 is ㅋ, and 09 쿠 is on its way to 퀼프 (a number of them alone
        // still reads as a number when the word is decided).
        let Some(before) = SebeolsikFinal::compose_jamo(keys) else { return false };
        if before.is_broken() {
            return false;
        }
        let mut with_key = keys.to_string();
        with_key.push(key);
        let Some(after) = SebeolsikFinal::compose_jamo(&with_key) else { return false };
        if after.is_broken() {
            return false;
        }
        // A vowel or final has to join a syllable (가 + ㅆ is 갔, ㅋ + ㅠ is 큐): left on its own, or
        // making one nobody types, it was the symbol.
        if jamo.position == Position::Initial {
            return true;
        }
        if let Some(Unit::Syllable(s)) = after.units.last() {
            return knows(*s);
        }
        false
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Position {
    Initial,
    Medial,
    Final,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SebeolsikKey {
    pub jamo: char,
    pub position: Position,
}

/// 세벌식 최종 (공병우 최종), as macOS's and libhangul's 3-set final type it. Jamo are kept as
/// Hangul Compatibility Jamo, as in `jamo`, with the position the key gives them.
///
/// The shifted keys that type a digit or symbol in Korean (⇧H 0, ⇧B ?, ⇧M ") have no jamo here: they
/// type as on QWERTY, and a word with one is not Korean.
pub struct SebeolsikFinal;

impl SebeolsikFinal {
    /// The jamo on a QWERTY key (shifted characters as typed: ! is the 1 key with Shift).
    pub fn jamo_on_seat(seat: char) -> Option<SebeolsikKey> {
        use Position::*;
        let (jamo, position) = match seat {
            'k' => ('ㄱ', Initial),
            'h' => ('ㄴ', Initial),
            'u' => ('ㄷ', Initial),
            'y' => ('ㄹ', Initial),
            'i' => ('ㅁ', Initial),
            ';' => ('ㅂ', Initial),
            'n' => ('ㅅ', Initial),
            'j' => ('ㅇ', Initial),
            'l' => ('ㅈ', Initial),
            'o' => ('ㅊ', Initial),
            '0' => ('ㅋ', Initial),
            '\'' => ('ㅌ', Initial),
            'p' => ('ㅍ', Initial),
            'm' => ('ㅎ', Initial),
            'f' => ('ㅏ', Medial),
            'r' => ('ㅐ', Medial),
            '6' => ('ㅑ', Medial),
            'G' => ('ㅒ', Medial),
            't' => ('ㅓ', Medial),
            'c' => ('ㅔ', Medial),
            'e' => ('ㅕ', Medial),
            '7' => ('ㅖ', Medial),
            'v' => ('ㅗ', Medial),
            '/' => ('ㅗ', Medial),
            '4' => ('ㅛ', Medial),
            'b' => ('ㅜ', Medial),
            '9' => ('ㅜ', Medial),
            '5' => ('ㅠ', Medial),
            'g' => ('ㅡ', Medial),
            '8' => ('ㅢ', Medial),
            'd' => ('ㅣ', Medial),
            'x' => ('ㄱ', Final),
            '!' => ('ㄲ', Final),
            'V' => ('ㄳ', Final),
            's' => ('ㄴ', Final),
            'E' => ('ㄵ', Final),
            'S' => ('ㄶ', Final),
            'A' => ('ㄷ', Final),
            'w' => ('ㄹ', Final),
            '@' => ('ㄺ', Final),
            'F' => ('ㄻ', Final),
            'D' => ('ㄼ', Final),
            'T' => ('ㄽ', Final),
            '%' => ('ㄾ', Final),
            '$' => ('ㄿ', Final),
            'R' => ('ㅀ', Final),
            'z' => ('ㅁ', Final),
            '3' => ('ㅂ', Final),
            'X' => ('ㅄ', Final),
            'q' => ('ㅅ', Final),
            '2' => ('ㅆ', Final),
            'a' => ('ㅇ', Final),
            '#' => ('ㅈ', Final),
            'Z' => ('ㅊ', Final),
            'C' => ('ㅋ', Final),
            'W' => ('ㅌ', Final),
            'Q' => ('ㅍ', Final),
            '1' => ('ㅎ', Final),
            _ => return None,
        };
        Some(SebeolsikKey { jamo, position })
    }

    /// Initial consonants typed twice: ㄱㄱ is ㄲ.
    fn double_initial(c: char) -> Option<char> {
        Some(match c {
            'ㄱ' => 'ㄲ',
            'ㄷ' => 'ㄸ',
            'ㅂ' => 'ㅃ',
            'ㅅ' => 'ㅆ',
            'ㅈ' => 'ㅉ',
            _ => return None,
        })
    }

    /// Finals made of two keys, on top of 2-set's: ㄱㄱ and ㅅㅅ (최종 has keys for the rest).
    fn final_combo(a: char, b: char) -> Option<char> {
        jamo::final_combo(a, b).or(match (a, b) {
            ('ㄱ', 'ㄱ') => Some('ㄲ'),
            ('ㅅ', 'ㅅ') => Some('ㅆ'),
            _ => None,
        })
    }

    /// Whether `key` (as typed) has a jamo here.
    pub fn jamo_for(key: char) -> Option<SebeolsikKey> {
        Self::jamo_on_seat(LatinLayout::current().seat_of(key))
    }

    /// Shift on a key whose shifted form is a jamo (a final, or ㅒ) is Korean typing.
    pub fn is_korean_shift_key(key: char) -> bool {
        let seat = LatinLayout::current().seat_of(key);
        seat.is_uppercase() && Self::jamo_on_seat(seat).is_some()
    }

    /// A capital with no jamo on its key (⇧H is 0 in Korean): only ever English.
    pub fn has_english_only_capital(keys: &str) -> bool {
        keys.chars().any(|key| {
            let seat = LatinLayout::current().seat_of(key);
            seat.is_uppercase() && Self::jamo_on_seat(seat).is_none()
        })
    }

    pub fn starts_syllable(key: char) -> bool {
        Self::jamo_for(key).map(|k| k.position) == Some(Position::Initial)
    }

    /// Nil when a key has no jamo, or when every key is a digit: a number stays a number (05 isn't
    /// 큐, 00 isn't ㅋㅋ).
    pub fn compose(keys: &str) -> Option<Composition> {
        if keys.chars().all(|c| c.is_numeric()) {
            return None;
        }
        Self::compose_jamo(keys)
    }

    /// The keys as jamo, digits alone too; nil when a key has none.
    pub fn compose_jamo(keys: &str) -> Option<Composition> {
        if keys.is_empty() {
            return None;
        }
        let mut units = Vec::new();
        let mut cho: Option<char> = None;
        let mut jung: Option<char> = None;
        let mut jong: Option<char> = None;

        fn flush(units: &mut Vec<Unit>, cho: &mut Option<char>, jung: &mut Option<char>, jong: &mut Option<char>) {
            if let (Some(c), Some(v)) = (*cho, *jung) {
                units.push(Unit::Syllable(jamo::syllable(c, v, *jong)));
            } else {
                for j in [*cho, *jung, *jong].into_iter().flatten() {
                    units.push(Unit::Jamo(j));
                }
            }
            *cho = None;
            *jung = None;
            *jong = None;
        }

        for key in keys.chars() {
            let typed = Self::jamo_for(key)?;
            let j = typed.jamo;
            match typed.position {
                Position::Initial => {
                    if let (Some(c), None) = (cho, jung) {
                        if c == j {
                            if let Some(double) = Self::double_initial(c) {
                                cho = Some(double);
                                continue;
                            }
                        }
                    }
                    flush(&mut units, &mut cho, &mut jung, &mut jong);
                    cho = Some(j);
                }
                Position::Medial => {
                    if let (Some(v), None) = (jung, jong) {
                        if let Some(combined) = jamo::vowel_combo(v, j) {
                            jung = Some(combined);
                            continue;
                        }
                    }
                    if jung.is_none() && jong.is_none() {
                        jung = Some(j);
                    } else {
                        flush(&mut units, &mut cho, &mut jung, &mut jong);
                        jung = Some(j);
                    }
                }
                Position::Final => {
                    if cho.is_some() && jung.is_some() && jong.is_none() {
                        jong = Some(j);
                    } else if let Some(combined) = jong.and_then(|f| Self::final_combo(f, j)) {
                        jong = Some(combined);
                    } else {
                        flush(&mut units, &mut cho, &mut jung, &mut jong);
                        units.push(Unit::Jamo(j));
                    }
                }
            }
        }
        flush(&mut units, &mut cho, &mut jung, &mut jong);
        Some(Composition { units })
    }
}
