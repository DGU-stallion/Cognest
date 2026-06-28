//! 共享类型定义 — StreamChunk, TokenUsage, LlmError, ChatMessage, Role 等
//!
//! 这些类型从原 core::llm 模块迁移而来，保持 serde 格式完全兼容。
//! 被 stream_adapter、commands/ai、jobs 等模块共同引用。

use serde::{Deserialize, Serialize};

// ─── Chat Types ─────────────────────────────────────────────────────────────

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

/// Completion finish reason
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FinishReason {
    Stop,
    Length,
    ContentFilter,
}

// ─── Streaming Types ────────────────────────────────────────────────────────

/// Streaming response chunk — 保持与旧版完全一致的 serde 格式
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
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

impl TokenUsage {
    /// 创建零值 TokenUsage（用于取消/无法获取 usage 的情况）
    pub fn zero() -> Self {
        Self {
            prompt_tokens: 0,
            completion_tokens: 0,
            total_tokens: 0,
        }
    }
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
