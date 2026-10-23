//! Per-client typing state: the word being composed, the language of the previous
//! word, the last committed word (so a switch key right after a commit can change it), and
//! an in-progress kana→kanji conversion.
//!
//! While a word is being composed:
//!   letters        extend it; the underlined preview shows the likely language
//!   switch key     cycle it: english → english+particle (github에) → korean → japanese
//!                  (the app's `switch_keys`: Esc, or `Key::Cycle` for the others). Where
//!                  Esc isn't one, or inside a vim that reports its modes, Esc commits and goes
//!                  to the app
//!   Delete         remove the last kana of a Japanese phrase; in Korean the last jamo of the
//!                  syllable being typed, then whole syllables (as the system's 2-set does);
//!                  else the last keystroke. The word keeps its language until the next letter.
//!   Space          English/Korean: commit, then a space. Japanese: convert to kanji
//!                  (`hold_space`: the space goes inside the underline instead, and the
//!                  word stays the word being typed until the next one starts; a symbol is held
//!                  the same way, and Space on a word held with a symbol commits it)
//!   0–9            with `full_width_digits`, after Japanese text or inside a phrase they are part
//!                  of it, full-width; otherwise the app types them
//!   symbols        , . ? ! - ' inside a Japanese phrase are part of it (、。？！ー); otherwise
//!                  a symbol commits the word, then types the key (`hold_space`: held like Space)
//!   Return         commit the word as shown, and nothing else (the next Return is the app's);
//!                  with `return_confirms_only` off the app gets it too, except after Japanese
//!   anything else  commit and let the app handle the key
//! While converting to kanji (candidate window open):
//!   Space / ↓      next candidate          ↑      previous candidate
//!   1–9            commit that candidate   Return commit the selected candidate
//!   Esc / Delete   back to kana            letters/punctuation  commit, then keep typing
//! Right after a word was committed with space/punctuation:
//!   switch keys    cycle that committed word the same way.
//!   Delete         the app deletes the space or punctuation; the word can still be changed.
//! A `Key::Cycle` with nothing to change is dropped, never passed to the app.
//! The same correction made twice is learned for good.

use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use crate::app_policy::{AppPolicies, AppPolicy, SwitchKey, STRICT_THRESHOLD};
use crate::converter::{ConversionCandidate, JapaneseConverter};
use crate::decision::{Decision, DecisionContext, DecisionEngine, Language};
use crate::english_model::EnglishModel;
use crate::hangul::{self, Unit};
use crate::japanese_model::{self, JapaneseModel};
use crate::japanese_segmenter::{JapaneseSegmenter, Segment};
use crate::jamo::DubeolsikLayout;
use crate::korean_model::{self, KoreanModel};
use crate::mixed_word::{self, SPLIT_CONFIDENCE};
use crate::romaji;
use crate::sebeolsik::{KoreanLayout, SebeolsikFinal};
use crate::text;
use crate::user_dictionary::DictionaryEngine;
use crate::vim::VimSignal;

/// A range of the text field in UTF-16 units, as NSString and TSF measure text.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TextRange {
    pub location: usize,
    pub length: usize,
}

impl TextRange {
    pub fn new(location: usize, length: usize) -> Self {
        TextRange { location, length }
    }

    pub fn end(self) -> usize {
        self.location + self.length
    }
}

/// The text field the input method is typing into. `None` stands for "not known" (NSNotFound).
pub trait TextClient {
    fn set_marked_text(&mut self, text: &str);
    /// Marked text in place of `range`, text the app already has (the marked text, if any, goes too).
    fn set_marked_text_replacing(&mut self, text: &str, range: TextRange);
    /// `range == None` replaces the marked text (or the selection if nothing is marked).
    fn insert_text(&mut self, text: &str, range: Option<TextRange>);
    fn selected_range(&mut self) -> Option<TextRange>;
    fn marked_range(&mut self) -> Option<TextRange>;
    fn text_in(&mut self, range: TextRange) -> Option<String>;
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Key {
    Letter(char),
    Delete,
    Escape,
    /// A switch key other than Esc (Shift+Esc, 한/영, a right Alt tap…), sent only where it is chosen.
    Cycle,
    /// Space, digit or punctuation typed by the input method.
    Printable(char),
    /// Shift+Space: a space of the other width (full-width where Space types a half-width one).
    ShiftSpace,
    Enter,
    Up,
    Down,
    /// ←/→: while converting, shorten or lengthen the first clause; otherwise like `Other`.
    Left,
    Right,
    /// Ctrl+K / F7 and Ctrl+J / F6: the Japanese reading as katakana / hiragana.
    Katakana,
    Hiragana,
    /// Tab, shortcuts…: commit and let the app handle the key.
    Other,
}

/// A kana→kanji conversion in progress.
#[derive(Clone, PartialEq, Debug)]
pub struct Conversion {
    /// The reading still being converted.
    pub kana: String,
    /// The romaji that produced `kana`, so Esc can go back to editing it.
    pub keys: String,
    pub candidates: Vec<ConversionCandidate>,
    pub selected: usize,
    /// The user chose Japanese for these keys with a switch key (going back to kana keeps that).
    user_forced: bool,
    /// Digits the app had already typed just before the phrase, taken into the reading
    /// (3ji → 3じ → 3時); not part of `keys`.
    number_prefix: String,
}

impl Conversion {
    pub fn current(&self) -> &ConversionCandidate {
        &self.candidates[self.selected]
    }

    /// What the user sees: the selected candidate, then any reading it doesn't cover.
    pub fn display(&self) -> String {
        let current = self.current();
        format!("{}{}", current.text, text::drop_first(&self.kana, current.kana_count))
    }
}

#[derive(Clone, Debug)]
struct Committed {
    keys: String,
    /// What Nunchi chose before any switch.
    decided: Language,
    /// What it went in as (a switch after the commit changes `language`).
    committed_as: Language,
    language: Language,
    /// Exactly what was inserted: the word plus its boundary character.
    text: String,
    boundary: String,
    location: Option<usize>,
    /// A segment of a URL or path: a switch there isn't a correction to learn.
    learns: bool,
}

impl Committed {
    /// The same word after Delete took back its boundary character.
    fn without_boundary(&self) -> Committed {
        let mut c = self.clone();
        c.text = text::drop_last(&self.text, text::len(&self.boundary)).to_string();
        c.boundary = String::new();
        c
    }
}

/// The last decision and what it was made from, for a typing test.
#[derive(Clone, Debug)]
pub struct Trace {
    pub keys: String,
    pub previous: Option<Language>,
    pub decision: Decision,
    /// Still being typed (the preview) rather than committed.
    pub partial: bool,
}

struct FinishedWord {
    keys: String,
    text: String,
    language: Language,
    decided: Language,
}

/// The preview only switches language mid-word when the new reading is at least this likely.
pub const PREVIEW_FLIP_CONFIDENCE: f64 = 0.7;
/// How far back the field is read for the run (UTF-16 units).
const RUN_LOOK_BEHIND: usize = 256;
/// What a number reading is made of: digits and the punctuation inside numbers, either width.
const NUMBER_CHARACTERS: &str = "0123456789０１２３４５６７８９－．，／：-.,/:";

fn is_number_character(c: char) -> bool {
    NUMBER_CHARACTERS.contains(c)
}

fn korean_model() -> KoreanModel {
    KoreanModel::new()
}

pub struct InputSession {
    pub engine: Arc<DictionaryEngine>,
    pub converter: Option<Rc<dyn JapaneseConverter>>,
    pub japanese_enabled: bool,
    keys: String,
    app: Option<String>,
    policy: AppPolicy,
    conversion: Option<Conversion>,
    policies: AppPolicies,
    forced: Option<Language>,
    /// Whether `forced` is the user's choice (a switch key) rather than "back to kana" after
    /// converting a phrase the engine read as Japanese anyway: only the user's choices are corrections.
    forced_by_user: bool,
    /// The language the engine itself showed last, before any switch: what a commit compares
    /// against, so a forced reading is never mistaken for the engine's own choice.
    engine_preview: Option<Language>,
    /// The last `decide` kept a short token (3 keys or fewer, like a command) English only
    /// because the app is set to Strict.
    kept_english_by_strict: bool,
    /// "henkan" came back from conversion as "henkan'" (the final n stays ん); dropped again
    /// if the word leaves Japanese, so English doesn't get an apostrophe nobody typed.
    added_n_marker: bool,
    previous_language: Option<Language>,
    last_commit: Option<Committed>,
    /// The boundary (Space or a symbol) the word was ended with in an app set to hold it
    /// (`hold_space`): the word stays composed, shown with that character, until the next
    /// word starts or another key finishes it.
    held_boundary: Option<char>,
    /// The mode vim last reported, nil when no vim is reporting. In normal mode every key goes
    /// straight through; while vim reports at all, Esc is vim's.
    vim_mode: Option<VimSignal>,
    /// The user paused Nunchi in this text field (the pause key): keys go through as typed
    /// until the next tap or another text field.
    paused: bool,
    /// Language of the current preview, so in-phrase symbols know where they belong.
    preview_language: Option<Language>,
    /// The last key passed to the app was a digit (so "." there is a decimal point).
    after_digit: bool,
    /// Digits and symbols the app just typed for us, one after another, and where they start.
    passed_run: Option<(String, usize)>,
    /// The word being typed started right after a number, in Japanese text: a counter (3ji → 3時).
    after_number_in_japanese: bool,
    /// The text gone in since the last Space, Tab or Return (`run_known`), or since the caret moved
    /// or the field changed, when the text before the caret is read the first time it matters.
    run_text: String,
    run_known: bool,
    /// The word being typed is a segment of a URL or path (`has_url_marker`): it goes in as typed.
    in_url_run: bool,
    /// Live conversion: a Japanese phrase is shown as its top kanji candidate while it is typed.
    pub live_conversion: bool,
    /// Return on an underlined word only confirms it (`Settings::return_confirms_only`).
    pub return_confirms_only: bool,
    /// The language shown when Delete was pressed, held until the next letter: a shorter word
    /// may read differently (한국 → 한구, not 한rn), but it shouldn't change under the user.
    pinned: Option<Language>,
    /// Where the Korean syllable being typed starts once Delete was used in the word: the system's
    /// 2-set input has committed everything before it, so those syllables delete whole and new
    /// jamo don't join them (세요 → 세ㅇ, not 셍). Nil until then: the word composes as one.
    syllable_start: Option<usize>,
    /// Digits after Japanese text, and inside a Japanese phrase, are part of it and full-width.
    pub full_width_digits: bool,
    /// A space after Japanese text is full-width.
    pub full_width_space: bool,
    /// A switch key showed the current phrase's kana instead (the next one switches language as usual).
    live_suppressed: bool,
    live_cache: Option<(String, Option<ConversionCandidate>)>,
    /// Full-width openers not closed yet in this sentence, so ) ] } close them full-width
    /// after an English word too.
    open_brackets: Vec<char>,
    /// The user's text replacements (shortcut → phrases).
    pub replacements: HashMap<String, Vec<String>>,
    /// Splits English words out of a Japanese phrase (google in kyouhagoogledekensaku).
    pub segmenter: Option<Arc<JapaneseSegmenter>>,
    last_trace: Option<Trace>,
    /// The last correction handed to the learning engine, for the word being typed or just
    /// committed: going back and forth is one correction.
    last_recorded: Option<(String, Language)>,
}

impl InputSession {
    pub fn new(
        engine: Arc<DictionaryEngine>,
        policies: AppPolicies,
        converter: Option<Rc<dyn JapaneseConverter>>,
        japanese_enabled: bool,
        live_conversion: bool,
        return_confirms_only: bool,
    ) -> Self {
        let segmenter = if japanese_enabled {
            JapaneseModel::shared().map(|model| Arc::new(JapaneseSegmenter::new(EnglishModel::shared(), model)))
        } else {
            None
        };
        InputSession {
            engine,
            converter,
            japanese_enabled,
            keys: String::new(),
            app: None,
            policy: policies.default_policy(),
            conversion: None,
            policies,
            forced: None,
            forced_by_user: false,
            engine_preview: None,
            kept_english_by_strict: false,
            added_n_marker: false,
            previous_language: None,
            last_commit: None,
            held_boundary: None,
            vim_mode: None,
            paused: false,
            preview_language: None,
            after_digit: false,
            passed_run: None,
            after_number_in_japanese: false,
            run_text: String::new(),
            run_known: false,
            in_url_run: false,
            live_conversion,
            return_confirms_only,
            pinned: None,
            syllable_start: None,
            full_width_digits: false,
            full_width_space: false,
            live_suppressed: false,
            live_cache: None,
            open_brackets: Vec::new(),
            replacements: HashMap::new(),
            segmenter,
            last_trace: None,
            last_recorded: None,
        }
    }

    pub fn keys(&self) -> &str {
        &self.keys
    }

    pub fn app(&self) -> Option<&str> {
        self.app.as_deref()
    }

    pub fn policy(&self) -> &AppPolicy {
        &self.policy
    }

    pub fn conversion(&self) -> Option<&Conversion> {
        self.conversion.as_ref()
    }

    pub fn paused(&self) -> bool {
        self.paused
    }

    pub fn last_trace(&self) -> Option<&Trace> {
        self.last_trace.as_ref()
    }

    /// Whether a word is being typed or converted (the input method holds marked text).
    pub fn is_composing(&self) -> bool {
        !self.keys.is_empty() || self.conversion.is_some()
    }

    fn set_previous(&mut self, language: Option<Language>) {
        self.previous_language = language;
        if language.is_none() {
            self.open_brackets.clear(); // a new context
        }
    }

    fn digits_full_width(&self) -> bool {
        self.japanese_enabled && self.full_width_digits && self.policy.full_width
    }

    /// A new text field became active: pick its app's policy and forget the sentence context.
    pub fn activate(&mut self, app: Option<&str>) {
        self.close_flip_window();
        let app = app.map(str::to_lowercase);
        // Coming back to the same terminal leaves vim in whatever mode it was.
        if app != self.app {
            self.vim_mode = None;
        }
        self.policy = self.policies.policy(app.as_deref());
        self.app = app;
        self.set_previous(None);
        self.passed_run = None;
        self.paused = false;
        self.reset_run();
    }

    /// Pauses or resumes Nunchi in this text field; what was being typed goes in first.
    pub fn toggle_pause(&mut self, client: Option<&mut dyn TextClient>) {
        if !self.paused {
            self.commit(client);
        }
        self.paused = !self.paused;
        self.set_previous(None);
        self.reset_run();
    }

    /// Vim's mode (the caller checks it came from this app).
    pub fn vim_changed(&mut self, signal: VimSignal, client: Option<&mut dyn TextClient>) {
        if signal == VimSignal::Normal && self.vim_mode != Some(VimSignal::Normal) {
            self.commit(client);
            self.set_previous(None);
            self.reset_run();
        }
        self.vim_mode = if signal == VimSignal::Off { None } else { Some(signal) };
    }

    /// Whether keys currently go straight to vim (normal, visual or command-line mode).
    pub fn in_vim_normal_mode(&self) -> bool {
        self.vim_mode == Some(VimSignal::Normal)
    }

    /// Returns true when the key was consumed.
    pub fn handle(&mut self, key: Key, client: &mut dyn TextClient) -> bool {
        if !self.policy.enabled || self.paused {
            return false;
        }
        if self.vim_mode == Some(VimSignal::Normal) {
            // Vim's normal mode: every key is a command. A switch key is the way out if vim's last
            // word was lost (vim not seeing the terminal's focus change): it only says "decide words".
            if key == Key::Cycle {
                self.vim_mode = None;
                return true;
            }
            self.set_previous(None);
            self.reset_run();
            return false;
        }
        if key == Key::ShiftSpace {
            // With nothing being typed, the other width: full-width unless Space already types that.
            if !(self.keys.is_empty() && self.conversion.is_none() && self.japanese_enabled && self.policy.full_width) {
                return self.handle(Key::Printable(' '), client);
            }
            self.close_flip_window();
            if self.full_width_space && self.previous_language == Some(Language::Japanese) {
                self.note_run(" ");
                return false; // the app types " "
            }
            client.insert_text("\u{3000}", None);
            self.note_run("\u{3000}");
            return true;
        }
        if self.conversion.is_some() {
            return self.handle_converting(key, client);
        }
        // On 3-set a digit or symbol key with a jamo goes on with the word where the jamo fits
        // (갔 is kf2, 밥 starts on ;); elsewhere it is the symbol. Not in a URL or path, though the
        // word itself doesn't make one: 밝 is ;f@, not an address.
        if let Key::Printable(ch) = key {
            if self.held_boundary.is_none()
                && (self.keys.is_empty() || !self.in_url_run)
                && KoreanLayout::current().continues_word(&self.keys, ch, self.after_digit, |s| {
                    korean_model().knows_syllable(s)
                })
            {
                let run = self.run(client);
                if !Self::has_url_marker(&run) {
                    return self.handle(Key::Letter(ch), client);
                }
            }
        }

        if !(matches!(key, Key::Printable(_)) && self.keys.is_empty()) {
            self.after_digit = false;
        }
        if !matches!(key, Key::Letter(_) | Key::Printable(_) | Key::Escape | Key::Cycle) {
            self.passed_run = None;
        }
        let pending = self.last_commit.take();
        // Esc switches languages where it is a switch key (and no reporting vim owns it); the
        // other switch keys arrive as `Cycle` only where they are chosen.
        let switches_language = key == Key::Cycle
            || (key == Key::Escape && self.policy.switch_keys.contains(&SwitchKey::Escape) && self.vim_mode.is_none());
        if switches_language && self.keys.is_empty() {
            if let Some(pending) = &pending {
                if let Some(changed) = self.cycle(pending, client) {
                    self.last_commit = Some(changed);
                    return true;
                }
            }
        }
        // Delete right after the commit: the app takes back the space or punctuation, and the
        // word itself can still be changed (Space before looking, then Delete and a switch key).
        if key == Key::Delete && self.keys.is_empty() {
            if let Some(pending) = &pending {
                if !pending.boundary.is_empty() {
                    self.last_commit = Some(pending.without_boundary());
                    self.note_deleted();
                    return false;
                }
            }
        }
        if let Some(pending) = &pending {
            self.finalize(pending);
        }
        if switches_language && self.keys.is_empty() {
            if let Some((text, range)) = self.passed_run_before_caret(client) {
                self.passed_run = None;
                self.start_symbol(&text, client, Some(range));
                return true;
            }
        }

        match key {
            Key::Letter(ch) => {
                if self.held_boundary.is_some() {
                    self.commit(Some(client)); // the held word goes in with its boundary; a new word starts
                }
                // A symbol on its own is confirmed by the next word, which is judged by itself.
                if !self.keys.is_empty()
                    && self.keys.chars().all(|c| romaji::symbol(c).is_some())
                    && !(KoreanLayout::current() == KoreanLayout::SebeolsikFinal
                        && self.keys.chars().all(|c| SebeolsikFinal::jamo_for(c).is_some()))
                {
                    self.commit(Some(client)); // (3-set's ; is ㅂ, a word on its way)
                }
                if self.keys.is_empty() {
                    // a new word
                    self.last_recorded = None;
                    self.live_suppressed = false;
                    self.after_number_in_japanese = self.japanese_enabled
                        && self.previous_language == Some(Language::Japanese)
                        && self.passed_run.as_ref().and_then(|r| text::last(&r.0)).is_some_and(|c| c.is_numeric())
                        && client
                            .selected_range()
                            .is_some_and(|cursor| self.passed_run_range(cursor.location, client).is_some());
                    let run = self.run(client);
                    self.in_url_run = Self::has_url_marker(&run);
                }
                self.added_n_marker = false;
                self.pinned = None;
                self.keys.push(ch);
                self.update_preview(client);
                true
            }

            Key::Delete => {
                if self.held_boundary.is_some() {
                    // Only the boundary comes back; the word is still there to be changed.
                    self.held_boundary = None;
                    self.update_preview(client);
                    return true;
                }
                if self.keys.is_empty() {
                    self.note_deleted();
                    return false;
                }
                let shown = self.forced.or(self.pinned).or(self.preview_language);
                if self.forced.is_none() {
                    self.pinned = shown;
                }
                match shown {
                    Some(Language::Japanese) => self.delete_kana(),
                    Some(Language::Korean) => self.delete_jamo_or_syllable(),
                    _ => {
                        self.keys.pop();
                    }
                }
                self.added_n_marker = false;
                if self.keys.is_empty() {
                    self.forced = None;
                    self.forced_by_user = false;
                    self.pinned = None;
                    self.syllable_start = None;
                }
                self.update_preview(client);
                true
            }

            Key::Escape | Key::Cycle => {
                // A chosen Shift+Esc is Nunchi's own: with nothing to change it goes nowhere, rather
                // than reaching the app as an Esc (in a terminal, Claude Code's "cancel the draft").
                if self.keys.is_empty() {
                    return key == Key::Cycle;
                }
                let shown_japanese = self.forced.or(self.preview_language) == Some(Language::Japanese);
                // A live-converted phrase goes back to its kana first, also with an Esc that isn't a
                // switch key (not inside vim: Esc is vim's there).
                if self.is_live() && shown_japanese && (switches_language || self.vim_mode.is_none()) {
                    self.live_suppressed = true;
                    self.update_preview(client);
                    return true;
                }
                // Kana on screen: the app's Esc cancels the phrase, as in a Japanese IME,
                // rather than committing it and closing a window in the app.
                if !switches_language && self.vim_mode.is_none() && shown_japanese {
                    self.cancel_composition(client);
                    return true;
                }
                if !switches_language {
                    // The app's Esc: the word goes in as shown, then the app gets the key.
                    self.commit(Some(client));
                    return false;
                }
                // Cycle from what the user sees.
                let shown = match self.forced.or(self.pinned).or(self.preview_language) {
                    Some(l) => l,
                    None => self.decide(true).language,
                };
                let splits = self.engine_splits(&self.keys.clone());
                self.forced = Some(shown.next(&self.keys, self.japanese_enabled, Some(splits)));
                self.pinned = None;
                self.forced_by_user = true;
                if self.forced != Some(Language::Japanese) {
                    self.drop_n_marker();
                }
                self.update_preview(client);
                true
            }

            Key::Printable(ch) => self.handle_printable(ch, client),

            Key::Enter => {
                if self.keys.is_empty() {
                    self.note_run("\n");
                    return false;
                }
                // Like a Japanese IME, Return on an underlined word only confirms it; the next Return
                // is the app's. Passed on (the setting off), terminals and web chats may still drop a
                // Return that ended a composition. A Japanese phrase is only confirmed either way.
                let japanese = self.current_language() == Language::Japanese;
                self.commit(Some(client));
                self.note_run("\n");
                self.return_confirms_only || japanese
            }

            Key::Katakana | Key::Hiragana => {
                // Only for a Japanese phrase; otherwise Ctrl+K/Ctrl+J stay the app's shortcuts.
                if self.held_boundary.is_some() {
                    self.commit(Some(client));
                    return false; // a held word is done: the shortcut is the app's
                }
                if self.keys.is_empty()
                    || !self.japanese_enabled
                    || !(self.forced == Some(Language::Japanese) || self.preview_language == Some(Language::Japanese))
                {
                    self.commit(Some(client));
                    return false;
                }
                self.show_kana_forms(key == Key::Katakana, None, None, client);
                true
            }

            Key::Up | Key::Down | Key::Left | Key::Right | Key::Other => {
                self.commit(Some(client));
                self.set_previous(None);
                self.reset_run();
                false
            }

            Key::ShiftSpace => false, // handled above
        }
    }

    fn handle_printable(&mut self, ch: char, client: &mut dyn TextClient) -> bool {
        if let Some(held) = self.held_boundary {
            // A held word is still the word being typed: Space on it in Japanese (after a switch)
            // converts, as it would have before; the held space was never a Japanese one. A held
            // symbol ended the word for good: Space then commits it, no conversion.
            if ch == ' ' && held == ' ' && self.current_language() == Language::Japanese && self.converter.is_some() {
                self.held_boundary = None;
                self.start_conversion(client);
                return true;
            }
            self.commit(Some(client)); // the held word goes in with its boundary; the key follows as after any commit
        }
        if self.keys.is_empty() {
            let handled = self.printable_alone(ch, client);
            self.after_digit = ch.is_numeric();
            return handled;
        }
        // A symbol that makes the run a URL or path (ho@, www.) ends the word, half-width.
        let ends_in_url = self.in_url_run || {
            let run = self.run(client);
            Self::has_url_marker(&format!("{run}{}{ch}", self.keys))
        };
        if self.japanese_enabled && !ends_in_url && self.belongs_to_japanese_phrase(ch) {
            self.keys.push(ch);
            // Digits don't read as anything: the phrase stays Japanese, as it was shown.
            if ch.is_numeric() && self.forced.is_none() {
                self.forced = Some(Language::Japanese);
                self.forced_by_user = false;
            }
            self.update_preview(client);
            return true;
        }
        if ch == ' ' && self.current_language() == Language::Japanese && self.converter.is_some() {
            self.start_conversion(client);
            return true;
        }
        if !ends_in_url && self.closes_full_width(ch) {
            // 日本語（English）
            self.commit(Some(client));
            self.start_symbol(&ch.to_string(), client, None);
            return true;
        }
        if self.policy.hold_space && (ch == ' ' || Self::is_boundary_symbol(ch)) {
            // The word stays underlined with its boundary, shown as it would be committed (the
            // whole word's decision, not the last keystroke's preview), so a switch key still
            // changes it and Delete takes back the boundary. The next word commits both.
            let language = self.current_language();
            if language != Language::Japanese {
                if self.forced.is_none() {
                    self.pinned = Some(language);
                }
                self.held_boundary = Some(ch);
                self.update_preview(client);
                return true;
            }
        }
        let location = match client.marked_range() {
            Some(marked) => Some(marked.location),
            None => client.selected_range().map(|r| r.location),
        };
        self.passed_run = None;
        let word = self.finish_word();
        // Only the word goes in; the app types the space or punctuation itself, as with the
        // system's Korean input. Handing over "word + space" made some terminals add a stray
        // character.
        client.insert_text(&word.text, None);
        let committed_text = format!("{}{ch}", word.text);
        self.last_commit = Some(Committed {
            keys: word.keys,
            decided: word.decided,
            committed_as: word.language,
            language: word.language,
            text: committed_text.clone(),
            boundary: ch.to_string(),
            location,
            learns: !self.in_url_run,
        });
        self.note_run(&committed_text);
        false
    }

    /// A printable key with nothing being typed.
    fn printable_alone(&mut self, ch: char, client: &mut dyn TextClient) -> bool {
        // Inside a URL or path symbols stay half-width, whatever came before: ほ@gmail.com.
        let run = self.run(client);
        if Self::has_url_marker(&format!("{run}{ch}")) {
            self.passed_run = None;
            self.note_run(&ch.to_string());
            return false;
        }
        if self.previous_language == Some(Language::Japanese) {
            if ch == ' ' && self.full_width_space && self.japanese_enabled && self.policy.full_width {
                client.insert_text("\u{3000}", None);
                return true;
            }
            // A number after Japanese text starts a Japanese word: １２３, ３がつ → ３月.
            if ch.is_ascii_digit() && self.digits_full_width() {
                self.start_symbol(&ch.to_string(), client, None);
                return true;
            }
        }
        // After Japanese text a symbol is Japanese too (、。？！「」・…), underlined until Return
        // or the next word confirms it, as in a Japanese IME. Not inside a number (3.5, 10:30),
        // but a bracket closes the way it was opened.
        if romaji::symbol(ch).is_some()
            && ((self.previous_language == Some(Language::Japanese) && !self.after_digit) || self.closes_full_width(ch))
        {
            self.start_symbol(&ch.to_string(), client, None);
            return true;
        }
        self.note_passed(ch, client);
        self.note_run(&ch.to_string());
        false
    }

    /// A modifier key that starts shortcuts (Ctrl or Win) went down. A held word goes in with
    /// its boundary now, before the shortcut: an app that acts on the shortcut itself never
    /// shows the input method the key and would paste over the underlined text.
    pub fn command_pressed(&mut self, client: &mut dyn TextClient) {
        if self.held_boundary.is_none() {
            return;
        }
        self.commit(Some(client));
        self.set_previous(None);
        self.reset_run();
    }

    /// Commits whatever is in progress as-is (focus change, shortcut, deactivation).
    pub fn commit(&mut self, mut client: Option<&mut dyn TextClient>) {
        self.close_flip_window();
        if let Some(conversion) = self.conversion.take() {
            let display = conversion.display();
            if let Some(client) = client.as_deref_mut() {
                client.insert_text(&display, None);
            }
            self.note_run(&display);
            self.note_brackets(&display);
            if let Some(converter) = &self.converter {
                converter.learn(conversion.current(), &conversion.kana);
            }
            self.keys.clear();
            self.set_previous(Some(Language::Japanese));
            return;
        }
        if self.keys.is_empty() {
            self.held_boundary = None;
            return;
        }
        let boundary = self.held_boundary.take().map(String::from).unwrap_or_default(); // a held word goes in with its space or symbol
        let word = self.finish_word();
        let text = word.text + &boundary;
        if let Some(client) = client {
            client.insert_text(&text, None);
        }
        self.note_run(&text);
    }

    /// Drops the word being typed: nothing goes in, as when Delete takes its last key.
    fn cancel_composition(&mut self, client: &mut dyn TextClient) {
        self.keys.clear();
        self.held_boundary = None;
        self.forced = None;
        self.forced_by_user = false;
        self.pinned = None;
        self.syllable_start = None;
        self.added_n_marker = false;
        self.live_suppressed = false;
        self.update_preview(client);
    }

    /// Commits what's in progress after the caret was moved elsewhere (a click; the app asks):
    /// the next word doesn't continue the previous sentence.
    pub fn commit_for_new_context(&mut self, client: Option<&mut dyn TextClient>) {
        self.commit(client);
        self.set_previous(None);
        self.reset_run();
    }

    /// The app ended the composition itself (a click elsewhere): what it shows stays as it is,
    /// and the session starts over without inserting anything.
    pub fn composition_terminated(&mut self) {
        self.close_flip_window();
        if self.conversion.take().is_some() {
            self.keys.clear();
        } else if !self.keys.is_empty() {
            let _ = self.finish_word();
        }
        self.held_boundary = None;
        self.set_previous(None);
        self.reset_run();
    }

    /// Commits candidate `index` of the open conversion (e.g. clicked in the candidate window).
    pub fn choose(&mut self, index: usize, client: &mut dyn TextClient) {
        let Some(conversion) = &mut self.conversion else { return };
        if index >= conversion.candidates.len() {
            return;
        }
        conversion.selected = index;
        self.commit_candidate(client);
    }

    // MARK: - URLs and paths

    /// Whether a run of text without spaces is a URL or path from here on: `://`, `www.`, `~/`,
    /// `./` or `../` at its start, a path that starts with `/` and has another, `@` after
    /// something (an email; `@홍길동` is a mention) or `localhost:`. A lone `.` or `/` isn't one:
    /// L.A.로, TV/라디오 and 남/여 are Korean.
    pub fn has_url_marker(run: &str) -> bool {
        run.contains("://")
            || run.contains("localhost:")
            || ["www.", "~/", "./", "../"].iter().any(|p| run.starts_with(p))
            || (run.starts_with('/') && run[1..].contains('/'))
            || text::drop_first(run, 1).contains('@')
    }

    /// The run of text before the word being typed: what went in since the last space, with
    /// what the field had before the caret when that isn't known (a pasted URL, a click).
    fn run(&mut self, client: &mut dyn TextClient) -> String {
        if self.run_known {
            return self.run_text.clone();
        }
        self.run_known = true;
        let marked = client.marked_range();
        let cursor = client.selected_range();
        // Up to the word being typed, if there is one on screen.
        let end = match (marked, cursor) {
            (Some(m), _) => Some(m.location),
            (None, Some(c)) if c.length == 0 => Some(c.location),
            _ => None,
        };
        let Some(end) = end else { return self.run_text.clone() };
        let start = end.saturating_sub(RUN_LOOK_BEHIND);
        let Some(before) = client.text_in(TextRange::new(start, end - start)) else { return self.run_text.clone() };
        let field: String = {
            let tail: Vec<char> = before.chars().rev().take_while(|c| !c.is_whitespace()).collect();
            tail.into_iter().rev().collect()
        };
        if field.ends_with(&self.run_text) {
            self.run_text = field;
        }
        self.run_text.clone()
    }

    /// Keeps the run up to date with text going in; Space, Tab and Return end it. A URL or path
    /// counts as English for the next word.
    fn note_run(&mut self, text: &str) {
        for ch in text.chars() {
            if ch.is_whitespace() {
                if Self::has_url_marker(&self.run_text) {
                    self.set_previous(Some(Language::English));
                }
                self.run_text.clear();
                self.run_known = true;
            } else {
                self.run_text.push(ch);
            }
        }
    }

    /// The app deleted the character before the caret, nothing being typed.
    fn note_deleted(&mut self) {
        if self.run_text.is_empty() {
            self.run_known = false;
        } else {
            self.run_text.pop();
        }
    }

    /// The caret moved or the field changed: what is before the caret is read again when needed.
    pub fn reset_run(&mut self) {
        self.run_text.clear();
        self.run_known = false;
    }

    // MARK: - Japanese conversion

    /// A symbol typed as its own Japanese word, shown full-width (or as the 「」・ a Japanese IME types).
    /// With `range`, it takes the place of what the app typed there, in one step.
    fn start_symbol(&mut self, symbol: &str, client: &mut dyn TextClient, range: Option<TextRange>) {
        self.last_recorded = None;
        self.live_suppressed = false;
        self.keys = symbol.to_string();
        self.forced = Some(Language::Japanese);
        self.forced_by_user = false;
        let Some(range) = range else { return self.update_preview(client) };
        self.preview_language = Some(Language::Japanese);
        let display = self.japanese_display();
        client.set_marked_text_replacing(&display, range);
    }

    /// Keeps `passed_run` up to date with a key the app is about to type.
    fn note_passed(&mut self, ch: char, client: &mut dyn TextClient) {
        let cursor = client.selected_range();
        let ok = self.japanese_enabled
            && self.policy.full_width
            && ch.is_ascii()
            && (ch.is_ascii_digit() || romaji::symbol(ch).is_some());
        let Some(cursor) = cursor.filter(|c| ok && c.length == 0) else {
            self.passed_run = None;
            return;
        };
        match &self.passed_run {
            Some((text, location)) if location + text::utf16_len(text) == cursor.location => {
                self.passed_run = Some((format!("{text}{ch}"), *location));
            }
            _ => self.passed_run = Some((ch.to_string(), cursor.location)),
        }
    }

    /// The range of `passed_run` when it is still in the text, unchanged, ending at `end`.
    fn passed_run_range(&self, end: usize, client: &mut dyn TextClient) -> Option<TextRange> {
        let (text, location) = self.passed_run.as_ref()?;
        let range = TextRange::new(*location, text::utf16_len(text));
        (range.end() == end && client.text_in(range).as_deref() == Some(text.as_str())).then_some(range)
    }

    /// The digits and symbols just typed, and where they are, when the caret is right after them.
    fn passed_run_before_caret(&self, client: &mut dyn TextClient) -> Option<(String, TextRange)> {
        if !self.japanese_enabled || self.conversion.is_some() {
            return None;
        }
        let (text, _) = self.passed_run.as_ref()?;
        let cursor = client.selected_range().filter(|c| c.length == 0)?;
        let range = self.passed_run_range(cursor.location, client)?;
        Some((text.clone(), range))
    }

    fn closing_opener(ch: char) -> Option<char> {
        match ch {
            ')' => Some('（'),
            ']' => Some('「'),
            '}' => Some('｛'),
            _ => None,
        }
    }

    fn pair_opener(ch: char) -> Option<char> {
        match ch {
            '）' => Some('（'),
            '」' => Some('「'),
            '｝' => Some('｛'),
            '』' => Some('『'),
            '】' => Some('【'),
            _ => None,
        }
    }

    fn closes_full_width(&self, ch: char) -> bool {
        Self::closing_opener(ch).is_some_and(|opener| self.open_brackets.last() == Some(&opener))
    }

    /// Keeps `open_brackets` up to date with text going in.
    fn note_brackets(&mut self, text: &str) {
        for ch in text.chars() {
            if matches!(ch, '（' | '「' | '｛' | '『' | '【') {
                self.open_brackets.push(ch);
            } else if let Some(opener) = Self::pair_opener(ch) {
                if self.open_brackets.last() == Some(&opener) {
                    self.open_brackets.pop();
                }
            }
        }
    }

    /// Whether a symbol typed after the current word continues a Japanese phrase.
    /// Sentence punctuation (、。？！) never changes the word's language: it joins only a
    /// word already read as Japanese ("hello." and "dho?" = 왜? stay what they were).
    /// The in-word marks ー and ' may also turn an English-looking start into Japanese
    /// ("ra" → "ra-" → らー), but never take a word away from Korean.
    fn belongs_to_japanese_phrase(&mut self, ch: char) -> bool {
        let pending_is_n = |keys: &str| {
            let p = romaji::compose(keys, true).pending.to_lowercase();
            p.is_empty() || p == "n"
        };
        if ch.is_ascii_digit() {
            if !self.digits_full_width() {
                return false;
            }
            return self.forced == Some(Language::Japanese)
                || (self.forced.is_none() && self.preview_language == Some(Language::Japanese) && pending_is_n(&self.keys));
        }
        if !japanese_model::is_in_phrase_symbol(ch) {
            return false;
        }
        if self.forced == Some(Language::Japanese) {
            return true;
        }
        // On the way to a Japanese text replacement shortcut with a symbol in it: gu- is ぐー,
        // even though gu alone read as 혀.
        if self.japanese_enabled && self.forced.is_none() {
            let kana = romaji::compose(&format!("{}{ch}", self.keys), true);
            if kana.is_clean() && self.replacements.keys().any(|k| k.starts_with(&kana.text)) {
                return true;
            }
        }
        // A letter still waiting for its vowel says the word may not be Japanese after all
        // (baz) is baz), not ばz）); a final n is ん (nihon. is にほん。).
        if self.preview_language == Some(Language::Japanese) {
            return ch == '-' || ch == '\'' || pending_is_n(&self.keys);
        }
        // An English word before the mark keeps it English (to-do, co-op); ra-men isn't one.
        if !(self.forced.is_none()
            && (ch == '-' || ch == '\'')
            && self.preview_language == Some(Language::English)
            && !EnglishModel::shared().is_known_word(&self.keys)
            && romaji::compose(&self.keys, true).stray_letters == 0)
        {
            return false;
        }
        let context = DecisionContext::new(self.previous_language, true, self.app.as_deref());
        self.engine.decide(&format!("{}{ch}", self.keys), &context).language == Language::Japanese
    }

    /// The Korean or Japanese reading of the keys that is a text replacement shortcut (주소, ㅈㅅ,
    /// よろ), unless the user taught Nunchi otherwise for these keys. English shortcuts are left
    /// to the engine.
    fn shortcut_reading(&self, partial: bool) -> Option<(Language, String)> {
        if self.replacements.is_empty() || self.keys.is_empty() || self.engine.has_learned(&self.keys) {
            return None;
        }
        if let Some(hangul) = self.hangul() {
            if self.replacements.contains_key(&hangul) {
                return Some((Language::Korean, hangul));
            }
        }
        if self.japanese_enabled {
            let kana = romaji::compose(&self.keys, partial);
            if kana.is_clean() && self.replacements.contains_key(&kana.text) {
                return Some((Language::Japanese, kana.text));
            }
        }
        None
    }

    /// Kana for the keys, with any English words inside the phrase kept as typed.
    fn japanese_reading(&self, partial: bool) -> String {
        // An English word with a particle (redde → redで) even when every key also makes kana,
        // as the engine split it; a reading the user picked by hand stays kana (れっで).
        if !(self.forced == Some(Language::Japanese) && self.forced_by_user) {
            if let Some((word, particle)) = self.segmenter.as_ref().and_then(|s| s.english_with_particle(&self.keys)) {
                return word + &particle;
            }
        }
        if !partial {
            // Letters still waiting for their vowel when the phrase ends go in as shown
            // (watashihag → わたしはg, not わたしhag); only n becomes ん.
            let typed = romaji::compose(&self.keys, true);
            if typed.stray_letters == 0 && !typed.pending.is_empty() {
                return romaji::compose(&self.keys, false).text;
            }
            if let Some(segments) = self.segmenter.as_ref().and_then(|s| s.segment(&self.keys)) {
                return JapaneseSegmenter::text(&segments);
            }
        }
        // While typing, a consonant at the end is just waiting for its vowel (watashihag → わたしはg),
        // not a stray letter to split an English word ("hag") out of.
        if partial && romaji::compose(&self.keys, true).stray_letters > 0 {
            if let Some(segments) = self.segmenter.as_ref().and_then(|s| s.segment(&self.keys)) {
                // While typing, the last segment may still be growing; keep its pending letters visible.
                let (last, front) = segments.split_last().expect("segments");
                let tail = match last {
                    Segment::Japanese { keys, .. } => romaji::compose(keys, true).text,
                    Segment::English(_) => JapaneseSegmenter::text(std::slice::from_ref(last)),
                };
                return JapaneseSegmenter::text(front) + &tail;
            }
        }
        romaji::compose(&self.keys, partial).text
    }

    // MARK: - Live conversion

    fn is_live(&self) -> bool {
        self.live_conversion && !self.live_suppressed && self.converter.is_some()
    }

    /// The top candidate covering all of `kana` (cached: the preview asks on every key).
    fn live_candidate(&mut self, kana: &str) -> Option<ConversionCandidate> {
        if let Some((cached, candidate)) = &self.live_cache {
            if cached == kana {
                return candidate.clone();
            }
        }
        let count = text::len(kana);
        let mut candidate =
            self.conversion_candidates(kana).into_iter().find(|c| c.kana_count == count && !c.is_prediction);
        // A short reading only turns into kanji (私, 今日), not into katakana mid-word
        // (と → ト, こんに → コンに): those flicker while the phrase is still being typed.
        if count <= 3 {
            if let Some(c) = &candidate {
                if !c.text.chars().any(|s| ('\u{4E00}'..='\u{9FFF}').contains(&s)) {
                    candidate = None;
                }
            }
        }
        self.live_cache = Some((kana.to_string(), candidate.clone()));
        candidate
    }

    /// The Japanese phrase as shown while typing: kana, or with live conversion the top
    /// candidate followed by letters still waiting for their vowel (今日はいい天気でs).
    fn japanese_display(&mut self) -> String {
        let reading = self.japanese_reading(true);
        if !self.is_live() {
            return reading;
        }
        let pending = romaji::compose(&self.keys, true).pending;
        if !reading.ends_with(&pending) {
            return reading;
        }
        let kana = text::drop_last(&reading, text::len(&pending)).to_string();
        if kana.is_empty() {
            return reading;
        }
        match self.live_candidate(&kana) {
            Some(candidate) => candidate.text + &pending,
            None => reading,
        }
    }

    /// What a Japanese phrase commits as without the candidate window: the kana, or with
    /// live conversion the candidate that was on screen (which the converter learns).
    fn committed_japanese(&mut self) -> String {
        let reading = self.japanese_reading(false);
        // Letters still waiting for their vowel follow the candidate, as japanese_display shows
        // them (今日はいい天気でs, not a candidate for the whole reading with the s in it).
        let pending = romaji::compose(&self.keys, true).pending;
        let kana = if reading.ends_with(&pending) {
            text::drop_last(&reading, text::len(&pending)).to_string()
        } else {
            reading.clone()
        };
        let letters = text::drop_first(&reading, text::len(&kana)).to_string();
        if !self.is_live() || kana.is_empty() {
            return reading;
        }
        let Some(candidate) = self.live_candidate(&kana) else { return reading };
        if let Some(converter) = &self.converter {
            converter.learn(&candidate, &kana);
        }
        candidate.text + &letters
    }

    fn start_conversion(&mut self, client: &mut dyn TextClient) {
        let mut kana = self.japanese_reading(false);
        if self.converter.is_none() || kana.is_empty() {
            return;
        }
        let user_forced = self.forced == Some(Language::Japanese) && self.forced_by_user;
        if user_forced && !self.in_url_run {
            let engine_choice = self.committed_language();
            self.record(&self.keys.clone(), Language::Japanese, engine_choice);
        }
        // A number the app typed just before the phrase is part of it: 3ji is 3時, not 3 and 時
        // picked out of 地, 字, 自…
        let mut prefix_range: Option<TextRange> = None;
        if let Some(marked) = client.marked_range() {
            if let Some((run, _)) = &self.passed_run {
                if text::last(run).is_some_and(|c| c.is_numeric()) && run.chars().all(is_number_character) {
                    if let Some(range) = self.passed_run_range(marked.location, client) {
                        prefix_range = Some(TextRange::new(range.location, range.length + marked.length));
                        kana = format!("{run}{kana}");
                    }
                }
            }
        }
        let number_prefix = if prefix_range.is_some() {
            self.passed_run.as_ref().map(|r| r.0.clone()).unwrap_or_default()
        } else {
            String::new()
        };
        self.passed_run = None;
        let mut candidates = self.conversion_candidates(&kana);
        if !number_prefix.is_empty() {
            // The number as it was typed (3時) before other widths and spellings (３時, 三時).
            let (first, rest): (Vec<_>, Vec<_>) = candidates.into_iter().partition(|c| c.text.starts_with(&number_prefix));
            candidates = first.into_iter().chain(rest).collect();
        }
        // Live conversion showed the phrase without the number, so no candidate is on screen yet.
        let live = if self.is_live() && number_prefix.is_empty() { self.live_candidate(&kana) } else { None };
        if let Some(phrases) = self.replacements.get(&kana).filter(|p| !p.is_empty()) {
            // Text replacements come first, or right after the live candidate already on screen.
            let at = live.as_ref().and_then(|l| candidates.iter().position(|c| c == l)).map(|i| i + 1).unwrap_or(0);
            let count = text::len(&kana);
            let added: Vec<ConversionCandidate> = phrases.iter().map(|p| ConversionCandidate::new(p.clone(), count)).collect();
            let before: Vec<_> = candidates[..at].iter().filter(|c| !phrases.contains(&c.text)).cloned().collect();
            let after: Vec<_> = candidates[at..].iter().filter(|c| !phrases.contains(&c.text)).cloned().collect();
            candidates = before.into_iter().chain(added).chain(after).collect();
        }
        // Every other homophone the engine has follows, for the candidate window (しょう → 硝).
        if let Some(converter) = &self.converter {
            let listed: std::collections::HashSet<String> = candidates.iter().map(|c| c.text.clone()).collect();
            candidates.extend(converter.more_candidates(&kana).into_iter().filter(|c| !listed.contains(&c.text)));
        }
        if candidates.is_empty() {
            self.commit(Some(client));
            return;
        }
        // With live conversion the top candidate is already on screen: Space shows the next one.
        let mut selected = 0;
        if let Some(i) = live.as_ref().and_then(|l| candidates.iter().position(|c| c == l)).filter(|_| candidates.len() > 1) {
            selected = (i + 1) % candidates.len();
        } else if self.live_conversion && number_prefix.is_empty() && candidates.len() > 1 && candidates[0].text == kana {
            // The kana is on screen and the engine put it first (しょう): Space shows a conversion.
            selected = 1;
        }
        self.live_suppressed = false;
        let conversion =
            Conversion { kana, keys: self.keys.clone(), candidates, selected, user_forced, number_prefix };
        let display = conversion.display();
        self.conversion = Some(conversion);
        self.keys.clear();
        self.forced = None;
        self.forced_by_user = false;
        self.pinned = None;
        self.syllable_start = None;
        self.engine_preview = None;
        self.set_previous(Some(Language::Japanese));
        match prefix_range {
            Some(range) => client.set_marked_text_replacing(&display, range), // the number, then the phrase
            None => client.set_marked_text(&display),
        }
    }

    /// Shows the reading as katakana (or hiragana) as the selected "candidate": Return
    /// commits it, Space flips to the other form, typing on commits it and continues.
    fn show_kana_forms(&mut self, katakana_first: bool, kana: Option<String>, romaji_keys: Option<String>, client: &mut dyn TextClient) {
        let kana = kana.unwrap_or_else(|| self.japanese_reading(false));
        let count = text::len(&kana);
        let mut forms =
            vec![ConversionCandidate::new(romaji::katakana(&kana), count), ConversionCandidate::new(kana.clone(), count)];
        if !katakana_first {
            forms.reverse();
        }
        let conversion = Conversion {
            kana,
            keys: romaji_keys.unwrap_or_else(|| self.keys.clone()),
            candidates: forms,
            selected: 0,
            user_forced: false,
            number_prefix: String::new(),
        };
        let display = conversion.display();
        self.conversion = Some(conversion);
        self.keys.clear();
        self.forced = None;
        self.forced_by_user = false;
        self.pinned = None;
        self.syllable_start = None;
        self.engine_preview = None;
        self.set_previous(Some(Language::Japanese));
        client.set_marked_text(&display);
    }

    /// The conversion with its first clause `step` kana shorter or longer (今日は|いいてんき):
    /// candidates for exactly that much of the reading, the rest shown as kana. A boundary
    /// never falls inside an English word (google). Nil when it can't move further.
    fn resize_first_clause(&mut self, conversion: &Conversion, step: isize) -> Option<Conversion> {
        self.converter.as_ref()?;
        let reading: Vec<char> = conversion.kana.chars().collect();
        let n = reading.len() as isize;
        let current = conversion.current().kana_count as isize;
        let mut length = current + step;
        let is_letter = |c: char| c.is_ascii() && c.is_alphabetic();
        while (1..n).contains(&length) && is_letter(reading[length as usize - 1]) && is_letter(reading[length as usize]) {
            length += step;
        }
        if !(1..=n).contains(&length) || length == current {
            return None;
        }
        let clause: String = reading[..length as usize].iter().collect();
        let candidates: Vec<_> =
            self.conversion_candidates(&clause).into_iter().filter(|c| c.kana_count == length as usize).collect();
        if candidates.is_empty() {
            return None;
        }
        Some(Conversion {
            kana: conversion.kana.clone(),
            keys: conversion.keys.clone(),
            candidates,
            selected: 0,
            user_forced: conversion.user_forced,
            number_prefix: conversion.number_prefix.clone(),
        })
    }

    fn handle_converting(&mut self, key: Key, client: &mut dyn TextClient) -> bool {
        let Some(mut conversion) = self.conversion.clone() else { return false };
        match key {
            Key::Printable(' ') | Key::Down | Key::ShiftSpace => {
                conversion.selected = (conversion.selected + 1) % conversion.candidates.len();
                client.set_marked_text(&conversion.display());
                self.conversion = Some(conversion);
                true
            }
            Key::Up => {
                let count = conversion.candidates.len();
                conversion.selected = (conversion.selected + count - 1) % count;
                client.set_marked_text(&conversion.display());
                self.conversion = Some(conversion);
                true
            }
            Key::Left | Key::Right => {
                if let Some(resized) = self.resize_first_clause(&conversion, if key == Key::Left { -1 } else { 1 }) {
                    client.set_marked_text(&resized.display());
                    self.conversion = Some(resized);
                }
                true
            }
            Key::Printable(ch) if ch.is_numeric() => {
                if let Some(n) = ch.to_digit(10).map(|n| n as usize) {
                    if n >= 1 && n <= conversion.candidates.len().min(9) {
                        self.choose(n - 1, client);
                        return true;
                    }
                }
                self.commit(Some(client));
                self.handle(key, client)
            }
            Key::Enter => {
                self.commit_candidate(client);
                true
            }
            Key::Katakana | Key::Hiragana => {
                self.conversion = None;
                self.show_kana_forms(key == Key::Katakana, Some(conversion.kana), Some(conversion.keys), client);
                true
            }
            Key::Escape | Key::Cycle | Key::Delete => {
                self.conversion = None;
                self.live_suppressed = true; // "back to kana" means kana, also with live conversion
                // A number taken in from before the phrase goes back as it was typed.
                if !conversion.number_prefix.is_empty() {
                    client.insert_text(&conversion.number_prefix, None);
                    self.note_run(&conversion.number_prefix);
                }
                let kana = text::drop_first(&conversion.kana, text::len(&conversion.number_prefix)).to_string();
                // Back to editing the romaji — when it still spells the reading. After part of a
                // phrase with English in it was committed it may not; keep the kana rather than lose it.
                if JapaneseSegmenter::reading(&conversion.keys, self.segmenter.as_deref()) != kana {
                    client.insert_text(&kana, None);
                    self.note_run(&kana);
                    self.set_previous(Some(Language::Japanese));
                    return true;
                }
                // A final "n" was shown as ん; keep it ("henkan" → "henkan'") instead of
                // letting it wait for a vowel again.
                self.keys = conversion.keys.clone();
                self.syllable_start = None;
                if self.keys.ends_with('n') && !self.keys.ends_with("nn") {
                    self.keys.push('\'');
                    self.added_n_marker = true;
                }
                self.forced = Some(Language::Japanese);
                self.forced_by_user = conversion.user_forced;
                self.update_preview(client);
                true
            }
            Key::Letter(_) | Key::Printable(_) => {
                self.commit(Some(client));
                self.handle(key, client)
            }
            Key::Other => {
                self.commit(Some(client));
                false
            }
        }
    }

    /// Commits the selected candidate; if it covers only the start of the reading,
    /// keeps converting the rest.
    fn commit_candidate(&mut self, client: &mut dyn TextClient) {
        let Some(conversion) = self.conversion.clone() else { return };
        let Some(converter) = self.converter.clone() else { return };
        let candidate = conversion.current().clone();
        converter.learn(&candidate, &conversion.kana);
        client.insert_text(&candidate.text, None);
        self.note_run(&candidate.text);
        self.note_brackets(&candidate.text);
        let rest = text::drop_first(&conversion.kana, candidate.kana_count).to_string();
        self.conversion = None;
        if rest.is_empty() {
            return;
        }
        let rest_keys = Self::keys_after(
            &conversion.keys,
            candidate.kana_count.saturating_sub(text::len(&conversion.number_prefix)),
        );
        let candidates = self.conversion_candidates(&rest);
        if candidates.is_empty() {
            client.insert_text(&rest, None);
            self.note_run(&rest);
        } else {
            // The rest is the engine's Japanese, not something the user corrected.
            let next = Conversion {
                kana: rest,
                keys: rest_keys,
                candidates,
                selected: 0,
                user_forced: false,
                number_prefix: String::new(),
            };
            client.set_marked_text(&next.display());
            self.conversion = Some(next);
        }
    }

    /// The converter's candidates, with a number kept as typed: a reading that is only a number
    /// offers itself (１２３) and its half-width form (123) first; otherwise the candidates that keep
    /// the digits come before those that spell them out (３月, 3月, then 三月).
    fn conversion_candidates(&self, kana: &str) -> Vec<ConversionCandidate> {
        let candidates = self.converter.as_ref().map(|c| c.candidates(kana)).unwrap_or_default();
        let digits = Self::digits(kana);
        if digits.is_empty() {
            return candidates;
        }
        let count = text::len(kana);
        let whole: Vec<_> = candidates.iter().filter(|c| c.kana_count == count).cloned().collect();
        let mut ordered: Vec<ConversionCandidate> = whole
            .iter()
            .filter(|c| Self::digits(&c.text) == digits)
            .chain(whole.iter().filter(|c| Self::digits(&c.text) != digits))
            .cloned()
            .collect();
        if kana.chars().all(is_number_character) {
            let mut front = vec![
                ConversionCandidate::new(kana, count),
                ConversionCandidate::new(Self::half_width(kana), count),
            ];
            front.append(&mut ordered);
            ordered = front;
        }
        let mut shown = std::collections::HashSet::new();
        ordered
            .into_iter()
            .chain(candidates.into_iter().filter(|c| c.kana_count != count))
            .filter(|c| shown.insert(c.text.clone()))
            .collect()
    }

    /// The digits of `text` in order, as ASCII ("東京都１－２" → "12").
    fn digits(text: &str) -> String {
        Self::half_width(text).chars().filter(|c| c.is_ascii_digit()).collect()
    }

    /// Full-width ASCII (！ to ～) as ASCII.
    pub fn half_width(text: &str) -> String {
        text.chars()
            .map(|c| if ('\u{FF01}'..='\u{FF5E}').contains(&c) { char::from_u32(c as u32 - 0xFEE0).unwrap_or(c) } else { c })
            .collect()
    }

    /// The romaji left after the part that produced the first `kana_count` kana: the
    /// shortest tail that composes to exactly the remaining kana ("kitte" after きっ is
    /// "te"; "konnichiha" after こん is "nichiha").
    pub fn keys_after(keys: &str, kana_count: usize) -> String {
        let chars: Vec<char> = keys.chars().collect();
        let composed = romaji::compose(keys, false).text;
        let rest = text::drop_first(&composed, kana_count);
        for i in (0..=chars.len()).rev() {
            let tail: String = chars[i..].iter().collect();
            if romaji::compose(&tail, false).text == rest {
                return tail;
            }
        }
        String::new()
    }

    // MARK: - Private

    fn trace(&mut self, decision: Decision, partial: bool) -> Decision {
        self.last_trace =
            Some(Trace { keys: self.keys.clone(), previous: self.previous_language, decision: decision.clone(), partial });
        decision
    }

    fn decide(&mut self, partial: bool) -> Decision {
        // A segment of a URL or path goes in as typed, before the dictionary is asked (so in a
        // URL is so even where it was taught to be 내).
        if self.in_url_run {
            self.kept_english_by_strict = false;
            let decision = Decision::new(Language::English, 1.0, self.keys.clone(), "inside a URL or path");
            return self.trace(decision, partial);
        }
        if let Some((language, text)) = self.shortcut_reading(partial) {
            // As sure as a split word, so a Strict app shows it too.
            self.kept_english_by_strict = false;
            let decision = Decision::new(language, SPLIT_CONFIDENCE, text, "text replacement shortcut");
            return self.trace(decision, partial);
        }
        // A word typed straight after a number in Japanese text is a counter when it is romaji
        // (3ji, 5fun, 3hon); a unit like km or pm has letters that aren't (5km stays English).
        if self.after_number_in_japanese
            && self.keys.chars().all(|c| c.is_alphabetic())
            && romaji::compose(&self.keys, partial).stray_letters == 0
        {
            self.kept_english_by_strict = false;
            let output = romaji::compose(&self.keys, partial).text;
            let decision = Decision::new(Language::Japanese, SPLIT_CONFIDENCE, output, "counter after a number");
            return self.trace(decision, partial);
        }
        let context = DecisionContext::new(self.previous_language, partial, self.app.as_deref());
        let mut d = self.engine.decide(&self.keys, &context);
        // A word learned as Japanese while Japanese was on: with it off, the engine without what
        // was learned decides it (sushi stays sushi, not すし). Turned on again, it is learned again.
        if d.language == Language::Japanese && !self.japanese_enabled {
            d = self.engine.base.decide(&self.keys, &context);
        }
        let threshold = self.policy.conversion_threshold;
        // While typing, a consonant at the end may still be waiting for its vowel, which keeps
        // Japanese close and a Korean word under a Strict app's threshold until the word ends
        // (qud: 0.60 while typed, 0.98 at Space). The word as it would go in now decides, so the
        // preview shows what Space commits (병), not English that turns Korean on Space.
        if partial && d.language == Language::Korean && d.confidence < threshold {
            let now = self.engine.decide(&self.keys, &DecisionContext::new(self.previous_language, false, self.app.as_deref()));
            if now.language == Language::Korean && now.confidence >= threshold {
                d = now;
            }
        }
        let kept = d.language != Language::English && d.confidence < threshold;
        self.kept_english_by_strict = kept && threshold >= STRICT_THRESHOLD && text::len(&self.keys) <= 3;
        let decision = if kept {
            Decision::new(
                Language::English,
                1.0 - d.confidence,
                self.keys.clone(),
                format!("kept English ({:.2} < {:.2}); {}", d.confidence, threshold, d.explanation),
            )
        } else {
            d
        };
        self.trace(decision, partial)
    }

    /// The language the current word will be committed in.
    fn current_language(&mut self) -> Language {
        match self.forced.or(self.pinned) {
            Some(l) => l,
            None => self.committed_language(),
        }
    }

    /// What Space/Return commits without a switch: the full-word decision, except that a hesitant
    /// change away from the reading on screen doesn't win (unless Strict kept it English). The
    /// same rule as the preview, so what you see is what you get.
    fn committed_language(&mut self) -> Language {
        let d = self.decide(false);
        let Some(shown) = self.engine_preview.filter(|&s| s != d.language) else { return d.language };
        // Japanese on screen, kana or kanji: Space converts it, unless the keys are an English
        // word. A short word reads as the start of a longer Japanese one while typed and as
        // English once it ends, and a Strict app's short token kept English would go in as
        // letters under its kana.
        if shown == Language::Japanese
            && d.language == Language::English
            && !EnglishModel::shared().is_known_word(&self.keys)
            && romaji::compose(&self.keys, false).is_clean()
        {
            return shown;
        }
        if self.is_weak_flip_allowed(&d, shown) {
            d.language
        } else {
            shown
        }
    }

    /// Whether `decision` may replace the reading on screen: when it's confident, when it's
    /// a dictionary English word (which is what Space/Return will commit), or when a Strict app
    /// kept a short token English: there the typed letters win over a reading shown along the
    /// way (gs showed ㅎㄴ, but gst stays gst).
    fn is_weak_flip_allowed(&mut self, decision: &Decision, shown: Language) -> bool {
        decision.confidence >= PREVIEW_FLIP_CONFIDENCE
            || (decision.language == Language::English
                && (self.kept_english_by_strict || EnglishModel::shared().is_known_word(&self.keys)))
            || self.preview(shown).is_none()
    }

    fn update_preview(&mut self, client: &mut dyn TextClient) {
        if self.keys.is_empty() {
            self.held_boundary = None;
            self.preview_language = None;
            self.engine_preview = None;
            client.set_marked_text("");
        } else if let Some(forced) = self.forced.or(self.pinned) {
            self.preview_language = Some(forced);
            let text = match forced {
                Language::Japanese => self.japanese_display(),
                Language::Korean => self.hangul().unwrap_or_else(|| self.keys.clone()),
                _ => self.engine.render(&self.keys, forced),
            };
            self.show(&text, client);
        } else {
            if !self.in_url_run {
                self.split_off_korean_before_other(client);
                self.split_off_japanese_before_korean(client);
            }
            let decision = self.decide(true);
            let mut language = decision.language;
            // Hysteresis: a weak flip on one keystroke (わた → "wat" → わたし) isn't shown,
            // except into a dictionary English word, which is what Space/Return will commit.
            if let Some(shown) = self.preview_language {
                if shown != language && text::len(&self.keys) > 1 && !self.is_weak_flip_allowed(&decision, shown) {
                    language = shown;
                }
            }
            self.preview_language = Some(language);
            self.engine_preview = Some(language);
            let text = if language == Language::Japanese {
                self.japanese_display()
            } else {
                self.preview(language).unwrap_or_else(|| self.keys.clone())
            };
            self.show(&text, client);
        }
    }

    /// The word being composed as the marked text, with its boundary while it is held.
    fn show(&self, text: &str, client: &mut dyn TextClient) {
        match self.held_boundary {
            Some(b) => client.set_marked_text(&format!("{text}{b}")),
            None => client.set_marked_text(text),
        }
    }

    /// An ASCII symbol that ends a word and is held with it in a `hold_space` app: anything the
    /// input method types after a word except Space (held on its own terms) and digits (which
    /// commit the word and go to the app, as they do with holding off).
    pub fn is_boundary_symbol(ch: char) -> bool {
        ch.is_ascii() && ch != ' ' && !ch.is_alphabetic() && !ch.is_numeric()
    }

    /// Korean with another language typed straight after it reads as broken Hangul: 이력 then
    /// rireki (dlfurrireki, 이력갹다ㅑ), 오늘은 then google에서 (dhsmfdmsgoogledptj, 오늘은해ㅐ힏에서).
    /// When the keys only make Hangul with stray jamo, but a front of two or more ordinary
    /// syllables is followed by a known English word (or one with a particle) or by romaji the
    /// engine is sure is Japanese, the Korean goes in as typed and the rest carries on as its own
    /// word.
    fn split_off_korean_before_other(&mut self, client: &mut dyn TextClient) {
        let count = text::len(&self.keys);
        if count < 7 || !hangul::compose(&self.keys).is_some_and(|c| c.is_broken()) {
            return;
        }
        // Only what reads as Korean on its own: a Japanese phrase whose start happens to
        // spell Hangul (chanwo…, 초무をころした) stays whole. Judged without context, since
        // right after Japanese everything leans Japanese.
        let context = DecisionContext::new(None, true, self.app.as_deref());
        if self.engine.decide(&self.keys, &context).language != Language::Korean {
            return;
        }
        let chars: Vec<char> = self.keys.chars().collect();
        let model = korean_model();
        for cut in 4..=(chars.len() - 3) {
            let front: String = chars[..cut].iter().collect();
            let back: String = chars[cut..].iter().collect();
            let Some(korean) = hangul::compose(&front) else { continue };
            if korean.has_stray_jamo()
                || korean.syllable_count() < 2
                || !korean
                    .syllables()
                    .into_iter()
                    .all(|s| model.syllable_log_probability(s) > korean_model::UNKNOWN_SYLLABLE_LOG_P)
                || !self.is_other_language_word(&back)
            {
                continue;
            }
            let text = korean.text();
            client.insert_text(&text, None);
            self.note_run(&text);
            self.keys = back;
            self.syllable_start = None;
            self.set_previous(Some(Language::Korean));
            self.preview_language = None;
            self.engine_preview = None;
            return;
        }
    }

    /// A known English word, one with a Korean particle (google에서), or romaji the engine is
    /// sure is Japanese (no word starts with っ or ん: that cut takes a letter from the Korean).
    fn is_other_language_word(&self, keys: &str) -> bool {
        let context = DecisionContext::new(Some(Language::Korean), true, self.app.as_deref());
        let decision = self.engine.decide(keys, &context);
        match decision.language {
            Language::English => text::len(keys) >= 3 && EnglishModel::shared().is_known_word(keys),
            Language::Mixed => true,
            Language::Japanese => {
                if !self.japanese_enabled || decision.confidence < 0.9 || text::len(keys) < 4 {
                    return false;
                }
                let romaji = romaji::compose(keys, true);
                romaji.stray_letters == 0 && text::first(&romaji.text).is_some_and(|c| c != 'っ' && c != 'ん')
            }
            Language::Korean => false,
        }
    }

    /// Japanese with Korean typed straight after it reads as broken Hangul: a particle
    /// (rirekifmf, 履歴를, not 갹다ㅑ를) or a word (kyouhaehtl…, 今日は도시락은). Once the Korean
    /// part is a particle or two ordinary syllables, the Japanese goes in (kanji with live
    /// conversion) and the rest carries on as Korean, so 를요, 에서 or 도시락은 can follow.
    fn split_off_japanese_before_korean(&mut self, client: &mut dyn TextClient) {
        if !self.japanese_enabled
            || text::len(&self.keys) < 5
            || !hangul::compose(&self.keys).map_or(true, |c| c.is_broken())
            // Keys that still spell romaji are more Japanese (kyouhago is きょうはご, not きょうは해).
            || romaji::compose(&self.keys, true).stray_letters == 0
        {
            return;
        }
        let chars: Vec<char> = self.keys.chars().collect();
        let model = korean_model();
        // The longest Japanese front first: きょうは|도시, not きょう|하ㄷ….
        for cut in (3..chars.len()).rev() {
            let front: String = chars[..cut].iter().collect();
            let back: String = chars[cut..].iter().collect();
            if !DubeolsikLayout::starts_syllable(chars[cut]) {
                continue;
            }
            let Some(korean) = hangul::compose(&back) else { continue };
            if korean.has_stray_jamo() {
                continue;
            }
            let korean_text = korean.text();
            let korean_ok = mixed_word::is_suffix(&korean_text)
                || (korean.syllable_count() >= 2
                    && korean
                        .syllables()
                        .into_iter()
                        .all(|s| model.syllable_log_probability(s) > korean_model::UNKNOWN_SYLLABLE_LOG_P));
            if !korean_ok || !romaji::compose(&front, false).is_clean() {
                continue;
            }
            // Not an English word with a particle (linderman isn't linで그무).
            if self.segmenter.as_ref().and_then(|s| s.english_with_particle(&front)).is_some() {
                continue;
            }
            let decision =
                self.engine.decide(&front, &DecisionContext::new(self.previous_language, false, self.app.as_deref()));
            if decision.language != Language::Japanese || decision.confidence < 0.9 {
                continue;
            }
            self.keys = front;
            let japanese = self.committed_japanese();
            client.insert_text(&japanese, None);
            self.note_run(&japanese);
            self.keys = back;
            self.syllable_start = None;
            self.set_previous(Some(Language::Japanese));
            self.forced = Some(Language::Korean);
            self.forced_by_user = false;
            self.preview_language = None;
            self.engine_preview = None;
            return;
        }
    }

    /// The word so far rendered in `language`, or nil when it has no such reading.
    fn preview(&self, language: Language) -> Option<String> {
        match language {
            Language::English => Some(self.keys.clone()),
            Language::Korean => self.hangul(),
            Language::Mixed => {
                let text = self.engine.render(&self.keys, Language::Mixed);
                (text != self.keys).then_some(text)
            }
            Language::Japanese => {
                // Only っ and letters (ss: っs) is no Japanese word, so it isn't a reading to hold on to.
                if !self.japanese_enabled || romaji::compose(&self.keys, true).is_only_sokuon() {
                    return None;
                }
                Some(self.japanese_reading(true))
            }
        }
    }

    /// Decides the current word, learns a switch override, and resets the buffer.
    fn finish_word(&mut self) -> FinishedWord {
        let engine_choice = self.committed_language();
        let language = self.forced.or(self.pinned).unwrap_or(engine_choice);
        if language != Language::Japanese {
            self.drop_n_marker();
        }
        // "Back to kana" without the user's switch, or a reading kept while deleting, isn't a
        // correction: report it as the engine's.
        let decided = if (self.forced.is_some() && !self.forced_by_user) || (self.forced.is_none() && self.pinned.is_some()) {
            language
        } else {
            engine_choice
        };
        if self.in_url_run {
            // Not a word: neither a correction to learn nor a use of one.
        } else if self.forced.is_some() && self.forced_by_user {
            self.record(&self.keys.clone(), language, decided);
        } else {
            self.engine.used(&self.keys, language);
        }
        let text = match language {
            Language::Japanese => self.committed_japanese(),
            Language::Korean => self.hangul().unwrap_or_else(|| self.engine.render(&self.keys, language)),
            _ => self.engine.render(&self.keys, language),
        };
        self.live_suppressed = false;
        self.note_brackets(&text);
        let word = FinishedWord { keys: std::mem::take(&mut self.keys), text, language, decided };
        self.set_previous(Some(language.sentence_language()));
        self.forced = None;
        self.forced_by_user = false;
        self.pinned = None;
        self.syllable_start = None;
        self.added_n_marker = false;
        self.preview_language = None;
        self.engine_preview = None;
        word
    }

    /// Takes the last kana off a Japanese phrase, as a Japanese IME does (かんこく → かんこ, not
    /// かんこk): the keys become a spelling of the shorter reading, keeping as much of what was
    /// typed as composes to it. A letter still waiting for its vowel goes alone (かんこk → かんこ),
    /// and so does anything in a phrase with English in it.
    fn delete_kana(&mut self) {
        let composed = romaji::compose(&self.keys, true);
        if !composed.pending.is_empty() || composed.stray_letters != 0 || composed.text.is_empty() {
            self.keys.pop();
            return;
        }
        let shorter = text::drop_last(&composed.text, 1);
        self.keys = Self::keys_spelling(shorter, &self.keys).unwrap_or_else(|| text::drop_last(&self.keys, 1).to_string());
    }

    /// The longest start of `keys` that composes to a start of `kana`, followed by a spelling of
    /// the rest ("kya" for き is "ki"; "kitte" for きっ is "kiltu").
    pub fn keys_spelling(kana: &str, keys: &str) -> Option<String> {
        let chars: Vec<char> = keys.chars().collect();
        for cut in (0..chars.len()).rev() {
            let front: String = chars[..cut].iter().collect();
            let composed = romaji::compose(&front, true);
            if !composed.pending.is_empty() || composed.stray_letters != 0 || !kana.starts_with(&composed.text) {
                continue;
            }
            let Some(rest) = romaji::romaji(text::drop_first(kana, text::len(&composed.text))) else { continue };
            let respelled = romaji::compose(&format!("{front}{rest}"), true);
            if respelled.pending.is_empty() && respelled.text == kana {
                return Some(front + &rest);
            }
        }
        None
    }

    /// Korean as the system's 2-set input deletes it: jamo by jamo in the syllable being typed
    /// (한국 → 한구 → 한ㄱ → 한), then whole syllables, which it had already committed (한 → nothing).
    fn delete_jamo_or_syllable(&mut self) {
        let chars: Vec<char> = self.keys.chars().collect();
        if self.syllable_start.is_none() {
            let Some(composition) = hangul::compose(&self.keys) else {
                self.keys.pop();
                return;
            };
            let earlier = &composition.units[..composition.units.len().saturating_sub(1)];
            self.syllable_start = Some(Self::cut(&chars, earlier));
        }
        if self.syllable_start.is_some_and(|s| s < chars.len()) {
            self.keys.pop();
            return;
        }
        // Nothing left being typed: a whole committed syllable goes.
        let Some(composition) = hangul::compose(&self.keys) else {
            self.keys.pop();
            return;
        };
        let earlier = &composition.units[..composition.units.len().saturating_sub(1)];
        let cut = Self::cut(&chars, earlier);
        self.keys = chars[..cut].iter().collect();
        self.syllable_start = Some(cut);
    }

    /// Where the keys of the last syllable start: the longest front that makes `earlier` alone.
    fn cut(chars: &[char], earlier: &[Unit]) -> usize {
        (0..chars.len())
            .rev()
            .find(|&i| hangul::compose(&chars[..i].iter().collect::<String>()).is_some_and(|c| c.units == earlier))
            .unwrap_or(chars.len().saturating_sub(1))
    }

    /// The word as Hangul, with the syllable being typed kept apart from those before it
    /// after a Delete (`syllable_start`).
    fn hangul(&self) -> Option<String> {
        match self.syllable_start {
            Some(start) if start < text::len(&self.keys) => {
                let front = hangul::compose(text::prefix(&self.keys, start))?;
                let back = hangul::compose(text::drop_first(&self.keys, start))?;
                Some(front.text() + &back.text())
            }
            _ => hangul::text(&self.keys),
        }
    }

    fn drop_n_marker(&mut self) {
        if self.added_n_marker && self.keys.ends_with('\'') {
            self.keys.pop();
        }
        self.added_n_marker = false;
    }

    fn record(&mut self, keys: &str, language: Language, engine_choice: Language) {
        if self.last_recorded.as_ref().is_some_and(|(k, l)| k == keys && *l == language) {
            return;
        }
        self.last_recorded = Some((keys.to_string(), language));
        self.engine.learn(keys, language, Some(engine_choice));
    }

    /// Whether the keys have a split reading to cycle to: the engine's (x에, hello월드), which the
    /// particle split alone doesn't find.
    fn engine_splits(&self, keys: &str) -> bool {
        self.engine.render(keys, Language::Mixed) != keys
    }

    /// Replaces a just-committed word with its next rendering, but only if the caret
    /// still sits right after it and the text there is what we inserted.
    fn cycle(&mut self, commit: &Committed, client: &mut dyn TextClient) -> Option<Committed> {
        let range = TextRange::new(commit.location?, text::utf16_len(&commit.text));
        let cursor = client.selected_range()?;
        if cursor.length != 0 || cursor.location != range.end() || client.text_in(range).as_deref() != Some(commit.text.as_str()) {
            return None;
        }
        let language = commit.language.next(&commit.keys, self.japanese_enabled, Some(self.engine_splits(&commit.keys)));
        let reading = if language == Language::Japanese {
            JapaneseSegmenter::reading(&commit.keys, self.segmenter.as_deref())
        } else {
            self.engine.render(&commit.keys, language)
        };
        let text = format!("{reading}{}", commit.boundary);
        client.insert_text(&text, Some(range));
        if self.run_text.ends_with(&commit.text) {
            self.run_text = format!("{}{text}", text::drop_last(&self.run_text, text::len(&commit.text)));
        }
        self.set_previous(Some(language.sentence_language()));
        let mut changed = commit.clone();
        changed.language = language;
        changed.text = text;
        Some(changed)
    }

    fn close_flip_window(&mut self) {
        if let Some(pending) = self.last_commit.take() {
            self.finalize(&pending);
        }
    }

    /// The undo window closed: learn where the word ended up (once, not every reading the switch
    /// key passed through on the way).
    fn finalize(&mut self, commit: &Committed) {
        if commit.learns && commit.language != commit.committed_as {
            self.record(&commit.keys, commit.language, commit.decided);
        }
    }
}
