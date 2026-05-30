//! Offline-graded evaluation harness for the PR review engine.
//!
//! Runs the headless single-file review path (`review_single_file`) over a set
//! of labeled fixtures and prints a precision / recall scorecard, including a
//! list of false-positive regressions. This is how we grade prompt and model
//! changes: capture a baseline, change something, re-run, compare.
//!
//! It calls a live LLM, so it is gated behind `PEX_EVAL=1` and never runs as
//! part of the normal build or `cargo test`.
//!
//! Usage:
//!   cd src-tauri
//!   PEX_EVAL=1 \
//!   PEX_AI_PROVIDER=openai \
//!   PEX_AI_ENDPOINT=https://api.openai.com \
//!   PEX_AI_MODEL=gpt-4.1 \
//!   PEX_AI_KEY=sk-... \
//!   cargo run --example eval_review
//!
//! Optional:
//!   PEX_EVAL_MODE=thorough          # default: fast
//!   PEX_CONFIDENCE_THRESHOLD=80     # default: 80
//!   PEX_EVAL_FIXTURES=/path/to/dir  # default: tests/eval/fixtures
//!
//! Fixture layout — one directory per case under the fixtures dir, each with:
//!   old.txt        the file's base content (empty file = added file)
//!   new.txt        the file's content under review
//!   expected.json  labels: { "path": "...", "expected": [ ... ] }
//! where each expected entry is:
//!   { "lineStart": <int|null>, "lineEnd": <int|null>,
//!     "falsePositive": <bool>, "note": "..." }
//! `falsePositive: true` marks a region the engine should NOT flag (a known
//! trap). Everything else is a true positive the engine SHOULD surface.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use pex_lib::ai::anthropic::AnthropicProvider;
use pex_lib::ai::openai::OpenAiProvider;
use pex_lib::ai::AiProvider;
use pex_lib::review::engine::{review_single_file, FileInput};
use pex_lib::review::state::ReviewMode;
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExpectedCase {
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    expected: Vec<ExpectedFinding>,
}

#[derive(Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct ExpectedFinding {
    #[serde(default)]
    line_start: Option<usize>,
    #[serde(default)]
    line_end: Option<usize>,
    #[serde(default)]
    false_positive: bool,
    #[serde(default)]
    note: String,
}

/// Two 1-based inclusive ranges overlap. `None` line numbers represent
/// file-level findings, which match only other file-level entries.
fn overlaps(a_start: Option<usize>, a_end: Option<usize>, b: &ExpectedFinding) -> bool {
    match (a_start, b.line_start) {
        (None, None) => true,
        (Some(s), Some(_)) => {
            let a_lo = s;
            let a_hi = a_end.unwrap_or(s).max(a_lo);
            let b_lo = b.line_start.unwrap();
            let b_hi = b.line_end.unwrap_or(b_lo).max(b_lo);
            a_lo <= b_hi && b_lo <= a_hi
        }
        _ => false,
    }
}

fn build_provider() -> Arc<dyn AiProvider> {
    let kind = std::env::var("PEX_AI_PROVIDER").unwrap_or_else(|_| "openai".into());
    let endpoint = std::env::var("PEX_AI_ENDPOINT").unwrap_or_else(|_| match kind.as_str() {
        "anthropic" => "https://api.anthropic.com".into(),
        _ => "https://api.openai.com".into(),
    });
    let model = std::env::var("PEX_AI_MODEL").unwrap_or_else(|_| "gpt-4.1".into());
    let key = std::env::var("PEX_AI_KEY").unwrap_or_default();
    if key.is_empty() {
        eprintln!("PEX_AI_KEY is required.");
        std::process::exit(2);
    }
    // Generous read timeout: eval cases are small but we don't want a slow
    // model to flake the run.
    match kind.as_str() {
        "anthropic" => Arc::new(AnthropicProvider::new(endpoint, model, key, 10, 120)),
        _ => Arc::new(OpenAiProvider::new(endpoint, model, key, 10, 120)),
    }
}

fn fixtures_dir() -> PathBuf {
    if let Ok(d) = std::env::var("PEX_EVAL_FIXTURES") {
        return PathBuf::from(d);
    }
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("eval")
        .join("fixtures")
}

fn read_case(dir: &Path) -> Option<(String, FileInput, Vec<ExpectedFinding>)> {
    let name = dir.file_name()?.to_string_lossy().to_string();
    let old = std::fs::read_to_string(dir.join("old.txt")).unwrap_or_default();
    let new = std::fs::read_to_string(dir.join("new.txt")).ok()?;
    let expected_raw = std::fs::read_to_string(dir.join("expected.json")).ok()?;
    let parsed: ExpectedCase = match serde_json::from_str(&expected_raw) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("[{name}] bad expected.json: {e}");
            return None;
        }
    };
    let path = parsed.path.unwrap_or_else(|| name.clone());
    Some((
        name,
        FileInput {
            path,
            old_content: old,
            new_content: new,
        },
        parsed.expected,
    ))
}

#[tokio::main]
async fn main() {
    if std::env::var("PEX_EVAL").ok().as_deref() != Some("1") {
        eprintln!("Refusing to run: set PEX_EVAL=1 (this harness calls a live LLM).");
        std::process::exit(2);
    }

    let mode = match std::env::var("PEX_EVAL_MODE").as_deref() {
        Ok("thorough") => ReviewMode::Thorough,
        _ => ReviewMode::Fast,
    };
    let threshold: u8 = std::env::var("PEX_CONFIDENCE_THRESHOLD")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(80);

    let dir = fixtures_dir();
    let mut cases: Vec<PathBuf> = match std::fs::read_dir(&dir) {
        Ok(rd) => rd
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.is_dir())
            .collect(),
        Err(e) => {
            eprintln!("Cannot read fixtures dir {}: {e}", dir.display());
            std::process::exit(1);
        }
    };
    cases.sort();
    if cases.is_empty() {
        eprintln!("No fixtures found in {}", dir.display());
        std::process::exit(1);
    }

    let provider = build_provider();

    println!(
        "Eval: mode={:?} threshold={} fixtures={} ({} cases)\n",
        mode,
        threshold,
        dir.display(),
        cases.len()
    );

    // Aggregate counters across all cases.
    let mut total_produced = 0usize; // every finding the engine surfaced
    let mut produced_on_tp = 0usize; // surfaced findings overlapping a true positive
    let mut tp_total = 0usize; // labeled true positives across cases
    let mut tp_matched = 0usize; // labeled true positives the engine caught
    let mut fp_regressions: Vec<String> = Vec::new(); // surfaced findings hitting a known trap

    for case_dir in &cases {
        let Some((name, file, expected)) = read_case(case_dir) else {
            continue;
        };

        let result =
            match review_single_file(provider.clone(), mode, &file, "", threshold, 1).await {
                Ok(r) => r,
                Err(e) => {
                    println!("[{name}] ERROR: {e}");
                    continue;
                }
            };

        let tps: Vec<&ExpectedFinding> = expected.iter().filter(|e| !e.false_positive).collect();
        let traps: Vec<&ExpectedFinding> = expected.iter().filter(|e| e.false_positive).collect();
        tp_total += tps.len();

        // Which labeled true positives did we catch?
        let mut matched_this = 0usize;
        for tp in &tps {
            let hit = result
                .findings
                .iter()
                .any(|f| overlaps(f.line_start, f.line_end, tp));
            if hit {
                tp_matched += 1;
                matched_this += 1;
            }
        }

        // Classify each produced finding.
        for f in &result.findings {
            total_produced += 1;
            let on_tp = tps.iter().any(|tp| overlaps(f.line_start, f.line_end, tp));
            if on_tp {
                produced_on_tp += 1;
            }
            if let Some(trap) = traps
                .iter()
                .find(|tp| overlaps(f.line_start, f.line_end, tp))
            {
                fp_regressions.push(format!(
                    "  [{name}] line {:?} hit known trap: {}",
                    f.line_start, trap.note
                ));
            }
        }

        println!(
            "[{name}] caught {}/{} TP · {} finding(s) surfaced",
            matched_this,
            tps.len(),
            result.findings.len()
        );
    }

    let precision = if total_produced == 0 {
        1.0
    } else {
        produced_on_tp as f64 / total_produced as f64
    };
    let recall = if tp_total == 0 {
        1.0
    } else {
        tp_matched as f64 / tp_total as f64
    };

    println!("\n================ Scorecard ================");
    println!("Findings surfaced:      {total_produced}");
    println!("True positives caught:  {tp_matched}/{tp_total}");
    println!("Precision:              {:.1}%", precision * 100.0);
    println!("Recall:                 {:.1}%", recall * 100.0);
    println!("False-positive traps:   {}", fp_regressions.len());
    for line in &fp_regressions {
        println!("{line}");
    }
    println!("===========================================");
}
