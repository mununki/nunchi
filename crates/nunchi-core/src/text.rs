//! String helpers in Swift's terms: counts and slices by character, not byte.

/// Number of characters.
pub fn len(s: &str) -> usize {
    s.chars().count()
}

/// The first `n` characters.
pub fn prefix(s: &str, n: usize) -> &str {
    match s.char_indices().nth(n) {
        Some((i, _)) => &s[..i],
        None => s,
    }
}

/// Everything after the first `n` characters.
pub fn drop_first(s: &str, n: usize) -> &str {
    match s.char_indices().nth(n) {
        Some((i, _)) => &s[i..],
        None => "",
    }
}

/// Everything but the last `n` characters.
pub fn drop_last(s: &str, n: usize) -> &str {
    let count = len(s);
    if n >= count {
        ""
    } else {
        prefix(s, count - n)
    }
}

/// The length in UTF-16 code units, the unit text ranges are measured in (as NSString and TSF).
pub fn utf16_len(s: &str) -> usize {
    s.encode_utf16().count()
}

pub fn first(s: &str) -> Option<char> {
    s.chars().next()
}

pub fn last(s: &str) -> Option<char> {
    s.chars().next_back()
}

pub fn pop(s: &mut String) {
    s.pop();
}

/// Swift's `Character.isLetter` for what Nunchi meets: any alphabetic character.
pub fn is_letter(c: char) -> bool {
    c.is_alphabetic()
}

pub fn is_ascii_letter(c: char) -> bool {
    c.is_ascii_alphabetic()
}

/// Swift's `Character.isNumber`.
pub fn is_number(c: char) -> bool {
    c.is_numeric()
}

pub fn is_upper(c: char) -> bool {
    c.is_uppercase()
}

pub fn is_lower(c: char) -> bool {
    c.is_lowercase()
}

/// A Hangul syllable block (가…힣).
pub fn is_hangul_syllable(c: char) -> bool {
    ('\u{AC00}'..='\u{D7A3}').contains(&c)
}

/// The text from `range` (UTF-16 units) of `s`, or nil when the range runs past it.
pub fn utf16_substring(s: &str, location: usize, length: usize) -> Option<String> {
    let units: Vec<u16> = s.encode_utf16().collect();
    if location + length > units.len() {
        return None;
    }
    String::from_utf16(&units[location..location + length]).ok()
}

/// `s` with `range` (UTF-16 units) replaced by `with`.
pub fn utf16_replace(s: &str, location: usize, length: usize, with: &str) -> String {
    let units: Vec<u16> = s.encode_utf16().collect();
    let end = (location + length).min(units.len());
    let start = location.min(end);
    let mut out: Vec<u16> = units[..start].to_vec();
    out.extend(with.encode_utf16());
    out.extend_from_slice(&units[end..]);
    String::from_utf16_lossy(&out)
}
