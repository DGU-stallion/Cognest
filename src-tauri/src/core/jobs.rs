// Cognest Core — JobQueue
// SQLite-backed persistent job queue with worker threads

use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use chrono::Utc;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::embedding::EmbeddingEngine;
use super::index::IndexDb;
use super::llm::{LlmError, LlmGateway};
use super::repo::FileRepo;

// ─── Constants ──────────────────────────────────────────────────────────────

/// Worker poll interval to avoid busy-waiting
const POLL_INTERVAL: Duration = Duration::from_millis(100);

/// Maximum concurrent workers
const MAX_WORKERS: usize = 2;

/// 指数退避重试参数：5s, 25s, 125s（基数 5，指数 2）
const RETRY_DELAYS: [Duration; 3] = [
    Duration::from_secs(5),
    Duration::from_secs(25),
    Duration::from_secs(125),
];

/// 单 job 执行超时
const JOB_TIMEOUT: Duration = Duration::from_secs(300);

// ─── Types ──────────────────────────────────────────────────────────────────

/// Job status enum
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum JobStatus {
    Pending,
    Running,
    Completed,
    Failed,
    Blocked,
}

impl JobStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            JobStatus::Pending => "pending",
            JobStatus::Running => "running",
            JobStatus::Completed => "completed",
            JobStatus::Failed => "failed",
            JobStatus::Blocked => "blocked",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "pending" => Some(JobStatus::Pending),
            "running" => Some(JobStatus::Running),
            "completed" => Some(JobStatus::Completed),
            "failed" => Some(JobStatus::Failed),
            "blocked" => Some(JobStatus::Blocked),
            _ => None,
        }
    }
}

/// Job type enum
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum JobType {
    CuratorClassify,
    CuratorRecluster,
    WritingContext,
    ReflectionDaily,
    ReflectionWeekly,
}

impl JobType {
    pub fn as_str(&self) -> &'static str {
        match self {
            JobType::CuratorClassify => "curator_classify",
            JobType::CuratorRecluster => "curator_recluster",
            JobType::WritingContext => "writing_context",
            JobType::ReflectionDaily => "reflection_daily",
            JobType::ReflectionWeekly => "reflection_weekly",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "curator_classify" => Some(JobType::CuratorClassify),
            "curator_recluster" => Some(JobType::CuratorRecluster),
            "writing_context" => Some(JobType::WritingContext),
            "reflection_daily" => Some(JobType::ReflectionDaily),
            "reflection_weekly" => Some(JobType::ReflectionWeekly),
            _ => None,
        }
    }
}

/// Persistent job record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobRecord {
    pub id: String,
    pub job_type: JobType,
    pub status: JobStatus,
    pub payload: serde_json::Value,
    pub result: Option<serde_json::Value>,
    pub created_at: String,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
    pub retry_count: u32,
    pub progress: Option<u8>,
    pub error_message: Option<String>,
}

/// Job status change event (sent to frontend)
#[derive(Debug, Clone, Serialize)]
pub struct JobStatusEvent {
    pub job_id: String,
    pub status: JobStatus,
    pub progress: Option<u8>,
}

/// Job failed event
#[derive(Debug, Clone, Serialize)]
pub struct JobFailedEvent {
    pub job_id: String,
    pub job_type: JobType,
    pub reason: String,
}

/// Provider needed event (emitted when job is blocked due to missing/failing provider)
#[derive(Debug, Clone, Serialize)]
pub struct ProviderNeededEvent {
    pub job_id: String,
    pub job_type: JobType,
}

/// Audit log record for privacy tracking.
/// Records each cloud LLM request with metadata (no request content).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditRecord {
    pub id: i64,
    pub timestamp: String,
    pub provider_name: String,
    pub operation: String,
    pub token_count: u32,
    pub success: bool,
}

/// Represents the result of a job execution attempt
#[derive(Debug)]
pub enum JobExecutionError {
    /// LLM provider not configured or authentication failure → block the job
    Blocked(LlmError),
    /// Timeout during execution (request may not have been sent)
    Timeout,
    /// Cloud LLM request failure (network error, rate limit, unknown error).
    /// Per Req 9.7: do NOT retry — discard payload, fail immediately.
    CloudFailure(String),
    /// Non-cloud runtime error that may be retried (e.g., local I/O, embedding error)
    Runtime(String),
}

/// Job Queue errors
#[derive(Debug, thiserror::Error)]
pub enum JobQueueError {
    #[error("数据库错误: {0}")]
    Database(#[from] rusqlite::Error),

    #[error("Job 不存在: {0}")]
    NotFound(String),

    #[error("无法取消状态为 {status} 的 Job")]
    InvalidCancel { status: String },

    #[error("序列化错误: {0}")]
    Serialization(String),
}

/// Worker execution context (injected agent dependencies).
/// Placeholder types for now — actual agent execution wired in tasks 7.1/8.1/9.1.
pub struct WorkerContext {
    pub embedding: Arc<Mutex<EmbeddingEngine>>,
    pub llm: Arc<Mutex<LlmGateway>>,
    pub repo: Arc<Mutex<FileRepo>>,
    pub index: Arc<Mutex<IndexDb>>,
}

// ─── JobQueue ───────────────────────────────────────────────────────────────

/// SQLite-backed persistent job queue with worker thread pool.
pub struct JobQueue {
    db: Arc<Mutex<Connection>>,
    /// Event emitter callback — (event_name, json_payload).
    /// Abstracted to avoid Tauri type dependency.
    event_emitter: Arc<dyn Fn(&str, &str) + Send + Sync>,
    max_workers: usize,
}

impl JobQueue {
    /// Initialize the JobQueue, creating the jobs and audit_log tables if they don't exist.
    pub fn new(
        db: Arc<Mutex<Connection>>,
        event_emitter: Box<dyn Fn(&str, &str) + Send + Sync>,
    ) -> Self {
        // Create the jobs table and audit_log table
        {
            let conn = db.lock().unwrap();
            conn.execute_batch(
                "CREATE TABLE IF NOT EXISTS jobs (
                    id TEXT PRIMARY KEY,
                    job_type TEXT NOT NULL,
                    status TEXT NOT NULL DEFAULT 'pending',
                    payload TEXT NOT NULL,
                    result TEXT,
                    created_at TEXT NOT NULL,
                    started_at TEXT,
                    completed_at TEXT,
                    retry_count INTEGER NOT NULL DEFAULT 0,
                    progress INTEGER,
                    error_message TEXT
                );
                CREATE INDEX IF NOT EXISTS idx_jobs_status ON jobs(status);
                CREATE INDEX IF NOT EXISTS idx_jobs_created ON jobs(created_at);

                CREATE TABLE IF NOT EXISTS audit_log (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    timestamp TEXT NOT NULL,
                    provider_name TEXT NOT NULL,
                    operation TEXT NOT NULL,
                    token_count INTEGER NOT NULL DEFAULT 0,
                    success INTEGER NOT NULL DEFAULT 1
                );
                CREATE INDEX IF NOT EXISTS idx_audit_timestamp ON audit_log(timestamp);",
            )
            .expect("Failed to create jobs/audit_log tables");
        }

        Self {
            db,
            event_emitter: Arc::from(event_emitter),
            max_workers: MAX_WORKERS,
        }
    }

    /// Recover on startup: reset running → pending, return count of recovered jobs.
    /// Re-enqueue all pending jobs in creation-time ascending order.
    pub fn recover_on_startup(&self) -> Result<u32, JobQueueError> {
        let conn = self.db.lock().unwrap();
        let count = conn.execute(
            "UPDATE jobs SET status = 'pending', started_at = NULL WHERE status = 'running'",
            [],
        )?;
        Ok(count as u32)
    }

    /// Enqueue a new job. Returns the generated job ID.
    pub fn enqueue(
        &self,
        job_type: JobType,
        payload: serde_json::Value,
    ) -> Result<String, JobQueueError> {
        let id = Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();
        let payload_str = serde_json::to_string(&payload)
            .map_err(|e| JobQueueError::Serialization(e.to_string()))?;
        let job_type_str = job_type.as_str();

        {
            let conn = self.db.lock().unwrap();
            conn.execute(
                "INSERT INTO jobs (id, job_type, status, payload, created_at, retry_count)
                 VALUES (?1, ?2, 'pending', ?3, ?4, 0)",
                params![id, job_type_str, payload_str, now],
            )?;
        }

        // Emit event
        let event = JobStatusEvent {
            job_id: id.clone(),
            status: JobStatus::Pending,
            progress: None,
        };
        if let Ok(json) = serde_json::to_string(&event) {
            (self.event_emitter)("job_status_changed", &json);
        }

        Ok(id)
    }

    /// List jobs ordered by created_at DESC, limited to `limit` entries.
    pub fn list_jobs(&self, limit: u32) -> Result<Vec<JobRecord>, JobQueueError> {
        let conn = self.db.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, job_type, status, payload, result,
                    created_at, started_at, completed_at,
                    retry_count, progress, error_message
             FROM jobs ORDER BY created_at DESC LIMIT ?1",
        )?;

        let rows = stmt.query_map(params![limit], |row| {
            let job_type_str: String = row.get(1)?;
            let status_str: String = row.get(2)?;
            let payload_str: String = row.get(3)?;
            let result_str: Option<String> = row.get(4)?;

            Ok(JobRecord {
                id: row.get(0)?,
                job_type: JobType::from_str(&job_type_str)
                    .unwrap_or(JobType::CuratorClassify),
                status: JobStatus::from_str(&status_str)
                    .unwrap_or(JobStatus::Pending),
                payload: serde_json::from_str(&payload_str)
                    .unwrap_or(serde_json::Value::Null),
                result: result_str
                    .and_then(|s| serde_json::from_str(&s).ok()),
                created_at: row.get(5)?,
                started_at: row.get(6)?,
                completed_at: row.get(7)?,
                retry_count: row.get::<_, i32>(8)? as u32,
                progress: row.get::<_, Option<i32>>(9)?
                    .map(|p| p as u8),
                error_message: row.get(10)?,
            })
        })?;

        let mut jobs = Vec::new();
        for row in rows {
            jobs.push(row?);
        }
        Ok(jobs)
    }

    /// Cancel a job that is in pending or blocked status.
    pub fn cancel_job(&self, job_id: &str) -> Result<(), JobQueueError> {
        let conn = self.db.lock().unwrap();

        // Check current status
        let status_str: String = conn
            .query_row(
                "SELECT status FROM jobs WHERE id = ?1",
                params![job_id],
                |row| row.get(0),
            )
            .map_err(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => {
                    JobQueueError::NotFound(job_id.to_string())
                }
                other => JobQueueError::Database(other),
            })?;

        let status = JobStatus::from_str(&status_str)
            .unwrap_or(JobStatus::Running);

        match status {
            JobStatus::Pending | JobStatus::Blocked => {
                let now = Utc::now().to_rfc3339();
                conn.execute(
                    "UPDATE jobs SET status = 'failed', error_message = 'Cancelled by user', completed_at = ?1 WHERE id = ?2",
                    params![now, job_id],
                )?;

                // Emit event
                let event = JobStatusEvent {
                    job_id: job_id.to_string(),
                    status: JobStatus::Failed,
                    progress: None,
                };
                if let Ok(json) = serde_json::to_string(&event) {
                    (self.event_emitter)("job_status_changed", &json);
                }

                Ok(())
            }
            _ => Err(JobQueueError::InvalidCancel {
                status: status_str,
            }),
        }
    }

    /// Start worker threads (up to max_workers).
    /// Workers poll the jobs table for pending jobs, dequeue and execute them.
    /// Supports retry with exponential backoff and blocked state for provider issues.
    /// The actual agent execution is a placeholder (completes immediately) —
    /// real dispatch will be wired in tasks 7.1/8.1/9.1.
    pub fn start_workers(&self, _context: Arc<WorkerContext>) {
        let db = Arc::clone(&self.db);
        let emitter = Arc::clone(&self.event_emitter);
        let num_workers = self.max_workers;

        for worker_id in 0..num_workers {
            let db = Arc::clone(&db);
            let emitter = Arc::clone(&emitter);

            thread::spawn(move || {
                log::info!("JobQueue worker {} started", worker_id);
                loop {
                    // Try to acquire a pending job atomically
                    let job = match dequeue_next_job(&db, &emitter) {
                        Some(job) => job,
                        None => {
                            // No work — sleep briefly
                            thread::sleep(POLL_INTERVAL);
                            continue;
                        }
                    };

                    // Execute the job (placeholder — actual dispatch in later tasks)
                    log::info!(
                        "Worker {} executing job {} (type: {})",
                        worker_id,
                        job.id,
                        job.job_type.as_str()
                    );

                    // Placeholder: execute with timeout. Actual agent execution
                    // will be wired in tasks 7.1/8.1/9.1.
                    let exec_result = execute_job_with_timeout(&job);

                    match exec_result {
                        Ok(result) => {
                            mark_job_completed(&db, &emitter, &job.id, result);
                        }
                        Err(exec_err) => {
                            handle_job_error(
                                &db, &emitter, &job.id, &job.job_type,
                                job.retry_count, exec_err,
                            );
                        }
                    }
                }
            });
        }
    }

    /// Record an audit log entry for a cloud LLM request.
    ///
    /// This records metadata only (timestamp, provider, operation, token count, success).
    /// Content of requests/responses is NEVER logged (privacy requirement 9.8).
    pub fn record_audit(
        &self,
        provider_name: &str,
        operation: &str,
        token_count: u32,
        success: bool,
    ) -> Result<(), JobQueueError> {
        let timestamp = Utc::now().to_rfc3339();
        let conn = self.db.lock().unwrap();
        conn.execute(
            "INSERT INTO audit_log (timestamp, provider_name, operation, token_count, success)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![timestamp, provider_name, operation, token_count, success as i32],
        )?;
        Ok(())
    }

    /// Query the audit_log table for recent entries.
    /// Returns entries ordered by timestamp descending (most recent first).
    pub fn query_audit_log(&self, limit: u32) -> Result<Vec<AuditRecord>, JobQueueError> {
        let conn = self.db.lock().unwrap();

        let mut stmt = conn
            .prepare(
                "SELECT id, timestamp, provider_name, operation, token_count, success \
                 FROM audit_log ORDER BY timestamp DESC LIMIT ?1",
            )
            .map_err(JobQueueError::Database)?;

        let records = stmt
            .query_map(params![limit], |row| {
                Ok(AuditRecord {
                    id: row.get(0)?,
                    timestamp: row.get(1)?,
                    provider_name: row.get(2)?,
                    operation: row.get(3)?,
                    token_count: row.get(4)?,
                    success: row.get(5)?,
                })
            })
            .map_err(JobQueueError::Database)?
            .filter_map(|r| r.ok())
            .collect();

        Ok(records)
    }
}

// ─── Worker Helper Functions ────────────────────────────────────────────────

/// Atomically dequeue the next pending job (oldest first).
/// Sets status to 'running' and started_at timestamp.
/// Returns None if no pending jobs exist.
fn dequeue_next_job(
    db: &Arc<Mutex<Connection>>,
    emitter: &Arc<dyn Fn(&str, &str) + Send + Sync>,
) -> Option<JobRecord> {
    let conn = db.lock().unwrap();

    // SELECT the oldest pending job and UPDATE to running in one transaction
    let result: Result<JobRecord, rusqlite::Error> = conn
        .query_row(
            "SELECT id, job_type, payload, retry_count FROM jobs
             WHERE status = 'pending'
             ORDER BY created_at ASC
             LIMIT 1",
            [],
            |row| {
                let id: String = row.get(0)?;
                let job_type_str: String = row.get(1)?;
                let payload_str: String = row.get(2)?;
                let retry_count: i32 = row.get(3)?;

                Ok(JobRecord {
                    id,
                    job_type: JobType::from_str(&job_type_str)
                        .unwrap_or(JobType::CuratorClassify),
                    status: JobStatus::Pending,
                    payload: serde_json::from_str(&payload_str)
                        .unwrap_or(serde_json::Value::Null),
                    result: None,
                    created_at: String::new(),
                    started_at: None,
                    completed_at: None,
                    retry_count: retry_count as u32,
                    progress: None,
                    error_message: None,
                })
            },
        );

    match result {
        Ok(job) => {
            let now = Utc::now().to_rfc3339();
            let updated = conn.execute(
                "UPDATE jobs SET status = 'running', started_at = ?1
                 WHERE id = ?2 AND status = 'pending'",
                params![now, job.id],
            );

            match updated {
                Ok(1) => {
                    // Successfully claimed the job
                    let event = JobStatusEvent {
                        job_id: job.id.clone(),
                        status: JobStatus::Running,
                        progress: Some(0),
                    };
                    if let Ok(json) = serde_json::to_string(&event) {
                        emitter("job_status_changed", &json);
                    }
                    Some(job)
                }
                _ => {
                    // Another worker took it — race condition handled
                    None
                }
            }
        }
        Err(rusqlite::Error::QueryReturnedNoRows) => None,
        Err(e) => {
            log::error!("Failed to dequeue job: {}", e);
            None
        }
    }
}

/// Mark a job as completed with optional result JSON.
fn mark_job_completed(
    db: &Arc<Mutex<Connection>>,
    emitter: &Arc<dyn Fn(&str, &str) + Send + Sync>,
    job_id: &str,
    result: Option<serde_json::Value>,
) {
    let now = Utc::now().to_rfc3339();
    let result_str = result.and_then(|v| serde_json::to_string(&v).ok());

    let conn = db.lock().unwrap();
    let _ = conn.execute(
        "UPDATE jobs SET status = 'completed', completed_at = ?1, result = ?2, progress = 100
         WHERE id = ?3",
        params![now, result_str, job_id],
    );

    let event = JobStatusEvent {
        job_id: job_id.to_string(),
        status: JobStatus::Completed,
        progress: Some(100),
    };
    if let Ok(json) = serde_json::to_string(&event) {
        emitter("job_status_changed", &json);
    }
}

/// Mark a job as failed: sets status=failed, completed_at, error_message, emits job_failed event.
fn mark_job_failed(
    db: &Arc<Mutex<Connection>>,
    emitter: &Arc<dyn Fn(&str, &str) + Send + Sync>,
    job_id: &str,
    job_type: &JobType,
    reason: &str,
) {
    let now = Utc::now().to_rfc3339();

    {
        let conn = db.lock().unwrap();
        let _ = conn.execute(
            "UPDATE jobs SET status = 'failed', completed_at = ?1, error_message = ?2
             WHERE id = ?3",
            params![now, reason, job_id],
        );
    }

    // Emit job_status_changed
    let status_event = JobStatusEvent {
        job_id: job_id.to_string(),
        status: JobStatus::Failed,
        progress: None,
    };
    if let Ok(json) = serde_json::to_string(&status_event) {
        emitter("job_status_changed", &json);
    }

    // Emit job_failed
    let failed_event = JobFailedEvent {
        job_id: job_id.to_string(),
        job_type: job_type.clone(),
        reason: reason.to_string(),
    };
    if let Ok(json) = serde_json::to_string(&failed_event) {
        emitter("job_failed", &json);
    }
}

/// Mark a job as blocked: sets status=blocked, error_message, emits provider_needed event.
fn mark_job_blocked(
    db: &Arc<Mutex<Connection>>,
    emitter: &Arc<dyn Fn(&str, &str) + Send + Sync>,
    job_id: &str,
    job_type: &JobType,
    reason: &str,
) {
    {
        let conn = db.lock().unwrap();
        let _ = conn.execute(
            "UPDATE jobs SET status = 'blocked', error_message = ?1
             WHERE id = ?2",
            params![reason, job_id],
        );
    }

    // Emit job_status_changed
    let status_event = JobStatusEvent {
        job_id: job_id.to_string(),
        status: JobStatus::Blocked,
        progress: None,
    };
    if let Ok(json) = serde_json::to_string(&status_event) {
        emitter("job_status_changed", &json);
    }

    // Emit provider_needed
    let provider_event = ProviderNeededEvent {
        job_id: job_id.to_string(),
        job_type: job_type.clone(),
    };
    if let Ok(json) = serde_json::to_string(&provider_event) {
        emitter("provider_needed", &json);
    }
}

/// Handle a job execution error: decides whether to retry, block, or fail.
///
/// - Blocked errors (NoProvider, AuthFailure) → mark blocked immediately
/// - CloudFailure (Req 9.7) → mark failed immediately, do NOT retry.
///   The payload is discarded (not cached for retry). Error message does not
///   confirm whether data reached the remote endpoint.
/// - Timeout → retry with exponential backoff up to 3 times
///   (timeout implies the request may not have been sent)
/// - Runtime errors (non-cloud) → retry with exponential backoff up to 3 times
fn handle_job_error(
    db: &Arc<Mutex<Connection>>,
    emitter: &Arc<dyn Fn(&str, &str) + Send + Sync>,
    job_id: &str,
    job_type: &JobType,
    current_retry_count: u32,
    error: JobExecutionError,
) {
    match error {
        JobExecutionError::Blocked(llm_err) => {
            let reason = llm_err.to_string();
            mark_job_blocked(db, emitter, job_id, job_type, &reason);
        }
        JobExecutionError::CloudFailure(reason) => {
            // Req 9.7: Cloud failures fail immediately — no auto-retry.
            // Payload is already dropped (not stored for retry).
            // Use neutral error message that doesn't confirm data transit status.
            let neutral_reason = "操作失败，请检查网络后重试".to_string();
            log::warn!(
                "Job {} cloud failure (no retry per Req 9.7): {}",
                job_id, reason
            );
            mark_job_failed(db, emitter, job_id, job_type, &neutral_reason);
        }
        JobExecutionError::Timeout => {
            // Timeout: request may not have been sent, safe to retry
            let reason = "执行超时 (300s)".to_string();
            retry_or_fail(db, emitter, job_id, job_type, current_retry_count, &reason);
        }
        JobExecutionError::Runtime(msg) => {
            // Non-cloud runtime errors (e.g., local I/O, embedding) can be retried
            retry_or_fail(db, emitter, job_id, job_type, current_retry_count, &msg);
        }
    }
}

/// Retry a failed job with exponential backoff, or mark as failed if retries exhausted.
///
/// - If retry_count < 3: increment retry_count, sleep for RETRY_DELAYS[retry_count],
///   then reset status to 'pending' for re-execution.
/// - If retry_count >= 3: mark as failed, emit job_failed event.
fn retry_or_fail(
    db: &Arc<Mutex<Connection>>,
    emitter: &Arc<dyn Fn(&str, &str) + Send + Sync>,
    job_id: &str,
    job_type: &JobType,
    current_retry_count: u32,
    reason: &str,
) {
    if current_retry_count < RETRY_DELAYS.len() as u32 {
        // Sleep for exponential backoff delay
        let delay = RETRY_DELAYS[current_retry_count as usize];
        thread::sleep(delay);

        // Increment retry_count and reset to pending for re-execution
        let new_retry_count = current_retry_count + 1;
        {
            let conn = db.lock().unwrap();
            let _ = conn.execute(
                "UPDATE jobs SET status = 'pending', started_at = NULL, retry_count = ?1,
                 error_message = ?2 WHERE id = ?3",
                params![new_retry_count as i32, reason, job_id],
            );
        }

        // Emit status change back to pending
        let event = JobStatusEvent {
            job_id: job_id.to_string(),
            status: JobStatus::Pending,
            progress: None,
        };
        if let Ok(json) = serde_json::to_string(&event) {
            emitter("job_status_changed", &json);
        }
    } else {
        // Retries exhausted — mark as failed
        mark_job_failed(db, emitter, job_id, job_type, reason);
    }
}

/// Execute a job with a timeout of JOB_TIMEOUT (300s).
/// Returns Ok(result) if successful, or Err(JobExecutionError) on failure.
///
/// This is currently a placeholder that completes immediately.
/// Tasks 7.1/8.1/9.1 will wire in actual agent dispatch here.
fn execute_job_with_timeout(
    _job: &JobRecord,
) -> Result<Option<serde_json::Value>, JobExecutionError> {
    // Placeholder: complete immediately with no result.
    // When actual agent execution is wired in, this function will:
    // 1. Spawn a thread for agent execution
    // 2. Wait up to JOB_TIMEOUT for completion
    // 3. Return Timeout if exceeded
    // 4. Return Blocked for NoProvider/AuthFailure LlmErrors
    // 5. Return Runtime for other errors
    let _timeout = JOB_TIMEOUT; // Referenced to avoid dead-code warning
    Ok(None)
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    /// Create an in-memory SQLite connection wrapped in Arc<Mutex<>>
    fn test_db() -> Arc<Mutex<Connection>> {
        let conn = Connection::open_in_memory().unwrap();
        Arc::new(Mutex::new(conn))
    }

    /// Create a no-op event emitter
    fn noop_emitter() -> Box<dyn Fn(&str, &str) + Send + Sync> {
        Box::new(|_event, _payload| {})
    }

    /// Create a counting event emitter
    fn counting_emitter(
        counter: Arc<AtomicU32>,
    ) -> Box<dyn Fn(&str, &str) + Send + Sync> {
        Box::new(move |_event, _payload| {
            counter.fetch_add(1, Ordering::SeqCst);
        })
    }

    #[test]
    fn test_new_creates_table() {
        let db = test_db();
        let _queue = JobQueue::new(db.clone(), noop_emitter());

        // Verify table exists by querying it
        let conn = db.lock().unwrap();
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM jobs", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn test_enqueue_creates_job() {
        let db = test_db();
        let queue = JobQueue::new(db.clone(), noop_emitter());

        let payload = serde_json::json!({"fragment_id": "frag-001"});
        let id = queue
            .enqueue(JobType::CuratorClassify, payload.clone())
            .unwrap();

        assert!(!id.is_empty());

        // Verify in DB
        let conn = db.lock().unwrap();
        let (status, job_type): (String, String) = conn
            .query_row(
                "SELECT status, job_type FROM jobs WHERE id = ?1",
                params![id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(status, "pending");
        assert_eq!(job_type, "curator_classify");
    }

    #[test]
    fn test_enqueue_emits_event() {
        let counter = Arc::new(AtomicU32::new(0));
        let db = test_db();
        let queue = JobQueue::new(db, counting_emitter(counter.clone()));

        let payload = serde_json::json!({"test": true});
        queue.enqueue(JobType::ReflectionDaily, payload).unwrap();

        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn test_list_jobs_returns_ordered() {
        let db = test_db();
        let queue = JobQueue::new(db, noop_emitter());

        // Enqueue several jobs
        queue
            .enqueue(JobType::CuratorClassify, serde_json::json!({"n": 1}))
            .unwrap();
        queue
            .enqueue(JobType::WritingContext, serde_json::json!({"n": 2}))
            .unwrap();
        queue
            .enqueue(JobType::ReflectionWeekly, serde_json::json!({"n": 3}))
            .unwrap();

        let jobs = queue.list_jobs(10).unwrap();
        assert_eq!(jobs.len(), 3);
        // Most recent first
        assert_eq!(jobs[0].job_type, JobType::ReflectionWeekly);
        assert_eq!(jobs[2].job_type, JobType::CuratorClassify);
    }

    #[test]
    fn test_list_jobs_respects_limit() {
        let db = test_db();
        let queue = JobQueue::new(db, noop_emitter());

        for i in 0..5 {
            queue
                .enqueue(JobType::CuratorClassify, serde_json::json!({"n": i}))
                .unwrap();
        }

        let jobs = queue.list_jobs(3).unwrap();
        assert_eq!(jobs.len(), 3);
    }

    #[test]
    fn test_cancel_pending_job() {
        let db = test_db();
        let queue = JobQueue::new(db.clone(), noop_emitter());

        let id = queue
            .enqueue(JobType::CuratorRecluster, serde_json::json!({}))
            .unwrap();

        queue.cancel_job(&id).unwrap();

        let conn = db.lock().unwrap();
        let status: String = conn
            .query_row(
                "SELECT status FROM jobs WHERE id = ?1",
                params![id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(status, "failed");
    }

    #[test]
    fn test_cancel_running_job_fails() {
        let db = test_db();
        let queue = JobQueue::new(db.clone(), noop_emitter());

        let id = queue
            .enqueue(JobType::CuratorClassify, serde_json::json!({}))
            .unwrap();

        // Manually set to running
        {
            let conn = db.lock().unwrap();
            conn.execute(
                "UPDATE jobs SET status = 'running' WHERE id = ?1",
                params![id],
            )
            .unwrap();
        }

        let result = queue.cancel_job(&id);
        assert!(result.is_err());
        match result.unwrap_err() {
            JobQueueError::InvalidCancel { status } => {
                assert_eq!(status, "running");
            }
            _ => panic!("Expected InvalidCancel error"),
        }
    }

    #[test]
    fn test_cancel_blocked_job_succeeds() {
        let db = test_db();
        let queue = JobQueue::new(db.clone(), noop_emitter());

        let id = queue
            .enqueue(JobType::WritingContext, serde_json::json!({}))
            .unwrap();

        // Manually set to blocked
        {
            let conn = db.lock().unwrap();
            conn.execute(
                "UPDATE jobs SET status = 'blocked' WHERE id = ?1",
                params![id],
            )
            .unwrap();
        }

        queue.cancel_job(&id).unwrap();

        let conn = db.lock().unwrap();
        let status: String = conn
            .query_row(
                "SELECT status FROM jobs WHERE id = ?1",
                params![id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(status, "failed");
    }

    #[test]
    fn test_cancel_nonexistent_job() {
        let db = test_db();
        let queue = JobQueue::new(db, noop_emitter());

        let result = queue.cancel_job("nonexistent-id");
        assert!(result.is_err());
        match result.unwrap_err() {
            JobQueueError::NotFound(id) => {
                assert_eq!(id, "nonexistent-id");
            }
            _ => panic!("Expected NotFound error"),
        }
    }

    #[test]
    fn test_recover_on_startup_resets_running() {
        let db = test_db();
        let queue = JobQueue::new(db.clone(), noop_emitter());

        // Create some jobs and manually set them to running
        let id1 = queue
            .enqueue(JobType::CuratorClassify, serde_json::json!({}))
            .unwrap();
        let id2 = queue
            .enqueue(JobType::ReflectionDaily, serde_json::json!({}))
            .unwrap();
        let id3 = queue
            .enqueue(JobType::WritingContext, serde_json::json!({}))
            .unwrap();

        {
            let conn = db.lock().unwrap();
            conn.execute(
                "UPDATE jobs SET status = 'running' WHERE id IN (?1, ?2)",
                params![id1, id2],
            )
            .unwrap();
            // id3 stays pending
            conn.execute(
                "UPDATE jobs SET status = 'completed' WHERE id = ?1",
                params![id3],
            )
            .unwrap();
        }

        let recovered = queue.recover_on_startup().unwrap();
        assert_eq!(recovered, 2);

        // Verify running jobs are now pending
        let conn = db.lock().unwrap();
        let status1: String = conn
            .query_row(
                "SELECT status FROM jobs WHERE id = ?1",
                params![id1],
                |row| row.get(0),
            )
            .unwrap();
        let status2: String = conn
            .query_row(
                "SELECT status FROM jobs WHERE id = ?1",
                params![id2],
                |row| row.get(0),
            )
            .unwrap();
        let status3: String = conn
            .query_row(
                "SELECT status FROM jobs WHERE id = ?1",
                params![id3],
                |row| row.get(0),
            )
            .unwrap();

        assert_eq!(status1, "pending");
        assert_eq!(status2, "pending");
        assert_eq!(status3, "completed"); // unchanged
    }

    #[test]
    fn test_recover_on_startup_no_running_jobs() {
        let db = test_db();
        let queue = JobQueue::new(db, noop_emitter());

        let recovered = queue.recover_on_startup().unwrap();
        assert_eq!(recovered, 0);
    }

    #[test]
    fn test_dequeue_next_job_fifo() {
        let db = test_db();
        let _queue = JobQueue::new(db.clone(), noop_emitter());
        let emitter: Arc<dyn Fn(&str, &str) + Send + Sync> =
            Arc::new(|_event: &str, _payload: &str| {});

        // Insert jobs manually with ordered timestamps
        {
            let conn = db.lock().unwrap();
            conn.execute(
                "INSERT INTO jobs (id, job_type, status, payload, created_at, retry_count)
                 VALUES ('job-a', 'curator_classify', 'pending', '{}', '2024-01-01T00:00:00Z', 0)",
                [],
            ).unwrap();
            conn.execute(
                "INSERT INTO jobs (id, job_type, status, payload, created_at, retry_count)
                 VALUES ('job-b', 'reflection_daily', 'pending', '{}', '2024-01-02T00:00:00Z', 0)",
                [],
            ).unwrap();
        }

        // Dequeue should return the oldest job first
        let job = dequeue_next_job(&db, &emitter).unwrap();
        assert_eq!(job.id, "job-a");

        // Next dequeue should return job-b
        let job = dequeue_next_job(&db, &emitter).unwrap();
        assert_eq!(job.id, "job-b");

        // No more pending jobs
        let job = dequeue_next_job(&db, &emitter);
        assert!(job.is_none());
    }

    #[test]
    fn test_job_types_roundtrip() {
        let types = vec![
            JobType::CuratorClassify,
            JobType::CuratorRecluster,
            JobType::WritingContext,
            JobType::ReflectionDaily,
            JobType::ReflectionWeekly,
        ];

        for jt in types {
            let s = jt.as_str();
            let recovered = JobType::from_str(s).unwrap();
            assert_eq!(jt, recovered);
        }
    }

    #[test]
    fn test_job_status_roundtrip() {
        let statuses = vec![
            JobStatus::Pending,
            JobStatus::Running,
            JobStatus::Completed,
            JobStatus::Failed,
            JobStatus::Blocked,
        ];

        for status in statuses {
            let s = status.as_str();
            let recovered = JobStatus::from_str(s).unwrap();
            assert_eq!(status, recovered);
        }
    }

    // ─── Retry Logic Tests ──────────────────────────────────────────────────

    #[test]
    fn test_retry_or_fail_increments_retry_count() {
        let db = test_db();
        let _queue = JobQueue::new(db.clone(), noop_emitter());
        let emitter: Arc<dyn Fn(&str, &str) + Send + Sync> =
            Arc::new(|_event: &str, _payload: &str| {});

        // Insert a job manually with retry_count=0
        {
            let conn = db.lock().unwrap();
            conn.execute(
                "INSERT INTO jobs (id, job_type, status, payload, created_at, retry_count)
                 VALUES ('retry-job-1', 'curator_classify', 'running', '{}', '2024-01-01T00:00:00Z', 0)",
                [],
            ).unwrap();
        }

        // Call retry_or_fail with current_retry_count=0
        retry_or_fail(
            &db, &emitter, "retry-job-1", &JobType::CuratorClassify,
            0, "test error",
        );

        // Verify retry_count incremented to 1 and status is pending
        let conn = db.lock().unwrap();
        let (status, retry_count): (String, i32) = conn
            .query_row(
                "SELECT status, retry_count FROM jobs WHERE id = 'retry-job-1'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(status, "pending");
        assert_eq!(retry_count, 1);
    }

    #[test]
    fn test_retry_or_fail_marks_failed_after_max_retries() {
        let db = test_db();
        let _queue = JobQueue::new(db.clone(), noop_emitter());
        let emitter: Arc<dyn Fn(&str, &str) + Send + Sync> =
            Arc::new(|_event: &str, _payload: &str| {});

        // Insert a job with retry_count=3 (already exhausted)
        {
            let conn = db.lock().unwrap();
            conn.execute(
                "INSERT INTO jobs (id, job_type, status, payload, created_at, retry_count)
                 VALUES ('retry-job-2', 'reflection_daily', 'running', '{}', '2024-01-01T00:00:00Z', 3)",
                [],
            ).unwrap();
        }

        // Call retry_or_fail with current_retry_count=3 (>= RETRY_DELAYS.len())
        retry_or_fail(
            &db, &emitter, "retry-job-2", &JobType::ReflectionDaily,
            3, "max retries exceeded",
        );

        // Verify status is failed
        let conn = db.lock().unwrap();
        let (status, error_msg): (String, Option<String>) = conn
            .query_row(
                "SELECT status, error_message FROM jobs WHERE id = 'retry-job-2'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(status, "failed");
        assert_eq!(error_msg, Some("max retries exceeded".to_string()));
    }

    #[test]
    fn test_mark_job_blocked_sets_status_and_emits_event() {
        let db = test_db();
        let _queue = JobQueue::new(db.clone(), noop_emitter());

        let events: Arc<Mutex<Vec<(String, String)>>> = Arc::new(Mutex::new(Vec::new()));
        let events_clone = Arc::clone(&events);
        let emitter: Arc<dyn Fn(&str, &str) + Send + Sync> =
            Arc::new(move |event: &str, payload: &str| {
                events_clone.lock().unwrap().push((event.to_string(), payload.to_string()));
            });

        // Insert a job
        {
            let conn = db.lock().unwrap();
            conn.execute(
                "INSERT INTO jobs (id, job_type, status, payload, created_at, retry_count)
                 VALUES ('blocked-job-1', 'writing_context', 'running', '{}', '2024-01-01T00:00:00Z', 0)",
                [],
            ).unwrap();
        }

        mark_job_blocked(
            &db, &emitter, "blocked-job-1", &JobType::WritingContext,
            "无可用 Provider，请在设置中配置",
        );

        // Verify DB status
        let conn = db.lock().unwrap();
        let (status, error_msg): (String, Option<String>) = conn
            .query_row(
                "SELECT status, error_message FROM jobs WHERE id = 'blocked-job-1'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(status, "blocked");
        assert_eq!(error_msg, Some("无可用 Provider，请在设置中配置".to_string()));
        drop(conn);

        // Verify events emitted
        let emitted = events.lock().unwrap();
        assert_eq!(emitted.len(), 2);
        assert_eq!(emitted[0].0, "job_status_changed");
        assert_eq!(emitted[1].0, "provider_needed");
        // Verify provider_needed payload contains job_id
        assert!(emitted[1].1.contains("blocked-job-1"));
    }

    #[test]
    fn test_mark_job_failed_sets_status_and_emits_events() {
        let db = test_db();
        let _queue = JobQueue::new(db.clone(), noop_emitter());

        let events: Arc<Mutex<Vec<(String, String)>>> = Arc::new(Mutex::new(Vec::new()));
        let events_clone = Arc::clone(&events);
        let emitter: Arc<dyn Fn(&str, &str) + Send + Sync> =
            Arc::new(move |event: &str, payload: &str| {
                events_clone.lock().unwrap().push((event.to_string(), payload.to_string()));
            });

        // Insert a job
        {
            let conn = db.lock().unwrap();
            conn.execute(
                "INSERT INTO jobs (id, job_type, status, payload, created_at, retry_count)
                 VALUES ('failed-job-1', 'curator_recluster', 'running', '{}', '2024-01-01T00:00:00Z', 3)",
                [],
            ).unwrap();
        }

        mark_job_failed(
            &db, &emitter, "failed-job-1", &JobType::CuratorRecluster,
            "执行超时 (300s)",
        );

        // Verify DB status
        let conn = db.lock().unwrap();
        let (status, error_msg, completed_at): (String, Option<String>, Option<String>) = conn
            .query_row(
                "SELECT status, error_message, completed_at FROM jobs WHERE id = 'failed-job-1'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(status, "failed");
        assert_eq!(error_msg, Some("执行超时 (300s)".to_string()));
        assert!(completed_at.is_some()); // completed_at should be set
        drop(conn);

        // Verify events emitted
        let emitted = events.lock().unwrap();
        assert_eq!(emitted.len(), 2);
        assert_eq!(emitted[0].0, "job_status_changed");
        assert_eq!(emitted[1].0, "job_failed");
        // Verify job_failed payload contains job_id and reason
        assert!(emitted[1].1.contains("failed-job-1"));
        assert!(emitted[1].1.contains("执行超时"));
    }

    #[test]
    fn test_handle_job_error_blocked_for_no_provider() {
        let db = test_db();
        let _queue = JobQueue::new(db.clone(), noop_emitter());

        let events: Arc<Mutex<Vec<(String, String)>>> = Arc::new(Mutex::new(Vec::new()));
        let events_clone = Arc::clone(&events);
        let emitter: Arc<dyn Fn(&str, &str) + Send + Sync> =
            Arc::new(move |event: &str, payload: &str| {
                events_clone.lock().unwrap().push((event.to_string(), payload.to_string()));
            });

        // Insert a job
        {
            let conn = db.lock().unwrap();
            conn.execute(
                "INSERT INTO jobs (id, job_type, status, payload, created_at, retry_count)
                 VALUES ('no-provider-job', 'curator_classify', 'running', '{}', '2024-01-01T00:00:00Z', 0)",
                [],
            ).unwrap();
        }

        // Simulate NoProvider error
        handle_job_error(
            &db, &emitter, "no-provider-job", &JobType::CuratorClassify,
            0, JobExecutionError::Blocked(LlmError::NoProvider),
        );

        // Verify status is blocked
        let conn = db.lock().unwrap();
        let status: String = conn
            .query_row(
                "SELECT status FROM jobs WHERE id = 'no-provider-job'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(status, "blocked");
        drop(conn);

        // Verify provider_needed event was emitted
        let emitted = events.lock().unwrap();
        assert!(emitted.iter().any(|(name, _)| name == "provider_needed"));
    }

    #[test]
    fn test_handle_job_error_blocked_for_auth_failure() {
        let db = test_db();
        let _queue = JobQueue::new(db.clone(), noop_emitter());
        let emitter: Arc<dyn Fn(&str, &str) + Send + Sync> =
            Arc::new(|_event: &str, _payload: &str| {});

        // Insert a job
        {
            let conn = db.lock().unwrap();
            conn.execute(
                "INSERT INTO jobs (id, job_type, status, payload, created_at, retry_count)
                 VALUES ('auth-fail-job', 'writing_context', 'running', '{}', '2024-01-01T00:00:00Z', 0)",
                [],
            ).unwrap();
        }

        // Simulate AuthFailure error
        handle_job_error(
            &db, &emitter, "auth-fail-job", &JobType::WritingContext,
            0, JobExecutionError::Blocked(LlmError::AuthFailure {
                provider: "DeepSeek".to_string(),
            }),
        );

        // Verify status is blocked
        let conn = db.lock().unwrap();
        let status: String = conn
            .query_row(
                "SELECT status FROM jobs WHERE id = 'auth-fail-job'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(status, "blocked");
    }

    #[test]
    fn test_handle_job_error_timeout_triggers_retry() {
        let db = test_db();
        let _queue = JobQueue::new(db.clone(), noop_emitter());
        let emitter: Arc<dyn Fn(&str, &str) + Send + Sync> =
            Arc::new(|_event: &str, _payload: &str| {});

        // Insert a job
        {
            let conn = db.lock().unwrap();
            conn.execute(
                "INSERT INTO jobs (id, job_type, status, payload, created_at, retry_count)
                 VALUES ('timeout-job', 'reflection_weekly', 'running', '{}', '2024-01-01T00:00:00Z', 0)",
                [],
            ).unwrap();
        }

        // Simulate Timeout error
        handle_job_error(
            &db, &emitter, "timeout-job", &JobType::ReflectionWeekly,
            0, JobExecutionError::Timeout,
        );

        // Verify retry_count incremented and status reset to pending
        let conn = db.lock().unwrap();
        let (status, retry_count): (String, i32) = conn
            .query_row(
                "SELECT status, retry_count FROM jobs WHERE id = 'timeout-job'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(status, "pending");
        assert_eq!(retry_count, 1);
    }

    #[test]
    fn test_handle_job_error_runtime_with_exhausted_retries() {
        let db = test_db();
        let _queue = JobQueue::new(db.clone(), noop_emitter());

        let events: Arc<Mutex<Vec<(String, String)>>> = Arc::new(Mutex::new(Vec::new()));
        let events_clone = Arc::clone(&events);
        let emitter: Arc<dyn Fn(&str, &str) + Send + Sync> =
            Arc::new(move |event: &str, payload: &str| {
                events_clone.lock().unwrap().push((event.to_string(), payload.to_string()));
            });

        // Insert a job with retry_count already at max
        {
            let conn = db.lock().unwrap();
            conn.execute(
                "INSERT INTO jobs (id, job_type, status, payload, created_at, retry_count)
                 VALUES ('exhausted-job', 'curator_classify', 'running', '{}', '2024-01-01T00:00:00Z', 3)",
                [],
            ).unwrap();
        }

        // Simulate Runtime error with retries exhausted
        handle_job_error(
            &db, &emitter, "exhausted-job", &JobType::CuratorClassify,
            3, JobExecutionError::Runtime("网络错误".to_string()),
        );

        // Verify status is failed
        let conn = db.lock().unwrap();
        let status: String = conn
            .query_row(
                "SELECT status FROM jobs WHERE id = 'exhausted-job'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(status, "failed");
        drop(conn);

        // Verify job_failed event was emitted
        let emitted = events.lock().unwrap();
        assert!(emitted.iter().any(|(name, _)| name == "job_failed"));
    }

    #[test]
    fn test_handle_job_error_cloud_failure_no_retry() {
        // Req 9.7: Cloud LLM failures should fail immediately without retry.
        let db = test_db();
        let _queue = JobQueue::new(db.clone(), noop_emitter());

        let events: Arc<Mutex<Vec<(String, String)>>> = Arc::new(Mutex::new(Vec::new()));
        let events_clone = Arc::clone(&events);
        let emitter: Arc<dyn Fn(&str, &str) + Send + Sync> =
            Arc::new(move |event: &str, payload: &str| {
                events_clone.lock().unwrap().push((event.to_string(), payload.to_string()));
            });

        // Insert a job with retry_count at 0 — should still NOT retry
        {
            let conn = db.lock().unwrap();
            conn.execute(
                "INSERT INTO jobs (id, job_type, status, payload, created_at, retry_count)
                 VALUES ('cloud-fail-job', 'curator_classify', 'running', '{\"fragment_id\": \"test\"}', '2024-01-01T00:00:00Z', 0)",
                [],
            ).unwrap();
        }

        // Simulate CloudFailure — should immediately fail, not retry
        handle_job_error(
            &db, &emitter, "cloud-fail-job", &JobType::CuratorClassify,
            0, JobExecutionError::CloudFailure("network interrupted".to_string()),
        );

        // Verify status is failed (NOT pending for retry)
        let conn = db.lock().unwrap();
        let (status, retry_count): (String, i32) = conn
            .query_row(
                "SELECT status, retry_count FROM jobs WHERE id = 'cloud-fail-job'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(status, "failed");
        assert_eq!(retry_count, 0); // retry_count not incremented

        // Verify error_message uses neutral phrasing (doesn't confirm data transit)
        let error_msg: String = conn
            .query_row(
                "SELECT error_message FROM jobs WHERE id = 'cloud-fail-job'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(error_msg, "操作失败，请检查网络后重试");
        assert!(!error_msg.contains("未发送"));
        assert!(!error_msg.contains("已收到"));
        assert!(!error_msg.contains("network"));
        drop(conn);

        // Verify job_failed event was emitted
        let emitted = events.lock().unwrap();
        assert!(emitted.iter().any(|(name, _)| name == "job_failed"));
    }

    #[test]
    fn test_retry_delays_are_correct() {
        assert_eq!(RETRY_DELAYS[0], Duration::from_secs(5));
        assert_eq!(RETRY_DELAYS[1], Duration::from_secs(25));
        assert_eq!(RETRY_DELAYS[2], Duration::from_secs(125));
    }

    #[test]
    fn test_job_timeout_constant() {
        assert_eq!(JOB_TIMEOUT, Duration::from_secs(300));
    }

    // ─── Audit Log Tests ────────────────────────────────────────────────────

    #[test]
    fn test_new_creates_audit_log_table() {
        let db = test_db();
        let _queue = JobQueue::new(db.clone(), noop_emitter());

        // Verify audit_log table exists by querying it
        let conn = db.lock().unwrap();
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM audit_log", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn test_record_audit_inserts_entry() {
        let db = test_db();
        let queue = JobQueue::new(db.clone(), noop_emitter());

        queue
            .record_audit("deepseek", "chat", 150, true)
            .unwrap();

        let conn = db.lock().unwrap();
        let (provider, operation, token_count, success): (String, String, u32, bool) = conn
            .query_row(
                "SELECT provider_name, operation, token_count, success FROM audit_log WHERE id = 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .unwrap();
        assert_eq!(provider, "deepseek");
        assert_eq!(operation, "chat");
        assert_eq!(token_count, 150);
        assert!(success);
    }

    #[test]
    fn test_record_audit_failure() {
        let db = test_db();
        let queue = JobQueue::new(db.clone(), noop_emitter());

        queue
            .record_audit("openai_compat", "stream_chat", 0, false)
            .unwrap();

        let conn = db.lock().unwrap();
        let (provider, operation, token_count, success): (String, String, u32, bool) = conn
            .query_row(
                "SELECT provider_name, operation, token_count, success FROM audit_log WHERE id = 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .unwrap();
        assert_eq!(provider, "openai_compat");
        assert_eq!(operation, "stream_chat");
        assert_eq!(token_count, 0);
        assert!(!success);
    }

    #[test]
    fn test_query_audit_log_returns_ordered() {
        let db = test_db();
        let queue = JobQueue::new(db, noop_emitter());

        // Insert multiple audit entries
        queue.record_audit("deepseek", "chat", 100, true).unwrap();
        queue.record_audit("deepseek", "stream_chat", 200, true).unwrap();
        queue.record_audit("openai_compat", "validate", 0, false).unwrap();

        let records = queue.query_audit_log(10).unwrap();
        assert_eq!(records.len(), 3);
        // Most recent first (validate was last inserted)
        assert_eq!(records[0].operation, "validate");
        assert_eq!(records[2].operation, "chat");
    }

    #[test]
    fn test_query_audit_log_respects_limit() {
        let db = test_db();
        let queue = JobQueue::new(db, noop_emitter());

        for i in 0..10 {
            queue
                .record_audit("deepseek", &format!("op_{}", i), i as u32 * 10, true)
                .unwrap();
        }

        let records = queue.query_audit_log(3).unwrap();
        assert_eq!(records.len(), 3);
    }

    #[test]
    fn test_query_audit_log_empty() {
        let db = test_db();
        let queue = JobQueue::new(db, noop_emitter());

        let records = queue.query_audit_log(50).unwrap();
        assert!(records.is_empty());
    }
}
