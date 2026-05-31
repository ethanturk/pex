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

    let blocking_confidence =
        crate::ai::read_blocking_confidence(&db).map_err(|e: crate::AppError| e.to_string())?;

    let auto_vote_on_blocking =
        crate::ai::read_auto_vote_on_blocking(&db).map_err(|e: crate::AppError| e.to_string())?;

    let incremental_review =
        crate::ai::read_incremental_review(&db).map_err(|e: crate::AppError| e.to_string())?;

    let auto_review =
        crate::ai::read_auto_review(&db).map_err(|e: crate::AppError| e.to_string())?;
    let auto_post_blocking =
        crate::ai::read_auto_post_blocking(&db).map_err(|e: crate::AppError| e.to_string())?;
    let auto_post_confidence =
        crate::ai::read_auto_post_confidence(&db).map_err(|e: crate::AppError| e.to_string())?;

    let ai_diagnostics =
        crate::ai::read_ai_diagnostics(&db).map_err(|e: crate::AppError| e.to_string())?;

    // Whether a key is stored for the *current* provider — drives the masked
    // placeholder in the UI. We never return the key itself.
    let provider_key = match provider.as_str() {
        "anthropic" => "anthropic",
        _ => "openai",
    };
    let has_api_key = crate::auth::keyring_store::KeyringStore::get_ai_token(provider_key)
        .map(|t| t.map(|s| !s.is_empty()).unwrap_or(false))
        .unwrap_or(false);

    Ok(AiSettingsNoKey {
        provider,
        endpoint,
        model,
        has_api_key,
        connect_timeout_secs,
        read_timeout_secs,
        hunk_concurrency,
        standards_max_chars,
        retry_count,
        confidence_threshold,
        blocking_confidence,
        auto_vote_on_blocking,
        incremental_review,
        auto_review,
        auto_post_blocking,
        auto_post_confidence,
        ai_diagnostics,
    })
}

/// Resolve a form API key against what's stored: a non-empty value is the new
/// key; an empty value means "keep the existing stored key" (the UI never echoes
/// the real key back, so blank = unchanged).
fn resolve_api_key(provider_key: &str, form_key: &str) -> Result<String, String> {
    if form_key.trim().is_empty() {
        Ok(
            crate::auth::keyring_store::KeyringStore::get_ai_token(provider_key)
                .map_err(|e: crate::AppError| e.to_string())?
                .unwrap_or_default(),
        )
    } else {
        crate::auth::keyring_store::KeyringStore::save_ai_token(provider_key, form_key)
            .map_err(|e: crate::AppError| e.to_string())?;
        Ok(form_key.to_string())
    }
}

fn clamp_timeout(secs: u64, default: u64) -> u64 {
    if secs == 0 {
        default
    } else {
        secs.min(crate::ai::MAX_TIMEOUT_SECS)
    }
}

/// Persist the "AI Defaults": provider, endpoint, model, API key, and the
/// connect/read timeouts — and reconfigure the live provider. This is the only
/// settings command gated behind an explicit Save (after a successful Test).
#[tauri::command]
pub async fn save_ai_defaults(
    state: State<'_, AppState>,
    provider: String,
    endpoint: String,
    model: String,
    api_key: String,
    connect_timeout_secs: u64,
    read_timeout_secs: u64,
) -> Result<(), String> {
    let kind: AiProviderKind = provider
        .parse()
        .map_err(|e: crate::AppError| e.to_string())?;
    let provider_key = match kind {
        AiProviderKind::OpenAI => "openai",
        AiProviderKind::Anthropic => "anthropic",
    };

    let connect_timeout = clamp_timeout(connect_timeout_secs, crate::ai::DEFAULT_CONNECT_TIMEOUT_SECS);
    let read_timeout = clamp_timeout(read_timeout_secs, crate::ai::DEFAULT_READ_TIMEOUT_SECS);

    let api_key = resolve_api_key(provider_key, &api_key)?;

    {
        let db = state.db.lock().map_err(|e| e.to_string())?;
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
        // Drop the legacy total-request timeout so future reads use the new semantics.
        let _ = crate::cache::delete_setting(&db, "ai_request_timeout_secs");
    }

    // Reconfigure the live provider.
    let mut ai_mgr_lock = state.ai_manager.lock().map_err(|e| e.to_string())?;
    match ai_mgr_lock.as_mut() {
        Some(mgr) => mgr.configure(kind, &endpoint, &model, &api_key, connect_timeout, read_timeout),
        None => {
            let mut mgr = crate::ai::AiManager::new();
            mgr.configure(kind, &endpoint, &model, &api_key, connect_timeout, read_timeout);
            *ai_mgr_lock = Some(mgr);
        }
    }

    Ok(())
}

/// Persist the review/automation preferences. These autosave on change in the
/// UI; they never touch the provider credentials, so saving them can't leak an
/// untested default to disk.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn save_ai_preferences(
    state: State<'_, AppState>,
    hunk_concurrency: u32,
    standards_max_chars: u32,
    retry_count: u32,
    confidence_threshold: u8,
    blocking_confidence: u8,
    auto_vote_on_blocking: bool,
    incremental_review: bool,
    auto_review: bool,
    auto_post_blocking: bool,
    auto_post_confidence: u8,
    ai_diagnostics: bool,
) -> Result<(), String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;

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

    crate::cache::set_setting(&db, "ai_hunk_concurrency", &concurrency.to_string())
        .map_err(|e: crate::AppError| e.to_string())?;
    // retry_count: 0 is valid ("do not retry"); just clamp the upper bound.
    let retries = retry_count.min(crate::ai::MAX_RETRY_COUNT);
    crate::cache::set_setting(&db, "ai_retry_count", &retries.to_string())
        .map_err(|e: crate::AppError| e.to_string())?;
    crate::cache::set_setting(&db, "ai_standards_max_chars", &std_chars.to_string())
        .map_err(|e: crate::AppError| e.to_string())?;
    // confidence_threshold: 0 is valid ("surface everything"); clamp upper bound.
    let threshold = confidence_threshold.min(crate::ai::MAX_CONFIDENCE_THRESHOLD);
    crate::cache::set_setting(&db, "ai_confidence_threshold", &threshold.to_string())
        .map_err(|e: crate::AppError| e.to_string())?;
    // blocking_confidence (the "critical line"): 0 is valid; clamp upper bound.
    let blocking = blocking_confidence.min(crate::ai::MAX_BLOCKING_CONFIDENCE);
    crate::cache::set_setting(&db, "ai_blocking_confidence", &blocking.to_string())
        .map_err(|e: crate::AppError| e.to_string())?;
    crate::cache::set_setting(
        &db,
        "ai_auto_vote_on_blocking",
        if auto_vote_on_blocking { "true" } else { "false" },
    )
    .map_err(|e: crate::AppError| e.to_string())?;
    crate::cache::set_setting(
        &db,
        "ai_incremental_review",
        if incremental_review { "true" } else { "false" },
    )
    .map_err(|e: crate::AppError| e.to_string())?;
    crate::cache::set_setting(
        &db,
        "ai_auto_review",
        if auto_review { "true" } else { "false" },
    )
    .map_err(|e: crate::AppError| e.to_string())?;
    crate::cache::set_setting(
        &db,
        "ai_auto_post_blocking",
        if auto_post_blocking { "true" } else { "false" },
    )
    .map_err(|e: crate::AppError| e.to_string())?;
    let auto_post_conf = auto_post_confidence.min(crate::ai::MAX_AUTO_POST_CONFIDENCE);
    crate::cache::set_setting(&db, "ai_auto_post_confidence", &auto_post_conf.to_string())
        .map_err(|e: crate::AppError| e.to_string())?;
    crate::cache::set_setting(
        &db,
        "ai_diagnostics",
        if ai_diagnostics { "true" } else { "false" },
    )
    .map_err(|e: crate::AppError| e.to_string())?;

    Ok(())
}

/// Test the AI Defaults form *without persisting anything*. Probes the given
/// provider/endpoint/key by listing models; an empty `api_key` falls back to the
/// stored key so the user can re-test without re-typing it. On success returns
/// the model list so the UI can populate (and validate) the Model dropdown.
/// This is what gates the Save button — Save is only enabled after Test passes.
#[tauri::command]
pub async fn test_ai_defaults(
    provider: String,
    endpoint: String,
    api_key: String,
) -> Result<Vec<String>, String> {
    let kind: AiProviderKind = provider
        .parse()
        .map_err(|e: crate::AppError| e.to_string())?;
    let provider_key = match kind {
        AiProviderKind::OpenAI => "openai",
        AiProviderKind::Anthropic => "anthropic",
    };

    let key = if api_key.trim().is_empty() {
        crate::auth::keyring_store::KeyringStore::get_ai_token(provider_key)
            .map_err(|e: crate::AppError| e.to_string())?
            .unwrap_or_default()
    } else {
        api_key
    };
    if key.is_empty() {
        return Err("Enter an API key to test.".to_string());
    }
    if endpoint.trim().is_empty() {
        return Err("Enter an endpoint URL to test.".to_string());
    }

    crate::ai::models::probe_models(kind, &endpoint, &key)
        .await
        .map_err(|e| e.to_string())
    // Note: the working provider in `state` is intentionally left untouched —
    // Test must not have side effects, so the user still has to Save.
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

// ---- Hunk Review ----

/// Extract structured diff hunks from old/new content for per-hunk review UI.
#[tauri::command]
pub fn get_diff_hunks(old_content: String, new_content: String) -> Result<Vec<DiffHunk>, String> {
    Ok(extract_hunks(&old_content, &new_content))
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
