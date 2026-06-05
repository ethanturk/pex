//! Commands for the Phase 3 trust feedback loop: recording reviewer verdicts on
//! findings and reading back calibration metrics.

use crate::review::feedback::{self, CalibrationStats, Verdict};
use crate::AppState;
use tauri::State;

fn get_client(state: &AppState) -> Result<crate::provider::GitClient, String> {
    let guard = state.client.lock().map_err(|e| e.to_string())?;
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
    sources: Vec<String>,
) -> Result<(), String> {
    let verdict = Verdict::from_str(&verdict).map_err(|e| e.to_string())?;
    let client = get_client(&state)?;
    let key = pr_key(&client.org_url(), &project_id, &repo_id, pr_id);
    let fp = feedback::fingerprint(&file_path, &comment);

    let conn = state.db.conn();
    feedback::record_verdict(
        &conn,
        &key,
        &fp,
        verdict,
        &file_path,
        &severity,
        &tier,
        confidence,
        &comment,
        &sources.join(","),
    )
    .await
    .map_err(|e| e.to_string())
}

/// Remove a recorded verdict for a finding. This is intentionally narrower
/// than clearing all feedback: undoing a dismissal should make the finding
/// eligible again without counting it as accepted.
#[tauri::command]
pub async fn clear_finding_verdict(
    state: State<'_, AppState>,
    project_id: String,
    repo_id: String,
    pr_id: i64,
    file_path: String,
    comment: String,
) -> Result<(), String> {
    let client = get_client(&state)?;
    let key = pr_key(&client.org_url(), &project_id, &repo_id, pr_id);
    let fp = feedback::fingerprint(&file_path, &comment);

    let conn = state.db.conn();
    feedback::clear_verdict(&conn, &key, &fp)
        .await
        .map_err(|e| e.to_string())
}

/// Aggregate calibration metrics across all recorded verdicts.
#[tauri::command]
pub async fn get_review_calibration(
    state: State<'_, AppState>,
) -> Result<CalibrationStats, String> {
    let conn = state.db.conn();
    feedback::calibration(&conn).await.map_err(|e| e.to_string())
}

/// Clear all recorded verdicts (resets calibration metrics and suppression).
#[tauri::command]
pub async fn clear_review_feedback(state: State<'_, AppState>) -> Result<(), String> {
    let conn = state.db.conn();
    feedback::clear_all(&conn).await.map_err(|e| e.to_string())
}

/// The directory where opt-in review diagnostic traces are written.
#[tauri::command]
pub async fn get_diagnostics_dir() -> Result<String, String> {
    crate::cache::diagnostics_dir().map_err(|e| e.to_string())
}
