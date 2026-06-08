use crate::ai::{
    AiProviderConfig, AiProviderConfigNoKey, AiProviderKind, AiSettingsNoKey, ChatMessage, ChatRole,
};
use crate::diff::engine::{extract_hunks, DiffHunk};
use crate::AppState;
use tauri::State;

// ---- Settings commands ----

#[tauri::command]
pub async fn get_ai_settings(state: State<'_, AppState>) -> Result<AiSettingsNoKey, String> {
    let conn = state.db.conn();

    let (default_provider_id, providers) = crate::ai::read_ai_provider_configs(&conn)
        .await
        .map_err(|e: crate::AppError| e.to_string())?;
    let providers_no_key = providers
        .iter()
        .map(provider_no_key)
        .collect::<Result<Vec<_>, _>>()?;

    let default_provider = providers
        .iter()
        .find(|p| p.id == default_provider_id)
        .or_else(|| providers.first())
        .ok_or_else(|| "AI provider list is empty.".to_string())?;

    let hunk_concurrency = crate::ai::read_hunk_concurrency(&conn)
        .await
        .map_err(|e: crate::AppError| e.to_string())?;

    let hunk_max_tokens = crate::ai::read_hunk_max_tokens(&conn)
        .await
        .map_err(|e: crate::AppError| e.to_string())?;

    let aggregate_max_tokens = crate::ai::read_aggregate_max_tokens(&conn)
        .await
        .map_err(|e: crate::AppError| e.to_string())?;

    let standards_max_chars = crate::ai::read_standards_max_chars(&conn)
        .await
        .map_err(|e: crate::AppError| e.to_string())?;

    let retry_count = crate::ai::read_retry_count(&conn)
        .await
        .map_err(|e: crate::AppError| e.to_string())?;

    let confidence_threshold = crate::ai::read_confidence_threshold(&conn)
        .await
        .map_err(|e: crate::AppError| e.to_string())?;

    let blocking_confidence = crate::ai::read_blocking_confidence(&conn)
        .await
        .map_err(|e: crate::AppError| e.to_string())?;

    let auto_vote_on_blocking = crate::ai::read_auto_vote_on_blocking(&conn)
        .await
        .map_err(|e: crate::AppError| e.to_string())?;

    let incremental_review = crate::ai::read_incremental_review(&conn)
        .await
        .map_err(|e: crate::AppError| e.to_string())?;

    let auto_review = crate::ai::read_auto_review(&conn)
        .await
        .map_err(|e: crate::AppError| e.to_string())?;
    let auto_post_blocking = crate::ai::read_auto_post_blocking(&conn)
        .await
        .map_err(|e: crate::AppError| e.to_string())?;
    let auto_post_confidence = crate::ai::read_auto_post_confidence(&conn)
        .await
        .map_err(|e: crate::AppError| e.to_string())?;

    let ai_diagnostics = crate::ai::read_ai_diagnostics(&conn)
        .await
        .map_err(|e: crate::AppError| e.to_string())?;

    Ok(AiSettingsNoKey {
        default_provider_id,
        providers: providers_no_key,
        provider: default_provider.provider.clone(),
        endpoint: default_provider.endpoint.clone(),
        model: default_provider.model.clone(),
        reasoning_effort: default_provider.reasoning_effort.clone(),
        has_api_key: provider_has_key(default_provider).unwrap_or(false),
        connect_timeout_secs: default_provider.connect_timeout_secs,
        read_timeout_secs: default_provider.read_timeout_secs,
        hunk_concurrency,
        hunk_max_tokens,
        aggregate_max_tokens,
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

fn provider_has_key(provider: &AiProviderConfig) -> Result<bool, String> {
    let key = crate::ai::ai_provider_secret_key(&provider.id);
    let stored = crate::auth::keyring_store::KeyringStore::get_ai_token(&key)
        .map_err(|e: crate::AppError| e.to_string())?
        .is_some_and(|s| !s.is_empty());
    if stored {
        return Ok(true);
    }

    if provider.id != "default" {
        return Ok(false);
    }
    let Ok(kind) = provider.provider.parse::<AiProviderKind>() else {
        return Ok(false);
    };
    crate::auth::keyring_store::KeyringStore::get_ai_token(crate::ai::legacy_provider_secret_key(
        kind,
    ))
    .map(|t| t.is_some_and(|s| !s.is_empty()))
    .map_err(|e: crate::AppError| e.to_string())
}

fn provider_no_key(provider: &AiProviderConfig) -> Result<AiProviderConfigNoKey, String> {
    Ok(AiProviderConfigNoKey {
        id: provider.id.clone(),
        name: provider.name.clone(),
        provider: provider.provider.clone(),
        endpoint: provider.endpoint.clone(),
        model: provider.model.clone(),
        reasoning_effort: provider.reasoning_effort.clone(),
        has_api_key: provider_has_key(provider)?,
        connect_timeout_secs: provider.connect_timeout_secs,
        read_timeout_secs: provider.read_timeout_secs,
    })
}

/// Resolve a form API key against what's stored: a non-empty value is the new
/// key; an empty value means "keep the existing stored key" (the UI never echoes
/// the real key back, so blank = unchanged).
fn resolve_api_key(
    provider_id: &str,
    kind: AiProviderKind,
    form_key: &str,
) -> Result<String, String> {
    let provider_key = crate::ai::ai_provider_secret_key(provider_id);
    if form_key.trim().is_empty() {
        let mut saved = crate::auth::keyring_store::KeyringStore::get_ai_token(&provider_key)
            .map_err(|e: crate::AppError| e.to_string())?;
        if saved.is_none() && provider_id == "default" {
            saved = crate::auth::keyring_store::KeyringStore::get_ai_token(
                crate::ai::legacy_provider_secret_key(kind),
            )
            .map_err(|e: crate::AppError| e.to_string())?;
        }
        Ok(saved.unwrap_or_default())
    } else {
        crate::auth::keyring_store::KeyringStore::save_ai_token(&provider_key, form_key)
            .map_err(|e: crate::AppError| e.to_string())?;
        Ok(form_key.to_string())
    }
}

fn clamp_timeout(secs: u64, default: u64) -> u64 {
    crate::ai::clamp_timeout_value(secs, default)
}

fn normalize_provider_config(
    mut provider: AiProviderConfigNoKey,
) -> Result<AiProviderConfig, String> {
    let kind: AiProviderKind = provider
        .provider
        .parse()
        .map_err(|e: crate::AppError| e.to_string())?;
    provider.id = provider.id.trim().to_string();
    if provider.id.is_empty() {
        return Err("Provider id is required.".to_string());
    }
    provider.name = provider.name.trim().to_string();
    if provider.name.is_empty() {
        provider.name = "Provider".to_string();
    }
    provider.endpoint = provider.endpoint.trim().to_string();
    if provider.endpoint.is_empty() {
        provider.endpoint = crate::ai::default_endpoint_for_kind(kind).to_string();
    }
    provider.model = provider.model.trim().to_string();
    provider.reasoning_effort =
        crate::ai::normalize_reasoning_effort(provider.reasoning_effort.as_deref());

    Ok(AiProviderConfig {
        id: provider.id,
        name: provider.name,
        provider: kind.to_string(),
        endpoint: provider.endpoint,
        model: provider.model,
        reasoning_effort: provider.reasoning_effort,
        connect_timeout_secs: clamp_timeout(
            provider.connect_timeout_secs,
            crate::ai::DEFAULT_CONNECT_TIMEOUT_SECS,
        ),
        read_timeout_secs: clamp_timeout(
            provider.read_timeout_secs,
            crate::ai::DEFAULT_READ_TIMEOUT_SECS,
        ),
    })
}

async fn mirror_default_settings(
    db: &libsql::Connection,
    provider: &AiProviderConfig,
) -> Result<(), crate::AppError> {
    crate::cache::set_setting(db, "ai_provider", &provider.provider).await?;
    crate::cache::set_setting(db, "ai_endpoint", &provider.endpoint).await?;
    crate::cache::set_setting(db, "ai_model", &provider.model).await?;
    match provider.reasoning_effort.as_deref() {
        Some(effort) => crate::cache::set_setting(db, "ai_reasoning_effort", effort).await?,
        None => crate::cache::delete_setting(db, "ai_reasoning_effort").await?,
    }
    crate::cache::set_setting(
        db,
        "ai_connect_timeout_secs",
        &provider.connect_timeout_secs.to_string(),
    )
    .await?;
    crate::cache::set_setting(
        db,
        "ai_read_timeout_secs",
        &provider.read_timeout_secs.to_string(),
    )
    .await?;
    let _ = crate::cache::delete_setting(db, "ai_request_timeout_secs").await;
    Ok(())
}

fn configure_default_provider(
    state: &State<'_, AppState>,
    provider: &AiProviderConfig,
    api_key: &str,
) -> Result<(), String> {
    if provider.model.trim().is_empty() || api_key.trim().is_empty() {
        return Ok(());
    }
    let kind: AiProviderKind = provider
        .provider
        .parse()
        .map_err(|e: crate::AppError| e.to_string())?;
    let mut ai_mgr_lock = state.ai_manager.lock().map_err(|e| e.to_string())?;
    match ai_mgr_lock.as_mut() {
        Some(mgr) => mgr.configure(
            kind,
            &provider.endpoint,
            &provider.model,
            api_key,
            provider.reasoning_effort.clone(),
            provider.connect_timeout_secs,
            provider.read_timeout_secs,
        ),
        None => {
            let mut mgr = crate::ai::AiManager::new();
            mgr.configure(
                kind,
                &provider.endpoint,
                &provider.model,
                api_key,
                provider.reasoning_effort.clone(),
                provider.connect_timeout_secs,
                provider.read_timeout_secs,
            );
            *ai_mgr_lock = Some(mgr);
        }
    }
    Ok(())
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
    reasoning_effort: Option<String>,
    connect_timeout_secs: u64,
    read_timeout_secs: u64,
) -> Result<(), String> {
    let kind: AiProviderKind = provider
        .parse()
        .map_err(|e: crate::AppError| e.to_string())?;

    let connect_timeout = clamp_timeout(
        connect_timeout_secs,
        crate::ai::DEFAULT_CONNECT_TIMEOUT_SECS,
    );
    let read_timeout = clamp_timeout(read_timeout_secs, crate::ai::DEFAULT_READ_TIMEOUT_SECS);

    let default_provider = AiProviderConfig {
        id: "default".to_string(),
        name: "Default".to_string(),
        provider: kind.to_string(),
        endpoint,
        model,
        reasoning_effort: crate::ai::normalize_reasoning_effort(reasoning_effort.as_deref()),
        connect_timeout_secs: connect_timeout,
        read_timeout_secs: read_timeout,
    };
    let api_key = resolve_api_key(&default_provider.id, kind, &api_key)?;

    {
        let conn = state.db.conn();
        crate::ai::write_ai_provider_configs(
            &conn,
            &default_provider.id,
            &[default_provider.clone()],
        )
        .await
        .map_err(|e: crate::AppError| e.to_string())?;
        mirror_default_settings(&conn, &default_provider)
            .await
            .map_err(|e: crate::AppError| e.to_string())?;
    }

    configure_default_provider(&state, &default_provider, &api_key)?;

    Ok(())
}

/// Persist one provider entry. The UI autosaves this on field changes. When the
/// entry is the default, the live AI manager and legacy flat settings are kept
/// in sync so existing review code continues to use the selected default.
#[tauri::command]
pub async fn save_ai_provider_config(
    state: State<'_, AppState>,
    provider_config: AiProviderConfigNoKey,
    api_key: String,
    make_default: bool,
) -> Result<(), String> {
    let provider = normalize_provider_config(provider_config)?;
    let kind: AiProviderKind = provider
        .provider
        .parse()
        .map_err(|e: crate::AppError| e.to_string())?;
    let resolved_key = resolve_api_key(&provider.id, kind, &api_key)?;

    let default_id = {
        let conn = state.db.conn();
        let (current_default_id, mut providers) = crate::ai::read_ai_provider_configs(&conn)
            .await
            .map_err(|e: crate::AppError| e.to_string())?;
        if let Some(existing) = providers.iter_mut().find(|p| p.id == provider.id) {
            *existing = provider.clone();
        } else {
            providers.push(provider.clone());
        }

        let default_id = if make_default {
            provider.id.clone()
        } else if providers.iter().any(|p| p.id == current_default_id) {
            current_default_id
        } else {
            providers[0].id.clone()
        };

        crate::ai::write_ai_provider_configs(&conn, &default_id, &providers)
            .await
            .map_err(|e: crate::AppError| e.to_string())?;
        if default_id == provider.id {
            mirror_default_settings(&conn, &provider)
                .await
                .map_err(|e: crate::AppError| e.to_string())?;
        }
        default_id
    };

    if default_id == provider.id {
        configure_default_provider(&state, &provider, &resolved_key)?;
    }

    Ok(())
}

#[tauri::command]
pub async fn remove_ai_provider(
    state: State<'_, AppState>,
    provider_id: String,
) -> Result<(), String> {
    let (new_default, providers) = {
        let conn = state.db.conn();
        let (current_default_id, mut providers) = crate::ai::read_ai_provider_configs(&conn)
            .await
            .map_err(|e: crate::AppError| e.to_string())?;
        if providers.len() <= 1 {
            return Err("At least one AI provider is required.".to_string());
        }
        providers.retain(|p| p.id != provider_id);
        if providers.is_empty() {
            return Err("At least one AI provider is required.".to_string());
        }
        let new_default = if current_default_id == provider_id {
            providers[0].id.clone()
        } else {
            current_default_id
        };
        crate::ai::write_ai_provider_configs(&conn, &new_default, &providers)
            .await
            .map_err(|e: crate::AppError| e.to_string())?;
        let default_provider = providers
            .iter()
            .find(|p| p.id == new_default)
            .ok_or_else(|| "Default AI provider not found.".to_string())?;
        mirror_default_settings(&conn, default_provider)
            .await
            .map_err(|e: crate::AppError| e.to_string())?;
        (new_default, providers)
    };

    let key = crate::ai::ai_provider_secret_key(&provider_id);
    crate::auth::keyring_store::KeyringStore::delete_ai_token(&key)
        .map_err(|e: crate::AppError| e.to_string())?;

    if let Some(default_provider) = providers.iter().find(|p| p.id == new_default) {
        let kind: AiProviderKind = default_provider
            .provider
            .parse()
            .map_err(|e: crate::AppError| e.to_string())?;
        let api_key = resolve_api_key(&default_provider.id, kind, "")?;
        configure_default_provider(&state, default_provider, &api_key)?;
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
    hunk_max_tokens: u32,
    aggregate_max_tokens: u32,
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
    let conn = state.db.conn();

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

    crate::cache::set_setting(&conn, "ai_hunk_concurrency", &concurrency.to_string())
        .await
        .map_err(|e: crate::AppError| e.to_string())?;
    // Output token caps: clamp to a sane range, falling back to the default for
    // a 0 / fat-fingered value so a cap can never truncate every response.
    let clamp_tokens = |n: u32, default: u32| {
        if n == 0 {
            default
        } else {
            n.clamp(crate::ai::MIN_MAX_TOKENS, crate::ai::MAX_MAX_TOKENS)
        }
    };
    let hunk_tokens = clamp_tokens(hunk_max_tokens, crate::ai::DEFAULT_HUNK_MAX_TOKENS);
    crate::cache::set_setting(&conn, "ai_hunk_max_tokens", &hunk_tokens.to_string())
        .await
        .map_err(|e: crate::AppError| e.to_string())?;
    let agg_tokens = clamp_tokens(
        aggregate_max_tokens,
        crate::ai::DEFAULT_AGGREGATE_MAX_TOKENS,
    );
    crate::cache::set_setting(&conn, "ai_aggregate_max_tokens", &agg_tokens.to_string())
        .await
        .map_err(|e: crate::AppError| e.to_string())?;
    // retry_count: 0 is valid ("do not retry"); just clamp the upper bound.
    let retries = retry_count.min(crate::ai::MAX_RETRY_COUNT);
    crate::cache::set_setting(&conn, "ai_retry_count", &retries.to_string())
        .await
        .map_err(|e: crate::AppError| e.to_string())?;
    crate::cache::set_setting(&conn, "ai_standards_max_chars", &std_chars.to_string())
        .await
        .map_err(|e: crate::AppError| e.to_string())?;
    // confidence_threshold: 0 is valid ("surface everything"); clamp upper bound.
    let threshold = confidence_threshold.min(crate::ai::MAX_CONFIDENCE_THRESHOLD);
    crate::cache::set_setting(&conn, "ai_confidence_threshold", &threshold.to_string())
        .await
        .map_err(|e: crate::AppError| e.to_string())?;
    // blocking_confidence (the "critical line"): 0 is valid; clamp upper bound.
    let blocking = blocking_confidence.min(crate::ai::MAX_BLOCKING_CONFIDENCE);
    crate::cache::set_setting(&conn, "ai_blocking_confidence", &blocking.to_string())
        .await
        .map_err(|e: crate::AppError| e.to_string())?;
    crate::cache::set_setting(
        &conn,
        "ai_auto_vote_on_blocking",
        if auto_vote_on_blocking {
            "true"
        } else {
            "false"
        },
    )
    .await
    .map_err(|e: crate::AppError| e.to_string())?;
    crate::cache::set_setting(
        &conn,
        "ai_incremental_review",
        if incremental_review { "true" } else { "false" },
    )
    .await
    .map_err(|e: crate::AppError| e.to_string())?;
    crate::cache::set_setting(
        &conn,
        "ai_auto_review",
        if auto_review { "true" } else { "false" },
    )
    .await
    .map_err(|e: crate::AppError| e.to_string())?;
    crate::cache::set_setting(
        &conn,
        "ai_auto_post_blocking",
        if auto_post_blocking { "true" } else { "false" },
    )
    .await
    .map_err(|e: crate::AppError| e.to_string())?;
    let auto_post_conf = auto_post_confidence.min(crate::ai::MAX_AUTO_POST_CONFIDENCE);
    crate::cache::set_setting(
        &conn,
        "ai_auto_post_confidence",
        &auto_post_conf.to_string(),
    )
    .await
    .map_err(|e: crate::AppError| e.to_string())?;
    crate::cache::set_setting(
        &conn,
        "ai_diagnostics",
        if ai_diagnostics { "true" } else { "false" },
    )
    .await
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
    state: State<'_, AppState>,
    provider: String,
    endpoint: String,
    api_key: String,
    provider_id: Option<String>,
) -> Result<Vec<String>, String> {
    let kind: AiProviderKind = provider
        .parse()
        .map_err(|e: crate::AppError| e.to_string())?;

    let key = if api_key.trim().is_empty() {
        match provider_id.as_ref().filter(|id| !id.trim().is_empty()) {
            Some(id) => resolve_api_key(id, kind, "")?,
            None => crate::auth::keyring_store::KeyringStore::get_ai_token(
                crate::ai::legacy_provider_secret_key(kind),
            )
            .map_err(|e: crate::AppError| e.to_string())?
            .unwrap_or_default(),
        }
    } else {
        api_key
    };
    if key.is_empty() {
        return Err("Enter an API key to test.".to_string());
    }
    if endpoint.trim().is_empty() {
        return Err("Enter an endpoint URL to test.".to_string());
    }

    let models = crate::ai::models::probe_models(kind, &endpoint, &key)
        .await
        .map_err(|e| e.to_string())?;

    // Persist the freshly-probed list so it survives a dialog reopen instead of
    // reverting to the older cached list. Only write when the tested provider is
    // the default one (the identity `get_cached` reads back), so testing a
    // non-default provider can't clobber the default's cache. The working
    // provider config in `state` is still left untouched — Test must not change
    // settings, so the user still has to Save.
    if let Some(id) = provider_id.as_ref().filter(|id| !id.trim().is_empty()) {
        let conn = state.db.conn();
        if let Ok((default_provider_id, _)) = crate::ai::read_ai_provider_configs(&conn).await {
            if *id == default_provider_id {
                let _ = crate::ai::models::set_cached(&conn, id, kind, &endpoint, &models).await;
            }
        }
    }

    Ok(models)
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
        let conn = state.db.conn();
        let needs_configure = state
            .ai_manager
            .lock()
            .map_err(|e| e.to_string())?
            .is_none();
        if needs_configure {
            let mut mgr = crate::ai::AiManager::new();
            let configured = mgr
                .try_configure_from_db(&conn)
                .await
                .map_err(|e: crate::AppError| e.to_string())?;
            if configured {
                let mut ai_mgr_lock = state.ai_manager.lock().map_err(|e| e.to_string())?;
                if ai_mgr_lock.is_none() {
                    *ai_mgr_lock = Some(mgr);
                }
            }
        }
        crate::ai::prompts::resolve_prompt(&conn, crate::ai::prompts::PromptKey::ExplainHunkSystem)
            .await
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
        match ai_mgr_lock.as_ref().and_then(|mgr| mgr.provider_clone()) {
            Some(provider) => provider,
            None => {
                let message = "AI not configured. Set up AI settings in Preferences.".to_string();
                eprintln!(
                    "explain_hunk failed for {} hunk {}: {}",
                    file_path, hunk_index, message
                );
                return Err(message);
            }
        }
    };

    let response = provider.chat(&messages).await.map_err(|e| {
        eprintln!(
            "explain_hunk failed for {} hunk {}: {}",
            file_path, hunk_index, e
        );
        e.to_string()
    })?;
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
    /// Per-prompt provider override. `None` means use the default provider.
    pub provider_id: Option<String>,
    /// Per-prompt model override. `None` means: fall back to the default
    /// provider/model from the AI tab.
    pub model: Option<String>,
}

async fn prompt_info(
    conn: &libsql::Connection,
    key: crate::ai::prompts::PromptKey,
) -> Result<PromptInfo, crate::AppError> {
    let stored = crate::cache::get_setting(conn, &format!("ai_prompt_{}", key.as_str())).await?;
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
        crate::ai::prompts::PromptKey::ReviewDesignPrinciplesSystem => (
            "Multi-pass: design principles",
            "Thorough PR review specialist — SOLID violations and cross-cutting DRY/duplication.",
        ),
    };

    let model_override = crate::ai::prompts::resolve_model_override(conn, key).await?;

    Ok(PromptInfo {
        key: key.as_str().to_string(),
        label: label.to_string(),
        description: description.to_string(),
        value,
        default_value,
        is_customized,
        provider_id: model_override.as_ref().and_then(|m| m.provider_id.clone()),
        model: model_override.map(|m| m.model),
    })
}

/// One Thorough-mode specialist as shown in the pre-review confirmation dialog.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewSpecialistInfo {
    /// Stable prompt key (matches `PromptKey::as_str`), sent back as the
    /// `enabled_specialists` selection when the user starts the review.
    pub key: String,
    pub label: String,
    pub description: String,
    /// The concrete model this specialist will use — its per-prompt override if
    /// set, otherwise the AI tab's default model. Never the literal "Default".
    pub model: String,
    /// The concrete provider this specialist will use.
    pub provider_name: String,
    /// Whether this specialist is selected by default in the dialog. The lean
    /// default set keeps per-hunk token cost down; the rest stay opt-in.
    pub default_enabled: bool,
}

/// The Thorough-mode specialist roster, in run order, each annotated with the
/// concrete model it will use. Backs the pre-review confirmation dialog.
#[tauri::command]
pub async fn get_review_specialists(
    state: State<'_, AppState>,
) -> Result<Vec<ReviewSpecialistInfo>, String> {
    let conn = state.db.conn();
    let (default_provider_id, providers) = crate::ai::read_ai_provider_configs(&conn)
        .await
        .map_err(|e: crate::AppError| e.to_string())?;
    let default_provider = providers
        .iter()
        .find(|p| p.id == default_provider_id)
        .or_else(|| providers.first())
        .ok_or_else(|| "AI provider list is empty.".to_string())?;
    let mut out = Vec::new();
    for k in crate::ai::prompts::PromptKey::THOROUGH_SPECIALISTS {
        let info = prompt_info(&conn, *k)
            .await
            .map_err(|e: crate::AppError| e.to_string())?;
        let provider = info
            .provider_id
            .as_ref()
            .and_then(|id| providers.iter().find(|p| p.id == *id))
            .unwrap_or(default_provider);
        out.push(ReviewSpecialistInfo {
            key: info.key,
            label: info.label,
            description: info.description,
            model: info.model.unwrap_or_else(|| provider.model.clone()),
            provider_name: provider.name.clone(),
            default_enabled: k.is_default_specialist(),
        });
    }
    Ok(out)
}

#[tauri::command]
pub async fn get_ai_prompts(state: State<'_, AppState>) -> Result<Vec<PromptInfo>, String> {
    let conn = state.db.conn();
    let mut out = Vec::new();
    for k in crate::ai::prompts::PromptKey::ALL {
        out.push(
            prompt_info(&conn, *k)
                .await
                .map_err(|e: crate::AppError| e.to_string())?,
        );
    }
    Ok(out)
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
    let conn = state.db.conn();
    crate::ai::prompts::save_prompt(&conn, prompt_key, trimmed)
        .await
        .map_err(|e: crate::AppError| e.to_string())
}

#[tauri::command]
pub async fn reset_ai_prompt(state: State<'_, AppState>, key: String) -> Result<(), String> {
    let prompt_key = crate::ai::prompts::PromptKey::from_str(&key)
        .map_err(|e: crate::AppError| e.to_string())?;
    let conn = state.db.conn();
    crate::ai::prompts::reset_prompt(&conn, prompt_key)
        .await
        .map_err(|e: crate::AppError| e.to_string())
}

#[tauri::command]
pub async fn save_ai_prompt_model(
    state: State<'_, AppState>,
    key: String,
    model: String,
    provider_id: Option<String>,
) -> Result<(), String> {
    let prompt_key = crate::ai::prompts::PromptKey::from_str(&key)
        .map_err(|e: crate::AppError| e.to_string())?;
    let trimmed = model.trim();
    let conn = state.db.conn();
    if trimmed.is_empty() {
        // Empty string means "use the default AI provider/model" — drop the override.
        crate::ai::prompts::reset_model(&conn, prompt_key)
            .await
            .map_err(|e: crate::AppError| e.to_string())
    } else {
        let provider_id = provider_id
            .as_deref()
            .map(str::trim)
            .filter(|id| !id.is_empty());
        if let Some(provider_id) = provider_id {
            let (_, providers) = crate::ai::read_ai_provider_configs(&conn)
                .await
                .map_err(|e: crate::AppError| e.to_string())?;
            if !providers.iter().any(|p| p.id == provider_id) {
                return Err("Selected AI provider was not found.".to_string());
            }
        }
        crate::ai::prompts::save_model_override(&conn, prompt_key, provider_id, trimmed)
            .await
            .map_err(|e: crate::AppError| e.to_string())
    }
}

#[tauri::command]
pub async fn reset_ai_prompt_model(state: State<'_, AppState>, key: String) -> Result<(), String> {
    let prompt_key = crate::ai::prompts::PromptKey::from_str(&key)
        .map_err(|e: crate::AppError| e.to_string())?;
    let conn = state.db.conn();
    crate::ai::prompts::reset_model(&conn, prompt_key)
        .await
        .map_err(|e: crate::AppError| e.to_string())
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
            let conn = state.db.conn();
            crate::ai::models::get_cached(&conn)
                .await
                .map_err(|e| e.to_string())?
        };
        if let Some(models) = cached {
            return Ok(models);
        }
    }

    let conn = state.db.conn();
    crate::ai::models::fetch_and_cache(&conn)
        .await
        .map_err(|e| e.to_string())
}

/// Returns the live model list for a specific configured provider.
#[tauri::command]
pub async fn list_ai_provider_models(
    state: State<'_, AppState>,
    provider_id: String,
) -> Result<Vec<String>, String> {
    let (kind, endpoint, api_key) = {
        let conn = state.db.conn();
        let (_, providers) = crate::ai::read_ai_provider_configs(&conn)
            .await
            .map_err(|e: crate::AppError| e.to_string())?;
        let provider = providers
            .iter()
            .find(|p| p.id == provider_id)
            .ok_or_else(|| "Selected AI provider was not found.".to_string())?;
        let kind: AiProviderKind = provider
            .provider
            .parse()
            .map_err(|e: crate::AppError| e.to_string())?;
        let api_key = crate::ai::read_ai_provider_api_key(provider)
            .map_err(|e: crate::AppError| e.to_string())?
            .ok_or_else(|| format!("API key not configured for {}.", provider.name))?;
        (kind, provider.endpoint.clone(), api_key)
    };

    crate::ai::models::probe_models(kind, &endpoint, &api_key)
        .await
        .map_err(|e| e.to_string())
}
