//! Minimal OpenAI-compatible chat client used for tech-stack classification.
//! Any provider speaking POST {base_url}/chat/completions works
//! (DeepSeek/Qwen/GLM/Ollama serve this shape).
use crate::config::settings::LlmConfig;
use reqwest::Client;
use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Debug, thiserror::Error)]
pub enum LlmError {
    #[error("LLM is not configured")]
    Disabled,
    #[error("LLM request failed: {0}")]
    Request(#[from] reqwest::Error),
    #[error("LLM HTTP {0}")]
    HttpStatus(u16),
    #[error("LLM returned code {0}: {1}")]
    Api(i64, String),
    #[error("LLM returned no usable content")]
    EmptyContent,
}

// Do not derive Debug: the API key must not appear in logs.
pub struct LlmClient {
    client: Client,
    cfg: LlmConfig,
}

#[derive(Deserialize)]
struct ChatResponse {
    #[serde(default)]
    error: Option<ChatError>,
    #[serde(default)]
    choices: Vec<ChatChoice>,
}

#[derive(Deserialize)]
struct ChatError {
    #[serde(default)]
    code: Value,
    #[serde(default)]
    message: String,
}

#[derive(Deserialize)]
struct ChatChoice {
    #[serde(default)]
    message: Option<ChatMessage>,
}

#[derive(Deserialize)]
struct ChatMessage {
    #[serde(default)]
    content: Option<String>,
}

impl LlmClient {
    pub fn new(cfg: LlmConfig) -> Result<Self, LlmError> {
        if !cfg.enabled
            || cfg.base_url.trim().is_empty()
            || cfg.model.trim().is_empty()
            || cfg.api_key.trim().is_empty()
        {
            return Err(LlmError::Disabled);
        }
        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(cfg.timeout_secs.max(3)))
            .build()?;
        Ok(Self { client, cfg })
    }

    /// One chat completion; temperature 0 for deterministic classification.
    pub async fn complete(&self, system: &str, user: &str) -> Result<String, LlmError> {
        let url = format!(
            "{}/chat/completions",
            self.cfg.base_url.trim().trim_end_matches('/')
        );
        let response = self
            .client
            .post(url)
            .bearer_auth(self.cfg.api_key.trim())
            .json(&json!({
                "model": self.cfg.model.trim(),
                "temperature": 0,
                "messages": [
                    {"role": "system", "content": system},
                    {"role": "user", "content": user}
                ]
            }))
            .send()
            .await?;
        let status = response.status();
        let body: ChatResponse = response
            .json()
            .await
            .map_err(|_| LlmError::HttpStatus(status.as_u16()))?;
        if let Some(error) = body.error {
            return Err(LlmError::Api(
                error.code.as_i64().unwrap_or_default(),
                error
                    .message
                    .chars()
                    .filter(|c| !c.is_control())
                    .take(200)
                    .collect(),
            ));
        }
        if !status.is_success() {
            return Err(LlmError::HttpStatus(status.as_u16()));
        }
        body.choices
            .into_iter()
            .find_map(|choice| choice.message.and_then(|message| message.content))
            .filter(|content| !content.trim().is_empty())
            .ok_or(LlmError::EmptyContent)
    }
}
