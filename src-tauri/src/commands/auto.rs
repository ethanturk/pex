//! Phase 4 automation commands: deciding which PRs need an auto-review, and
//! auto-posting the highest-confidence Blocking findings after one.

use crate::review::engine::{post_single_finding, select_auto_post_findings, should_auto_review, Finding};
use crate::review::feedback::{self, Verdict};
use crate::AppState;
use tauri::State;

fn get_client(state: &AppState) -> Result<crate::provider::GitClient, String> {
    let guard = state.client.lock().map_err(|e| e.to_string())?;
    guard
        .as_ref()
        .cloned()
        .ok_or_else(|| "Not authenticated".to_string())
}

fn pr_key(org_url: &str, project_id: &str, repo_id: &str, pr_id: i64) -> String {
    format!("{}/{}/{}/{}", org_url, project_id, repo_id, pr_id)
}

/// A PR plus its current iteration count, as the frontend sees them.
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrIteration {
    pub pr_id: i64,
    pub iteration_count: i32,
}

/// Given the PRs currently listed, return the IDs that should be auto-reviewed:
/// only when auto-review is enabled and a PR has a newer iteration than the last
/// one we reviewed (or was never reviewed). Returns an empty list when disabled.
#[tauri::command]
pub async fn auto_review_candidates(
    state: State<'_, AppState>,
    project_id: String,
    repo_id: String,
    prs: Vec<PrIteration>,
) -> Result<Vec<i64>, String> {
    let conn = state.db.conn();
    let enabled = crate::ai::read_auto_review(&conn)
        .await
        .map_err(|e: crate::AppError| e.to_string())?;
    if !enabled {
        return Ok(Vec::new());
    }
    let client = get_client(&state)?;
    let mut out = Vec::new();
    for pr in prs {
        let key = pr_key(&client.org_url(), &project_id, &repo_id, pr.pr_id);
        let last = feedback::get_last_reviewed_iteration(&conn, &key).await;
        if should_auto_review(true, last, pr.iteration_count) {
            out.push(pr.pr_id);
        }
    }
    Ok(out)
}

/// After an auto-review completes, post the highest-confidence Blocking findings
/// (Blocking tier at/above the configured confidence floor) as individual
/// comments and record them as accepted. Everything else is left for the human
/// gate. Returns the number of findings posted. A no-op when auto-post is off.
#[tauri::command]
pub async fn auto_post_review_findings(
    state: State<'_, AppState>,
    project_id: String,
    repo_id: String,
    pr_id: i64,
    findings: Vec<Finding>,
) -> Result<usize, String> {
    let conn = state.db.conn();
    let (enabled, floor) = (
        crate::ai::read_auto_post_blocking(&conn)
            .await
            .map_err(|e: crate::AppError| e.to_string())?,
        crate::ai::read_auto_post_confidence(&conn)
            .await
            .map_err(|e: crate::AppError| e.to_string())?,
    );
    if !enabled {
        return Ok(0);
    }

    let client = get_client(&state)?;
    let key = pr_key(&client.org_url(), &project_id, &repo_id, pr_id);

    let selected = select_auto_post_findings(&findings, floor);
    let mut posted = 0usize;
    for finding in selected {
        if post_single_finding(&client, &project_id, &repo_id, pr_id, finding)
            .await
            .is_err()
        {
            continue;
        }
        posted += 1;
        // Auto-posting is an acceptance; record it so calibration sees it and it
        // isn't re-flagged as a fresh candidate next run.
        let fp = feedback::fingerprint(&finding.file_path, &finding.comment);
        let _ = feedback::record_verdict(
            &conn,
            &key,
            &fp,
            Verdict::Accepted,
            &finding.file_path,
            severity_str(finding.severity),
            tier_str(finding.tier),
            finding.confidence,
            &finding.comment,
            &finding.sources.join(","),
        )
        .await;
    }
    Ok(posted)
}

fn severity_str(s: crate::review::engine::Severity) -> &'static str {
    use crate::review::engine::Severity::*;
    match s {
        Critical => "critical",
        Moderate => "moderate",
        Minor => "minor",
    }
}

fn tier_str(t: crate::review::engine::Tier) -> &'static str {
    use crate::review::engine::Tier::*;
    match t {
        Blocking => "blocking",
        ShouldFix => "should-fix",
        Nit => "nit",
        Fyi => "fyi",
    }
}
