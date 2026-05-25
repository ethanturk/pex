use crate::review::engine::{self, FileInput, ReviewInput, ReviewOutput};
use crate::AppState;
use crate::cache::standards_cache::StandardsCacheKey;
use crate::diff::engine::DiffView;
use tauri::{Emitter, State};

/// Start a native multi-pass PR review. Streams progress via `review-progress` events.
#[tauri::command]
pub async fn start_review(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    project_id: String,
    repo_id: String,
    pr_id: i64,
    pr_title: String,
) -> Result<ReviewOutput, String> {
    // Gather the ADO client and context
    let (client, org_url) = {
        let ado = state.ado_client.lock().map_err(|e| e.to_string())?;
        let client = ado
            .as_ref()
            .ok_or_else(|| "Not logged in. Connect to an ADO org first.".to_string())?
            .clone();
        let org_url = client.org_url.clone();
        (client, org_url)
    };

    // Get the AI provider
    let provider = {
        let ai_mgr_lock = state.ai_manager.lock().map_err(|e| e.to_string())?;
        ai_mgr_lock
            .as_ref()
            .and_then(|mgr| mgr.provider_clone())
            .ok_or_else(|| "AI not configured. Set up AI settings in Preferences.".to_string())?
    };

    // Fetch PR files
    let pr_files_result = client
        .get_pr_files(&project_id, &repo_id, pr_id, 1)
        .await
        .map_err(|e| e.to_string())?;

    // Fetch diffs for each file
    let mut file_inputs: Vec<FileInput> = Vec::new();
    for file in &pr_files_result.files {
        match client
            .get_file_diff(&project_id, &repo_id, pr_id, &file.item.path, 1, DiffView::Inline)
            .await
        {
            Ok(diff) => {
                file_inputs.push(FileInput {
                    path: diff.path,
                    old_content: diff.old_content,
                    new_content: diff.new_content,
                });
            }
            Err(e) => {
                let _ = app.emit(
                    "review-progress",
                    serde_json::json!({
                        "phase": "file-skipped",
                        "detail": format!("Skipping {}: {}", file.item.path, e),
                    }),
                );
            }
        }
    }

    if file_inputs.is_empty() {
        return Err("No files with diffs found in this PR.".into());
    }

    // Load standards from cache (use a wildcard key for the org-level cache)
    let standards = {
        let sc = &state.standards_cache;
        // Try to get standards for the first file's directory
        let key = StandardsCacheKey {
            org_url: org_url.clone(),
            project_id: project_id.clone(),
            repo_id: repo_id.clone(),
            commit: String::new(),
            path: String::new(),
        };
        sc.get(&key).unwrap_or_default().unwrap_or_default()
    };

    let input = ReviewInput {
        pr_key: format!("{}/{}/{}/{}", org_url, project_id, repo_id, pr_id),
        pr_title,
        files: file_inputs,
        standards,
        project_id: project_id.clone(),
        repo_id: repo_id.clone(),
        pr_id,
    };

    // Run review — the engine handles all the streaming
    let output = engine::run_review(app.clone(), provider, input, &state.db).await
        .map_err(|e| e.to_string())?;

    let _ = app.emit(
        "review-done",
        serde_json::json!({
            "success": true,
            "summary": output.summary,
            "findings": output.findings,
        }),
    );

    Ok(output)
}

/// Post a completed review's findings to ADO.
/// First runs the review, then posts findings as comments.
#[tauri::command]
pub async fn start_review_post(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    project_id: String,
    repo_id: String,
    pr_id: i64,
    pr_title: String,
) -> Result<(), String> {
    // Run the review first (reuses start_review logic inline)
    let (client, org_url) = {
        let ado = state.ado_client.lock().map_err(|e| e.to_string())?;
        let client = ado
            .as_ref()
            .ok_or_else(|| "Not logged in.".to_string())?
            .clone();
        let org_url = client.org_url.clone();
        (client, org_url)
    };

    let provider = {
        let ai_mgr_lock = state.ai_manager.lock().map_err(|e| e.to_string())?;
        ai_mgr_lock
            .as_ref()
            .and_then(|mgr| mgr.provider_clone())
            .ok_or_else(|| "AI not configured.".to_string())?
    };

    let pr_files_result = client
        .get_pr_files(&project_id, &repo_id, pr_id, 1)
        .await
        .map_err(|e| e.to_string())?;

    let mut file_inputs: Vec<FileInput> = Vec::new();
    for file in &pr_files_result.files {
        if let Ok(diff) = client
            .get_file_diff(&project_id, &repo_id, pr_id, &file.item.path, 1, DiffView::Inline)
            .await
        {
            file_inputs.push(FileInput {
                path: diff.path,
                old_content: diff.old_content,
                new_content: diff.new_content,
            });
        }
    }

    if file_inputs.is_empty() {
        return Err("No files with diffs found.".into());
    }

    let standards = {
        let sc = &state.standards_cache;
        let key = StandardsCacheKey {
            org_url: org_url.clone(),
            project_id: project_id.clone(),
            repo_id: repo_id.clone(),
            commit: String::new(),
            path: String::new(),
        };
        sc.get(&key).unwrap_or_default().unwrap_or_default()
    };

    let input = ReviewInput {
        pr_key: format!("{}/{}/{}/{}", org_url, project_id, repo_id, pr_id),
        pr_title,
        files: file_inputs,
        standards,
        project_id: project_id.clone(),
        repo_id: repo_id.clone(),
        pr_id,
    };

    let output = engine::run_review(app.clone(), provider, input, &state.db).await
        .map_err(|e| e.to_string())?;

    // Post to ADO
    let _ = app.emit(
        "review-progress",
        serde_json::json!({
            "phase": "posting",
            "detail": "Posting findings to ADO...",
        }),
    );

    engine::post_findings(&output.findings, &output.summary, &project_id, &repo_id, pr_id, &client)
        .await
        .map_err(|e| e.to_string())?;

    let _ = app.emit(
        "review-post-done",
        serde_json::json!({
            "success": true,
            "message": format!("Posted {} findings to ADO.", output.findings.len()),
        }),
    );

    // Clear saved state
    {
        let db = state.db.lock().map_err(|e| e.to_string())?;
        crate::review::state::clear_state(&db).map_err(|e: crate::AppError| e.to_string())?;
    }

    Ok(())
}

/// Cancel a running review. Clears saved state.
#[tauri::command]
pub async fn cancel_review(state: State<'_, AppState>) -> Result<(), String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    crate::review::state::clear_state(&db).map_err(|e: crate::AppError| e.to_string())
}

/// Check if there's a saved review state that can be resumed.
#[tauri::command]
pub async fn get_saved_review(state: State<'_, AppState>) -> Result<Option<crate::review::state::ReviewState>, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    crate::review::state::load_state(&db).map_err(|e: crate::AppError| e.to_string())
}

/// Clear any saved review state.
#[tauri::command]
pub async fn clear_saved_review(state: State<'_, AppState>) -> Result<(), String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    crate::review::state::clear_state(&db).map_err(|e: crate::AppError| e.to_string())
}
