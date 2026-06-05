use crate::AppState;
use tauri::State;

#[tauri::command]
pub async fn get_pr_files(
    state: State<'_, AppState>,
    project_id: String,
    repo_id: String,
    pr_id: i64,
    iteration: i32,
) -> Result<Vec<serde_json::Value>, String> {
    let client = get_client(&state)?;
    let result = client
        .get_pr_files(&project_id, &repo_id, pr_id, iteration)
        .await
        .map_err(|e| e.to_string())?;

    let change_type_map = |ct: &str| match ct {
        "add" => "add",
        "edit" => "edit",
        "delete" => "delete",
        "rename" => "rename",
        _ => "edit",
    };

    Ok(result
        .files
        .into_iter()
        .map(|f| {
            serde_json::json!({
                "path": f.item.path.strip_prefix('/').unwrap_or(&f.item.path),
                "status": change_type_map(&f.change_type),
                "viewed": false  // client fills in from cache
            })
        })
        .collect())
}

#[tauri::command]
pub async fn get_file_diff(
    state: State<'_, AppState>,
    project_id: String,
    repo_id: String,
    pr_id: i64,
    file_path: String,
    iteration: i32,
    view: Option<String>,
) -> Result<serde_json::Value, String> {
    let client = get_client(&state)?;
    let view_str = view.as_deref().unwrap_or("inline").to_string();
    let view_enum = crate::diff::engine::DiffView::from_str(&view_str);

    let cache_key = crate::cache::diff_cache::DiffCacheKey {
        org_url: client.org_url().to_string(),
        project_id: project_id.clone(),
        repo_id: repo_id.clone(),
        pr_id,
        file_path: file_path.clone(),
        view: view_str,
        iteration,
    };

    let diff = if let Some(hit) = state.diff_cache.get(&cache_key) {
        hit
    } else {
        let fresh = client
            .get_file_diff(
                &project_id,
                &repo_id,
                pr_id,
                &file_path,
                iteration,
                view_enum,
            )
            .await
            .map_err(|e| e.to_string())?;
        state.diff_cache.put(cache_key, fresh.clone());
        fresh
    };

    Ok(serde_json::json!({
        "html": diff.html,
        "path": diff.path,
        "status": diff.status,
        "sourceCommit": diff.source_commit,
        "baseCommit": diff.base_commit,
        "oldContent": diff.old_content,
        "newContent": diff.new_content,
    }))
}

#[tauri::command]
pub async fn prefetch_pr_diffs(
    state: State<'_, AppState>,
    project_id: String,
    repo_id: String,
    pr_id: i64,
    iteration: i32,
    file_paths: Vec<String>,
) -> Result<serde_json::Value, String> {
    let client = get_client(&state)?;
    let org_url = client.org_url().to_string();
    let concurrency = {
        let conn = state.db.conn();
        crate::ai::read_hunk_concurrency(&conn)
            .await
            .unwrap_or(crate::ai::DEFAULT_HUNK_CONCURRENCY)
            .max(1) as usize
    };

    let mut cached = 0usize;
    let mut fetched = 0usize;
    let mut failed = 0usize;
    let mut misses = Vec::new();

    for file_path in file_paths {
        let file_path = file_path
            .strip_prefix('/')
            .unwrap_or(&file_path)
            .to_string();
        let cache_key = crate::cache::diff_cache::DiffCacheKey {
            org_url: org_url.clone(),
            project_id: project_id.clone(),
            repo_id: repo_id.clone(),
            pr_id,
            file_path: file_path.clone(),
            view: "inline".to_string(),
            iteration,
        };

        if state.diff_cache.get(&cache_key).is_some() {
            cached += 1;
        } else {
            misses.push(file_path);
        }
    }

    for chunk in misses.chunks(concurrency) {
        let mut handles = Vec::new();
        for file_path in chunk {
            let client = client.clone();
            let project_id = project_id.clone();
            let repo_id = repo_id.clone();
            let file_path = file_path.clone();
            handles.push((
                file_path.clone(),
                tokio::spawn(async move {
                    client
                        .get_file_diff(
                            &project_id,
                            &repo_id,
                            pr_id,
                            &file_path,
                            iteration,
                            crate::diff::engine::DiffView::Inline,
                        )
                        .await
                }),
            ));
        }

        for (file_path, handle) in handles {
            match handle.await {
                Ok(Ok(diff)) => {
                    let cache_key = crate::cache::diff_cache::DiffCacheKey {
                        org_url: org_url.clone(),
                        project_id: project_id.clone(),
                        repo_id: repo_id.clone(),
                        pr_id,
                        file_path,
                        view: "inline".to_string(),
                        iteration,
                    };
                    state.diff_cache.put(cache_key, diff);
                    fetched += 1;
                }
                Ok(Err(_)) | Err(_) => {
                    failed += 1;
                }
            }
        }
    }

    Ok(serde_json::json!({
        "cached": cached,
        "fetched": fetched,
        "failed": failed,
    }))
}

#[tauri::command]
pub async fn get_file_lines(
    state: State<'_, AppState>,
    project_id: String,
    repo_id: String,
    commit_id: String,
    file_path: String,
    start_line: usize,
    end_line: usize,
) -> Result<Vec<String>, String> {
    let client = get_client(&state)?;
    client
        .get_file_lines(
            &project_id,
            &repo_id,
            &commit_id,
            &file_path,
            start_line,
            end_line,
        )
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn mark_file_viewed(
    state: State<'_, AppState>,
    project_id: String,
    repo_id: String,
    pr_id: i64,
    file_path: String,
    viewed: bool,
) -> Result<(), String> {
    // Resolve the org URL under the client lock, then drop it before awaiting
    // the DB write (the std Mutex guard isn't Send across an await point).
    let org_url = {
        let client_lock = state.client.lock().unwrap();
        let client = client_lock.as_ref().ok_or("Not authenticated")?;
        client.org_url().to_string()
    };

    let conn = state.db.conn();
    crate::cache::set_viewed(
        &conn,
        &org_url,
        &project_id,
        &repo_id,
        pr_id,
        &file_path,
        viewed,
    )
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_viewed_files(
    state: State<'_, AppState>,
    project_id: String,
    repo_id: String,
    pr_id: i64,
) -> Result<Vec<String>, String> {
    let org_url = {
        let client_lock = state.client.lock().unwrap();
        let client = client_lock.as_ref().ok_or("Not authenticated")?;
        client.org_url().to_string()
    };

    let conn = state.db.conn();
    crate::cache::get_viewed(&conn, &org_url, &project_id, &repo_id, pr_id)
        .await
        .map_err(|e| e.to_string())
}

fn get_client(state: &AppState) -> Result<crate::provider::GitClient, String> {
    let guard = state.client.lock().unwrap();
    guard
        .as_ref()
        .cloned()
        .ok_or_else(|| "Not authenticated".to_string())
}
