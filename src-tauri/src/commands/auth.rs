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
            let client = crate::ado::AdoClient::new(org_url.clone(), pat);
            *state.ado_client.lock().unwrap() = Some(client);

            let conn = state.db.lock().unwrap();
            crate::cache::save_org(&conn, &org_url, &name, "pat").map_err(|e| e.to_string())?;

            Ok(true)
        }
        Err(e) => Err(e.to_string()),
    }
}

#[tauri::command]
pub async fn login_oauth(
    state: State<'_, AppState>,
    _app_handle: tauri::AppHandle,
    org_url: String,
    client_id: String,
    client_secret: String,
) -> Result<serde_json::Value, String> {
    let token = crate::auth::oauth::start_oauth_flow(
        &org_url,
        &client_id,
        &client_secret,
        |url| {
            // Open the authorize URL in the system browser
            let _ = tauri_plugin_opener::open_url(url, None::<&str>);
        },
    )
    .await
    .map_err(|e| e.to_string())?;

    let client = crate::ado::AdoClient::with_bearer_token(org_url.clone(), token.access_token.clone());
    *state.ado_client.lock().unwrap() = Some(client);

    // Save to cache
    let conn = state.db.lock().unwrap();
    crate::cache::save_org(&conn, &org_url, &org_url, "oauth").map_err(|e| e.to_string())?;

    // Save token to keyring (store the access token)
    crate::auth::keyring_store::KeyringStore::save_pat(
        &format!("oauth:{}", org_url),
        &token.access_token,
    )
    .map_err(|e| e.to_string())?;

    Ok(serde_json::json!({
        "access_token": token.access_token,
        "expires_in": token.expires_in
    }))
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
