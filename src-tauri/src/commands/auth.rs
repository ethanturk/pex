use tauri::State;
use crate::AppState;

#[tauri::command]
pub async fn login_pat(
    state: State<'_, AppState>,
    org_url: String,
    pat: String,
) -> Result<bool, String> {
    match crate::auth::pat::validate_pat(&org_url, &pat).await {
        Ok(name) => {
            // Build ADO client
            let client = crate::ado::AdoClient::new(org_url.clone(), pat);
            *state.ado_client.lock().unwrap() = Some(client);

            // Save to keyring (already done in validate_pat)
            // Save org metadata to cache
            let conn = state.db.lock().unwrap();
            crate::cache::save_org(&conn, &org_url, &name, "pat").map_err(|e| e.to_string())?;

            Ok(true)
        }
        Err(e) => {
            Err(e.to_string())
        }
    }
}

#[tauri::command]
pub async fn login_oauth(_state: State<'_, AppState>, org_url: String) -> Result<String, String> {
    crate::auth::oauth::start_oauth_flow(&org_url).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_saved_orgs(state: State<'_, AppState>) -> Result<Vec<serde_json::Value>, String> {
    let conn = state.db.lock().unwrap();
    let orgs = crate::cache::list_orgs(&conn).map_err(|e| e.to_string())?;
    Ok(orgs
        .into_iter()
        .map(|(url, name, token_type)| {
            serde_json::json!({
                "orgUrl": url,
                "name": name,
                "tokenType": token_type
            })
        })
        .collect())
}

#[tauri::command]
pub fn remove_org(state: State<'_, AppState>, org_url: String) -> Result<(), String> {
    let conn = state.db.lock().unwrap();
    crate::cache::remove_org(&conn, &org_url).map_err(|e| e.to_string())?;
    crate::auth::keyring_store::KeyringStore::delete_pat(&org_url).map_err(|e| e.to_string())?;
    Ok(())
}
