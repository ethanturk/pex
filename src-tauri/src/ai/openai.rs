use crate::ai::{AiProvider, ChatMessage};
use crate::AppError;
use serde::Serialize;

/// OpenAI-compatible provider.
/// Works with any endpoint that speaks the OpenAI chat completions API
/// (OpenAI, Azure OpenAI, Ollama, OpenRouter, vLLM, etc.).
pub struct OpenAiProvider {
    endpoint: String,
    model: String,
    api_key: String,
    http: reqwest::Client,
}

impl OpenAiProvider {
    pub fn new(endpoint: String, model: String, api_key: String) -> Self {
        Self {
            endpoint,
            model,
            api_key,
            http: reqwest::Client::new(),
        }
    }
}

#[async_trait::async_trait]
impl AiProvider for OpenAiProvider {
    async fn chat(&self, messages: &[ChatMessage]) -> Result<String, AppError> {
        let url = format!("{}/v1/chat/completions", self.endpoint.trim_end_matches('/'));

        let request_body = OpenAiChatRequest {
            model: &self.model,
            messages: messages
                .iter()
                .map(|m| OpenAiMessage {
                    role: m.role.as_str(),
                    content: &m.content,
                })
                .collect(),
        };

        let resp = self
            .http
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&request_body)
            .send()
            .await
            .map_err(|e| AppError::Ai(format!("OpenAI request failed: {}", e)))?;

        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();

        if !status.is_success() {
            // Try to extract error detail from response
            let detail = serde_json::from_str::<serde_json::Value>(&body)
                .ok()
                .and_then(|v| {
                    v.get("error")
                        .and_then(|e| e.get("message"))
                        .and_then(|m| m.as_str())
                        .map(|s| s.to_string())
                })
                .unwrap_or_else(|| body.clone());

            return Err(AppError::Ai(format!("OpenAI {} {}: {}", status.as_u16(), status.canonical_reason().unwrap_or(""), detail)));
        }

        let response: OpenAiChatResponse = serde_json::from_str(&body)
            .map_err(|e| AppError::Ai(format!("Failed to parse OpenAI response: {}", e)))?;

        let content = response
            .choices
            .into_iter()
            .next()
            .and_then(|c| c.message.content)
            .unwrap_or_default();

        Ok(content)
    }
}

#[derive(Serialize)]
struct OpenAiChatRequest<'a> {
    model: &'a str,
    messages: Vec<OpenAiMessage<'a>>,
}

#[derive(Serialize)]
struct OpenAiMessage<'a> {
    role: &'a str,
    content: &'a str,
}

#[derive(serde::Deserialize)]
struct OpenAiChatResponse {
    choices: Vec<OpenAiChoice>,
}

#[derive(serde::Deserialize)]
struct OpenAiChoice {
    message: OpenAiResponseMessage,
}

#[derive(serde::Deserialize)]
struct OpenAiResponseMessage {
    content: Option<String>,
}
