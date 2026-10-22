//! Araso's typing logic, ported from the macOS input method's ArasoCore
//! (https://github.com/nyttastudio/araso): the user types on QWERTY and Araso decides per word
//! whether it is English, Korean (2-set or 3-set) or Japanese romaji.
//!
//! Nothing here knows about Windows: the TSF text service drives `InputSession` through the
//! `TextClient` trait, as the macOS app drives it through InputMethodKit.

pub mod app_policy;
pub mod converter;
pub mod decision;
mod embedded;
#[allow(dead_code)] // its short words go into the tables only (build.rs)
mod english_lists;
pub mod english_model;
pub mod hangul;
pub mod input_session;
pub mod jamo;
pub mod japanese_model;
pub mod japanese_segmenter;
pub mod korean_model;
pub mod korean_syllables;
pub mod latin_layout;
pub mod mixed_word;
pub mod particle_splitter;
pub mod plain_field;
pub mod resources;
pub mod romaji;
pub mod sebeolsik;
pub mod settings;
pub mod support_dir;
pub mod tables;
pub mod text;
pub mod text_converter;
pub mod user_dictionary;
pub mod vim;

use std::path::PathBuf;
use std::sync::Arc;

pub use app_policy::{AppPolicies, AppPolicy, SwitchKey};
pub use decision::{Decision, DecisionContext, DecisionEngine, HeuristicEngine, Language};
pub use english_model::EnglishModel;
pub use input_session::{InputSession, Key, TextClient, TextRange};
pub use korean_model::KoreanModel;
pub use latin_layout::LatinLayout;
pub use mixed_word::SplittingEngine;
pub use sebeolsik::KoreanLayout;
pub use settings::Settings;
pub use user_dictionary::DictionaryEngine;

/// Order of authority: the user's dictionary (words added, or corrected the same way twice;
/// one correction only nudges), then mixed-word splitting (github에, x에, hello월드, githubで),
/// then the heuristic.
pub fn make_engine(japanese: bool, store_url: Option<PathBuf>) -> Arc<DictionaryEngine> {
    let heuristic = HeuristicEngine::standard(japanese);
    let english = heuristic.english.clone();
    let korean = heuristic.korean.clone();
    let segmenter = heuristic.segmenter.clone();
    let japanese_particles = segmenter.is_some();
    let splitting = SplittingEngine::new(Arc::new(heuristic), english.clone(), korean.clone(), segmenter);
    Arc::new(DictionaryEngine::new(Arc::new(splitting), english, korean, japanese_particles, store_url, None))
}
