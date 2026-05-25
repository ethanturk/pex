pub mod anthropic;
pub mod openai;
pub mod prompts;

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
    /// Send a chat request and return the model's response text.
    async fn chat(&self, messages: &[ChatMessage]) -> Result<String, AppError>;
}

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
pub struct AiSettingsNoKey {
    pub provider: String,
    pub endpoint: String,
    pub model: String,
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
    ) {
        self.provider = Some(match kind {
            AiProviderKind::OpenAI => {
                let p = openai::OpenAiProvider::new(
                    endpoint.to_string(),
                    model.to_string(),
                    api_key.to_string(),
                );
                std::sync::Arc::new(p) as std::sync::Arc<dyn AiProvider>
            }
            AiProviderKind::Anthropic => {
                let p = anthropic::AnthropicProvider::new(
                    endpoint.to_string(),
                    model.to_string(),
                    api_key.to_string(),
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

        self.configure(kind, &endpoint, &model, &api_key);
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
