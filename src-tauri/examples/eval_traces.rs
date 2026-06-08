//! Evaluate review diagnostic traces — primarily for token cost.
//!
//! Reads the JSONL trace files written when the "Write diagnostic traces"
//! setting (`ai_diagnostics`) is on, and prints a per-run and overall report:
//! token usage broken down by pipeline stage, LLM call counts, and the derived
//! output-tokens-per-file figure that drives wall-clock on slow local models.
//! This is how you check whether a prompt / cap / roster change actually moved
//! token cost, instead of estimating from tokens/sec.
//!
//! Usage:
//!   cd src-tauri
//!   cargo run --example eval_traces                 # scans the default dir
//!   cargo run --example eval_traces -- a.jsonl b.jsonl
//!   cargo run --example eval_traces -- /path/to/diagnostics
//!
//! With no arguments it scans `pex_lib::cache::diagnostics_dir()` (the same
//! directory the app writes to). Arguments may be individual `.jsonl` files or
//! directories to scan.
//!
//! It only reads existing traces — it never calls an LLM.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde_json::Value;

/// Token totals for one stage (e.g. `hunk`, `specialist:code-reviewer`,
/// `adjudicate`, `batch`, `synthesis`, `anchor`).
#[derive(Default, Clone)]
struct StageStats {
    input: u64,
    output: u64,
    calls: u64,
}

/// Aggregated view of a single review run (one trace file).
#[derive(Default)]
struct RunStats {
    run_id: String,
    mode: String,
    files: u64,
    by_stage: BTreeMap<String, StageStats>,
    /// From `run_done`, when present (the run completed).
    done_input: Option<u64>,
    done_output: Option<u64>,
    done_calls: Option<u64>,
    calls_without_usage: u64,
    findings: Option<u64>,
}

impl RunStats {
    fn totals(&self) -> StageStats {
        let mut t = StageStats::default();
        for s in self.by_stage.values() {
            t.input += s.input;
            t.output += s.output;
            t.calls += s.calls;
        }
        t
    }
}

fn as_u64(v: &Value, key: &str) -> Option<u64> {
    v.get(key).and_then(Value::as_u64)
}

fn as_str<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    v.get(key).and_then(Value::as_str)
}

fn parse_trace(path: &Path) -> std::io::Result<RunStats> {
    let body = std::fs::read_to_string(path)?;
    let mut run = RunStats {
        run_id: path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("?")
            .to_string(),
        ..Default::default()
    };

    for line in body.lines().filter(|l| !l.trim().is_empty()) {
        let Ok(event) = serde_json::from_str::<Value>(line) else {
            continue; // tolerate a partial final line from a crashed run
        };
        match as_str(&event, "kind") {
            Some("run_start") => {
                if let Some(m) = as_str(&event, "mode") {
                    run.mode = m.to_string();
                }
                if let Some(n) = as_u64(&event, "fileCount") {
                    run.files = n;
                }
            }
            Some("token_usage") => {
                let stage = as_str(&event, "stage").unwrap_or("unknown").to_string();
                let entry = run.by_stage.entry(stage).or_default();
                entry.input += as_u64(&event, "inputTokens").unwrap_or(0);
                entry.output += as_u64(&event, "outputTokens").unwrap_or(0);
                entry.calls += 1;
            }
            Some("run_done") => {
                run.done_input = as_u64(&event, "inputTokens");
                run.done_output = as_u64(&event, "outputTokens");
                run.done_calls = as_u64(&event, "llmCalls");
                run.calls_without_usage = as_u64(&event, "callsWithoutUsage").unwrap_or(0);
                run.findings = as_u64(&event, "findings");
            }
            _ => {}
        }
    }
    Ok(run)
}

/// Expand the CLI args (files or directories) into a flat list of `.jsonl`
/// files. With no args, scan the app's diagnostics directory.
fn collect_trace_files(args: &[String]) -> Vec<PathBuf> {
    let mut roots: Vec<PathBuf> = args.iter().map(PathBuf::from).collect();
    if roots.is_empty() {
        match pex_lib::cache::diagnostics_dir() {
            Ok(dir) => {
                println!("No paths given; scanning {dir}\n");
                roots.push(PathBuf::from(dir));
            }
            Err(e) => {
                eprintln!("Could not resolve the diagnostics directory: {e}");
                std::process::exit(2);
            }
        }
    }

    let mut files = Vec::new();
    for root in roots {
        if root.is_dir() {
            if let Ok(entries) = std::fs::read_dir(&root) {
                for entry in entries.flatten() {
                    let p = entry.path();
                    if p.extension().and_then(|e| e.to_str()) == Some("jsonl") {
                        files.push(p);
                    }
                }
            }
        } else if root.exists() {
            files.push(root);
        } else {
            eprintln!("Skipping {} — not found", root.display());
        }
    }
    files.sort();
    files
}

fn print_run(run: &RunStats) {
    let totals = run.totals();
    println!("── {} ─────────────────────────────", run.run_id);
    println!(
        "   mode: {:<8} files: {:<4} llm calls: {}",
        if run.mode.is_empty() { "?" } else { &run.mode },
        run.files,
        run.done_calls.unwrap_or(totals.calls),
    );
    if !run.by_stage.is_empty() {
        println!(
            "   {:<28} {:>10} {:>10} {:>7}",
            "stage", "input", "output", "calls"
        );
        for (stage, s) in &run.by_stage {
            println!(
                "   {:<28} {:>10} {:>10} {:>7}",
                stage, s.input, s.output, s.calls
            );
        }
    }
    let out = run.done_output.unwrap_or(totals.output);
    let inp = run.done_input.unwrap_or(totals.input);
    println!(
        "   {:<28} {:>10} {:>10} {:>7}",
        "TOTAL", inp, out, totals.calls
    );
    if run.files > 0 {
        println!(
            "   output tokens / file: {:.0}",
            out as f64 / run.files as f64
        );
    }
    if let Some(f) = run.findings {
        println!("   findings surfaced: {f}");
    }
    if run.calls_without_usage > 0 {
        println!(
            "   ⚠ {} call(s) reported no usage — totals are a lower bound",
            run.calls_without_usage
        );
    }
    println!();
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let files = collect_trace_files(&args);
    if files.is_empty() {
        eprintln!("No .jsonl trace files found.");
        std::process::exit(1);
    }

    let mut runs = Vec::new();
    for f in &files {
        match parse_trace(f) {
            Ok(run) => runs.push(run),
            Err(e) => eprintln!("Skipping {} — {e}", f.display()),
        }
    }

    for run in &runs {
        print_run(run);
    }

    // Overall rollup across every run, by stage.
    let mut overall: BTreeMap<String, StageStats> = BTreeMap::new();
    let mut grand_files = 0u64;
    for run in &runs {
        grand_files += run.files;
        for (stage, s) in &run.by_stage {
            let e = overall.entry(stage.clone()).or_default();
            e.input += s.input;
            e.output += s.output;
            e.calls += s.calls;
        }
    }

    println!(
        "══ OVERALL ({} run(s), {} file(s)) ══════════",
        runs.len(),
        grand_files
    );
    println!(
        "   {:<28} {:>10} {:>10} {:>7}",
        "stage", "input", "output", "calls"
    );
    let mut g = StageStats::default();
    for (stage, s) in &overall {
        println!(
            "   {:<28} {:>10} {:>10} {:>7}",
            stage, s.input, s.output, s.calls
        );
        g.input += s.input;
        g.output += s.output;
        g.calls += s.calls;
    }
    println!(
        "   {:<28} {:>10} {:>10} {:>7}",
        "TOTAL", g.input, g.output, g.calls
    );
    if grand_files > 0 {
        println!(
            "   output tokens / file (mean): {:.0}",
            g.output as f64 / grand_files as f64
        );
    }
}
