//! Diagnostic trace logging for review runs.
//!
//! When enabled, every review run writes a JSONL trace file — one JSON object
//! per line — capturing both the **non-deterministic** behaviors (the exact
//! prompts sent to the model and the raw responses back) and the
//! **deterministic** ones (confidence/anchor guard drops, tier assignments,
//! suppression hits, final findings). Each finding event carries the same
//! `fingerprint` used by the feedback loop, so a trace (the *features*) can be
//! joined to recorded verdicts (the *labels*) to build an evaluation/training
//! set from real runs.
//!
//! Opt-in (`ai_diagnostics`, default off): traces contain source content and
//! full prompts, so they're written only when the user asks for them.
//!
//! The handle is cheap to clone (an `Arc` inside) and a no-op when disabled, so
//! it can be threaded through the engine and into spawned tasks without
//! ceremony.

use std::fs::{create_dir_all, OpenOptions};
use std::io::{BufWriter, Write};
use std::sync::{Arc, Mutex};

#[derive(Clone)]
pub struct Diagnostics {
    inner: Option<Arc<Mutex<Inner>>>,
}

struct Inner {
    writer: BufWriter<std::fs::File>,
    run_id: String,
    path: String,
}

impl Diagnostics {
    /// A no-op sink. Used when diagnostics are off and by headless callers.
    pub fn disabled() -> Self {
        Self { inner: None }
    }

    /// Create a per-run JSONL trace at `dir/<run_id>.jsonl`. Falls back to a
    /// disabled sink if the file can't be opened — diagnostics must never break
    /// a review.
    pub fn create(dir: &str, run_id: &str) -> Self {
        match Self::try_create(dir, run_id) {
            Ok(d) => d,
            Err(e) => {
                eprintln!("[diagnostics] disabled — could not open trace file: {e}");
                Self::disabled()
            }
        }
    }

    fn try_create(dir: &str, run_id: &str) -> std::io::Result<Self> {
        create_dir_all(dir)?;
        let path = format!("{}/{}.jsonl", dir, sanitize(run_id));
        let file = OpenOptions::new().create(true).append(true).open(&path)?;
        Ok(Self {
            inner: Some(Arc::new(Mutex::new(Inner {
                writer: BufWriter::new(file),
                run_id: run_id.to_string(),
                path,
            }))),
        })
    }

    pub fn is_enabled(&self) -> bool {
        self.inner.is_some()
    }

    /// Absolute path of the trace file, if enabled.
    pub fn path(&self) -> Option<String> {
        self.inner
            .as_ref()
            .and_then(|i| i.lock().ok().map(|g| g.path.clone()))
    }

    /// Append one event. `payload` should be a JSON object; its keys are merged
    /// alongside `ts`, `runId`, and `kind`. No-op when disabled. Flushes each
    /// line so a crash mid-run still leaves a usable partial trace.
    pub fn event(&self, kind: &str, payload: serde_json::Value) {
        let Some(inner) = &self.inner else {
            return;
        };
        let Ok(mut g) = inner.lock() else {
            return;
        };
        let mut line = serde_json::json!({
            "ts": now_rfc3339(),
            "runId": g.run_id,
            "kind": kind,
        });
        if let (Some(obj), serde_json::Value::Object(extra)) = (line.as_object_mut(), payload) {
            for (k, v) in extra {
                obj.insert(k, v);
            }
        }
        if let Ok(s) = serde_json::to_string(&line) {
            let _ = writeln!(g.writer, "{}", s);
            let _ = g.writer.flush();
        }
    }
}

fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339()
}

/// Keep run ids filesystem-safe.
fn sanitize(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_is_noop() {
        let d = Diagnostics::disabled();
        assert!(!d.is_enabled());
        d.event("anything", serde_json::json!({"a": 1})); // must not panic
        assert!(d.path().is_none());
    }

    #[test]
    fn enabled_writes_jsonl_lines() {
        let dir = std::env::temp_dir().join(format!("pex-diag-{}", std::process::id()));
        let dir = dir.to_string_lossy().to_string();
        let d = Diagnostics::create(&dir, "run/with:bad*chars");
        assert!(d.is_enabled());
        d.event("run_start", serde_json::json!({"prKey": "p", "mode": "fast"}));
        d.event("finding_final", serde_json::json!({"fingerprint": "abc", "tier": "blocking"}));
        let path = d.path().unwrap();
        drop(d); // flush/close
        let body = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = body.lines().collect();
        assert_eq!(lines.len(), 2);
        let first: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
        assert_eq!(first["kind"], "run_start");
        assert_eq!(first["prKey"], "p");
        assert!(first["ts"].is_string());
        let _ = std::fs::remove_file(&path);
    }
}
