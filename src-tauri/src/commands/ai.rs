// Cognest IPC Command Layer — AI Commands
//
// Thin #[tauri::command] functions for AI subsystem.
// Forwards to EmbeddingEngine, LlmGateway, JobQueue, SettingsManager,
// WritingAgent, and ReflectionAgent.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use futures::StreamExt;
use serde::{Deserialize, Serialize};
use tauri::{Emitter, State};

use crate::commands::AppState;
use crate::core::agents::reflection::ViewSpec;
use crate::core::agents::writing::WritingAgent;
use crate::core::embedding::EmbeddingEngine;
use crate::core::jobs::{AuditRecord, JobQueue, JobRecord, WorkerContext};
use crate::core::llm::{ChatMessage, ChatOptions, LlmGateway, Role, StreamChunk};
use crate::core::settings::{AppSettings, ProviderConfig, SettingsManager};

// ─── AI Application State ───────────────────────────────────────────────────

/// Shared AI state holding subsystem instances behind Arc/Mutex guards.
pub struct AiState {
    pub embedding: Arc<Mutex<EmbeddingEngine>>,
    pub llm: Arc<Mutex<LlmGateway>>,
    pub jobs: Arc<JobQueue>,
    pub settings: Arc<Mutex<SettingsManager>>,
}

// ─── IPC-specific DTO Types ─────────────────────────────────────────────────

/// Embedding status information for the frontend.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbeddingStatus {
    /// Number of fragments with cached vectors.
    pub cached_count: usize,
    /// Total number of known fragment IDs registered with the engine.
    pub total: usize,
}

/// A fragment with its similarity score.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimilarFragment {
    pub fragment_id: String,
    pub similarity: f32,
}

// ─── Embedding Commands ─────────────────────────────────────────────────────

/// Get the current embedding status (cached count vs total fragments).
#[tauri::command]
pub fn get_embedding_status(
    state: State<'_, AiState>,
    app_state: State<'_, AppState>,
) -> Result<EmbeddingStatus, String> {
    let engine = state.embedding.lock().map_err(|e| e.to_string())?;
    let index = app_state.index.lock().map_err(|e| e.to_string())?;

    // Total fragments from the index database
    let total = index.fragment_count().map_err(|e| e.to_string())? as usize;

    // Cached count from the vector cache
    let cached_count = engine.vector_cache_len();

    Ok(EmbeddingStatus {
        cached_count,
        total,
    })
}

/// Find fragments most similar to the given fragment.
#[tauri::command]
pub fn find_similar_fragments(
    state: State<'_, AiState>,
    fragment_id: String,
    limit: Option<usize>,
) -> Result<Vec<SimilarFragment>, String> {
    let engine = state.embedding.lock().map_err(|e| e.to_string())?;
    let top_k = limit.unwrap_or(5);

    // Get all known fragment IDs as candidates (excluding the target itself)
    let all_ids = engine.cached_fragment_ids();
    let candidates: Vec<String> = all_ids
        .into_iter()
        .filter(|id| id != &fragment_id)
        .collect();

    let results = engine
        .find_similar(&fragment_id, &candidates, top_k)
        .map_err(|e| e.to_string())?;

    Ok(results
        .into_iter()
        .map(|(id, score)| SimilarFragment {
            fragment_id: id,
            similarity: score,
        })
        .collect())
}

// ─── Writing Agent Commands ─────────────────────────────────────────────────

/// Synchronous writing chat (returns full response).
#[tauri::command(async)]
pub async fn writing_chat(
    state: State<'_, AiState>,
    app_state: State<'_, AppState>,
    article_id: String,
    message: String,
    history: Vec<ChatMessage>,
) -> Result<String, String> {
    let embedding = state.embedding.clone();
    let llm = state.llm.clone();
    let jobs = state.jobs.clone();
    let repo = Arc::new(Mutex::new(
        app_state.repo.lock().map_err(|e| e.to_string())?.clone_for_ai()
    ));
    let index_arc = app_state.index_arc.clone();

    let result = tokio::task::spawn_blocking(move || {
        let context = WorkerContext {
            embedding,
            llm: llm.clone(),
            repo,
            index: index_arc,
        };
        let agent = WritingAgent;

        // Resolve provider name for audit logging
        let provider_name = {
            let llm_guard = llm.lock().map_err(|e| e.to_string())?;
            llm_guard.resolve_provider_name("writing").unwrap_or_default()
        };
        let is_cloud = !provider_name.is_empty() && provider_name != "ollama";

        // Use article_id as a reference — load actual content from repo
        let article_content = {
            let repo_guard = context.repo.lock().map_err(|e| e.to_string())?;
            match repo_guard.read_article(&article_id) {
                Ok((_meta, body)) => body,
                Err(_) => article_id.clone(), // Fallback: use the ID as content hint
            }
        };

        let response = agent
            .chat(&article_content, &message, &history, &context)
            .map_err(|_e| {
                // Record audit for failed cloud request (success=false per Req 9.7)
                if is_cloud {
                    let _ = jobs.record_audit(&provider_name, "chat", 0, false);
                }
                // Privacy (Req 9.7): return neutral error message to frontend.
                // Don't confirm whether data reached the remote endpoint.
                format!("操作失败，请检查网络后重试")
            })?;

        // Record audit for successful cloud request (no content logged)
        if is_cloud {
            let _ = jobs.record_audit(
                &provider_name,
                "chat",
                response.usage.total_tokens,
                true,
            );
        }

        Ok::<String, String>(response.content)
    })
    .await
    .map_err(|e| format!("Task join error: {}", e))?;

    result
}

/// Streaming writing chat — emits "writing_chunk" events to the frontend.
#[tauri::command(async)]
pub async fn writing_stream_chat(
    state: State<'_, AiState>,
    app_state: State<'_, AppState>,
    app: tauri::AppHandle,
    article_id: String,
    message: String,
    history: Vec<ChatMessage>,
) -> Result<(), String> {
    let embedding = state.embedding.clone();
    let llm = state.llm.clone();
    let jobs = state.jobs.clone();
    let repo = Arc::new(Mutex::new(
        app_state.repo.lock().map_err(|e| e.to_string())?.clone_for_ai()
    ));
    let index_arc = app_state.index_arc.clone();

    // Resolve provider name for audit logging before entering blocking task
    let provider_name = {
        let llm_guard = state.llm.lock().map_err(|e| e.to_string())?;
        llm_guard.resolve_provider_name("writing").unwrap_or_default()
    };
    let is_cloud = !provider_name.is_empty() && provider_name != "ollama";

    // Build context and get stream in a blocking task
    let stream_result = tokio::task::spawn_blocking(move || {
        let context = WorkerContext {
            embedding,
            llm,
            repo,
            index: index_arc,
        };
        let agent = WritingAgent;

        let article_content = {
            let repo_guard = context.repo.lock().map_err(|e| e.to_string())?;
            match repo_guard.read_article(&article_id) {
                Ok((_meta, body)) => body,
                Err(_) => article_id.clone(),
            }
        };

        let stream = agent
            .stream_chat(&article_content, &message, &history, &context)
            .map_err(|_e| {
                // Privacy (Req 9.7): return neutral error message.
                "操作失败，请检查网络后重试".to_string()
            })?;

        Ok::<_, String>(stream)
    })
    .await
    .map_err(|e| format!("Task join error: {}", e))?;

    match stream_result {
        Ok(mut stream) => {
            // Consume the stream and emit events to the frontend
            let mut total_tokens: u32 = 0;
            let mut stream_success = true;

            while let Some(chunk) = stream.next().await {
                let payload = serde_json::to_string(&chunk).unwrap_or_default();
                let _ = app.emit("writing_chunk", &payload);

                // Track final token usage and detect errors
                match &chunk {
                    StreamChunk::Done { usage } => {
                        total_tokens = usage.total_tokens;
                        break;
                    }
                    StreamChunk::Error { partial_tokens, .. } => {
                        total_tokens = *partial_tokens;
                        stream_success = false;
                        break;
                    }
                    _ => {}
                }
            }

            // Record audit for cloud request
            if is_cloud {
                let _ = jobs.record_audit(
                    &provider_name,
                    "stream_chat",
                    total_tokens,
                    stream_success,
                );
            }

            Ok(())
        }
        Err(e) => {
            // Record audit for failed cloud request (success=false per Req 9.7)
            if is_cloud {
                let _ = jobs.record_audit(&provider_name, "stream_chat", 0, false);
            }
            // Privacy (Req 9.7): return neutral error message to frontend.
            Err("操作失败，请检查网络后重试".to_string())
        }
    }
}

/// Recommend related fragments for the given article content.
#[tauri::command]
pub fn writing_recommend(
    state: State<'_, AiState>,
    article_content: String,
    limit: Option<usize>,
) -> Result<Vec<SimilarFragment>, String> {
    let engine = state.embedding.lock().map_err(|e| e.to_string())?;
    let top_k = limit.unwrap_or(5);

    // Embed the article content and find similar cached fragments
    let article_vec = engine.embed_text(&article_content).map_err(|e| e.to_string())?;
    let all_ids = engine.cached_fragment_ids();

    // Compute similarity against all cached fragments
    let mut scored: Vec<(String, f32)> = Vec::new();
    for id in &all_ids {
        if let Ok(frag_vec) = engine.get_vector(id) {
            let sim = cosine_sim(&article_vec, &frag_vec);
            scored.push((id.clone(), sim));
        }
    }

    // Sort descending by similarity
    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    scored.truncate(top_k);

    Ok(scored
        .into_iter()
        .map(|(id, score)| SimilarFragment {
            fragment_id: id,
            similarity: score,
        })
        .collect())
}

// ─── View Generation Commands ───────────────────────────────────────────────

/// Generate a view from a natural language prompt.
#[tauri::command(async)]
pub async fn generate_view(
    state: State<'_, AiState>,
    prompt: String,
) -> Result<ViewSpec, String> {
    let llm = state.llm.clone();
    let jobs = state.jobs.clone();

    // Resolve provider name for audit logging
    let provider_name = {
        let llm_guard = state.llm.lock().map_err(|e| e.to_string())?;
        llm_guard.resolve_provider_name("view_generator").unwrap_or_default()
    };
    let is_cloud = !provider_name.is_empty() && provider_name != "ollama";

    let result = tokio::task::spawn_blocking(move || {
        let llm_guard = llm.lock().map_err(|e| e.to_string())?;

        let system_msg = ChatMessage {
            role: Role::System,
            content: concat!(
                "你是 Cognest 的视图生成助手。根据用户的自然语言描述，生成一个 ViewSpec JSON。",
                "支持的类型: graph, timeline, list, chart, summary。",
                "返回纯 JSON，不要额外解释。",
                "JSON 格式: {\"id\": \"<uuid>\", \"type\": \"<type>\", \"title\": \"<标题>\", ",
                "\"query\": \"<原始提示>\", \"created\": \"\", \"pinned\": false, ",
                "\"config\": {}, \"data\": {\"markdown\": \"<内容>\", \"stats\": {}}}"
            )
            .to_string(),
        };

        let user_msg = ChatMessage {
            role: Role::User,
            content: prompt.clone(),
        };

        let options = ChatOptions {
            json_schema: Some(serde_json::json!({
                "type": "object",
                "properties": {
                    "id": {"type": "string"},
                    "type": {"type": "string", "enum": ["graph", "timeline", "list", "chart", "summary"]},
                    "title": {"type": "string"},
                    "query": {"type": "string"},
                    "pinned": {"type": "boolean"},
                    "config": {"type": "object"},
                    "data": {"type": "object"}
                },
                "required": ["id", "type", "title", "query", "data"]
            })),
            ..Default::default()
        };

        let response = llm_guard
            .chat_for_agent("view_generator", &[system_msg, user_msg], &options)
            .map_err(|_e| {
                // Record audit for failed cloud request (success=false per Req 9.7)
                if is_cloud {
                    let _ = jobs.record_audit(&provider_name, "generate_view", 0, false);
                }
                // Privacy (Req 9.7): return neutral error message to frontend.
                // Don't confirm whether data reached the remote endpoint.
                format!("操作失败，请检查网络后重试")
            })?;

        // Record audit for successful cloud request
        if is_cloud {
            let _ = jobs.record_audit(
                &provider_name,
                "generate_view",
                response.usage.total_tokens,
                true,
            );
        }

        // Parse the response content as ViewSpec
        let mut view_spec: ViewSpec =
            serde_json::from_str(&response.content).map_err(|e| {
                format!("视图 JSON 解析失败: {} — 原始响应: {}", e, response.content)
            })?;

        // Ensure created timestamp and query are populated
        if view_spec.created.is_empty() {
            view_spec.created = chrono::Local::now().to_rfc3339();
        }
        if view_spec.query.is_empty() {
            view_spec.query = prompt;
        }

        Ok::<ViewSpec, String>(view_spec)
    })
    .await
    .map_err(|e| format!("Task join error: {}", e))?;

    result
}

/// Pin a view (save to persistent storage).
#[tauri::command]
pub fn pin_view(
    state: State<'_, AiState>,
    view_spec: ViewSpec,
) -> Result<(), String> {
    let settings_mgr = state.settings.lock().map_err(|e| e.to_string())?;
    let views_dir = settings_mgr.views_dir();

    std::fs::create_dir_all(&views_dir).map_err(|e| e.to_string())?;

    let mut pinned_view = view_spec;
    pinned_view.pinned = true;

    let file_path = views_dir.join(format!("{}.json", pinned_view.id));
    let json = serde_json::to_string_pretty(&pinned_view).map_err(|e| e.to_string())?;
    std::fs::write(&file_path, json).map_err(|e| e.to_string())?;

    Ok(())
}

/// List all pinned views.
#[tauri::command]
pub fn list_pinned_views(
    state: State<'_, AiState>,
) -> Result<Vec<ViewSpec>, String> {
    let settings_mgr = state.settings.lock().map_err(|e| e.to_string())?;
    let views_dir = settings_mgr.views_dir();

    if !views_dir.exists() {
        return Ok(Vec::new());
    }

    let mut views = Vec::new();
    let entries = std::fs::read_dir(&views_dir).map_err(|e| e.to_string())?;

    for entry in entries {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();
        if path.extension().map_or(false, |ext| ext == "json") {
            let content = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
            if let Ok(view) = serde_json::from_str::<ViewSpec>(&content) {
                views.push(view);
            }
        }
    }

    // Sort by created descending (most recent first)
    views.sort_by(|a, b| b.created.cmp(&a.created));
    Ok(views)
}

// ─── Settings Commands ──────────────────────────────────────────────────────

/// Get current AI settings.
#[tauri::command]
pub fn get_ai_settings(
    state: State<'_, AiState>,
) -> Result<AppSettings, String> {
    let settings_mgr = state.settings.lock().map_err(|e| e.to_string())?;
    settings_mgr.load().map_err(|e| e.to_string())
}

/// Save AI settings and API keys.
#[tauri::command]
pub fn save_ai_settings(
    state: State<'_, AiState>,
    settings: AppSettings,
    api_keys: HashMap<String, String>,
) -> Result<(), String> {
    let settings_mgr = state.settings.lock().map_err(|e| e.to_string())?;

    // Save settings to encrypted file
    settings_mgr.save(&settings).map_err(|e| e.to_string())?;

    // Save each API key to macOS Keychain
    for (provider_id, key) in &api_keys {
        if key.is_empty() {
            // Empty key means user wants to delete it
            settings_mgr
                .delete_api_key(provider_id)
                .map_err(|e| e.to_string())?;
        } else {
            settings_mgr
                .set_api_key(provider_id, key)
                .map_err(|e| e.to_string())?;
        }
    }

    // Hot-reload LLM Gateway with new settings
    drop(settings_mgr);
    let mut llm = state.llm.lock().map_err(|e| e.to_string())?;
    let settings_mgr = state.settings.lock().map_err(|e| e.to_string())?;
    llm.reload(&settings_mgr).map_err(|e| e.to_string())?;

    Ok(())
}

/// Validate a provider configuration by testing the connection.
#[tauri::command(async)]
pub async fn validate_provider(
    state: State<'_, AiState>,
    provider: ProviderConfig,
    api_key: String,
) -> Result<bool, String> {
    use crate::core::settings::ProviderType;

    let jobs = state.jobs.clone();
    let provider_name = provider.name.clone();

    // If no API key provided by frontend, try to fetch from Keychain
    let effective_key = if api_key.is_empty() {
        let settings_mgr = state.settings.lock().map_err(|e| e.to_string())?;
        settings_mgr.get_api_key(&provider.id).map_err(|e| e.to_string())?.unwrap_or_default()
    } else {
        api_key
    };
    let is_cloud = !matches!(provider.provider_type, ProviderType::Ollama);

    // Build the HTTP request directly (no spawn_blocking — we're already async)
    let client = reqwest::Client::new();
    let timeout = std::time::Duration::from_secs(15);

    let validation_result = match provider.provider_type {
        ProviderType::DeepSeek => {
            let url = format!(
                "{}/v1/chat/completions",
                provider.endpoint.trim_end_matches('/')
            );
            client
                .post(&url)
                .header("Authorization", format!("Bearer {}", effective_key))
                .header("Content-Type", "application/json")
                .timeout(timeout)
                .json(&serde_json::json!({
                    "model": provider.model,
                    "messages": [{"role": "user", "content": "hi"}],
                    "max_tokens": 1
                }))
                .send()
                .await
        }
        ProviderType::Ollama => {
            let url = format!(
                "{}/api/tags",
                provider.endpoint.trim_end_matches('/')
            );
            client.get(&url).timeout(timeout).send().await
        }
        ProviderType::OpenAiCompat => {
            let url = format!(
                "{}/v1/chat/completions",
                provider.endpoint.trim_end_matches('/')
            );
            client
                .post(&url)
                .header("Authorization", format!("Bearer {}", effective_key))
                .header("Content-Type", "application/json")
                .timeout(timeout)
                .json(&serde_json::json!({
                    "model": provider.model,
                    "messages": [{"role": "user", "content": "hi"}],
                    "max_tokens": 1
                }))
                .send()
                .await
        }
    };

    match validation_result {
        Ok(resp) if resp.status().is_success() => {
            if is_cloud {
                let _ = jobs.record_audit(&provider_name, "validate", 0, true);
            }
            Ok(true)
        }
        Ok(resp) => {
            log::warn!("Provider validation failed with status: {}", resp.status());
            if is_cloud {
                let _ = jobs.record_audit(&provider_name, "validate", 0, false);
            }
            Ok(false)
        }
        Err(e) => {
            log::warn!("Provider validation error: {}", e);
            if is_cloud {
                let _ = jobs.record_audit(&provider_name, "validate", 0, false);
            }
            Ok(false)
        }
    }
}

/// List available models from an Ollama endpoint.
#[tauri::command(async)]
pub async fn list_ollama_models(
    _state: State<'_, AiState>,
    endpoint: String,
) -> Result<Vec<String>, String> {
    use crate::core::llm::ollama::OllamaProvider;

    let result = tokio::task::spawn_blocking(move || {
        let provider = OllamaProvider::new(endpoint, String::new());
        provider.list_models().map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("Task join error: {}", e))?;

    result
}

// ─── Job Queue Commands ─────────────────────────────────────────────────────

/// List recent jobs from the queue.
#[tauri::command]
pub fn list_jobs(
    state: State<'_, AiState>,
    limit: Option<u32>,
) -> Result<Vec<JobRecord>, String> {
    state
        .jobs
        .list_jobs(limit.unwrap_or(20))
        .map_err(|e| e.to_string())
}

/// Cancel a pending or blocked job.
#[tauri::command]
pub fn cancel_job(
    state: State<'_, AiState>,
    job_id: String,
) -> Result<(), String> {
    state.jobs.cancel_job(&job_id).map_err(|e| e.to_string())
}

// ─── Privacy / Audit Commands ───────────────────────────────────────────────

/// Get audit log entries (most recent first).
#[tauri::command]
pub fn get_audit_log(
    state: State<'_, AiState>,
    limit: Option<u32>,
) -> Result<Vec<AuditRecord>, String> {
    let jobs = &state.jobs;
    // Audit log is stored in the same SQLite database as jobs.
    jobs.query_audit_log(limit.unwrap_or(50))
        .map_err(|e| e.to_string())
}

// ─── Helper Functions ───────────────────────────────────────────────────────

/// Compute cosine similarity between two vectors.
fn cosine_sim(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }

    let mut dot = 0.0_f32;
    let mut norm_a = 0.0_f32;
    let mut norm_b = 0.0_f32;

    for (x, y) in a.iter().zip(b.iter()) {
        dot += x * y;
        norm_a += x * x;
        norm_b += y * y;
    }

    let denom = norm_a.sqrt() * norm_b.sqrt();
    if denom < f32::EPSILON {
        0.0
    } else {
        dot / denom
    }
}
