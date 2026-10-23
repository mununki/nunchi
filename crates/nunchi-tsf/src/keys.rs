//! Windows key events as `InputSession` keys, and taps of a lone modifier (right Alt, right
//! Shift, right Ctrl) as upstream's `flagsChanged` handling sees them.

use std::time::{Duration, Instant};

use nunchi_core::settings::{KeyModifiers, PauseKey, Tap};
use nunchi_core::{AppPolicy, Key, LatinLayout, SwitchKey};
use windows::Win32::UI::Input::KeyboardAndMouse::*;

/// What a key down is for Nunchi.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Action {
    /// A key for the session.
    Key(Key),
    /// The pause key: pause or resume Nunchi in this text field.
    Pause,
    /// Ctrl or Win went down: a held word goes in before the shortcut (`command_pressed`).
    Command,
    /// Not Nunchi's: the app gets it, and the session doesn't hear of it.
    Pass,
}

fn down(vk: VIRTUAL_KEY) -> bool {
    unsafe { GetKeyState(vk.0 as i32) < 0 }
}

fn toggled(vk: VIRTUAL_KEY) -> bool {
    unsafe { GetKeyState(vk.0 as i32) & 1 != 0 }
}

pub fn modifiers() -> KeyModifiers {
    KeyModifiers {
        control: down(VK_CONTROL),
        alt: down(VK_MENU),
        shift: down(VK_SHIFT),
        win: down(VK_LWIN) || down(VK_RWIN),
    }
}

/// The extended-key bit of a key message's lParam: the right Ctrl and Alt, arrows off the keypad.
fn extended(lparam: isize) -> bool {
    lparam & (1 << 24) != 0
}

fn scan_code(lparam: isize) -> u32 {
    ((lparam >> 16) & 0xFF) as u32
}

/// A right-hand modifier, from a key event (TSF hands Shift, Ctrl and Alt over without sides).
fn right_modifier(vk: VIRTUAL_KEY, lparam: isize) -> Option<Tap> {
    match vk {
        VK_RSHIFT => Some(Tap::RightShift),
        VK_RCONTROL => Some(Tap::RightControl),
        VK_RMENU => Some(Tap::RightAlt),
        VK_SHIFT if scan_code(lparam) == 0x36 => Some(Tap::RightShift),
        VK_CONTROL if extended(lparam) => Some(Tap::RightControl),
        VK_MENU if extended(lparam) => Some(Tap::RightAlt),
        _ => None,
    }
}

pub fn is_modifier(vk: VIRTUAL_KEY) -> bool {
    matches!(
        vk,
        VK_SHIFT | VK_LSHIFT | VK_RSHIFT | VK_CONTROL | VK_LCONTROL | VK_RCONTROL | VK_MENU | VK_LMENU | VK_RMENU
            | VK_LWIN | VK_RWIN | VK_CAPITAL | VK_NUMLOCK | VK_SCROLL
    )
}

/// The character a US keyboard types on a key, for the keys outside the letters.
fn printable(vk: VIRTUAL_KEY, shift: bool) -> Option<char> {
    let (plain, shifted) = match vk {
        VK_SPACE => (' ', ' '),
        VIRTUAL_KEY(0x30) => ('0', ')'),
        VIRTUAL_KEY(0x31) => ('1', '!'),
        VIRTUAL_KEY(0x32) => ('2', '@'),
        VIRTUAL_KEY(0x33) => ('3', '#'),
        VIRTUAL_KEY(0x34) => ('4', '$'),
        VIRTUAL_KEY(0x35) => ('5', '%'),
        VIRTUAL_KEY(0x36) => ('6', '^'),
        VIRTUAL_KEY(0x37) => ('7', '&'),
        VIRTUAL_KEY(0x38) => ('8', '*'),
        VIRTUAL_KEY(0x39) => ('9', '('),
        VK_OEM_1 => (';', ':'),
        VK_OEM_PLUS => ('=', '+'),
        VK_OEM_COMMA => (',', '<'),
        VK_OEM_MINUS => ('-', '_'),
        VK_OEM_PERIOD => ('.', '>'),
        VK_OEM_2 => ('/', '?'),
        VK_OEM_3 => ('`', '~'),
        VK_OEM_4 => ('[', '{'),
        VK_OEM_5 => ('\\', '|'),
        VK_OEM_6 => (']', '}'),
        VK_OEM_7 => ('\'', '"'),
        VK_MULTIPLY => ('*', '*'),
        VK_ADD => ('+', '+'),
        VK_SUBTRACT => ('-', '-'),
        VK_DECIMAL => ('.', '.'),
        VK_DIVIDE => ('/', '/'),
        _ if (VK_NUMPAD0.0..=VK_NUMPAD9.0).contains(&vk.0) => {
            let c = char::from(b'0' + (vk.0 - VK_NUMPAD0.0) as u8);
            (c, c)
        }
        _ => return None,
    };
    Some(if shift { shifted } else { plain })
}

/// The QWERTY seat of a key in the letter rows: its letter, or ; for the key right of L.
fn letter_seat(vk: VIRTUAL_KEY) -> Option<char> {
    match vk.0 {
        0x41..=0x5A => Some(char::from(vk.0 as u8).to_ascii_lowercase()),
        _ if vk == VK_OEM_1 => Some(';'),
        _ => None,
    }
}

pub fn translate(vk: VIRTUAL_KEY, lparam: isize, policy: &AppPolicy, pause: &PauseKey) -> Action {
    let m = modifiers();
    if vk == VK_HANJA {
        return if matches!(pause, PauseKey::Tap(Tap::HanjaKey)) { Action::Pause } else { Action::Pass };
    }
    if vk == VK_HANGUL {
        return if policy.switch_keys.contains(&SwitchKey::HangulKey) { Action::Key(Key::Cycle) } else { Action::Pass };
    }
    if let PauseKey::Shortcut(shortcut) = pause {
        if shortcut.matches(vk.0, m) {
            return Action::Pause;
        }
    }
    if is_modifier(vk) {
        let shortcut = matches!(vk, VK_CONTROL | VK_LCONTROL | VK_RCONTROL | VK_LWIN | VK_RWIN);
        // A right Ctrl that is the pause key is a tap, not the start of a shortcut.
        let pause_tap = matches!(pause, PauseKey::Tap(Tap::RightControl | Tap::HanjaKey))
            && right_modifier(vk, lparam) == Some(Tap::RightControl);
        return if shortcut && !pause_tap { Action::Command } else { Action::Pass };
    }
    if vk == VK_PROCESSKEY || vk == VK_PACKET {
        return Action::Pass;
    }
    if m.control && !m.alt && !m.win {
        return Action::Key(match vk.0 {
            0x4B if !m.shift => Key::Katakana, // Ctrl+K
            0x4A if !m.shift => Key::Hiragana, // Ctrl+J
            _ => Key::Other,
        });
    }
    if m.control || m.alt || m.win {
        return Action::Key(Key::Other);
    }
    if vk == VK_ESCAPE {
        if m.shift {
            // Not chosen: the app's Shift+Esc, after the word goes in as shown.
            return Action::Key(if policy.switch_keys.contains(&SwitchKey::ShiftEscape) { Key::Cycle } else { Key::Other });
        }
        return Action::Key(Key::Escape);
    }
    let key = match vk {
        VK_F7 => Key::Katakana,
        VK_F6 => Key::Hiragana,
        VK_BACK => Key::Delete,
        VK_RETURN => Key::Enter,
        VK_LEFT => Key::Left,
        VK_RIGHT => Key::Right,
        VK_UP => Key::Up,
        VK_DOWN => Key::Down,
        _ => {
            if let Some(seat) = letter_seat(vk) {
                let layout = LatinLayout::current();
                if let Some(ch) = layout.character_for_seat(seat, m.shift, toggled(VK_CAPITAL)) {
                    if ch.is_ascii_alphabetic() || layout.is_letter_seat(ch) {
                        return Action::Key(Key::Letter(ch));
                    }
                    return Action::Key(Key::Printable(ch));
                }
            }
            match printable(vk, m.shift) {
                Some(' ') if m.shift => Key::ShiftSpace,
                Some(ch) => Key::Printable(ch),
                None => Key::Other,
            }
        }
    };
    Action::Key(key)
}

/// A right-hand modifier pressed and released on its own within a moment.
#[derive(Default)]
pub struct Taps {
    down: Option<(Tap, Instant)>,
}

const TAP_TIME: Duration = Duration::from_millis(500);

impl Taps {
    pub fn key_down(&mut self, vk: VIRTUAL_KEY, lparam: isize) {
        let repeat = lparam & (1 << 30) != 0;
        match right_modifier(vk, lparam) {
            Some(tap) if repeat && self.down.map(|d| d.0) == Some(tap) => {}
            Some(tap) => {
                // Only on its own: a Hyper key (all modifiers at once) is bound to something else.
                let m = modifiers();
                let others = [m.control && tap != Tap::RightControl, m.alt && tap != Tap::RightAlt,
                              m.shift && tap != Tap::RightShift, m.win];
                self.down = if others.iter().any(|&o| o) { None } else { Some((tap, Instant::now())) };
            }
            None => self.down = None, // another key with it: a shortcut or a character, not a tap
        }
    }

    pub fn key_up(&mut self, vk: VIRTUAL_KEY, lparam: isize) -> Option<Tap> {
        let tap = right_modifier(vk, lparam)?;
        let (down, at) = self.down.take()?;
        (down == tap && at.elapsed() < TAP_TIME).then_some(tap)
    }
}

/// The switch key a tap is, if the app chose it.
pub fn switch_tap(tap: Tap, policy: &AppPolicy, pause: &PauseKey) -> bool {
    if *pause == PauseKey::Tap(tap) {
        return false; // the pause key wins where an app chose the same key
    }
    match tap {
        Tap::RightAlt => policy.switch_keys.contains(&SwitchKey::RightAlt),
        Tap::RightShift => policy.switch_keys.contains(&SwitchKey::RightShift),
        _ => false,
    }
}

/// The pause key, as a tap.
pub fn pause_tap(tap: Tap, pause: &PauseKey) -> bool {
    match pause {
        PauseKey::Tap(Tap::HanjaKey) => tap == Tap::RightControl, // where the keyboard has no 한자 key
        PauseKey::Tap(t) => *t == tap,
        _ => false,
    }
}

/// The kind of key, for the log: never which character.
pub fn kind(key: Key) -> &'static str {
    match key {
        Key::Letter(_) => "letter",
        Key::Delete => "delete",
        Key::Escape => "escape",
        Key::Cycle => "cycle",
        Key::Printable(_) => "printable",
        Key::ShiftSpace => "shiftSpace",
        Key::Enter => "enter",
        Key::Up | Key::Down | Key::Left | Key::Right => "arrow",
        Key::Katakana | Key::Hiragana => "kana",
        Key::Other => "other",
    }
}
