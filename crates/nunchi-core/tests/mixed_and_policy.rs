//! Upstream MixedAndPolicyTests: MixedWordTests, AppPolicyTests, SessionPolicyTests.

mod common;

use std::sync::{Arc, OnceLock};

use nunchi_core::app_policy::{Override, DEFAULT_KEY};
use nunchi_core::mixed_word::{FinalSound, MixedWord};
use nunchi_core::plain_field::PlainTextField;
use nunchi_core::{
    text_converter, AppPolicies, AppPolicy, DecisionContext, DecisionEngine, InputSession, Key, Language,
    SplittingEngine, SwitchKey,
};
use common::*;

/// `SplittingEngine(base: sharedEngine, english: sharedEngine.english)` upstream: no Japanese.
fn engine() -> Arc<SplittingEngine> {
    static ENGINE: OnceLock<Arc<SplittingEngine>> = OnceLock::new();
    ENGINE.get_or_init(|| splitting(shared_engine())).clone()
}

fn check(cases: &[(&str, &str)], language: Language) {
    let mut f = Failures::new();
    for (keys, expected) in cases {
        let d = engine().decide(keys, &DecisionContext::default());
        f.check(d.language == language && d.output == *expected, || {
            format!("{keys} → {} ({:?}) {}", d.output, d.language, d.explanation)
        });
    }
    f.assert_none();
}

#[test]
fn splits_english_plus_particle() {
    check(
        &[
            ("githubdp", "github에"), ("APIfmf", "API를"), ("commitgotj", "commit해서"),
            ("serverdptj", "server에서"), ("pythondmfh", "python으로"), ("deploygoTdj", "deploy했어"),
            ("TextEditdptj", "TextEdit에서"), ("Slackdptj", "Slack에서"), ("VSCodedptj", "VSCode에서"),
            ("GitHubdp", "GitHub에"), ("iPhonedptj", "iPhone에서"),
            ("TVdp", "TV에"), ("QAfmf", "QA를"), ("RNdms", "RN은"),
        ],
        Language::Mixed,
    );
}

#[test]
fn acronym_with_agreeing_particle_splits() {
    check(
        &[
            ("TMdms", "TM은"), ("TMdmf", "TM을"), ("TLdms", "TL은"), ("TLdmf", "TL을"),
            ("ELdl", "EL이"), ("ELdms", "EL은"), ("RJdmf", "RJ을"), ("RJdl", "RJ이"),
        ],
        Language::Mixed,
    );
}

#[test]
fn acronym_shaped_particle_korean_stays_korean() {
    check(
        &[("TMsms", "쓰는"), ("RNsms", "꾸는"), ("TLdp", "씨에"), ("Tkfdmf", "쌀을"), ("Tlsms", "씨는")],
        Language::Korean,
    );
}

#[test]
fn particle_form_follows_the_final_sound() {
    let f = |s: char| MixedWord::final_sound(s).unwrap();
    let a = MixedWord::final_sound_of_acronym;
    assert_eq!(MixedWord::particle_agrees("을", f('책')), Some(true));
    assert_eq!(MixedWord::particle_agrees("를", f('책')), Some(false));
    assert_eq!(MixedWord::particle_agrees("이", f('씨')), Some(false));
    assert_eq!(MixedWord::particle_agrees("가", f('씨')), Some(true));
    assert_eq!(MixedWord::particle_agrees("로", f('울')), Some(true)); // 서울로
    assert_eq!(MixedWord::particle_agrees("으로", f('울')), Some(false));
    assert_eq!(MixedWord::particle_agrees("을", f('울')), Some(true));
    assert_eq!(MixedWord::particle_agrees("에", f('씨')), None);
    assert_eq!(MixedWord::particle_agrees("은", a("TM")), Some(true));
    assert_eq!(MixedWord::particle_agrees("는", a("TM")), Some(false));
    assert_eq!(MixedWord::particle_agrees("는", a("TV")), Some(true));
    assert_eq!(MixedWord::particle_agrees("로", a("URL")), Some(true));
    assert_eq!(a("URL"), FinalSound::Rieul);
}

#[test]
fn lingering_shift_is_not_a_name() {
    check(
        &[("RNfdmf", "꿀을"), ("qkRNleh", "바뀌도"), ("djEJsrj", "어떤거"), ("TMfkrh", "쓰라고")],
        Language::Korean,
    );
}

#[test]
fn acronym_shaped_korean_stays_korean() {
    check(
        &[
            ("EKfktj", "따라서"), ("RJsoek", "꺼내다"), ("WHroek", "쪼개다"), ("TMfprl", "쓰레기"),
            ("RUdksrh", "껴안고"), ("ELehdrkq", "띠동갑"), ("QYwhrgks", "뾰족한"), ("QLdpfh", "삐에로"),
            ("QLwuTdj", "삐졌어"), ("WUajrek", "쪄먹다"), ("WUqjfuTdj", "쪄버렸어"), ("WlffjTdj", "찔렀어"),
            ("WNrmflrh", "쭈그리고"), ("EHrhdfyddl", "또공룡이"),
            ("TMwjdcor", "쓰정책"), ("TLtjqj", "씨서버"),
        ],
        Language::Korean,
    );
}

#[test]
fn splits_english_plus_korean_word() {
    check(
        &[
            ("hellodnjfem", "hello월드"), ("githubtjqj", "github서버"), ("loginghkaus", "login화면"),
            ("designvhsxm", "design폰트"), ("Reactzjavhsjsxm", "React컴포넌트"), ("VSCodetjfwjd", "VSCode설정"),
            ("PDFvkdlf", "PDF파일"), ("WWDCvhfja", "WWDC포럼"), ("OAuthdlswmd", "OAuth인증"),
            ("RPAdlswmd", "RPA인증"), ("TPSvkdlf", "TPS파일"),
            ("EUwjdcor", "EU정책"), ("TUtjqj", "TU서버"), ("TBvkdlf", "TB파일"), ("ETLwjdcor", "ETL정책"),
            ("TTLtjfwjd", "TTL설정"),
            ("PCzkxhrdptj", "PC카톡에서"), ("APIzkxhrdptj", "API카톡에서"),
        ],
        Language::Mixed,
    );
}

#[test]
fn leaves_whole_words_alone() {
    let mut f = Failures::new();
    for keys in [
        "work", "park", "hello", "thanks", "dkssudgktpdy", "akadp", "ruxdp", "wkfrk", "TkdRjvnfdms", "dlTdj", "Rmxdptj",
        "ThkTdjdy", "RoRmtgkgfusk", "cartman", "Cartman", "stuntman", "comstock", "wolfsbane", "hasansowo", "nonakawo",
    ] {
        let d = engine().decide(keys, &DecisionContext::default());
        f.check(d.language != Language::Mixed, || format!("{keys} → {} {}", d.output, d.explanation));
    }
    f.assert_none();
}

#[test]
fn esc_cycles_through_readings() {
    assert_eq!(Language::English.next("githubdp", false, None), Language::Mixed);
    assert_eq!(Language::Mixed.next("githubdp", false, None), Language::English); // 햐소ㅕㅠ에 is garbled: skipped
    assert_eq!(Language::Korean.next("githubdp", false, None), Language::English);
    assert_eq!(Language::English.next("dkssud", false, None), Language::Korean);
    // No split available: english ↔ korean.
    assert_eq!(Language::English.next("hello", false, None), Language::Korean);
    // The engine's split (x에), which the particle split doesn't find: ㅌ에 is garbled, skipped.
    assert_eq!(Language::English.next("xdp", false, Some(true)), Language::Mixed);
    assert_eq!(Language::Mixed.next("xdp", false, Some(true)), Language::English);
    // Chat jamo (ㅜㅜ, ㅋㅋ) are a clean Korean reading, not garbled.
    assert_eq!(Language::English.next("nn", true, None), Language::Korean);
    assert_eq!(Language::Korean.next("nn", true, None), Language::Japanese);
    assert_eq!(Language::Japanese.next("nn", true, None), Language::English);
}

/// Every two of ㄱ…ㅎ typed alone (ㅁㅊ, ㅈㅅ, ㄹㅇ) is a reading the switch key reaches, also
/// when the keys read cleanly as romaji and English both (ra: ら, ra, ㄱㅁ).
#[test]
fn switch_key_reaches_every_consonant_pair() {
    let keys: Vec<char> = "rsefaqtdwczxvg".chars().collect();
    for japanese in [true, false] {
        for &a in &keys {
            for &b in &keys {
                let pair: String = [a, b].iter().collect();
                let mut language = Language::English;
                let mut seen = Vec::new();
                for _ in 0..4 {
                    language = language.next(&pair, japanese, None);
                    seen.push(language);
                }
                assert!(seen.contains(&Language::Korean), "{pair}: {seen:?}");
            }
        }
    }
}

#[test]
fn mixed_sentence() {
    assert_eq!(text_converter::convert(&*engine(), "dlrjs githubdp commitgkf rjdi").0, "이건 github에 commit할 거야");
}

// AppPolicyTests

fn with_keys(keys: &[SwitchKey]) -> AppPolicy {
    AppPolicy { switch_keys: keys.iter().copied().collect(), ..AppPolicy::standard() }
}

/// No app is special unless the user lists it.
#[test]
fn every_app_starts_at_the_default() {
    let policies = AppPolicies::default();
    assert_eq!(policies.policy(Some("windowsterminal.exe")), AppPolicy::standard());
    assert_eq!(policies.policy(Some("kakaotalk.exe")), AppPolicy::standard());
    assert_eq!(policies.policy(None), AppPolicy::standard());
    assert!(policies.apps().is_empty());
}

#[test]
fn user_entries_and_prefixes() {
    let url = temp_path("apps");
    std::fs::write(&url, r#"{"WindowsTerminal.exe": {"enabled": false}, "jetbrains.": {"conversionThreshold": 0.97}}"#).unwrap();
    let policies = AppPolicies::load(Some(&url));
    assert!(!policies.policy(Some("windowsterminal.exe")).enabled);
    assert_eq!(policies.policy(Some("jetbrains.intellij.exe")).conversion_threshold, 0.97);
    assert!(AppPolicies::load(Some(&url.with_extension("missing"))).overrides.is_empty());
    let _ = std::fs::remove_file(url);
}

/// Of two prefixes that match, the longer one is the app's, every time.
#[test]
fn the_longest_prefix_wins() {
    let url = temp_path("apps");
    let others: Vec<String> = (0..40).map(|i| format!(r#""app{i}.exe": {{"enabled": false}}"#)).collect();
    std::fs::write(
        &url,
        format!(r#"{{"j.": {{"conversionThreshold": 0.5}}, "j.brains.": {{"conversionThreshold": 0.97}}, {}}}"#, others.join(", ")),
    )
    .unwrap();
    for _ in 0..10 {
        let policies = AppPolicies::load(Some(&url));
        assert_eq!(policies.policy(Some("j.brains.goland")).conversion_threshold, 0.97);
        assert_eq!(policies.policy(Some("j.terminal")).conversion_threshold, 0.5);
    }
    let _ = std::fs::remove_file(url);
}

/// An app's entry changes only the keys it names; the rest follows the user's default.
#[test]
fn app_entries_sit_on_the_default() {
    let url = temp_path("apps");
    std::fs::write(
        &url,
        r#"{"*": {"switchKeys": ["escape"]}, "ghostty.exe": {"conversionThreshold": 0.97}, "game.exe": {"enabled": false}}"#,
    )
    .unwrap();
    let policies = AppPolicies::load(Some(&url));
    assert_eq!(policies.default_policy(), with_keys(&[SwitchKey::Escape]));
    assert_eq!(
        policies.policy(Some("ghostty.exe")),
        AppPolicy { conversion_threshold: 0.97, ..with_keys(&[SwitchKey::Escape]) }
    );
    assert_eq!(policies.policy(Some("game.exe")), AppPolicy { enabled: false, ..with_keys(&[SwitchKey::Escape]) });
    assert_eq!(policies.policy(Some("notepad.exe")), with_keys(&[SwitchKey::Escape]));
    assert_eq!(policies.apps(), vec!["game.exe".to_string(), "ghostty.exe".to_string()]);
    let _ = std::fs::remove_file(url);
}

/// Holding the word after Space is an app's own choice, and only that key is written for it.
#[test]
fn hold_space_is_per_app() {
    let url = temp_path("apps");
    std::fs::write(&url, r#"{"ghostty.exe": {"holdSpace": true}}"#).unwrap();
    let mut policies = AppPolicies::load(Some(&url));
    let holding = AppPolicy { hold_space: true, ..AppPolicy::standard() };
    assert_eq!(policies.policy(Some("ghostty.exe")), holding);
    assert_eq!(policies.policy(Some("notepad.exe")), AppPolicy::standard());
    policies.set(&holding, "WindowsTerminal.exe");
    assert_eq!(policies.overrides["windowsterminal.exe"], Override { hold_space: Some(true), ..Default::default() });
    policies.save(Some(&url)).unwrap();
    assert_eq!(AppPolicies::load(Some(&url)).policy(Some("windowsterminal.exe")), holding);
    let _ = std::fs::remove_file(url);
}

// SessionPolicyTests

struct SessionPolicy {
    field: PlainTextField,
    learning: Arc<nunchi_core::DictionaryEngine>,
    session: InputSession,
}

impl SessionPolicy {
    fn new() -> Self {
        let learning = dictionary(engine(), Some(temp_path("lex")));
        let mut policies = esc_switches();
        policies.overrides.insert("test.strict".into(), Override { conversion_threshold: Some(0.9999), ..Default::default() });
        policies.overrides.insert("test.off".into(), Override { enabled: Some(false), ..Default::default() });
        let session = InputSession::new(learning.clone(), policies, None, false, false, true);
        let _ = DEFAULT_KEY;
        SessionPolicy { field: PlainTextField::new(), learning, session }
    }

    fn type_(&mut self, s: &str) {
        type_script(&mut self.session, &mut self.field, s);
    }
}

#[test]
fn strict_app_keeps_uncertain_words_english() {
    let mut t = SessionPolicy::new();
    t.session.activate(Some("test.strict"));
    t.type_("dhsmf ");
    assert_eq!(t.field.text, "dhsmf ");
}

#[test]
fn disabled_app_passes_keys_through() {
    let mut t = SessionPolicy::new();
    t.session.activate(Some("test.off"));
    assert!(!t.session.handle(Key::Letter('d'), &mut t.field));
    assert!(t.field.text.is_empty());
}

#[test]
fn esc_cycles_mixed_word_after_commit() {
    let mut t = SessionPolicy::new();
    t.type_("githubdp ");
    assert_eq!(t.field.text, "github에 ");
    // The garbled Korean reading (햐소ㅕㅠ에) is skipped: github에 ↔ githubdp.
    t.session.handle(Key::Escape, &mut t.field);
    assert_eq!(t.field.text, "githubdp ");
    t.session.handle(Key::Escape, &mut t.field);
    assert_eq!(t.field.text, "github에 ");
}

#[test]
fn correction_counts_once_with_the_final_choice() {
    let mut t = SessionPolicy::new();
    t.session.activate(Some("chat.exe"));
    t.type_("hello ");
    t.session.handle(Key::Escape, &mut t.field); // hello → ㅗ디ㅣㅐ
    assert!(t.learning.pending().is_empty()); // window still open
    t.type_("x"); // any other key closes it
    assert_eq!(t.learning.pending(), [("hello".to_string(), Language::Korean)].into_iter().collect());
}

#[test]
fn nothing_counts_when_the_word_is_flipped_back() {
    let mut t = SessionPolicy::new();
    t.type_("hello ");
    t.session.handle(Key::Escape, &mut t.field);
    t.session.handle(Key::Escape, &mut t.field);
    t.type_("x");
    assert!(t.learning.pending().is_empty());
}
