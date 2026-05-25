use tauri::State;
use crate::AppState;

#[tauri::command]
pub async fn get_threads(
    state: State<'_, AppState>,
    project_id: String,
    repo_id: String,
    pr_id: i64,
) -> Result<Vec<serde_json::Value>, String> {
    let client = get_client(&state)?;
    let threads = client
        .get_threads(&project_id, &repo_id, pr_id)
        .await
        .map_err(|e| e.to_string())?;
    Ok(threads
        .into_iter()
        .map(|t| {
            serde_json::json!({
                "id": t.id,
                "filePath": t.thread_context.as_ref()
                    .and_then(|ctx| ctx.file_path.clone())
                    .unwrap_or_default(),
                "lineStart": t.thread_context.as_ref()
                    .and_then(|ctx| ctx.right_file_start.as_ref().map(|p| p.line))
                    .unwrap_or(0),
                "lineEnd": t.thread_context.as_ref()
                    .and_then(|ctx| ctx.right_file_end.as_ref().map(|p| p.line))
                    .unwrap_or(0),
                "status": t.status,
                "comments": t.comments.iter().map(|c| serde_json::json!({
                    "id": c.id,
                    "author": c.author.as_ref().map(|a| a.display_name.clone()).unwrap_or_default(),
                    "content": c.content,
                    "publishedDate": c.published_date
                })).collect::<Vec<_>>()
            })
        })
        .collect())
}

#[tauri::command]
pub async fn post_comment(
    state: State<'_, AppState>,
    project_id: String,
    repo_id: String,
    pr_id: i64,
    file_path: String,
    line: i64,
    content: String,
) -> Result<serde_json::Value, String> {
    let client = get_client(&state)?;

    let body = serde_json::json!({
        "comments": [{
            "parentCommentId": 0,
            "content": content,
            "commentType": 1
        }],
        "status": 1,
        "threadContext": {
            "filePath": file_path,
            "rightFileStart": { "line": line, "offset": 1 },
            "rightFileEnd": { "line": line, "offset": 1 }
        }
    });

    let thread = client
        .post_thread(&project_id, &repo_id, pr_id, &body)
        .await
        .map_err(|e| e.to_string())?;

    Ok(serde_json::json!({
        "id": thread.id,
        "status": thread.status,
        "comments": thread.comments.iter().map(|c| serde_json::json!({
            "id": c.id,
            "author": c.author.as_ref().map(|a| a.display_name.clone()).unwrap_or_default(),
            "content": c.content
        })).collect::<Vec<_>>()
    }))
}

#[tauri::command]
pub async fn post_reply(
    state: State<'_, AppState>,
    project_id: String,
    repo_id: String,
    pr_id: i64,
    thread_id: i64,
    content: String,
) -> Result<serde_json::Value, String> {
    let client = get_client(&state)?;

    // ADO doesn't have a dedicated reply endpoint — add a comment to the thread
    let body = serde_json::json!({
        "comments": [{
            "parentCommentId": thread_id,
            "content": content,
            "commentType": 1
        }],
        "status": 1
    });

    let thread = client
        .post_thread(&project_id, &repo_id, pr_id, &body)
        .await
        .map_err(|e| e.to_string())?;

    let last_comment = thread.comments.last();
    Ok(serde_json::json!({
        "id": last_comment.map(|c| c.id).unwrap_or(0),
        "author": last_comment.and_then(|c| c.author.as_ref().map(|a| a.display_name.clone())).unwrap_or_default(),
        "content": last_comment.and_then(|c| c.content.clone()).unwrap_or_default(),
        "publishedDate": last_comment.and_then(|c| c.published_date.clone()).unwrap_or_default()
    }))
}

#[tauri::command]
pub async fn update_reviewer_status(
    state: State<'_, AppState>,
    project_id: String,
    repo_id: String,
    pr_id: i64,
    vote: i32,
) -> Result<(), String> {
    let client = get_client(&state)?;

    // Get the PR to find our reviewer ID
    let pr = client
        .get_pull_request(&project_id, &repo_id, pr_id)
        .await
        .map_err(|e| e.to_string())?;

    // Find the current user among reviewers
    let reviewer = pr
        .reviewers
        .iter()
        .find(|r| r.vote == 0 || r.is_required)
        .or_else(|| pr.reviewers.first());

    match reviewer {
        Some(r) => {
            client
                .update_reviewer_status(&project_id, &repo_id, pr_id, &r.id, vote)
                .await
                .map_err(|e| e.to_string())
        }
        None => Err("Could not find your reviewer entry on this PR".to_string()),
    }
}

fn get_client(state: &AppState) -> Result<crate::ado::AdoClient, String> {
    let guard = state.ado_client.lock().unwrap();
    guard
        .as_ref()
        .cloned()
        .ok_or_else(|| "Not authenticated".to_string())
}
