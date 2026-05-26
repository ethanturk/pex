use crate::AppState;
use crate::ado::CommentThread;
use tauri::State;

fn thread_to_json(t: &CommentThread) -> serde_json::Value {
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
}

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
    Ok(threads.iter().map(thread_to_json).collect())
}

#[tauri::command]
pub async fn post_comment(
    state: State<'_, AppState>,
    project_id: String,
    repo_id: String,
    pr_id: i64,
    file_path: String,
    line_start: i64,
    line_end: i64,
    content: String,
) -> Result<serde_json::Value, String> {
    let client = get_client(&state)?;
    let lo = line_start.min(line_end);
    let hi = line_start.max(line_end);

    let body = serde_json::json!({
        "comments": [{
            "parentCommentId": 0,
            "content": content,
            "commentType": 1
        }],
        "status": 1,
        "threadContext": {
            "filePath": file_path,
            "rightFileStart": { "line": lo, "offset": 1 },
            // ADO PR comment ranges are inclusive on both ends; offsets are 1-based
            // column positions within the line.
            "rightFileEnd": { "line": hi, "offset": 1 }
        }
    });

    let thread = client
        .post_thread(&project_id, &repo_id, pr_id, &body)
        .await
        .map_err(|e| e.to_string())?;

    Ok(thread_to_json(&thread))
}

/// Post a review finding to ADO. Supports three anchoring modes:
/// - file + line range  → thread anchored to those lines
/// - file only          → file-level thread (no line); on ADO 400, falls back
///                        to a PR-level comment with the file path bolded in
/// - neither            → plain PR-level comment
///
/// Returns the same serde_json shape as `post_comment` so the frontend can
/// reuse the existing thread-display path.
#[tauri::command]
pub async fn post_review_finding(
    state: State<'_, AppState>,
    project_id: String,
    repo_id: String,
    pr_id: i64,
    file_path: Option<String>,
    line_start: Option<i64>,
    line_end: Option<i64>,
    content: String,
) -> Result<serde_json::Value, String> {
    let client = get_client(&state)?;

    // Build the anchored payload (if we can).
    let anchored = match (&file_path, line_start, line_end) {
        (Some(path), Some(a), Some(b)) => {
            let lo = a.min(b);
            let hi = a.max(b);
            Some(serde_json::json!({
                "comments": [{ "parentCommentId": 0, "content": content, "commentType": 1 }],
                "status": 1,
                "threadContext": {
                    "filePath": path,
                    "rightFileStart": { "line": lo, "offset": 1 },
                    "rightFileEnd":   { "line": hi, "offset": 1 },
                },
            }))
        }
        (Some(path), None, None) => Some(serde_json::json!({
            "comments": [{ "parentCommentId": 0, "content": content, "commentType": 1 }],
            "status": 1,
            "threadContext": { "filePath": path },
        })),
        _ => None,
    };

    // Try the anchored payload first; on failure, fall back to PR-level with
    // the file path prefixed into the body.
    let thread = match anchored {
        Some(body) => match client.post_thread(&project_id, &repo_id, pr_id, &body).await {
            Ok(t) => t,
            Err(err) => {
                let fallback_body = match &file_path {
                    Some(p) => format!("**{}**\n\n{}", p, content),
                    None => content.clone(),
                };
                let pr_level = serde_json::json!({
                    "comments": [{ "parentCommentId": 0, "content": fallback_body, "commentType": 1 }],
                    "status": 1,
                });
                eprintln!("[review] anchored thread post failed ({err}); falling back to PR-level");
                client
                    .post_thread(&project_id, &repo_id, pr_id, &pr_level)
                    .await
                    .map_err(|e| e.to_string())?
            }
        },
        None => {
            let body = serde_json::json!({
                "comments": [{ "parentCommentId": 0, "content": content, "commentType": 1 }],
                "status": 1,
            });
            client
                .post_thread(&project_id, &repo_id, pr_id, &body)
                .await
                .map_err(|e| e.to_string())?
        }
    };

    Ok(thread_to_json(&thread))
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

    let body = serde_json::json!({
        "content": content,
        "parentCommentId": 0,
        "commentType": 1
    });

    let comment = client
        .add_comment_to_thread(&project_id, &repo_id, pr_id, thread_id, &body)
        .await
        .map_err(|e| e.to_string())?;

    Ok(serde_json::json!({
        "id": comment["id"],
        "author": comment.get("author").and_then(|a| a.get("displayName")).and_then(|v| v.as_str()).unwrap_or(""),
        "content": comment.get("content").and_then(|v| v.as_str()).unwrap_or(""),
        "publishedDate": comment.get("publishedDate").and_then(|v| v.as_str()).unwrap_or("")
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

    // PUT on the reviewer endpoint adds the user as a reviewer if missing, so
    // we no longer need to pre-check whether the user is on the PR.
    let me = client
        .get_authenticated_user_id()
        .await
        .map_err(|e| e.to_string())?;

    client
        .update_reviewer_status(&project_id, &repo_id, pr_id, &me, vote)
        .await
        .map_err(|e| e.to_string())
}

fn get_client(state: &AppState) -> Result<crate::ado::AdoClient, String> {
    let guard = state.ado_client.lock().unwrap();
    guard
        .as_ref()
        .cloned()
        .ok_or_else(|| "Not authenticated".to_string())
}
