pub mod anthropic;
pub mod models;
pub mod openai;
pub mod prompts;
pub mod standards;

use crate::AppError;

/// AI provider kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AiProviderKind {
    OpenAI,
    Anthropic,
}

impl std::fmt::Display for AiProviderKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AiProviderKind::OpenAI => write!(f, "openai"),
            AiProviderKind::Anthropic => write!(f, "anthropic"),
        }
    }
}

impl std::str::FromStr for AiProviderKind {
    type Err = AppError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "openai" => Ok(AiProviderKind::OpenAI),
            "anthropic" => Ok(AiProviderKind::Anthropic),
            _ => Err(AppError::Ai(format!("Unknown provider: {}", s))),
        }
    }
}

/// A chat message for the AI provider.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ChatMessage {
    pub role: ChatRole,
    pub content: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ChatRole {
    System,
    User,
    Assistant,
}

impl ChatRole {
    pub fn as_str(&self) -> &str {
        match self {
            ChatRole::System => "system",
            ChatRole::User => "user",
            ChatRole::Assistant => "assistant",
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub arguments: serde_json::Value,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ToolChatMessage {
    Message(ChatMessage),
    AssistantToolCalls {
        content: Option<String>,
        tool_calls: Vec<ToolCall>,
    },
    ToolResult {
        tool_call_id: String,
        name: String,
        content: String,
    },
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ToolChatResponse {
    pub content: String,
    #[serde(default)]
    pub tool_calls: Vec<ToolCall>,
}

/// AI provider trait — implemented by OpenAI and Anthropic backends.
#[async_trait::async_trait]
pub trait AiProvider: Send + Sync {
    /// Send a chat request using the provider's configured model.
    async fn chat(&self, messages: &[ChatMessage]) -> Result<String, AppError> {
        self.chat_with_model(messages, None).await
    }

    /// Send a chat request, optionally overriding the model for this single call.
    /// `None` means "use the provider's configured model" — same behavior as `chat`.
    async fn chat_with_model(
        &self,
        messages: &[ChatMessage],
        model_override: Option<&str>,
    ) -> Result<String, AppError>;

    /// Send a tool-enabled chat request. Providers that do not support native
    /// tool calls return an error; callers should treat that as a signal to
    /// continue without the optional context layer.
    async fn chat_with_tools(
        &self,
        _messages: &[ToolChatMessage],
        _tools: &[ToolDefinition],
        _model_override: Option<&str>,
    ) -> Result<ToolChatResponse, AppError> {
        Err(AppError::Ai(
            "Tool calls are not supported by this provider".to_string(),
        ))
    }
}

/// Default request timeout in seconds when none is configured.
/// Kept only to preserve old call shapes during migration — no longer wired in.
pub const DEFAULT_REQUEST_TIMEOUT_SECS: u64 = 120;

/// Default TCP/TLS handshake budget. Catches dead servers quickly without
/// punishing slow generation.
pub const DEFAULT_CONNECT_TIMEOUT_SECS: u64 = 10;
/// Default per-read budget — the maximum time the client will wait between
/// successive bytes coming back from the server. A long inference is fine as
/// long as the server is sending *something*; this only fires on actual stalls.
/// Tighter is fine; looser is fine — pick the value that matches the network
/// you're working against, not the model's wall-clock latency.
pub const DEFAULT_READ_TIMEOUT_SECS: u64 = 60;
/// Hard ceiling for both connect and read so a user fat-fingering "999999"
/// doesn't accidentally disable cancellation entirely.
pub const MAX_TIMEOUT_SECS: u64 = 3600;

/// Default number of hunks that can be reviewed in parallel.
pub const DEFAULT_HUNK_CONCURRENCY: u32 = 1;
/// Hard cap to keep users from accidentally hammering the LLM.
pub const MAX_HUNK_CONCURRENCY: u32 = 16;

/// Default number of times the review engine retries a failed LLM call before
/// giving up on that hunk. 1 = one extra attempt after the first failure.
/// Set to 0 for local providers to avoid sending duplicate work to a slow
/// model that didn't really fail — it just hadn't finished yet.
pub const DEFAULT_RETRY_COUNT: u32 = 1;
/// Hard cap to keep retry counts from looping forever on persistent errors.
pub const MAX_RETRY_COUNT: u32 = 10;

/// Default per-file size cap (characters) for AGENTS.md / STYLE.md content
/// injected into Review prompts. Large enough for typical convention files,
/// small enough to leave room for the actual hunk.
pub const DEFAULT_STANDARDS_MAX_CHARS: u32 = 8000;
pub const MIN_STANDARDS_MAX_CHARS: u32 = 500;
pub const MAX_STANDARDS_MAX_CHARS: u32 = 65535;

/// Minimum confidence (0–100) a review finding must reach to be surfaced.
/// Mirrors the pr-review-toolkit's ≥80 reporting threshold: below this a
/// finding is treated as a likely false positive or low-impact nit and
/// dropped before the reviewer ever sees it. 0 surfaces everything.
pub const DEFAULT_CONFIDENCE_THRESHOLD: u8 = 80;
pub const MAX_CONFIDENCE_THRESHOLD: u8 = 100;

/// Confidence (0–100) at or above which a Critical finding is tiered as
/// Blocking rather than Should-fix — the "critical line." Configurable so teams
/// can decide how sure the reviewer must be before a critical issue gates the
/// PR. Critical findings below this line are still actionable (Should-fix).
pub const DEFAULT_BLOCKING_CONFIDENCE: u8 = 85;
pub const MAX_BLOCKING_CONFIDENCE: u8 = 100;

/// Hard ceiling (characters) on the surrounding-file context injected into
/// hunk reviews and the file adjudicator. Bounds token cost and latency on
/// large files while still letting reviewers see definitions / callers that
/// kill the most common false positives. Not user-configurable in Phase 1.
pub const FILE_CONTEXT_MAX_CHARS: usize = 12000;

/// Whether posting a review with at least one Blocking finding also casts a
/// "wait for author" reviewer vote. Off by default — auto-voting is a visible
/// side effect, so it is strictly opt-in.
pub const DEFAULT_AUTO_VOTE_ON_BLOCKING: bool = false;

/// ADO reviewer vote value for "wait for author". Used by the opt-in
/// auto-vote-on-blocking behavior. (10 approve, 5 approve w/ suggestions,
/// 0 none, -5 wait for author, -10 reject.)
pub const VOTE_WAIT_FOR_AUTHOR: i32 = -5;

/// AI settings stored in SQLite + keyring.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AiSettings {
    pub provider: String,
    pub endpoint: String,
    pub model: String,
    pub api_key: String,
}

/// One configured AI provider. API keys are stored separately in keyring.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiProviderConfig {
    pub id: String,
    pub name: String,
    pub provider: String,
    pub endpoint: String,
    pub model: String,
    pub connect_timeout_secs: u64,
    pub read_timeout_secs: u64,
}

/// One configured AI provider as returned to the UI. The key itself is never
/// returned; `has_api_key` only drives the masked input placeholder.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiProviderConfigNoKey {
    pub id: String,
    pub name: String,
    pub provider: String,
    pub endpoint: String,
    pub model: String,
    pub has_api_key: bool,
    pub connect_timeout_secs: u64,
    pub read_timeout_secs: u64,
}

/// Settings stored in SQLite (no API key).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiSettingsNoKey {
    pub default_provider_id: String,
    pub providers: Vec<AiProviderConfigNoKey>,
    pub provider: String,
    pub endpoint: String,
    pub model: String,
    /// Whether an API key is stored for the current provider. The key itself is
    /// never returned; the UI uses this to show a masked placeholder instead of
    /// a misleadingly-empty field.
    pub has_api_key: bool,
    /// TCP/TLS handshake budget in seconds.
    pub connect_timeout_secs: u64,
    /// Per-read stalled-stream budget in seconds. Does NOT bound total generation
    /// time — a slow model that keeps the connection alive will not be killed.
    pub read_timeout_secs: u64,
    pub hunk_concurrency: u32,
    pub standards_max_chars: u32,
    /// Number of retries the review engine performs after a failed LLM call.
    /// 0 = no retries (recommended for local providers).
    pub retry_count: u32,
    /// Minimum confidence (0–100) a finding must reach to be reported.
    pub confidence_threshold: u8,
    /// Confidence (0–100) at/above which a Critical finding is tiered Blocking
    /// (the "critical line").
    pub blocking_confidence: u8,
    /// Opt-in: cast a "wait for author" vote when posting a review that has at
    /// least one Blocking finding.
    pub auto_vote_on_blocking: bool,
    /// Opt-in: review only files changed since the last reviewed iteration.
    pub incremental_review: bool,
    /// Opt-in: auto-trigger a review on a new PR / iteration.
    pub auto_review: bool,
    /// Opt-in: after an auto-review, auto-post high-confidence Blocking findings.
    pub auto_post_blocking: bool,
    /// Confidence floor (0–100) for auto-posting a Blocking finding.
    pub auto_post_confidence: u8,
    /// Opt-in: write a JSONL diagnostic trace per review run.
    pub ai_diagnostics: bool,
}

pub const AI_PROVIDERS_SETTING: &str = "ai_providers";
pub const AI_DEFAULT_PROVIDER_ID_SETTING: &str = "ai_default_provider_id";

pub fn ai_provider_secret_key(provider_id: &str) -> String {
    format!("provider:{}", provider_id)
}

pub fn legacy_provider_secret_key(kind: AiProviderKind) -> &'static str {
    match kind {
        AiProviderKind::OpenAI => "openai",
        AiProviderKind::Anthropic => "anthropic",
    }
}

pub fn default_endpoint_for_kind(kind: AiProviderKind) -> &'static str {
    match kind {
        AiProviderKind::OpenAI => "https://api.openai.com",
        AiProviderKind::Anthropic => "https://api.anthropic.com",
    }
}

pub fn default_model_for_kind(kind: AiProviderKind) -> &'static str {
    match kind {
        AiProviderKind::OpenAI => "gpt-4.1",
        AiProviderKind::Anthropic => "claude-3-5-sonnet-latest",
    }
}

pub async fn read_ai_provider_configs(
    conn: &libsql::Connection,
) -> Result<(String, Vec<AiProviderConfig>), AppError> {
    let configured = match crate::cache::get_setting(conn, AI_PROVIDERS_SETTING).await? {
        Some(raw) if !raw.trim().is_empty() => {
            serde_json::from_str::<Vec<AiProviderConfig>>(&raw).unwrap_or_default()
        }
        _ => Vec::new(),
    };

    let mut providers = configured
        .into_iter()
        .filter(|p| !p.id.trim().is_empty())
        .map(|mut p| {
            p.name = p.name.trim().to_string();
            if p.name.is_empty() {
                p.name = "Provider".to_string();
            }
            p.provider = p.provider.trim().to_lowercase();
            p.endpoint = p.endpoint.trim().to_string();
            p.model = p.model.trim().to_string();
            p.connect_timeout_secs =
                clamp_timeout_value(p.connect_timeout_secs, DEFAULT_CONNECT_TIMEOUT_SECS);
            p.read_timeout_secs =
                clamp_timeout_value(p.read_timeout_secs, DEFAULT_READ_TIMEOUT_SECS);
            p
        })
        .collect::<Vec<_>>();

    if providers.is_empty() {
        let provider = crate::cache::get_setting(conn, "ai_provider")
            .await?
            .unwrap_or_else(|| "openai".to_string());
        let kind = provider.parse().unwrap_or(AiProviderKind::OpenAI);
        providers.push(AiProviderConfig {
            id: "default".to_string(),
            name: "Default".to_string(),
            provider: kind.to_string(),
            endpoint: crate::cache::get_setting(conn, "ai_endpoint")
                .await?
                .unwrap_or_else(|| default_endpoint_for_kind(kind).to_string()),
            model: crate::cache::get_setting(conn, "ai_model")
                .await?
                .unwrap_or_else(|| default_model_for_kind(kind).to_string()),
            connect_timeout_secs: read_connect_timeout(conn).await?,
            read_timeout_secs: read_read_timeout(conn).await?,
        });
    }

    let default_id = crate::cache::get_setting(conn, AI_DEFAULT_PROVIDER_ID_SETTING)
        .await?
        .filter(|id| providers.iter().any(|p| p.id == *id))
        .unwrap_or_else(|| providers[0].id.clone());

    Ok((default_id, providers))
}

pub async fn write_ai_provider_configs(
    conn: &libsql::Connection,
    default_provider_id: &str,
    providers: &[AiProviderConfig],
) -> Result<(), AppError> {
    let payload = serde_json::to_string(providers)
        .map_err(|e| AppError::Ai(format!("Failed to serialize AI providers: {}", e)))?;
    crate::cache::set_setting(conn, AI_PROVIDERS_SETTING, &payload).await?;
    crate::cache::set_setting(conn, AI_DEFAULT_PROVIDER_ID_SETTING, default_provider_id).await?;
    Ok(())
}

pub fn clamp_timeout_value(secs: u64, default: u64) -> u64 {
    if secs == 0 {
        default
    } else {
        secs.min(MAX_TIMEOUT_SECS)
    }
}

pub fn provider_from_config(
    provider: &AiProviderConfig,
    api_key: &str,
) -> Result<std::sync::Arc<dyn AiProvider>, AppError> {
    let kind: AiProviderKind = provider.provider.parse()?;
    Ok(match kind {
        AiProviderKind::OpenAI => {
            let p = openai::OpenAiProvider::new(
                provider.endpoint.clone(),
                provider.model.clone(),
                api_key.to_string(),
                provider.connect_timeout_secs,
                provider.read_timeout_secs,
            );
            std::sync::Arc::new(p) as std::sync::Arc<dyn AiProvider>
        }
        AiProviderKind::Anthropic => {
            let p = anthropic::AnthropicProvider::new(
                provider.endpoint.clone(),
                provider.model.clone(),
                api_key.to_string(),
                provider.connect_timeout_secs,
                provider.read_timeout_secs,
            );
            std::sync::Arc::new(p) as std::sync::Arc<dyn AiProvider>
        }
    })
}

pub fn read_ai_provider_api_key(provider: &AiProviderConfig) -> Result<Option<String>, AppError> {
    let provider_key = ai_provider_secret_key(&provider.id);
    let mut api_key = crate::auth::keyring_store::KeyringStore::get_ai_token(&provider_key)?;
    if api_key.is_none() && provider.id == "default" {
        let kind: AiProviderKind = provider.provider.parse()?;
        api_key = crate::auth::keyring_store::KeyringStore::get_ai_token(
            legacy_provider_secret_key(kind),
        )?;
    }
    Ok(api_key.filter(|key| !key.is_empty()))
}

/// Read the TCP/TLS connect timeout (seconds), defaulting if missing.
/// Treats 0 as "use default."
pub async fn read_connect_timeout(conn: &libsql::Connection) -> Result<u64, AppError> {
    let raw = crate::cache::get_setting(conn, "ai_connect_timeout_secs").await?;
    Ok(raw
        .and_then(|s| s.parse::<u64>().ok())
        .filter(|n| *n > 0)
        .map(|n| n.min(MAX_TIMEOUT_SECS))
        .unwrap_or(DEFAULT_CONNECT_TIMEOUT_SECS))
}

/// Read the per-read (stalled-stream) timeout (seconds), defaulting if missing.
/// Treats 0 as "use default."
///
/// Backward compatibility: if `ai_read_timeout_secs` is unset, falls back to
/// the legacy `ai_request_timeout_secs` value so existing users don't lose
/// their tuning. This fallback is harmless because the legacy value was
/// already a total-request timeout — interpreting it as a read timeout is
/// strictly more lenient (slow generation now succeeds where it used to fail).
pub async fn read_read_timeout(conn: &libsql::Connection) -> Result<u64, AppError> {
    let new_key = crate::cache::get_setting(conn, "ai_read_timeout_secs").await?;
    if let Some(n) = new_key
        .and_then(|s| s.parse::<u64>().ok())
        .filter(|n| *n > 0)
    {
        return Ok(n.min(MAX_TIMEOUT_SECS));
    }
    let legacy = crate::cache::get_setting(conn, "ai_request_timeout_secs").await?;
    Ok(legacy
        .and_then(|s| s.parse::<u64>().ok())
        .filter(|n| *n > 0)
        .map(|n| n.min(MAX_TIMEOUT_SECS))
        .unwrap_or(DEFAULT_READ_TIMEOUT_SECS))
}

/// Read the configured hunk concurrency (max parallel hunk reviews), clamped to a sane range.
pub async fn read_hunk_concurrency(conn: &libsql::Connection) -> Result<u32, AppError> {
    let raw = crate::cache::get_setting(conn, "ai_hunk_concurrency").await?;
    Ok(raw
        .and_then(|s| s.parse::<u32>().ok())
        .filter(|n| *n >= 1)
        .map(|n| n.min(MAX_HUNK_CONCURRENCY))
        .unwrap_or(DEFAULT_HUNK_CONCURRENCY))
}

/// Read the configured retry count for failed LLM calls during a PR review.
/// 0 means "do not retry" — useful for local providers where a "failure" is
/// often just a slow generation that's still in flight.
pub async fn read_retry_count(conn: &libsql::Connection) -> Result<u32, AppError> {
    let raw = crate::cache::get_setting(conn, "ai_retry_count").await?;
    Ok(raw
        .and_then(|s| s.parse::<u32>().ok())
        .map(|n| n.min(MAX_RETRY_COUNT))
        .unwrap_or(DEFAULT_RETRY_COUNT))
}

/// Read the configured minimum confidence threshold (0–100) for surfacing
/// findings. Unlike most numeric settings, 0 is a valid, meaningful value
/// ("surface everything"), so it is not coerced to the default.
pub async fn read_confidence_threshold(conn: &libsql::Connection) -> Result<u8, AppError> {
    let raw = crate::cache::get_setting(conn, "ai_confidence_threshold").await?;
    Ok(raw
        .and_then(|s| s.parse::<u8>().ok())
        .map(|n| n.min(MAX_CONFIDENCE_THRESHOLD))
        .unwrap_or(DEFAULT_CONFIDENCE_THRESHOLD))
}

/// Read the "critical line": the confidence at/above which a Critical finding
/// is tiered Blocking. 0 means every Critical finding blocks; just clamp the
/// upper bound.
pub async fn read_blocking_confidence(conn: &libsql::Connection) -> Result<u8, AppError> {
    let raw = crate::cache::get_setting(conn, "ai_blocking_confidence").await?;
    Ok(raw
        .and_then(|s| s.parse::<u8>().ok())
        .map(|n| n.min(MAX_BLOCKING_CONFIDENCE))
        .unwrap_or(DEFAULT_BLOCKING_CONFIDENCE))
}

/// Read whether posting a review should auto-cast a "wait for author" vote when
/// there is at least one Blocking finding. Defaults to off.
pub async fn read_auto_vote_on_blocking(conn: &libsql::Connection) -> Result<bool, AppError> {
    let raw = crate::cache::get_setting(conn, "ai_auto_vote_on_blocking").await?;
    Ok(raw
        .map(|s| s == "true")
        .unwrap_or(DEFAULT_AUTO_VOTE_ON_BLOCKING))
}

/// Whether reviews are incremental: on a re-review, only files changed since the
/// last reviewed iteration are reviewed. Defaults to off (always full review).
pub const DEFAULT_INCREMENTAL_REVIEW: bool = false;

/// Read whether incremental review is enabled.
pub async fn read_incremental_review(conn: &libsql::Connection) -> Result<bool, AppError> {
    let raw = crate::cache::get_setting(conn, "ai_incremental_review").await?;
    Ok(raw
        .map(|s| s == "true")
        .unwrap_or(DEFAULT_INCREMENTAL_REVIEW))
}

/// Write a JSONL diagnostic trace per review run (prompts, responses, and every
/// deterministic decision) for evaluation/tuning. Off by default — traces
/// contain source content and full prompts.
pub const DEFAULT_AI_DIAGNOSTICS: bool = false;

pub async fn read_ai_diagnostics(conn: &libsql::Connection) -> Result<bool, AppError> {
    let raw = crate::cache::get_setting(conn, "ai_diagnostics").await?;
    Ok(raw.map(|s| s == "true").unwrap_or(DEFAULT_AI_DIAGNOSTICS))
}

// ---- Phase 4: automation (earned autonomy) ----

/// Auto-trigger a review when a PR is first seen or has a new iteration.
/// Off by default — auto-review consumes provider quota.
pub const DEFAULT_AUTO_REVIEW: bool = false;

/// After an auto-review, auto-post the highest-confidence Blocking findings.
/// Off by default — this posts comments without a human in the loop.
pub const DEFAULT_AUTO_POST_BLOCKING: bool = false;

/// Confidence (0–100) a Blocking finding must reach to be auto-posted. Set high
/// by default: autonomy is earned, so only near-certain blockers post unattended.
pub const DEFAULT_AUTO_POST_CONFIDENCE: u8 = 90;
pub const MAX_AUTO_POST_CONFIDENCE: u8 = 100;

pub async fn read_auto_review(conn: &libsql::Connection) -> Result<bool, AppError> {
    let raw = crate::cache::get_setting(conn, "ai_auto_review").await?;
    Ok(raw.map(|s| s == "true").unwrap_or(DEFAULT_AUTO_REVIEW))
}

pub async fn read_auto_post_blocking(conn: &libsql::Connection) -> Result<bool, AppError> {
    let raw = crate::cache::get_setting(conn, "ai_auto_post_blocking").await?;
    Ok(raw
        .map(|s| s == "true")
        .unwrap_or(DEFAULT_AUTO_POST_BLOCKING))
}

pub async fn read_auto_post_confidence(conn: &libsql::Connection) -> Result<u8, AppError> {
    let raw = crate::cache::get_setting(conn, "ai_auto_post_confidence").await?;
    Ok(raw
        .and_then(|s| s.parse::<u8>().ok())
        .map(|n| n.min(MAX_AUTO_POST_CONFIDENCE))
        .unwrap_or(DEFAULT_AUTO_POST_CONFIDENCE))
}

/// Read the configured per-file size cap for injected AGENTS.md / STYLE.md content.
pub async fn read_standards_max_chars(conn: &libsql::Connection) -> Result<u32, AppError> {
    let raw = crate::cache::get_setting(conn, "ai_standards_max_chars").await?;
    Ok(raw
        .and_then(|s| s.parse::<u32>().ok())
        .filter(|n| *n >= MIN_STANDARDS_MAX_CHARS)
        .map(|n| n.min(MAX_STANDARDS_MAX_CHARS))
        .unwrap_or(DEFAULT_STANDARDS_MAX_CHARS))
}

/// Manages the active AI provider, constructed lazily from stored settings.
pub struct AiManager {
    provider: Option<std::sync::Arc<dyn AiProvider>>,
}

impl AiManager {
    pub fn new() -> Self {
        Self { provider: None }
    }

    /// Build or rebuild the provider from settings.
    pub fn configure(
        &mut self,
        kind: AiProviderKind,
        endpoint: &str,
        model: &str,
        api_key: &str,
        connect_timeout_secs: u64,
        read_timeout_secs: u64,
    ) {
        self.provider = Some(match kind {
            AiProviderKind::OpenAI => {
                let p = openai::OpenAiProvider::new(
                    endpoint.to_string(),
                    model.to_string(),
                    api_key.to_string(),
                    connect_timeout_secs,
                    read_timeout_secs,
                );
                std::sync::Arc::new(p) as std::sync::Arc<dyn AiProvider>
            }
            AiProviderKind::Anthropic => {
                let p = anthropic::AnthropicProvider::new(
                    endpoint.to_string(),
                    model.to_string(),
                    api_key.to_string(),
                    connect_timeout_secs,
                    read_timeout_secs,
                );
                std::sync::Arc::new(p) as std::sync::Arc<dyn AiProvider>
            }
        });
    }

    /// Try to auto-configure from stored settings in SQLite + keyring.
    pub async fn try_configure_from_db(
        &mut self,
        conn: &libsql::Connection,
    ) -> Result<bool, AppError> {
        let (default_id, providers) = read_ai_provider_configs(conn).await?;
        let Some(default_provider) = providers.iter().find(|p| p.id == default_id) else {
            return Ok(false);
        };

        let api_key = read_ai_provider_api_key(default_provider)?;
        let Some(api_key) = api_key else {
            return Ok(false);
        };

        self.provider = Some(provider_from_config(default_provider, &api_key)?);
        Ok(true)
    }

    pub async fn chat(&self, messages: &[ChatMessage]) -> Result<String, AppError> {
        match &self.provider {
            Some(p) => p.chat(messages).await,
            None => Err(AppError::Ai(
                "AI not configured. Set up AI settings first.".into(),
            )),
        }
    }

    pub fn is_configured(&self) -> bool {
        self.provider.is_some()
    }

    /// Clone the inner provider Arc for use across await points.
    pub fn provider_clone(&self) -> Option<std::sync::Arc<dyn AiProvider>> {
        self.provider.clone()
    }
}
