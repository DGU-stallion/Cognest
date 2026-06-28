// Cognest Core — Reflection Agent
// Daily/weekly review card generation and scheduling
//
// Migrated from core/agents/reflection.rs — the Agent trait is removed.
// LLM-powered insight generation is now handled via rig_agents layer.
// The old LlmGateway path has been removed; ReflectionAgent generates
// stats-only cards when no async LLM call path is available (graceful degradation).

use std::collections::HashMap;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use chrono::{Datelike, Local, NaiveTime, Weekday};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::core::index::FragmentFilter;
use crate::core::jobs::{JobQueue, JobType, WorkerContext};

// ─── Constants ──────────────────────────────────────────────────────────────

/// Scheduled review hour (22:00 local time).
const REVIEW_HOUR: u32 = 22;

/// Scheduler poll interval (check every 30 seconds).
const SCHEDULER_POLL_INTERVAL: Duration = Duration::from_secs(30);

// ─── Error Type ─────────────────────────────────────────────────────────────

/// Reflection agent errors.
#[derive(Debug, thiserror::Error)]
pub enum ReflectionError {
    #[error("文件操作错误: {0}")]
    Repo(String),

    #[error("索引错误: {0}")]
    Index(String),

    #[error("LLM 错误: {0}")]
    Llm(String),

    #[error("无效参数: {0}")]
    InvalidPayload(String),
}

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
    /// Execute a reflection job (called by the worker thread).
    pub fn execute(
        &self,
        payload: &Value,
        context: &WorkerContext,
    ) -> Result<Value, ReflectionError> {
        let job_type = payload
            .get("job_type")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        match job_type {
            "reflection_daily" => {
                let view_spec = self.daily_review(context)?;
                let mut result = serde_json::to_value(&view_spec)
                    .map_err(|e| ReflectionError::Repo(e.to_string()))?;
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
                    .map_err(|e| ReflectionError::Repo(e.to_string()))?;
                if let Some(obj) = result.as_object_mut() {
                    obj.insert(
                        "_emit_event".to_string(),
                        serde_json::json!("new_feed_card"),
                    );
                }
                Ok(result)
            }
            _ => Err(ReflectionError::InvalidPayload(format!(
                "未知 Reflection job_type: {}",
                job_type
            ))),
        }
    }

    /// Generate a daily review card.
    pub fn daily_review(&self, context: &WorkerContext) -> Result<ViewSpec, ReflectionError> {
        let today = Local::now().date_naive();
        let today_str = today.format("%Y-%m-%d").to_string();
        let view_id = format!("review-daily-{}", today_str);

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
            id: view_id,
            view_type: "summary".to_string(),
            title: format!("每日回顾 - {}", today_str),
            query: String::new(),
            created: now,
            pinned: false,
            config: serde_json::json!({}),
            data: ViewData { markdown, stats },
            partial: if is_partial { Some(true) } else { None },
        };

        self.save_view_spec(&view_spec, context)?;
        Ok(view_spec)
    }

    /// Generate a weekly review card.
    pub fn weekly_review(&self, context: &WorkerContext) -> Result<ViewSpec, ReflectionError> {
        let today = Local::now().date_naive();
        let (year, week) = (today.iso_week().year(), today.iso_week().week());
        let view_id = format!("review-weekly-{}-W{:02}", year, week);

        let days_since_monday = today.weekday().num_days_from_monday();
        let monday = today - chrono::Duration::days(days_since_monday as i64);
        let monday_str = monday.format("%Y-%m-%d").to_string();
        let today_str = today.format("%Y-%m-%d").to_string();

        let (fragments_week, topic_activity, new_topics) =
            self.get_weekly_stats(&monday_str, &today_str, context)?;
        let fragments_count = fragments_week.len();

        let mut topic_vec: Vec<(String, usize)> = topic_activity.into_iter().collect();
        topic_vec.sort_by(|a, b| b.1.cmp(&a.1));
        let top_3: Vec<String> = topic_vec.iter().take(3).map(|(k, _)| k.clone()).collect();

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
            id: view_id,
            view_type: "summary".to_string(),
            title: format!("每周回顾 - {}-W{:02}", year, week),
            query: String::new(),
            created: now,
            pinned: false,
            config: serde_json::json!({}),
            data: ViewData { markdown, stats },
            partial: if is_partial { Some(true) } else { None },
        };

        self.save_view_spec(&view_spec, context)?;
        Ok(view_spec)
    }

    /// Check if any reviews were missed (called on app startup).
    pub fn check_missed_reviews(
        &self,
        context: &WorkerContext,
    ) -> Result<Vec<JobType>, ReflectionError> {
        let mut missing = Vec::new();
        let today = Local::now().date_naive();
        let today_str = today.format("%Y-%m-%d").to_string();

        let daily_id = format!("review-daily-{}", today_str);
        if !self.view_exists(&daily_id, context)? {
            missing.push(JobType::ReflectionDaily);
        }

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

    fn get_daily_stats(
        &self,
        today_str: &str,
        context: &WorkerContext,
    ) -> Result<(Vec<String>, HashMap<String, usize>), ReflectionError> {
        let index = context.index.lock()
            .map_err(|e| ReflectionError::Index(e.to_string()))?;

        let all_fragments = index
            .list_fragments(FragmentFilter::All, 0, 10000)
            .map_err(|e| ReflectionError::Index(e.to_string()))?;

        let mut fragments_today: Vec<String> = Vec::new();
        let mut active_topics: HashMap<String, usize> = HashMap::new();

        for frag in &all_fragments {
            if frag.created_at.starts_with(today_str) {
                fragments_today.push(frag.content.clone());
                for topic in &frag.topics {
                    *active_topics.entry(topic.clone()).or_insert(0) += 1;
                }
            }
        }

        Ok((fragments_today, active_topics))
    }

    fn get_weekly_stats(
        &self,
        monday_str: &str,
        today_str: &str,
        context: &WorkerContext,
    ) -> Result<(Vec<String>, HashMap<String, usize>, Vec<String>), ReflectionError> {
        let index = context.index.lock()
            .map_err(|e| ReflectionError::Index(e.to_string()))?;

        let all_fragments = index
            .list_fragments(FragmentFilter::All, 0, 10000)
            .map_err(|e| ReflectionError::Index(e.to_string()))?;

        let mut fragments_week: Vec<String> = Vec::new();
        let mut topic_activity: HashMap<String, usize> = HashMap::new();

        for frag in &all_fragments {
            let frag_date = &frag.created_at[..10];
            if frag_date >= monday_str && frag_date <= today_str {
                fragments_week.push(frag.content.clone());
                for topic in &frag.topics {
                    *topic_activity.entry(topic.clone()).or_insert(0) += 1;
                }
            }
        }

        let new_topics = self.find_new_topics_this_week(monday_str, today_str, context)?;

        Ok((fragments_week, topic_activity, new_topics))
    }

    fn find_new_topics_this_week(
        &self,
        monday_str: &str,
        today_str: &str,
        context: &WorkerContext,
    ) -> Result<Vec<String>, ReflectionError> {
        let repo = context.repo.lock()
            .map_err(|e| ReflectionError::Repo(e.to_string()))?;
        let topics_dir = repo.vault_path().join("topics");

        let mut new_topics = Vec::new();
        if !topics_dir.exists() {
            return Ok(new_topics);
        }

        let entries = std::fs::read_dir(&topics_dir)
            .map_err(|e| ReflectionError::Repo(e.to_string()))?;

        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("md") {
                continue;
            }
            let content = std::fs::read_to_string(&path).unwrap_or_default();
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

    fn generate_daily_insight(
        &self,
        _fragments_count: usize,
        _active_topics: &[String],
        _fragment_contents: &[String],
        _context: &WorkerContext,
    ) -> Result<String, ReflectionError> {
        // LLM insight generation now requires async rig_agents path.
        // From synchronous WorkerContext, we cannot call async agents.
        // Return error to trigger graceful degradation (stats-only card).
        // Full async integration will be done when JobQueue workers are migrated to async.
        Err(ReflectionError::Llm(
            "LLM insight generation pending async migration".to_string(),
        ))
    }

    fn generate_weekly_insight(
        &self,
        _fragments_count: usize,
        _new_topics: &[String],
        _top_3_topics: &[String],
        _fragment_contents: &[String],
        _context: &WorkerContext,
    ) -> Result<String, ReflectionError> {
        // LLM insight generation now requires async rig_agents path.
        // From synchronous WorkerContext, we cannot call async agents.
        // Return error to trigger graceful degradation (stats-only card).
        // Full async integration will be done when JobQueue workers are migrated to async.
        Err(ReflectionError::Llm(
            "LLM insight generation pending async migration".to_string(),
        ))
    }

    fn save_view_spec(
        &self,
        view_spec: &ViewSpec,
        context: &WorkerContext,
    ) -> Result<(), ReflectionError> {
        let repo = context.repo.lock()
            .map_err(|e| ReflectionError::Repo(e.to_string()))?;
        let views_dir = repo.vault_path().join("views");
        std::fs::create_dir_all(&views_dir)
            .map_err(|e| ReflectionError::Repo(e.to_string()))?;

        let file_path = views_dir.join(format!("{}.json", view_spec.id));
        let json = serde_json::to_string_pretty(view_spec)
            .map_err(|e| ReflectionError::Repo(e.to_string()))?;
        std::fs::write(&file_path, &json)
            .map_err(|e| ReflectionError::Repo(e.to_string()))?;

        Ok(())
    }

    fn view_exists(
        &self,
        view_id: &str,
        context: &WorkerContext,
    ) -> Result<bool, ReflectionError> {
        let repo = context.repo.lock()
            .map_err(|e| ReflectionError::Repo(e.to_string()))?;
        let views_dir = repo.vault_path().join("views");
        let file_path = views_dir.join(format!("{}.json", view_id));
        Ok(file_path.exists())
    }
}

// ─── ReflectionScheduler ────────────────────────────────────────────────────

/// Scheduler that runs in an independent thread.
pub struct ReflectionScheduler {
    queue: Arc<JobQueue>,
}

impl ReflectionScheduler {
    pub fn new(queue: Arc<JobQueue>) -> Self {
        Self { queue }
    }

    pub fn start(self) {
        thread::spawn(move || {
            log::info!("ReflectionScheduler started");
            loop {
                let sleep_duration = self.duration_until_next_review();
                log::info!(
                    "ReflectionScheduler sleeping for {:?} until next 22:00",
                    sleep_duration
                );
                thread::sleep(sleep_duration);

                let daily_payload = serde_json::json!({
                    "job_type": "reflection_daily"
                });
                match self.queue.enqueue(JobType::ReflectionDaily, daily_payload) {
                    Ok(id) => log::info!("已入队每日回顾 job: {}", id),
                    Err(e) => log::error!("入队每日回顾失败: {}", e),
                }

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

                thread::sleep(Duration::from_secs(60));
            }
        });
    }

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

fn extract_frontmatter_field(content: &str, field: &str) -> Option<String> {
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
            let value = value.trim_matches('"').trim_matches('\'');
            return Some(value.to_string());
        }
    }
    None
}
