use crate::cache::diff_cache::{DiffCache, DiffCacheKey};
use crate::cache::standards_cache::StandardsCacheKey;
use crate::diff::engine::DiffView;
use crate::review::engine::{self, FileInput, ReviewInput, ReviewOutput};
use crate::review::related::related_file_groups;
use crate::review::rules::{ReviewRuleMatch, ReviewRuleResolver, RuleDecision};
use crate::review::state::ReviewMode;
use crate::AppState;
use std::collections::HashMap;
use std::sync::atomic::Ordering;
use tauri::{Emitter, State};

const DEFAULT_DIFF_FETCH_CONCURRENCY: usize = 6;

async fn read_diff_fetch_concurrency(store: &crate::db::Store) -> usize {
    let conn = store.conn();
    crate::ai::read_hunk_concurrency(&conn)
        .await
        .unwrap_or(DEFAULT_DIFF_FETCH_CONCURRENCY as u32)
        .max(1) as usize
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewPreview {
    pub iteration: i32,
    pub total_files: usize,
    pub reviewable_files: usize,
    pub skipped_files: usize,
    pub total_hunks: usize,
    pub changed_lines: usize,
    pub files: Vec<ReviewPreviewFile>,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewPreviewFile {
    pub path: String,
    pub status: String,
    pub will_review: bool,
    pub skip_reason: Option<String>,
    pub hunk_count: usize,
    pub changed_lines: usize,
    pub rule_source: Option<String>,
    pub rule_pattern: Option<String>,
    pub rule_title: Option<String>,
    #[serde(default)]
    pub related_files: Vec<String>,
}

struct PreparedReview {
    iteration: i32,
    pr_key: String,
    preview: ReviewPreview,
    file_inputs: Vec<FileInput>,
    rules: HashMap<String, ReviewRuleMatch>,
    related_files: HashMap<String, Vec<String>>,
    standards: String,
}

fn normalize_review_path(path: &str) -> String {
    path.trim_start_matches('/').replace('\\', "/")
}

fn status_for_change(change_type: &str) -> String {
    match change_type {
        "add" => "add",
        "edit" => "edit",
        "delete" => "delete",
        "rename" => "rename",
        _ => "edit",
    }
    .to_string()
}

fn hunk_changed_lines(hunks: &[crate::diff::engine::DiffHunk]) -> usize {
    hunks
        .iter()
        .flat_map(|h| h.lines.iter())
        .filter(|line| line.kind == "+")
        .count()
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
    let (enabled, last) = {
        let conn = state.db.conn();
        (
            crate::ai::read_incremental_review(&conn).await.unwrap_or(false),
            crate::review::feedback::get_last_reviewed_iteration(&conn, pr_key).await,
        )
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
async fn remember_reviewed_iteration(state: &AppState, pr_key: &str, iteration: i32) {
    let conn = state.db.conn();
    let _ = crate::review::feedback::set_last_reviewed_iteration(&conn, pr_key, iteration).await;
}

/// Build the diagnostics sink for a run: a JSONL trace when `ai_diagnostics` is
/// enabled, otherwise a no-op. The run id ties the trace to the PR and a
/// timestamp so successive runs don't collide.
async fn make_diagnostics(
    state: &AppState,
    pr_key: &str,
) -> crate::review::diagnostics::Diagnostics {
    use crate::review::diagnostics::Diagnostics;
    let enabled = {
        let conn = state.db.conn();
        crate::ai::read_ai_diagnostics(&conn).await.unwrap_or(false)
    };
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

async fn load_rule_resolver(
    client: &crate::provider::GitClient,
    project_id: &str,
    repo_id: &str,
    commit_id: &str,
) -> Result<ReviewRuleResolver, String> {
    match client
        .get_file_at_commit(project_id, repo_id, commit_id, ".pex/review-rules.json")
        .await
    {
        Ok(Some(raw)) if !raw.trim().is_empty() => ReviewRuleResolver::from_json(&raw),
        _ => Ok(ReviewRuleResolver::default()),
    }
}

#[allow(clippy::too_many_arguments)]
async fn prepare_review(
    app: &tauri::AppHandle,
    state: &AppState,
    client: &crate::provider::GitClient,
    org_url: &str,
    project_id: &str,
    repo_id: &str,
    pr_id: i64,
    emit_skips: bool,
    diag: &crate::review::diagnostics::Diagnostics,
) -> Result<PreparedReview, String> {
    let iteration = latest_iteration(client, project_id, repo_id, pr_id).await;
    let pr_key = format!("{}/{}/{}/{}", org_url, project_id, repo_id, pr_id);

    let pr_files_result = client
        .get_pr_files(project_id, repo_id, pr_id, iteration)
        .await
        .map_err(|e| e.to_string())?;

    let mut paths = incremental_paths(
        state,
        client,
        &pr_key,
        project_id,
        repo_id,
        pr_id,
        iteration,
        pr_files_result
            .files
            .iter()
            .map(|f| f.item.path.clone())
            .collect(),
    )
    .await;
    paths = paths
        .into_iter()
        .map(|p| normalize_review_path(&p))
        .collect();

    let statuses: HashMap<String, String> = pr_files_result
        .files
        .iter()
        .map(|f| {
            (
                normalize_review_path(&f.item.path),
                status_for_change(&f.change_type),
            )
        })
        .collect();

    if emit_skips {
        let _ = app.emit(
            "review-progress",
            serde_json::json!({
                "phase": "preflight",
                "stage": "rules",
                "detail": "Resolving review rules…",
            }),
        );
    }
    let resolver =
        load_rule_resolver(client, project_id, repo_id, &pr_files_result.commit_id).await?;

    if emit_skips {
        let _ = app.emit(
            "review-progress",
            serde_json::json!({
                "phase": "preflight",
                "stage": "group",
                "detail": "Grouping related files…",
            }),
        );
    }
    let related_files = related_file_groups(&paths);

    if emit_skips {
        let _ = app.emit(
            "review-progress",
            serde_json::json!({
                "phase": "preflight",
                "stage": "filter",
                "detail": format!("Applying rules to {} changed file(s)…", paths.len()),
                "totalFiles": paths.len(),
            }),
        );
    }

    let mut preview_files = Vec::new();
    let mut candidate_paths = Vec::new();
    let mut rules = HashMap::new();

    for path in &paths {
        let status = statuses
            .get(path)
            .cloned()
            .unwrap_or_else(|| "edit".to_string());
        match resolver.resolve(path, &status) {
            RuleDecision::Review(rule) => {
                if diag.is_enabled() {
                    diag.event(
                        "rule_match",
                        serde_json::json!({
                            "filePath": path,
                            "source": rule.source,
                            "pattern": rule.pattern,
                            "title": rule.title,
                        }),
                    );
                }
                candidate_paths.push(path.clone());
                rules.insert(path.clone(), rule.clone());
                preview_files.push(ReviewPreviewFile {
                    path: path.clone(),
                    status,
                    will_review: true,
                    skip_reason: None,
                    hunk_count: 0,
                    changed_lines: 0,
                    rule_source: Some(rule.source),
                    rule_pattern: rule.pattern,
                    rule_title: Some(rule.title),
                    related_files: related_files.get(path).cloned().unwrap_or_default(),
                });
            }
            RuleDecision::Skip { reason } => {
                if emit_skips {
                    let _ = app.emit(
                        "review-progress",
                        serde_json::json!({
                            "phase": "file-skipped",
                            "detail": format!("Skipping {}: {}", path, reason),
                        }),
                    );
                }
                preview_files.push(ReviewPreviewFile {
                    path: path.clone(),
                    status,
                    will_review: false,
                    skip_reason: Some(reason),
                    hunk_count: 0,
                    changed_lines: 0,
                    rule_source: None,
                    rule_pattern: None,
                    rule_title: None,
                    related_files: related_files.get(path).cloned().unwrap_or_default(),
                });
            }
        }
    }

    if emit_skips {
        let reviewable = candidate_paths.len();
        let skipped = preview_files.len().saturating_sub(reviewable);
        let _ = app.emit(
            "review-progress",
            serde_json::json!({
                "phase": "preflight",
                "stage": "filtered",
                "detail": format!(
                    "Rules applied — {reviewable} file(s) to review, {skipped} skipped"
                ),
                "reviewableFiles": reviewable,
                "skippedFiles": skipped,
            }),
        );
    }

    let fetched_inputs = fetch_file_inputs(
        app,
        client,
        &state.diff_cache,
        org_url,
        project_id,
        repo_id,
        pr_id,
        iteration,
        candidate_paths,
        read_diff_fetch_concurrency(&state.db).await,
        emit_skips,
    )
    .await;

    let mut file_inputs = Vec::new();
    let mut by_path: HashMap<String, FileInput> = fetched_inputs
        .into_iter()
        .map(|input| (normalize_review_path(&input.path), input))
        .collect();

    for preview in &mut preview_files {
        if !preview.will_review {
            if diag.is_enabled() {
                diag.event(
                    "preflight_file",
                    serde_json::to_value(&preview).unwrap_or_default(),
                );
            }
            continue;
        }
        let Some(input) = by_path.remove(&preview.path) else {
            preview.will_review = false;
            preview.skip_reason = Some("diffUnavailable".to_string());
            if diag.is_enabled() {
                diag.event(
                    "preflight_file",
                    serde_json::to_value(&preview).unwrap_or_default(),
                );
            }
            continue;
        };
        let hunks = crate::diff::engine::extract_hunks(&input.old_content, &input.new_content);
        preview.hunk_count = hunks.len();
        preview.changed_lines = hunk_changed_lines(&hunks);
        if hunks.is_empty() {
            preview.will_review = false;
            preview.skip_reason = Some("noChanges".to_string());
            if diag.is_enabled() {
                diag.event(
                    "preflight_file",
                    serde_json::to_value(&preview).unwrap_or_default(),
                );
            }
            continue;
        }
        file_inputs.push(input);
        if diag.is_enabled() {
            diag.event(
                "preflight_file",
                serde_json::to_value(&preview).unwrap_or_default(),
            );
        }
    }

    let standards = {
        let sc = &state.standards_cache;
        let key = StandardsCacheKey {
            org_url: org_url.to_string(),
            project_id: project_id.to_string(),
            repo_id: repo_id.to_string(),
            commit: String::new(),
            path: String::new(),
        };
        sc.get(&key).unwrap_or_default().unwrap_or_default()
    };

    let reviewable_files = preview_files.iter().filter(|f| f.will_review).count();
    let total_hunks = preview_files.iter().map(|f| f.hunk_count).sum();
    let changed_lines = preview_files.iter().map(|f| f.changed_lines).sum();
    let preview = ReviewPreview {
        iteration,
        total_files: preview_files.len(),
        reviewable_files,
        skipped_files: preview_files.len().saturating_sub(reviewable_files),
        total_hunks,
        changed_lines,
        files: preview_files,
    };

    Ok(PreparedReview {
        iteration,
        pr_key,
        preview,
        file_inputs,
        rules,
        related_files,
        standards,
    })
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

    if emit_skips {
        let _ = app.emit(
            "review-progress",
            serde_json::json!({
                "phase": "diff-fetch",
                "detail": format!("Preparing review diffs 0/{}", total),
                "fileNum": 0,
                "totalFiles": total,
            }),
        );
    }

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
            if emit_skips {
                let _ = app.emit(
                    "review-progress",
                    serde_json::json!({
                        "phase": "diff-fetch",
                        "detail": format!("Preparing review diffs {}/{} (cache hit)", completed, total),
                        "fileNum": completed,
                        "totalFiles": total,
                    }),
                );
            }
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
            if emit_skips {
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
    }

    file_inputs
}

/// Preview the deterministic review preflight: which files will be reviewed,
/// which are skipped, and which path-scoped checklist each file matched.
#[tauri::command]
pub async fn preview_review(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    project_id: String,
    repo_id: String,
    pr_id: i64,
) -> Result<ReviewPreview, String> {
    let (client, org_url) = {
        let ado = state.client.lock().map_err(|e| e.to_string())?;
        let client = ado
            .as_ref()
            .ok_or_else(|| "Not logged in. Connect to an ADO org first.".to_string())?
            .clone();
        let org_url = client.org_url().to_string();
        (client, org_url)
    };
    let diag = crate::review::diagnostics::Diagnostics::disabled();
    let prepared = prepare_review(
        &app,
        &state,
        &client,
        &org_url,
        &project_id,
        &repo_id,
        pr_id,
        false,
        &diag,
    )
    .await?;
    Ok(prepared.preview)
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

    let pr_key = format!("{}/{}/{}/{}", org_url, project_id, repo_id, pr_id);
    let diag = make_diagnostics(&state, &pr_key).await;
    let prepared = prepare_review(
        &app,
        &state,
        &client,
        &org_url,
        &project_id,
        &repo_id,
        pr_id,
        true,
        &diag,
    )
    .await?;

    if prepared.file_inputs.is_empty() {
        return Err("No reviewable files with diffs found in this PR.".into());
    }

    let reviewed_iteration = prepared.iteration;
    let pr_title_for_persist = pr_title.clone();
    let input = ReviewInput {
        pr_key: prepared.pr_key.clone(),
        pr_title,
        files: prepared.file_inputs,
        standards: prepared.standards,
        project_id: project_id.clone(),
        repo_id: repo_id.clone(),
        pr_id,
        mode,
        enabled_specialists,
        rules: prepared.rules,
        related_files: prepared.related_files,
    };

    // Clear any prior cancel signal before starting a fresh run.
    state.review_cancel.store(false, Ordering::SeqCst);
    let cancel = state.review_cancel.clone();

    // Run review — the engine handles all the streaming
    let conn = state.db.conn();
    let output = engine::run_review(app.clone(), provider, input, &conn, cancel, diag)
        .await
        .map_err(|e| e.to_string())?;

    // Record the baseline for the next incremental run.
    remember_reviewed_iteration(&state, &pr_key, reviewed_iteration).await;

    // Persist the finished review so it survives restarts and stays visible
    // until completed. Best-effort: a storage failure only loses persistence,
    // not the live result the caller already has.
    {
        let conn = state.db.conn();
        let _ = crate::review::persist::save_review(
            &conn,
            &pr_key,
            &project_id,
            &repo_id,
            pr_id,
            &pr_title_for_persist,
            reviewed_iteration,
            crate::review::persist::STATUS_OUTSTANDING,
            &output,
        )
        .await;
    }

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

    let pr_key = format!("{}/{}/{}/{}", org_url, project_id, repo_id, pr_id);
    let diag = make_diagnostics(&state, &pr_key).await;
    let prepared = prepare_review(
        &app,
        &state,
        &client,
        &org_url,
        &project_id,
        &repo_id,
        pr_id,
        false,
        &diag,
    )
    .await?;

    if prepared.file_inputs.is_empty() {
        return Err("No reviewable files with diffs found.".into());
    }

    let reviewed_iteration = prepared.iteration;
    let pr_title_for_persist = pr_title.clone();
    let input = ReviewInput {
        pr_key: prepared.pr_key.clone(),
        pr_title,
        files: prepared.file_inputs,
        standards: prepared.standards,
        project_id: project_id.clone(),
        repo_id: repo_id.clone(),
        pr_id,
        mode,
        // The auto/post path runs the full specialist roster.
        enabled_specialists: None,
        rules: prepared.rules,
        related_files: prepared.related_files,
    };

    state.review_cancel.store(false, Ordering::SeqCst);
    let cancel = state.review_cancel.clone();
    let conn = state.db.conn();
    let output = engine::run_review(app.clone(), provider, input, &conn, cancel, diag)
        .await
        .map_err(|e| e.to_string())?;

    // Record the baseline for the next incremental run.
    remember_reviewed_iteration(&state, &pr_key, reviewed_iteration).await;

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
        let conn = state.db.conn();
        crate::ai::read_auto_vote_on_blocking(&conn)
            .await
            .unwrap_or(false)
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

    // Persist the posted review as completed — posting is one of the two ways a
    // stored review reaches the "completed" lifecycle (the other is the manual
    // "Mark completed" button). Best-effort.
    {
        let conn = state.db.conn();
        let _ = crate::review::persist::save_review(
            &conn,
            &pr_key,
            &project_id,
            &repo_id,
            pr_id,
            &pr_title_for_persist,
            reviewed_iteration,
            crate::review::persist::STATUS_COMPLETED,
            &output,
        )
        .await;
    }

    let _ = app.emit(
        "review-post-done",
        serde_json::json!({
            "success": true,
            "message": format!("Posted {} findings to ADO.", output.findings.len()),
        }),
    );

    // Clear saved (resume) state — the durable result now lives in `pr_reviews`.
    {
        let conn = state.db.conn();
        crate::review::state::clear_state(&conn)
            .await
            .map_err(|e: crate::AppError| e.to_string())?;
    }

    Ok(())
}

/// Cancel a running review. Signals the engine to stop between LLM calls and
/// clears any persisted resume state so a future run starts fresh.
#[tauri::command]
pub async fn cancel_review(state: State<'_, AppState>) -> Result<(), String> {
    state.review_cancel.store(true, Ordering::SeqCst);
    let conn = state.db.conn();
    crate::review::state::clear_state(&conn)
        .await
        .map_err(|e: crate::AppError| e.to_string())
}

/// Check if there's a saved review state that can be resumed.
#[tauri::command]
pub async fn get_saved_review(
    state: State<'_, AppState>,
) -> Result<Option<crate::review::state::ReviewState>, String> {
    let conn = state.db.conn();
    let saved =
        crate::review::state::load_state(&conn).await.map_err(|e: crate::AppError| e.to_string())?;
    if saved.as_ref().map(|s| s.is_done()).unwrap_or(false) {
        crate::review::state::clear_state(&conn)
            .await
            .map_err(|e: crate::AppError| e.to_string())?;
        return Ok(None);
    }
    Ok(saved)
}

/// Clear any saved review state.
#[tauri::command]
pub async fn clear_saved_review(state: State<'_, AppState>) -> Result<(), String> {
    let conn = state.db.conn();
    crate::review::state::clear_state(&conn)
        .await
        .map_err(|e: crate::AppError| e.to_string())
}

/// Build the canonical `pr_key` (`{org_url}/{project}/{repo}/{pr_id}`) for the
/// currently-connected org, matching the format used when reviews are saved.
fn pr_key_for(
    state: &AppState,
    project_id: &str,
    repo_id: &str,
    pr_id: i64,
) -> Result<String, String> {
    let ado = state.client.lock().map_err(|e| e.to_string())?;
    let org_url = ado
        .as_ref()
        .ok_or_else(|| "Not logged in.".to_string())?
        .org_url()
        .to_string();
    Ok(format!("{}/{}/{}/{}", org_url, project_id, repo_id, pr_id))
}

/// Load the persisted completed review for a PR, if any.
#[tauri::command]
pub async fn get_completed_review(
    state: State<'_, AppState>,
    project_id: String,
    repo_id: String,
    pr_id: i64,
) -> Result<Option<crate::review::persist::StoredReview>, String> {
    let pr_key = pr_key_for(&state, &project_id, &repo_id, pr_id)?;
    let conn = state.db.conn();
    crate::review::persist::get_review(&conn, &pr_key)
        .await
        .map_err(|e: crate::AppError| e.to_string())
}

/// List every persisted review (outstanding + completed), newest first.
#[tauri::command]
pub async fn list_completed_reviews(
    state: State<'_, AppState>,
) -> Result<Vec<crate::review::persist::StoredReview>, String> {
    let conn = state.db.conn();
    crate::review::persist::list_reviews(&conn)
        .await
        .map_err(|e: crate::AppError| e.to_string())
}

/// Mark a persisted review as completed (manual "Mark completed" action).
#[tauri::command]
pub async fn complete_review(
    state: State<'_, AppState>,
    project_id: String,
    repo_id: String,
    pr_id: i64,
) -> Result<(), String> {
    let pr_key = pr_key_for(&state, &project_id, &repo_id, pr_id)?;
    let conn = state.db.conn();
    crate::review::persist::mark_completed(&conn, &pr_key)
        .await
        .map_err(|e: crate::AppError| e.to_string())
}

/// Delete a persisted review outright (PR closed/merged/abandoned).
#[tauri::command]
pub async fn delete_completed_review(
    state: State<'_, AppState>,
    project_id: String,
    repo_id: String,
    pr_id: i64,
) -> Result<(), String> {
    let pr_key = pr_key_for(&state, &project_id, &repo_id, pr_id)?;
    let conn = state.db.conn();
    crate::review::persist::delete_review(&conn, &pr_key)
        .await
        .map_err(|e: crate::AppError| e.to_string())
}
