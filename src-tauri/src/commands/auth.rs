use crate::AppState;
use tauri::State;

#[tauri::command]
pub async fn login_pat(
    state: State<'_, AppState>,
    provider: Option<String>,
    org_url: String,
    pat: String,
) -> Result<bool, String> {
    let provider = provider.unwrap_or_else(|| "ado".to_string());
    match provider.as_str() {
        "github" => {
            let api_base = crate::github::api_base_for(&org_url);
            let canonical = crate::github::canonical_org_url(&org_url);
            let login = crate::auth::github_pat::validate_github_pat(&api_base, &pat)
                .await
                .map_err(|e| e.to_string())?;

            let client = crate::provider::GitClient::Github(crate::github::GithubClient::new(
                org_url.clone(),
                pat.clone(),
            ));
            *state.client.lock().unwrap() = Some(client);

            // Keyring + saved-org rows are keyed by the canonical URL so
            // `activate_org` can rebuild the client identically on restart.
            crate::auth::keyring_store::KeyringStore::save_pat(&canonical, &pat)
                .map_err(|e| e.to_string())?;
            let conn = state.db.lock().unwrap();
            crate::cache::save_org(&conn, &canonical, &login, "pat", "github")
                .map_err(|e| e.to_string())?;
            Ok(true)
        }
        _ => match crate::auth::pat::validate_pat(&org_url, &pat).await {
            Ok(name) => {
                let client = crate::provider::GitClient::Ado(crate::ado::AdoClient::new(
                    org_url.clone(),
                    pat.clone(),
                ));
                *state.client.lock().unwrap() = Some(client);

                crate::auth::keyring_store::KeyringStore::save_pat(&org_url, &pat)
                    .map_err(|e| e.to_string())?;

                let conn = state.db.lock().unwrap();
                crate::cache::save_org(&conn, &org_url, &name, "pat", "ado")
                    .map_err(|e| e.to_string())?;

                Ok(true)
            }
            Err(e) => Err(e.to_string()),
        },
    }
}

#[tauri::command]
pub async fn get_current_user_id(state: State<'_, AppState>) -> Result<String, String> {
    let client = {
        let guard = state.client.lock().unwrap();
        guard
            .as_ref()
            .ok_or_else(|| "Not authenticated".to_string())?
            .clone()
    };
    client
        .get_authenticated_user_id()
        .await
        .map_err(|e| e.to_string())
}

/// Rehydrate the in-memory provider client for a saved org by reading
/// credentials from the keyring. Called on app startup and when switching orgs.
#[tauri::command]
pub async fn activate_org(state: State<'_, AppState>, org_url: String) -> Result<bool, String> {
    // Look up the saved org to determine provider + token type.
    let (token_type, provider) = {
        let conn = state.db.lock().unwrap();
        crate::cache::list_orgs(&conn)
            .map_err(|e| e.to_string())?
            .into_iter()
            .find(|(url, _, _, _)| url == &org_url)
            .map(|(_, _, t, p)| (t, p))
            .ok_or_else(|| "Org not found in saved list".to_string())?
    };

    let client = match (provider.as_str(), token_type.as_str()) {
        ("github", "pat") => {
            let pat = crate::auth::keyring_store::KeyringStore::get_pat(&org_url)
                .map_err(|e| e.to_string())?
                .ok_or_else(|| "No saved PAT for this org. Please sign in again.".to_string())?;
            crate::provider::GitClient::Github(crate::github::GithubClient::new(
                org_url.clone(),
                pat,
            ))
        }
        ("github", other) => {
            return Err(format!("GitHub {} auth is not yet supported.", other));
        }
        (_, "pat") => {
            let pat = crate::auth::keyring_store::KeyringStore::get_pat(&org_url)
                .map_err(|e| e.to_string())?
                .ok_or_else(|| "No saved PAT for this org. Please sign in again.".to_string())?;
            crate::provider::GitClient::Ado(crate::ado::AdoClient::new(org_url.clone(), pat))
        }
        (_, "oauth") => {
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
            crate::provider::GitClient::Ado(crate::ado::AdoClient::with_bearer_token(
                org_url.clone(),
                token.access_token,
            ))
        }
        (_, other) => return Err(format!("Unknown token type: {}", other)),
    };

    *state.client.lock().unwrap() = Some(client);

    // Pre-warm the AI manager so the AI API key's keychain prompt fires here, alongside
    // the provider PAT prompt above, rather than later when the user first clicks Explain/Review.
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
    // OAuth is currently Azure DevOps only.
    let token = crate::auth::oauth::start_oauth_flow(&org_url, &client_id, &client_secret, |url| {
        let _ = tauri_plugin_opener::open_url(url, None::<&str>);
    })
    .await
    .map_err(|e| e.to_string())?;

    let client = crate::provider::GitClient::Ado(crate::ado::AdoClient::with_bearer_token(
        org_url.clone(),
        token.access_token.clone(),
    ));
    *state.client.lock().unwrap() = Some(client);

    // Save org to cache
    let conn = state.db.lock().unwrap();
    crate::cache::save_org(&conn, &org_url, &org_url, "oauth", "ado").map_err(|e| e.to_string())?;

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

    // Update the client with new token
    let client = crate::provider::GitClient::Ado(crate::ado::AdoClient::with_bearer_token(
        org_url.clone(),
        token.access_token.clone(),
    ));
    *state.client.lock().unwrap() = Some(client);

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
        .map(|(url, name, token_type, provider)| {
            serde_json::json!({
                "orgUrl": url,
                "name": name,
                "tokenType": token_type,
                "provider": provider
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
