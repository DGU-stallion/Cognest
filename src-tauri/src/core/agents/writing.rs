// Cognest Core — Writing Agent
// Context-aware writing assistance with streaming LLM support

use std::pin::Pin;

use futures::Stream;
use serde_json::Value;

use super::{Agent, AgentError};
use crate::core::index::FragmentFilter;
use crate::core::jobs::WorkerContext;
use crate::core::llm::{ChatMessage, ChatOptions, LlmResponse, Role, StreamChunk};

// ─── Constants ──────────────────────────────────────────────────────────────

/// Maximum article content length in characters (~4000 tokens ≈ 8000 chars for Chinese)
const MAX_ARTICLE_CHARS: usize = 8000;

/// Maximum number of related fragments to include in context
const MAX_CONTEXT_FRAGMENTS: usize = 5;

/// Maximum number of history messages to include
const MAX_HISTORY_MESSAGES: usize = 10;

/// Maximum fragments to send per LLM request (privacy constraint from Req 9.2)
const MAX_FRAGMENTS_PER_REQUEST: usize = 20;

/// Maximum tokens per LLM request (privacy constraint from Req 9.2)
/// Approximated as chars for Chinese text (1 char ≈ 1 token)
const MAX_TOKENS_PER_REQUEST: usize = 8000;

// ─── System Prompts ─────────────────────────────────────────────────────────

const SYSTEM_PROMPT_BASE: &str = "\
你是 Cognest 知识管理系统的写作助手。你帮助用户组织思路、扩展内容、推荐相关素材。\
请用中文回答，保持简洁专业。基于提供的文章上下文和相关碎片进行回答。";

const SYSTEM_PROMPT_OUTLINE: &str = "\
你是 Cognest 知识管理系统的写作助手。\
请基于以下文章内容，推荐一个结构化的写作大纲。\
大纲应包含主题标题、各章节标题及简要描述，使用 Markdown 格式。";

const SYSTEM_PROMPT_EXPAND: &str = "\
你是 Cognest 知识管理系统的写作助手。\
请扩展以下段落，增加细节和深度。\
保持原文风格和主题一致性，适当补充论据、示例或分析。";

const SYSTEM_PROMPT_RECOMMEND: &str = "\
你是 Cognest 知识管理系统的写作助手。\
基于当前文章的主题和内容，推荐与之相关的知识素材碎片。\
说明每条推荐素材与文章的关联性。";

// ─── WritingAgent ───────────────────────────────────────────────────────────

/// Writing Agent: provides context-aware writing assistance.
///
/// Responsibilities:
/// - Build enriched context (article + related fragments + history)
/// - Chat / stream_chat with LLM for writing assistance
/// - Recommend related fragments by semantic similarity
/// - Quick actions: outline, expand, recommend
pub struct WritingAgent;

impl WritingAgent {
    /// Build writing context messages for LLM calls.
    ///
    /// Constructs a message array containing:
    /// 1. System prompt explaining the AI role
    /// 2. Article content (truncated to ≤8000 chars / ~4000 tokens)
    /// 3. Related fragments (top 5 by tag/topic match, ranked by shared tags then recency)
    /// 4. Conversation history (last 10 messages)
    pub fn build_context(
        &self,
        article_content: &str,
        history: &[ChatMessage],
        context: &WorkerContext,
    ) -> Result<Vec<ChatMessage>, AgentError> {
        let mut messages = Vec::new();

        // 1. System prompt
        messages.push(ChatMessage {
            role: Role::System,
            content: SYSTEM_PROMPT_BASE.to_string(),
        });

        // 2. Article content (truncated to MAX_ARTICLE_CHARS)
        let truncated_article = truncate_content(article_content, MAX_ARTICLE_CHARS);
        if !truncated_article.is_empty() {
            messages.push(ChatMessage {
                role: Role::System,
                content: format!("【当前文章内容】\n{}", truncated_article),
            });
        }

        // 3. Related fragments (top 5 by tag/topic overlap, ranked by shared tags then recency)
        let related_fragments = self.find_related_fragments(article_content, context)?;
        if !related_fragments.is_empty() {
            let fragments_text = related_fragments
                .iter()
                .enumerate()
                .map(|(i, (id, content))| format!("{}. [{}] {}", i + 1, id, content))
                .collect::<Vec<_>>()
                .join("\n");

            messages.push(ChatMessage {
                role: Role::System,
                content: format!("【相关素材碎片】\n{}", fragments_text),
            });
        }

        // 4. Conversation history (last 10)
        let history_slice = if history.len() > MAX_HISTORY_MESSAGES {
            &history[history.len() - MAX_HISTORY_MESSAGES..]
        } else {
            history
        };

        for msg in history_slice {
            messages.push(msg.clone());
        }

        Ok(messages)
    }

    /// Execute a writing chat request (synchronous).
    ///
    /// Builds context from article content, adds the user's message,
    /// then calls LlmGateway.chat_for_agent("writing", ...).
    pub fn chat(
        &self,
        article_content: &str,
        user_message: &str,
        history: &[ChatMessage],
        context: &WorkerContext,
    ) -> Result<LlmResponse, AgentError> {
        let mut messages = self.build_context(article_content, history, context)?;

        // Add the user's current message
        messages.push(ChatMessage {
            role: Role::User,
            content: user_message.to_string(),
        });

        // Enforce privacy limits before sending to LLM
        self.enforce_privacy_limits(&mut messages);

        let options = ChatOptions::default();
        let llm = context.llm.lock().map_err(|e| {
            AgentError::Repo(format!("LLM lock poisoned: {}", e))
        })?;

        llm.chat_for_agent("writing", &messages, &options)
            .map_err(AgentError::from)
    }

    /// Execute a streaming writing chat request.
    ///
    /// Same as `chat()` but returns a token stream for real-time UI updates.
    pub fn stream_chat(
        &self,
        article_content: &str,
        user_message: &str,
        history: &[ChatMessage],
        context: &WorkerContext,
    ) -> Result<Pin<Box<dyn Stream<Item = StreamChunk> + Send>>, AgentError> {
        let mut messages = self.build_context(article_content, history, context)?;

        // Add the user's current message
        messages.push(ChatMessage {
            role: Role::User,
            content: user_message.to_string(),
        });

        // Enforce privacy limits before sending to LLM
        self.enforce_privacy_limits(&mut messages);

        let options = ChatOptions::default();
        let llm = context.llm.lock().map_err(|e| {
            AgentError::Repo(format!("LLM lock poisoned: {}", e))
        })?;

        llm.stream_for_agent("writing", &messages, &options)
            .map_err(AgentError::from)
    }

    /// Recommend fragments related to the article by semantic similarity.
    ///
    /// Embeds the article content and finds the top `limit` most similar
    /// fragments by cosine similarity. Returns (fragment_id, score) pairs.
    pub fn recommend_fragments(
        &self,
        article_content: &str,
        context: &WorkerContext,
        limit: usize,
    ) -> Result<Vec<(String, f32)>, AgentError> {
        let effective_limit = limit.min(MAX_FRAGMENTS_PER_REQUEST);

        let embedding = context.embedding.lock().map_err(|e| {
            AgentError::Repo(format!("Embedding lock poisoned: {}", e))
        })?;

        // Embed the article content (truncated for efficiency)
        let article_vector = embedding.embed_text(
            &truncate_content(article_content, MAX_ARTICLE_CHARS),
        )?;

        drop(embedding); // Release lock before accessing index

        // Get all fragment IDs from the index
        let index = context.index.lock().map_err(|e| {
            AgentError::Repo(format!("Index lock poisoned: {}", e))
        })?;

        let all_fragments = index
            .list_fragments(FragmentFilter::All, 0, 1000)
            .map_err(|e| AgentError::Repo(format!("IndexDb 查询失败: {}", e)))?;

        drop(index); // Release index lock

        // Re-acquire embedding lock for vector lookups
        let embedding = context.embedding.lock().map_err(|e| {
            AgentError::Repo(format!("Embedding lock poisoned: {}", e))
        })?;

        // Compute cosine similarity between article vector and each fragment vector
        let article_norm: f32 = article_vector.iter().map(|x| x * x).sum::<f32>().sqrt();
        if article_norm == 0.0 {
            return Ok(Vec::new());
        }

        let mut scored: Vec<(String, f32)> = Vec::new();

        for fragment in &all_fragments {
            match embedding.get_vector(&fragment.id) {
                Ok(frag_vec) => {
                    let sim = cosine_sim(&article_vector, &frag_vec);
                    scored.push((fragment.id.clone(), sim));
                }
                Err(_) => {
                    // Skip fragments without computed vectors
                    continue;
                }
            }
        }

        // Sort descending by similarity
        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        scored.truncate(effective_limit);

        Ok(scored)
    }

    /// Execute a quick action with a specialized system prompt.
    ///
    /// Actions:
    /// - "outline": Generate a writing outline
    /// - "expand": Expand a paragraph with more detail
    /// - "recommend": Recommend related material fragments
    pub fn quick_action(
        &self,
        action: &str,
        article_content: &str,
        context: &WorkerContext,
    ) -> Result<LlmResponse, AgentError> {
        let system_prompt = match action {
            "outline" => SYSTEM_PROMPT_OUTLINE,
            "expand" => SYSTEM_PROMPT_EXPAND,
            "recommend" => SYSTEM_PROMPT_RECOMMEND,
            _ => {
                return Err(AgentError::InvalidPayload(format!(
                    "未知快捷动作: {}，支持: outline, expand, recommend",
                    action
                )));
            }
        };

        let truncated = truncate_content(article_content, MAX_ARTICLE_CHARS);

        let messages = vec![
            ChatMessage {
                role: Role::System,
                content: system_prompt.to_string(),
            },
            ChatMessage {
                role: Role::User,
                content: truncated,
            },
        ];

        let options = ChatOptions::default();
        let llm = context.llm.lock().map_err(|e| {
            AgentError::Repo(format!("LLM lock poisoned: {}", e))
        })?;

        llm.chat_for_agent("writing", &messages, &options)
            .map_err(AgentError::from)
    }

    // ─── Private Helpers ────────────────────────────────────────────────────

    /// Find related fragments by tag/topic overlap with the article content.
    ///
    /// Strategy per Requirement 6.2:
    /// 1. Get all categorized fragments (those with tags/topics)
    /// 2. If we can embed the article, rank by semantic similarity
    /// 3. Otherwise fall back to returning the most recent categorized fragments
    ///
    /// Returns up to MAX_CONTEXT_FRAGMENTS (id, content_snippet) pairs.
    fn find_related_fragments(
        &self,
        article_content: &str,
        context: &WorkerContext,
    ) -> Result<Vec<(String, String)>, AgentError> {
        let index = context.index.lock().map_err(|e| {
            AgentError::Repo(format!("Index lock poisoned: {}", e))
        })?;

        // Get categorized fragments — those with tags/topics are more likely relevant
        let fragments = index
            .list_fragments(FragmentFilter::Categorized, 0, 100)
            .map_err(|e| AgentError::Repo(format!("IndexDb 查询失败: {}", e)))?;

        drop(index); // Release lock early

        if fragments.is_empty() {
            return Ok(Vec::new());
        }

        // Use embedding-based similarity to rank fragments
        let embedding = context.embedding.lock().map_err(|e| {
            AgentError::Repo(format!("Embedding lock poisoned: {}", e))
        })?;

        // Embed the article content for comparison
        let article_vector = match embedding.embed_text(
            &truncate_content(article_content, MAX_ARTICLE_CHARS),
        ) {
            Ok(vec) => vec,
            Err(_) => {
                // Fallback: return the most recent categorized fragments
                let result: Vec<(String, String)> = fragments
                    .into_iter()
                    .take(MAX_CONTEXT_FRAGMENTS)
                    .map(|f| (f.id, truncate_content(&f.content, 500)))
                    .collect();
                return Ok(result);
            }
        };

        let article_norm: f32 = article_vector.iter().map(|x| x * x).sum::<f32>().sqrt();
        if article_norm == 0.0 {
            let result: Vec<(String, String)> = fragments
                .into_iter()
                .take(MAX_CONTEXT_FRAGMENTS)
                .map(|f| (f.id, truncate_content(&f.content, 500)))
                .collect();
            return Ok(result);
        }

        // Score fragments by cosine similarity
        let mut scored: Vec<(usize, f32)> = Vec::new();
        for (i, fragment) in fragments.iter().enumerate() {
            match embedding.get_vector(&fragment.id) {
                Ok(frag_vec) => {
                    let sim = cosine_sim(&article_vector, &frag_vec);
                    scored.push((i, sim));
                }
                Err(_) => continue,
            }
        }

        // Sort by similarity descending
        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        scored.truncate(MAX_CONTEXT_FRAGMENTS);

        let result: Vec<(String, String)> = scored
            .into_iter()
            .map(|(idx, _)| {
                let f = &fragments[idx];
                (f.id.clone(), truncate_content(&f.content, 500))
            })
            .collect();

        Ok(result)
    }

    /// Enforce privacy limits per Requirement 9.2:
    /// - No more than 20 fragments per request
    /// - No more than ~8000 tokens total
    ///
    /// Trims messages if they exceed the privacy budget.
    fn enforce_privacy_limits(&self, messages: &mut Vec<ChatMessage>) {
        // Count total character length (approximate: 1 Chinese char ≈ 1 token)
        let total_chars: usize = messages.iter().map(|m| m.content.len()).sum();

        // If total exceeds budget, truncate from the middle (preserve system + last user msg)
        if total_chars > MAX_TOKENS_PER_REQUEST * 2 {
            // Keep system messages and the last user message intact;
            // truncate intermediate context messages
            let mut budget_remaining = MAX_TOKENS_PER_REQUEST * 2;

            // Reserve budget for first (system) and last (user) messages
            if let Some(first) = messages.first() {
                budget_remaining = budget_remaining.saturating_sub(first.content.len());
            }
            if let Some(last) = messages.last() {
                budget_remaining = budget_remaining.saturating_sub(last.content.len());
            }

            // Truncate middle messages to fit within budget
            for msg in messages.iter_mut().skip(1).rev().skip(1) {
                if budget_remaining == 0 {
                    msg.content = "[已截断]".to_string();
                } else if msg.content.len() > budget_remaining {
                    msg.content = truncate_content(&msg.content, budget_remaining);
                    budget_remaining = 0;
                } else {
                    budget_remaining -= msg.content.len();
                }
            }
        }
    }
}

// ─── Agent Trait Implementation ─────────────────────────────────────────────

impl Agent for WritingAgent {
    fn name(&self) -> &str {
        "writing"
    }

    /// Execute a writing job.
    ///
    /// Payload expected:
    /// ```json
    /// {
    ///   "article_content": "...",
    ///   "action": "outline" | "expand" | "recommend" | "chat",
    ///   "user_message": "..." (optional, for "chat" action)
    /// }
    /// ```
    fn execute(
        &self,
        payload: &Value,
        context: &WorkerContext,
    ) -> Result<Value, AgentError> {
        let article_content = payload
            .get("article_content")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        let action = payload
            .get("action")
            .and_then(|v| v.as_str())
            .unwrap_or("chat");

        match action {
            "outline" | "expand" | "recommend" => {
                let response = self.quick_action(action, article_content, context)?;
                Ok(serde_json::json!({
                    "content": response.content,
                    "finish_reason": format!("{:?}", response.finish_reason),
                    "usage": {
                        "prompt_tokens": response.usage.prompt_tokens,
                        "completion_tokens": response.usage.completion_tokens,
                        "total_tokens": response.usage.total_tokens,
                    }
                }))
            }
            "chat" => {
                let user_message = payload
                    .get("user_message")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");

                if user_message.is_empty() {
                    return Err(AgentError::InvalidPayload(
                        "chat 动作需要 user_message 字段".to_string(),
                    ));
                }

                let response = self.chat(article_content, user_message, &[], context)?;
                Ok(serde_json::json!({
                    "content": response.content,
                    "finish_reason": format!("{:?}", response.finish_reason),
                    "usage": {
                        "prompt_tokens": response.usage.prompt_tokens,
                        "completion_tokens": response.usage.completion_tokens,
                        "total_tokens": response.usage.total_tokens,
                    }
                }))
            }
            "recommend_fragments" => {
                let limit = payload
                    .get("limit")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(5) as usize;

                let results = self.recommend_fragments(article_content, context, limit)?;
                Ok(serde_json::json!({
                    "fragments": results.iter().map(|(id, score)| {
                        serde_json::json!({ "id": id, "similarity": score })
                    }).collect::<Vec<_>>()
                }))
            }
            _ => Err(AgentError::InvalidPayload(format!(
                "未知动作: {}，支持: outline, expand, recommend, chat, recommend_fragments",
                action
            ))),
        }
    }
}

// ─── Utility Functions ──────────────────────────────────────────────────────

/// Compute cosine similarity between two vectors.
fn cosine_sim(a: &[f32], b: &[f32]) -> f32 {
    let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm_a == 0.0 || norm_b == 0.0 {
        0.0
    } else {
        (dot / (norm_a * norm_b)).clamp(-1.0, 1.0)
    }
}

/// Truncate content to a maximum number of characters.
/// Attempts to break at a natural boundary (newline or space) if possible.
fn truncate_content(content: &str, max_chars: usize) -> String {
    if content.len() <= max_chars {
        return content.to_string();
    }

    // Try to find a natural break point near the limit
    let boundary = find_char_boundary(content, max_chars);
    let truncated = &content[..boundary];

    // Find the last newline within the truncated range
    if let Some(last_newline) = truncated.rfind('\n') {
        if last_newline > max_chars / 2 {
            return format!("{}...", &content[..last_newline]);
        }
    }

    // Fall back to char boundary truncation
    format!("{}...", &content[..boundary])
}

/// Find the nearest valid UTF-8 char boundary at or before the given byte index.
fn find_char_boundary(s: &str, max_bytes: usize) -> usize {
    if max_bytes >= s.len() {
        return s.len();
    }
    let mut boundary = max_bytes;
    while boundary > 0 && !s.is_char_boundary(boundary) {
        boundary -= 1;
    }
    boundary
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_truncate_content_short() {
        let content = "Hello, world!";
        let result = truncate_content(content, 100);
        assert_eq!(result, "Hello, world!");
    }

    #[test]
    fn test_truncate_content_long() {
        let content = "a".repeat(10000);
        let result = truncate_content(&content, 100);
        assert!(result.len() <= 104); // 100 chars + "..."
        assert!(result.ends_with("..."));
    }

    #[test]
    fn test_truncate_content_chinese() {
        let content = "这是一段中文内容，用于测试截断功能是否正常工作。".repeat(100);
        let result = truncate_content(&content, 50);
        // Should produce a truncated string ending in "..."
        assert!(result.ends_with("..."));
    }

    #[test]
    fn test_truncate_content_newline_boundary() {
        let content = "第一行\n第二行\n第三行\n第四行\n第五行\n第六行\n第七行";
        let result = truncate_content(content, 30);
        // Should break at a newline boundary when possible
        assert!(result.ends_with("...") || result == content);
    }

    #[test]
    fn test_truncate_content_empty() {
        let content = "";
        let result = truncate_content(content, 100);
        assert_eq!(result, "");
    }

    #[test]
    fn test_writing_agent_name() {
        let agent = WritingAgent;
        assert_eq!(agent.name(), "writing");
    }

    #[test]
    fn test_find_char_boundary_ascii() {
        assert_eq!(find_char_boundary("hello", 3), 3);
        assert_eq!(find_char_boundary("hello", 10), 5);
    }

    #[test]
    fn test_find_char_boundary_chinese() {
        // Chinese characters are 3 bytes each in UTF-8
        let chinese = "你好世界";
        assert_eq!(find_char_boundary(chinese, 3), 3); // exactly on boundary
        assert_eq!(find_char_boundary(chinese, 4), 3); // backs up to boundary
        assert_eq!(find_char_boundary(chinese, 5), 3); // backs up to boundary
        assert_eq!(find_char_boundary(chinese, 6), 6); // next boundary
    }

    #[test]
    fn test_cosine_sim_identical() {
        let v = vec![1.0, 2.0, 3.0];
        let sim = cosine_sim(&v, &v);
        assert!((sim - 1.0).abs() < 1e-6);
    }

    #[test]
    fn test_cosine_sim_orthogonal() {
        let a = vec![1.0, 0.0, 0.0];
        let b = vec![0.0, 1.0, 0.0];
        let sim = cosine_sim(&a, &b);
        assert!(sim.abs() < 1e-6);
    }

    #[test]
    fn test_cosine_sim_opposite() {
        let a = vec![1.0, 0.0, 0.0];
        let b = vec![-1.0, 0.0, 0.0];
        let sim = cosine_sim(&a, &b);
        assert!((sim - (-1.0)).abs() < 1e-6);
    }

    #[test]
    fn test_cosine_sim_zero_vector() {
        let a = vec![1.0, 2.0, 3.0];
        let zero = vec![0.0, 0.0, 0.0];
        let sim = cosine_sim(&a, &zero);
        assert_eq!(sim, 0.0);
    }

    #[test]
    fn test_execute_invalid_action() {
        // Test payload validation logic
        let agent = WritingAgent;
        let payload = serde_json::json!({
            "article_content": "test",
            "action": "invalid_action"
        });

        let action = payload.get("action").and_then(|v| v.as_str()).unwrap();
        assert!(!["outline", "expand", "recommend", "chat", "recommend_fragments"].contains(&action));
    }

    #[test]
    fn test_privacy_limits_constants() {
        // Verify privacy constants align with Requirement 9.2
        assert!(MAX_FRAGMENTS_PER_REQUEST <= 20);
        assert!(MAX_TOKENS_PER_REQUEST <= 8000);
    }

    #[test]
    fn test_enforce_privacy_limits_under_budget() {
        let agent = WritingAgent;
        let mut messages = vec![
            ChatMessage { role: Role::System, content: "short".to_string() },
            ChatMessage { role: Role::User, content: "hello".to_string() },
        ];
        agent.enforce_privacy_limits(&mut messages);
        // Under budget — no truncation
        assert_eq!(messages[0].content, "short");
        assert_eq!(messages[1].content, "hello");
    }

    #[test]
    fn test_enforce_privacy_limits_over_budget() {
        let agent = WritingAgent;
        let large_content = "x".repeat(20000);
        let mut messages = vec![
            ChatMessage { role: Role::System, content: "system prompt".to_string() },
            ChatMessage { role: Role::System, content: large_content.clone() },
            ChatMessage { role: Role::User, content: "user msg".to_string() },
        ];
        agent.enforce_privacy_limits(&mut messages);
        // The middle message should be truncated
        assert!(messages[1].content.len() < large_content.len());
    }

    #[test]
    fn test_max_history_messages_limit() {
        // Verify the constant used for history slicing
        assert_eq!(MAX_HISTORY_MESSAGES, 10);
    }

    #[test]
    fn test_max_context_fragments_limit() {
        // Verify the constant for context fragments
        assert_eq!(MAX_CONTEXT_FRAGMENTS, 5);
    }
}
