//! Upstream DecisionEngineTests and HangulComposerTests.

mod common;

use nunchi_core::app_policy::STRICT_THRESHOLD;
use nunchi_core::{hangul, text_converter, DecisionContext, DecisionEngine, EnglishModel, Language};
use common::*;

fn decide(keys: &str, previous: Option<Language>) -> nunchi_core::Decision {
    shared_engine().decide(keys, &DecisionContext::previous(previous))
}

#[test]
fn english_words_stay_english() {
    let mut f = Failures::new();
    for word in [
        "hello", "world", "the", "keyboard", "language", "switch", "input", "method", "files", "running", "commit",
        "github", "please", "thanks", "Apple", "API", "is", "a", "computer", "people", "quick", "brown", "fox", "jumps",
        "over", "lazy", "dog",
    ] {
        let d = decide(word, None);
        f.check(d.language == Language::English, || format!("{word} → {} {}", d.output, d.explanation));
    }
    f.assert_none();
}

#[test]
fn korean_keystrokes_become_hangul() {
    let mut f = Failures::new();
    for (keys, expected) in [
        ("dkssudgktpdy", "안녕하세요"), ("gksrmf", "한글"), ("rkatkgkqslek", "감사합니다"),
        ("dhsmf", "오늘"), ("skfTl", "날씨"), ("whgdk", "좋아"), ("dlTek", "있다"),
        ("tkfkd", "사랑"), ("zjavbxj", "컴퓨터"), ("vmfhrmfoa", "프로그램"), ("wjdakf", "정말"),
        ("rhakdnj", "고마워"), ("wkf", "잘"), ("ajrdj", "먹어"), ("zzz", "ㅋㅋㅋ"),
        ("dlrjs", "이건"), ("djEjg", "어떻"), ("tlrks", "시간"), ("rmfjsep", "그런데"),
    ] {
        let d = decide(keys, None);
        f.check(d.language == Language::Korean && d.output == expected, || {
            format!("{keys} → {} {}", d.output, d.explanation)
        });
    }
    f.assert_none();
}

/// Shift held a key too long after a double consonant (어떤's ㄸ, 바뀌's ㄲ) capitalizes the
/// next key, which has no shifted jamo. That is Korean typed fast, not English.
#[test]
fn lingering_shift_after_korean_shift_key_stays_korean() {
    let mut f = Failures::new();
    for (keys, expected) in [
        ("djEJs", "어떤"), ("qkRNl", "바뀌"), ("qkRNleh", "바뀌도"), ("RNfdmf", "꿀을"),
        ("EKfktj", "따라서"), ("TkdRjvnf", "쌍꺼풀"), ("TKdRJvnf", "쌍꺼풀"),
        ("djEJfRkdy", "어떨까요"), ("gkfRK", "할까"), ("wlsWK", "진짜"), ("TMfkrh", "쓰라고"),
        ("qkRNjTdj", "바꿨어"), ("ahtjflWHrdl", "모서리쪽이"), ("RKwl", "까지"),
        ("dPFmf", "예를"), ("rOSp", "걔네"), ("rPFks", "계란"), ("dPDhl", "예외"),
        ("dlTDj", "있어"), ("dlTSp", "있네"), ("qkRDp", "밖에"),
    ] {
        let d = decide(keys, None);
        f.check(d.language == Language::Korean && d.output == expected, || {
            format!("{keys} → {} {}", d.output, d.explanation)
        });
    }
    f.assert_none();
}

/// A capital that no lingering Shift explains is still only ever English, after a Korean
/// word too.
#[test]
fn capitals_outside_korean_shift_keys_stay_english() {
    let mut f = Failures::new();
    for word in [
        "Hello", "API", "TV", "OK", "QA", "WE", "QUICK", "TEst", "iPhone", "GitHub", "dHello", "TM", "EU", "RN", "TL",
        "THe", "WHat", "THis", "QUery", "tvOS", "PCqkd", "OAuth", "TFxla",
    ] {
        for previous in [None, Some(Language::English), Some(Language::Korean)] {
            let d = decide(word, previous);
            f.check(d.language == Language::English && d.output == word, || {
                format!("{word} after {previous:?} → {} {}", d.output, d.explanation)
            });
        }
    }
    f.assert_none();
}

#[test]
fn english_only_capital_is_certain_after_korean() {
    let mut f = Failures::new();
    for word in ["PCqkd", "OAuth", "TFxla", "PDF", "WWDC"] {
        let d = decide(word, Some(Language::Korean));
        f.check(d.language == Language::English && d.confidence >= 0.99, || {
            format!("{word} → {} {}", d.confidence, d.explanation)
        });
    }
    f.assert_none();
}

#[test]
fn mixed_sentence() {
    let engine = shared_engine();
    assert_eq!(text_converter::convert(&*engine, "dkssudgktpdy, hello world!").0, "안녕하세요, hello world!");
    assert_eq!(text_converter::convert(&*engine, "dlrjs github dp commit gkf rjdi").0, "이건 github 에 commit 할 거야");
    assert_eq!(text_converter::convert(&*engine, "tmvmfls API qkfvy wnsql").0, "스프린 API 발표 준비");
}

#[test]
fn context_resolves_short_collisions() {
    let engine = shared_engine();
    // "go" is English, but also 해 — the previous word tips it.
    assert_eq!(text_converter::convert(&*engine, "let's go").0, "let's go");
    assert_eq!(text_converter::convert(&*engine, "dlrj go").0, "이거 해");
}

/// A Korean word that Tatoeba's sentences lack (닥터, 했죠), or have only with a particle
/// (혜택 from 혜택을), is as sure after English as a Strict app needs.
#[test]
fn vocabulary_words_after_english() {
    let mut f = Failures::new();
    for (keys, expected) in [("gPxor", "혜택"), ("ekrxj", "닥터"), ("goTwy", "했죠"), ("whwl", "조지")] {
        let d = decide(keys, Some(Language::English));
        f.check(d.language == Language::Korean && d.output == expected && d.confidence >= STRICT_THRESHOLD, || {
            format!("{keys} → {} {:.3} {}", d.output, d.confidence, d.explanation)
        });
    }
    f.assert_none();
}

#[test]
fn rare_noun_from_the_full_list() {
    let d = decide("chrao", None);
    assert!(d.language == Language::Korean && d.output == "촉매", "{} {}", d.output, d.explanation);
}

#[test]
fn english_words_spelling_vocabulary_stay_english() {
    let mut f = Failures::new();
    for word in ["work", "ahem", "claus"] {
        for previous in [None, Some(Language::Korean)] {
            let d = decide(word, previous);
            f.check(d.language == Language::English, || format!("{word} after {previous:?} → {} {}", d.output, d.explanation));
        }
    }
    f.assert_none();
}

#[test]
fn no_guessed_inflection_of_a_rare_stem() {
    let mut f = Failures::new();
    for (keys, expected) in [("thrus", "소견"), ("ahems", "모든")] {
        f.check(!EnglishModel::shared().is_known_word(keys), || format!("{keys} is a known word"));
        let d = decide(keys, None);
        f.check(d.language == Language::Korean && d.output == expected, || format!("{keys} → {} {}", d.output, d.explanation));
    }
    f.assert_none();
}

#[test]
fn guessed_inflection_of_a_common_stem_is_a_word() {
    for word in ["travelling", "cancelled", "maths", "onwards"] {
        assert!(EnglishModel::shared().is_known_word(word), "{word}");
    }
}

#[test]
fn learning_overrides_base() {
    let url = temp_path("test");
    let learning = dictionary(shared_engine(), Some(url.clone()));
    let ctx = DecisionContext::default();
    assert_eq!(learning.decide("hello", &ctx).language, Language::English);
    learning.learn("hello", Language::Korean, None);
    assert_eq!(learning.decide("hello", &ctx).language, Language::English); // once: not yet
    learning.learn("hello", Language::Korean, None);
    assert_eq!(learning.decide("hello", &ctx).output, "ㅗ디ㅣㅐ");
    // Persisted and reloaded.
    let reloaded = dictionary(shared_engine(), Some(url.clone()));
    assert_eq!(reloaded.decide("hello", &ctx).language, Language::Korean);
    // Learning the base choice removes the override.
    reloaded.learn("hello", Language::English, None);
    assert_eq!(reloaded.decide("hello", &ctx).language, Language::English);
    let _ = std::fs::remove_file(url);
}

/// A dictionary.json it can't read (hand-edited, another version) is set aside, not overwritten.
#[test]
fn unreadable_dictionary_is_kept_aside() {
    let url = temp_path("test");
    let mut aside = url.clone().into_os_string();
    aside.push(".unreadable");
    std::fs::write(&url, r#"{"de": "japanese"}"#).unwrap();
    let learning = dictionary(shared_engine(), Some(url.clone()));
    assert!(learning.learned().is_empty() && learning.pending().is_empty());
    assert!(std::path::Path::new(&aside).exists());
    // A different correction than before starts over.
    learning.learn("rjtm", Language::English, Some(Language::Korean));
    learning.learn("rjtm", Language::Mixed, Some(Language::Korean));
    assert!(learning.learned().is_empty());
    assert_eq!(learning.pending(), [("rjtm".to_string(), Language::Mixed)].into_iter().collect());
    let _ = std::fs::remove_file(url);
    let _ = std::fs::remove_file(aside);
}

#[test]
fn partial_preview_does_not_punish_pending_consonant() {
    // "dkss" = 안 + next ㄴ while typing 안녕
    let d = shared_engine().decide("dkss", &DecisionContext { partial: true, ..Default::default() });
    assert_eq!(d.language, Language::Korean, "{}", d.explanation);
}

// HangulComposerTests

#[test]
fn composes() {
    for (keys, expected) in [
        ("dkssudgktpdy", "안녕하세요"), ("gksrmf", "한글"), ("rkqt", "값"), ("rkqtdl", "값이"), ("dlTek", "있다"),
        ("dhkswjsgl", "완전히"), ("ehlwl", "되지"), ("dmlwk", "의자"), ("ekfrdl", "닭이"), ("rhkswls", "관진"),
        ("Qkffl", "빨리"), ("dkQk", "아빠"), ("zzz", "ㅋㅋㅋ"), ("rk", "가"), ("r", "ㄱ"), ("k", "ㅏ"),
    ] {
        assert_eq!(hangul::text(keys).as_deref(), Some(expected), "{keys}");
    }
}

#[test]
fn double_consonant_that_cannot_be_final_starts_new_syllable() {
    // 아 + ㄸ: ㄸ can't be 받침, so it waits as the next initial.
    let result = hangul::compose("dkE").unwrap();
    assert_eq!(result.text(), "아ㄸ");
    assert!(result.ends_with_pending_consonant());
}

#[test]
fn english_produces_stray_jamo() {
    assert!(hangul::compose("hello").unwrap().has_stray_jamo());
}

#[test]
fn rejects_non_letters() {
    assert!(hangul::compose("ab1").is_none());
}
