use tauri::State;
use crate::AppState;

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

    Ok(result.files
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
) -> Result<serde_json::Value, String> {
    let client = get_client(&state)?;
    let diff = client
        .get_file_diff(&project_id, &repo_id, pr_id, &file_path, iteration)
        .await
        .map_err(|e| e.to_string())?;

    Ok(serde_json::json!({
        "html": diff.html,
        "path": diff.path,
        "status": diff.status
    }))
}

#[tauri::command]
pub fn mark_file_viewed(
    state: State<'_, AppState>,
    project_id: String,
    repo_id: String,
    pr_id: i64,
    file_path: String,
    viewed: bool,
) -> Result<(), String> {
    let client_lock = state.ado_client.lock().unwrap();
    let client = client_lock.as_ref().ok_or("Not authenticated")?;
    let org_url = &client.org_url;

    let conn = state.db.lock().unwrap();
    crate::cache::set_viewed(&conn, org_url, &project_id, &repo_id, pr_id, &file_path, viewed)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_viewed_files(
    state: State<'_, AppState>,
    project_id: String,
    repo_id: String,
    pr_id: i64,
) -> Result<Vec<String>, String> {
    let client_lock = state.ado_client.lock().unwrap();
    let client = client_lock.as_ref().ok_or("Not authenticated")?;
    let org_url = &client.org_url;

    let conn = state.db.lock().unwrap();
    crate::cache::get_viewed(&conn, org_url, &project_id, &repo_id, pr_id)
        .map_err(|e| e.to_string())
}

fn get_client(state: &AppState) -> Result<crate::ado::AdoClient, String> {
    let guard = state.ado_client.lock().unwrap();
    guard
        .as_ref()
        .cloned()
        .ok_or_else(|| "Not authenticated".to_string())
}
