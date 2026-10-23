//! Usage:
//!   nunchi-cli "dkssudgktpdy rmfjgek hello world"   convert arguments
//!   nunchi-cli -v "..."                              also print per-word scores
//!   echo "..." | nunchi-cli                          convert stdin line by line
//!   nunchi-cli --compose < keys.txt                  print "keys<TAB>hangul" per line
//!   nunchi-cli --kana < romaji.txt                   print "hiragana<TAB>stray letters" per line
//!   nunchi-cli --decide [--japanese] < keys_prev.tsv verdict for "keys<TAB>previous" lines
//!   nunchi-cli --type [--japanese] < keys.txt        "keys<TAB>text" after typing keys + Space into a session
//! NUNCHI_LAYOUT=colemak (or colemakDH): keys are that layout's letters, as settings.json's latinLayout.
//! NUNCHI_KOREAN_LAYOUT=sebeolsikFinal: Korean is read as 3-set, as settings.json's koreanLayout.
//! NUNCHI_TIMING=1 with --decide: also time each decision.

use std::io::{BufRead, Write};
use std::sync::Arc;
use std::time::Instant;

use nunchi_core::app_policy::{Override, DEFAULT_KEY};
use nunchi_core::plain_field::PlainTextField;
use nunchi_core::{
    hangul, romaji, text_converter, AppPolicies, DecisionContext, DecisionEngine, DictionaryEngine, HeuristicEngine,
    InputSession, Key, KoreanLayout, Language, LatinLayout, SplittingEngine,
};

fn splitting(japanese: bool) -> SplittingEngine {
    let heuristic = HeuristicEngine::standard(japanese);
    let (english, korean, segmenter) = (heuristic.english.clone(), heuristic.korean.clone(), heuristic.segmenter.clone());
    SplittingEngine::new(Arc::new(heuristic), english, korean, segmenter)
}

fn lines() -> impl Iterator<Item = String> {
    std::io::stdin().lock().lines().map_while(Result::ok).collect::<Vec<_>>().into_iter()
}

fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    if let Some(layout) = std::env::var("NUNCHI_LAYOUT").ok().and_then(|n| LatinLayout::from_raw(&n)) {
        LatinLayout::set_active(layout);
    }
    if let Some(layout) = std::env::var("NUNCHI_JAPANESE_LAYOUT").ok().and_then(|n| LatinLayout::from_raw(&n)) {
        LatinLayout::set_active_romaji(Some(layout));
    }
    if let Some(layout) = std::env::var("NUNCHI_KOREAN_LAYOUT").ok().and_then(|n| KoreanLayout::from_raw(&n)) {
        KoreanLayout::set_active(layout);
    }
    let out = std::io::stdout();
    let mut out = out.lock();
    let japanese = args.get(1).map(String::as_str) == Some("--japanese");

    match args.first().map(String::as_str) {
        Some("--compose") => {
            for keys in lines() {
                let _ = writeln!(out, "{keys}\t{}", hangul::text(&keys).unwrap_or_else(|| keys.clone()));
            }
            return;
        }
        Some("--kana") => {
            for keys in lines() {
                let r = romaji::compose(&keys, false);
                let _ = writeln!(out, "{}\t{}", r.text, r.stray_letters);
            }
            return;
        }
        Some("--decide") => {
            let engine = splitting(japanese);
            let timing = std::env::var("NUNCHI_TIMING").is_ok();
            let (mut times, mut finals) = (Vec::new(), Vec::new());
            for line in lines() {
                let fields: Vec<&str> = line.split('\t').collect();
                let keys = fields[0];
                let previous = fields.get(1).and_then(|p| Language::from_raw(p));
                let mut start = Instant::now();
                if timing {
                    let _ = engine.decide(keys, &DecisionContext { previous_language: previous, partial: true, ..Default::default() });
                    times.push(start.elapsed().as_secs_f64() * 1000.0);
                    start = Instant::now();
                }
                // A third column "partial": decided as while typing (the preview).
                let partial = fields.get(2) == Some(&"partial");
                let d = engine.decide(keys, &DecisionContext { previous_language: previous, partial, ..Default::default() });
                if timing {
                    finals.push(start.elapsed().as_secs_f64() * 1000.0);
                }
                let mut row = format!(
                    "{keys}\t{}\t{:.4}\t{}\t{}\t{}",
                    d.language.raw(),
                    d.confidence,
                    d.output,
                    d.explanation,
                    d.runner_up.map(Language::raw).unwrap_or("")
                );
                if timing {
                    row += &format!("\t{:.2}ms\t{:.2}ms", times.last().unwrap(), finals.last().unwrap());
                }
                let _ = writeln!(out, "{row}");
            }
            if timing && times.len() > 1 {
                times.remove(0); // the first line also loads the models
                finals.remove(0);
                let avg = |v: &[f64]| v.iter().sum::<f64>() / v.len() as f64;
                let max = |v: &[f64]| v.iter().cloned().fold(0.0, f64::max);
                eprintln!(
                    "keystroke avg {:.2} max {:.2} ms; final avg {:.2} max {:.2} ms",
                    avg(&times),
                    max(&times),
                    avg(&finals),
                    max(&finals)
                );
            }
            return;
        }
        Some("--type") => {
            // Each line's keys and a Space through a fresh InputSession (real engines, no converter):
            // what ends up in the field, as typed. NUNCHI_THRESHOLD=0.97: as in a Strict app.
            let engine = splitting(japanese);
            let english = engine.english.clone();
            let korean = engine.korean.clone();
            let japanese_particles = engine.japanese.is_some();
            let dictionary =
                Arc::new(DictionaryEngine::new(Arc::new(engine), english, korean, japanese_particles, None, None));
            let mut policies = AppPolicies::default();
            if let Some(threshold) = std::env::var("NUNCHI_THRESHOLD").ok().and_then(|t| t.parse::<f64>().ok()) {
                policies
                    .overrides
                    .insert(DEFAULT_KEY.into(), Override { conversion_threshold: Some(threshold), ..Default::default() });
            }
            for keys in lines() {
                let mut field = PlainTextField::new();
                let mut session = InputSession::new(dictionary.clone(), policies.clone(), None, japanese, false, true);
                for ch in format!("{keys} ").chars() {
                    let key = if ch.is_alphabetic() { Key::Letter(ch) } else { Key::Printable(ch) };
                    if !session.handle(key, &mut field) {
                        nunchi_core::TextClient::insert_text(&mut field, &ch.to_string(), None); // the app types keys passed on
                    }
                }
                let _ = writeln!(out, "{keys}\t{}", field.text);
            }
            return;
        }
        _ => {}
    }

    let verbose = args.first().map(String::as_str) == Some("-v");
    if verbose {
        args.remove(0);
    }
    let engine = splitting(false);
    let mut run = |line: &str| {
        let (output, words) = text_converter::convert(&engine, line);
        let _ = writeln!(out, "{output}");
        if verbose {
            for word in words {
                let d = word.decision;
                let lang = match d.language {
                    Language::English => "en",
                    Language::Korean => "ko",
                    Language::Mixed => "mixed",
                    Language::Japanese => "ja",
                };
                let _ = writeln!(out, "  {:<16} → {} [{} {:.2}] {}", word.keys, d.output, lang, d.confidence, d.explanation);
            }
        }
    };
    if args.is_empty() {
        for line in lines() {
            run(&line);
        }
    } else {
        run(&args.join(" "));
    }
}
