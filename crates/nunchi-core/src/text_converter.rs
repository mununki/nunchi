//! Converts a whole line of QWERTY text word by word, carrying language context
//! across words exactly as the input method does while typing.

use crate::decision::{Decision, DecisionContext, DecisionEngine, Language};

pub struct Word {
    pub keys: String,
    pub decision: Decision,
}

pub fn convert(engine: &dyn DecisionEngine, line: &str) -> (String, Vec<Word>) {
    let mut output = String::new();
    let mut words = Vec::new();
    let mut token = String::new();
    let mut previous: Option<Language> = None;

    let mut flush = |token: &mut String, output: &mut String, previous: &mut Option<Language>| {
        if token.is_empty() {
            return;
        }
        let decision = engine.decide(token, &DecisionContext::previous(*previous));
        output.push_str(&decision.output);
        *previous = Some(decision.language.sentence_language());
        words.push(Word { keys: std::mem::take(token), decision });
    };

    for ch in line.chars() {
        if ch.is_ascii_alphabetic() {
            token.push(ch);
        } else {
            flush(&mut token, &mut output, &mut previous);
            output.push(ch);
            if ch == '\n' || ch == '\r' {
                previous = None;
            }
        }
    }
    flush(&mut token, &mut output, &mut previous);
    (output, words)
}
