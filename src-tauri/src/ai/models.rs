//! Fetch + cache the list of models available from the user's configured AI
//! provider. Used by the prompt-customization UI to populate per-specialist
//! model dropdowns.
//!
//! Cache: a single SQLite row keyed `ai_models_cache` holding JSON
//! `{ provider, endpoint, models }`. The cache is invalidated implicitly when
//! `provider` or `endpoint` changes — callers consume the cache only when those
//! two fields match the current settings.

use crate::ai::AiProviderKind;
use crate::AppError;
use serde::{Deserialize, Serialize};

const CACHE_KEY: &str = "ai_models_cache";

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CachedModels {
    provider: String,
    endpoint: String,
    models: Vec<String>,
}

/// Return the cached models for the currently-configured provider+endpoint,
/// or `None` if there is no cache, the cache is stale (different provider /
/// endpoint), or provider settings aren't configured.
pub fn get_cached(conn: &rusqlite::Connection) -> Result<Option<Vec<String>>, AppError> {
    let provider = crate::cache::get_setting(conn, "ai_provider")?;
    let endpoint = crate::cache::get_setting(conn, "ai_endpoint")?;
    let raw = crate::cache::get_setting(conn, CACHE_KEY)?;

    let (Some(provider), Some(endpoint), Some(raw)) = (provider, endpoint, raw) else {
        return Ok(None);
    };

    let cached: CachedModels = match serde_json::from_str(&raw) {
        Ok(c) => c,
        // Stale schema — drop it so we re-fetch fresh next call.
        Err(_) => {
            let _ = crate::cache::delete_setting(conn, CACHE_KEY);
            return Ok(None);
        }
    };

    if cached.provider != provider || cached.endpoint != endpoint {
        return Ok(None);
    }

    Ok(Some(cached.models))
}

/// Fetch the live model list from the provider's /models endpoint and persist
/// it to the cache. Requires AI settings to be fully configured.
pub async fn fetch_and_cache(
    conn_mutex: &std::sync::Mutex<rusqlite::Connection>,
) -> Result<Vec<String>, AppError> {
    // Read the settings we need under a short lock, then drop the guard before
    // doing network I/O so other DB consumers aren't blocked on the request.
    let (kind, endpoint, api_key) = {
        let conn = conn_mutex
            .lock()
            .map_err(|_| AppError::Ai("Failed to acquire DB lock".to_string()))?;
        let provider = crate::cache::get_setting(&conn, "ai_provider")?
            .ok_or_else(|| AppError::Ai("AI provider not configured.".to_string()))?;
        let endpoint = crate::cache::get_setting(&conn, "ai_endpoint")?
            .ok_or_else(|| AppError::Ai("AI endpoint not configured.".to_string()))?;
        let kind: AiProviderKind = provider.parse()?;
        let provider_key = match kind {
            AiProviderKind::OpenAI => "openai",
            AiProviderKind::Anthropic => "anthropic",
        };
        let api_key = crate::auth::keyring_store::KeyringStore::get_ai_token(provider_key)?
            .ok_or_else(|| AppError::Ai("API key not configured.".to_string()))?;
        (kind, endpoint, api_key)
    };

    let models = match kind {
        AiProviderKind::OpenAI => fetch_openai_models(&endpoint, &api_key).await?,
        AiProviderKind::Anthropic => fetch_anthropic_models(&endpoint, &api_key).await?,
    };

    // Persist after a successful fetch.
    let provider_str = match kind {
        AiProviderKind::OpenAI => "openai",
        AiProviderKind::Anthropic => "anthropic",
    };
    let payload = serde_json::to_string(&CachedModels {
        provider: provider_str.to_string(),
        endpoint,
        models: models.clone(),
    })
    .map_err(|e| AppError::Ai(format!("Failed to serialize models cache: {}", e)))?;
    {
        let conn = conn_mutex
            .lock()
            .map_err(|_| AppError::Ai("Failed to acquire DB lock".to_string()))?;
        crate::cache::set_setting(&conn, CACHE_KEY, &payload)?;
    }

    Ok(models)
}

/// Fetch the model list for an explicit provider/endpoint/key, without reading
/// or writing any stored settings. Used by the "Test" button on the AI Defaults
/// form so the user can validate credentials before saving them.
pub async fn probe_models(
    kind: AiProviderKind,
    endpoint: &str,
    api_key: &str,
) -> Result<Vec<String>, AppError> {
    match kind {
        AiProviderKind::OpenAI => fetch_openai_models(endpoint, api_key).await,
        AiProviderKind::Anthropic => fetch_anthropic_models(endpoint, api_key).await,
    }
}

#[derive(Deserialize)]
struct OpenAiModelsResponse {
    data: Vec<OpenAiModel>,
}

#[derive(Deserialize)]
struct OpenAiModel {
    id: String,
}

async fn fetch_openai_models(endpoint: &str, api_key: &str) -> Result<Vec<String>, AppError> {
    let url = format!("{}/models", endpoint.trim_end_matches('/'));
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| AppError::Ai(format!("HTTP client error: {}", e)))?;

    let resp = client
        .get(&url)
        .bearer_auth(api_key)
        .send()
        .await
        .map_err(|e| AppError::Ai(format!("Failed to fetch models: {}", e)))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(AppError::Ai(format!(
            "Models endpoint returned {}: {}",
            status, body
        )));
    }

    let parsed: OpenAiModelsResponse = resp
        .json()
        .await
        .map_err(|e| AppError::Ai(format!("Failed to parse models response: {}", e)))?;

    let mut ids: Vec<String> = parsed.data.into_iter().map(|m| m.id).collect();
    ids.sort();
    Ok(ids)
}

#[derive(Deserialize)]
struct AnthropicModelsResponse {
    data: Vec<AnthropicModel>,
}

#[derive(Deserialize)]
struct AnthropicModel {
    id: String,
}

async fn fetch_anthropic_models(endpoint: &str, api_key: &str) -> Result<Vec<String>, AppError> {
    // Anthropic's models endpoint lives at `${endpoint}/v1/models` and requires
    // both x-api-key and anthropic-version headers.
    let base = endpoint.trim_end_matches('/');
    // The configured endpoint may already include `/v1`; handle both shapes.
    let url = if base.ends_with("/v1") {
        format!("{}/models", base)
    } else {
        format!("{}/v1/models", base)
    };

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| AppError::Ai(format!("HTTP client error: {}", e)))?;

    let resp = client
        .get(&url)
        .header("x-api-key", api_key)
        .header("anthropic-version", "2023-06-01")
        .send()
        .await
        .map_err(|e| AppError::Ai(format!("Failed to fetch models: {}", e)))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(AppError::Ai(format!(
            "Models endpoint returned {}: {}",
            status, body
        )));
    }

    let parsed: AnthropicModelsResponse = resp
        .json()
        .await
        .map_err(|e| AppError::Ai(format!("Failed to parse models response: {}", e)))?;

    let mut ids: Vec<String> = parsed.data.into_iter().map(|m| m.id).collect();
    ids.sort();
    Ok(ids)
}
