use crate::ai::{
    AiProvider, ChatMessage, ChatRole, ToolCall, ToolChatMessage, ToolChatResponse, ToolDefinition,
};
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
        let url = format!("{}/v1/messages", self.endpoint.trim_end_matches('/'));

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

    async fn chat_with_tools(
        &self,
        messages: &[ToolChatMessage],
        tools: &[ToolDefinition],
        model_override: Option<&str>,
    ) -> Result<ToolChatResponse, AppError> {
        let url = format!("{}/v1/messages", self.endpoint.trim_end_matches('/'));

        let system = messages
            .iter()
            .filter_map(|message| match message {
                ToolChatMessage::Message(m) if m.role == ChatRole::System => {
                    Some(m.content.as_str())
                }
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n\n");

        let anthropic_messages: Vec<AnthropicToolMessage> =
            messages.iter().filter_map(anthropic_tool_message).collect();

        let mut request_body = AnthropicToolRequest {
            model: model_override.unwrap_or(&self.model).to_string(),
            max_tokens: 4096,
            messages: anthropic_messages,
            tools: tools
                .iter()
                .map(|tool| AnthropicToolDefinition {
                    name: tool.name.clone(),
                    description: tool.description.clone(),
                    input_schema: tool.parameters.clone(),
                })
                .collect(),
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
            .map_err(|e| AppError::Ai(format!("Anthropic tool request failed: {}", e)))?;

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
                "Anthropic tool request {} {}: {}",
                status.as_u16(),
                status.canonical_reason().unwrap_or(""),
                detail
            )));
        }

        let response: AnthropicResponse = serde_json::from_str(&body)
            .map_err(|e| AppError::Ai(format!("Failed to parse Anthropic tool response: {}", e)))?;

        let mut content = String::new();
        let mut tool_calls = Vec::new();
        for block in response.content {
            match block {
                AnthropicContentBlock::Text { text } => content.push_str(&text),
                AnthropicContentBlock::ToolUse { id, name, input } => {
                    tool_calls.push(ToolCall {
                        id,
                        name,
                        arguments: input,
                    });
                }
                AnthropicContentBlock::Other => {}
            }
        }

        Ok(ToolChatResponse {
            content,
            tool_calls,
        })
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

#[derive(Serialize)]
struct AnthropicToolRequest {
    model: String,
    max_tokens: u32,
    messages: Vec<AnthropicToolMessage>,
    tools: Vec<AnthropicToolDefinition>,
    #[serde(skip_serializing_if = "Option::is_none")]
    system: Option<AnthropicSystemContent>,
}

#[derive(Serialize)]
struct AnthropicToolDefinition {
    name: String,
    description: String,
    input_schema: serde_json::Value,
}

#[derive(Serialize)]
struct AnthropicToolMessage {
    role: String,
    content: Vec<AnthropicToolContentBlock>,
}

#[derive(Serialize)]
#[serde(tag = "type")]
enum AnthropicToolContentBlock {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "tool_use")]
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },
    #[serde(rename = "tool_result")]
    ToolResult {
        tool_use_id: String,
        content: String,
    },
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
    #[serde(rename = "tool_use")]
    ToolUse {
        id: String,
        name: String,
        #[serde(default)]
        input: serde_json::Value,
    },
    #[serde(other)]
    Other,
}

fn anthropic_tool_message(message: &ToolChatMessage) -> Option<AnthropicToolMessage> {
    match message {
        ToolChatMessage::Message(chat) if chat.role == ChatRole::System => None,
        ToolChatMessage::Message(chat) => Some(AnthropicToolMessage {
            role: match chat.role {
                ChatRole::User | ChatRole::System => "user",
                ChatRole::Assistant => "assistant",
            }
            .to_string(),
            content: vec![AnthropicToolContentBlock::Text {
                text: chat.content.clone(),
            }],
        }),
        ToolChatMessage::AssistantToolCalls {
            content,
            tool_calls,
        } => {
            let mut blocks = Vec::new();
            if let Some(content) = content.as_ref().filter(|c| !c.trim().is_empty()) {
                blocks.push(AnthropicToolContentBlock::Text {
                    text: content.clone(),
                });
            }
            blocks.extend(
                tool_calls
                    .iter()
                    .map(|call| AnthropicToolContentBlock::ToolUse {
                        id: call.id.clone(),
                        name: call.name.clone(),
                        input: call.arguments.clone(),
                    }),
            );
            Some(AnthropicToolMessage {
                role: "assistant".to_string(),
                content: blocks,
            })
        }
        ToolChatMessage::ToolResult {
            tool_call_id,
            content,
            ..
        } => Some(AnthropicToolMessage {
            role: "user".to_string(),
            content: vec![AnthropicToolContentBlock::ToolResult {
                tool_use_id: tool_call_id.clone(),
                content: content.clone(),
            }],
        }),
    }
}
