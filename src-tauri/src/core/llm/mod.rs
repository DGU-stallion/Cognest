// Cognest Core — LLM Gateway
// Unified trait and routing for multiple LLM providers

pub mod deepseek;
pub mod ollama;
pub mod openai_compat;

use std::collections::HashMap;
use std::pin::Pin;

use futures::Stream;
use serde::{Deserialize, Serialize};

use super::settings::{AppSettings, ProviderConfig, ProviderType, SettingsManager};

// ─── Core Types ─────────────────────────────────────────────────────────────

/// Chat message role
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    User,
    Assistant,
}

/// Single chat message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: Role,
    pub content: String,
}

/// LLM call options
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatOptions {
    pub model: Option<String>,
    pub temperature: Option<f32>,
    pub max_tokens: Option<u32>,
    /// Structured output JSON Schema (optional)
    pub json_schema: Option<serde_json::Value>,
    /// Request timeout in seconds, default 30
    pub timeout_secs: Option<u64>,
}

impl Default for ChatOptions {
    fn default() -> Self {
        Self {
            model: None,
            temperature: None,
            max_tokens: None,
            json_schema: None,
            timeout_secs: Some(60),
        }
    }
}

/// Complete LLM response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmResponse {
    pub content: String,
    pub finish_reason: FinishReason,
    pub usage: TokenUsage,
}

/// Streaming response chunk
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum StreamChunk {
    /// Incremental content
    Delta { content: String },
    /// Stream completed summary
    Done { usage: TokenUsage },
    /// Stream interrupted with error
    Error { error: LlmError, partial_tokens: u32 },
}

/// Token usage statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenUsage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}

/// Completion finish reason
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FinishReason {
    Stop,
    Length,
    ContentFilter,
}

// ─── Error Types ────────────────────────────────────────────────────────────

/// LLM error classification
///
/// Privacy note (Req 9.7): Error display strings use neutral phrasing that
/// does NOT confirm whether data reached the remote endpoint.
#[derive(Debug, Clone, Serialize, Deserialize, thiserror::Error)]
pub enum LlmError {
    #[error("[{provider}] 请求超时")]
    Timeout { provider: String },

    #[error("[{provider}] 速率限制")]
    RateLimit { provider: String },

    #[error("[{provider}] 认证失败")]
    AuthFailure { provider: String },

    #[error("[{provider}] 操作失败，请检查网络后重试")]
    NetworkError { provider: String, reason: String },

    #[error("无可用 Provider，请在设置中配置")]
    NoProvider,

    #[error("结构化输出验证失败: {details}")]
    SchemaValidation { details: String },

    #[error("[{provider}] 操作未能完成")]
    Unknown { provider: String, reason: String },
}

// ─── Trait Definition ───────────────────────────────────────────────────────

/// Unified LLM Provider trait.
/// All providers must implement this trait.
pub trait LlmProvider: Send + Sync {
    /// Provider name identifier
    fn name(&self) -> &str;

    /// Synchronous chat call
    fn chat(
        &self,
        messages: &[ChatMessage],
        options: &ChatOptions,
    ) -> Result<LlmResponse, LlmError>;

    /// Streaming chat call
    fn stream_chat(
        &self,
        messages: &[ChatMessage],
        options: &ChatOptions,
    ) -> Result<Pin<Box<dyn Stream<Item = StreamChunk> + Send>>, LlmError>;

    /// Validate connection (lightweight test call)
    fn validate(&self) -> Result<(), LlmError>;
}

// ─── LlmGateway ─────────────────────────────────────────────────────────────

/// Gateway managing routing across multiple LLM providers.
pub struct LlmGateway {
    providers: Vec<Box<dyn LlmProvider>>,
    default_provider: Option<String>,
    /// Agent → Provider name mapping
    agent_overrides: HashMap<String, String>,
}

impl LlmGateway {
    /// Create an empty gateway with no providers configured.
    /// All chat/stream calls will return LlmError::NoProvider.
    pub fn empty() -> Self {
        Self {
            providers: Vec::new(),
            default_provider: None,
            agent_overrides: HashMap::new(),
        }
    }

    /// Load gateway configuration from SettingsManager.
    ///
    /// Reads AppSettings, sets up routing (default_provider + agent_overrides).
    /// Individual provider instantiation is deferred to tasks 4.2–4.4;
    /// for now, enabled providers are noted but not constructed.
    pub fn from_config(settings: &SettingsManager) -> Result<Self, LlmError> {
        let app_settings = settings.load().map_err(|_| LlmError::NoProvider)?;

        Self::build_from_settings(&app_settings, settings)
    }

    /// Hot-reload configuration (settings take effect within 2s of save).
    pub fn reload(&mut self, settings: &SettingsManager) -> Result<(), LlmError> {
        let app_settings = settings.load().map_err(|_| LlmError::NoProvider)?;

        let new_gateway = Self::build_from_settings(&app_settings, settings)?;
        self.providers = new_gateway.providers;
        self.default_provider = new_gateway.default_provider;
        self.agent_overrides = new_gateway.agent_overrides;

        Ok(())
    }

    /// Route a chat request for the specified agent.
    ///
    /// Routing logic:
    /// 1. Check agent_overrides for an agent-specific provider
    /// 2. Fall back to default_provider
    /// 3. Return LlmError::NoProvider if no provider is found
    pub fn chat_for_agent(
        &self,
        agent: &str,
        messages: &[ChatMessage],
        options: &ChatOptions,
    ) -> Result<LlmResponse, LlmError> {
        let provider = self.resolve_provider(agent)?;
        provider.chat(messages, options)
    }

    /// Stream a chat response for the specified agent.
    ///
    /// Same routing logic as chat_for_agent.
    pub fn stream_for_agent(
        &self,
        agent: &str,
        messages: &[ChatMessage],
        options: &ChatOptions,
    ) -> Result<Pin<Box<dyn Stream<Item = StreamChunk> + Send>>, LlmError> {
        let provider = self.resolve_provider(agent)?;
        provider.stream_chat(messages, options)
    }

    /// Resolve the provider name for a given agent (used for audit logging).
    ///
    /// Returns the provider name string, or None if no provider is available.
    /// Ollama returns "ollama" which signals a local provider (no cloud audit needed).
    pub fn resolve_provider_name(&self, agent: &str) -> Option<String> {
        self.resolve_provider(agent)
            .ok()
            .map(|p| p.name().to_string())
    }

    // ─── Private Helpers ────────────────────────────────────────────────────

    /// Build a new LlmGateway from AppSettings.
    /// Provider instantiation is handled per ProviderType.
    fn build_from_settings(
        app_settings: &AppSettings,
        settings: &SettingsManager,
    ) -> Result<Self, LlmError> {
        let mut providers: Vec<Box<dyn LlmProvider>> = Vec::new();

        for config in &app_settings.providers {
            if !config.enabled {
                continue;
            }

            // Attempt to create provider based on type.
            // Individual provider construction will be fully implemented in tasks 4.2–4.4.
            if let Some(provider) = Self::create_provider(config, settings) {
                providers.push(provider);
            }
        }

        let default_provider = app_settings.routing.default_provider.clone();
        let agent_overrides = app_settings.routing.overrides.clone();

        Ok(Self {
            providers,
            default_provider,
            agent_overrides,
        })
    }

    /// Create a provider instance from configuration.
    /// Returns None if the provider cannot be instantiated (e.g., missing API key).
    /// Full implementations are in tasks 4.2–4.4.
    fn create_provider(
        config: &ProviderConfig,
        settings: &SettingsManager,
    ) -> Option<Box<dyn LlmProvider>> {
        match config.provider_type {
            ProviderType::DeepSeek => {
                let api_key = settings.get_api_key(&config.id).ok().flatten()?;
                Some(Box::new(deepseek::DeepSeekProvider::new(
                    config.endpoint.clone(),
                    api_key,
                    config.model.clone(),
                )))
            }
            ProviderType::Ollama => {
                Some(Box::new(ollama::OllamaProvider::new(
                    config.endpoint.clone(),
                    config.model.clone(),
                )))
            }
            ProviderType::OpenAiCompat => {
                let api_key = settings.get_api_key(&config.id).ok().flatten()?;
                Some(Box::new(openai_compat::OpenAiCompatProvider::new(
                    config.endpoint.clone(),
                    api_key,
                    config.model.clone(),
                )))
            }
        }
    }

    /// Resolve which provider to use for a given agent.
    ///
    /// Priority:
    /// 1. Agent-specific override
    /// 2. Default provider
    /// 3. First available provider (fallback)
    /// 4. NoProvider error
    fn resolve_provider(&self, agent: &str) -> Result<&dyn LlmProvider, LlmError> {
        // 1. Check agent-specific override
        if let Some(provider_name) = self.agent_overrides.get(agent) {
            if let Some(provider) = self.find_provider_by_name(provider_name) {
                return Ok(provider);
            }
        }

        // 2. Fall back to default provider
        if let Some(ref default_name) = self.default_provider {
            if let Some(provider) = self.find_provider_by_name(default_name) {
                return Ok(provider);
            }
        }

        // 3. Use first available provider as last resort
        if let Some(provider) = self.providers.first() {
            return Ok(provider.as_ref());
        }

        // 4. No provider available
        Err(LlmError::NoProvider)
    }

    /// Find a provider by name from the registered providers list.
    fn find_provider_by_name(&self, name: &str) -> Option<&dyn LlmProvider> {
        self.providers
            .iter()
            .find(|p| p.name() == name)
            .map(|p| p.as_ref())
    }
}
