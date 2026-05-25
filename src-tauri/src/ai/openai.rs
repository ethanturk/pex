use crate::ai::{AiProvider, ChatMessage};
use crate::AppError;
use async_openai::types::chat::{
    ChatCompletionRequestAssistantMessage, ChatCompletionRequestMessage,
    ChatCompletionRequestSystemMessage, ChatCompletionRequestUserMessage,
    CreateChatCompletionRequestArgs,
};

/// OpenAI-compatible provider backed by the `async-openai` crate.
/// Works with any endpoint that speaks the OpenAI chat completions API
/// (OpenAI, Azure OpenAI, Ollama, OpenRouter, vLLM, etc.).
pub struct OpenAiProvider {
    client: async_openai::Client<async_openai::config::OpenAIConfig>,
    model: String,
    request_timeout: std::time::Duration,
}

impl OpenAiProvider {
    pub fn new(endpoint: String, model: String, api_key: String, request_timeout_secs: u64) -> Self {
        let config = async_openai::config::OpenAIConfig::default()
            .with_api_base(endpoint)
            .with_api_key(api_key);

        let client = async_openai::Client::with_config(config);
        Self {
            client,
            model,
            // async-openai pins its own reqwest version, so we can't inject a
            // timeout-configured client. Enforce the timeout at the call boundary instead.
            request_timeout: std::time::Duration::from_secs(request_timeout_secs),
        }
    }
}

#[async_trait::async_trait]
impl AiProvider for OpenAiProvider {
    async fn chat(&self, messages: &[ChatMessage]) -> Result<String, AppError> {
        let openai_messages: Vec<ChatCompletionRequestMessage> = messages
            .iter()
            .map(|m| match m.role {
                crate::ai::ChatRole::System => {
                    ChatCompletionRequestSystemMessage::from(m.content.clone()).into()
                }
                crate::ai::ChatRole::User => {
                    ChatCompletionRequestUserMessage::from(m.content.clone()).into()
                }
                crate::ai::ChatRole::Assistant => {
                    ChatCompletionRequestAssistantMessage::from(m.content.clone()).into()
                }
            })
            .collect();

        let request = CreateChatCompletionRequestArgs::default()
            .model(&self.model)
            .messages(openai_messages)
            .build()
            .map_err(|e| AppError::Ai(format!("Failed to build request: {}", e)))?;

        let response = tokio::time::timeout(
            self.request_timeout,
            self.client.chat().create(request),
        )
        .await
        .map_err(|_| {
            AppError::Ai(format!(
                "OpenAI request timed out after {}s",
                self.request_timeout.as_secs()
            ))
        })?
        .map_err(|e| AppError::Ai(format!("OpenAI request failed: {}", e)))?;

        let content = response
            .choices
            .into_iter()
            .next()
            .and_then(|c| c.message.content)
            .unwrap_or_default();

        Ok(content)
    }
}
