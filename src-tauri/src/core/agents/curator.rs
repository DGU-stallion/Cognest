// Cognest Core — Curator Agent
// Fragment classification, topic management, and AI tagging

use std::collections::HashMap;

use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{Agent, AgentError};
use crate::core::embedding::EmbeddingEngine;
use crate::core::frontmatter;
use crate::core::index::FragmentFilter;
use crate::core::jobs::WorkerContext;
use crate::core::llm::{ChatMessage, ChatOptions, Role};
use crate::core::repo::FileRepo;

// ─── Constants ──────────────────────────────────────────────────────────────

/// Minimum cosine similarity between fragment and topic centroid for assignment.
const TOPIC_ASSIGN_THRESHOLD: f32 = 0.75;

/// Minimum average pairwise similarity for forming a new cluster.
const CLUSTER_FORM_THRESHOLD: f32 = 0.70;

/// Minimum uncategorized fragments needed to form a new topic cluster.
const MIN_CLUSTER_SIZE: usize = 5;

/// Maximum characters for a topic title.
const MAX_TITLE_CHARS: usize = 20;

/// Maximum characters for a topic summary.
const MAX_SUMMARY_CHARS: usize = 100;

/// Maximum tag count per fragment.
const MAX_TAGS: usize = 5;

/// Maximum characters per tag.
const MAX_TAG_CHARS: usize = 10;

/// Top-k similar fragments to retrieve.
const TOP_K_SIMILAR: usize = 5;

// ─── Result Types ───────────────────────────────────────────────────────────

/// Result of classifying a single fragment.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClassifyResult {
    pub fragment_id: String,
    pub assigned_topic: Option<String>,
    pub new_topic_created: bool,
    pub tags: Vec<String>,
}

/// Result of recluster operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReclusterResult {
    pub processed: u32,
    pub assigned: u32,
    pub new_topics: u32,
}

// ─── Internal Types ─────────────────────────────────────────────────────────

/// LLM-generated topic metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct TopicGeneration {
    title: String,
    summary: String,
}

/// LLM-generated tags for a fragment.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct TagGeneration {
    tags: Vec<String>,
}

/// Topic file frontmatter.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct TopicMeta {
    r#type: String,
    title: String,
    fragment_count: u32,
    maturity: String,
    created: String,
    updated: String,
}

// ─── CuratorAgent ───────────────────────────────────────────────────────────

/// Curator Agent: handles fragment classification, topic management, and AI tagging.
pub struct CuratorAgent;

impl CuratorAgent {
    /// Classify a single fragment: find similar → assign/create topic → generate tags → update frontmatter.
    pub fn classify_fragment(
        &self,
        fragment_id: &str,
        context: &WorkerContext,
    ) -> Result<ClassifyResult, AgentError> {
        // 1. Read the fragment to get its body content (preserved byte-identical later)
        let original_body = {
            let repo = context.repo.lock()
                .map_err(|e| AgentError::Repo(e.to_string()))?;
            let (_meta, body) = repo.read_fragment(fragment_id)
                .map_err(|e| AgentError::Repo(e.to_string()))?;
            body
        };

        // 2. Get all fragment IDs for similarity search
        let all_fragment_ids = self.get_all_fragment_ids(context)?;

        // 3. Find top-5 similar fragments via EmbeddingEngine
        let similar = {
            let embedding = context.embedding.lock()
                .map_err(|e| AgentError::Repo(e.to_string()))?;
            embedding.find_similar(fragment_id, &all_fragment_ids, TOP_K_SIMILAR)?
        };

        // 4. Try assigning to existing topic (centroid > 0.75)
        let assigned_topic = self.try_assign_to_existing_topic(fragment_id, context)?;

        // 5. If no existing topic matches, try creating a new topic
        let mut new_topic_created = false;
        let final_topic = if let Some(topic) = assigned_topic {
            Some(topic)
        } else {
            match self.try_create_new_topic(fragment_id, context, &similar) {
                Ok(Some(topic)) => {
                    new_topic_created = true;
                    Some(topic)
                }
                Ok(None) => None,
                Err(e) => {
                    log::warn!("创建新 Topic 失败: {}", e);
                    None
                }
            }
        };

        // 6. Generate AI tags via LLM (1-5 tags, each ≤10 chars)
        let tags = self.generate_tags(fragment_id, &original_body, context)?;

        // 7. Update fragment frontmatter (topics, tags), preserving body unchanged
        self.update_fragment_frontmatter(
            fragment_id,
            &final_topic,
            &tags,
            &original_body,
            context,
        )?;

        Ok(ClassifyResult {
            fragment_id: fragment_id.to_string(),
            assigned_topic: final_topic,
            new_topic_created,
            tags,
        })
    }

    /// Re-run classification on all uncategorized fragments.
    pub fn recluster(&self, context: &WorkerContext) -> Result<ReclusterResult, AgentError> {
        let uncategorized = self.get_uncategorized_fragment_ids(context)?;
        let total = uncategorized.len() as u32;
        let mut assigned = 0u32;
        let mut new_topics = 0u32;

        for frag_id in &uncategorized {
            match self.classify_fragment(frag_id, context) {
                Ok(result) => {
                    if result.assigned_topic.is_some() {
                        assigned += 1;
                    }
                    if result.new_topic_created {
                        new_topics += 1;
                    }
                }
                Err(e) => {
                    log::warn!("重聚类碎片 {} 失败: {}", frag_id, e);
                }
            }
        }

        Ok(ReclusterResult {
            processed: total,
            assigned,
            new_topics,
        })
    }

    // ─── Private Helpers ────────────────────────────────────────────────────

    /// Get all fragment IDs from the index.
    fn get_all_fragment_ids(&self, context: &WorkerContext) -> Result<Vec<String>, AgentError> {
        let index = context.index.lock()
            .map_err(|e| AgentError::Index(e.to_string()))?;
        let fragments = index
            .list_fragments(FragmentFilter::All, 0, 10000)
            .map_err(|e| AgentError::Index(e.to_string()))?;
        Ok(fragments.iter().map(|f| f.id.clone()).collect())
    }

    /// Get IDs of fragments that have no topic assigned.
    fn get_uncategorized_fragment_ids(
        &self,
        context: &WorkerContext,
    ) -> Result<Vec<String>, AgentError> {
        let index = context.index.lock()
            .map_err(|e| AgentError::Index(e.to_string()))?;
        let fragments = index
            .list_fragments(FragmentFilter::Uncategorized, 0, 10000)
            .map_err(|e| AgentError::Index(e.to_string()))?;
        Ok(fragments.iter().map(|f| f.id.clone()).collect())
    }

    /// Try to assign the fragment to an existing topic whose centroid similarity > 0.75.
    fn try_assign_to_existing_topic(
        &self,
        fragment_id: &str,
        context: &WorkerContext,
    ) -> Result<Option<String>, AgentError> {
        let topic_fragments = self.get_topic_fragment_map(context)?;
        if topic_fragments.is_empty() {
            return Ok(None);
        }

        let embedding = context.embedding.lock()
            .map_err(|e| AgentError::Repo(e.to_string()))?;
        let fragment_vec = embedding.get_vector(fragment_id)?;

        let mut best_topic: Option<String> = None;
        let mut best_similarity: f32 = 0.0;

        for (topic_slug, frag_ids) in &topic_fragments {
            if frag_ids.is_empty() {
                continue;
            }
            // Collect vectors for all fragments in this topic
            let mut vectors: Vec<Vec<f32>> = Vec::new();
            for fid in frag_ids {
                if let Ok(vec) = embedding.get_vector(fid) {
                    vectors.push(vec);
                }
            }
            if vectors.is_empty() {
                continue;
            }
            // Compute centroid (arithmetic mean of all fragment vectors)
            let centroid = EmbeddingEngine::compute_centroid(&vectors);
            let sim = cosine_sim(&fragment_vec, &centroid);
            if sim > best_similarity {
                best_similarity = sim;
                best_topic = Some(topic_slug.clone());
            }
        }

        if best_similarity > TOPIC_ASSIGN_THRESHOLD {
            Ok(best_topic)
        } else {
            Ok(None)
        }
    }

    /// Build a map of topic_slug → list of fragment IDs assigned to that topic.
    fn get_topic_fragment_map(
        &self,
        context: &WorkerContext,
    ) -> Result<HashMap<String, Vec<String>>, AgentError> {
        let index = context.index.lock()
            .map_err(|e| AgentError::Index(e.to_string()))?;
        let categorized = index
            .list_fragments(FragmentFilter::Categorized, 0, 10000)
            .map_err(|e| AgentError::Index(e.to_string()))?;

        let mut topic_map: HashMap<String, Vec<String>> = HashMap::new();
        for frag in categorized {
            for topic in &frag.topics {
                topic_map
                    .entry(topic.clone())
                    .or_default()
                    .push(frag.id.clone());
            }
        }
        Ok(topic_map)
    }

    /// Try to create a new topic if 5+ uncategorized fragments form a cluster
    /// with avg pairwise similarity > 0.70.
    fn try_create_new_topic(
        &self,
        fragment_id: &str,
        context: &WorkerContext,
        similar: &[(String, f32)],
    ) -> Result<Option<String>, AgentError> {
        let uncategorized = self.get_uncategorized_fragment_ids(context)?;
        if uncategorized.len() < MIN_CLUSTER_SIZE {
            return Ok(None);
        }

        // Build cluster candidates: start with current fragment + similar uncategorized
        let mut cluster: Vec<String> = vec![fragment_id.to_string()];
        for (sim_id, _) in similar {
            if uncategorized.contains(sim_id) && !cluster.contains(sim_id) {
                cluster.push(sim_id.clone());
            }
        }
        // Fill up to MIN_CLUSTER_SIZE from remaining uncategorized
        for uc_id in &uncategorized {
            if cluster.len() >= MIN_CLUSTER_SIZE {
                break;
            }
            if !cluster.contains(uc_id) {
                cluster.push(uc_id.clone());
            }
        }
        if cluster.len() < MIN_CLUSTER_SIZE {
            return Ok(None);
        }

        // Check average pairwise similarity of the first MIN_CLUSTER_SIZE fragments
        let slice = &cluster[..MIN_CLUSTER_SIZE];
        let avg_sim = self.compute_avg_pairwise_similarity(slice, context)?;
        if avg_sim < CLUSTER_FORM_THRESHOLD {
            return Ok(None);
        }

        // Generate topic name and summary via LLM
        let contents = self.get_fragment_contents(slice, context)?;
        let (title, summary) = match self.generate_topic_via_llm(&contents, context) {
            Ok((t, s)) => (t, s),
            Err(e) => {
                // Failure fallback: placeholder title + enqueue retry
                log::warn!("LLM Topic 命名失败: {}, 使用占位标题", e);
                let timestamp = Utc::now().format("%Y%m%d%H%M%S").to_string();
                let placeholder = format!("未命名主题-{}", timestamp);
                (
                    truncate_str(&placeholder, MAX_TITLE_CHARS),
                    "待分类碎片集合".to_string(),
                )
            }
        };

        // Write the topic file
        let slug = generate_slug(&title);
        self.write_topic_file(&slug, &title, &summary, MIN_CLUSTER_SIZE as u32, context)?;

        // Assign all cluster members to the new topic
        for fid in slice {
            if fid != fragment_id {
                let _ = self.add_topic_to_fragment(fid, &slug, context);
            }
        }

        Ok(Some(slug))
    }

    /// Compute average pairwise cosine similarity among a set of fragments.
    fn compute_avg_pairwise_similarity(
        &self,
        fragment_ids: &[String],
        context: &WorkerContext,
    ) -> Result<f32, AgentError> {
        let embedding = context.embedding.lock()
            .map_err(|e| AgentError::Repo(e.to_string()))?;

        let mut total: f32 = 0.0;
        let mut count: u32 = 0;

        for i in 0..fragment_ids.len() {
            for j in (i + 1)..fragment_ids.len() {
                if let Ok(sim) = embedding.cosine_similarity(&fragment_ids[i], &fragment_ids[j]) {
                    total += sim;
                    count += 1;
                }
            }
        }

        if count == 0 {
            Ok(0.0)
        } else {
            Ok(total / count as f32)
        }
    }

    /// Get body contents for a set of fragment IDs.
    fn get_fragment_contents(
        &self,
        fragment_ids: &[String],
        context: &WorkerContext,
    ) -> Result<Vec<String>, AgentError> {
        let repo = context.repo.lock()
            .map_err(|e| AgentError::Repo(e.to_string()))?;
        let mut contents = Vec::new();
        for id in fragment_ids {
            if let Ok((_meta, body)) = repo.read_fragment(id) {
                contents.push(body);
            }
        }
        Ok(contents)
    }

    /// Call LLM to generate topic title (≤20 chars) and summary (≤100 chars).
    fn generate_topic_via_llm(
        &self,
        fragment_contents: &[String],
        context: &WorkerContext,
    ) -> Result<(String, String), AgentError> {
        let combined = fragment_contents.join("\n---\n");
        let truncated = truncate_str(&combined, 2000);

        let messages = vec![
            ChatMessage {
                role: Role::System,
                content: "你是一个知识分类助手。根据以下碎片内容，生成一个主题标题和摘要。\
                    标题不超过20个字符，摘要不超过100个字符。\
                    以 JSON 格式返回: {\"title\": \"...\", \"summary\": \"...\"}"
                    .to_string(),
            },
            ChatMessage {
                role: Role::User,
                content: format!("碎片内容:\n{}", truncated),
            },
        ];

        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "title": { "type": "string", "maxLength": 20 },
                "summary": { "type": "string", "maxLength": 100 }
            },
            "required": ["title", "summary"]
        });

        let options = ChatOptions {
            json_schema: Some(schema),
            temperature: Some(0.3),
            ..ChatOptions::default()
        };

        let llm = context.llm.lock()
            .map_err(|e| AgentError::Repo(e.to_string()))?;
        let response = llm.chat_for_agent("curator", &messages, &options)?;

        let gen: TopicGeneration = serde_json::from_str(&response.content)
            .map_err(|e| AgentError::Repo(format!("LLM 返回解析失败: {}", e)))?;

        Ok((
            truncate_str(&gen.title, MAX_TITLE_CHARS),
            truncate_str(&gen.summary, MAX_SUMMARY_CHARS),
        ))
    }

    /// Generate 1-5 AI tags for a fragment via LLM.
    fn generate_tags(
        &self,
        _fragment_id: &str,
        body: &str,
        context: &WorkerContext,
    ) -> Result<Vec<String>, AgentError> {
        let truncated = truncate_str(body, 2000);

        let messages = vec![
            ChatMessage {
                role: Role::System,
                content: "你是一个知识标签生成助手。根据以下碎片内容生成1-5个标签。\
                    每个标签不超过10个字符。\
                    以 JSON 格式返回: {\"tags\": [\"标签1\", \"标签2\"]}"
                    .to_string(),
            },
            ChatMessage {
                role: Role::User,
                content: format!("碎片内容:\n{}", truncated),
            },
        ];

        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "tags": {
                    "type": "array",
                    "items": { "type": "string", "maxLength": 10 },
                    "minItems": 1,
                    "maxItems": 5
                }
            },
            "required": ["tags"]
        });

        let options = ChatOptions {
            json_schema: Some(schema),
            temperature: Some(0.3),
            ..ChatOptions::default()
        };

        let llm = context.llm.lock()
            .map_err(|e| AgentError::Repo(e.to_string()))?;

        match llm.chat_for_agent("curator", &messages, &options) {
            Ok(response) => {
                let gen: TagGeneration = serde_json::from_str(&response.content)
                    .unwrap_or(TagGeneration { tags: vec![] });
                let tags: Vec<String> = gen
                    .tags
                    .into_iter()
                    .take(MAX_TAGS)
                    .map(|t| truncate_str(&t, MAX_TAG_CHARS))
                    .collect();
                Ok(tags)
            }
            Err(e) => {
                log::warn!("LLM 标签生成失败: {}, 返回空标签", e);
                Ok(vec![])
            }
        }
    }

    /// Write a new topic file to `topics/<slug>.md`.
    fn write_topic_file(
        &self,
        slug: &str,
        title: &str,
        summary: &str,
        fragment_count: u32,
        context: &WorkerContext,
    ) -> Result<(), AgentError> {
        let now = Utc::now().to_rfc3339();
        let meta = TopicMeta {
            r#type: "Topic".to_string(),
            title: title.to_string(),
            fragment_count,
            maturity: "seed".to_string(),
            created: now.clone(),
            updated: now,
        };

        let body = format!("{}\n", summary);
        let document = frontmatter::serialize(&meta, &body)
            .map_err(|e| AgentError::Repo(e.to_string()))?;

        let repo = context.repo.lock()
            .map_err(|e| AgentError::Repo(e.to_string()))?;
        let topics_dir = repo.vault_path().join("topics");
        std::fs::create_dir_all(&topics_dir)
            .map_err(|e| AgentError::Repo(e.to_string()))?;

        let file_path = topics_dir.join(format!("{}.md", slug));
        std::fs::write(&file_path, &document)
            .map_err(|e| AgentError::Repo(e.to_string()))?;

        Ok(())
    }

    /// Update a fragment's frontmatter with topic and tags, preserving body byte-identical.
    fn update_fragment_frontmatter(
        &self,
        fragment_id: &str,
        topic: &Option<String>,
        tags: &[String],
        original_body: &str,
        context: &WorkerContext,
    ) -> Result<(), AgentError> {
        let repo = context.repo.lock()
            .map_err(|e| AgentError::Repo(e.to_string()))?;

        let (mut meta, _body) = repo.read_fragment(fragment_id)
            .map_err(|e| AgentError::Repo(e.to_string()))?;

        // Update topics field
        if let Some(ref topic_slug) = topic {
            if !meta.topics.contains(topic_slug) {
                meta.topics.push(topic_slug.clone());
            }
        }

        // Update tags (merge, no duplicates)
        for tag in tags {
            if !meta.tags.contains(tag) {
                meta.tags.push(tag.clone());
            }
        }

        // Re-serialize with original body preserved byte-identical
        let document = frontmatter::serialize(&meta, original_body)
            .map_err(|e| AgentError::Repo(e.to_string()))?;

        let capture_dir = repo.vault_path().join("capture");
        let file_path = find_fragment_file(&capture_dir, fragment_id)
            .ok_or_else(|| AgentError::Repo(format!("碎片文件未找到: {}", fragment_id)))?;

        std::fs::write(&file_path, &document)
            .map_err(|e| AgentError::Repo(e.to_string()))?;

        // Update the index record to stay in sync
        drop(repo);
        let index = context.index.lock()
            .map_err(|e| AgentError::Index(e.to_string()))?;

        let record = crate::core::index::FragmentRecord {
            id: meta.id.clone(),
            content: original_body.to_string(),
            created_at: meta.created.to_rfc3339(),
            source: meta.source.clone(),
            tags: meta.tags.clone(),
            topics: meta.topics.clone(),
            content_hash: FileRepo::content_hash(original_body.as_bytes()),
        };
        let _ = index.update_fragment(&record);

        Ok(())
    }

    /// Add a topic slug to a fragment's frontmatter (used when creating new topics
    /// to assign all cluster members).
    fn add_topic_to_fragment(
        &self,
        fragment_id: &str,
        topic_slug: &str,
        context: &WorkerContext,
    ) -> Result<(), AgentError> {
        let repo = context.repo.lock()
            .map_err(|e| AgentError::Repo(e.to_string()))?;

        let (mut meta, body) = repo.read_fragment(fragment_id)
            .map_err(|e| AgentError::Repo(e.to_string()))?;

        if !meta.topics.contains(&topic_slug.to_string()) {
            meta.topics.push(topic_slug.to_string());
        }

        let document = frontmatter::serialize(&meta, &body)
            .map_err(|e| AgentError::Repo(e.to_string()))?;

        let capture_dir = repo.vault_path().join("capture");
        let file_path = find_fragment_file(&capture_dir, fragment_id)
            .ok_or_else(|| AgentError::Repo(format!("碎片文件未找到: {}", fragment_id)))?;

        std::fs::write(&file_path, &document)
            .map_err(|e| AgentError::Repo(e.to_string()))?;

        Ok(())
    }
}

// ─── Agent Trait Implementation ─────────────────────────────────────────────

impl Agent for CuratorAgent {
    fn name(&self) -> &str {
        "curator"
    }

    fn execute(&self, payload: &Value, context: &WorkerContext) -> Result<Value, AgentError> {
        let job_type = payload
            .get("job_type")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        match job_type {
            "curator_classify" => {
                let fragment_id = payload
                    .get("fragment_id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        AgentError::InvalidPayload("缺少 fragment_id 字段".to_string())
                    })?;
                let result = self.classify_fragment(fragment_id, context)?;
                serde_json::to_value(&result).map_err(|e| AgentError::Repo(e.to_string()))
            }
            "curator_recluster" => {
                let result = self.recluster(context)?;
                serde_json::to_value(&result).map_err(|e| AgentError::Repo(e.to_string()))
            }
            _ => Err(AgentError::InvalidPayload(format!(
                "未知 Curator job_type: {}",
                job_type
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
        return 0.0;
    }
    (dot / (norm_a * norm_b)).clamp(-1.0, 1.0)
}

/// Generate a URL-safe slug from a title.
fn generate_slug(title: &str) -> String {
    let slug: String = title
        .chars()
        .map(|c| {
            if c.is_alphanumeric() {
                c.to_lowercase().next().unwrap_or(c)
            } else {
                '-'
            }
        })
        .collect();

    // Collapse consecutive dashes and trim trailing dashes
    let mut result = String::new();
    let mut prev_dash = false;
    for c in slug.chars() {
        if c == '-' {
            if !prev_dash && !result.is_empty() {
                result.push('-');
            }
            prev_dash = true;
        } else {
            result.push(c);
            prev_dash = false;
        }
    }

    let trimmed = result.trim_end_matches('-').to_string();
    if trimmed.is_empty() {
        format!("topic-{}", Utc::now().timestamp())
    } else {
        trimmed
    }
}

/// Truncate a string to max characters (char-aware for CJK).
fn truncate_str(s: &str, max_chars: usize) -> String {
    s.chars().take(max_chars).collect()
}

/// Recursively find a fragment file by ID under the given directory.
fn find_fragment_file(dir: &std::path::Path, fragment_id: &str) -> Option<std::path::PathBuf> {
    let filename = format!("{}.md", fragment_id);
    find_file_recursive(dir, &filename)
}

fn find_file_recursive(dir: &std::path::Path, filename: &str) -> Option<std::path::PathBuf> {
    let entries = std::fs::read_dir(dir).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if let Some(found) = find_file_recursive(&path, filename) {
                return Some(found);
            }
        } else if path.file_name().and_then(|n| n.to_str()) == Some(filename) {
            return Some(path);
        }
    }
    None
}
