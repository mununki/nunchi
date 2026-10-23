//! Upstream InputSessionTests and MixedWordSessionTests.

mod common;

use std::sync::Arc;

use nunchi_core::plain_field::PlainTextField;
use nunchi_core::{text, AppPolicies, DecisionEngine, InputSession, Key, SwitchKey, TextClient, TextRange};
use common::*;

struct T {
    field: PlainTextField,
    session: InputSession,
}

impl T {
    fn new() -> Self {
        let engine = dictionary(shared_engine(), Some(temp_path("session")));
        T { field: PlainTextField::new(), session: InputSession::new(engine, esc_switches(), None, false, false, true) }
    }

    fn with(session: InputSession) -> Self {
        T { field: PlainTextField::new(), session }
    }

    fn type_(&mut self, s: &str) -> Vec<bool> {
        type_script(&mut self.session, &mut self.field, s)
    }

    fn key(&mut self, key: Key) -> bool {
        self.session.handle(key, &mut self.field)
    }

    /// The app's own Delete: the character before the caret.
    fn app_delete(&mut self) {
        if !self.key(Key::Delete) {
            let cursor = self.field.cursor;
            self.field.insert_text("", Some(TextRange::new(cursor - 1, 1)));
        }
    }
}

fn session(japanese: bool) -> InputSession {
    let base: Arc<dyn DecisionEngine> = if japanese { splitting(trilingual()) } else { splitting(shared_engine()) };
    let engine = Arc::new(nunchi_core::DictionaryEngine::new(
        base,
        nunchi_core::EnglishModel::shared(),
        nunchi_core::KoreanModel::new(),
        japanese,
        Some(temp_path("mixed")),
        None,
    ));
    InputSession::new(engine, esc_switches(), None, japanese, false, true)
}

#[test]
fn types_mixed_sentence() {
    let mut t = T::new();
    t.type_("dkssudgktpdy hello ");
    assert_eq!(t.field.text, "안녕하세요 hello ");
}

#[test]
fn esc_after_commit_flips_the_word_and_learns_it() {
    let mut t = T::new();
    t.type_("hello ");
    assert_eq!(t.field.text, "hello ");
    assert!(t.key(Key::Escape));
    assert_eq!(t.field.text, "ㅗ디ㅣㅐ ");
    assert_eq!(t.field.cursor, text::utf16_len("ㅗ디ㅣㅐ "));
    // Esc again flips back.
    assert!(t.key(Key::Escape));
    assert_eq!(t.field.text, "hello ");
    // Flipping back to the engine's own choice clears the learned override.
    t.type_("hello ");
    assert_eq!(t.field.text, "hello hello ");
}

/// One Esc changes only that word (a habitual Esc shouldn't stick); the same
/// correction a second time is remembered.
#[test]
fn flipped_choice_is_remembered_the_second_time() {
    let mut t = T::new();
    t.type_("rjtm ");
    let first = t.field.text.clone();
    t.key(Key::Escape);
    let flipped = t.field.text.clone();
    assert_ne!(flipped, first);
    t.type_("rjtm ");
    assert_eq!(t.field.text, format!("{flipped}{first}"));
    t.key(Key::Escape);
    assert_eq!(t.field.text, format!("{flipped}{flipped}"));
    t.type_("rjtm ");
    assert_eq!(t.field.text, format!("{flipped}{flipped}{flipped}"));
}

#[test]
fn flip_only_applies_to_the_word_before_the_caret() {
    let mut t = T::new();
    t.type_("dkssud ");
    t.field.cursor = 0; // user clicked elsewhere
    assert!(!t.key(Key::Escape));
    assert_eq!(t.field.text, "안녕 ");
}

#[test]
fn esc_passes_through_when_text_cannot_be_verified() {
    let mut t = T::new();
    t.field.unreadable = true;
    t.type_("dkssud ");
    assert!(!t.key(Key::Escape));
    assert_eq!(t.field.text, "안녕 ");
}

#[test]
fn any_other_key_ends_the_flip_window() {
    let mut t = T::new();
    t.type_("dkssud ");
    assert!(!t.key(Key::Other));
    assert!(!t.key(Key::Escape));
    assert_eq!(t.field.text, "안녕 ");
}

/// Space before looking, then Delete and Esc: the word still changes.
#[test]
fn delete_after_commit_keeps_the_word_changeable() {
    let mut t = T::new();
    t.type_("dkssud ");
    t.app_delete();
    assert_eq!(t.field.text, "안녕");
    assert!(t.key(Key::Escape));
    assert!(t.field.text == "dkssud" && t.field.cursor == 6);
    t.type_(" ");
    assert_eq!(t.field.text, "dkssud ");
}

#[test]
fn delete_after_punctuation_keeps_the_word_changeable() {
    let mut t = T::new();
    t.type_("hello.");
    t.app_delete();
    assert!(t.key(Key::Cycle));
    assert_eq!(t.field.text, "ㅗ디ㅣㅐ");
}

/// Only the boundary: a second Delete takes a letter of the word and ends the chance.
#[test]
fn second_delete_ends_the_flip_window() {
    let mut t = T::new();
    t.type_("dkssud ");
    t.app_delete();
    t.app_delete();
    assert_eq!(t.field.text, "안");
    assert!(!t.key(Key::Escape));
    assert_eq!(t.field.text, "안");
}

/// Shift+Esc is Nunchi's key: when there is nothing to change it is dropped, not sent to the
/// app as an Esc.
#[test]
fn shift_esc_with_nothing_to_change_never_reaches_the_app() {
    let mut t = T::new();
    assert!(t.key(Key::Cycle));
    t.field.unreadable = true;
    t.type_("dkssud ");
    assert!(t.key(Key::Cycle));
    assert_eq!(t.field.text, "안녕 ");
    assert!(!t.key(Key::Escape)); // plain Esc still goes on
}

#[test]
fn esc_while_composing_flips_the_preview() {
    let mut t = T::new();
    t.type_("dkssud");
    assert_eq!(t.field.text, "안녕");
    t.key(Key::Escape);
    assert_eq!(t.field.text, "dkssud");
    t.type_(" ");
    assert_eq!(t.field.text, "dkssud ");
}

/// A consonant that joins the syllable before as its final is the next one's first on its
/// way: 소개 + ㅎ shows 소갷, not throg, while 소개하는 is typed.
#[test]
fn preview_keeps_korean_through_a_pending_final() {
    let mut t = T::new();
    let mut shown = Vec::new();
    for ch in "throgksms".chars() {
        t.type_(&ch.to_string());
        shown.push(t.field.text.clone());
    }
    assert!(shown.iter().skip(3).all(|s| s.chars().all(|c| !c.is_ascii())), "{shown:?}");
    assert_eq!(shown.last().unwrap(), "소개하는");
}

#[test]
fn delete_removes_keystrokes() {
    let mut t = T::new();
    t.type_("dkssud");
    t.key(Key::Delete);
    assert_eq!(t.field.text, "안녀");
}

/// As the system's 2-set input: the syllable being typed goes jamo by jamo, earlier ones whole.
#[test]
fn delete_korean_like_the_system() {
    let mut t = T::new();
    t.type_("gksrnr");
    assert_eq!(t.field.text, "한국");
    for expected in ["한구", "한ㄱ", "한", ""] {
        t.key(Key::Delete);
        assert_eq!(t.field.text, expected);
    }
    t.type_("dkssudgktpdy");
    for expected in ["안녕하세ㅇ", "안녕하세", "안녕하", "안녕"] {
        t.key(Key::Delete);
        assert_eq!(t.field.text, expected);
    }
    t.type_("gk");
    assert_eq!(t.field.text, "안녕하");
    t.key(Key::Delete);
    assert_eq!(t.field.text, "안녕ㅎ");
}

/// After a Delete the syllable being typed stays apart from the ones before: 세요 → 세ㅇ → 세유.
#[test]
fn retyping_after_delete_keeps_syllables_apart() {
    let mut t = T::new();
    t.type_("dkssudgktpdy");
    t.key(Key::Delete);
    t.type_("b ");
    assert_eq!(t.field.text, "안녕하세유 ");
}

#[test]
fn delete_keeps_the_language_shown() {
    let mut t = T::new();
    t.type_("gksrnr");
    t.key(Key::Delete);
    t.key(Key::Delete);
    assert_eq!(t.field.text, "한ㄱ");
    t.type_(" ");
    assert_eq!(t.field.text, "한ㄱ ");
}

#[test]
fn delete_english_key_by_key() {
    let mut t = T::new();
    t.type_("hello");
    t.key(Key::Delete);
    assert_eq!(t.field.text, "hell");
}

// MixedWordSessionTests

/// The English + Korean word split is what gets typed, not only what the engine decides.
#[test]
fn types_english_then_korean_word() {
    let mut t = T::with(session(false));
    t.type_("hellodnjfem githubtjqj ");
    assert_eq!(t.field.text, "hello월드 github서버 ");
}

/// Return on an underlined word only confirms it; with nothing underlined it is the app's.
#[test]
fn return_confirms_then_goes_to_the_app() {
    let mut t = T::with(session(false));
    t.type_("dkssud");
    assert!(t.key(Key::Enter));
    assert!(t.field.text == "안녕" && t.field.marked.is_none());
    assert!(!t.key(Key::Enter));
}

/// By default Esc isn't a switch key: the word goes in as shown and the app gets the Esc,
/// also right after a commit. The chosen keys (`Cycle`) still switch.
#[test]
fn esc_goes_to_the_app_unless_chosen() {
    let engine = dictionary(shared_engine(), Some(temp_path("esc")));
    let mut t = T::with(InputSession::new(engine, AppPolicies::default(), None, false, false, true));
    assert_eq!(
        t.session.policy().switch_keys,
        [SwitchKey::HangulKey, SwitchKey::RightAlt, SwitchKey::ShiftEscape].into_iter().collect()
    );
    t.type_("dkssud");
    assert!(!t.key(Key::Escape));
    assert!(t.field.text == "안녕" && t.field.marked.is_none());
    t.type_(" ");
    assert!(!t.key(Key::Escape));
    assert_eq!(t.field.text, "안녕 ");
    assert!(t.key(Key::Cycle)); // nothing to change: dropped
    t.type_("dkssud ");
    assert!(t.key(Key::Cycle));
    assert_eq!(t.field.text, "안녕 dkssud ");
}

/// With the setting off, Return confirms the word and the app gets it too.
#[test]
fn return_goes_on_when_it_doesnt_only_confirm() {
    let engine = dictionary(shared_engine(), Some(temp_path("return")));
    let mut t = T::with(InputSession::new(engine, AppPolicies::default(), None, false, false, false));
    t.type_("dkssud");
    assert!(!t.key(Key::Enter));
    assert!(t.field.text == "안녕" && t.field.marked.is_none());
}

/// Esc cycles back to the engine's own split (x에, hello월드), which the particle split alone
/// doesn't find, and skips the garbled readings while it is there (ㅌ에, ㅗ디ㅣㅐ월드).
#[test]
fn esc_cycles_back_to_the_engines_split() {
    for (keys, split) in [("xdp", "x에"), ("hellodnjfem", "hello월드")] {
        let mut t = T::with(session(true));
        t.type_(keys);
        assert_eq!(t.field.text, split);
        let mut seen = Vec::new();
        for _ in 0..4 {
            t.key(Key::Escape);
            seen.push(t.field.text.clone());
        }
        assert_eq!(seen, vec![keys, split, keys, split]);
    }
}

#[test]
fn types_single_letter_with_particle() {
    let mut t = T::with(session(false));
    t.type_("xdp ndl Xfmf Xeh ");
    assert_eq!(t.field.text, "x에 n이 X를 X도 ");
}

/// Short English ending in the keys of 도 or 나 stays English.
#[test]
fn keeps_short_english_ending_in_eh_or_sk() {
    let mut t = T::with(session(false));
    t.type_("heh meh tsk ");
    assert_eq!(t.field.text, "heh meh tsk ");
}

/// Korean then romaji with no space: the Korean goes in, the rest is a Japanese word.
#[test]
fn types_korean_then_japanese_word() {
    let mut t = T::with(session(true));
    t.type_("qusruddlfurrireki dlfurrireki ");
    assert_eq!(t.field.text, "변경이력りれき 이력りれき ");
}

/// A Japanese word with a Korean particle: the word as Japanese, the particle as Korean.
#[test]
fn types_japanese_word_then_particle() {
    let mut t = T::with(session(true));
    t.type_("duwjsgl rirekifmf rirekidptj ");
    assert_eq!(t.field.text, "여전히 りれき를 りれき에서 ");
}

/// A Japanese phrase with a Korean word straight after it.
#[test]
fn types_japanese_then_korean_word() {
    let mut t = T::with(session(true));
    t.type_("kyouhaehtlfkrdms ");
    assert_eq!(t.field.text, "きょうは도시락은 ");
}

fn each_typed(japanese: bool, cases: &[(&str, &str)]) {
    let mut f = Failures::new();
    for (keys, expected) in cases {
        let mut t = T::with(session(japanese));
        t.type_(&format!("{keys} "));
        f.check(t.field.text == format!("{expected} "), || format!("{keys} → {:?}, not {expected}", t.field.text));
    }
    f.assert_none();
}

/// An English word and a particle whose keys also all make kana (redde is れっで).
#[test]
fn types_english_word_spelled_in_kana_with_particle() {
    each_typed(
        true,
        &[("redde", "redで"), ("bugga", "bugが"), ("loginde", "loginで"), ("filede", "fileで"), ("linuxni", "linuxに"),
          ("vimmade", "vimまで"), ("homede", "homeで")],
    );
}

/// Japanese that an English word and a particle could also spell isn't split (kit + と).
#[test]
fn keeps_japanese_that_spells_english_with_particle() {
    each_typed(
        true,
        &[("kitto", "きっと"), ("densha", "でんしゃ"), ("sitede", "してで"), ("kyouha", "きょうは"), ("Kyouha", "Kyouha"),
          ("nomede", "のめで")],
    );
}

/// Acronyms and names typed with their capitals take a Japanese particle too.
#[test]
fn types_capitalized_word_with_particle() {
    each_typed(
        true,
        &[("APIde", "APIで"), ("PRwo", "PRを"), ("GitHubyori", "GitHubより"), ("VSCodede", "VSCodeで"),
          ("YouTubeha", "YouTubeは"), ("VSCode", "VSCode")],
    );
}

/// Which particle: the reading that scores best as word + particle (figma + で, not fig + まで).
#[test]
fn picks_the_particle_by_the_word() {
    each_typed(true, &[("figmade", "figmaで"), ("Dockermade", "Dockerまで"), ("AWSmade", "AWSまで")]);
}

/// A Japanese particle beats an English + Korean word split (beau + 쇼재), and an English
/// word with a particle never goes in mid-word before Korean (lin + で + 그무).
#[test]
fn japanese_particle_before_korean_splits() {
    each_typed(true, &[("beautywo", "beautyを"), ("buddyto", "buddyと"), ("linderman", "linderman")]);
}

/// Everyday English with a Korean particle whose keys also spell Hangul nobody writes.
#[test]
fn types_english_word_with_korean_particle() {
    each_typed(
        false,
        &[("formdptj", "form에서"), ("alsodp", "also에"), ("macshotdptj", "macshot에서"), ("PRdp", "PR에"),
          ("Slackdptj", "Slack에서")],
    );
}

/// Korean words that an English word and a particle could also spell stay Korean.
#[test]
fn keeps_korean_that_spells_english_with_particle() {
    each_typed(
        true,
        &[("gosemf", "핸들"), ("gosemfdmf", "핸들을"), ("thansdms", "소문은"), ("thansdp", "소문에"), ("goTsms", "했는"),
          ("godgks", "행한"), ("godgksms", "행하는"), ("duckgkaus", "여차하면"), ("Thesms", "쏟는"),
          ("tkrlRnsdms", "사기꾼은"), ("akffkRoddl", "말라깽이")],
    );
}

/// Korean then English with no space: 오늘은 then google에서.
#[test]
fn types_korean_then_english_word() {
    let mut t = T::with(session(true));
    t.type_("dhsmfdmsgoogledptj dlrjtdmsgithubdp ");
    assert_eq!(t.field.text, "오늘은google에서 이것은github에 ");
}

#[test]
fn plain_korean_stays_whole() {
    let mut t = T::with(session(true));
    t.type_("dkssudgktpdy qusruddlfur rhkswjadmfh ");
    assert_eq!(t.field.text, "안녕하세요 변경이력 관점으로 ");
    // A consonant just typed for the next syllable isn't broken Hangul (하느님ㅇ, then lad…).
    let mut t = T::with(session(true));
    t.type_("gksmsladml tlsqnsladms godirpTspdy ");
    assert_eq!(t.field.text, "하느님의 신부님은 해야겠네요 ");
}
