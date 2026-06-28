//! Rig Agent 层 — 基于 Rig 框架的 async-first Agent 管理
//!
//! 子模块：
//! - registry: Agent 注册中心与生命周期管理
//! - router: Provider 路由器
//! - writing: Writing Agent（流式输出）
//! - curator: Curator Agent（tool calling）
//! - reflection: Reflection Agent
//! - stream_adapter: Rig stream → Tauri event 适配层
//! - types: 共享类型（StreamChunk, TokenUsage, LlmError, ChatMessage 等）

pub mod registry;
pub mod router;
pub mod writing;
pub mod curator;
pub mod reflection;
pub mod stream_adapter;
pub mod types;

use serde::{Deserialize, Serialize};

/// Rig Agent 层统一错误类型
#[derive(Debug, thiserror::Error, Serialize, Deserialize)]
pub enum AgentError {
    #[error("Agent 不可用: {reason}")]
    AgentUnavailable { agent: String, reason: String },

    #[error("无可用 Provider")]
    NoProvider,

    #[error("Provider 回退: {from} → {to}")]
    ProviderFallback { from: String, to: String },

    #[error("LLM 调用失败: {0}")]
    LlmFailure(String),

    #[error("Tool 调用失败: {0}")]
    ToolFailure(String),

    #[error("请求超时")]
    Timeout,

    #[error("流已取消")]
    Cancelled,

    #[error("Embedding 错误: {0}")]
    Embedding(String),

    #[error("进程 spawn 失败: {0}")]
    ProcessSpawn(String),

    #[error("进程已在运行")]
    ProcessAlreadyRunning,
}

/// Agent 可用性状态
#[derive(Debug, Clone)]
pub enum AgentStatus {
    Available,
    Unavailable { reason: String },
    Reloading,
}
