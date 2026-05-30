use crate::ai::{AiProviderKind, AiSettingsNoKey, ChatMessage, ChatRole};
use crate::diff::engine::{extract_hunks, DiffHunk};
use crate::AppState;
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

    let connect_timeout_secs =
        crate::ai::read_connect_timeout(&db).map_err(|e: crate::AppError| e.to_string())?;

    let read_timeout_secs =
        crate::ai::read_read_timeout(&db).map_err(|e: crate::AppError| e.to_string())?;

    let hunk_concurrency =
        crate::ai::read_hunk_concurrency(&db).map_err(|e: crate::AppError| e.to_string())?;

    let standards_max_chars =
        crate::ai::read_standards_max_chars(&db).map_err(|e: crate::AppError| e.to_string())?;

    let retry_count =
        crate::ai::read_retry_count(&db).map_err(|e: crate::AppError| e.to_string())?;

    let confidence_threshold =
        crate::ai::read_confidence_threshold(&db).map_err(|e: crate::AppError| e.to_string())?;

    Ok(AiSettingsNoKey {
        provider,
        endpoint,
        model,
        connect_timeout_secs,
        read_timeout_secs,
        hunk_concurrency,
        standards_max_chars,
        retry_count,
        confidence_threshold,
    })
}

#[tauri::command]
pub async fn save_ai_settings(
    state: State<'_, AppState>,
    provider: String,
    endpoint: String,
    model: String,
    api_key: String,
    connect_timeout_secs: u64,
    read_timeout_secs: u64,
    hunk_concurrency: u32,
    standards_max_chars: u32,
    retry_count: u32,
    confidence_threshold: u8,
) -> Result<(), String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;

    // Validate provider
    let kind: AiProviderKind = provider
        .parse()
        .map_err(|e: crate::AppError| e.to_string())?;

    // Clamp timeouts to a sane range; 0 falls back to default.
    let connect_timeout = if connect_timeout_secs == 0 {
        crate::ai::DEFAULT_CONNECT_TIMEOUT_SECS
    } else {
        connect_timeout_secs.min(crate::ai::MAX_TIMEOUT_SECS)
    };
    let read_timeout = if read_timeout_secs == 0 {
        crate::ai::DEFAULT_READ_TIMEOUT_SECS
    } else {
        read_timeout_secs.min(crate::ai::MAX_TIMEOUT_SECS)
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
    crate::cache::set_setting(&db, "ai_provider", &provider)
        .map_err(|e: crate::AppError| e.to_string())?;
    crate::cache::set_setting(&db, "ai_endpoint", &endpoint)
        .map_err(|e: crate::AppError| e.to_string())?;
    crate::cache::set_setting(&db, "ai_model", &model)
        .map_err(|e: crate::AppError| e.to_string())?;
    crate::cache::set_setting(&db, "ai_connect_timeout_secs", &connect_timeout.to_string())
        .map_err(|e: crate::AppError| e.to_string())?;
    crate::cache::set_setting(&db, "ai_read_timeout_secs", &read_timeout.to_string())
        .map_err(|e: crate::AppError| e.to_string())?;
    // Drop the legacy total-request timeout once the new keys are set so future
    // reads use the new semantics instead of silently falling back.
    let _ = crate::cache::delete_setting(&db, "ai_request_timeout_secs");
    crate::cache::set_setting(&db, "ai_hunk_concurrency", &concurrency.to_string())
        .map_err(|e: crate::AppError| e.to_string())?;
    // retry_count: 0 is a valid value ("do not retry"), unlike the other
    // numeric settings where 0 means "use default". Just clamp the upper bound.
    let retries = retry_count.min(crate::ai::MAX_RETRY_COUNT);

    crate::cache::set_setting(&db, "ai_retry_count", &retries.to_string())
        .map_err(|e: crate::AppError| e.to_string())?;
    crate::cache::set_setting(&db, "ai_standards_max_chars", &std_chars.to_string())
        .map_err(|e: crate::AppError| e.to_string())?;
    // confidence_threshold: 0 is valid ("surface everything"); just clamp the
    // upper bound to 100.
    let threshold = confidence_threshold.min(crate::ai::MAX_CONFIDENCE_THRESHOLD);
    crate::cache::set_setting(&db, "ai_confidence_threshold", &threshold.to_string())
        .map_err(|e: crate::AppError| e.to_string())?;

    // Save API key to keyring when the user provided one. The UI intentionally
    // does not echo stored keys back into the password field, so an empty value
    // means "keep the existing key" instead of "overwrite with blank".
    let provider_key = match provider.as_str() {
        "openai" => "openai",
        "anthropic" => "anthropic",
        _ => return Err(format!("Unknown provider: {}", provider)),
    };
    let api_key = if api_key.trim().is_empty() {
        crate::auth::keyring_store::KeyringStore::get_ai_token(provider_key)
            .map_err(|e: crate::AppError| e.to_string())?
            .unwrap_or_default()
    } else {
        crate::auth::keyring_store::KeyringStore::save_ai_token(provider_key, &api_key)
            .map_err(|e: crate::AppError| e.to_string())?;
        api_key
    };

    // Reconfigure the AI manager
    let mut ai_mgr_lock = state.ai_manager.lock().map_err(|e| e.to_string())?;
    if let Some(ref mut mgr) = *ai_mgr_lock {
        mgr.configure(
            kind,
            &endpoint,
            &model,
            &api_key,
            connect_timeout,
            read_timeout,
        );
    } else {
        let mut mgr = crate::ai::AiManager::new();
        mgr.configure(
            kind,
            &endpoint,
            &model,
            &api_key,
            connect_timeout,
            read_timeout,
        );
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
    let hunk = hunks.get(hunk_index).ok_or_else(|| {
        format!(
            "Hunk index {} not found ({} hunks total)",
            hunk_index,
            hunks.len()
        )
    })?;

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
        ai_mgr_lock.as_ref().and_then(|mgr| mgr.provider_clone())
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

// ---- Hunk Review ----

/// Extract structured diff hunks from old/new content for per-hunk review UI.
#[tauri::command]
pub fn get_diff_hunks(old_content: String, new_content: String) -> Result<Vec<DiffHunk>, String> {
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
    let hunk = hunks.get(hunk_index).ok_or_else(|| {
        format!(
            "Hunk index {} not found ({} hunks total)",
            hunk_index,
            hunks.len()
        )
    })?;

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
        let cap =
            crate::ai::read_standards_max_chars(&db).map_err(|e: crate::AppError| e.to_string())?;
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
    /// Per-prompt model override. `None` means: fall back to the AI tab's model.
    /// Only consulted by Thorough PR review for specialist prompts today, but
    /// stored for any prompt so the UI is uniform.
    pub model: Option<String>,
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
            "Used by the Fast PR review mode when reviewing each diff hunk, and when you click Review on a single hunk.",
        ),
        crate::ai::prompts::PromptKey::ReviewCodeReviewerSystem => (
            "Multi-pass: code reviewer",
            "Thorough PR review specialist — guideline adherence, style, best practices.",
        ),
        crate::ai::prompts::PromptKey::ReviewSilentFailureSystem => (
            "Multi-pass: silent-failure hunter",
            "Thorough PR review specialist — silent failures, error handling, suppressed errors.",
        ),
        crate::ai::prompts::PromptKey::ReviewCommentAnalyzerSystem => (
            "Multi-pass: comment analyzer",
            "Thorough PR review specialist — comment accuracy and long-term maintainability.",
        ),
        crate::ai::prompts::PromptKey::ReviewTestAnalyzerSystem => (
            "Multi-pass: test-coverage analyzer",
            "Thorough PR review specialist — behavioral test coverage and critical gaps.",
        ),
        crate::ai::prompts::PromptKey::ReviewTypeDesignSystem => (
            "Multi-pass: type-design analyzer",
            "Thorough PR review specialist — type design, invariants, encapsulation.",
        ),
        crate::ai::prompts::PromptKey::ReviewCodeSimplifierSystem => (
            "Multi-pass: code simplifier",
            "Thorough PR review specialist — clarity, redundancy, unnecessary complexity.",
        ),
    };

    let model = crate::ai::prompts::resolve_model(conn, key)?;

    Ok(PromptInfo {
        key: key.as_str().to_string(),
        label: label.to_string(),
        description: description.to_string(),
        value,
        default_value,
        is_customized,
        model,
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
        return Err(
            "Prompt cannot be empty. Use the Reset button to restore the default.".to_string(),
        );
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
    crate::ai::prompts::reset_prompt(&db, prompt_key).map_err(|e: crate::AppError| e.to_string())
}

#[tauri::command]
pub async fn save_ai_prompt_model(
    state: State<'_, AppState>,
    key: String,
    model: String,
) -> Result<(), String> {
    let prompt_key = crate::ai::prompts::PromptKey::from_str(&key)
        .map_err(|e: crate::AppError| e.to_string())?;
    let trimmed = model.trim();
    let db = state.db.lock().map_err(|e| e.to_string())?;
    if trimmed.is_empty() {
        // Empty string means "use the default AI model" — just drop the override.
        crate::ai::prompts::reset_model(&db, prompt_key).map_err(|e: crate::AppError| e.to_string())
    } else {
        crate::ai::prompts::save_model(&db, prompt_key, trimmed)
            .map_err(|e: crate::AppError| e.to_string())
    }
}

#[tauri::command]
pub async fn reset_ai_prompt_model(state: State<'_, AppState>, key: String) -> Result<(), String> {
    let prompt_key = crate::ai::prompts::PromptKey::from_str(&key)
        .map_err(|e: crate::AppError| e.to_string())?;
    let db = state.db.lock().map_err(|e| e.to_string())?;
    crate::ai::prompts::reset_model(&db, prompt_key).map_err(|e: crate::AppError| e.to_string())
}

/// Returns the list of models available from the configured provider.
/// If `refresh` is true (or there is no cache for the current provider/endpoint),
/// fetches the live /models endpoint; otherwise returns the cached list.
#[tauri::command]
pub async fn list_ai_models(
    state: State<'_, AppState>,
    refresh: Option<bool>,
) -> Result<Vec<String>, String> {
    let refresh = refresh.unwrap_or(false);

    if !refresh {
        let cached = {
            let db = state.db.lock().map_err(|e| e.to_string())?;
            crate::ai::models::get_cached(&db).map_err(|e| e.to_string())?
        };
        if let Some(models) = cached {
            return Ok(models);
        }
    }

    crate::ai::models::fetch_and_cache(&state.db)
        .await
        .map_err(|e| e.to_string())
}
