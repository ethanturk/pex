//! Fetch + cache the list of models available from the user's configured AI
//! provider. Used by the prompt-customization UI to populate per-specialist
//! model dropdowns.
//!
//! Cache: a single SQLite row keyed `ai_models_cache` holding JSON
//! `{ provider_id, provider, endpoint, models }`. The cache is invalidated
//! implicitly when the selected default provider, provider kind, or endpoint
//! changes.

use crate::ai::AiProviderKind;
use crate::AppError;
use serde::{Deserialize, Serialize};

const CACHE_KEY: &str = "ai_models_cache";

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CachedModels {
    #[serde(default)]
    provider_id: String,
    provider: String,
    endpoint: String,
    models: Vec<String>,
}

/// Return the cached models for the currently-configured provider+endpoint,
/// or `None` if there is no cache, the cache is stale (different provider /
/// endpoint), or provider settings aren't configured.
pub async fn get_cached(conn: &libsql::Connection) -> Result<Option<Vec<String>>, AppError> {
    let (default_provider_id, providers) = crate::ai::read_ai_provider_configs(conn).await?;
    let Some(default_provider) = providers.iter().find(|p| p.id == default_provider_id) else {
        return Ok(None);
    };
    let raw = crate::cache::get_setting(conn, CACHE_KEY).await?;

    let Some(raw) = raw else {
        return Ok(None);
    };

    let cached: CachedModels = match serde_json::from_str(&raw) {
        Ok(c) => c,
        // Stale schema — drop it so we re-fetch fresh next call.
        Err(_) => {
            let _ = crate::cache::delete_setting(conn, CACHE_KEY).await;
            return Ok(None);
        }
    };

    let provider_id_matches =
        cached.provider_id.is_empty() || cached.provider_id == default_provider.id;
    if !provider_id_matches
        || cached.provider != default_provider.provider
        || cached.endpoint != default_provider.endpoint
    {
        return Ok(None);
    }

    Ok(Some(cached.models))
}

/// Fetch the live model list from the provider's /models endpoint and persist
/// it to the cache. Requires AI settings to be fully configured.
pub async fn fetch_and_cache(conn: &libsql::Connection) -> Result<Vec<String>, AppError> {
    // libsql connections are internally synchronized and cheap to clone, so
    // there's no exclusive lock to hold across the network call — read the
    // settings, do the fetch, then persist.
    let (provider_id, kind, endpoint, api_key) = {
        let (default_provider_id, providers) = crate::ai::read_ai_provider_configs(conn).await?;
        let provider = providers
            .iter()
            .find(|p| p.id == default_provider_id)
            .ok_or_else(|| AppError::Ai("AI provider not configured.".to_string()))?;
        let kind: AiProviderKind = provider.provider.parse()?;
        let provider_key = crate::ai::ai_provider_secret_key(&provider.id);
        let mut api_key = crate::auth::keyring_store::KeyringStore::get_ai_token(&provider_key)?;
        if api_key.is_none() && provider.id == "default" {
            api_key = crate::auth::keyring_store::KeyringStore::get_ai_token(
                crate::ai::legacy_provider_secret_key(kind),
            )?;
        }
        let api_key = api_key.ok_or_else(|| AppError::Ai("API key not configured.".to_string()))?;
        (
            provider.id.clone(),
            kind,
            provider.endpoint.clone(),
            api_key,
        )
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
        provider_id,
        provider: provider_str.to_string(),
        endpoint,
        models: models.clone(),
    })
    .map_err(|e| AppError::Ai(format!("Failed to serialize models cache: {}", e)))?;
    crate::cache::set_setting(conn, CACHE_KEY, &payload).await?;

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
