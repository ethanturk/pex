use tauri::State;
use crate::AppState;

#[tauri::command]
pub async fn list_projects(state: State<'_, AppState>) -> Result<Vec<serde_json::Value>, String> {
    let client = get_client(&state)?;
    let projects = client.list_projects().await.map_err(|e| e.to_string())?;
    Ok(projects
        .into_iter()
        .map(|p| serde_json::json!({ "id": p.id, "name": p.name }))
        .collect())
}

#[tauri::command]
pub async fn list_repositories(
    state: State<'_, AppState>,
    project_id: String,
) -> Result<Vec<serde_json::Value>, String> {
    let client = get_client(&state)?;
    let repos = client
        .list_repositories(&project_id)
        .await
        .map_err(|e| e.to_string())?;
    Ok(repos
        .into_iter()
        .map(|r| {
            serde_json::json!({
                "id": r.id,
                "name": r.name,
                "defaultBranch": r.default_branch
            })
        })
        .collect())
}

#[tauri::command]
pub async fn list_pull_requests(
    state: State<'_, AppState>,
    project_id: String,
    repo_id: String,
) -> Result<Vec<serde_json::Value>, String> {
    let client = get_client(&state)?;
    let prs = client
        .list_pull_requests(&project_id, &repo_id)
        .await
        .map_err(|e| e.to_string())?;
    Ok(prs
        .into_iter()
        .map(|pr| {
            serde_json::json!({
                "pullRequestId": pr.pull_request_id,
                "title": pr.title,
                "description": pr.description,
                "status": pr.status,
                "createdBy": {
                    "displayName": pr.created_by.display_name,
                    "id": pr.created_by.id
                },
                "sourceRefName": pr.source_ref_name,
                "targetRefName": pr.target_ref_name,
                "creationDate": pr.creation_date,
                "mergeStatus": pr.merge_status,
                "reviewers": pr.reviewers.iter().map(|r| serde_json::json!({
                    "id": r.id,
                    "displayName": r.display_name,
                    "vote": r.vote,
                    "isRequired": r.is_required
                })).collect::<Vec<_>>(),
                "iterationCount": 1
            })
        })
        .collect())
}

#[tauri::command]
pub async fn get_iterations(
    state: State<'_, AppState>,
    project_id: String,
    repo_id: String,
    pr_id: i64,
) -> Result<Vec<serde_json::Value>, String> {
    let client = get_client(&state)?;
    let iterations = client
        .get_iterations(&project_id, &repo_id, pr_id)
        .await
        .map_err(|e| e.to_string())?;
    Ok(iterations
        .into_iter()
        .map(|i| serde_json::json!({ "id": i.id, "name": i.name }))
        .collect())
}

fn get_client(state: &AppState) -> Result<crate::ado::AdoClient, String> {
    let guard = state.ado_client.lock().unwrap();
    guard
        .as_ref()
        .cloned()
        .ok_or_else(|| "Not authenticated".to_string())
}
