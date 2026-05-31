use crate::cache::diff_cache::{DiffCache, DiffCacheKey};
use crate::cache::standards_cache::StandardsCacheKey;
use crate::diff::engine::DiffView;
use crate::review::engine::{self, FileInput, ReviewInput, ReviewOutput};
use crate::review::state::ReviewMode;
use crate::AppState;
use std::sync::atomic::Ordering;
use tauri::{Emitter, State};

const DEFAULT_DIFF_FETCH_CONCURRENCY: usize = 6;

fn read_diff_fetch_concurrency(db: &std::sync::Mutex<rusqlite::Connection>) -> usize {
    match db.lock() {
        Ok(c) => {
            crate::ai::read_hunk_concurrency(&c).unwrap_or(DEFAULT_DIFF_FETCH_CONCURRENCY as u32)
        }
        Err(_) => DEFAULT_DIFF_FETCH_CONCURRENCY as u32,
    }
    .max(1) as usize
}

/// When incremental review is enabled and this PR has been reviewed before,
/// narrow `all_paths` to just the files changed since the last reviewed
/// iteration. Falls back to the full set whenever incremental can't safely
/// apply (disabled, no prior iteration, no forward delta, or the delta doesn't
/// intersect the current file list) so we never silently skip everything.
async fn incremental_paths(
    state: &AppState,
    client: &crate::provider::GitClient,
    pr_key: &str,
    project_id: &str,
    repo_id: &str,
    pr_id: i64,
    iteration: i32,
    all_paths: Vec<String>,
) -> Vec<String> {
    let (enabled, last) = match state.db.lock() {
        Ok(db) => (
            crate::ai::read_incremental_review(&db).unwrap_or(false),
            crate::review::feedback::get_last_reviewed_iteration(&db, pr_key),
        ),
        Err(_) => (false, None),
    };
    if !enabled {
        return all_paths;
    }
    let Some(last) = last else {
        return all_paths;
    };
    if last >= iteration {
        return all_paths;
    }
    let changed = match client
        .changed_paths_since_iteration(project_id, repo_id, pr_id, last, iteration)
        .await
    {
        Ok(c) if !c.is_empty() => c,
        _ => return all_paths,
    };
    let filtered: Vec<String> = all_paths
        .iter()
        .filter(|p| changed.contains(p.trim_start_matches('/')))
        .cloned()
        .collect();
    if filtered.is_empty() {
        all_paths
    } else {
        filtered
    }
}

/// Persist the iteration we just reviewed so the next incremental run knows the
/// baseline. Best-effort: a failure here only means the next run is non-incremental.
fn remember_reviewed_iteration(state: &AppState, pr_key: &str, iteration: i32) {
    if let Ok(db) = state.db.lock() {
        let _ = crate::review::feedback::set_last_reviewed_iteration(&db, pr_key, iteration);
    }
}

/// Build the diagnostics sink for a run: a JSONL trace when `ai_diagnostics` is
/// enabled, otherwise a no-op. The run id ties the trace to the PR and a
/// timestamp so successive runs don't collide.
fn make_diagnostics(
    state: &AppState,
    pr_key: &str,
) -> crate::review::diagnostics::Diagnostics {
    use crate::review::diagnostics::Diagnostics;
    let enabled = state
        .db
        .lock()
        .ok()
        .and_then(|db| crate::ai::read_ai_diagnostics(&db).ok())
        .unwrap_or(false);
    if !enabled {
        return Diagnostics::disabled();
    }
    let dir = match crate::cache::diagnostics_dir() {
        Ok(d) => d,
        Err(_) => return Diagnostics::disabled(),
    };
    // pr_key tail (the numeric PR id) + timestamp keeps the filename readable.
    let pr_tail = pr_key.rsplit('/').next().unwrap_or("pr");
    let run_id = format!(
        "{}-pr{}",
        chrono::Utc::now().format("%Y%m%dT%H%M%S"),
        pr_tail
    );
    Diagnostics::create(&dir, &run_id)
}

async fn latest_iteration(
    client: &crate::provider::GitClient,
    project_id: &str,
    repo_id: &str,
    pr_id: i64,
) -> i32 {
    client
        .get_iterations(project_id, repo_id, pr_id)
        .await
        .ok()
        .and_then(|iterations| iterations.into_iter().map(|i| i.id).max())
        .and_then(|id| i32::try_from(id).ok())
        .unwrap_or(1)
}

async fn fetch_file_inputs(
    app: &tauri::AppHandle,
    client: &crate::provider::GitClient,
    diff_cache: &DiffCache,
    org_url: &str,
    project_id: &str,
    repo_id: &str,
    pr_id: i64,
    iteration: i32,
    file_paths: Vec<String>,
    concurrency: usize,
    emit_skips: bool,
) -> Vec<FileInput> {
    let limit = concurrency.max(1);
    let total = file_paths.len();
    let mut completed = 0usize;
    let mut file_inputs = Vec::new();
    let mut misses = Vec::new();

    let _ = app.emit(
        "review-progress",
        serde_json::json!({
            "phase": "diff-fetch",
            "detail": format!("Preparing review diffs 0/{}", total),
            "fileNum": 0,
            "totalFiles": total,
        }),
    );

    for path in file_paths {
        let path = path.strip_prefix('/').unwrap_or(&path).to_string();
        let cache_key = DiffCacheKey {
            org_url: org_url.to_string(),
            project_id: project_id.to_string(),
            repo_id: repo_id.to_string(),
            pr_id,
            file_path: path.clone(),
            view: "inline".to_string(),
            iteration,
        };

        if let Some(diff) = diff_cache.get(&cache_key) {
            file_inputs.push(FileInput {
                path: diff.path,
                old_content: diff.old_content,
                new_content: diff.new_content,
            });
            completed += 1;
            let _ = app.emit(
                "review-progress",
                serde_json::json!({
                    "phase": "diff-fetch",
                    "detail": format!("Preparing review diffs {}/{} (cache hit)", completed, total),
                    "fileNum": completed,
                    "totalFiles": total,
                }),
            );
        } else {
            misses.push(path);
        }
    }

    for chunk in misses.chunks(limit) {
        let mut handles = Vec::new();
        for path in chunk {
            let client = client.clone();
            let project_id = project_id.to_string();
            let repo_id = repo_id.to_string();
            let path = path.clone();
            handles.push((
                path.clone(),
                tokio::spawn(async move {
                    client
                        .get_file_diff(
                            &project_id,
                            &repo_id,
                            pr_id,
                            &path,
                            iteration,
                            DiffView::Inline,
                        )
                        .await
                }),
            ));
        }

        for (path, handle) in handles {
            match handle.await {
                Ok(Ok(diff)) => {
                    let cache_key = DiffCacheKey {
                        org_url: org_url.to_string(),
                        project_id: project_id.to_string(),
                        repo_id: repo_id.to_string(),
                        pr_id,
                        file_path: path.clone(),
                        view: "inline".to_string(),
                        iteration,
                    };
                    diff_cache.put(cache_key, diff.clone());
                    file_inputs.push(FileInput {
                        path: diff.path,
                        old_content: diff.old_content,
                        new_content: diff.new_content,
                    });
                }
                Ok(Err(e)) => {
                    if emit_skips {
                        let _ = app.emit(
                            "review-progress",
                            serde_json::json!({
                                "phase": "file-skipped",
                                "detail": format!("Skipping {}: {}", path, e),
                            }),
                        );
                    }
                }
                Err(e) => {
                    if emit_skips {
                        let _ = app.emit(
                            "review-progress",
                            serde_json::json!({
                                "phase": "file-skipped",
                                "detail": format!("Skipping {}: diff task failed: {}", path, e),
                            }),
                        );
                    }
                }
            }
            completed += 1;
            let _ = app.emit(
                "review-progress",
                serde_json::json!({
                    "phase": "diff-fetch",
                    "detail": format!("Preparing review diffs {}/{}", completed, total),
                    "fileNum": completed,
                    "totalFiles": total,
                }),
            );
        }
    }

    file_inputs
}

/// Start a native multi-pass PR review. Streams progress via `review-progress` events.
#[tauri::command]
pub async fn start_review(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    project_id: String,
    repo_id: String,
    pr_id: i64,
    pr_title: String,
    mode: Option<ReviewMode>,
    enabled_specialists: Option<Vec<String>>,
) -> Result<ReviewOutput, String> {
    let mode = mode.unwrap_or_default();
    // Gather the ADO client and context
    let (client, org_url) = {
        let ado = state.client.lock().map_err(|e| e.to_string())?;
        let client = ado
            .as_ref()
            .ok_or_else(|| "Not logged in. Connect to an ADO org first.".to_string())?
            .clone();
        let org_url = client.org_url().to_string();
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

    let iteration = latest_iteration(&client, &project_id, &repo_id, pr_id).await;
    let pr_key = format!("{}/{}/{}/{}", org_url, project_id, repo_id, pr_id);

    // Fetch PR files
    let pr_files_result = client
        .get_pr_files(&project_id, &repo_id, pr_id, iteration)
        .await
        .map_err(|e| e.to_string())?;

    let paths = incremental_paths(
        &state,
        &client,
        &pr_key,
        &project_id,
        &repo_id,
        pr_id,
        iteration,
        pr_files_result
            .files
            .iter()
            .map(|f| f.item.path.clone())
            .collect(),
    )
    .await;

    let file_inputs = fetch_file_inputs(
        &app,
        &client,
        &state.diff_cache,
        &org_url,
        &project_id,
        &repo_id,
        pr_id,
        iteration,
        paths,
        read_diff_fetch_concurrency(&state.db),
        true,
    )
    .await;

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
        pr_key: pr_key.clone(),
        pr_title,
        files: file_inputs,
        standards,
        project_id: project_id.clone(),
        repo_id: repo_id.clone(),
        pr_id,
        mode,
        enabled_specialists,
    };

    // Clear any prior cancel signal before starting a fresh run.
    state.review_cancel.store(false, Ordering::SeqCst);
    let cancel = state.review_cancel.clone();

    // Run review — the engine handles all the streaming
    let diag = make_diagnostics(&state, &pr_key);
    let output = engine::run_review(app.clone(), provider, input, &state.db, cancel, diag)
        .await
        .map_err(|e| e.to_string())?;

    // Record the baseline for the next incremental run.
    remember_reviewed_iteration(&state, &pr_key, iteration);

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
    mode: Option<ReviewMode>,
) -> Result<(), String> {
    let mode = mode.unwrap_or_default();
    // Run the review first (reuses start_review logic inline)
    let (client, org_url) = {
        let ado = state.client.lock().map_err(|e| e.to_string())?;
        let client = ado
            .as_ref()
            .ok_or_else(|| "Not logged in.".to_string())?
            .clone();
        let org_url = client.org_url().to_string();
        (client, org_url)
    };

    let provider = {
        let ai_mgr_lock = state.ai_manager.lock().map_err(|e| e.to_string())?;
        ai_mgr_lock
            .as_ref()
            .and_then(|mgr| mgr.provider_clone())
            .ok_or_else(|| "AI not configured.".to_string())?
    };

    let iteration = latest_iteration(&client, &project_id, &repo_id, pr_id).await;
    let pr_key = format!("{}/{}/{}/{}", org_url, project_id, repo_id, pr_id);
    let pr_files_result = client
        .get_pr_files(&project_id, &repo_id, pr_id, iteration)
        .await
        .map_err(|e| e.to_string())?;

    let paths = incremental_paths(
        &state,
        &client,
        &pr_key,
        &project_id,
        &repo_id,
        pr_id,
        iteration,
        pr_files_result
            .files
            .iter()
            .map(|f| f.item.path.clone())
            .collect(),
    )
    .await;

    let file_inputs = fetch_file_inputs(
        &app,
        &client,
        &state.diff_cache,
        &org_url,
        &project_id,
        &repo_id,
        pr_id,
        iteration,
        paths,
        read_diff_fetch_concurrency(&state.db),
        false,
    )
    .await;

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
        pr_key: pr_key.clone(),
        pr_title,
        files: file_inputs,
        standards,
        project_id: project_id.clone(),
        repo_id: repo_id.clone(),
        pr_id,
        mode,
        // The auto/post path runs the full specialist roster.
        enabled_specialists: None,
    };

    state.review_cancel.store(false, Ordering::SeqCst);
    let cancel = state.review_cancel.clone();
    let diag = make_diagnostics(&state, &pr_key);
    let output = engine::run_review(app.clone(), provider, input, &state.db, cancel, diag)
        .await
        .map_err(|e| e.to_string())?;

    // Record the baseline for the next incremental run.
    remember_reviewed_iteration(&state, &pr_key, iteration);

    // Post to ADO
    let _ = app.emit(
        "review-progress",
        serde_json::json!({
            "phase": "posting",
            "detail": "Posting findings to ADO...",
        }),
    );

    engine::post_findings(
        &output.findings,
        &output.summary,
        &project_id,
        &repo_id,
        pr_id,
        &client,
    )
    .await
    .map_err(|e| e.to_string())?;

    // Opt-in "pull forward": when the review found a blocking issue and the user
    // enabled auto-vote, cast a "wait for author" vote so the PR can't be
    // approved out from under an unaddressed blocker.
    let auto_vote = {
        let db = state.db.lock().map_err(|e| e.to_string())?;
        crate::ai::read_auto_vote_on_blocking(&db).unwrap_or(false)
    };
    let has_blocking = output
        .findings
        .iter()
        .any(|f| f.tier == crate::review::engine::Tier::Blocking);
    if auto_vote && has_blocking {
        if let Ok(me) = client.get_authenticated_user_id().await {
            let _ = client
                .update_reviewer_status(
                    &project_id,
                    &repo_id,
                    pr_id,
                    &me,
                    crate::ai::VOTE_WAIT_FOR_AUTHOR,
                )
                .await;
        }
    }

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

/// Cancel a running review. Signals the engine to stop between LLM calls and
/// clears any persisted resume state so a future run starts fresh.
#[tauri::command]
pub async fn cancel_review(state: State<'_, AppState>) -> Result<(), String> {
    state.review_cancel.store(true, Ordering::SeqCst);
    let db = state.db.lock().map_err(|e| e.to_string())?;
    crate::review::state::clear_state(&db).map_err(|e: crate::AppError| e.to_string())
}

/// Check if there's a saved review state that can be resumed.
#[tauri::command]
pub async fn get_saved_review(
    state: State<'_, AppState>,
) -> Result<Option<crate::review::state::ReviewState>, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    let saved =
        crate::review::state::load_state(&db).map_err(|e: crate::AppError| e.to_string())?;
    if saved.as_ref().map(|s| s.is_done()).unwrap_or(false) {
        crate::review::state::clear_state(&db).map_err(|e: crate::AppError| e.to_string())?;
        return Ok(None);
    }
    Ok(saved)
}

/// Clear any saved review state.
#[tauri::command]
pub async fn clear_saved_review(state: State<'_, AppState>) -> Result<(), String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    crate::review::state::clear_state(&db).map_err(|e: crate::AppError| e.to_string())
}
