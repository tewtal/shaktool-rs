#![allow(dead_code)]

use reqwest::header::USER_AGENT;
use serde::{Deserialize, Serialize};

use crate::Error;

const AGENT: &str = "shaktool-rs/2.0";
const DEFAULT_OPENAI_BASE_URL: &str = "https://api.openai.com/v1";
const DEFAULT_ANTHROPIC_BASE_URL: &str = "https://api.anthropic.com/v1";
const DEFAULT_ANTHROPIC_VERSION: &str = "2023-06-01";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LlmVendor {
    OpenAi,
    Anthropic,
    /// Any service implementing OpenAI's `/chat/completions` request shape.
    OpenAiCompatible,
}

impl LlmVendor {
    fn parse(value: &str) -> Option<Self> {
        match value {
            "openai" => Some(Self::OpenAi),
            "anthropic" => Some(Self::Anthropic),
            "openai-compatible" | "compatible" => Some(Self::OpenAiCompatible),
            _ => None,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::OpenAi => "openai",
            Self::Anthropic => "anthropic",
            Self::OpenAiCompatible => "openai-compatible",
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum LlmRole {
    System,
    User,
    Assistant,
}

#[derive(Clone, Debug, Serialize)]
pub struct LlmMessage {
    pub role: LlmRole,
    pub content: String,
}

impl LlmMessage {
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: LlmRole::System,
            content: content.into(),
        }
    }

    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: LlmRole::User,
            content: content.into(),
        }
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: LlmRole::Assistant,
            content: content.into(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct LlmRequest {
    pub messages: Vec<LlmMessage>,
    pub temperature: Option<f32>,
    pub max_tokens: Option<u32>,
}

impl LlmRequest {
    pub fn new(messages: Vec<LlmMessage>) -> Self {
        Self {
            messages,
            temperature: None,
            max_tokens: None,
        }
    }

    pub fn with_temperature(mut self, temperature: f32) -> Self {
        self.temperature = Some(temperature);
        self
    }

    pub fn with_max_tokens(mut self, max_tokens: u32) -> Self {
        self.max_tokens = Some(max_tokens);
        self
    }
}

#[derive(Clone, Debug)]
pub struct LlmResponse {
    pub text: String,
    pub vendor: LlmVendor,
    pub model: String,
}

#[derive(Clone, Debug)]
pub struct LlmConfig {
    pub vendor: LlmVendor,
    pub model: String,
    pub api_key: String,
    pub base_url: String,
    pub anthropic_version: String,
}

impl LlmConfig {
    pub fn from_env() -> Result<Self, Error> {
        let vendor = std::env::var("LLM_VENDOR")
            .unwrap_or_else(|_| "openai".to_string())
            .to_ascii_lowercase();
        let vendor = LlmVendor::parse(&vendor)
            .ok_or_else(|| format!("unsupported LLM_VENDOR '{}'", vendor))?;

        let model = std::env::var("LLM_MODEL")
            .map_err(|_| "LLM_MODEL must be set to choose the backend model")?;
        let api_key = std::env::var("LLM_API_KEY")
            .or_else(|_| match vendor {
                LlmVendor::OpenAi | LlmVendor::OpenAiCompatible => std::env::var("OPENAI_API_KEY"),
                LlmVendor::Anthropic => std::env::var("ANTHROPIC_API_KEY"),
            })
            .map_err(|_| format!("no API key configured for {} backend", vendor.label()))?;

        let base_url = std::env::var("LLM_BASE_URL").unwrap_or_else(|_| match vendor {
            LlmVendor::OpenAi => DEFAULT_OPENAI_BASE_URL.to_string(),
            LlmVendor::Anthropic => DEFAULT_ANTHROPIC_BASE_URL.to_string(),
            LlmVendor::OpenAiCompatible => DEFAULT_OPENAI_BASE_URL.to_string(),
        });
        let anthropic_version = std::env::var("ANTHROPIC_VERSION")
            .unwrap_or_else(|_| DEFAULT_ANTHROPIC_VERSION.to_string());

        Ok(Self {
            vendor,
            model,
            api_key,
            base_url,
            anthropic_version,
        })
    }
}

pub struct LlmClient {
    http: reqwest::Client,
    config: LlmConfig,
}

impl LlmClient {
    pub fn from_env() -> Result<Self, Error> {
        Ok(Self::new(LlmConfig::from_env()?))
    }

    pub fn new(config: LlmConfig) -> Self {
        Self {
            http: reqwest::Client::new(),
            config,
        }
    }

    pub async fn chat(&self, request: LlmRequest) -> Result<LlmResponse, Error> {
        match self.config.vendor {
            LlmVendor::OpenAi | LlmVendor::OpenAiCompatible => {
                self.chat_openai_compatible(request).await
            }
            LlmVendor::Anthropic => self.chat_anthropic(request).await,
        }
    }

    async fn chat_openai_compatible(&self, request: LlmRequest) -> Result<LlmResponse, Error> {
        let body = OpenAiChatRequest {
            model: &self.config.model,
            messages: &request.messages,
            temperature: request.temperature,
            max_tokens: request.max_tokens,
        };
        let response = self
            .http
            .post(format!(
                "{}/chat/completions",
                self.config.base_url.trim_end_matches('/')
            ))
            .header(USER_AGENT, AGENT)
            .bearer_auth(&self.config.api_key)
            .json(&body)
            .send()
            .await?
            .error_for_status()?;
        let response: OpenAiChatResponse = response.json().await?;
        let text = response
            .choices
            .first()
            .and_then(|choice| choice.message.content.clone())
            .unwrap_or_default();

        Ok(LlmResponse {
            text,
            vendor: self.config.vendor,
            model: response.model,
        })
    }

    async fn chat_anthropic(&self, request: LlmRequest) -> Result<LlmResponse, Error> {
        let (system, messages) = split_anthropic_messages(request.messages);
        let body = AnthropicChatRequest {
            model: &self.config.model,
            max_tokens: request.max_tokens.unwrap_or(1024),
            temperature: request.temperature,
            system,
            messages,
        };
        let response = self
            .http
            .post(format!(
                "{}/messages",
                self.config.base_url.trim_end_matches('/')
            ))
            .header(USER_AGENT, AGENT)
            .header("x-api-key", &self.config.api_key)
            .header("anthropic-version", &self.config.anthropic_version)
            .json(&body)
            .send()
            .await?
            .error_for_status()?;
        let response: AnthropicChatResponse = response.json().await?;
        let text = response
            .content
            .into_iter()
            .filter_map(|part| match part {
                AnthropicContent::Text { text, .. } => Some(text),
                AnthropicContent::Other => None,
            })
            .collect::<Vec<_>>()
            .join("");

        Ok(LlmResponse {
            text,
            vendor: self.config.vendor,
            model: response.model,
        })
    }
}

/// Sends a chat request through the backend selected by environment variables.
pub async fn chat(request: LlmRequest) -> Result<LlmResponse, Error> {
    LlmClient::from_env()?.chat(request).await
}

#[derive(Serialize)]
struct OpenAiChatRequest<'a> {
    model: &'a str,
    messages: &'a [LlmMessage],
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
}

#[derive(Deserialize)]
struct OpenAiChatResponse {
    model: String,
    choices: Vec<OpenAiChoice>,
}

#[derive(Deserialize)]
struct OpenAiChoice {
    message: OpenAiMessage,
}

#[derive(Deserialize)]
struct OpenAiMessage {
    content: Option<String>,
}

#[derive(Serialize)]
struct AnthropicChatRequest<'a> {
    model: &'a str,
    max_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    system: Option<String>,
    messages: Vec<AnthropicMessage>,
}

#[derive(Serialize)]
struct AnthropicMessage {
    role: AnthropicRole,
    content: String,
}

#[derive(Serialize)]
#[serde(rename_all = "lowercase")]
enum AnthropicRole {
    User,
    Assistant,
}

#[derive(Deserialize)]
struct AnthropicChatResponse {
    model: String,
    content: Vec<AnthropicContent>,
}

#[derive(Deserialize)]
#[serde(tag = "type")]
enum AnthropicContent {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(other)]
    Other,
}

fn split_anthropic_messages(messages: Vec<LlmMessage>) -> (Option<String>, Vec<AnthropicMessage>) {
    let mut system = Vec::new();
    let mut chat = Vec::new();
    for message in messages {
        match message.role {
            LlmRole::System => system.push(message.content),
            LlmRole::User => chat.push(AnthropicMessage {
                role: AnthropicRole::User,
                content: message.content,
            }),
            LlmRole::Assistant => chat.push(AnthropicMessage {
                role: AnthropicRole::Assistant,
                content: message.content,
            }),
        }
    }

    let system = if system.is_empty() {
        None
    } else {
        Some(system.join("\n\n"))
    };
    (system, chat)
}
