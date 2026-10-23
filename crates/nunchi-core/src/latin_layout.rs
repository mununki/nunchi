//! The layout the user types English (and romaji) on: `settings.json`'s `"latinLayout"`.
//!
//! Keys are kept as this layout's letters, so English and romaji read them as they are, and only
//! Korean looks through to the physical key: 2-set Korean sits on the keys where QWERTY has its
//! letters, whatever layout the letters come from (`DubeolsikLayout` asks `seat_of`). On QWERTY
//! a letter and its seat are the same character, so nothing changes there.
//!
//! Dvorak is left out on purpose: it puts `'`, `,`, `.` and `;` on the keys of ㅂ, ㅈ, ㄷ and ㅋ,
//! and a word can't end on punctuation and keep composing on it at once without a rule for which
//! one was meant (upstream docs/keyboard-layouts.md).

use std::cell::Cell;
use std::sync::atomic::{AtomicU8, Ordering};

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum LatinLayout {
    Qwerty,
    Colemak,
    /// Colemak Mod-DH, ANSI rows (no angle mod): D and H move to the bottom row.
    ColemakDH,
}

const NONE: u8 = u8::MAX;
static ACTIVE: AtomicU8 = AtomicU8::new(0);
static ACTIVE_ROMAJI: AtomicU8 = AtomicU8::new(NONE);

thread_local! {
    static OVERRIDE: Cell<Option<LatinLayout>> = const { Cell::new(None) };
    static ROMAJI_OVERRIDE: Cell<Option<LatinLayout>> = const { Cell::new(None) };
}

/// QWERTY's character on each key, unshifted, in the order of `rows`.
const QWERTY_ROWS: [&str; 3] = ["qwertyuiop", "asdfghjkl;", "zxcvbnm"];

impl LatinLayout {
    pub const ALL: [LatinLayout; 3] = [LatinLayout::Qwerty, LatinLayout::Colemak, LatinLayout::ColemakDH];

    /// The name settings.json uses.
    pub fn raw(self) -> &'static str {
        match self {
            LatinLayout::Qwerty => "qwerty",
            LatinLayout::Colemak => "colemak",
            LatinLayout::ColemakDH => "colemakDH",
        }
    }

    pub fn from_raw(raw: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|l| l.raw() == raw)
    }

    /// How the settings show it.
    pub fn name(self) -> &'static str {
        match self {
            LatinLayout::Qwerty => "QWERTY",
            LatinLayout::Colemak => "Colemak",
            LatinLayout::ColemakDH => "Colemak-DH",
        }
    }

    fn code(self) -> u8 {
        self as u8
    }

    fn from_code(code: u8) -> Option<Self> {
        Self::ALL.get(code as usize).copied()
    }

    /// What the input method types with: set from the settings when they load.
    pub fn set_active(layout: LatinLayout) {
        ACTIVE.store(layout.code(), Ordering::Relaxed);
    }

    pub fn active() -> LatinLayout {
        Self::from_code(ACTIVE.load(Ordering::Relaxed)).unwrap_or(LatinLayout::Qwerty)
    }

    /// The layout romaji is typed on, when it isn't the English one (`settings.json`'s
    /// `"japaneseLayout"`): English on Colemak, romaji on QWERTY.
    pub fn set_active_romaji(layout: Option<LatinLayout>) {
        ACTIVE_ROMAJI.store(layout.map_or(NONE, LatinLayout::code), Ordering::Relaxed);
    }

    pub fn current() -> LatinLayout {
        OVERRIDE.with(Cell::get).unwrap_or_else(Self::active)
    }

    pub fn romaji() -> LatinLayout {
        ROMAJI_OVERRIDE
            .with(Cell::get)
            .or_else(|| Self::from_code(ACTIVE_ROMAJI.load(Ordering::Relaxed)))
            .unwrap_or_else(Self::current)
    }

    /// For a test (or a tool) that types on another layout without touching the active one.
    pub fn with_override<R>(layout: LatinLayout, f: impl FnOnce() -> R) -> R {
        let old = OVERRIDE.with(|o| o.replace(Some(layout)));
        let result = f();
        OVERRIDE.with(|o| o.set(old));
        result
    }

    pub fn with_romaji_override<R>(layout: LatinLayout, f: impl FnOnce() -> R) -> R {
        let old = ROMAJI_OVERRIDE.with(|o| o.replace(Some(layout)));
        let result = f();
        ROMAJI_OVERRIDE.with(|o| o.set(old));
        result
    }

    /// Keys (the English layout's letters) as romaji's layout spells them: each key's character
    /// on romaji's layout, as 2-set looks through to the key. The same keys when the layouts match.
    pub fn romaji_keys(keys: &str) -> String {
        let english = Self::current();
        let romaji = Self::romaji();
        if romaji == english {
            return keys.to_string();
        }
        keys.chars().map(|c| romaji.character_on_seat(english.seat_of(c))).collect()
    }

    fn rows(self) -> [&'static str; 3] {
        match self {
            LatinLayout::Qwerty => QWERTY_ROWS,
            LatinLayout::Colemak => ["qwfpgjluy;", "arstdhneio", "zxcvbkm"],
            LatinLayout::ColemakDH => ["qwfpbjluy;", "arstgmneio", "zxcdvkh"],
        }
    }

    /// The layout's character on the key with QWERTY's `seat` (unshifted or shifted), e.g. Colemak
    /// types `f` on the e key and `o` on the ; key. Keys the rows don't cover type the same everywhere.
    pub fn character_on_seat(self, seat: char) -> char {
        if self == LatinLayout::Qwerty {
            return seat;
        }
        let Some((row, column, shifted)) = Self::position(seat) else { return seat };
        let ch = self.rows()[row].chars().nth(column).unwrap_or(seat);
        if shifted {
            Self::shift(ch)
        } else {
            ch
        }
    }

    /// The QWERTY key a character of this layout is typed on: the inverse of `character_on_seat`.
    pub fn seat_of(self, character: char) -> char {
        if self == LatinLayout::Qwerty {
            return character;
        }
        let lowered = Self::unshift(character);
        let shifted = lowered != character;
        for (r, row) in self.rows().iter().enumerate() {
            if let Some(c) = row.chars().position(|x| x == lowered) {
                let seat = QWERTY_ROWS[r].chars().nth(c).unwrap_or(character);
                return if shifted { Self::shift(seat) } else { seat };
            }
        }
        character
    }

    /// Whether the key typing `character` is one of QWERTY's letter keys, where 2-set Korean has
    /// its jamo, or romaji's layout types a letter: Colemak's `;` (on the p key, ㅔ) is part of a
    /// word too, and so is QWERTY's `;` when romaji is on Colemak (its o).
    pub fn is_letter_seat(self, character: char) -> bool {
        let seat = self.seat_of(character);
        (seat.is_ascii() && seat.is_alphabetic()) || Self::romaji().character_on_seat(seat).is_alphabetic()
    }

    fn position(seat: char) -> Option<(usize, usize, bool)> {
        let lowered = Self::unshift(seat);
        for (r, row) in QWERTY_ROWS.iter().enumerate() {
            if let Some(c) = row.chars().position(|x| x == lowered) {
                return Some((r, c, lowered != seat));
            }
        }
        None
    }

    fn shift(ch: char) -> char {
        if ch == ';' {
            ':'
        } else {
            ch.to_ascii_uppercase()
        }
    }

    fn unshift(ch: char) -> char {
        if ch == ':' {
            ';'
        } else {
            ch.to_ascii_lowercase()
        }
    }

    /// The character a key types on this layout, from its QWERTY seat (the letter on a US
    /// keyboard, or `;`), with Shift (or Caps Lock, for letters) as given; nil for keys outside
    /// the letter rows.
    pub fn character_for_seat(self, seat: char, shift: bool, caps_lock: bool) -> Option<char> {
        let seat = seat.to_ascii_lowercase();
        Self::position(seat)?;
        let mut ch = self.character_on_seat(seat);
        if shift || (caps_lock && ch.is_alphabetic()) {
            ch = Self::shift(ch);
        }
        Some(ch)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colemak_seats_round_trip() {
        let l = LatinLayout::Colemak;
        assert_eq!(l.character_on_seat('e'), 'f');
        assert_eq!(l.character_on_seat(';'), 'o');
        assert_eq!(l.seat_of('f'), 'e');
        assert_eq!(l.seat_of('O'), ':');
        assert_eq!(l.seat_of('F'), 'E');
        for ch in "qwertyuiopasdfghjklzxcvbnm".chars() {
            assert_eq!(l.seat_of(l.character_on_seat(ch)), ch);
        }
    }
}
