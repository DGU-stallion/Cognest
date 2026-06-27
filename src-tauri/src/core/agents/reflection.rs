// Cognest Core — Reflection Agent
// Daily/weekly review card generation and scheduling

use std::collections::HashMap;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use chrono::{Datelike, Local, NaiveTime, Weekday};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{Agent, AgentError};
use crate::core::index::FragmentFilter;
use crate::core::jobs::{JobQueue, JobType, WorkerContext};
use crate::core::llm::{ChatMessage, ChatOptions, Role};

// ─── Constants ──────────────────────────────────────────────────────────────

/// Maximum characters for daily AI insight.
const MAX_DAILY_INSIGHT_CHARS: usize = 150;

/// Maximum characters for weekly AI insight.
const MAX_WEEKLY_INSIGHT_CHARS: usize = 400;

/// Scheduled review hour (22:00 local time).
const REVIEW_HOUR: u32 = 22;

/// Scheduler poll interval (check every 30 seconds).
const SCHEDULER_POLL_INTERVAL: Duration = Duration::from_secs(30);

// ─── ViewSpec ───────────────────────────────────────────────────────────────

/// View specification for generated views (summary type for reflection).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ViewSpec {
    pub id: String,
    #[serde(rename = "type")]
    pub view_type: String,
    pub title: String,
    pub query: String,
    pub created: String,
    pub pinned: bool,
    pub config: serde_json::Value,
    pub data: ViewData,
    /// Whether the card is partial (stats-only, no AI insight).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub partial: Option<bool>,
}

/// Data payload for summary-type views.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ViewData {
    pub markdown: String,
    pub stats: serde_json::Value,
}

// ─── ReflectionAgent ────────────────────────────────────────────────────────

/// Reflection Agent: generates periodic daily/weekly review cards.
pub struct ReflectionAgent;

impl ReflectionAgent {
    /// Generate a daily review card.
    ///
    /// Queries fragments created today, counts active topics,
    /// generates AI insight (≤150 chars), saves ViewSpec to views/.
    pub fn daily_review(&self, context: &WorkerContext) -> Result<ViewSpec, AgentError> {
        let today = Local::now().date_naive();
        let today_str = today.format("%Y-%m-%d").to_string();
        let view_id = format!("review-daily-{}", today_str);

        // Query all fragments and filter those created today
        let (fragments_today, active_topics) = self.get_daily_stats(&today_str, context)?;
        let fragments_count = fragments_today.len();
        let active_topic_names: Vec<String> = active_topics.keys().cloned().collect();

        // Try to generate AI insight via LLM
        let (insight, is_partial) = match self.generate_daily_insight(
            fragments_count,
            &active_topic_names,
            &fragments_today,
            context,
        ) {
            Ok(insight) => (insight, false),
            Err(e) => {
                log::warn!("每日回顾 LLM 生成失败: {}, 使用纯统计卡片", e);
                (String::new(), true)
            }
        };

        // Build markdown content
        let topics_display = if active_topic_names.is_empty() {
            "无".to_string()
        } else {
            active_topic_names.join(", ")
        };

        let mut markdown = format!(
            "## 今日概览\n- 新增碎片: {}\n- 活跃主题: {}\n",
            fragments_count, topics_display
        );
        if !insight.is_empty() {
            markdown.push_str(&format!("\n### AI 洞察\n{}\n", insight));
        }

        let stats = serde_json::json!({
            "fragments_count": fragments_count,
            "active_topics": active_topic_names.len()
        });

        let now = Local::now().to_rfc3339();
        let view_spec = ViewSpec {
            id: view_id.clone(),
            view_type: "summary".to_string(),
            title: format!("每日回顾 - {}", today_str),
            query: String::new(),
            created: now,
            pinned: false,
            config: serde_json::json!({}),
            data: ViewData { markdown, stats },
            partial: if is_partial { Some(true) } else { None },
        };

        // Save ViewSpec to views/ directory
        self.save_view_spec(&view_spec, context)?;

        // Emit new_feed_card event
        self.emit_new_feed_card(&view_spec, context)?;

        Ok(view_spec)
    }

    /// Generate a weekly review card.
    ///
    /// Queries fragments from Monday 00:00 to Sunday 22:00,
    /// finds new topics, top-3 active topics, generates AI insight (≤400 chars).
    pub fn weekly_review(&self, context: &WorkerContext) -> Result<ViewSpec, AgentError> {
        let today = Local::now().date_naive();
        let (year, week) = (today.iso_week().year(), today.iso_week().week());
        let view_id = format!("review-weekly-{}-W{:02}", year, week);

        // Calculate week start (Monday 00:00) as ISO date string
        let days_since_monday = today.weekday().num_days_from_monday();
        let monday = today - chrono::Duration::days(days_since_monday as i64);
        let monday_str = monday.format("%Y-%m-%d").to_string();
        let today_str = today.format("%Y-%m-%d").to_string();

        // Query weekly stats
        let (fragments_week, topic_activity, new_topics) =
            self.get_weekly_stats(&monday_str, &today_str, context)?;
        let fragments_count = fragments_week.len();

        // Top-3 most active topics (by new fragment linkages this week)
        let mut topic_vec: Vec<(String, usize)> = topic_activity.into_iter().collect();
        topic_vec.sort_by(|a, b| b.1.cmp(&a.1));
        let top_3: Vec<String> = topic_vec.iter().take(3).map(|(k, _)| k.clone()).collect();

        // Try to generate AI insight via LLM
        let (insight, is_partial) = match self.generate_weekly_insight(
            fragments_count,
            &new_topics,
            &top_3,
            &fragments_week,
            context,
        ) {
            Ok(insight) => (insight, false),
            Err(e) => {
                log::warn!("每周回顾 LLM 生成失败: {}, 使用纯统计卡片", e);
                (String::new(), true)
            }
        };

        // Build markdown content
        let top_3_display = if top_3.is_empty() {
            "无".to_string()
        } else {
            top_3.join(", ")
        };

        let mut markdown = format!(
            "## 本周概览\n- 新增碎片: {}\n- 新建主题: {}\n- Top-3 活跃主题: {}\n",
            fragments_count,
            new_topics.len(),
            top_3_display
        );
        if !insight.is_empty() {
            markdown.push_str(&format!("\n### AI 洞察\n{}\n", insight));
        }

        let stats = serde_json::json!({
            "fragments_count": fragments_count,
            "new_topics": new_topics.len(),
            "top_active_topics": top_3
        });

        let now = Local::now().to_rfc3339();
        let view_spec = ViewSpec {
            id: view_id.clone(),
            view_type: "summary".to_string(),
            title: format!("每周回顾 - {}-W{:02}", year, week),
            query: String::new(),
            created: now,
            pinned: false,
            config: serde_json::json!({}),
            data: ViewData { markdown, stats },
            partial: if is_partial { Some(true) } else { None },
        };

        // Save ViewSpec to views/ directory
        self.save_view_spec(&view_spec, context)?;

        // Emit new_feed_card event
        self.emit_new_feed_card(&view_spec, context)?;

        Ok(view_spec)
    }

    /// Check if any reviews were missed (called on app startup).
    ///
    /// - Check if today's daily review exists in views/
    /// - Check if this week's weekly review exists (if today is Sunday or later)
    /// - Returns list of missing review JobTypes to enqueue
    pub fn check_missed_reviews(
        &self,
        context: &WorkerContext,
    ) -> Result<Vec<JobType>, AgentError> {
        let mut missing = Vec::new();
        let today = Local::now().date_naive();
        let today_str = today.format("%Y-%m-%d").to_string();

        // Check daily review
        let daily_id = format!("review-daily-{}", today_str);
        if !self.view_exists(&daily_id, context)? {
            missing.push(JobType::ReflectionDaily);
        }

        // Check weekly review (if today is Sunday)
        if today.weekday() == Weekday::Sun {
            let (year, week) = (today.iso_week().year(), today.iso_week().week());
            let weekly_id = format!("review-weekly-{}-W{:02}", year, week);
            if !self.view_exists(&weekly_id, context)? {
                missing.push(JobType::ReflectionWeekly);
            }
        }

        Ok(missing)
    }

    // ─── Private Helpers ────────────────────────────────────────────────────

    /// Get fragments created today and active topics (topics that received new fragments today).
    fn get_daily_stats(
        &self,
        today_str: &str,
        context: &WorkerContext,
    ) -> Result<(Vec<String>, HashMap<String, usize>), AgentError> {
        let index = context.index.lock()
            .map_err(|e| AgentError::Index(e.to_string()))?;

        let all_fragments = index
            .list_fragments(FragmentFilter::All, 0, 10000)
            .map_err(|e| AgentError::Index(e.to_string()))?;

        let mut fragments_today: Vec<String> = Vec::new();
        let mut active_topics: HashMap<String, usize> = HashMap::new();

        for frag in &all_fragments {
            // Check if created_at starts with today's date (YYYY-MM-DD)
            if frag.created_at.starts_with(today_str) {
                fragments_today.push(frag.content.clone());
                // Count active topics
                for topic in &frag.topics {
                    *active_topics.entry(topic.clone()).or_insert(0) += 1;
                }
            }
        }

        Ok((fragments_today, active_topics))
    }

    /// Get weekly stats: fragments this week, topic activity map, new topics created.
    fn get_weekly_stats(
        &self,
        monday_str: &str,
        today_str: &str,
        context: &WorkerContext,
    ) -> Result<(Vec<String>, HashMap<String, usize>, Vec<String>), AgentError> {
        let index = context.index.lock()
            .map_err(|e| AgentError::Index(e.to_string()))?;

        let all_fragments = index
            .list_fragments(FragmentFilter::All, 0, 10000)
            .map_err(|e| AgentError::Index(e.to_string()))?;

        let mut fragments_week: Vec<String> = Vec::new();
        let mut topic_activity: HashMap<String, usize> = HashMap::new();

        for frag in &all_fragments {
            // Check if created_at date is within [monday_str, today_str]
            let frag_date = &frag.created_at[..10]; // extract YYYY-MM-DD
            if frag_date >= monday_str && frag_date <= today_str {
                fragments_week.push(frag.content.clone());
                for topic in &frag.topics {
                    *topic_activity.entry(topic.clone()).or_insert(0) += 1;
                }
            }
        }

        // Detect new topics created this week by checking topic files
        let new_topics = self.find_new_topics_this_week(monday_str, today_str, context)?;

        Ok((fragments_week, topic_activity, new_topics))
    }

    /// Find topics created this week by scanning the topics/ directory.
    fn find_new_topics_this_week(
        &self,
        monday_str: &str,
        today_str: &str,
        context: &WorkerContext,
    ) -> Result<Vec<String>, AgentError> {
        let repo = context.repo.lock()
            .map_err(|e| AgentError::Repo(e.to_string()))?;
        let topics_dir = repo.vault_path().join("topics");

        let mut new_topics = Vec::new();
        if !topics_dir.exists() {
            return Ok(new_topics);
        }

        let entries = std::fs::read_dir(&topics_dir)
            .map_err(|e| AgentError::Repo(e.to_string()))?;

        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("md") {
                continue;
            }
            // Read file metadata for creation time, or parse frontmatter
            let content = std::fs::read_to_string(&path)
                .unwrap_or_default();
            // Try to extract "created" field from frontmatter
            if let Some(created) = extract_frontmatter_field(&content, "created") {
                let created_date = &created[..10.min(created.len())];
                if created_date >= monday_str && created_date <= today_str {
                    if let Some(title) = extract_frontmatter_field(&content, "title") {
                        new_topics.push(title);
                    } else if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                        new_topics.push(stem.to_string());
                    }
                }
            }
        }

        Ok(new_topics)
    }

    /// Generate daily AI insight via LLM (≤150 chars).
    fn generate_daily_insight(
        &self,
        fragments_count: usize,
        active_topics: &[String],
        fragment_contents: &[String],
        context: &WorkerContext,
    ) -> Result<String, AgentError> {
        // Build a summary of today's content for the LLM
        let content_summary: String = fragment_contents
            .iter()
            .take(10)
            .map(|c| truncate_str(c, 200))
            .collect::<Vec<_>>()
            .join("\n---\n");

        let topics_str = if active_topics.is_empty() {
            "无".to_string()
        } else {
            active_topics.join(", ")
        };

        let messages = vec![
            ChatMessage {
                role: Role::System,
                content: "你是 Cognest 知识管理系统的回顾助手。\
                    根据用户今天记录的碎片内容，生成一句简洁的洞察总结。\
                    回复不超过150个字符，直接输出洞察内容，不要加前缀。".to_string(),
            },
            ChatMessage {
                role: Role::User,
                content: format!(
                    "今日统计：新增碎片 {} 条，活跃主题：{}\n\n碎片内容摘要:\n{}",
                    fragments_count, topics_str, content_summary
                ),
            },
        ];

        let options = ChatOptions {
            temperature: Some(0.7),
            max_tokens: Some(100),
            ..ChatOptions::default()
        };

        let llm = context.llm.lock()
            .map_err(|e| AgentError::Repo(e.to_string()))?;
        let response = llm.chat_for_agent("reflection", &messages, &options)?;

        Ok(truncate_str(&response.content, MAX_DAILY_INSIGHT_CHARS))
    }

    /// Generate weekly AI insight via LLM (≤400 chars).
    fn generate_weekly_insight(
        &self,
        fragments_count: usize,
        new_topics: &[String],
        top_3_topics: &[String],
        fragment_contents: &[String],
        context: &WorkerContext,
    ) -> Result<String, AgentError> {
        let content_summary: String = fragment_contents
            .iter()
            .take(20)
            .map(|c| truncate_str(c, 150))
            .collect::<Vec<_>>()
            .join("\n---\n");

        let new_topics_str = if new_topics.is_empty() {
            "无".to_string()
        } else {
            new_topics.join(", ")
        };

        let top_3_str = if top_3_topics.is_empty() {
            "无".to_string()
        } else {
            top_3_topics.join(", ")
        };

        let messages = vec![
            ChatMessage {
                role: Role::System,
                content: "你是 Cognest 知识管理系统的回顾助手。\
                    根据用户本周记录的碎片内容，生成2-3句关于知识增长方向的洞察。\
                    回复不超过400个字符，直接输出洞察内容，不要加前缀。".to_string(),
            },
            ChatMessage {
                role: Role::User,
                content: format!(
                    "本周统计：新增碎片 {} 条，新建主题：{}，Top-3 活跃主题：{}\n\n碎片内容摘要:\n{}",
                    fragments_count, new_topics_str, top_3_str, content_summary
                ),
            },
        ];

        let options = ChatOptions {
            temperature: Some(0.7),
            max_tokens: Some(250),
            ..ChatOptions::default()
        };

        let llm = context.llm.lock()
            .map_err(|e| AgentError::Repo(e.to_string()))?;
        let response = llm.chat_for_agent("reflection", &messages, &options)?;

        Ok(truncate_str(&response.content, MAX_WEEKLY_INSIGHT_CHARS))
    }

    /// Save a ViewSpec JSON file to `<vault_path>/views/<id>.json`.
    fn save_view_spec(
        &self,
        view_spec: &ViewSpec,
        context: &WorkerContext,
    ) -> Result<(), AgentError> {
        let repo = context.repo.lock()
            .map_err(|e| AgentError::Repo(e.to_string()))?;
        let views_dir = repo.vault_path().join("views");
        std::fs::create_dir_all(&views_dir)
            .map_err(|e| AgentError::Repo(e.to_string()))?;

        let file_path = views_dir.join(format!("{}.json", view_spec.id));
        let json = serde_json::to_string_pretty(view_spec)
            .map_err(|e| AgentError::Repo(e.to_string()))?;
        std::fs::write(&file_path, &json)
            .map_err(|e| AgentError::Repo(e.to_string()))?;

        Ok(())
    }

    /// Check if a view file already exists.
    fn view_exists(
        &self,
        view_id: &str,
        context: &WorkerContext,
    ) -> Result<bool, AgentError> {
        let repo = context.repo.lock()
            .map_err(|e| AgentError::Repo(e.to_string()))?;
        let views_dir = repo.vault_path().join("views");
        let file_path = views_dir.join(format!("{}.json", view_id));
        Ok(file_path.exists())
    }

    /// Emit the `new_feed_card` event with the ViewSpec as JSON payload.
    fn emit_new_feed_card(
        &self,
        view_spec: &ViewSpec,
        context: &WorkerContext,
    ) -> Result<(), AgentError> {
        // The event emission happens through the job queue's emitter.
        // Since we don't have direct access to the emitter here, we store
        // the view_spec in the job result which will be emitted by the worker.
        // The actual event emission is handled by returning the result to the
        // job queue worker, which emits via the stored event_emitter callback.
        // We simply ensure the data is available for emission in the execute() return.
        let _ = view_spec; // Event emission handled by job result in execute()
        Ok(())
    }
}

// ─── Agent Trait Implementation ─────────────────────────────────────────────

impl Agent for ReflectionAgent {
    fn name(&self) -> &str {
        "reflection"
    }

    fn execute(&self, payload: &Value, context: &WorkerContext) -> Result<Value, AgentError> {
        let job_type = payload
            .get("job_type")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        match job_type {
            "reflection_daily" => {
                let view_spec = self.daily_review(context)?;
                // The job result includes the view_spec so the worker can emit
                // the new_feed_card event after job completion.
                let mut result = serde_json::to_value(&view_spec)
                    .map_err(|e| AgentError::Repo(e.to_string()))?;
                // Add event hint for the worker to emit
                if let Some(obj) = result.as_object_mut() {
                    obj.insert(
                        "_emit_event".to_string(),
                        serde_json::json!("new_feed_card"),
                    );
                }
                Ok(result)
            }
            "reflection_weekly" => {
                let view_spec = self.weekly_review(context)?;
                let mut result = serde_json::to_value(&view_spec)
                    .map_err(|e| AgentError::Repo(e.to_string()))?;
                if let Some(obj) = result.as_object_mut() {
                    obj.insert(
                        "_emit_event".to_string(),
                        serde_json::json!("new_feed_card"),
                    );
                }
                Ok(result)
            }
            _ => Err(AgentError::InvalidPayload(format!(
                "未知 Reflection job_type: {}",
                job_type
            ))),
        }
    }
}

// ─── ReflectionScheduler ────────────────────────────────────────────────────

/// Scheduler that runs in an independent thread.
/// Sleeps until 22:00 local time, then enqueues daily (and weekly on Sundays) review jobs.
pub struct ReflectionScheduler {
    queue: Arc<JobQueue>,
}

impl ReflectionScheduler {
    /// Create a new ReflectionScheduler with the given job queue.
    pub fn new(queue: Arc<JobQueue>) -> Self {
        Self { queue }
    }

    /// Start the scheduler in an independent thread.
    /// The thread sleeps until 22:00 local time each day, then:
    /// - Enqueues a `reflection_daily` job
    /// - On Sundays, also enqueues a `reflection_weekly` job
    pub fn start(self) {
        thread::spawn(move || {
            log::info!("ReflectionScheduler started");
            loop {
                // Calculate sleep duration until next 22:00
                let sleep_duration = self.duration_until_next_review();
                log::info!(
                    "ReflectionScheduler sleeping for {:?} until next 22:00",
                    sleep_duration
                );
                thread::sleep(sleep_duration);

                // Enqueue daily review
                let daily_payload = serde_json::json!({
                    "job_type": "reflection_daily"
                });
                match self.queue.enqueue(JobType::ReflectionDaily, daily_payload) {
                    Ok(id) => log::info!("已入队每日回顾 job: {}", id),
                    Err(e) => log::error!("入队每日回顾失败: {}", e),
                }

                // Check if today is Sunday → also enqueue weekly review
                let now = Local::now();
                if now.weekday() == Weekday::Sun {
                    let weekly_payload = serde_json::json!({
                        "job_type": "reflection_weekly"
                    });
                    match self.queue.enqueue(JobType::ReflectionWeekly, weekly_payload) {
                        Ok(id) => log::info!("已入队每周回顾 job: {}", id),
                        Err(e) => log::error!("入队每周回顾失败: {}", e),
                    }
                }

                // Sleep briefly to avoid re-triggering in the same minute
                thread::sleep(Duration::from_secs(60));
            }
        });
    }

    /// Calculate the duration from now until the next 22:00 local time.
    fn duration_until_next_review(&self) -> Duration {
        let now = Local::now();
        let today_review_time = now
            .date_naive()
            .and_time(NaiveTime::from_hms_opt(REVIEW_HOUR, 0, 0).unwrap());
        let today_review = today_review_time
            .and_local_timezone(Local)
            .single();

        let target = match today_review {
            Some(t) if t > now => t,
            _ => {
                // Already past 22:00 today, schedule for tomorrow
                let tomorrow = now.date_naive() + chrono::Duration::days(1);
                let tomorrow_time = tomorrow
                    .and_time(NaiveTime::from_hms_opt(REVIEW_HOUR, 0, 0).unwrap());
                tomorrow_time
                    .and_local_timezone(Local)
                    .single()
                    .unwrap_or(now + chrono::Duration::days(1))
            }
        };

        let diff = target - now;
        diff.to_std().unwrap_or(SCHEDULER_POLL_INTERVAL)
    }
}

// ─── Utility Functions ──────────────────────────────────────────────────────

/// Truncate a string to max characters (char-aware for CJK).
fn truncate_str(s: &str, max_chars: usize) -> String {
    s.chars().take(max_chars).collect()
}

/// Extract a value from YAML-style frontmatter by field name.
/// Simple parser that looks for `field: value` patterns.
fn extract_frontmatter_field(content: &str, field: &str) -> Option<String> {
    // Frontmatter is between first "---" and second "---"
    let parts: Vec<&str> = content.splitn(3, "---").collect();
    if parts.len() < 3 {
        return None;
    }
    let frontmatter = parts[1];
    let pattern = format!("{}:", field);
    for line in frontmatter.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with(&pattern) {
            let value = trimmed[pattern.len()..].trim();
            // Remove surrounding quotes if present
            let value = value.trim_matches('"').trim_matches('\'');
            return Some(value.to_string());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_truncate_str() {
        assert_eq!(truncate_str("hello world", 5), "hello");
        assert_eq!(truncate_str("你好世界", 2), "你好");
        assert_eq!(truncate_str("short", 100), "short");
    }

    #[test]
    fn test_extract_frontmatter_field() {
        let content = "---\ntype: Topic\ntitle: AI研究\ncreated: 2024-01-15T10:00:00Z\n---\nBody";
        assert_eq!(
            extract_frontmatter_field(content, "title"),
            Some("AI研究".to_string())
        );
        assert_eq!(
            extract_frontmatter_field(content, "created"),
            Some("2024-01-15T10:00:00Z".to_string())
        );
        assert_eq!(extract_frontmatter_field(content, "missing"), None);
    }

    #[test]
    fn test_extract_frontmatter_field_with_quotes() {
        let content = "---\ntitle: \"Rust 编程\"\ncreated: '2024-02-01'\n---\nBody";
        assert_eq!(
            extract_frontmatter_field(content, "title"),
            Some("Rust 编程".to_string())
        );
        assert_eq!(
            extract_frontmatter_field(content, "created"),
            Some("2024-02-01".to_string())
        );
    }

    #[test]
    fn test_view_spec_serialization() {
        let spec = ViewSpec {
            id: "review-daily-2024-01-15".to_string(),
            view_type: "summary".to_string(),
            title: "每日回顾 - 2024-01-15".to_string(),
            query: String::new(),
            created: "2024-01-15T22:00:00+08:00".to_string(),
            pinned: false,
            config: serde_json::json!({}),
            data: ViewData {
                markdown: "## 今日概览\n- 新增碎片: 5\n".to_string(),
                stats: serde_json::json!({"fragments_count": 5, "active_topics": 2}),
            },
            partial: None,
        };

        let json = serde_json::to_string(&spec).unwrap();
        assert!(json.contains("\"type\":\"summary\""));
        assert!(json.contains("review-daily-2024-01-15"));
        // partial should not appear when None
        assert!(!json.contains("partial"));
    }

    #[test]
    fn test_view_spec_partial_serialization() {
        let spec = ViewSpec {
            id: "review-daily-2024-01-15".to_string(),
            view_type: "summary".to_string(),
            title: "每日回顾 - 2024-01-15".to_string(),
            query: String::new(),
            created: "2024-01-15T22:00:00+08:00".to_string(),
            pinned: false,
            config: serde_json::json!({}),
            data: ViewData {
                markdown: "## 今日概览\n- 新增碎片: 3\n".to_string(),
                stats: serde_json::json!({"fragments_count": 3, "active_topics": 1}),
            },
            partial: Some(true),
        };

        let json = serde_json::to_string(&spec).unwrap();
        assert!(json.contains("\"partial\":true"));
    }
}
