use crate::ai::{AiProviderKind, AiSettingsNoKey, ChatMessage, ChatRole};
use crate::diff::engine::{extract_hunks, DiffHunk};
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

    let request_timeout_secs =
        crate::ai::read_request_timeout(&db).map_err(|e: crate::AppError| e.to_string())?;

    let hunk_concurrency =
        crate::ai::read_hunk_concurrency(&db).map_err(|e: crate::AppError| e.to_string())?;

    let standards_max_chars =
        crate::ai::read_standards_max_chars(&db).map_err(|e: crate::AppError| e.to_string())?;

    Ok(AiSettingsNoKey {
        provider,
        endpoint,
        model,
        request_timeout_secs,
        hunk_concurrency,
        standards_max_chars,
    })
}

#[tauri::command]
pub async fn save_ai_settings(
    state: State<'_, AppState>,
    provider: String,
    endpoint: String,
    model: String,
    api_key: String,
    request_timeout_secs: u64,
    hunk_concurrency: u32,
    standards_max_chars: u32,
) -> Result<(), String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;

    // Validate provider
    let kind: AiProviderKind = provider.parse().map_err(|e: crate::AppError| e.to_string())?;

    // Clamp timeout to a sane range; 0 falls back to default.
    let timeout = if request_timeout_secs == 0 {
        crate::ai::DEFAULT_REQUEST_TIMEOUT_SECS
    } else {
        request_timeout_secs.min(3600)
    };

    let concurrency = if hunk_concurrency == 0 {
        crate::ai::DEFAULT_HUNK_CONCURRENCY
    } else {
        hunk_concurrency.min(crate::ai::MAX_HUNK_CONCURRENCY)
    };

    let std_chars = if standards_max_chars == 0 {
        crate::ai::DEFAULT_STANDARDS_MAX_CHARS
    } else {
        standards_max_chars
            .max(crate::ai::MIN_STANDARDS_MAX_CHARS)
            .min(crate::ai::MAX_STANDARDS_MAX_CHARS)
    };

    // Save non-sensitive settings to SQLite
    crate::cache::set_setting(&db, "ai_provider", &provider).map_err(|e: crate::AppError| e.to_string())?;
    crate::cache::set_setting(&db, "ai_endpoint", &endpoint).map_err(|e: crate::AppError| e.to_string())?;
    crate::cache::set_setting(&db, "ai_model", &model).map_err(|e: crate::AppError| e.to_string())?;
    crate::cache::set_setting(&db, "ai_request_timeout_secs", &timeout.to_string())
        .map_err(|e: crate::AppError| e.to_string())?;
    crate::cache::set_setting(&db, "ai_hunk_concurrency", &concurrency.to_string())
        .map_err(|e: crate::AppError| e.to_string())?;
    crate::cache::set_setting(&db, "ai_standards_max_chars", &std_chars.to_string())
        .map_err(|e: crate::AppError| e.to_string())?;

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
        mgr.configure(kind, &endpoint, &model, &api_key, timeout);
    } else {
        let mut mgr = crate::ai::AiManager::new();
        mgr.configure(kind, &endpoint, &model, &api_key, timeout);
        *ai_mgr_lock = Some(mgr);
    }

    Ok(())
}

// ---- Explain hunk ----

/// Get an AI explanation of a single diff hunk.
#[tauri::command]
pub async fn explain_hunk(
    state: State<'_, AppState>,
    file_path: String,
    old_content: String,
    new_content: String,
    hunk_index: usize,
) -> Result<String, String> {
    let hunks = extract_hunks(&old_content, &new_content);
    let hunk = hunks
        .get(hunk_index)
        .ok_or_else(|| format!("Hunk index {} not found ({} hunks total)", hunk_index, hunks.len()))?;

    let hunk_text: String = hunk
        .lines
        .iter()
        .map(|l| format!("{}{}", l.kind, l.content))
        .collect();

    // Ensure AI manager is configured + resolve the (possibly user-customized) system prompt.
    let system_prompt = {
        let db = state.db.lock().map_err(|e| e.to_string())?;
        let mut ai_mgr_lock = state.ai_manager.lock().map_err(|e| e.to_string())?;
        if ai_mgr_lock.is_none() {
            let mut mgr = crate::ai::AiManager::new();
            let configured = mgr
                .try_configure_from_db(&db)
                .map_err(|e: crate::AppError| e.to_string())?;
            if configured {
                *ai_mgr_lock = Some(mgr);
            }
        }
        crate::ai::prompts::resolve_prompt(&db, crate::ai::prompts::PromptKey::ExplainHunkSystem)
            .map_err(|e: crate::AppError| e.to_string())?
    };

    let messages = vec![
        ChatMessage {
            role: ChatRole::System,
            content: system_prompt,
        },
        ChatMessage {
            role: ChatRole::User,
            content: crate::ai::prompts::explain_hunk_user(&file_path, &hunk.header, &hunk_text),
        },
    ];

    let provider = {
        let ai_mgr_lock = state.ai_manager.lock().map_err(|e| e.to_string())?;
        ai_mgr_lock
            .as_ref()
            .and_then(|mgr| mgr.provider_clone())
            .ok_or_else(|| "AI not configured. Set up AI settings in Preferences.".to_string())?
    };

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

// ---- Purist config file editor ----

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PuristConfigPayload {
    /// Resolved config file path (user override if set, otherwise `<purist_path>/config.yaml`).
    pub path: String,
    /// Whether the path was explicitly chosen by the user, vs. derived from `purist_path`.
    pub is_custom_path: bool,
    /// Whether the file currently exists on disk.
    pub exists: bool,
    /// File contents (empty string if the file doesn't exist yet).
    pub content: String,
}

fn resolve_purist_config_path(conn: &rusqlite::Connection) -> Result<(String, bool), crate::AppError> {
    if let Some(custom) = crate::cache::get_setting(conn, "purist_config_path")?
        .filter(|s| !s.is_empty())
    {
        return Ok((custom, true));
    }
    let base = crate::cache::get_setting(conn, "purist_path")?.unwrap_or_default();
    if base.is_empty() {
        return Err(crate::AppError::Ai(
            "Purist path is not configured. Set it first under the Purist tab.".to_string(),
        ));
    }
    let derived = std::path::Path::new(&base)
        .join("config.yaml")
        .to_string_lossy()
        .into_owned();
    Ok((derived, false))
}

#[tauri::command]
pub async fn get_purist_config(state: State<'_, AppState>) -> Result<PuristConfigPayload, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    let (path, is_custom_path) =
        resolve_purist_config_path(&db).map_err(|e: crate::AppError| e.to_string())?;
    let exists = std::path::Path::new(&path).is_file();
    let content = if exists {
        std::fs::read_to_string(&path).map_err(|e| format!("Failed to read {}: {}", path, e))?
    } else {
        String::new()
    };
    Ok(PuristConfigPayload {
        path,
        is_custom_path,
        exists,
        content,
    })
}

#[tauri::command]
pub async fn save_purist_config(
    state: State<'_, AppState>,
    content: String,
) -> Result<PuristConfigPayload, String> {
    let path = {
        let db = state.db.lock().map_err(|e| e.to_string())?;
        let (p, _) = resolve_purist_config_path(&db).map_err(|e: crate::AppError| e.to_string())?;
        p
    };
    if let Some(parent) = std::path::Path::new(&path).parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create directory for {}: {}", path, e))?;
    }
    std::fs::write(&path, &content).map_err(|e| format!("Failed to write {}: {}", path, e))?;
    get_purist_config(state).await
}

#[tauri::command]
pub async fn set_purist_config_path(
    state: State<'_, AppState>,
    path: String,
) -> Result<(), String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    let trimmed = path.trim();
    if trimmed.is_empty() {
        crate::cache::delete_setting(&db, "purist_config_path")
            .map_err(|e: crate::AppError| e.to_string())
    } else {
        crate::cache::set_setting(&db, "purist_config_path", trimmed)
            .map_err(|e: crate::AppError| e.to_string())
    }
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

// ---- Hunk Review ----

/// Extract structured diff hunks from old/new content for per-hunk review UI.
#[tauri::command]
pub fn get_diff_hunks(
    old_content: String,
    new_content: String,
) -> Result<Vec<DiffHunk>, String> {
    Ok(extract_hunks(&old_content, &new_content))
}

/// Get an AI review of a single diff hunk.
#[tauri::command]
pub async fn review_hunk(
    state: State<'_, AppState>,
    file_path: String,
    old_content: String,
    new_content: String,
    hunk_index: usize,
    // Optional ADO context — when all four are present we look up the nearest
    // AGENTS.md / STYLE.md at `source_commit` to ground the review in project
    // conventions. Older callers (and unit-test paths) pass None and behave
    // exactly as before.
    org_url: Option<String>,
    project_id: Option<String>,
    repo_id: Option<String>,
    source_commit: Option<String>,
) -> Result<String, String> {
    // Extract the specific hunk
    let hunks = extract_hunks(&old_content, &new_content);
    let hunk = hunks
        .get(hunk_index)
        .ok_or_else(|| format!("Hunk index {} not found ({} hunks total)", hunk_index, hunks.len()))?;

    // Build hunk text: header + each line with its +/-/space prefix
    let hunk_text: String = hunk
        .lines
        .iter()
        .map(|l| format!("{}{}", l.kind, l.content))
        .collect::<Vec<_>>()
        .join("");

    // Ensure AI manager is configured + resolve the (possibly user-customized) system prompt
    // and the standards-injection size cap in one DB lock acquisition.
    let (system_prompt, standards_max_chars) = {
        let db = state.db.lock().map_err(|e| e.to_string())?;
        let mut ai_mgr_lock = state.ai_manager.lock().map_err(|e| e.to_string())?;
        if ai_mgr_lock.is_none() {
            let mut mgr = crate::ai::AiManager::new();
            let configured = mgr
                .try_configure_from_db(&db)
                .map_err(|e: crate::AppError| e.to_string())?;
            if configured {
                *ai_mgr_lock = Some(mgr);
            }
        }
        let prompt = crate::ai::prompts::resolve_prompt(
            &db,
            crate::ai::prompts::PromptKey::ReviewHunkSystem,
        )
        .map_err(|e: crate::AppError| e.to_string())?;
        let cap = crate::ai::read_standards_max_chars(&db)
            .map_err(|e: crate::AppError| e.to_string())?;
        (prompt, cap)
    };

    // Resolve AGENTS.md / STYLE.md if the caller supplied full ADO context.
    let standards_ctx = match (&org_url, &project_id, &repo_id, &source_commit) {
        (Some(org), Some(project), Some(repo), Some(commit))
            if !org.is_empty() && !commit.is_empty() =>
        {
            let client = {
                let guard = state.ado_client.lock().map_err(|e| e.to_string())?;
                guard.as_ref().cloned()
            };
            if let Some(client) = client {
                crate::ai::standards::resolve(
                    &client,
                    &state.standards_cache,
                    org,
                    project,
                    repo,
                    commit,
                    &file_path,
                    standards_max_chars as usize,
                )
                .await
            } else {
                crate::ai::standards::StandardsContext::default()
            }
        }
        _ => crate::ai::standards::StandardsContext::default(),
    };

    let agents_arg = standards_ctx
        .agents
        .as_ref()
        .map(|d| (d.path.as_str(), d.content.as_str()));
    let style_arg = standards_ctx
        .style
        .as_ref()
        .map(|d| (d.path.as_str(), d.content.as_str()));

    let messages = vec![
        ChatMessage {
            role: ChatRole::System,
            content: system_prompt,
        },
        ChatMessage {
            role: ChatRole::User,
            content: crate::ai::prompts::review_hunk_user(
                &file_path,
                &hunk.header,
                &hunk_text,
                agents_arg,
                style_arg,
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

    let response = provider.chat(&messages).await.map_err(|e| e.to_string())?;
    Ok(response)
}

// ---- Prompt customization ----

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PromptInfo {
    pub key: String,
    pub label: String,
    pub description: String,
    pub value: String,
    pub default_value: String,
    pub is_customized: bool,
}

fn prompt_info(
    conn: &rusqlite::Connection,
    key: crate::ai::prompts::PromptKey,
) -> Result<PromptInfo, crate::AppError> {
    let stored = crate::cache::get_setting(conn, &format!("ai_prompt_{}", key.as_str()))?;
    let default_value = key.default_text().to_string();
    let is_customized = stored.as_ref().is_some_and(|s| !s.is_empty());
    let value = stored
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| default_value.clone());

    let (label, description) = match key {
        crate::ai::prompts::PromptKey::ExplainHunkSystem => (
            "Explain hunk — system prompt",
            "Used when you click ✨ Explain on a diff hunk.",
        ),
        crate::ai::prompts::PromptKey::ReviewHunkSystem => (
            "Review hunk — system prompt",
            "Used when you click Review on a diff hunk.",
        ),
    };

    Ok(PromptInfo {
        key: key.as_str().to_string(),
        label: label.to_string(),
        description: description.to_string(),
        value,
        default_value,
        is_customized,
    })
}

#[tauri::command]
pub async fn get_ai_prompts(state: State<'_, AppState>) -> Result<Vec<PromptInfo>, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    crate::ai::prompts::PromptKey::ALL
        .iter()
        .map(|k| prompt_info(&db, *k).map_err(|e: crate::AppError| e.to_string()))
        .collect()
}

#[tauri::command]
pub async fn save_ai_prompt(
    state: State<'_, AppState>,
    key: String,
    value: String,
) -> Result<(), String> {
    let prompt_key = crate::ai::prompts::PromptKey::from_str(&key)
        .map_err(|e: crate::AppError| e.to_string())?;
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err("Prompt cannot be empty. Use the Reset button to restore the default.".to_string());
    }
    let db = state.db.lock().map_err(|e| e.to_string())?;
    crate::ai::prompts::save_prompt(&db, prompt_key, trimmed)
        .map_err(|e: crate::AppError| e.to_string())
}

#[tauri::command]
pub async fn reset_ai_prompt(state: State<'_, AppState>, key: String) -> Result<(), String> {
    let prompt_key = crate::ai::prompts::PromptKey::from_str(&key)
        .map_err(|e: crate::AppError| e.to_string())?;
    let db = state.db.lock().map_err(|e| e.to_string())?;
    crate::ai::prompts::reset_prompt(&db, prompt_key)
        .map_err(|e: crate::AppError| e.to_string())
}
