use crate::review::engine::FileAggregateResult;
use crate::AppError;
use serde::{Deserialize, Serialize};

/// Which review strategy to run.
/// - `Fast`: single generalist pass per hunk (original behavior).
/// - `Thorough`: multiple specialist passes per hunk (slower, broader coverage).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum ReviewMode {
    #[default]
    Fast,
    Thorough,
}

/// Bumped whenever the persisted `ReviewState` / finding shape changes in a way
/// that makes a mid-run resume incoherent. `load_state` discards any saved
/// state below this version (completed findings are cheap to regenerate; a
/// half-old resume is not worth the risk). States written before this field
/// existed deserialize to 0 via `serde(default)` and are therefore discarded.
pub const CURRENT_SCHEMA_VERSION: u32 = 3;

/// Serializable progress state for resumable PR reviews.
/// Persisted to SQLite so the user can continue after cancellation or crash.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewState {
    /// Schema version of this persisted state (see `CURRENT_SCHEMA_VERSION`).
    #[serde(default)]
    pub schema_version: u32,
    /// The PR URL or identifier
    pub pr_key: String,
    /// Which review strategy this run is using. Defaults to Fast so older
    /// persisted states still deserialize.
    #[serde(default)]
    pub mode: ReviewMode,
    /// Phase: "hunk-review", "file-aggregate", "batch-aggregate", "synthesis", "done"
    pub phase: String,
    /// All file paths being reviewed (sorted largest first)
    pub file_paths: Vec<String>,
    /// Index into file_paths of the current file
    pub current_file_idx: usize,
    /// For the current file: total hunks
    pub current_file_hunks: usize,
    /// For the current file: which hunk we're on (1-based)
    pub current_hunk: usize,
    /// Collected hunk findings for the current file: (hunk_num, finding_text)
    pub current_file_findings: Vec<(usize, String)>,
    /// Completed file results: (file_path, parsed aggregate)
    pub completed_files: Vec<(String, FileAggregateResult)>,
    /// Batch summaries from completed batches
    pub batch_summaries: Vec<String>,
    /// Current batch number (1-based)
    pub current_batch: usize,
    /// Total batches
    pub total_batches: usize,
    /// Final review text (set when phase = "done")
    pub final_review: Option<String>,
}

impl ReviewState {
    pub fn new(pr_key: String, file_paths: Vec<String>, mode: ReviewMode) -> Self {
        let total_batches = (file_paths.len() + 4) / 5; // ceil division by 5
        Self {
            schema_version: CURRENT_SCHEMA_VERSION,
            pr_key,
            mode,
            phase: "hunk-review".into(),
            file_paths,
            current_file_idx: 0,
            current_file_hunks: 0,
            current_hunk: 0,
            current_file_findings: vec![],
            completed_files: vec![],
            batch_summaries: vec![],
            current_batch: 1,
            total_batches,
            final_review: None,
        }
    }

    pub fn is_done(&self) -> bool {
        self.phase == "done"
    }

    /// Whether all files have been processed in the hunk-review phase.
    pub fn all_files_done(&self) -> bool {
        self.current_file_idx >= self.file_paths.len()
    }

    /// Whether the current file's hunks are all done.
    pub fn current_file_hunks_done(&self) -> bool {
        self.current_hunk >= self.current_file_hunks
    }
}

/// Save review state to SQLite.
pub async fn save_state(conn: &libsql::Connection, state: &ReviewState) -> Result<(), AppError> {
    let json = serde_json::to_string(state)
        .map_err(|e| AppError::Ai(format!("Failed to serialize review state: {}", e)))?;

    conn.execute(
        "INSERT OR REPLACE INTO settings (key, value) VALUES ('review_state', ?1)",
        libsql::params![json],
    )
    .await?;

    Ok(())
}

/// Load review state from SQLite (returns None if no saved state).
pub async fn load_state(conn: &libsql::Connection) -> Result<Option<ReviewState>, AppError> {
    let json: Option<String> = crate::cache::get_setting(conn, "review_state").await?;

    match json {
        Some(j) => match serde_json::from_str::<ReviewState>(&j) {
            // A state from an older schema may deserialize cleanly (new fields
            // fall back to their serde defaults) yet be incoherent to resume —
            // e.g. its findings predate confidence scoring. Discard it so the
            // next run starts fresh rather than resuming half-old data.
            Ok(state) if state.schema_version < CURRENT_SCHEMA_VERSION => {
                let _ = clear_state(conn).await;
                Ok(None)
            }
            Ok(state) => Ok(Some(state)),
            // Schema drift that fails to parse outright: discard rather than
            // erroring out so the user can start a fresh review.
            Err(_) => {
                let _ = clear_state(conn).await;
                Ok(None)
            }
        },
        None => Ok(None),
    }
}

/// Clear saved review state.
pub async fn clear_state(conn: &libsql::Connection) -> Result<(), AppError> {
    conn.execute("DELETE FROM settings WHERE key = 'review_state'", ())
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_state_stamps_current_schema_version() {
        let s = ReviewState::new("k".into(), vec!["a".into()], ReviewMode::Fast);
        assert_eq!(s.schema_version, CURRENT_SCHEMA_VERSION);
    }

    #[test]
    fn legacy_state_without_schema_version_defaults_to_zero() {
        // A state persisted before `schema_version` existed deserializes with
        // the serde default (0), which is below CURRENT_SCHEMA_VERSION and so is
        // discarded on load. Here we just confirm the default round-trips.
        let json = r#"{
            "prKey":"k","phase":"hunk-review","filePaths":["a"],
            "currentFileIdx":0,"currentFileHunks":0,"currentHunk":0,
            "currentFileFindings":[],"completedFiles":[],"batchSummaries":[],
            "currentBatch":1,"totalBatches":1,"finalReview":null
        }"#;
        let state: ReviewState = serde_json::from_str(json).expect("deserialize legacy state");
        assert_eq!(state.schema_version, 0);
        assert!(state.schema_version < CURRENT_SCHEMA_VERSION);
    }
}
