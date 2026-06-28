//! Reflection Agent — 基于 Rig 框架的后台反思/回顾 Agent
//!
//! 用于 JobQueue 触发的后台反思任务（每日/每周回顾等）。
//! 无 tool calling，仅接收上下文内容并返回反思/洞察文本。
//! 使用 `client.agent(model).preamble(REFLECTION_SYSTEM_PROMPT).build()` 构建。

use std::sync::Arc;

use rig::agent::Agent;
use rig::completion::{Chat, Message};
use rig::prelude::*;
use rig::providers::openai;

use super::AgentError;

/// Reflection Agent 系统提示词
const REFLECTION_SYSTEM_PROMPT: &str = "\
你是 Cognest 知识管理系统的反思与回顾助手。\
你的任务是根据用户提供的碎片内容和统计数据，生成简洁有深度的洞察总结。\
重点关注知识之间的关联、增长趋势和潜在的探索方向。\
直接输出洞察内容，不要加前缀或解释性文字。";

/// 每日洞察最大字符数
const MAX_DAILY_INSIGHT_CHARS: usize = 150;

/// 每周洞察最大字符数
const MAX_WEEKLY_INSIGHT_CHARS: usize = 400;

/// Reflection Agent — 基于 Rig 框架，用于后台反思任务
///
/// 特点：
/// - 无 tool calling（纯文本生成）
/// - 无 streaming（后台任务不需要流式输出）
/// - 简单的 prompt → completion 模式
#[derive(Clone)]
pub struct ReflectionRigAgent {
    agent: Arc<Agent<openai::completion::CompletionModel>>,
}

impl std::fmt::Debug for ReflectionRigAgent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ReflectionRigAgent").finish_non_exhaustive()
    }
}

impl ReflectionRigAgent {
    /// 使用 Rig CompletionsClient 构建 Reflection Agent
    pub fn new(client: &openai::CompletionsClient, model: &str) -> Self {
        let agent = client
            .agent(model)
            .preamble(REFLECTION_SYSTEM_PROMPT)
            .build();
        Self {
            agent: Arc::new(agent),
        }
    }

    /// 通用反思方法 — 接收上下文内容，返回反思/洞察文本
    ///
    /// 适用于任意后台反思任务。调用方组装好 prompt 内容后传入。
    pub async fn reflect(&self, content: &str) -> Result<String, AgentError> {
        let response = self
            .agent
            .chat(content, vec![])
            .await
            .map_err(|e| AgentError::LlmFailure(e.to_string()))?;

        Ok(response)
    }

    /// 带历史消息的反思 — 用于多轮上下文场景
    pub async fn reflect_with_history(
        &self,
        content: &str,
        history: Vec<Message>,
    ) -> Result<String, AgentError> {
        let response = self
            .agent
            .chat(content, history)
            .await
            .map_err(|e| AgentError::LlmFailure(e.to_string()))?;

        Ok(response)
    }

    /// 生成每日回顾洞察
    ///
    /// 根据当天的碎片统计和内容摘要生成简洁洞察（≤150 字符）。
    pub async fn daily_insight(
        &self,
        fragments_count: usize,
        active_topics: &[String],
        content_summary: &str,
    ) -> Result<String, AgentError> {
        let topics_str = if active_topics.is_empty() {
            "无".to_string()
        } else {
            active_topics.join(", ")
        };

        let prompt = format!(
            "请为今日的知识记录生成一句简洁的洞察总结（不超过150个字符）。\n\n\
            今日统计：新增碎片 {} 条，活跃主题：{}\n\n\
            碎片内容摘要:\n{}",
            fragments_count, topics_str, content_summary
        );

        let response = self.reflect(&prompt).await?;
        Ok(truncate_chars(&response, MAX_DAILY_INSIGHT_CHARS))
    }

    /// 生成每周回顾洞察
    ///
    /// 根据本周碎片统计、新建主题、活跃主题等信息生成 2-3 句洞察（≤400 字符）。
    pub async fn weekly_insight(
        &self,
        fragments_count: usize,
        new_topics: &[String],
        top_active_topics: &[String],
        content_summary: &str,
    ) -> Result<String, AgentError> {
        let new_topics_str = if new_topics.is_empty() {
            "无".to_string()
        } else {
            new_topics.join(", ")
        };

        let top_topics_str = if top_active_topics.is_empty() {
            "无".to_string()
        } else {
            top_active_topics.join(", ")
        };

        let prompt = format!(
            "请为本周的知识记录生成2-3句关于知识增长方向的洞察（不超过400个字符）。\n\n\
            本周统计：新增碎片 {} 条，新建主题：{}，Top-3 活跃主题：{}\n\n\
            碎片内容摘要:\n{}",
            fragments_count, new_topics_str, top_topics_str, content_summary
        );

        let response = self.reflect(&prompt).await?;
        Ok(truncate_chars(&response, MAX_WEEKLY_INSIGHT_CHARS))
    }
}

/// 按字符边界截取字符串到指定最大字符数
fn truncate_chars(s: &str, max_chars: usize) -> String {
    s.chars().take(max_chars).collect()
}

// ─── 公开常量供外部使用 ──────────────────────────────────────────────────────

/// 每日洞察字符上限
pub const DAILY_INSIGHT_MAX_CHARS: usize = MAX_DAILY_INSIGHT_CHARS;

/// 每周洞察字符上限
pub const WEEKLY_INSIGHT_MAX_CHARS: usize = MAX_WEEKLY_INSIGHT_CHARS;

/// 测试辅助：构造一个不访问网络的 dummy ReflectionRigAgent
#[cfg(test)]
impl ReflectionRigAgent {
    pub(crate) fn dummy() -> Self {
        let client = openai::CompletionsClient::builder()
            .api_key("test")
            .base_url("http://127.0.0.1:1/v1")
            .build()
            .unwrap();
        Self::new(&client, "test-model")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_truncate_chars_within_limit() {
        assert_eq!(truncate_chars("hello", 10), "hello");
    }

    #[test]
    fn test_truncate_chars_exact_limit() {
        assert_eq!(truncate_chars("hello", 5), "hello");
    }

    #[test]
    fn test_truncate_chars_over_limit() {
        assert_eq!(truncate_chars("hello world", 5), "hello");
    }

    #[test]
    fn test_truncate_chars_unicode() {
        assert_eq!(truncate_chars("你好世界测试", 4), "你好世界");
    }

    #[test]
    fn test_truncate_chars_empty() {
        assert_eq!(truncate_chars("", 100), "");
    }

    #[tokio::test]
    async fn test_reflection_agent_debug() {
        let agent = ReflectionRigAgent::dummy();
        let debug_str = format!("{:?}", agent);
        assert!(debug_str.contains("ReflectionRigAgent"));
    }

    #[tokio::test]
    async fn test_reflection_agent_clone() {
        let agent = ReflectionRigAgent::dummy();
        let cloned = agent.clone();
        // Both should format the same (Arc is shared)
        assert_eq!(format!("{:?}", agent), format!("{:?}", cloned));
    }

    #[test]
    fn test_constants() {
        assert_eq!(DAILY_INSIGHT_MAX_CHARS, 150);
        assert_eq!(WEEKLY_INSIGHT_MAX_CHARS, 400);
    }
}
