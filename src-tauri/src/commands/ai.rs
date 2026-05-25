use crate::ai::{AiProviderKind, AiSettingsNoKey, ChatMessage, ChatRole};
use crate::AppState;
use tauri::Emitter;
use tauri::State;

// ---- Settings commands ----

#[tauri::command]
pub async fn get_ai_settings(state: State<'_, AppState>) -> Result<AiSettingsNoKey, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;

    let provider = crate::cache::get_setting(&db, "ai_provider")
        .map_err(|e: crate::AppError| e.to_string())?
        .unwrap_or_else(|| "openai".to_string());

    let endpoint = crate::cache::get_setting(&db, "ai_endpoint")
        .map_err(|e: crate::AppError| e.to_string())?
        .unwrap_or_else(|| "https://api.openai.com".to_string());

    let model = crate::cache::get_setting(&db, "ai_model")
        .map_err(|e: crate::AppError| e.to_string())?
        .unwrap_or_else(|| "gpt-4.1".to_string());

    Ok(AiSettingsNoKey {
        provider,
        endpoint,
        model,
    })
}

#[tauri::command]
pub async fn save_ai_settings(
    state: State<'_, AppState>,
    provider: String,
    endpoint: String,
    model: String,
    api_key: String,
) -> Result<(), String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;

    // Validate provider
    let kind: AiProviderKind = provider.parse().map_err(|e: crate::AppError| e.to_string())?;

    // Save non-sensitive settings to SQLite
    crate::cache::set_setting(&db, "ai_provider", &provider).map_err(|e: crate::AppError| e.to_string())?;
    crate::cache::set_setting(&db, "ai_endpoint", &endpoint).map_err(|e: crate::AppError| e.to_string())?;
    crate::cache::set_setting(&db, "ai_model", &model).map_err(|e: crate::AppError| e.to_string())?;

    // Save API key to keyring
    let service = match provider.as_str() {
        "openai" => "pex-ai-openai",
        "anthropic" => "pex-ai-anthropic",
        _ => return Err(format!("Unknown provider: {}", provider)),
    };
    crate::auth::keyring_store::KeyringStore::save_token(service, &api_key)
        .map_err(|e: crate::AppError| e.to_string())?;

    // Reconfigure the AI manager
    let mut ai_mgr_lock = state.ai_manager.lock().map_err(|e| e.to_string())?;
    if let Some(ref mut mgr) = *ai_mgr_lock {
        mgr.configure(kind, &endpoint, &model, &api_key);
    } else {
        let mut mgr = crate::ai::AiManager::new();
        mgr.configure(kind, &endpoint, &model, &api_key);
        *ai_mgr_lock = Some(mgr);
    }

    Ok(())
}

// ---- Explain diff ----

#[tauri::command]
pub async fn explain_diff(
    state: State<'_, AppState>,
    file_path: String,
    old_content: String,
    new_content: String,
) -> Result<String, String> {
    // Ensure AI manager is configured
    {
        let mut ai_mgr_lock = state.ai_manager.lock().map_err(|e| e.to_string())?;
        if ai_mgr_lock.is_none() {
            // Try to auto-configure from DB + keyring
            let db = state.db.lock().map_err(|e| e.to_string())?;
            let mut mgr = crate::ai::AiManager::new();
            let configured = mgr
                .try_configure_from_db(&db)
                .map_err(|e: crate::AppError| e.to_string())?;
            if configured {
                *ai_mgr_lock = Some(mgr);
            }
        }
    }

    let messages = vec![
        ChatMessage {
            role: ChatRole::System,
            content: crate::ai::prompts::EXPLAIN_DIFF_SYSTEM.to_string(),
        },
        ChatMessage {
            role: ChatRole::User,
            content: crate::ai::prompts::explain_diff_user(
                &old_content,
                &new_content,
                &file_path,
            ),
        },
    ];

    let provider = {
        let ai_mgr_lock = state.ai_manager.lock().map_err(|e| e.to_string())?;
        ai_mgr_lock
            .as_ref()
            .and_then(|mgr| mgr.provider_clone())
            .ok_or_else(|| "AI not configured. Set up AI settings in Preferences.".to_string())?
    };
    // Lock is dropped here — safe to await

    let response = provider.chat(&messages).await.map_err(|e| e.to_string())?;
    Ok(response)
}

// ---- Purist commands ----

#[tauri::command]
pub async fn test_ai_connection(state: State<'_, AppState>) -> Result<String, String> {
    let provider = {
        let ai_mgr_lock = state.ai_manager.lock().map_err(|e| e.to_string())?;
        ai_mgr_lock
            .as_ref()
            .and_then(|mgr| mgr.provider_clone())
    };

    let provider = match provider {
        Some(p) => p,
        None => {
            // Try auto-configuring
            let db = state.db.lock().map_err(|e| e.to_string())?;
            let mut ai_mgr_lock = state.ai_manager.lock().map_err(|e| e.to_string())?;
            let mut mgr = crate::ai::AiManager::new();
            let configured = mgr
                .try_configure_from_db(&db)
                .map_err(|e: crate::AppError| e.to_string())?;
            if configured {
                *ai_mgr_lock = Some(mgr);
                ai_mgr_lock
                    .as_ref()
                    .and_then(|mgr| mgr.provider_clone())
                    .ok_or_else(|| "AI not configured. Set up AI settings first.".to_string())?
            } else {
                return Err("AI not configured. Set up AI settings first.".to_string());
            }
        }
    };

    let messages = vec![ChatMessage {
        role: ChatRole::User,
        content: "Hi! Respond with just 'OK'.".to_string(),
    }];

    match provider.chat(&messages).await {
        Ok(response) => Ok(format!("Connected! Response: {}", response.trim())),
        Err(e) => Err(format!("Connection failed: {}", e)),
    }
}

#[tauri::command]
pub async fn check_purist(purist_path: String) -> Result<PuristCheckResult, String> {
    crate::purist::check(&purist_path).map_err(|e: crate::AppError| e.to_string())
}

#[tauri::command]
pub async fn get_purist_path(state: State<'_, AppState>) -> Result<Option<String>, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    crate::cache::get_setting(&db, "purist_path").map_err(|e: crate::AppError| e.to_string())
}

#[tauri::command]
pub async fn save_purist_path(
    state: State<'_, AppState>,
    path: String,
) -> Result<(), String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    crate::cache::set_setting(&db, "purist_path", &path).map_err(|e: crate::AppError| e.to_string())
}

#[derive(serde::Serialize)]
pub struct PuristCheckResult {
    pub ok: bool,
    pub message: String,
}

// ---- PR Review (Purist) ----

/// Start a dry-run PR review via Purist. Streams output via Tauri events.
/// Events emitted: `review-output-chunk` ({text}), `review-output-done` ({success, message})
#[tauri::command]
pub async fn review_pr_dry_run(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    org_url: String,
    project: String,
    repo: String,
    pr_id: i64,
) -> Result<(), String> {
    let pr_url = build_pr_url(&state, &org_url, &project, &repo, pr_id).await?;
    let config = gather_purist_config(&state, &org_url)?;
    let process_holder = state.purist_pid.clone();

    tokio::spawn(async move {
        let app_clone = app.clone();
        if let Err(e) = run_purist(&config, &pr_url, true, "review-output", app, process_holder).await {
            let _ = app_clone.emit(
                "review-output-done",
                serde_json::json!({"success": false, "message": e.to_string()}),
            );
        }
    });

    Ok(())
}

/// Run Purist and post findings to ADO. Streams output via Tauri events.
/// Events emitted: `review-post-chunk` ({text}), `review-post-done` ({success, message})
#[tauri::command]
pub async fn review_pr_post(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    org_url: String,
    project: String,
    repo: String,
    pr_id: i64,
) -> Result<(), String> {
    let pr_url = build_pr_url(&state, &org_url, &project, &repo, pr_id).await?;
    let config = gather_purist_config(&state, &org_url)?;
    let process_holder = state.purist_pid.clone();

    tokio::spawn(async move {
        let app_clone = app.clone();
        if let Err(e) = run_purist(&config, &pr_url, false, "review-post", app, process_holder).await {
            let _ = app_clone.emit(
                "review-post-done",
                serde_json::json!({"success": false, "message": e.to_string()}),
            );
        }
    });

    Ok(())
}

/// Build an ADO PR URL from org URL + project/repo IDs.
async fn build_pr_url(
    state: &State<'_, AppState>,
    org_url: &str,
    project: &str,
    repo: &str,
    pr_id: i64,
) -> Result<String, String> {
    // Clone the ADO client out of the mutex to avoid holding lock across awaits
    let client = {
        let ado = state.ado_client.lock().map_err(|e| e.to_string())?;
        ado.clone()
            .ok_or_else(|| "Not logged in. Connect to an ADO org first.".to_string())?
    };
    // MutexGuard dropped here — safe to await

    // Fetch projects to resolve project name from ID
    let projects = client
        .list_projects()
        .await
        .map_err(|e| e.to_string())?;
    let project_name = projects
        .iter()
        .find(|p| p.id == project)
        .map(|p| p.name.clone())
        .unwrap_or_else(|| project.to_string());

    // Fetch repos to resolve repo name from ID
    let repos = client
        .list_repositories(&project_name)
        .await
        .map_err(|e| e.to_string())?;
    let repo_name = repos
        .iter()
        .find(|r| r.id == repo)
        .map(|r| r.name.clone())
        .unwrap_or_else(|| repo.to_string());

    Ok(format!(
        "{}/{}/_git/{}/pullrequest/{}",
        org_url.trim_end_matches('/'),
        project_name,
        repo_name,
        pr_id
    ))
}

/// Convenience wrapper for running Purist with gathered config.
async fn run_purist(
    config: &(String, String, String, String, String, String),
    pr_url: &str,
    dry_run: bool,
    event_prefix: &str,
    app: tauri::AppHandle,
    process_holder: std::sync::Arc<std::sync::Mutex<Option<u32>>>,
) -> Result<(), crate::AppError> {
    let (purist_path, llm_provider, llm_endpoint, llm_model, llm_api_key, ado_pat) = config;
    crate::purist::run_review(
        purist_path,
        pr_url,
        dry_run,
        ado_pat,
        llm_provider,
        llm_endpoint,
        llm_api_key,
        llm_model,
        event_prefix,
        app,
        process_holder,
    )
    .await
}

/// Cancel a running Purist review.
#[tauri::command]
pub async fn cancel_review(state: State<'_, AppState>) -> Result<(), String> {
    let pid = {
        let mut holder = state.purist_pid.lock().map_err(|e| e.to_string())?;
        holder.take()
    };
    if let Some(pid) = pid {
        crate::purist::cancel(pid);
    }
    Ok(())
}

/// Gather all config needed to run Purist.
fn gather_purist_config(
    state: &State<'_, AppState>,
    org_url: &str,
) -> Result<(String, String, String, String, String, String), String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;

    // Purist path
    let purist_path = crate::cache::get_setting(&db, "purist_path")
        .map_err(|e: crate::AppError| e.to_string())?
        .ok_or_else(|| "Purist path not configured. Set it in AI Settings.".to_string())?;

    // AI settings
    let provider = crate::cache::get_setting(&db, "ai_provider")
        .map_err(|e: crate::AppError| e.to_string())?
        .unwrap_or_else(|| "openai".to_string());
    let endpoint = crate::cache::get_setting(&db, "ai_endpoint")
        .map_err(|e: crate::AppError| e.to_string())?
        .unwrap_or_else(|| "https://api.openai.com".to_string());
    let model = crate::cache::get_setting(&db, "ai_model")
        .map_err(|e: crate::AppError| e.to_string())?
        .unwrap_or_else(|| "gpt-4.1".to_string());
    drop(db);

    // AI API key from keyring
    let ai_service = match provider.as_str() {
        "openai" => "pex-ai-openai",
        "anthropic" => "pex-ai-anthropic",
        _ => "pex-ai-openai",
    };
    let llm_api_key = crate::auth::keyring_store::KeyringStore::get_token(ai_service)
        .map_err(|e: crate::AppError| e.to_string())?
        .ok_or_else(|| "AI API key not configured. Set it in AI Settings.".to_string())?;

    // ADO PAT from keyring
    let ado_pat = crate::auth::keyring_store::KeyringStore::get_pat(org_url)
        .map_err(|e: crate::AppError| e.to_string())?
        .ok_or_else(|| format!("No credentials found for {}. Log in first.", org_url))?;

    Ok((purist_path, provider, endpoint, model, llm_api_key, ado_pat))
}
