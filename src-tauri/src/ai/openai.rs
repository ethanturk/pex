use crate::ai::{AiProvider, ChatMessage};
use crate::AppError;
use serde::{Deserialize, Serialize};

/// OpenAI-compatible provider — hits any endpoint that speaks the OpenAI chat
/// completions API (OpenAI, Azure OpenAI, Ollama, OpenRouter, vLLM, LM Studio).
///
/// Uses a direct `reqwest` client (rather than `async-openai`) so we can wire
/// separate `connect_timeout` and `read_timeout`. The previous async-openai
/// integration only exposed a total-request timeout — that killed long but
/// healthy generations from slow local models, while the model kept burning
/// cycles on the dropped request.
pub struct OpenAiProvider {
    endpoint: String,
    model: String,
    api_key: String,
    http: reqwest::Client,
}

impl OpenAiProvider {
    /// `connect_timeout_secs` bounds the TCP/TLS handshake.
    /// `read_timeout_secs` bounds the time between successive bytes from the
    /// server — it does NOT bound total wall-clock generation time.
    pub fn new(
        endpoint: String,
        model: String,
        api_key: String,
        connect_timeout_secs: u64,
        read_timeout_secs: u64,
    ) -> Self {
        let http = reqwest::Client::builder()
            .connect_timeout(std::time::Duration::from_secs(connect_timeout_secs))
            .read_timeout(std::time::Duration::from_secs(read_timeout_secs))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        Self {
            endpoint,
            model,
            api_key,
            http,
        }
    }
}

#[derive(Serialize)]
struct OpenAiRequest<'a> {
    model: &'a str,
    messages: Vec<OpenAiMessage<'a>>,
}

#[derive(Serialize)]
struct OpenAiMessage<'a> {
    role: &'a str,
    content: &'a str,
}

#[derive(Deserialize)]
struct OpenAiResponse {
    choices: Vec<OpenAiChoice>,
}

#[derive(Deserialize)]
struct OpenAiChoice {
    message: OpenAiResponseMessage,
}

#[derive(Deserialize)]
struct OpenAiResponseMessage {
    #[serde(default)]
    content: String,
}

#[async_trait::async_trait]
impl AiProvider for OpenAiProvider {
    async fn chat_with_model(
        &self,
        messages: &[ChatMessage],
        model_override: Option<&str>,
    ) -> Result<String, AppError> {
        let openai_messages: Vec<OpenAiMessage> = messages
            .iter()
            .map(|m| OpenAiMessage {
                role: match m.role {
                    crate::ai::ChatRole::System => "system",
                    crate::ai::ChatRole::User => "user",
                    crate::ai::ChatRole::Assistant => "assistant",
                },
                content: &m.content,
            })
            .collect();

        let model = model_override.unwrap_or(&self.model);
        let body = OpenAiRequest {
            model,
            messages: openai_messages,
        };

        let url = format!("{}/chat/completions", self.endpoint.trim_end_matches('/'));
        let response = self
            .http
            .post(&url)
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .await
            .map_err(|e| AppError::Ai(format!("OpenAI request failed: {}", e)))?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(AppError::Ai(format!(
                "OpenAI request returned {}: {}",
                status, text
            )));
        }

        let parsed: OpenAiResponse = response
            .json()
            .await
            .map_err(|e| AppError::Ai(format!("Failed to parse OpenAI response: {}", e)))?;

        let content = parsed
            .choices
            .into_iter()
            .next()
            .map(|c| c.message.content)
            .unwrap_or_default();

        Ok(content)
    }
}
