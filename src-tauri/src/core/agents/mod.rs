// Cognest Core — Agent trait and implementations
// Background AI agents: Curator, Writing, Reflection

pub mod curator;
pub mod writing;
pub mod reflection;

use serde_json::Value;

use super::embedding::EmbeddingError;
use super::jobs::WorkerContext;
use super::llm::LlmError;

// ─── Agent Trait ────────────────────────────────────────────────────────────

/// Unified Agent trait.
/// Each agent is invoked by the JobQueue worker to perform background AI tasks.
pub trait Agent: Send + Sync {
    /// Agent name identifier (used for LLM routing).
    fn name(&self) -> &str;

    /// Execute a job (called by the worker thread).
    fn execute(
        &self,
        payload: &Value,
        context: &WorkerContext,
    ) -> Result<Value, AgentError>;
}

/// Agent execution errors.
#[derive(Debug, thiserror::Error)]
pub enum AgentError {
    #[error("Embedding 错误: {0}")]
    Embedding(#[from] EmbeddingError),

    #[error("LLM 错误: {0}")]
    Llm(#[from] LlmError),

    #[error("文件操作错误: {0}")]
    Repo(String),

    #[error("索引错误: {0}")]
    Index(String),

    #[error("执行超时")]
    Timeout,

    #[error("无效参数: {0}")]
    InvalidPayload(String),
}
