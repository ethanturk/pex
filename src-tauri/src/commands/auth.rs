use crate::AppState;
use tauri::State;

#[tauri::command]
pub async fn login_pat(
    state: State<'_, AppState>,
    org_url: String,
    pat: String,
) -> Result<bool, String> {
    match crate::auth::pat::validate_pat(&org_url, &pat).await {
        Ok(name) => {
            let client = crate::ado::AdoClient::new(org_url.clone(), pat.clone());
            *state.ado_client.lock().unwrap() = Some(client);

            crate::auth::keyring_store::KeyringStore::save_pat(&org_url, &pat)
                .map_err(|e| e.to_string())?;

            let conn = state.db.lock().unwrap();
            crate::cache::save_org(&conn, &org_url, &name, "pat").map_err(|e| e.to_string())?;

            Ok(true)
        }
        Err(e) => Err(e.to_string()),
    }
}

/// Rehydrate the in-memory ADO client for a saved org by reading credentials
/// from the keyring. Called on app startup and when switching orgs.
#[tauri::command]
pub async fn activate_org(
    state: State<'_, AppState>,
    org_url: String,
) -> Result<bool, String> {
    // Look up the saved org to determine token type.
    let token_type = {
        let conn = state.db.lock().unwrap();
        crate::cache::list_orgs(&conn)
            .map_err(|e| e.to_string())?
            .into_iter()
            .find(|(url, _, _)| url == &org_url)
            .map(|(_, _, t)| t)
            .ok_or_else(|| "Org not found in saved list".to_string())?
    };

    let client = match token_type.as_str() {
        "pat" => {
            let pat = crate::auth::keyring_store::KeyringStore::get_pat(&org_url)
                .map_err(|e| e.to_string())?
                .ok_or_else(|| "No saved PAT for this org. Please sign in again.".to_string())?;
            crate::ado::AdoClient::new(org_url.clone(), pat)
        }
        "oauth" => {
            // Refresh the access token using stored refresh token + client secret.
            let (refresh_token, client_secret) =
                crate::auth::keyring_store::KeyringStore::get_oauth(&org_url)
                    .map_err(|e| e.to_string())?
                    .ok_or_else(|| {
                        "No saved OAuth credentials for this org. Please sign in again.".to_string()
                    })?;
            let token = crate::auth::oauth::refresh_oauth_token(
                &client_secret,
                &refresh_token,
                "http://localhost:0/callback",
            )
            .await
            .map_err(|e| e.to_string())?;
            crate::auth::keyring_store::KeyringStore::save_oauth(
                &org_url,
                &token.refresh_token,
                &client_secret,
            )
            .map_err(|e| e.to_string())?;
            crate::ado::AdoClient::with_bearer_token(org_url.clone(), token.access_token)
        }
        other => return Err(format!("Unknown token type: {}", other)),
    };

    *state.ado_client.lock().unwrap() = Some(client);

    // Pre-warm the AI manager so the AI API key's keychain prompt fires here, alongside
    // the ADO PAT prompt above, rather than later when the user first clicks Explain/Review.
    // macOS will still show one OS dialog per keychain item — they just appear back-to-back
    // during the same startup gesture. Errors are non-fatal: AI may simply be unconfigured.
    {
        let db = state.db.lock().unwrap();
        let mut ai_mgr_lock = state.ai_manager.lock().unwrap();
        if ai_mgr_lock.is_none() {
            let mut mgr = crate::ai::AiManager::new();
            if let Ok(true) = mgr.try_configure_from_db(&db) {
                *ai_mgr_lock = Some(mgr);
            }
        }
    }

    Ok(true)
}

#[tauri::command]
pub async fn login_oauth(
    state: State<'_, AppState>,
    _app_handle: tauri::AppHandle,
    org_url: String,
    client_id: String,
    client_secret: String,
) -> Result<serde_json::Value, String> {
    let token = crate::auth::oauth::start_oauth_flow(&org_url, &client_id, &client_secret, |url| {
        let _ = tauri_plugin_opener::open_url(url, None::<&str>);
    })
    .await
    .map_err(|e| e.to_string())?;

    let client =
        crate::ado::AdoClient::with_bearer_token(org_url.clone(), token.access_token.clone());
    *state.ado_client.lock().unwrap() = Some(client);

    // Save org to cache
    let conn = state.db.lock().unwrap();
    crate::cache::save_org(&conn, &org_url, &org_url, "oauth").map_err(|e| e.to_string())?;

    // Store refresh token + client secret for later refresh
    crate::auth::keyring_store::KeyringStore::save_oauth(
        &org_url,
        &token.refresh_token,
        &client_secret,
    )
    .map_err(|e| e.to_string())?;

    Ok(serde_json::json!({
        "access_token": token.access_token,
        "expires_in": token.expires_in
    }))
}

#[tauri::command]
pub async fn refresh_oauth_token(
    state: State<'_, AppState>,
    org_url: String,
) -> Result<serde_json::Value, String> {
    // Retrieve stored refresh token and client secret
    let (refresh_token, client_secret) =
        crate::auth::keyring_store::KeyringStore::get_oauth(&org_url)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "No OAuth credentials stored for this org".to_string())?;

    let token = crate::auth::oauth::refresh_oauth_token(
        &client_secret,
        &refresh_token,
        "http://localhost:0/callback", // dummy redirect for refresh
    )
    .await
    .map_err(|e| e.to_string())?;

    // Update the ADO client with new token
    let client =
        crate::ado::AdoClient::with_bearer_token(org_url.clone(), token.access_token.clone());
    *state.ado_client.lock().unwrap() = Some(client);

    // Store updated refresh token
    crate::auth::keyring_store::KeyringStore::save_oauth(
        &org_url,
        &token.refresh_token,
        &client_secret,
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
