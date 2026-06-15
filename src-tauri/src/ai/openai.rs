use crate::ai::{
    AiProvider, ChatMessage, ChatResponse, TokenUsage, ToolCall, ToolChatMessage, ToolChatResponse,
    ToolDefinition,
};
use crate::AppError;
use serde::de::DeserializeOwned;
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
    reasoning_effort: Option<String>,
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
        reasoning_effort: Option<String>,
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
            reasoning_effort,
            http,
        }
    }

    fn reasoning_effort_for_request(&self) -> Option<&str> {
        match self.reasoning_effort.as_deref() {
            Some("none" | "minimal" | "low" | "medium" | "high" | "xhigh") => {
                self.reasoning_effort.as_deref()
            }
            Some("max") => Some("xhigh"),
            _ => None,
        }
    }
}

async fn parse_success_json<T: DeserializeOwned>(
    response: reqwest::Response,
    label: &str,
) -> Result<T, AppError> {
    let bytes = response
        .bytes()
        .await
        .map_err(|e| AppError::Ai(format!("Failed to read {} response body: {}", label, e)))?;
    serde_json::from_slice::<T>(&bytes).map_err(|e| {
        AppError::Ai(format!(
            "Failed to parse {} response: {}; body preview: {}",
            label,
            e,
            response_body_preview(&bytes)
        ))
    })
}

fn response_body_preview(bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(bytes);
    let preview = cap_chars(text.trim(), 1200);
    if preview.is_empty() {
        "<empty>".to_string()
    } else {
        preview
    }
}

fn cap_chars(text: &str, max_chars: usize) -> String {
    let mut out = String::new();
    for (idx, ch) in text.chars().enumerate() {
        if idx >= max_chars {
            out.push_str("...[truncated]");
            break;
        }
        out.push(ch);
    }
    out
}

#[derive(Serialize)]
struct OpenAiRequest<'a> {
    model: &'a str,
    messages: Vec<OpenAiMessage<'a>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reasoning_effort: Option<&'a str>,
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
    #[serde(default)]
    finish_reason: Option<String>,
    #[serde(default)]
    error: Option<OpenAiChoiceError>,
}

#[derive(Deserialize)]
struct OpenAiResponseMessage {
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    reasoning: Option<String>,
}

#[derive(Deserialize)]
struct OpenAiChoiceError {
    #[serde(default)]
    code: Option<serde_json::Value>,
    #[serde(default)]
    message: Option<String>,
}

fn openai_choice_content(
    choice: OpenAiChoice,
    model: &str,
    max_tokens: Option<u32>,
    usage: Option<TokenUsage>,
) -> Result<String, AppError> {
    if let Some(error) = choice.error {
        let message = error
            .message
            .unwrap_or_else(|| "choice-level provider error".to_string());
        let code = error.code.map(|c| format!(" ({})", c)).unwrap_or_default();
        return Err(AppError::Ai(format!(
            "OpenAI provider API error{} for model `{}` (max_tokens={}, usage={}): {}",
            code,
            model,
            max_tokens_label(max_tokens),
            usage_label(usage),
            message
        )));
    }

    let content = choice.message.content.unwrap_or_default();
    if !content.trim().is_empty() {
        return Ok(content);
    }

    let finish_reason = choice.finish_reason.as_deref().unwrap_or("unknown");
    let reason = if choice
        .message
        .reasoning
        .as_deref()
        .is_some_and(|r| !r.trim().is_empty())
    {
        "reasoning was returned, but final content was empty"
    } else {
        "final content was empty"
    };
    Err(AppError::Ai(format!(
        "OpenAI response finished with `{}` and no usable content for model `{}` (max_tokens={}, usage={}; {})",
        finish_reason,
        model,
        max_tokens_label(max_tokens),
        usage_label(usage),
        reason
    )))
}

fn max_tokens_label(max_tokens: Option<u32>) -> String {
    max_tokens
        .map(|n| n.to_string())
        .unwrap_or_else(|| "provider-default".to_string())
}

fn usage_label(usage: Option<TokenUsage>) -> String {
    usage
        .map(|u| format!("input={}, output={}", u.input_tokens, u.output_tokens))
        .unwrap_or_else(|| "unreported".to_string())
}

#[derive(Serialize)]
struct OpenAiToolRequest {
    model: String,
    messages: Vec<OpenAiToolMessage>,
    tools: Vec<OpenAiToolDefinition>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reasoning_effort: Option<String>,
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
    #[serde(default)]
    usage: Option<OpenAiUsage>,
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
            reasoning_effort: self.reasoning_effort_for_request(),
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

        let parsed: OpenAiResponse = parse_success_json(response, "OpenAI").await?;

        let usage = parsed.usage.as_ref().map(|u| TokenUsage {
            input_tokens: u.prompt_tokens,
            output_tokens: u.completion_tokens,
        });
        let choice = parsed
            .choices
            .into_iter()
            .next()
            .ok_or_else(|| AppError::Ai("OpenAI response contained no choices".into()))?;
        let content = openai_choice_content(choice, model, max_tokens, usage)?;

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
            reasoning_effort: self.reasoning_effort_for_request().map(str::to_string),
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

        let parsed: OpenAiToolResponse = parse_success_json(response, "OpenAI tool").await?;
        let usage = parsed.usage.as_ref().map(|u| TokenUsage {
            input_tokens: u.prompt_tokens,
            output_tokens: u.completion_tokens,
        });
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
            usage,
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
