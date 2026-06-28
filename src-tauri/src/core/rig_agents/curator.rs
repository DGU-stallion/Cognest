//! Curator Agent — 带 Rig tool calling 的分类/聚类 Agent
//!
//! 本模块实现 `EmbeddingSearchTool`（Rig Tool trait），供 Curator Agent
//! 在分类时自主调用 embedding 相似度搜索。

use std::sync::{Arc, Mutex};

use rig::completion::ToolDefinition;
use rig::tool::Tool;
use schemars::{JsonSchema, schema_for};
use serde::{Deserialize, Serialize};

use crate::core::embedding::{EmbeddingEngine, EmbeddingError};
use crate::core::index::IndexDb;

// ─── EmbeddingSearchTool ─────────────────────────────────────────────────────

/// 查询文本最大处理长度（字符数）
pub const MAX_QUERY_CHARS: usize = 2000;

/// 返回结果的最大条数
pub const MAX_RESULTS: usize = 5;

/// EmbeddingSearch 工具输入参数
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct EmbeddingSearchArgs {
    /// 查询文本，工具将只处理前 2000 个字符
    pub query: String,
}

/// 单条相似匹配结果
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SimilarMatch {
    /// 碎片 ID
    pub fragment_id: String,
    /// 余弦相似度分数，范围 [0.0, 1.0]
    pub similarity: f32,
}

/// EmbeddingSearch 工具输出
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct EmbeddingSearchResult {
    /// 按相似度降序排列的匹配结果（最多 5 条）
    pub matches: Vec<SimilarMatch>,
}

/// EmbeddingSearch 工具错误
#[derive(Debug, thiserror::Error)]
pub enum EmbeddingSearchError {
    #[error("Embedding 计算失败: {0}")]
    Embedding(#[from] EmbeddingError),

    #[error("索引访问失败: {0}")]
    Index(String),
}

/// Rig Tool 实现 — 搜索与查询文本最相似的碎片
///
/// 工具名称: `embedding_search`
/// 输入: 查询文本（截取前 2000 字符）
/// 输出: top-5 相似碎片的 ID 和相似度分数
pub struct EmbeddingSearchTool {
    embedding: Arc<Mutex<EmbeddingEngine>>,
    index: Arc<Mutex<IndexDb>>,
}

impl EmbeddingSearchTool {
    /// 创建 EmbeddingSearchTool 实例
    pub fn new(embedding: Arc<Mutex<EmbeddingEngine>>, index: Arc<Mutex<IndexDb>>) -> Self {
        Self { embedding, index }
    }
}

impl Tool for EmbeddingSearchTool {
    const NAME: &'static str = "embedding_search";

    type Error = EmbeddingSearchError;
    type Args = EmbeddingSearchArgs;
    type Output = EmbeddingSearchResult;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "embedding_search".to_string(),
            description: "搜索与查询文本语义最相似的知识碎片，返回 top-5 结果及相似度分数"
                .to_string(),
            parameters: serde_json::to_value(schema_for!(EmbeddingSearchArgs))
                .expect("schema_for should not fail"),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        // 截取前 MAX_QUERY_CHARS 个字符
        let query = truncate_chars(&args.query, MAX_QUERY_CHARS);

        // 获取所有已缓存向量的碎片 ID 作为候选集
        let candidates = {
            let engine = self.embedding.lock().map_err(|e| {
                EmbeddingSearchError::Index(format!("Embedding lock poisoned: {}", e))
            })?;
            engine.cached_fragment_ids()
        };

        if candidates.is_empty() {
            return Ok(EmbeddingSearchResult {
                matches: Vec::new(),
            });
        }

        // 计算查询文本的 embedding 向量
        let query_vec = {
            let engine = self.embedding.lock().map_err(|e| {
                EmbeddingSearchError::Index(format!("Embedding lock poisoned: {}", e))
            })?;
            engine.embed_text(query)?
        };

        // 在候选集中查找 top-k 相似碎片
        let results = {
            let engine = self.embedding.lock().map_err(|e| {
                EmbeddingSearchError::Index(format!("Embedding lock poisoned: {}", e))
            })?;
            engine.find_similar_by_vec(&query_vec, &candidates, MAX_RESULTS)?
        };

        // 将相似度归一化到 [0.0, 1.0]（cosine similarity 原始范围 [-1.0, 1.0]）
        let matches = results
            .into_iter()
            .map(|(id, sim)| SimilarMatch {
                fragment_id: id,
                // 将 [-1.0, 1.0] 映射到 [0.0, 1.0]
                similarity: normalize_similarity(sim),
            })
            .collect();

        Ok(EmbeddingSearchResult { matches })
    }
}

/// 将余弦相似度从 [-1.0, 1.0] 归一化到 [0.0, 1.0]
/// 公式: (sim + 1.0) / 2.0，然后 clamp 确保边界
pub fn normalize_similarity(sim: f32) -> f32 {
    ((sim + 1.0) / 2.0).clamp(0.0, 1.0)
}

/// 按字符边界截取字符串到指定最大字符数
pub fn truncate_chars(s: &str, max_chars: usize) -> &str {
    if s.chars().count() <= max_chars {
        return s;
    }
    // 找到第 max_chars 个字符的字节偏移
    let byte_offset = s
        .char_indices()
        .nth(max_chars)
        .map(|(idx, _)| idx)
        .unwrap_or(s.len());
    &s[..byte_offset]
}

// ─── CuratorRigAgent ─────────────────────────────────────────────────────────

use std::sync::Arc as StdArc;

use rig::agent::Agent;
use rig::completion::TypedPrompt;
use rig::prelude::*;
use rig::providers::openai;

use super::AgentError;

/// 聚类阈值常量
pub const TOPIC_ASSIGN_THRESHOLD: f64 = 0.75;
pub const CLUSTER_FORM_THRESHOLD: f64 = 0.70;

/// 最大标签数量
const MAX_TAGS: usize = 5;

/// 每个标签的最大字符数
const MAX_TAG_CHARS: usize = 10;

/// Curator Agent 系统提示词
const CURATOR_SYSTEM_PROMPT: &str = "\
你是 Cognest 知识库管理助手，负责对知识碎片进行分类和标签生成。\
\n\n你拥有一个工具 embedding_search，可以搜索知识库中与指定文本语义相似的碎片。\
你可以根据需要调用此工具来辅助分类决策。\
\n\n分类任务：根据碎片内容判断其所属主题（topic），可调用 embedding_search 查找相似碎片来辅助判断。\
标签生成任务：根据碎片内容生成 1-5 个描述性标签，每个标签不超过 10 个字符。";

/// 分类结果
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ClassifyResult {
    /// 分配到的主题（如有）
    pub assigned_topic: Option<String>,
    /// 是否建议创建新主题
    pub suggest_new_topic: bool,
    /// LLM 给出的分类理由
    pub reasoning: String,
}

/// 标签生成结果
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct TagGenerationResult {
    /// 生成的标签列表（1-5 个，每个不超过 10 字符）
    pub tags: Vec<String>,
}

/// Curator Agent — 带 Rig tool calling，执行分类和标签生成任务
///
/// 使用 `client.agent(model).preamble(...).tool(embedding_search_tool).build()` 构建。
/// LLM 可自主决定是否调用 EmbeddingSearch 工具来辅助分类决策。
#[derive(Clone)]
pub struct CuratorRigAgent {
    agent: StdArc<Agent<openai::completion::CompletionModel>>,
}

impl std::fmt::Debug for CuratorRigAgent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CuratorRigAgent").finish_non_exhaustive()
    }
}

impl CuratorRigAgent {
    /// 使用 Rig CompletionsClient 构建 Curator Agent（带 EmbeddingSearchTool）
    pub fn new(
        client: &openai::CompletionsClient,
        model: &str,
        embedding: Arc<Mutex<EmbeddingEngine>>,
        index: Arc<Mutex<IndexDb>>,
    ) -> Self {
        let tool = EmbeddingSearchTool::new(embedding, index);
        let agent = client
            .agent(model)
            .preamble(CURATOR_SYSTEM_PROMPT)
            .tool(tool)
            .build();
        Self {
            agent: StdArc::new(agent),
        }
    }

    /// 构建不带 EmbeddingSearchTool 的 Curator Agent
    ///
    /// 用于 EmbeddingEngine 尚未就绪时的初始化场景。
    /// Agent 仍可进行分类和标签生成，但 LLM 无法调用 embedding_search 工具。
    pub fn new_without_tool(client: &openai::CompletionsClient, model: &str) -> Self {
        let agent = client
            .agent(model)
            .preamble(CURATOR_SYSTEM_PROMPT)
            .build();
        Self {
            agent: StdArc::new(agent),
        }
    }

    /// 执行分类任务 — 调用 Agent，LLM 自主决定是否调用 EmbeddingSearch
    ///
    /// 如果 EmbeddingSearch 调用失败，降级为未分类状态（返回 assigned_topic = None）。
    pub async fn classify_fragment(
        &self,
        fragment_content: &str,
    ) -> Result<ClassifyResult, AgentError> {
        let truncated = truncate_chars(fragment_content, MAX_QUERY_CHARS);

        let prompt = format!(
            "请对以下知识碎片进行分类。你可以使用 embedding_search 工具搜索相似碎片来辅助判断。\
            \n如果找到高度相似的已分类碎片（相似度 ≥ {threshold}），将此碎片归入相同主题。\
            \n如果没有找到匹配的主题，设置 suggest_new_topic 为 true。\
            \n\n碎片内容:\n{content}",
            threshold = TOPIC_ASSIGN_THRESHOLD,
            content = truncated,
        );

        // 使用 prompt_typed 获取结构化输出
        let result: Result<ClassifyResult, _> = self.agent.prompt_typed(&prompt).await;

        match result {
            Ok(classify_result) => Ok(classify_result),
            Err(e) => {
                // EmbeddingSearch 失败或 LLM 解析失败 → 降级为未分类
                log::warn!("Curator 分类失败，降级为未分类状态: {}", e);
                Ok(ClassifyResult {
                    assigned_topic: None,
                    suggest_new_topic: false,
                    reasoning: format!("分类降级: {}", e),
                })
            }
        }
    }

    /// 生成标签（1-5 个，每个 ≤ 10 字符）
    ///
    /// 标签用于描述碎片的关键概念。生成失败时返回空列表。
    pub async fn generate_tags(
        &self,
        fragment_content: &str,
    ) -> Result<Vec<String>, AgentError> {
        let truncated = truncate_chars(fragment_content, MAX_QUERY_CHARS);

        let prompt = format!(
            "请为以下知识碎片生成 1 到 5 个描述性标签。\
            \n每个标签不超过 10 个字符，使用简短的中文或英文关键词。\
            \n标签应概括碎片的核心主题或关键概念。\
            \n\n碎片内容:\n{content}",
            content = truncated,
        );

        let result: Result<TagGenerationResult, _> = self.agent.prompt_typed(&prompt).await;

        match result {
            Ok(tag_result) => {
                // 确保标签满足约束：最多 5 个，每个 ≤ 10 字符
                let tags = sanitize_tags(tag_result.tags);
                Ok(tags)
            }
            Err(e) => {
                log::warn!("标签生成失败，返回空列表: {}", e);
                Ok(vec![])
            }
        }
    }
}

/// 清理和验证标签列表：最多 MAX_TAGS 个，每个截取到 MAX_TAG_CHARS 字符
pub fn sanitize_tags(tags: Vec<String>) -> Vec<String> {
    tags.into_iter()
        .filter(|t| !t.trim().is_empty())
        .take(MAX_TAGS)
        .map(|t| {
            let trimmed = t.trim().to_string();
            if trimmed.chars().count() > MAX_TAG_CHARS {
                trimmed.chars().take(MAX_TAG_CHARS).collect()
            } else {
                trimmed
            }
        })
        .collect()
}

/// 合并 topics 列表，保证无重复
pub fn merge_topics(existing: &[String], new_topic: &str) -> Vec<String> {
    let mut topics: Vec<String> = Vec::new();
    // Deduplicate existing topics
    for topic in existing {
        if !topics.iter().any(|t| t == topic) {
            topics.push(topic.clone());
        }
    }
    // Add new topic if non-empty and not already present
    if !new_topic.is_empty() && !topics.iter().any(|t| t == new_topic) {
        topics.push(new_topic.to_string());
    }
    topics
}

/// 合并 tags 列表，保证无重复
pub fn merge_tags(existing: &[String], new_tags: &[String]) -> Vec<String> {
    let mut tags: Vec<String> = Vec::new();
    // Deduplicate existing tags
    for tag in existing {
        if !tags.iter().any(|t| t == tag) {
            tags.push(tag.clone());
        }
    }
    // Add new non-empty, non-duplicate tags
    for tag in new_tags {
        if !tag.trim().is_empty() && !tags.iter().any(|t| t == tag) {
            tags.push(tag.clone());
        }
    }
    tags
}

/// 测试辅助：构造一个不访问网络的 dummy CuratorRigAgent（不含 EmbeddingSearchTool）
#[cfg(test)]
impl CuratorRigAgent {
    pub(crate) fn dummy() -> Self {
        let client = openai::CompletionsClient::builder()
            .api_key("test")
            .base_url("http://127.0.0.1:1/v1")
            .build()
            .unwrap();
        // 测试用 agent 不注册 tool（避免依赖真实 EmbeddingEngine）
        let agent = client
            .agent("test-model")
            .preamble(CURATOR_SYSTEM_PROMPT)
            .build();
        Self {
            agent: StdArc::new(agent),
        }
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_truncate_chars_within_limit() {
        let s = "hello world";
        assert_eq!(truncate_chars(s, 2000), "hello world");
    }

    #[test]
    fn test_truncate_chars_exceeds_limit() {
        let s = "abcdefghij"; // 10 chars
        assert_eq!(truncate_chars(s, 5), "abcde");
    }

    #[test]
    fn test_truncate_chars_multibyte() {
        let s = "你好世界测试文本"; // 8 Chinese chars
        let truncated = truncate_chars(s, 4);
        assert_eq!(truncated, "你好世界");
    }

    #[test]
    fn test_truncate_chars_empty() {
        assert_eq!(truncate_chars("", 2000), "");
    }

    #[test]
    fn test_normalize_similarity() {
        assert!((normalize_similarity(1.0) - 1.0).abs() < f32::EPSILON);
        assert!((normalize_similarity(0.0) - 0.5).abs() < f32::EPSILON);
        assert!((normalize_similarity(-1.0) - 0.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_normalize_similarity_clamp() {
        // Values should be clamped within [0.0, 1.0]
        assert_eq!(normalize_similarity(1.5), 1.0);
        assert_eq!(normalize_similarity(-1.5), 0.0);
    }

    #[test]
    fn test_embedding_search_result_serialization() {
        let result = EmbeddingSearchResult {
            matches: vec![
                SimilarMatch {
                    fragment_id: "frag-001".to_string(),
                    similarity: 0.95,
                },
                SimilarMatch {
                    fragment_id: "frag-002".to_string(),
                    similarity: 0.82,
                },
            ],
        };

        let json = serde_json::to_string(&result).unwrap();
        let parsed: EmbeddingSearchResult = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.matches.len(), 2);
        assert_eq!(parsed.matches[0].fragment_id, "frag-001");
        assert!((parsed.matches[0].similarity - 0.95).abs() < f32::EPSILON);
    }

    #[test]
    fn test_tool_name() {
        assert_eq!(EmbeddingSearchTool::NAME, "embedding_search");
    }

    // ─── CuratorRigAgent unit tests ─────────────────────────────────────────

    #[test]
    fn test_sanitize_tags_within_limits() {
        let tags = vec!["rust".to_string(), "AI".to_string(), "编程".to_string()];
        let result = sanitize_tags(tags);
        assert_eq!(result, vec!["rust", "AI", "编程"]);
    }

    #[test]
    fn test_sanitize_tags_truncates_long_tags() {
        let tags = vec!["这是一个超过十个字符的标签内容".to_string()];
        let result = sanitize_tags(tags);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].chars().count(), MAX_TAG_CHARS);
    }

    #[test]
    fn test_sanitize_tags_limits_to_max() {
        let tags: Vec<String> = (0..10).map(|i| format!("tag{}", i)).collect();
        let result = sanitize_tags(tags);
        assert_eq!(result.len(), MAX_TAGS);
    }

    #[test]
    fn test_sanitize_tags_filters_empty() {
        let tags = vec!["valid".to_string(), "".to_string(), "  ".to_string(), "ok".to_string()];
        let result = sanitize_tags(tags);
        assert_eq!(result, vec!["valid", "ok"]);
    }

    #[test]
    fn test_sanitize_tags_trims_whitespace() {
        let tags = vec!["  rust  ".to_string(), " AI ".to_string()];
        let result = sanitize_tags(tags);
        assert_eq!(result, vec!["rust", "AI"]);
    }

    #[test]
    fn test_merge_topics_adds_new() {
        let existing = vec!["topic-a".to_string()];
        let result = merge_topics(&existing, "topic-b");
        assert_eq!(result, vec!["topic-a", "topic-b"]);
    }

    #[test]
    fn test_merge_topics_no_duplicates() {
        let existing = vec!["topic-a".to_string(), "topic-b".to_string()];
        let result = merge_topics(&existing, "topic-a");
        assert_eq!(result, vec!["topic-a", "topic-b"]);
    }

    #[test]
    fn test_merge_topics_ignores_empty() {
        let existing = vec!["topic-a".to_string()];
        let result = merge_topics(&existing, "");
        assert_eq!(result, vec!["topic-a"]);
    }

    #[test]
    fn test_merge_tags_adds_new() {
        let existing = vec!["tag1".to_string()];
        let new_tags = vec!["tag2".to_string(), "tag3".to_string()];
        let result = merge_tags(&existing, &new_tags);
        assert_eq!(result, vec!["tag1", "tag2", "tag3"]);
    }

    #[test]
    fn test_merge_tags_no_duplicates() {
        let existing = vec!["tag1".to_string(), "tag2".to_string()];
        let new_tags = vec!["tag2".to_string(), "tag3".to_string()];
        let result = merge_tags(&existing, &new_tags);
        assert_eq!(result, vec!["tag1", "tag2", "tag3"]);
    }

    #[test]
    fn test_merge_tags_ignores_empty_and_whitespace() {
        let existing = vec!["tag1".to_string()];
        let new_tags = vec!["".to_string(), "  ".to_string(), "tag2".to_string()];
        let result = merge_tags(&existing, &new_tags);
        assert_eq!(result, vec!["tag1", "tag2"]);
    }

    #[test]
    fn test_constants() {
        assert_eq!(TOPIC_ASSIGN_THRESHOLD, 0.75);
        assert_eq!(CLUSTER_FORM_THRESHOLD, 0.70);
    }

    #[tokio::test]
    async fn test_curator_agent_debug() {
        let agent = CuratorRigAgent::dummy();
        let debug_str = format!("{:?}", agent);
        assert!(debug_str.contains("CuratorRigAgent"));
    }

    #[tokio::test]
    async fn test_curator_agent_clone() {
        let agent = CuratorRigAgent::dummy();
        let cloned = agent.clone();
        assert_eq!(format!("{:?}", agent), format!("{:?}", cloned));
    }

    #[test]
    fn test_classify_result_serialization() {
        let result = ClassifyResult {
            assigned_topic: Some("rust-programming".to_string()),
            suggest_new_topic: false,
            reasoning: "与现有 Rust 主题高度相似".to_string(),
        };
        let json = serde_json::to_string(&result).unwrap();
        let parsed: ClassifyResult = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.assigned_topic, Some("rust-programming".to_string()));
        assert!(!parsed.suggest_new_topic);
    }

    #[test]
    fn test_tag_generation_result_serialization() {
        let result = TagGenerationResult {
            tags: vec!["Rust".to_string(), "编程".to_string(), "系统".to_string()],
        };
        let json = serde_json::to_string(&result).unwrap();
        let parsed: TagGenerationResult = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.tags.len(), 3);
        assert_eq!(parsed.tags[0], "Rust");
    }
}
