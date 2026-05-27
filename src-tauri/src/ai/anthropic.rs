use crate::ai::{AiProvider, ChatMessage, ChatRole};
use crate::AppError;
use serde::Serialize;

/// Anthropic-compatible provider.
/// Works with Anthropic's Messages API and compatible endpoints.
pub struct AnthropicProvider {
    endpoint: String,
    model: String,
    api_key: String,
    http: reqwest::Client,
}

impl AnthropicProvider {
    /// `connect_timeout_secs` bounds the TCP/TLS handshake.
    /// `read_timeout_secs` bounds the time between successive bytes from the
    /// server — it does NOT bound total wall-clock generation time, so a slow
    /// model that keeps the stream alive will be allowed to finish.
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

#[async_trait::async_trait]
impl AiProvider for AnthropicProvider {
    async fn chat_with_model(
        &self,
        messages: &[ChatMessage],
        model_override: Option<&str>,
    ) -> Result<String, AppError> {
        let url = format!(
            "{}/v1/messages",
            self.endpoint.trim_end_matches('/')
        );

        // Anthropic API separates system message from the messages array.
        // Our ChatMessage uses ChatRole::System — extract it.
        let system = messages
            .iter()
            .filter(|m| m.role == ChatRole::System)
            .map(|m| m.content.as_str())
            .collect::<Vec<_>>()
            .join("\n\n");

        let anthropic_messages: Vec<AnthropicMessage> = messages
            .iter()
            .filter(|m| m.role != ChatRole::System)
            .map(|m| AnthropicMessage {
                role: match m.role {
                    ChatRole::User => "user",
                    ChatRole::Assistant => "assistant",
                    ChatRole::System => "user", // fallback, should not happen due to filter
                },
                content: AnthropicContent::Text(AnthropicTextContent {
                    content_type: "text",
                    text: &m.content,
                }),
            })
            .collect();

        let model = model_override.unwrap_or(&self.model);
        let mut request_body = AnthropicRequest {
            model,
            max_tokens: 4096,
            messages: anthropic_messages,
            system: None,
        };

        if !system.is_empty() {
            request_body.system = Some(AnthropicSystemContent::Text(system));
        }

        let resp = self
            .http
            .post(&url)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("Content-Type", "application/json")
            .json(&request_body)
            .send()
            .await
            .map_err(|e| AppError::Ai(format!("Anthropic request failed: {}", e)))?;

        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();

        if !status.is_success() {
            let detail = serde_json::from_str::<serde_json::Value>(&body)
                .ok()
                .and_then(|v| {
                    v.get("error")
                        .and_then(|e| e.get("message"))
                        .and_then(|m| m.as_str())
                        .map(|s| s.to_string())
                })
                .unwrap_or_else(|| body.clone());

            return Err(AppError::Ai(format!(
                "Anthropic {} {}: {}",
                status.as_u16(),
                status.canonical_reason().unwrap_or(""),
                detail
            )));
        }

        let response: AnthropicResponse = serde_json::from_str(&body)
            .map_err(|e| AppError::Ai(format!("Failed to parse Anthropic response: {}", e)))?;

        let content = response
            .content
            .into_iter()
            .filter_map(|block| match block {
                AnthropicContentBlock::Text { text } => Some(text),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("");

        Ok(content)
    }
}

#[derive(Serialize)]
struct AnthropicRequest<'a> {
    model: &'a str,
    max_tokens: u32,
    messages: Vec<AnthropicMessage<'a>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    system: Option<AnthropicSystemContent>,
}

#[derive(Serialize)]
struct AnthropicMessage<'a> {
    role: &'a str,
    content: AnthropicContent<'a>,
}

#[derive(Serialize)]
#[serde(untagged)]
enum AnthropicContent<'a> {
    Text(AnthropicTextContent<'a>),
}

#[derive(Serialize)]
struct AnthropicTextContent<'a> {
    #[serde(rename = "type")]
    content_type: &'a str,
    text: &'a str,
}

#[derive(Serialize)]
#[serde(untagged)]
enum AnthropicSystemContent {
    Text(String),
}

#[derive(serde::Deserialize)]
struct AnthropicResponse {
    content: Vec<AnthropicContentBlock>,
}

#[derive(serde::Deserialize)]
#[serde(tag = "type")]
enum AnthropicContentBlock {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(other)]
    Other,
}
