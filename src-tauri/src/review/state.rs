use crate::AppError;
use crate::review::engine::FileAggregateResult;
use serde::{Deserialize, Serialize};

/// Serializable progress state for resumable PR reviews.
/// Persisted to SQLite so the user can continue after cancellation or crash.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewState {
    /// The PR URL or identifier
    pub pr_key: String,
    /// Phase: "hunk-review", "batch-aggregate", "synthesis", "done"
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
    pub fn new(pr_key: String, file_paths: Vec<String>) -> Self {
        let total_batches = (file_paths.len() + 4) / 5; // ceil division by 5
        Self {
            pr_key,
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
pub fn save_state(conn: &rusqlite::Connection, state: &ReviewState) -> Result<(), AppError> {
    let json = serde_json::to_string(state)
        .map_err(|e| AppError::Ai(format!("Failed to serialize review state: {}", e)))?;

    conn.execute(
        "INSERT OR REPLACE INTO settings (key, value) VALUES ('review_state', ?1)",
        rusqlite::params![json],
    )?;

    Ok(())
}

/// Load review state from SQLite (returns None if no saved state).
pub fn load_state(conn: &rusqlite::Connection) -> Result<Option<ReviewState>, AppError> {
    let json: Option<String> = conn
        .query_row(
            "SELECT value FROM settings WHERE key = 'review_state'",
            [],
            |row| row.get(0),
        )
        .ok();

    match json {
        Some(j) => match serde_json::from_str::<ReviewState>(&j) {
            Ok(state) => Ok(Some(state)),
            // Schema drift: discard the saved state instead of erroring out so
            // the user can start a fresh review. Old multi-section markdown
            // summaries won't fit the new completed_files shape.
            Err(_) => {
                let _ = clear_state(conn);
                Ok(None)
            }
        },
        None => Ok(None),
    }
}

/// Clear saved review state.
pub fn clear_state(conn: &rusqlite::Connection) -> Result<(), AppError> {
    conn.execute("DELETE FROM settings WHERE key = 'review_state'", [])?;
    Ok(())
}
