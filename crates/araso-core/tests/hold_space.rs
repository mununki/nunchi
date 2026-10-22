//! Upstream HoldSpaceTests: an app set to hold the word (`hold_space`, for terminals): after Space
//! or a symbol the word stays underlined with that boundary until the next word starts, so a
//! switch key still changes it and Delete takes back only the boundary.

mod common;

use std::rc::Rc;
use std::sync::Arc;

use araso_core::app_policy::{Override, STRICT_THRESHOLD};
use araso_core::plain_field::PlainTextField;
use araso_core::vim::VimSignal;
use araso_core::{text, AppPolicies, DictionaryEngine, InputSession, Key, Language, TextClient};
use common::*;

fn holding(threshold: Option<f64>, hold: bool) -> AppPolicies {
    let mut policies = AppPolicies::default();
    policies.overrides.insert(
        "test.hold".into(),
        Override { hold_space: Some(hold), conversion_threshold: threshold, ..Default::default() },
    );
    policies
}

struct T {
    field: PlainTextField,
    learning: Arc<DictionaryEngine>,
    session: InputSession,
}

impl T {
    fn new() -> Self {
        let learning = dictionary(splitting(shared_engine()), Some(temp_path("hold")));
        let mut session = InputSession::new(learning.clone(), holding(None, true), None, false, false, true);
        session.activate(Some("test.hold"));
        T { field: PlainTextField::new(), learning, session }
    }

    fn type_(&mut self, s: &str) -> Vec<bool> {
        type_script(&mut self.session, &mut self.field, s)
    }

    fn key(&mut self, key: Key) -> bool {
        self.session.handle(key, &mut self.field)
    }

    /// The underlined text, nil when nothing is underlined.
    fn marked(&self) -> Option<String> {
        self.field.marked_text()
    }
}

/// A holding session with Japanese on, and its field.
fn japanese_session(live: bool) -> (InputSession, PlainTextField) {
    let engine = trilingual_dictionary(Some(temp_path("hold-ja")));
    let mut session =
        InputSession::new(engine, holding(None, true), Some(Rc::new(FakeConverter::default())), true, live, true);
    session.activate(Some("test.hold"));
    (session, PlainTextField::new())
}

#[test]
fn space_keeps_the_word_underlined_with_its_space() {
    let mut t = T::new();
    assert_eq!(t.type_("dkssud ").last(), Some(&true));
    assert_eq!(t.field.text, "안녕 ");
    assert_eq!(t.marked().as_deref(), Some("안녕 "));
}

/// A symbol is a boundary like Space: the word stays underlined with it, so it can still be changed.
#[test]
fn a_symbol_keeps_the_word_underlined_with_it() {
    for symbol in [".", ",", "?", "!", "-", "'", ";", ":", ")", "\"", "/"] {
        let mut t = T::new();
        assert_eq!(t.type_(&format!("dkssud{symbol}")).last(), Some(&true), "{symbol}");
        assert_eq!(t.field.text, format!("안녕{symbol}"));
        assert_eq!(t.marked(), Some(format!("안녕{symbol}")));
    }
}

/// A switch key changes the word; the symbol stays as typed, half-width, also for Japanese.
#[test]
fn a_switch_right_after_a_symbol_changes_the_word_and_keeps_the_symbol() {
    let mut t = T::new();
    t.type_("dkssud.");
    assert!(t.key(Key::Cycle));
    assert!(t.field.text == "dkssud." && t.marked().as_deref() == Some("dkssud."));
    assert!(t.key(Key::Cycle));
    assert!(t.field.text == "안녕." && t.marked().as_deref() == Some("안녕."));
}

#[test]
fn delete_takes_back_only_the_symbol_then_a_shift_esc_changes_the_word() {
    let mut t = T::new();
    t.type_("dkssud.");
    assert!(t.key(Key::Delete));
    assert!(t.field.text == "안녕" && t.marked().as_deref() == Some("안녕"));
    assert!(t.key(Key::Cycle));
    assert!(t.field.text == "dkssud" && t.marked().as_deref() == Some("dkssud"));
}

/// One boundary only: the next letter puts the word in with its symbol in one insertion.
#[test]
fn the_next_letter_commits_the_word_with_its_symbol_in_one_insertion() {
    let mut t = T::new();
    t.type_("dkssud.gktpdy");
    assert!(t.field.text == "안녕.하세요" && t.marked().as_deref() == Some("하세요"));
    assert_eq!(t.field.insertions, vec!["안녕."]);
}

#[test]
fn return_commits_the_word_with_its_symbol_and_only_confirms() {
    let mut t = T::new();
    t.type_("dkssud.");
    assert!(t.key(Key::Enter));
    assert!(t.field.text == "안녕." && t.marked().is_none());
    assert!(!t.key(Key::Enter));
}

#[test]
fn a_second_boundary_commits_the_word_with_its_symbol_and_goes_to_the_app() {
    for next in [".", " ", "3", "!"] {
        let mut t = T::new();
        assert_eq!(t.type_(&format!("dkssud.{next}")).last(), Some(&false), "{next}");
        assert!(t.field.text == format!("안녕.{next}") && t.marked().is_none());
        assert_eq!(t.field.insertions, vec!["안녕.".to_string(), next.to_string()]);
    }
}

/// A held symbol ended the word: switched to Japanese, Space commits it instead of converting.
#[test]
fn space_on_a_held_symbol_word_switched_to_japanese_commits_without_converting() {
    let (mut japanese, mut field) = japanese_session(false);
    for ch in "hello".chars() {
        japanese.handle(Key::Letter(ch), &mut field);
    }
    japanese.handle(Key::Printable('.'), &mut field);
    assert_eq!(field.text, "hello.");
    japanese.handle(Key::Cycle, &mut field);
    assert_eq!(field.text, "へっぉ.");
    assert!(!japanese.handle(Key::Printable(' '), &mut field));
    assert!(japanese.conversion().is_none());
    assert!(field.text == "へっぉ." && field.marked.is_none());
}

#[test]
fn the_next_word_commits_the_held_one() {
    let mut t = T::new();
    t.type_("dkssud gktpdy");
    assert_eq!(t.field.text, "안녕 하세요");
    assert_eq!(t.marked().as_deref(), Some("하세요"));
    t.type_(" ");
    assert_eq!(t.marked().as_deref(), Some("하세요 "));
}

#[test]
fn delete_takes_back_only_the_space() {
    let mut t = T::new();
    t.type_("dkssud ");
    assert!(t.key(Key::Delete));
    assert!(t.field.text == "안녕" && t.marked().as_deref() == Some("안녕"));
    // Delete again: the word itself, as while typing.
    assert!(t.key(Key::Delete));
    assert_eq!(t.field.text, "안녀");
}

/// What this is for: Space pressed before looking, then Delete and Shift+Esc.
#[test]
fn delete_then_shift_esc_changes_the_word() {
    let mut t = T::new();
    t.type_("dkssud ");
    t.key(Key::Delete);
    assert!(t.key(Key::Cycle));
    assert!(t.field.text == "dkssud" && t.marked().as_deref() == Some("dkssud"));
    t.type_(" ");
    assert!(t.field.text == "dkssud " && t.marked().as_deref() == Some("dkssud "));
    t.type_("x");
    assert!(t.field.text.starts_with("dkssud "));
    assert_eq!(t.field.marked.map(|m| m.location), Some(text::utf16_len("dkssud ")));
}

#[test]
fn a_switch_right_after_space_changes_the_word_and_keeps_the_space() {
    let mut t = T::new();
    t.type_("dkssud ");
    assert!(t.key(Key::Cycle));
    assert!(t.field.text == "dkssud " && t.marked().as_deref() == Some("dkssud "));
    assert!(t.key(Key::Cycle));
    assert!(t.field.text == "안녕 " && t.marked().as_deref() == Some("안녕 "));
}

/// Esc that isn't one of the app's switch keys finishes the word with its space and goes to the app.
#[test]
fn esc_not_a_switch_key_commits_the_held_word_with_its_space() {
    let mut t = T::new();
    t.type_("dkssud ");
    assert!(!t.key(Key::Escape));
    assert!(t.field.text == "안녕 " && t.marked().is_none());
}

/// What is shown after Space is what would have been committed: the whole word's decision,
/// not the preview of its last keystroke.
#[test]
fn held_word_reads_as_it_would_commit() {
    let mut f = Failures::new();
    for keys in ["go", "so", "did", "hello", "dkssud", "githubdp", "rjtm", "dhsmf", "wkqtm", "gst", "APIfmf"] {
        let mut plain = PlainTextField::new();
        let engine = dictionary(splitting(shared_engine()), Some(temp_path("hold-plain")));
        let mut standard = InputSession::new(engine, AppPolicies::default(), None, false, false, true);
        for ch in keys.chars() {
            standard.handle(Key::Letter(ch), &mut plain);
        }
        standard.handle(Key::Printable(' '), &mut plain); // the app would type the space
        let mut t = T::new();
        t.type_(&format!("{keys} "));
        f.check(t.field.text == format!("{} ", plain.text), || format!("{keys}: {:?} vs {:?}", t.field.text, plain.text));
    }
    f.assert_none();
}

#[test]
fn return_commits_the_held_word_with_its_space_and_only_confirms() {
    let mut t = T::new();
    t.type_("dkssud ");
    assert!(t.key(Key::Enter));
    assert!(t.field.text == "안녕 " && t.marked().is_none());
    assert!(!t.key(Key::Enter));
}

#[test]
fn other_keys_commit_the_held_word_with_its_space() {
    let mut t = T::new();
    t.type_("dkssud ");
    assert!(!t.key(Key::Other));
    assert!(t.field.text == "안녕 " && t.marked().is_none());
}

/// Ctrl going down puts the held word in before the shortcut: an app pasting on Ctrl+V without
/// showing the input method the key would paste over the underlined word.
#[test]
fn command_going_down_commits_the_held_word_so_a_paste_lands_after_it() {
    let mut t = T::new();
    t.type_("dkssud ");
    t.session.command_pressed(&mut t.field);
    assert!(t.field.text == "안녕 " && t.marked().is_none());
    assert_eq!(t.field.insertions, vec!["안녕 "]);
    t.field.insert_text("hello", None); // the app's paste
    assert_eq!(t.field.text, "안녕 hello");
}

#[test]
fn command_going_down_commits_the_word_with_its_symbol() {
    for symbol in [".", "!"] {
        let mut t = T::new();
        t.type_(&format!("dkssud{symbol}"));
        t.session.command_pressed(&mut t.field);
        assert!(t.field.text == format!("안녕{symbol}") && t.marked().is_none());
    }
}

/// A word without a boundary keeps composing.
#[test]
fn command_going_down_leaves_a_word_without_a_boundary_composing() {
    let mut t = T::new();
    t.type_("dkss");
    t.session.command_pressed(&mut t.field);
    assert!(t.marked().is_some() && t.field.insertions.is_empty());
    t.type_("ud ");
    assert!(t.field.text == "안녕 " && t.marked().as_deref() == Some("안녕 "));
}

/// A paste reaches the input method only as Ctrl going down, so the pasted text is read
/// from the field: a pasted URL makes the next segment typed English.
#[test]
fn a_url_pasted_after_command_counts() {
    let mut t = T::new();
    t.type_("dkssud ");
    t.session.command_pressed(&mut t.field);
    t.field.insert_text("https://x.io/", None); // the app's paste
    t.type_("dl ");
    assert_eq!(t.field.text, "안녕 https://x.io/dl ");
}

#[test]
fn a_switch_key_after_command_leaves_the_word_as_it_went() {
    let mut t = T::new();
    t.type_("dkssud ");
    t.session.command_pressed(&mut t.field);
    t.key(Key::Cycle);
    assert!(t.field.text == "안녕 " && t.marked().is_none());
}

#[test]
fn command_going_down_with_nothing_typed_does_nothing() {
    let mut t = T::new();
    t.session.command_pressed(&mut t.field);
    assert!(t.field.text.is_empty() && t.field.insertions.is_empty());
}

#[test]
fn a_second_space_and_punctuation_go_to_the_app() {
    let mut t = T::new();
    t.type_("dkssud  ");
    assert!(t.field.text == "안녕  " && t.marked().is_none());
    t.type_("gktpdy .");
    assert!(t.field.text == "안녕  하세요 ." && t.marked().is_none());
}

#[test]
fn losing_focus_commits_the_held_word_with_its_space() {
    let mut t = T::new();
    t.type_("dkssud ");
    t.session.commit(Some(&mut t.field));
    assert!(t.field.text == "안녕 " && t.marked().is_none());
}

#[test]
fn esc_while_held_is_learned_when_the_word_goes_in() {
    let mut t = T::new();
    t.type_("hello ");
    t.key(Key::Cycle);
    assert_eq!(t.field.text, "ㅗ디ㅣㅐ ");
    assert!(t.learning.pending().is_empty());
    t.type_("x");
    assert_eq!(t.learning.pending(), [("hello".to_string(), Language::Korean)].into_iter().collect());
}

/// A held word is still the word being typed: switched to Japanese, Space converts it.
#[test]
fn space_on_a_held_word_switched_to_japanese_converts() {
    let (mut japanese, mut field) = japanese_session(false);
    for ch in "hello".chars() {
        japanese.handle(Key::Letter(ch), &mut field);
    }
    japanese.handle(Key::Printable(' '), &mut field);
    assert_eq!(field.text, "hello ");
    japanese.handle(Key::Cycle, &mut field);
    assert_eq!(field.text, "へっぉ ");
    assert!(japanese.handle(Key::Printable(' '), &mut field));
    assert!(japanese.conversion().is_some());
    assert_eq!(field.text, "へっぉ");
}

/// Holding changes when a word goes in, never what goes in: in a Strict app a line comes out
/// the same with or without it.
#[test]
fn strict_app_types_the_same_line() {
    let run = |keys: &str, hold: bool| {
        let mut field = PlainTextField::new();
        let engine = dictionary(splitting(shared_engine()), Some(temp_path("hold-strict")));
        let mut session = InputSession::new(engine, holding(Some(STRICT_THRESHOLD), hold), None, false, false, true);
        session.activate(Some("test.hold"));
        type_script(&mut session, &mut field, keys);
        session.commit(Some(&mut field));
        field.text
    };
    let mut f = Failures::new();
    for keys in [
        "gst ls", "cd src", "git status", "rm -rf", "ls -al", "gs ", "vi ", "tmux a", "brew install", "npm run dev",
        "dkssud gktpdy", "rkskek ", "echo \"hi.\"", "git commit -m \"fix.\"", "ls -al.", "a/b/c", "dkssud, gktpdy!", "x; y",
    ] {
        let (held, plain) = (run(keys, true), run(keys, false));
        f.check(held == plain, || format!("{keys}: {held:?} held, {plain:?} not"));
    }
    f.assert_none();
}

#[test]
fn pausing_commits_the_held_word_with_its_space() {
    let mut t = T::new();
    t.type_("dkssud ");
    t.session.toggle_pause(Some(&mut t.field));
    assert!(t.field.text == "안녕 " && t.marked().is_none());
    // Paused, keys go to the app as typed.
    assert_eq!(t.type_("gk"), vec![false, false]);
    assert_eq!(t.field.text, "안녕 gk");
}

#[test]
fn vim_leaving_insert_mode_commits_the_held_word_with_its_space() {
    let mut t = T::new();
    t.type_("dkssud ");
    t.session.vim_changed(VimSignal::Normal, Some(&mut t.field));
    assert!(t.field.text == "안녕 " && t.marked().is_none());
}

/// Shift+Space on a held word is a second Space: the word goes in with its space, the app types another.
#[test]
fn shift_space_on_a_held_word_is_a_second_space() {
    let mut t = T::new();
    t.type_("dkssud ");
    if !t.key(Key::ShiftSpace) {
        t.field.insert_text(" ", None);
    }
    assert!(t.field.text == "안녕  " && t.marked().is_none());
}

#[test]
fn a_digit_commits_the_held_word_and_goes_to_the_app() {
    let mut t = T::new();
    t.type_("dkssud 3");
    assert!(t.field.text == "안녕 3" && t.marked().is_none());
}

/// With live conversion on too, Space on a held word switched to Japanese converts, without the held space.
#[test]
fn space_on_a_held_word_switched_to_japanese_converts_with_live_conversion() {
    let (mut live, mut field) = japanese_session(true);
    for ch in "hello".chars() {
        live.handle(Key::Letter(ch), &mut field);
    }
    live.handle(Key::Printable(' '), &mut field);
    live.handle(Key::Cycle, &mut field);
    assert!(field.text.ends_with(' '));
    assert!(live.handle(Key::Printable(' '), &mut field));
    assert!(live.conversion().is_some());
    assert!(!field.text.ends_with(' '));
}

#[test]
fn other_apps_finish_the_word_on_space() {
    let mut t = T::new();
    t.session.activate(Some("editor.exe"));
    assert_eq!(t.type_("dkssud ").last(), Some(&false));
    assert!(t.field.text == "안녕 " && t.marked().is_none());
}
