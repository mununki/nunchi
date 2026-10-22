//! Upstream URLRunTests and CorrectionTests (with the fake converter instead of AzooKey).

mod common;

use std::rc::Rc;
use std::sync::Arc;

use araso_core::plain_field::PlainTextField;
use araso_core::user_dictionary::{Entry, Source};
use araso_core::{DecisionContext, DecisionEngine, DictionaryEngine, InputSession, Key, Language, TextClient};
use common::*;

/// After a URL or path marker (`://`, `www.`, `~/`, `x@`…), segments go in as typed; a lone `.`
/// or `/` is no marker.
struct Url {
    field: PlainTextField,
    learning: Arc<DictionaryEngine>,
    session: InputSession,
    japanese: InputSession,
}

impl Url {
    fn new() -> Self {
        let learning = trilingual_dictionary(Some(temp_path("url")));
        let session = InputSession::new(learning.clone(), esc_switches(), None, false, false, true);
        let japanese = InputSession::new(learning.clone(), esc_switches(), None, true, false, true);
        Url { field: PlainTextField::new(), learning, session, japanese }
    }

    fn type_(&mut self, s: &str) {
        type_script(&mut self.session, &mut self.field, s);
    }

    fn type_japanese(&mut self, s: &str) {
        type_script(&mut self.japanese, &mut self.field, s);
    }
}

#[test]
fn segments_after_a_marker_stay_as_typed() {
    let mut f = Failures::new();
    for keys in [
        "https://example.com/dl ", "https://a.io/?q=dl&p=sk ", "cd ~/src/rk ", "www.ak.com/bb ", "/usr/lib/dl ",
        "./ak/sk ", "http://localhost:3000/dl ", "localhost:3000/dl ",
    ] {
        let mut t = Url::new();
        t.type_(keys);
        f.check(t.field.text == keys, || format!("{keys:?} → {:?}", t.field.text));
    }
    f.assert_none();
}

#[test]
fn email_after_the_at() {
    let mut t = Url::new();
    t.type_("x@gmail.com ");
    assert!(t.field.text.ends_with("@gmail.com "), "{}", t.field.text);
}

#[test]
fn no_marker_no_change() {
    let mut f = Failures::new();
    for (keys, text) in [
        ("@dlrjs ", "@이건 "), ("ska/du ", "남/여 "), ("TV/fkeldh ", "TV/라디오 "), ("L.A.fh ", "L.A.로 "),
        ("1:1ansdml ", "1:1문의 "), ("/sk ", "/나 "),
    ] {
        let mut t = Url::new();
        t.type_(keys);
        f.check(t.field.text == text, || format!("{keys:?} → {:?}, not {text:?}", t.field.text));
    }
    f.assert_none();
}

#[test]
fn japanese_on_keeps_the_url_half_width() {
    let mut t = Url::new();
    t.type_japanese("https://x.io/ho.da ");
    assert_eq!(t.field.text, "https://x.io/ho.da ");
}

#[test]
fn kana_before_the_at_keeps_the_rest_half_width() {
    let mut t = Url::new();
    t.type_japanese("ho@gmail.com ");
    assert_eq!(t.field.text, "ほ@gmail.com ");
}

#[test]
fn japanese_punctuation_without_a_marker() {
    let mut t = Url::new();
    t.type_japanese("kyouha.");
    assert_eq!(t.field.text, "きょうは。");
}

#[test]
fn the_dictionary_is_not_asked() {
    let mut t = Url::new();
    t.type_("so⎋ x so⎋ ");
    assert_eq!(t.learning.learned().get("so"), Some(&Language::Korean));
    t.field = PlainTextField::new();
    t.session.commit_for_new_context(Some(&mut t.field));
    t.type_("https://x.io/so ");
    assert_eq!(t.field.text, "https://x.io/so ");
}

#[test]
fn a_switched_segment_is_not_learned() {
    let mut t = Url::new();
    t.type_("https://x.io/dkssud");
    for _ in 0..4 {
        if t.field.text.ends_with("안녕") {
            break;
        }
        t.type_("⎋");
    }
    assert_eq!(t.field.text, "https://x.io/안녕");
    t.type_(" ");
    t.type_("https://x.io/dkssud⎋ ");
    assert!(!t.learning.learned().contains_key("dkssud"));
    assert!(!t.learning.pending().contains_key("dkssud"));
}

#[test]
fn space_ends_the_run() {
    let mut t = Url::new();
    t.type_("https://x.io/a dkssud ");
    assert_eq!(t.field.text, "https://x.io/a 안녕 ");
    assert_eq!(t.session.last_trace().and_then(|t| t.previous), Some(Language::English));
}

#[test]
fn a_new_context_ends_the_run() {
    let mut t = Url::new();
    t.type_("https://x.io/");
    t.field.cursor = 0; // a click at the start of the field
    t.session.commit_for_new_context(Some(&mut t.field));
    t.type_("dkssud ");
    assert!(t.field.text.starts_with("안녕 "), "{}", t.field.text);
}

#[test]
fn a_pasted_url_counts() {
    let mut t = Url::new();
    let _ = t.session.handle(Key::Other, &mut t.field); // Ctrl+V
    t.field.insert_text("https://x.io/", None);
    t.type_("dl ");
    assert_eq!(t.field.text, "https://x.io/dl ");
}

#[test]
fn a_field_that_cant_be_read_only_has_what_was_typed() {
    let mut t = Url::new();
    t.field.unreadable = true;
    let _ = t.session.handle(Key::Other, &mut t.field);
    t.field.insert_text("https://x.io/", None);
    t.type_("dkssud ");
    assert_eq!(t.field.text, "https://x.io/안녕 ");
}

// CorrectionTests: what counts as a correction and what gets learned, Japanese on.

struct Correction {
    field: PlainTextField,
    learning: Arc<DictionaryEngine>,
    session: InputSession,
}

impl Correction {
    fn new() -> Self {
        let learning = trilingual_dictionary(Some(temp_path("corr")));
        let mut session =
            InputSession::new(learning.clone(), esc_switches(), Some(Rc::new(FakeConverter::default())), true, false, true);
        session.activate(Some("notepad.exe"));
        Correction { field: PlainTextField::new(), learning, session }
    }

    /// Letters as themselves without the app typing passed-on letters, as upstream.
    fn type_(&mut self, script: &str) {
        for ch in script.chars() {
            match ch {
                '⏎' => {
                    if !self.session.handle(Key::Enter, &mut self.field) {
                        self.field.insert_text("\n", None);
                    }
                }
                '⎋' => {
                    self.session.handle(Key::Escape, &mut self.field);
                }
                '←' => {
                    self.session.handle(Key::Left, &mut self.field);
                }
                c if c.is_alphabetic() => {
                    self.session.handle(Key::Letter(c), &mut self.field);
                }
                c => {
                    if !self.session.handle(Key::Printable(c), &mut self.field) {
                        self.field.insert_text(&c.to_string(), None);
                    }
                }
            }
        }
    }
}

/// A word learned as mixed (gitlabdp → gitlab에) renders through the base engine.
#[test]
fn learned_mixed_word_renders_without_recursing() {
    let mut t = Correction::new();
    t.learning.learn("gitlabdp", Language::Mixed, Some(Language::English));
    t.learning.learn("gitlabdp", Language::Mixed, Some(Language::English));
    assert_eq!(t.learning.learned().get("gitlabdp"), Some(&Language::Mixed));
    let d = t.learning.decide("gitlabdp", &DecisionContext::default());
    assert_eq!(d.language, Language::Mixed);
    assert_eq!(d.output, "gitlab에");
    t.type_("gitlabdp ");
    assert_eq!(t.field.text, "gitlab에 ");
}

/// The engine is unsure about "so" (English, 0.55): an Esc to 내 must still count.
#[test]
fn esc_while_typing_is_learned_the_second_time() {
    let mut t = Correction::new();
    t.type_("so⎋ ");
    assert_eq!(t.field.text, "내 ");
    t.type_("x so⎋ ");
    assert_eq!(t.learning.learned().get("so"), Some(&Language::Korean));
    t.type_("x so ");
    assert!(t.field.text.ends_with("내 "), "{}", t.field.text);
}

/// One correction is weak: it turns a close call but isn't learned, and typing the word as
/// corrected keeps it.
#[test]
fn one_correction_turns_a_close_call() {
    let mut t = Correction::new();
    t.type_("so⎋ ");
    t.session.activate(Some("notepad.exe"));
    t.type_("so ");
    assert_eq!(t.field.text, "내 내 ");
    assert_eq!(t.learning.pending(), [("so".to_string(), Language::Korean)].into_iter().collect());
    assert!(t.learning.learned().is_empty());
    // Switched back where the nudge turned it: forgotten, not a weak entry the other way.
    t.type_("⎋⎋x "); // 내 → そ → so
    assert!(t.field.text.ends_with("내 so x "), "{}", t.field.text);
    assert!(t.learning.pending().is_empty() && t.learning.learned().is_empty());
}

/// A word learned as English splits off a particle typed after it: ghostty에, not ghost쇼에.
#[test]
fn learned_word_with_particle() {
    let mut t = Correction::new();
    t.type_("ghosttydp ");
    assert_eq!(t.field.text, "ghost쇼에 ");
    t.learning.learn("ghostty", Language::English, Some(Language::Korean));
    t.learning.learn("ghostty", Language::English, Some(Language::Korean));
    t.type_("ghosttydp ");
    assert_eq!(t.field.text, "ghost쇼에 ghostty에 ");
}

/// A word added for one app is typed so there only.
#[test]
fn word_added_for_an_app() {
    let mut t = Correction::new();
    t.learning.add(Entry::new("rt", Some(Language::English), None, Source::User, None, Some(vec!["notepad.exe".into()]), None));
    t.type_("rt ");
    t.session.activate(Some("wordpad.exe"));
    t.type_("rt ");
    assert_eq!(t.field.text, "rt ㄱㅅ ");
}

/// A switch after a commit passing through other readings records only where it ended up.
#[test]
fn undo_detour_is_one_correction() {
    let mut t = Correction::new();
    // ozzy: English → Korean (ㅐㅋ쿄) → Japanese.
    t.type_("ozzy ⎋⎋");
    t.type_("x "); // any key ends the undo window
    assert_eq!(t.learning.pending().get("ozzy"), Some(&Language::Japanese));
    t.type_("ozzy ⎋⎋");
    t.type_("x ");
    assert_eq!(t.learning.learned().get("ozzy"), Some(&Language::Japanese));
}

/// Back to kana, then Esc to English: no apostrophe the user never typed.
#[test]
fn no_apostrophe_after_back_to_kana() {
    let mut t = Correction::new();
    t.type_("henkan ⎋⎋ ");
    assert_eq!(t.field.text, "henkan ");
}

/// Committing a clause and going back to kana for the rest isn't a correction.
#[test]
fn back_to_kana_on_the_rest_is_not_a_correction() {
    let mut t = Correction::new();
    t.type_("kyouhaiitenkidesune ");
    t.type_("←←←←←←←←⏎"); // 今日は, the rest keeps converting
    t.type_("⎋⏎");
    // The fake converter has no kanji for きょうは alone (AzooKey upstream gives 今日は).
    assert!(t.field.text.starts_with("きょうは"), "{}", t.field.text);
    assert!(t.learning.pending().is_empty());
    assert!(t.learning.learned().is_empty());
}

/// Contractions and hyphenated words stay English with Japanese on.
#[test]
fn apostrophes_and_hyphens_stay_english() {
    let mut f = Failures::new();
    for word in ["don't ", "aren't ", "haven't ", "won't ", "weren't ", "to-do ", "e-mail ", "can't "] {
        let mut t = Correction::new();
        t.type_(word);
        f.check(t.field.text == word, || format!("{word:?} → {:?}", t.field.text));
    }
    f.assert_none();
}

/// Converting a word learned as Japanese again doesn't erase it.
#[test]
fn learned_word_survives_being_used() {
    let mut t = Correction::new();
    t.learning.learn("ozzy", Language::Japanese, Some(Language::English));
    t.learning.learn("ozzy", Language::Japanese, Some(Language::English));
    assert_eq!(t.learning.learned().get("ozzy"), Some(&Language::Japanese));
    t.type_("ozzy ⎋ "); // convert, back to kana, convert again
    t.type_("⏎");
    assert_eq!(t.learning.learned().get("ozzy"), Some(&Language::Japanese));
}
