//! Writing Agent — 基于 Rig 框架的写作辅助 Agent，支持流式输出
//!
//! 使用 Rig `client.agent(model).preamble(...).build()` 构建 Agent。
//! 提供 `stream_chat()` 和 `chat()` 两种调用方式。

use std::pin::Pin;
use std::sync::Arc;

use futures::{Stream, StreamExt};
use rig::agent::{Agent, MultiTurnStreamItem};
use rig::completion::{Chat, Message};
use rig::prelude::*;
use rig::providers::openai;
use rig::streaming::StreamingChat;

use super::AgentError;

/// 写作 Agent 系统提示词
const WRITING_SYSTEM_PROMPT: &str = "\
你是 Cognest 写作助手，帮助用户撰写和润色文章。\
你基于用户提供的文章上下文和相关知识碎片来辅助写作。\
请保持专业、简洁的风格，尊重用户的写作意图。\
如果上下文中提供了相关碎片，请适当引用其中的观点来丰富内容。";

/// 文章上下文截取最大字符数
const MAX_ARTICLE_CONTEXT_CHARS: usize = 4000;

/// 最多注入的相关碎片数量
const MAX_RELATED_FRAGMENTS: usize = 5;

/// 相关碎片的最低相似度阈值
const MIN_FRAGMENT_SIMILARITY: f64 = 0.5;

/// Writing Agent — 基于 Rig 框架，支持流式和同步两种对话方式
#[derive(Clone)]
pub struct WritingRigAgent {
    agent: Arc<Agent<openai::completion::CompletionModel>>,
}

impl std::fmt::Debug for WritingRigAgent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WritingRigAgent").finish_non_exhaustive()
    }
}

impl WritingRigAgent {
    /// 使用 Rig CompletionsClient 构建 Writing Agent
    pub fn new(client: &openai::CompletionsClient, model: &str) -> Self {
        let agent = client
            .agent(model)
            .preamble(WRITING_SYSTEM_PROMPT)
            .build();
        Self {
            agent: Arc::new(agent),
        }
    }

    /// 流式对话 — 返回文本 chunk 流
    ///
    /// 组装 prompt：文章上下文（截取前 4000 字符）+ 相关碎片（最多 5 条，相似度 ≥ 0.5）+ 用户消息
    /// 调用方负责将流转为 Tauri event。
    pub async fn stream_chat(
        &self,
        article_context: &str,
        related_fragments: &[(String, f64)],
        message: &str,
        history: Vec<Message>,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<StreamChunkItem, AgentError>> + Send>>, AgentError>
    {
        let prompt = Self::build_prompt(article_context, related_fragments, message);

        let stream = self
            .agent
            .stream_chat(&prompt, history)
            .await;

        // 将 rig 的 MultiTurnStreamItem 流映射为简化的 StreamChunkItem
        let mapped = stream.filter_map(|item| async move {
            match item {
                Ok(multi_turn_item) => extract_text_from_stream_item(multi_turn_item),
                Err(e) => Some(Err(AgentError::LlmFailure(e.to_string()))),
            }
        });

        Ok(Box::pin(mapped))
    }

    /// 同步对话（完整响应）
    ///
    /// 组装 prompt 同 stream_chat，但等待完整响应后返回。
    pub async fn chat(
        &self,
        article_context: &str,
        related_fragments: &[(String, f64)],
        message: &str,
        history: Vec<Message>,
    ) -> Result<String, AgentError> {
        let prompt = Self::build_prompt(article_context, related_fragments, message);

        let response = self
            .agent
            .chat(&prompt, history)
            .await
            .map_err(|e| AgentError::LlmFailure(e.to_string()))?;

        Ok(response)
    }

    /// 组装完整的 prompt
    ///
    /// 格式：
    /// - 文章上下文（截取前 4000 字符）
    /// - 相关碎片（最多 5 条，相似度 ≥ 0.5）
    /// - 用户消息
    pub fn build_prompt(
        article_context: &str,
        related_fragments: &[(String, f64)],
        message: &str,
    ) -> String {
        let mut prompt = String::new();

        // 注入文章上下文（截取前 MAX_ARTICLE_CONTEXT_CHARS 字符）
        if !article_context.is_empty() {
            let truncated = truncate_str(article_context, MAX_ARTICLE_CONTEXT_CHARS);
            prompt.push_str("【当前文章上下文】\n");
            prompt.push_str(truncated);
            prompt.push_str("\n\n");
        }

        // 注入相关碎片（最多 MAX_RELATED_FRAGMENTS 条，相似度 ≥ MIN_FRAGMENT_SIMILARITY）
        let filtered_fragments: Vec<&(String, f64)> = related_fragments
            .iter()
            .filter(|(_, sim)| *sim >= MIN_FRAGMENT_SIMILARITY)
            .take(MAX_RELATED_FRAGMENTS)
            .collect();

        if !filtered_fragments.is_empty() {
            prompt.push_str("【相关知识碎片】\n");
            for (i, (content, similarity)) in filtered_fragments.iter().enumerate() {
                prompt.push_str(&format!(
                    "碎片 {} (相似度: {:.2}):\n{}\n\n",
                    i + 1,
                    similarity,
                    content
                ));
            }
        }

        // 用户消息
        prompt.push_str("【用户消息】\n");
        prompt.push_str(message);

        prompt
    }
}

/// 流式输出的单个文本 chunk
#[derive(Debug, Clone)]
pub enum StreamChunkItem {
    /// 文本 delta
    Text(String),
    /// 流结束
    Done,
}

/// 从 MultiTurnStreamItem 中提取文本内容
fn extract_text_from_stream_item<R: Clone>(
    item: MultiTurnStreamItem<R>,
) -> Option<Result<StreamChunkItem, AgentError>> {
    use rig::streaming::StreamedAssistantContent;

    match item {
        MultiTurnStreamItem::StreamAssistantItem(content) => match content {
            StreamedAssistantContent::Text(text) => {
                Some(Ok(StreamChunkItem::Text(text.text)))
            }
            StreamedAssistantContent::Final(_) => Some(Ok(StreamChunkItem::Done)),
            // 忽略 ToolCall、Reasoning 等（Writing Agent 不使用 tool）
            _ => None,
        },
        MultiTurnStreamItem::FinalResponse(_) => Some(Ok(StreamChunkItem::Done)),
        // 忽略 StreamUserItem（tool result 等）
        _ => None,
    }
}

/// 截取字符串前 n 个字符（按 char 边界）
fn truncate_str(s: &str, max_chars: usize) -> &str {
    if s.chars().count() <= max_chars {
        return s;
    }
    let byte_idx = s
        .char_indices()
        .nth(max_chars)
        .map(|(idx, _)| idx)
        .unwrap_or(s.len());
    &s[..byte_idx]
}

// ─── 公开常量供属性测试使用 ─────────────────────────────────────────────

/// 公开的上下文约束常量（供属性测试使用）
pub const CONTEXT_MAX_CHARS: usize = MAX_ARTICLE_CONTEXT_CHARS;
pub const CONTEXT_MAX_FRAGMENTS: usize = MAX_RELATED_FRAGMENTS;
pub const CONTEXT_MIN_SIMILARITY: f64 = MIN_FRAGMENT_SIMILARITY;

/// 测试辅助：构造一个不访问网络的 dummy WritingRigAgent
#[cfg(test)]
impl WritingRigAgent {
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
    fn test_truncate_str_within_limit() {
        let s = "hello world";
        assert_eq!(truncate_str(s, 20), "hello world");
    }

    #[test]
    fn test_truncate_str_exact_limit() {
        let s = "hello";
        assert_eq!(truncate_str(s, 5), "hello");
    }

    #[test]
    fn test_truncate_str_over_limit() {
        let s = "hello world";
        assert_eq!(truncate_str(s, 5), "hello");
    }

    #[test]
    fn test_truncate_str_unicode() {
        let s = "你好世界测试";
        assert_eq!(truncate_str(s, 4), "你好世界");
    }

    #[test]
    fn test_build_prompt_empty_context() {
        let prompt = WritingRigAgent::build_prompt("", &[], "写一段话");
        assert!(prompt.contains("【用户消息】"));
        assert!(prompt.contains("写一段话"));
        assert!(!prompt.contains("【当前文章上下文】"));
        assert!(!prompt.contains("【相关知识碎片】"));
    }

    #[test]
    fn test_build_prompt_with_context() {
        let article = "这是文章内容";
        let prompt = WritingRigAgent::build_prompt(article, &[], "帮我润色");
        assert!(prompt.contains("【当前文章上下文】"));
        assert!(prompt.contains("这是文章内容"));
    }

    #[test]
    fn test_build_prompt_truncates_article() {
        // 构造超过 4000 字符的文章
        let article: String = "字".repeat(5000);
        let prompt = WritingRigAgent::build_prompt(&article, &[], "继续");
        // 上下文部分应该只包含 4000 个 "字"
        let context_section = prompt
            .split("【当前文章上下文】\n")
            .nth(1)
            .unwrap()
            .split("\n\n")
            .next()
            .unwrap();
        assert_eq!(context_section.chars().count(), MAX_ARTICLE_CONTEXT_CHARS);
    }

    #[test]
    fn test_build_prompt_filters_low_similarity() {
        let fragments = vec![
            ("高相似度碎片".to_string(), 0.8),
            ("低相似度碎片".to_string(), 0.3),
            ("边界碎片".to_string(), 0.5),
        ];
        let prompt = WritingRigAgent::build_prompt("", &fragments, "问题");
        assert!(prompt.contains("高相似度碎片"));
        assert!(!prompt.contains("低相似度碎片"));
        assert!(prompt.contains("边界碎片"));
    }

    #[test]
    fn test_build_prompt_limits_fragments_to_5() {
        let fragments: Vec<(String, f64)> = (0..10)
            .map(|i| (format!("碎片{}", i), 0.9))
            .collect();
        let prompt = WritingRigAgent::build_prompt("", &fragments, "问题");
        // 只有前 5 条出现
        assert!(prompt.contains("碎片0"));
        assert!(prompt.contains("碎片4"));
        assert!(!prompt.contains("碎片5"));
    }
}
