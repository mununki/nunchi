//! What vim reports on each mode change (upstream scripts/vim, `Araso --vim <mode>`).

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VimSignal {
    /// Insert (or replace) mode: words are decided.
    Insert,
    /// Normal, visual or command-line mode: keys go straight to vim.
    Normal,
    /// Vim quit, was suspended or lost focus: whatever is in front now isn't vim.
    Off,
}

impl VimSignal {
    pub fn from_raw(raw: &str) -> Option<VimSignal> {
        match raw {
            "insert" => Some(VimSignal::Insert),
            "normal" => Some(VimSignal::Normal),
            "off" => Some(VimSignal::Off),
            _ => None,
        }
    }
}
