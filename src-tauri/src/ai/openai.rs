use crate::ai::{
    AiProvider, ChatMessage, ChatResponse, TokenUsage, ToolCall, ToolChatMessage, ToolChatResponse,
    ToolDefinition,
};
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
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
}

#[derive(Serialize)]
struct OpenAiMessage<'a> {
    role: &'a str,
    content: &'a str,
}

#[derive(Deserialize)]
struct OpenAiResponse {
    choices: Vec<OpenAiChoice>,
    #[serde(default)]
    usage: Option<OpenAiUsage>,
}

/// OpenAI-compatible usage block. Optional because some local servers omit it.
#[derive(Deserialize)]
struct OpenAiUsage {
    #[serde(default)]
    prompt_tokens: u32,
    #[serde(default)]
    completion_tokens: u32,
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

#[derive(Serialize)]
struct OpenAiToolRequest {
    model: String,
    messages: Vec<OpenAiToolMessage>,
    tools: Vec<OpenAiToolDefinition>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
}

#[derive(Serialize)]
struct OpenAiToolMessage {
    role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_calls: Option<Vec<OpenAiOutboundToolCall>>,
}

#[derive(Serialize)]
struct OpenAiOutboundToolCall {
    id: String,
    #[serde(rename = "type")]
    call_type: &'static str,
    function: OpenAiOutboundToolFunction,
}

#[derive(Serialize)]
struct OpenAiOutboundToolFunction {
    name: String,
    arguments: String,
}

#[derive(Serialize)]
struct OpenAiToolDefinition {
    #[serde(rename = "type")]
    tool_type: &'static str,
    function: OpenAiFunctionDefinition,
}

#[derive(Serialize)]
struct OpenAiFunctionDefinition {
    name: String,
    description: String,
    parameters: serde_json::Value,
}

#[derive(Deserialize)]
struct OpenAiToolResponse {
    choices: Vec<OpenAiToolChoice>,
}

#[derive(Deserialize)]
struct OpenAiToolChoice {
    message: OpenAiToolResponseMessage,
}

#[derive(Deserialize)]
struct OpenAiToolResponseMessage {
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    tool_calls: Vec<OpenAiInboundToolCall>,
}

#[derive(Deserialize)]
struct OpenAiInboundToolCall {
    id: String,
    function: OpenAiInboundToolFunction,
}

#[derive(Deserialize)]
struct OpenAiInboundToolFunction {
    name: String,
    #[serde(default)]
    arguments: String,
}

#[async_trait::async_trait]
impl AiProvider for OpenAiProvider {
    async fn chat_full(
        &self,
        messages: &[ChatMessage],
        model_override: Option<&str>,
        max_tokens: Option<u32>,
    ) -> Result<ChatResponse, AppError> {
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
            max_tokens,
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

        let usage = parsed.usage.as_ref().map(|u| TokenUsage {
            input_tokens: u.prompt_tokens,
            output_tokens: u.completion_tokens,
        });
        let content = parsed
            .choices
            .into_iter()
            .next()
            .map(|c| c.message.content)
            .unwrap_or_default();

        Ok(ChatResponse { content, usage })
    }

    async fn chat_with_tools(
        &self,
        messages: &[ToolChatMessage],
        tools: &[ToolDefinition],
        model_override: Option<&str>,
        max_tokens: Option<u32>,
    ) -> Result<ToolChatResponse, AppError> {
        let body = OpenAiToolRequest {
            model: model_override.unwrap_or(&self.model).to_string(),
            messages: messages.iter().map(openai_tool_message).collect(),
            tools: tools
                .iter()
                .map(|tool| OpenAiToolDefinition {
                    tool_type: "function",
                    function: OpenAiFunctionDefinition {
                        name: tool.name.clone(),
                        description: tool.description.clone(),
                        parameters: tool.parameters.clone(),
                    },
                })
                .collect(),
            max_tokens,
        };

        let url = format!("{}/chat/completions", self.endpoint.trim_end_matches('/'));
        let response = self
            .http
            .post(&url)
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .await
            .map_err(|e| AppError::Ai(format!("OpenAI tool request failed: {}", e)))?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(AppError::Ai(format!(
                "OpenAI tool request returned {}: {}",
                status, text
            )));
        }

        let parsed: OpenAiToolResponse = response
            .json()
            .await
            .map_err(|e| AppError::Ai(format!("Failed to parse OpenAI tool response: {}", e)))?;
        let message = parsed
            .choices
            .into_iter()
            .next()
            .map(|choice| choice.message)
            .unwrap_or(OpenAiToolResponseMessage {
                content: None,
                tool_calls: Vec::new(),
            });

        Ok(ToolChatResponse {
            content: message.content.unwrap_or_default(),
            tool_calls: message
                .tool_calls
                .into_iter()
                .map(|call| ToolCall {
                    id: call.id,
                    name: call.function.name,
                    arguments: serde_json::from_str(&call.function.arguments)
                        .unwrap_or_else(|_| serde_json::json!({ "raw": call.function.arguments })),
                })
                .collect(),
        })
    }
}

fn openai_tool_message(message: &ToolChatMessage) -> OpenAiToolMessage {
    match message {
        ToolChatMessage::Message(chat) => OpenAiToolMessage {
            role: chat.role.as_str().to_string(),
            content: Some(chat.content.clone()),
            tool_call_id: None,
            name: None,
            tool_calls: None,
        },
        ToolChatMessage::AssistantToolCalls {
            content,
            tool_calls,
        } => OpenAiToolMessage {
            role: "assistant".to_string(),
            content: content.clone(),
            tool_call_id: None,
            name: None,
            tool_calls: Some(
                tool_calls
                    .iter()
                    .map(|call| OpenAiOutboundToolCall {
                        id: call.id.clone(),
                        call_type: "function",
                        function: OpenAiOutboundToolFunction {
                            name: call.name.clone(),
                            arguments: call.arguments.to_string(),
                        },
                    })
                    .collect(),
            ),
        },
        ToolChatMessage::ToolResult {
            tool_call_id,
            name,
            content,
        } => OpenAiToolMessage {
            role: "tool".to_string(),
            content: Some(content.clone()),
            tool_call_id: Some(tool_call_id.clone()),
            name: Some(name.clone()),
            tool_calls: None,
        },
    }
}
