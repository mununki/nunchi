//! Upstream VimTests, PauseTests (with StrictSplitTests), UserDictionaryTests, SebeolsikTests
//! and LatinLayoutTests.

mod common;

use std::sync::Arc;

use nunchi_core::app_policy::{Override, STRICT_THRESHOLD};
use nunchi_core::plain_field::PlainTextField;
use nunchi_core::user_dictionary::{Entry, Source, Strength, UserDictionary};
use nunchi_core::vim::VimSignal;
use nunchi_core::{
    hangul, romaji, AppPolicies, Decision, DecisionContext, DecisionEngine, DictionaryEngine, EnglishModel, InputSession,
    Key, KoreanLayout, KoreanModel, Language, LatinLayout, TextClient,
};
use chrono::NaiveDate;
use common::*;

// VimSessionTests: a terminal left at the default policy (Esc switches language), with vim
// reporting its modes.

struct Vim {
    field: PlainTextField,
    session: InputSession,
}

impl Vim {
    fn new() -> Self {
        let engine = dictionary(shared_engine(), Some(temp_path("vim")));
        let mut session = InputSession::new(engine, esc_switches(), None, false, false, true);
        session.activate(Some("test.terminal"));
        Vim { field: PlainTextField::new(), session }
    }

    /// Types `script` ("⎋" Esc, "⏎" Return) and returns the keys the app got straight.
    fn type_(&mut self, script: &str) -> String {
        let mut passed = String::new();
        for ch in script.chars() {
            let key = match ch {
                '⎋' => Key::Escape,
                '⏎' => Key::Enter,
                c if c.is_ascii_alphabetic() => Key::Letter(c),
                c => Key::Printable(c),
            };
            if !self.session.handle(key, &mut self.field) {
                passed.push(ch);
                if ch != '⎋' {
                    let typed = if ch == '⏎' { "\n".to_string() } else { ch.to_string() };
                    self.field.insert_text(&typed, None);
                }
            }
        }
        passed
    }

    fn vim(&mut self, signal: VimSignal) {
        self.session.vim_changed(signal, Some(&mut self.field));
    }
}

#[test]
fn normal_mode_keys_go_straight_through() {
    let mut t = Vim::new();
    t.vim(VimSignal::Normal); // VimEnter, before any Esc
    assert_eq!(t.type_("sddciw:wq⏎"), "sddciw:wq⏎");
    assert!(t.field.marked.is_none());
}

#[test]
fn words_are_decided_in_insert_mode() {
    let mut t = Vim::new();
    t.vim(VimSignal::Normal);
    t.type_("o");
    t.vim(VimSignal::Insert);
    t.type_("dkssud ");
    assert_eq!(t.field.text, "o안녕 ");
}

#[test]
fn esc_commits_the_word_then_vim_says_normal() {
    let mut t = Vim::new();
    t.vim(VimSignal::Insert);
    t.type_("dkssud⎋");
    assert_eq!(t.field.text, "안녕");
    t.vim(VimSignal::Normal); // InsertLeave
    assert_eq!(t.type_("dd"), "dd");
}

#[test]
fn keys_are_held_until_vim_says_insert() {
    let mut t = Vim::new();
    t.vim(VimSignal::Normal);
    assert_eq!(t.type_(" i"), " i");
    t.vim(VimSignal::Insert);
    assert_eq!(t.type_("dkssud"), "");
}

#[test]
fn leaving_vim_gives_the_shell_words_back() {
    let mut t = Vim::new();
    t.vim(VimSignal::Normal);
    t.type_(":wq⏎");
    t.vim(VimSignal::Off); // VimLeavePre
    t.type_("dkssud ");
    assert!(t.field.text.ends_with("안녕 "), "{}", t.field.text);
}

#[test]
fn shift_esc_starts_deciding_words_again() {
    let mut t = Vim::new();
    t.vim(VimSignal::Normal);
    assert!(t.session.handle(Key::Cycle, &mut t.field));
    assert!(!t.session.in_vim_normal_mode());
    t.type_("dkssud ");
    assert_eq!(t.field.text, "안녕 ");
}

#[test]
fn coming_back_to_the_terminal_keeps_vims_mode() {
    let mut t = Vim::new();
    t.vim(VimSignal::Normal);
    t.session.activate(Some("test.terminal"));
    assert_eq!(t.type_("dd"), "dd");
}

#[test]
fn esc_is_vims_while_vim_reports_even_where_esc_switches_language() {
    let mut t = Vim::new();
    t.vim(VimSignal::Insert);
    assert_eq!(t.type_("dkssud⎋"), "⎋");
    assert_eq!(t.field.text, "안녕");
    // Right after a committed word too: Esc leaves insert mode instead of flipping the word.
    t.type_(" ");
    t.vim(VimSignal::Insert);
    assert_eq!(t.type_("dkssud ⎋"), " ⎋");
    assert_eq!(t.field.text, "안녕 안녕 ");
}

#[test]
fn without_vim_esc_still_switches_language() {
    let mut t = Vim::new();
    assert_eq!(t.type_("dkssud⎋"), "");
    assert_eq!(t.field.text, "dkssud");
}

#[test]
fn another_app_forgets_vim() {
    let mut t = Vim::new();
    t.vim(VimSignal::Normal);
    t.session.activate(Some("test.other"));
    assert!(!t.session.in_vim_normal_mode());
}

// PauseTests: the pause key pauses Nunchi in the text field.

fn pause_session() -> (InputSession, PlainTextField) {
    let engine = dictionary(shared_engine(), Some(temp_path("pause")));
    let mut session = InputSession::new(engine, AppPolicies::default(), None, false, false, true);
    session.activate(Some("test.app"));
    (session, PlainTextField::new())
}

#[test]
fn paused_keys_go_through_as_typed() {
    let (mut session, mut field) = pause_session();
    session.toggle_pause(Some(&mut field));
    type_script(&mut session, &mut field, "jjkkhl dkssud");
    assert_eq!(field.text, "jjkkhl dkssud");
    assert!(field.marked.is_none());
}

#[test]
fn pausing_commits_the_word_in_progress() {
    let (mut session, mut field) = pause_session();
    type_script(&mut session, &mut field, "dkssud");
    session.toggle_pause(Some(&mut field));
    assert_eq!(field.text, "안녕");
    assert!(field.marked.is_none());
}

#[test]
fn second_tap_resumes() {
    let (mut session, mut field) = pause_session();
    session.toggle_pause(Some(&mut field));
    type_script(&mut session, &mut field, "jk ");
    session.toggle_pause(Some(&mut field));
    type_script(&mut session, &mut field, "dkssud ");
    assert_eq!(field.text, "jk 안녕 ");
}

#[test]
fn another_text_field_resumes() {
    let (mut session, mut field) = pause_session();
    session.toggle_pause(Some(&mut field));
    session.activate(Some("test.other"));
    assert!(!session.paused());
}

/// An app set to Strict keeps short commands as typed but still splits mixed words.
#[test]
fn strict_still_splits_mixed_words() {
    let mut policies = AppPolicies::default();
    policies
        .overrides
        .insert("test.terminal".into(), Override { conversion_threshold: Some(STRICT_THRESHOLD), ..Default::default() });
    let engine = dictionary(splitting(shared_engine()), Some(temp_path("strict")));
    let mut session = InputSession::new(engine, policies, None, false, false, true);
    session.activate(Some("test.terminal"));
    let mut field = PlainTextField::new();
    type_script(&mut session, &mut field, "ciw gst githubdp xdp hellodnjfem ");
    assert_eq!(field.text, "ciw gst github에 x에 hello월드 ");
}

// UserDictionaryTests: corrected once nudges, twice wins, weak entries expire.

fn day() -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 10, 15).unwrap()
}

fn days(n: i64) -> NaiveDate {
    day() + chrono::Duration::days(n)
}

fn dated(base: Arc<dyn DecisionEngine>, url: std::path::PathBuf, today: NaiveDate) -> DictionaryEngine {
    DictionaryEngine::new(base, EnglishModel::shared(), KoreanModel::new(), false, Some(url), Some(Box::new(move || today)))
}

/// One correction only nudges: `so` (en −1.90, ko −2.00) turns, `hello` doesn't.
#[test]
fn once_nudges_a_close_call_only() {
    let engine = dictionary(shared_engine(), Some(temp_path("d")));
    let ctx = DecisionContext::default();
    assert_eq!(engine.decide("so", &ctx).language, Language::English);
    engine.learn("so", Language::Korean, Some(Language::English));
    assert_eq!(engine.pending(), [("so".to_string(), Language::Korean)].into_iter().collect());
    assert!(engine.learned().is_empty());
    let so = engine.decide("so", &ctx);
    assert!(so.language == Language::Korean && so.output == "내", "{}", so.explanation);
    // After an English word the previous word's pull and the nudge cancel out.
    assert_eq!(engine.decide("so", &DecisionContext::previous(Some(Language::English))).language, Language::English);

    engine.learn("hello", Language::Korean, Some(Language::English));
    assert_eq!(engine.decide("hello", &ctx).language, Language::English);
    engine.learn("hello", Language::Korean, Some(Language::English));
    let hello = engine.decide("hello", &ctx);
    assert!(hello.language == Language::Korean && hello.confidence == 1.0 && hello.explanation == "learned");
}

#[test]
fn learning_rules() {
    let mut d = UserDictionary::default();
    assert!(!d.learn("dlrjs", Language::Korean, Language::English, day()));
    assert_eq!(d.language_entry("dlrjs", None).and_then(|e| e.strength), Some(Strength::Weak));
    // Corrected to something else: weak for that one instead.
    d.learn("dlrjs", Language::Japanese, Language::English, day());
    assert_eq!(d.language_entry("dlrjs", None).and_then(|e| e.language), Some(Language::Japanese));
    assert_eq!(d.language_entry("dlrjs", None).and_then(|e| e.strength), Some(Strength::Weak));
    assert!(d.learn("dlrjs", Language::Japanese, Language::English, day()));
    assert_eq!(d.language_entry("dlrjs", None).and_then(|e| e.strength), Some(Strength::Sure));
    // Back to what was shown without the entry: forgotten.
    d.learn("dlrjs", Language::English, Language::English, day());
    assert!(d.entries().is_empty());
    // Turned by a weak entry and switched away from it: forgotten.
    d.learn("so", Language::Korean, Language::English, day());
    d.learn("so", Language::English, Language::Korean, day());
    assert!(d.entries().is_empty());
    // A sentence switched as one token isn't learned.
    d.learn("taxidnswjstndkwjTlkonnichiha", Language::Korean, Language::English, day());
    assert!(d.entries().is_empty());
}

#[test]
fn words_added_by_hand_stay() {
    let mut d = UserDictionary::new(vec![
        Entry::user("hoge", Language::Japanese),
        Entry::new("rt", None, Some("감사합니다".into()), Source::User, None, None, None),
    ]);
    d.learn("hoge", Language::English, Language::Japanese, day());
    d.learn("hoge", Language::English, Language::Japanese, day());
    assert_eq!(d.language_entry("hoge", None).and_then(|e| e.language), Some(Language::Japanese));
    d.learn("rk", Language::Korean, Language::English, day());
    d.forget_learned();
    assert_eq!(d.entries().len(), 2);
    // The text entry isn't a language: forgetting the keys leaves it.
    d.forget("rt");
    d.forget("hoge");
    assert_eq!(d.entries().iter().map(|e| e.keys.as_str()).collect::<Vec<_>>(), vec!["rt"]);
}

#[test]
fn weak_entries_expire_unless_used() {
    let mut d = UserDictionary::default();
    d.learn("so", Language::Korean, Language::English, day());
    d.learn("rk", Language::Korean, Language::English, day());
    d.learn("rk", Language::Korean, Language::English, day());
    d.learn("gks", Language::Korean, Language::English, day());
    let used = d.used("gks", Language::Korean, days(20));
    let same_day = d.used("gks", Language::Korean, days(20));
    let other_language = d.used("gks", Language::English, days(21));
    assert!(used && !same_day && !other_language);
    let early = d.drop_expired(days(29));
    let month = d.drop_expired(days(30));
    assert!(!early && month);
    // `so` went; the sure `rk` and the `gks` typed on day 20 stay.
    assert_eq!(d.entries().iter().map(|e| e.keys.as_str()).collect::<Vec<_>>(), vec!["rk", "gks"]);
}

#[test]
fn saves_and_reads_back() {
    let url = temp_path("dictionary");
    let engine = dated(shared_engine(), url.clone(), day());
    engine.learn("hello", Language::Korean, Some(Language::English));
    engine.learn("hello", Language::Korean, Some(Language::English));
    engine.learn("so", Language::Korean, Some(Language::English));
    let json = std::fs::read_to_string(&url).unwrap();
    assert!(json.contains(r#""as": "korean""#) && json.contains(r#""from": "learned""#) && !json.contains("text"), "{json}");
    let reloaded = dated(shared_engine(), url.clone(), day());
    assert_eq!(reloaded.learned(), [("hello".to_string(), Language::Korean)].into_iter().collect());
    assert_eq!(reloaded.pending(), [("so".to_string(), Language::Korean)].into_iter().collect());
    // A month later the weak one is gone, also from the file.
    let later = dated(shared_engine(), url.clone(), days(31));
    assert!(later.pending().is_empty());
    assert_eq!(later.learned(), [("hello".to_string(), Language::Korean)].into_iter().collect());
    assert_eq!(UserDictionary::decode(&std::fs::read(&url).unwrap()).unwrap().entries().len(), 1);
    let _ = std::fs::remove_file(url);
}

struct English;

impl DecisionEngine for English {
    fn decide(&self, keys: &str, _context: &DecisionContext) -> Decision {
        Decision::new(Language::English, 0.6, keys, "fixed")
    }
}

fn fixed_engine(entries: Vec<Entry>, japanese: bool) -> DictionaryEngine {
    let url = temp_path("p");
    std::fs::write(&url, UserDictionary::new(entries).encoded().unwrap()).unwrap();
    DictionaryEngine::new(Arc::new(English), EnglishModel::shared(), KoreanModel::new(), japanese, Some(url), None)
}

fn learned(keys: &str, language: Language, strength: Strength) -> Entry {
    Entry::new(keys, Some(language), None, Source::Learned, Some(strength), None, None)
}

/// A word that wins carries over to the word with a particle after it.
#[test]
fn word_with_particle() {
    let engine = fixed_engine(
        vec![
            Entry::user("chrao", Language::Korean),
            learned("tkfka", Language::Korean, Strength::Sure),
            Entry::user("ghostty", Language::English),
            Entry::user("hoge", Language::Japanese),
            learned("zig", Language::English, Strength::Weak),
        ],
        true,
    );
    let read = |keys: &str| {
        let d = engine.decide(keys, &DecisionContext::default());
        format!("{} {}", d.language.raw(), d.output)
    };
    assert_eq!(read("chraofmf"), "korean 촉매를");
    assert_eq!(read("chraork"), "korean 촉매가");
    assert_eq!(read("tkfkadms"), "korean 사람은");
    // The form the word doesn't take (사람는) isn't the word with a particle.
    assert_eq!(read("tkfkasms"), "english tkfkasms");
    assert_eq!(read("ghosttydp"), "mixed ghostty에");
    assert_eq!(read("ghosttydptj"), "mixed ghostty에서");
    assert_eq!(read("ghosttyde"), "japanese ghosttyで");
    assert_eq!(read("hogede"), "japanese ほげで");
    assert_eq!(read("hogekara"), "japanese ほげから");
    // Not a particle, a weak word, or keys that are an English word as a whole.
    assert_eq!(read("ghosttyx"), "english ghosttyx");
    assert_eq!(read("zigdp"), "english zigdp");
    // Keys that read as a Korean word with a particle, or as clean romaji, stay whole;
    // a lowercase English word needs 3 letters.
    let short = fixed_engine(
        vec![
            Entry::user("sha", Language::English),
            Entry::user("toki", Language::English),
            Entry::user("to", Language::English),
            Entry::user("PC", Language::English),
        ],
        true,
    );
    let ctx = DecisionContext::default();
    assert_eq!(short.decide("shaemfdmf", &ctx).language, Language::English); // 놈들을
    assert_eq!(short.decide("tokiha", &ctx).language, Language::English); // ときは
    assert_eq!(short.decide("tosms", &ctx).language, Language::English); // 새는
    assert_eq!(short.decide("PCdp", &ctx).output, "PC에");
    let off = fixed_engine(vec![Entry::user("hoge", Language::Japanese)], false);
    assert_eq!(off.decide("hogede", &ctx).language, Language::English);
    let ghost = fixed_engine(vec![Entry::user("ghos", Language::English)], false);
    assert_eq!(ghost.decide("ghost", &ctx).output, "ghost");
}

/// Added from the settings window: one entry per keys and apps, an app's own one first.
#[test]
fn add_pin_remove_and_apps() {
    let engine = fixed_engine(vec![learned("dlrjs", Language::Korean, Strength::Sure)], false);
    let slack = "slack.exe";
    let for_apps = |keys: &str, language: Language, apps: &[&str]| {
        Entry::new(keys, Some(language), None, Source::User, None, Some(apps.iter().map(|a| a.to_string()).collect()), None)
    };
    engine.add(Entry::user("gst", Language::Korean));
    engine.add(for_apps("gst", Language::English, &[slack]));
    engine.add(for_apps("gst", Language::Japanese, &["jetbrains."]));
    let read = |keys: &str, app: Option<&str>| engine.decide(keys, &DecisionContext::new(None, false, app)).language;
    assert_eq!(read("gst", None), Language::Korean);
    assert_eq!(read("gst", Some("notepad.exe")), Language::Korean);
    assert_eq!(read("gst", Some(slack)), Language::English);
    assert_eq!(read("gst", Some("jetbrains.intellij")), Language::Japanese);
    // The app's own entry is for the keys as a whole: no particle split from it.
    assert_ne!(engine.decide("gstdp", &DecisionContext::new(None, false, Some(slack))).output, "gst에");
    // Added again with the same keys and apps: changed, not a second entry.
    engine.add(Entry::user("gst", Language::English));
    assert_eq!(read("gst", None), Language::English);
    assert_eq!(engine.dictionary().entries().len(), 4);

    // Pinned: the learned word is the user's, and corrections leave it alone.
    let id = engine.dictionary().language_entry("dlrjs", None).unwrap().id();
    engine.pin(&id);
    assert_eq!(engine.dictionary().language_entry("dlrjs", None).unwrap().source, Source::User);
    engine.learn("dlrjs", Language::English, Some(Language::Korean));
    assert_eq!(engine.learned().get("dlrjs"), Some(&Language::Korean));
    // forget_all keeps it; remove takes it.
    engine.forget_all();
    assert_eq!(engine.learned().get("dlrjs"), Some(&Language::Korean));
    engine.remove(&id);
    assert!(!engine.learned().contains_key("dlrjs"));
}

/// The input method tells an open settings window when it saved a change, and only then.
#[test]
fn saved_is_called_after_writes() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let count = Arc::new(AtomicUsize::new(0));
    let engine = fixed_engine(vec![], false);
    let c = count.clone();
    engine.set_saved(move || {
        c.fetch_add(1, Ordering::Relaxed);
    });
    engine.learn("so", Language::Korean, Some(Language::English));
    assert_eq!(count.load(Ordering::Relaxed), 1);
    engine.learn("so", Language::Korean, Some(Language::English));
    engine.reload();
    engine.used("xx", Language::Korean); // nothing to change: nothing written
    assert_eq!(count.load(Ordering::Relaxed), 2);
}

/// Another process (another app's input method) wrote the file: it is read again before use.
#[test]
fn reloads_when_another_process_writes() {
    let url = temp_path("shared");
    let a = dictionary(shared_engine(), Some(url.clone()));
    let b = dictionary(shared_engine(), Some(url.clone()));
    a.learn("hello", Language::Korean, Some(Language::English));
    a.learn("hello", Language::Korean, Some(Language::English));
    b.reload_if_changed();
    assert_eq!(b.learned().get("hello"), Some(&Language::Korean));
    // b's own change starts from a's, not from what b read before. It lands in the same tick of
    // the file's time as a's last write (as two quick writes can): a notices it all the same.
    let written = std::fs::metadata(&url).and_then(|m| m.modified()).expect("a's write");
    b.learn("so", Language::Korean, Some(Language::English));
    std::fs::File::options().write(true).open(&url).and_then(|f| f.set_modified(written)).expect("same time");
    a.reload_if_changed();
    assert_eq!(a.learned().get("hello"), Some(&Language::Korean));
    assert_eq!(a.pending().get("so"), Some(&Language::Korean));
    let _ = std::fs::remove_file(url);
}

// SebeolsikTests: Korean on 3-set final.

fn compose3(keys: &str) -> Option<String> {
    KoreanLayout::with_override(KoreanLayout::SebeolsikFinal, || hangul::text(keys))
}

#[test]
fn composes_by_position() {
    for (keys, expected) in [
        ("jfsheamfncj4", "안녕하세요"), ("kf2uf", "갔다"), ("kkd", "끼"), ("j/f", "와"), ("i9tmr", "뭐해"), ("m8", "희"),
        ("jtXjt", "없어"), ("jd@uf", "읽다"), ("k/rsofSjf", "괜찮아"), ("kfxx", "갂"), ("kfxf", "각ㅏ"), ("jfshea!", "안녕ㄲ"),
    ] {
        assert_eq!(compose3(keys).as_deref(), Some(expected), "{keys}");
    }
}

#[test]
fn numbers_and_symbols_arent_korean() {
    assert_eq!(compose3("2026"), None); // a number stays a number
    assert_eq!(compose3("00"), None);
    assert_eq!(compose3("Hello"), None); // ⇧H is 0 in Korean: no jamo
    assert_eq!(compose3("kf?"), None);
}

#[test]
fn two_set_is_unchanged() {
    assert_eq!(hangul::text("dkssud").as_deref(), Some("안녕"));
    assert!(hangul::compose("2026").is_none());
}

#[test]
fn sebeolsik_decides() {
    KoreanLayout::with_override(KoreanLayout::SebeolsikFinal, || {
        let engine = splitting(shared_engine());
        let decide = |keys: &str| engine.decide(keys, &DecisionContext::default()).output;
        assert_eq!(decide("jfsheamfncj4"), "안녕하세요");
        assert_eq!(decide("nnfwjgw"), "쌀을");
        assert_eq!(decide("jd3yex"), "입력");
        assert_eq!(decide("0tpd"), "커피");
        assert_eq!(decide("hello"), "hello");
        assert_eq!(decide("world"), "world");
        assert_eq!(decide("githubjc"), "github에");
        assert_eq!(decide("2026"), "2026");
    });
}

/// Typing through a session, digits and symbols arriving as the app's printable keys.
fn type3(text: &str) -> String {
    KoreanLayout::with_override(KoreanLayout::SebeolsikFinal, || {
        let mut field = PlainTextField::new();
        let engine = dictionary(splitting(shared_engine()), Some(temp_path("3set")));
        let mut session = InputSession::new(engine, AppPolicies::default(), None, false, false, true);
        type_script(&mut session, &mut field, text);
        field.text
    })
}

#[test]
fn digits_and_symbols_type_jamo_inside_a_word() {
    assert_eq!(type3("jfshea hello "), "안녕 hello ");
    assert_eq!(type3("kf2uf "), "갔다 "); // 2 is the final ㅆ
    assert_eq!(type3(";f3 "), "밥 "); // a word starts on ; (ㅂ)
    assert_eq!(type3("0tpd "), "커피 "); // and on 0 (ㅋ)
    assert_eq!(type3("mr2uf. "), "했다. ");
}

#[test]
fn delete_takes_back_the_last_jamo() {
    KoreanLayout::with_override(KoreanLayout::SebeolsikFinal, || {
        let mut field = PlainTextField::new();
        let engine = dictionary(splitting(shared_engine()), Some(temp_path("3set")));
        let mut session = InputSession::new(engine, AppPolicies::default(), None, false, false, true);
        type_script(&mut session, &mut field, "kf2uf");
        session.handle(Key::Delete, &mut field);
        assert_eq!(field.text, "갔ㄷ");
        session.handle(Key::Delete, &mut field);
        session.handle(Key::Delete, &mut field);
        assert_eq!(field.text, "가");
    });
}

#[test]
fn symbols_that_dont_fit_end_the_word() {
    assert_eq!(type3("jfshea! "), "안녕! "); // ㄲ can't follow 녕's ㅇ: it is the !
    assert_eq!(type3("jfsheamfncj4! "), "안녕하세요! "); // 욖 isn't a syllable anyone types
    assert_eq!(type3(";f! "), "밖 "); // 밖 is
    assert_eq!(type3("hello! "), "hello! ");
    assert_eq!(type3("100 "), "100 "); // a number stays a number
    assert_eq!(type3("2026 "), "2026 ");
    assert_eq!(type3("2026 0tpd "), "2026 커피 "); // after a space 0 starts a word again
    assert_eq!(type3("it's "), "it's ");
}

// LatinLayoutTests: typing English and romaji on Colemak.

/// What a QWERTY 2-set typist's fingers type on `layout`: the same keys, that layout's letters.
fn keys_on(qwerty: &str, layout: LatinLayout) -> String {
    qwerty.chars().map(|c| layout.character_on_seat(c)).collect()
}

#[test]
fn colemak_moves_letters_and_keeps_seats() {
    let c = LatinLayout::Colemak;
    assert_eq!(keys_on("dkssud", c), "serrls");
    assert_eq!(c.character_on_seat(';'), 'o');
    assert_eq!(c.character_on_seat('p'), ';');
    assert_eq!(c.character_on_seat('P'), ':');
    assert_eq!(c.character_on_seat('E'), 'F');
    for seat in "qwertyuiopasdfghjkl;zxcvbnmQWERTYUIOPASDFGHJKL:ZXCVBNM".chars() {
        assert_eq!(c.seat_of(c.character_on_seat(seat)), seat);
    }
    assert_eq!(LatinLayout::ColemakDH.character_on_seat('g'), 'g');
    assert_eq!(LatinLayout::ColemakDH.character_on_seat('h'), 'm');
    assert_eq!(LatinLayout::ColemakDH.character_on_seat('v'), 'd');
    assert_eq!(LatinLayout::Qwerty.character_on_seat(';'), ';');
}

#[test]
fn seats_type_the_layouts_letters() {
    let c = LatinLayout::Colemak;
    assert_eq!(c.character_for_seat('e', false, false), Some('f')); // the e key
    assert_eq!(c.character_for_seat(';', true, false), Some('O')); // the ; key
    assert_eq!(c.character_for_seat('p', false, true), Some(';')); // Caps Lock leaves ; alone
    assert_eq!(c.character_for_seat(' ', false, false), None); // Space: not a letter key
    assert!(c.is_letter_seat(';'));
    assert!(!LatinLayout::Qwerty.is_letter_seat(';'));
}

#[test]
fn decides_the_same_on_every_layout() {
    for layout in LatinLayout::ALL {
        LatinLayout::with_override(layout, || {
            let engine = splitting(shared_engine());
            let decide = |keys: &str| engine.decide(keys, &DecisionContext::default()).output;
            assert_eq!(decide(&keys_on("dkssudgktpdy", layout)), "안녕하세요", "{layout:?}");
            assert_eq!(decide(&keys_on("Tkfdmf", layout)), "쌀을", "{layout:?}");
            assert_eq!(decide(&keys_on("dlqfur", layout)), "입력", "{layout:?}");
            assert_eq!(decide("hello"), "hello", "{layout:?}");
            assert_eq!(decide(&format!("github{}", keys_on("dp", layout))), "github에", "{layout:?}");
            assert_eq!(decide(&format!("QA{}", keys_on("fmf", layout))), "QA를", "{layout:?}");
        });
    }
}

#[test]
fn japanese_romaji_is_the_layouts_letters() {
    LatinLayout::with_override(LatinLayout::Colemak, || {
        let d = trilingual().decide("kyouhaiitenkidesune", &DecisionContext::default());
        assert_eq!(d.language, Language::Japanese, "{}", d.explanation);
    });
}

/// English on Colemak, romaji on QWERTY: the keys are Colemak's letters, and Japanese reads
/// each through QWERTY, so a QWERTY romaji typist's fingers still make kana.
#[test]
fn romaji_has_its_own_layout() {
    LatinLayout::with_override(LatinLayout::Colemak, || {
        LatinLayout::with_romaji_override(LatinLayout::Qwerty, || {
            let keys = keys_on("kyouhaiitenkidesune", LatinLayout::Colemak);
            assert_eq!(LatinLayout::romaji_keys(&keys), "kyouhaiitenkidesune");
            assert_eq!(romaji::compose(&keys, false).text, "きょうはいいてんきですね");
            let d = trilingual().decide(&keys, &DecisionContext::default());
            assert_eq!(d.language, Language::Japanese, "{}", d.explanation);
            assert_eq!(trilingual().decide("hello", &DecisionContext::default()).output, "hello");
        });
    });
    // English on QWERTY, romaji on Colemak: QWERTY's ; key is romaji's o.
    LatinLayout::with_romaji_override(LatinLayout::Colemak, || {
        assert!(LatinLayout::Qwerty.is_letter_seat(';'));
        let keys: String = "kyouha".chars().map(|c| LatinLayout::Colemak.seat_of(c)).collect();
        assert_eq!(keys, "no;iha");
        assert_eq!(romaji::compose(&keys, false).text, "きょうは");
    });
    assert!(!LatinLayout::Qwerty.is_letter_seat(';'));
}

#[test]
fn a_session_types_korean_and_english_on_colemak() {
    LatinLayout::with_override(LatinLayout::Colemak, || {
        let mut field = PlainTextField::new();
        let engine = dictionary(splitting(shared_engine()), Some(temp_path("layout")));
        let mut session = InputSession::new(engine, AppPolicies::default(), None, false, false, true);
        let script = format!("{} hello github{} ", keys_on("dkssud", LatinLayout::Colemak), keys_on("dp", LatinLayout::Colemak));
        for ch in script.chars() {
            let key = if ch == ' ' { Key::Printable(ch) } else { Key::Letter(ch) };
            if !session.handle(key, &mut field) {
                field.insert_text(&ch.to_string(), None);
            }
        }
        assert_eq!(field.text, "안녕 hello github에 ");
    });
}
