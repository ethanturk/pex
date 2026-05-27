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
}

/// Default request timeout in seconds when none is configured.
pub const DEFAULT_REQUEST_TIMEOUT_SECS: u64 = 120;

/// Default number of hunks that can be reviewed in parallel.
pub const DEFAULT_HUNK_CONCURRENCY: u32 = 1;
/// Hard cap to keep users from accidentally hammering the LLM.
pub const MAX_HUNK_CONCURRENCY: u32 = 16;

/// Default per-file size cap (characters) for AGENTS.md / STYLE.md content
/// injected into Review prompts. Large enough for typical convention files,
/// small enough to leave room for the actual hunk.
pub const DEFAULT_STANDARDS_MAX_CHARS: u32 = 8000;
pub const MIN_STANDARDS_MAX_CHARS: u32 = 500;
pub const MAX_STANDARDS_MAX_CHARS: u32 = 65535;

/// AI settings stored in SQLite + keyring.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AiSettings {
    pub provider: String,
    pub endpoint: String,
    pub model: String,
    pub api_key: String,
}

/// Settings stored in SQLite (no API key).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiSettingsNoKey {
    pub provider: String,
    pub endpoint: String,
    pub model: String,
    pub request_timeout_secs: u64,
    pub hunk_concurrency: u32,
    pub standards_max_chars: u32,
}

/// Read the configured request timeout (seconds) from SQLite, falling back to the default
/// if missing or unparseable. Treats 0 as "use default" rather than "no timeout".
pub fn read_request_timeout(conn: &rusqlite::Connection) -> Result<u64, AppError> {
    let raw = crate::cache::get_setting(conn, "ai_request_timeout_secs")?;
    Ok(raw
        .and_then(|s| s.parse::<u64>().ok())
        .filter(|n| *n > 0)
        .unwrap_or(DEFAULT_REQUEST_TIMEOUT_SECS))
}

/// Read the configured hunk concurrency (max parallel hunk reviews), clamped to a sane range.
pub fn read_hunk_concurrency(conn: &rusqlite::Connection) -> Result<u32, AppError> {
    let raw = crate::cache::get_setting(conn, "ai_hunk_concurrency")?;
    Ok(raw
        .and_then(|s| s.parse::<u32>().ok())
        .filter(|n| *n >= 1)
        .map(|n| n.min(MAX_HUNK_CONCURRENCY))
        .unwrap_or(DEFAULT_HUNK_CONCURRENCY))
}

/// Read the configured per-file size cap for injected AGENTS.md / STYLE.md content.
pub fn read_standards_max_chars(conn: &rusqlite::Connection) -> Result<u32, AppError> {
    let raw = crate::cache::get_setting(conn, "ai_standards_max_chars")?;
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
        request_timeout_secs: u64,
    ) {
        self.provider = Some(match kind {
            AiProviderKind::OpenAI => {
                let p = openai::OpenAiProvider::new(
                    endpoint.to_string(),
                    model.to_string(),
                    api_key.to_string(),
                    request_timeout_secs,
                );
                std::sync::Arc::new(p) as std::sync::Arc<dyn AiProvider>
            }
            AiProviderKind::Anthropic => {
                let p = anthropic::AnthropicProvider::new(
                    endpoint.to_string(),
                    model.to_string(),
                    api_key.to_string(),
                    request_timeout_secs,
                );
                std::sync::Arc::new(p) as std::sync::Arc<dyn AiProvider>
            }
        });
    }

    /// Try to auto-configure from stored settings in SQLite + keyring.
    pub fn try_configure_from_db(
        &mut self,
        conn: &rusqlite::Connection,
    ) -> Result<bool, AppError> {
        let provider_str = crate::cache::get_setting(conn, "ai_provider")?;
        let endpoint = crate::cache::get_setting(conn, "ai_endpoint")?;
        let model = crate::cache::get_setting(conn, "ai_model")?;
        let timeout = read_request_timeout(conn)?;

        let (Some(provider_str), Some(endpoint), Some(model)) = (provider_str, endpoint, model) else {
            return Ok(false);
        };

        let kind: AiProviderKind = provider_str.parse()?;
        let api_key = match kind {
            AiProviderKind::OpenAI => {
                crate::auth::keyring_store::KeyringStore::get_token("pex-ai-openai")?
            }
            AiProviderKind::Anthropic => {
                crate::auth::keyring_store::KeyringStore::get_token("pex-ai-anthropic")?
            }
        };

        let Some(api_key) = api_key else {
            return Ok(false);
        };

        self.configure(kind, &endpoint, &model, &api_key, timeout);
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
