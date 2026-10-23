//! Words typed into real EDIT and RichEdit controls, with no keyboard and no TSF: the session
//! talks to `EditClient` as the text service has it do, and a key the session leaves alone is
//! done to the control as the control itself would (a character typed, Backspace, Ctrl+Z). The
//! cases are the typing test's (crates/araso-typing-test/cases) for the fields Araso types into
//! itself, so they run on every `cargo test`, in CI too.

use std::sync::Arc;

use araso_core::{AppPolicies, DictionaryEngine, InputSession, Key, Settings};
use windows::core::w;
use windows::Win32::System::LibraryLoader::LoadLibraryW;

use super::*;

const SCRIPTS: &[&str] = &[
    include_str!("../../araso-typing-test/cases/basic.tsv"),
    include_str!("../../araso-typing-test/cases/mid-word.tsv"),
    include_str!("../../araso-typing-test/cases/undo.tsv"),
    // Last: a word switched is learned, and that changes the cases after it.
    include_str!("../../araso-typing-test/cases/switch.tsv"),
];
const EM_UNDO: u32 = 0x00C7;

/// The cases for `field`: "keys<TAB>expected<TAB>fields" lines, as the typing test reads them.
fn cases(field: &str) -> Vec<(String, String)> {
    let mut cases = Vec::new();
    for line in SCRIPTS.iter().flat_map(|s| s.lines()).filter(|l| !l.starts_with('#') && !l.trim().is_empty()) {
        let mut columns = line.splitn(3, '\t');
        let (Some(keys), Some(expected)) = (columns.next(), columns.next()) else { continue };
        let names: Vec<&str> = columns.next().unwrap_or("").split(',').map(str::trim).filter(|n| !n.is_empty()).collect();
        let excluded = names.iter().any(|n| n.strip_prefix('-') == Some(field));
        let only: Vec<&&str> = names.iter().filter(|n| !n.starts_with('-')).collect();
        if !excluded && (only.is_empty() || only.iter().any(|n| **n == field)) {
            cases.push((keys.to_string(), expected.replace("\\n", "\n")));
        }
    }
    cases
}

/// A control in a window of its own, never shown, and a session typing into it.
struct Typing {
    field: Field,
    word: Option<Word>,
    session: InputSession,
}

impl Typing {
    fn new(kind: Kind) -> Self {
        let class = match kind {
            Kind::Edit => w!("EDIT"),
            Kind::RichEdit => {
                let _ = unsafe { LoadLibraryW(w!("Msftedit.dll")) };
                w!("RICHEDIT50W")
            }
        };
        let style = WS_POPUP | WINDOW_STYLE((ES_MULTILINE | ES_AUTOVSCROLL | ES_WANTRETURN) as u32);
        let hwnd = unsafe { CreateWindowExW(WINDOW_EX_STYLE(0), class, w!(""), style, 0, 0, 600, 200, None, None, None, None) }
            .expect("the control");
        let settings = Settings::default();
        // An engine of its own (what one test teaches it stays there), keeping nothing on disk.
        let engine: Arc<DictionaryEngine> = araso_core::make_engine(false, None);
        let session = InputSession::new(
            engine,
            AppPolicies::default(),
            None,
            false,
            settings.live_conversion,
            settings.return_confirms_only,
        );
        Typing { field: Field { hwnd, kind }, word: None, session }
    }

    /// Clears the control for the next case.
    fn clear(&mut self) {
        self.handle(Key::Enter);
        unsafe {
            let _ = SetWindowTextW(self.field.hwnd, w!(""));
        }
        self.session.reset_run();
    }

    /// What the text service does with a key: a word the caret left goes as it is, then the
    /// session has the key. Whether it took it.
    fn handle(&mut self, key: Key) -> bool {
        if let Some(word) = self.word.clone() {
            let word = EditWord { field: self.field, word };
            if caret_left(&word) {
                settle(word);
                self.word = None;
                self.session.composition_terminated();
            }
        }
        let mut client = EditClient { field: self.field, word: &mut self.word };
        self.session.handle(key, &mut client)
    }

    fn type_keys(&mut self, keys: &str) {
        let hwnd = self.field.hwnd;
        for ch in keys.chars() {
            let key = match ch {
                '⇧' => Key::Cycle,
                '⎋' => Key::Escape,
                '⏎' => Key::Enter,
                '⌫' => Key::Delete,
                '↶' => {
                    // Ctrl+Z, as the keys come: Ctrl (a shortcut starts), then Z with it (a key
                    // the session doesn't take: the word goes in), then the control undoes.
                    let mut client = EditClient { field: self.field, word: &mut self.word };
                    self.session.command_pressed(&mut client);
                    if !self.handle(Key::Other) {
                        unsafe { SendMessageW(hwnd, EM_UNDO, None, None) };
                    }
                    continue;
                }
                '⇤' => {
                    // A click at the start of the field.
                    unsafe { SendMessageW(hwnd, EM_SETSEL, Some(WPARAM(0)), Some(LPARAM(0))) };
                    continue;
                }
                c if c.is_ascii_alphabetic() => Key::Letter(c),
                c => Key::Printable(c),
            };
            if self.handle(key) {
                continue;
            }
            // The session left the key to the control.
            match key {
                Key::Letter(c) | Key::Printable(c) => unsafe {
                    SendMessageW(hwnd, WM_CHAR, Some(WPARAM(c as usize)), Some(LPARAM(1)));
                },
                Key::Delete => {
                    let caret = selection(hwnd).map_or(0, |s| s.location);
                    let mut client = EditClient { field: self.field, word: &mut None };
                    client.insert_text("", Some(TextRange::new(caret.saturating_sub(1), caret.min(1))));
                }
                Key::Enter => unsafe {
                    SendMessageW(hwnd, WM_CHAR, Some(WPARAM('\r' as usize)), Some(LPARAM(1)));
                },
                _ => {}
            }
        }
    }

    fn text(&self) -> String {
        let mut buffer = vec![0u16; 4096];
        let n = unsafe { GetWindowTextW(self.field.hwnd, &mut buffer) } as usize;
        String::from_utf16_lossy(&buffer[..n]).replace("\r\n", "\n").replace('\r', "\n")
    }
}

impl Drop for Typing {
    fn drop(&mut self) {
        unsafe {
            let _ = DestroyWindow(self.field.hwnd);
        }
    }
}

fn run(kind: Kind, field: &str) {
    let mut typing = Typing::new(kind);
    let mut failures = Vec::new();
    let cases = cases(field);
    assert!(cases.len() > 20, "the case files weren't read");
    for (keys, expected) in &cases {
        typing.clear();
        typing.type_keys(keys);
        let got = typing.text();
        if got != *expected {
            failures.push(format!("{keys:?} → {got:?} (expected {expected:?})"));
        }
    }
    assert!(failures.is_empty(), "{} of {} cases in {field}:\n{}", failures.len(), cases.len(), failures.join("\n"));
}

#[test]
fn edit_control() {
    run(Kind::Edit, "edit");
}

#[test]
fn rich_edit_control() {
    run(Kind::RichEdit, "richedit-imm");
}

