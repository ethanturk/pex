use crate::provider::{CommentThread, Reviewer};
use crate::AppState;
use tauri::State;

/// ADO PR comment threads require `threadContext.filePath` to be repo-root
/// relative with a leading slash. Posting a slashless path is accepted by the
/// REST API but the web UI then can't match the thread to a file and shows
/// "This file no longer exists in the latest pull request changes."
fn normalize_ado_file_path(path: &str) -> String {
    if path.starts_with('/') {
        path.to_string()
    } else {
        format!("/{path}")
    }
}

/// Threads/comments soft-deleted in ADO (the web UI hides them) are still
/// returned by the REST endpoint. Without this filter the user sees ghost
/// threads — e.g. three copies of a comment they deleted twice.
fn visible_comments(t: &CommentThread) -> Vec<&crate::provider::Comment> {
    t.comments.iter().filter(|c| !c.is_deleted).collect()
}

fn comment_author_id(c: &crate::provider::Comment) -> String {
    c.author.as_ref().map(|a| a.id.clone()).unwrap_or_default()
}

fn comment_to_json(
    c: &crate::provider::Comment,
    current_user_id: Option<&str>,
) -> serde_json::Value {
    let author_id = comment_author_id(c);
    serde_json::json!({
        "id": c.id,
        "author": c.author.as_ref().map(|a| a.display_name.clone()).unwrap_or_default(),
        "authorId": author_id,
        "content": c.content,
        "publishedDate": c.published_date,
        "canEdit": current_user_id
            .map(|me| !author_id.is_empty() && author_id.eq_ignore_ascii_case(me))
            .unwrap_or(false)
    })
}

fn property_value<'a>(
    properties: &'a Option<serde_json::Value>,
    key: &str,
) -> Option<&'a serde_json::Value> {
    properties.as_ref()?.get(key)?.get("$value")
}

fn property_string(properties: &Option<serde_json::Value>, key: &str) -> Option<String> {
    property_value(properties, key).and_then(|value| {
        value
            .as_str()
            .map(str::to_string)
            .or_else(|| value.as_i64().map(|n| n.to_string()))
    })
}

fn property_i32(properties: &Option<serde_json::Value>, key: &str) -> Option<i32> {
    property_value(properties, key).and_then(|value| {
        value
            .as_i64()
            .and_then(|n| i32::try_from(n).ok())
            .or_else(|| value.as_str()?.parse::<i32>().ok())
    })
}

fn reviewer_display_name<'a>(reviewers: &'a [Reviewer], reviewer_id: &str) -> Option<&'a str> {
    reviewers
        .iter()
        .find(|reviewer| !reviewer_id.is_empty() && reviewer.id.eq_ignore_ascii_case(reviewer_id))
        .map(|reviewer| reviewer.display_name.as_str())
}

fn vote_comment_content(t: &CommentThread) -> Option<String> {
    visible_comments(t)
        .into_iter()
        .find_map(|c| c.content.clone().filter(|content| !content.is_empty()))
}

fn vote_name_from_comment(content: &str) -> Option<String> {
    let (name, _) = content.split_once(" voted ")?;
    let name = name.trim();
    if name.is_empty() {
        None
    } else {
        Some(name.to_string())
    }
}

fn useful_reviewer_name(name: &str, reviewer_id: &str) -> Option<String> {
    let name = name.trim();
    if name.is_empty()
        || (!reviewer_id.is_empty() && name.eq_ignore_ascii_case(reviewer_id))
        || name.chars().all(|c| c.is_ascii_digit())
    {
        None
    } else {
        Some(name.to_string())
    }
}

fn vote_history_event(t: &CommentThread, reviewers: &[Reviewer]) -> Option<serde_json::Value> {
    let thread_type = property_string(&t.properties, "CodeReviewThreadType")?;
    if !thread_type.eq_ignore_ascii_case("VoteUpdate") {
        return None;
    }

    let reviewer_id = property_string(&t.properties, "CodeReviewVotedByTfId")
        .or_else(|| property_string(&t.properties, "CodeReviewVotedByIdentity"))
        .unwrap_or_default();
    let property_reviewer_name = property_string(&t.properties, "CodeReviewVotedByDisplayName");
    let reviewer_name = property_reviewer_name
        .as_deref()
        .and_then(|name| useful_reviewer_name(name, &reviewer_id))
        .or_else(|| reviewer_display_name(reviewers, &reviewer_id).map(str::to_string))
        .or_else(|| vote_comment_content(t).and_then(|content| vote_name_from_comment(&content)))
        .or_else(|| property_reviewer_name.filter(|name| !name.trim().is_empty()))
        .unwrap_or_default();
    let vote = property_i32(&t.properties, "CodeReviewVoteResult")?;
    let content = vote_comment_content(t).unwrap_or_default();
    let published_date = visible_comments(t)
        .into_iter()
        .filter_map(|c| c.published_date.clone())
        .next()
        .unwrap_or_default();

    Some(serde_json::json!({
        "threadId": t.id,
        "reviewerId": reviewer_id,
        "reviewerName": reviewer_name,
        "vote": vote,
        "publishedDate": published_date,
        "content": content,
    }))
}

fn thread_to_json(t: &CommentThread, current_user_id: Option<&str>) -> serde_json::Value {
    // ADO returns thread filePath with a leading "/" (and post_comment normalizes
    // to that shape), but the frontend works in slashless paths because
    // get_pr_files strips the prefix. Strip here so `t.filePath === d.path`
    // matches in PRDetail's thread filter.
    let file_path = t
        .thread_context
        .as_ref()
        .and_then(|ctx| ctx.file_path.as_deref())
        .map(|p| p.strip_prefix('/').unwrap_or(p).to_string())
        .unwrap_or_default();
    serde_json::json!({
        "id": t.id,
        "filePath": file_path,
        "lineStart": t.thread_context.as_ref()
            .and_then(|ctx| ctx.right_file_start.as_ref().map(|p| p.line))
            .unwrap_or(0),
        "lineEnd": t.thread_context.as_ref()
            .and_then(|ctx| ctx.right_file_end.as_ref().map(|p| p.line))
            .unwrap_or(0),
        "status": t.status,
        "comments": visible_comments(t)
            .iter()
            .map(|c| comment_to_json(c, current_user_id))
            .collect::<Vec<_>>()
    })
}

#[tauri::command]
pub async fn get_vote_history(
    state: State<'_, AppState>,
    project_id: String,
    repo_id: String,
    pr_id: i64,
) -> Result<Vec<serde_json::Value>, String> {
    let client = get_client(&state)?;
    let (threads, reviewers) = tokio::try_join!(
        client.get_threads(&project_id, &repo_id, pr_id),
        client.get_pull_request(&project_id, &repo_id, pr_id),
    )
    .map_err(|e| e.to_string())
    .map(|(threads, pr)| (threads, pr.reviewers))?;
    let mut events = threads
        .iter()
        .filter(|t| !t.is_deleted)
        .filter_map(|t| vote_history_event(t, &reviewers))
        .collect::<Vec<_>>();
    events.sort_by(|a, b| {
        a.get("publishedDate")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .cmp(
                b.get("publishedDate")
                    .and_then(|v| v.as_str())
                    .unwrap_or(""),
            )
    });
    Ok(events)
}

#[tauri::command]
pub async fn get_threads(
    state: State<'_, AppState>,
    project_id: String,
    repo_id: String,
    pr_id: i64,
) -> Result<Vec<serde_json::Value>, String> {
    let client = get_client(&state)?;
    let current_user_id = client.get_authenticated_user_id().await.ok();
    let threads = client
        .get_threads(&project_id, &repo_id, pr_id)
        .await
        .map_err(|e| e.to_string())?;
    Ok(threads
        .iter()
        .filter(|t| !t.is_deleted && !visible_comments(t).is_empty())
        .map(|t| thread_to_json(t, current_user_id.as_deref()))
        .collect())
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
    let current_user_id = client.get_authenticated_user_id().await.ok();
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
            "filePath": normalize_ado_file_path(&file_path),
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

    Ok(thread_to_json(&thread, current_user_id.as_deref()))
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
    let current_user_id = client.get_authenticated_user_id().await.ok();

    // Build the anchored payload (if we can).
    let anchored = match (&file_path, line_start, line_end) {
        (Some(path), Some(a), Some(b)) => {
            let lo = a.min(b);
            let hi = a.max(b);
            Some(serde_json::json!({
                "comments": [{ "parentCommentId": 0, "content": content, "commentType": 1 }],
                "status": 1,
                "threadContext": {
                    "filePath": normalize_ado_file_path(path),
                    "rightFileStart": { "line": lo, "offset": 1 },
                    "rightFileEnd":   { "line": hi, "offset": 1 },
                },
            }))
        }
        (Some(path), None, None) => Some(serde_json::json!({
            "comments": [{ "parentCommentId": 0, "content": content, "commentType": 1 }],
            "status": 1,
            "threadContext": { "filePath": normalize_ado_file_path(path) },
        })),
        _ => None,
    };

    // Try the anchored payload first; on failure, fall back to PR-level with
    // the file path prefixed into the body.
    let thread = match anchored {
        Some(body) => match client
            .post_thread(&project_id, &repo_id, pr_id, &body)
            .await
        {
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

    Ok(thread_to_json(&thread, current_user_id.as_deref()))
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
    let current_user_id = client.get_authenticated_user_id().await.ok();

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
        "authorId": comment.get("authorId")
            .and_then(|v| v.as_str())
            .or_else(|| comment.get("author").and_then(|a| a.get("id")).and_then(|v| v.as_str()))
            .unwrap_or(""),
        "content": comment.get("content").and_then(|v| v.as_str()).unwrap_or(""),
        "publishedDate": comment.get("publishedDate").and_then(|v| v.as_str()).unwrap_or(""),
        "canEdit": current_user_id
            .as_deref()
            .map(|me| {
                comment.get("authorId")
                    .and_then(|v| v.as_str())
                    .or_else(|| comment.get("author").and_then(|a| a.get("id")).and_then(|v| v.as_str()))
                    .map(|id| id.eq_ignore_ascii_case(me))
                    .unwrap_or(false)
            })
            .unwrap_or(false)
    }))
}

#[tauri::command]
pub async fn update_comment(
    state: State<'_, AppState>,
    project_id: String,
    repo_id: String,
    pr_id: i64,
    thread_id: i64,
    comment_id: i64,
    content: String,
    is_pr_level: bool,
) -> Result<serde_json::Value, String> {
    let client = get_client(&state)?;
    let current_user_id = client
        .get_authenticated_user_id()
        .await
        .map_err(|e| e.to_string())?;
    let threads = client
        .get_threads(&project_id, &repo_id, pr_id)
        .await
        .map_err(|e| e.to_string())?;
    let comment = threads
        .iter()
        .flat_map(|t| visible_comments(t))
        .find(|c| c.id == comment_id)
        .ok_or_else(|| "Comment not found.".to_string())?;
    let author_id = comment_author_id(comment);
    if author_id.is_empty() || !author_id.eq_ignore_ascii_case(&current_user_id) {
        return Err("You can only edit your own comments.".to_string());
    }

    let updated = client
        .update_comment(
            &project_id,
            &repo_id,
            pr_id,
            thread_id,
            comment_id,
            &content,
            is_pr_level,
        )
        .await
        .map_err(|e| e.to_string())?;

    Ok(serde_json::json!({
        "id": updated["id"],
        "author": updated.get("author").and_then(|a| a.get("displayName")).and_then(|v| v.as_str()).unwrap_or(""),
        "authorId": updated.get("authorId")
            .and_then(|v| v.as_str())
            .or_else(|| updated.get("author").and_then(|a| a.get("id")).and_then(|v| v.as_str()))
            .unwrap_or(author_id.as_str()),
        "content": updated.get("content").and_then(|v| v.as_str()).unwrap_or(""),
        "publishedDate": updated.get("publishedDate").and_then(|v| v.as_str()).unwrap_or(""),
        "canEdit": true
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

fn get_client(state: &AppState) -> Result<crate::provider::GitClient, String> {
    let guard = state.client.lock().unwrap();
    guard
        .as_ref()
        .cloned()
        .ok_or_else(|| "Not authenticated".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn prop(value: impl serde::Serialize) -> serde_json::Value {
        serde_json::json!({ "$value": value })
    }

    #[test]
    fn vote_history_event_extracts_ado_vote_update() {
        let thread = CommentThread {
            id: 42,
            thread_context: None,
            status: None,
            is_deleted: false,
            properties: Some(serde_json::json!({
                "CodeReviewThreadType": prop("VoteUpdate"),
                "CodeReviewVotedByTfId": prop("user-1"),
                "CodeReviewVotedByDisplayName": prop("Ethan Turk"),
                "CodeReviewVoteResult": prop(10),
            })),
            comments: vec![crate::provider::Comment {
                id: 1,
                author: None,
                content: None,
                published_date: Some("2026-06-02T18:00:00Z".to_string()),
                is_deleted: false,
            }],
        };

        let event = vote_history_event(&thread, &[]).expect("vote event");
        assert_eq!(event["threadId"], 42);
        assert_eq!(event["reviewerId"], "user-1");
        assert_eq!(event["reviewerName"], "Ethan Turk");
        assert_eq!(event["vote"], 10);
        assert_eq!(event["publishedDate"], "2026-06-02T18:00:00Z");
    }

    #[test]
    fn vote_history_event_extracts_reset_vote() {
        let thread = CommentThread {
            id: 43,
            thread_context: None,
            status: None,
            is_deleted: false,
            properties: Some(serde_json::json!({
                "CodeReviewThreadType": prop("VoteUpdate"),
                "CodeReviewVotedByTfId": prop("user-1"),
                "CodeReviewVotedByDisplayName": prop("Ethan Turk"),
                "CodeReviewVoteResult": prop(0),
            })),
            comments: vec![],
        };

        let event = vote_history_event(&thread, &[]).expect("vote event");
        assert_eq!(event["vote"], 0);
        assert_eq!(event["publishedDate"], "");
    }

    #[test]
    fn vote_history_event_ignores_non_vote_threads() {
        let thread = CommentThread {
            id: 44,
            thread_context: None,
            status: None,
            comments: vec![],
            is_deleted: false,
            properties: Some(serde_json::json!({
                "CodeReviewThreadType": prop("General"),
            })),
        };

        assert!(vote_history_event(&thread, &[]).is_none());
    }

    #[test]
    fn vote_history_event_uses_reviewer_name_when_thread_has_only_id() {
        let thread = CommentThread {
            id: 45,
            thread_context: None,
            status: None,
            is_deleted: false,
            properties: Some(serde_json::json!({
                "CodeReviewThreadType": prop("VoteUpdate"),
                "CodeReviewVotedByTfId": prop("user-1"),
                "CodeReviewVoteResult": prop(10),
            })),
            comments: vec![],
        };
        let reviewers = vec![Reviewer {
            id: "user-1".to_string(),
            display_name: "Ethan Turk".to_string(),
            vote: 10,
            is_required: false,
        }];

        let event = vote_history_event(&thread, &reviewers).expect("vote event");
        assert_eq!(event["reviewerName"], "Ethan Turk");
    }

    #[test]
    fn vote_history_event_uses_system_comment_when_properties_lack_name() {
        let thread = CommentThread {
            id: 46,
            thread_context: None,
            status: None,
            is_deleted: false,
            properties: Some(serde_json::json!({
                "CodeReviewThreadType": prop("VoteUpdate"),
                "CodeReviewVotedByTfId": prop("1"),
                "CodeReviewVoteResult": prop(-5),
            })),
            comments: vec![crate::provider::Comment {
                id: 1,
                author: None,
                content: Some("Ethan Turk voted -5".to_string()),
                published_date: Some("2026-06-02T18:00:00Z".to_string()),
                is_deleted: false,
            }],
        };

        let event = vote_history_event(&thread, &[]).expect("vote event");
        assert_eq!(event["reviewerName"], "Ethan Turk");
        assert_eq!(event["content"], "Ethan Turk voted -5");
    }

    #[test]
    fn vote_history_event_prefers_system_comment_over_numeric_display_name() {
        let thread = CommentThread {
            id: 47,
            thread_context: None,
            status: None,
            is_deleted: false,
            properties: Some(serde_json::json!({
                "CodeReviewThreadType": prop("VoteUpdate"),
                "CodeReviewVotedByTfId": prop("1"),
                "CodeReviewVotedByDisplayName": prop("1"),
                "CodeReviewVoteResult": prop(10),
            })),
            comments: vec![crate::provider::Comment {
                id: 1,
                author: None,
                content: Some("Ethan Turk voted 10".to_string()),
                published_date: Some("2026-06-02T18:00:00Z".to_string()),
                is_deleted: false,
            }],
        };

        let event = vote_history_event(&thread, &[]).expect("vote event");
        assert_eq!(event["reviewerName"], "Ethan Turk");
    }
}
