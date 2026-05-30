//! Commands for the Phase 3 trust feedback loop: recording reviewer verdicts on
//! findings and reading back calibration metrics.

use crate::review::feedback::{self, CalibrationStats, Verdict};
use crate::AppState;
use tauri::State;

fn get_client(state: &AppState) -> Result<crate::ado::AdoClient, String> {
    let guard = state.ado_client.lock().map_err(|e| e.to_string())?;
    guard
        .as_ref()
        .cloned()
        .ok_or_else(|| "Not authenticated".to_string())
}

/// Build the PR key exactly as `run_review` does so verdict fingerprints and
/// suppression line up across the two paths.
fn pr_key(org_url: &str, project_id: &str, repo_id: &str, pr_id: i64) -> String {
    format!("{}/{}/{}/{}", org_url, project_id, repo_id, pr_id)
}

/// Record a reviewer's verdict on a finding (accepted / dismissed / edited).
/// Dismissed findings are suppressed on future review runs for this PR.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn record_finding_verdict(
    state: State<'_, AppState>,
    project_id: String,
    repo_id: String,
    pr_id: i64,
    verdict: String,
    file_path: String,
    comment: String,
    severity: String,
    tier: String,
    confidence: u8,
) -> Result<(), String> {
    let verdict = Verdict::from_str(&verdict).map_err(|e| e.to_string())?;
    let client = get_client(&state)?;
    let key = pr_key(&client.org_url, &project_id, &repo_id, pr_id);
    let fp = feedback::fingerprint(&file_path, &comment);

    let db = state.db.lock().map_err(|e| e.to_string())?;
    feedback::record_verdict(
        &db, &key, &fp, verdict, &file_path, &severity, &tier, confidence, &comment,
    )
    .map_err(|e| e.to_string())
}

/// Aggregate calibration metrics across all recorded verdicts.
#[tauri::command]
pub async fn get_review_calibration(
    state: State<'_, AppState>,
) -> Result<CalibrationStats, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    feedback::calibration(&db).map_err(|e| e.to_string())
}

/// Clear all recorded verdicts (resets calibration metrics and suppression).
#[tauri::command]
pub async fn clear_review_feedback(state: State<'_, AppState>) -> Result<(), String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    feedback::clear_all(&db).map_err(|e| e.to_string())
}
