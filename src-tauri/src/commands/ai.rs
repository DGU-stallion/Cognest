// Cognest IPC Command Layer — AI Commands
//
// Thin #[tauri::command] functions for AI subsystem.
// Forwards to EmbeddingEngine, JobQueue, SettingsManager,
// WritingAgent (via Rig), and ReflectionAgent.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::{Emitter, State};
use tokio_util::sync::CancellationToken;

use crate::commands::AppState;
use crate::core::reflection::ViewSpec;
use crate::core::embedding::EmbeddingEngine;
use crate::core::jobs::{AuditRecord, JobQueue, JobRecord};
use crate::core::rig_agents::types::{ChatMessage, Role};
use crate::core::rig_agents::registry::AgentRegistry;
use crate::core::rig_agents::stream_adapter::stream_to_tauri_events;
use crate::core::rig_agents::AgentError;
use crate::core::settings::{AppSettings, ProviderConfig, SettingsManager};

// ─── AI Application State ───────────────────────────────────────────────────

/// Shared AI state holding subsystem instances behind Arc/Mutex guards.
pub struct AiState {
    pub embedding: Arc<Mutex<EmbeddingEngine>>,
    pub jobs: Arc<JobQueue>,
    pub settings: Arc<Mutex<SettingsManager>>,
}

// ─── Rig Agent State ────────────────────────────────────────────────────────

/// Tauri managed state for the Rig Agent layer.
///
/// Contains the AgentRegistry which manages all Rig Agent instances.
/// This is separate from the legacy AiState to allow incremental migration.
pub struct RigState {
    pub registry: AgentRegistry,
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

// ─── Writing Agent Commands (Rig-based async) ──────────────────────────────

/// Synchronous writing chat via Rig Agent (returns full response).
///
/// Uses AgentRegistry to obtain WritingRigAgent, calls `chat()`, and returns
/// the complete response. Fully async.
#[tauri::command(async)]
pub async fn writing_chat(
    rig_state: State<'_, RigState>,
    app_state: State<'_, AppState>,
    app: tauri::AppHandle,
    article_id: String,
    message: String,
    history: Vec<ChatMessage>,
) -> Result<String, String> {
    // 1. Get WritingAgent from registry
    let agent = rig_state
        .registry
        .writing_agent()
        .await
        .map_err(|e| handle_agent_error(&app, e))?;

    // 2. Load article content from repo
    let article_content = {
        let repo = app_state.repo.lock().map_err(|e| e.to_string())?;
        match repo.read_article(&article_id) {
            Ok((_meta, body)) => body,
            Err(_) => article_id.clone(),
        }
    };

    // 3. Find related fragments via embedding (best-effort)
    let related_fragments = find_related_fragments_for_writing(&rig_state, &article_content);

    // 4. Convert ChatMessage history to rig::completion::Message
    let rig_history = convert_history_to_rig(&history);

    // 5. Call Rig WritingAgent.chat()
    let response = agent
        .chat(&article_content, &related_fragments, &message, rig_history)
        .await
        .map_err(|e| handle_agent_error(&app, e))?;

    Ok(response)
}

/// Streaming writing chat via Rig Agent — emits "writing_chunk" events to the frontend.
///
/// Uses AgentRegistry to obtain WritingRigAgent, calls `stream_chat()`,
/// then adapts the stream to Tauri events via `stream_to_tauri_events()`.
/// Fully async.
#[tauri::command(async)]
pub async fn writing_stream_chat(
    rig_state: State<'_, RigState>,
    app_state: State<'_, AppState>,
    app: tauri::AppHandle,
    article_id: String,
    message: String,
    history: Vec<ChatMessage>,
) -> Result<(), String> {
    // 1. Get WritingAgent from registry
    let agent = rig_state
        .registry
        .writing_agent()
        .await
        .map_err(|e| handle_agent_error(&app, e))?;

    // 2. Load article content from repo
    let article_content = {
        let repo = app_state.repo.lock().map_err(|e| e.to_string())?;
        match repo.read_article(&article_id) {
            Ok((_meta, body)) => body,
            Err(_) => article_id.clone(),
        }
    };

    // 3. Find related fragments via embedding (best-effort)
    let related_fragments = find_related_fragments_for_writing(&rig_state, &article_content);

    // 4. Convert ChatMessage history to rig::completion::Message
    let rig_history = convert_history_to_rig(&history);

    // 5. Call Rig WritingAgent.stream_chat()
    let stream = agent
        .stream_chat(&article_content, &related_fragments, &message, rig_history)
        .await
        .map_err(|e| handle_agent_error(&app, e))?;

    // 6. Adapt stream to Tauri events (with cancellation support)
    let cancel_token = CancellationToken::new();
    let _result = stream_to_tauri_events(stream, &app, cancel_token).await;

    Ok(())
}

// ─── Writing Agent Helper Functions ─────────────────────────────────────────

/// Find related fragments for the writing context using the embedding engine.
///
/// Returns (content, similarity) pairs. Best-effort: returns empty vec on failure.
fn find_related_fragments_for_writing(
    _rig_state: &State<'_, RigState>,
    _article_content: &str,
) -> Vec<(String, f64)> {
    // Note: Full embedding integration happens in task 5.3 (app startup integration).
    // For now return empty — the WritingAgent handles empty fragments gracefully.
    Vec::new()
}

/// Convert frontend ChatMessage history to rig::completion::Message format.
fn convert_history_to_rig(history: &[ChatMessage]) -> Vec<rig::completion::Message> {
    history
        .iter()
        .filter_map(|msg| match msg.role {
            Role::User => Some(rig::completion::Message::user(&msg.content)),
            Role::Assistant => Some(rig::completion::Message::assistant(&msg.content)),
            Role::System => None, // System messages are handled via agent preamble
        })
        .collect()
}

/// Handle AgentError: emit provider fallback notification if applicable,
/// and return a user-friendly error string.
fn handle_agent_error(app: &tauri::AppHandle, error: AgentError) -> String {
    match &error {
        AgentError::ProviderFallback { from, to } => {
            // Notify frontend about provider fallback via Tauri event (Req 2.6)
            let payload = serde_json::json!({
                "from": from,
                "to": to,
                "message": format!("Provider 回退: {} → {}", from, to)
            });
            let _ = app.emit("provider_fallback", payload);
            // Still return as error string for command result
            format!("Provider 回退: {} → {}", from, to)
        }
        AgentError::AgentUnavailable { reason, .. } => {
            format!("AI 模型未配置: {}", reason)
        }
        AgentError::NoProvider => {
            "无可用 AI 模型，请在设置中配置".to_string()
        }
        AgentError::Timeout => {
            "请求超时，请稍后重试".to_string()
        }
        AgentError::Cancelled => {
            "请求已取消".to_string()
        }
        _ => {
            // Privacy: neutral error message, don't leak provider details
            "操作失败，请检查网络后重试".to_string()
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
///
/// Uses direct HTTP call to the configured LLM provider (via settings).
#[tauri::command(async)]
pub async fn generate_view(
    state: State<'_, AiState>,
    prompt: String,
) -> Result<ViewSpec, String> {
    let jobs = state.jobs.clone();

    // Load settings to resolve provider
    let (endpoint, api_key, model, provider_name) = {
        let settings_mgr = state.settings.lock().map_err(|e| e.to_string())?;
        let app_settings = settings_mgr.load().map_err(|e| e.to_string())?;

        // Find the default provider or first enabled provider
        let default_id = app_settings.routing.default_provider.clone();
        let provider = app_settings
            .providers
            .iter()
            .find(|p| p.enabled && Some(p.id.clone()) == default_id)
            .or_else(|| app_settings.providers.iter().find(|p| p.enabled))
            .ok_or_else(|| "无可用 Provider，请在设置中配置".to_string())?
            .clone();

        let api_key = settings_mgr
            .get_api_key(&provider.id)
            .ok()
            .flatten()
            .unwrap_or_default();

        (provider.endpoint.clone(), api_key, provider.model.clone(), provider.name.clone())
    };

    let is_cloud = !provider_name.is_empty() && provider_name.to_lowercase() != "ollama";

    let system_msg = serde_json::json!({
        "role": "system",
        "content": concat!(
            "你是 Cognest 的视图生成助手。根据用户的自然语言描述，生成一个 ViewSpec JSON。",
            "支持的类型: graph, timeline, list, chart, summary。",
            "返回纯 JSON，不要额外解释。",
            "JSON 格式: {\"id\": \"<uuid>\", \"type\": \"<type>\", \"title\": \"<标题>\", ",
            "\"query\": \"<原始提示>\", \"created\": \"\", \"pinned\": false, ",
            "\"config\": {}, \"data\": {\"markdown\": \"<内容>\", \"stats\": {}}}"
        )
    });
    let user_msg = serde_json::json!({
        "role": "user",
        "content": prompt.clone()
    });

    let url = {
        let base = endpoint.trim_end_matches('/');
        // Avoid double /v1 if user already included it in the endpoint
        if base.ends_with("/v1") {
            format!("{}/chat/completions", base)
        } else {
            format!("{}/v1/chat/completions", base)
        }
    };
    let body = serde_json::json!({
        "model": model,
        "messages": [system_msg, user_msg],
        "response_format": {
            "type": "json_object"
        }
    });

    let client = reqwest::Client::new();
    let mut req = client
        .post(&url)
        .header("Content-Type", "application/json")
        .timeout(Duration::from_secs(60));

    if !api_key.is_empty() {
        req = req.header("Authorization", format!("Bearer {}", api_key));
    }

    let response = req.json(&body).send().await.map_err(|e| {
        log::error!("[generate_view] HTTP request failed: {}", e);
        if is_cloud {
            let _ = jobs.record_audit(&provider_name, "generate_view", 0, false);
        }
        format!("操作失败，请检查网络后重试")
    })?;

    if !response.status().is_success() {
        let status = response.status();
        let body_text = response.text().await.unwrap_or_default();
        log::error!("[generate_view] API error {}: {}", status, body_text);
        if is_cloud {
            let _ = jobs.record_audit(&provider_name, "generate_view", 0, false);
        }
        return Err(format!("AI 请求失败 ({})", status));
    }

    #[derive(Deserialize)]
    struct ApiResponse { choices: Vec<ApiChoice>, usage: Option<ApiUsage> }
    #[derive(Deserialize)]
    struct ApiChoice { message: Option<ApiMsg> }
    #[derive(Deserialize)]
    struct ApiMsg { content: Option<String> }
    #[derive(Deserialize)]
    struct ApiUsage { total_tokens: u32 }

    let api_resp: ApiResponse = response.json().await.map_err(|e| {
        format!("响应解析失败: {}", e)
    })?;

    let total_tokens = api_resp.usage.map(|u| u.total_tokens).unwrap_or(0);
    if is_cloud {
        let _ = jobs.record_audit(&provider_name, "generate_view", total_tokens, true);
    }

    let content = api_resp.choices.first()
        .and_then(|c| c.message.as_ref())
        .and_then(|m| m.content.clone())
        .unwrap_or_default();

    let mut view_spec: ViewSpec =
        serde_json::from_str(&content).map_err(|e| {
            format!("视图 JSON 解析失败: {} — 原始响应: {}", e, content)
        })?;

    if view_spec.created.is_empty() {
        view_spec.created = chrono::Local::now().to_rfc3339();
    }
    if view_spec.query.is_empty() {
        view_spec.query = prompt;
    }

    Ok(view_spec)
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
///
/// After persisting settings, triggers hot-reload of the Rig AgentRegistry
/// so that updated Provider config takes effect immediately without restarting the app.
#[tauri::command(async)]
pub async fn save_ai_settings(
    state: State<'_, AiState>,
    rig_state: State<'_, RigState>,
    settings: AppSettings,
    api_keys: HashMap<String, String>,
) -> Result<(), String> {
    // --- Synchronous section: all std::sync::Mutex work before any .await ---
    let settings_for_reload = settings.clone();

    {
        let settings_mgr = state.settings.lock().map_err(|e| e.to_string())?;

        // Save settings to encrypted file
        settings_mgr.save(&settings).map_err(|e| e.to_string())?;

        // Save each API key to macOS Keychain
        for (provider_id, key) in &api_keys {
            if key.is_empty() {
                settings_mgr
                    .delete_api_key(provider_id)
                    .map_err(|e| e.to_string())?;
            } else {
                settings_mgr
                    .set_api_key(provider_id, key)
                    .map_err(|e| e.to_string())?;
            }
        }
    } // settings_mgr dropped here

    // --- Async section: Rig AgentRegistry reload ---
    // AgentRegistry::reload() internally calls build_inner (sync) then acquires a
    // tokio::RwLock (async). We must not hold std::sync::MutexGuard across await.
    rig_state
        .registry
        .reload_with_sync_settings(&settings_for_reload, &state.settings)
        .await
        .map_err(|e| e.to_string())?;
    log::info!("AgentRegistry 热重载完成");

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
    let url = format!("{}/api/tags", endpoint.trim_end_matches('/'));
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|e| e.to_string())?;

    let response = client.get(&url).send().await.map_err(|e| {
        if e.is_timeout() {
            "Ollama 请求超时".to_string()
        } else {
            "Ollama 连接失败 — is Ollama running?".to_string()
        }
    })?;

    if !response.status().is_success() {
        return Err(format!("Ollama HTTP {}", response.status()));
    }

    #[derive(Deserialize)]
    struct TagsResponse { models: Vec<ModelInfo> }
    #[derive(Deserialize)]
    struct ModelInfo { name: String }

    let tags: TagsResponse = response.json().await.map_err(|e| {
        format!("Ollama 响应解析失败: {}", e)
    })?;

    Ok(tags.models.into_iter().map(|m| m.name).collect())
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

// ─── Curate Fragment Command ────────────────────────────────────────────────

/// Trigger auto-tagging for a newly created fragment.
///
/// Calls the Curator Agent's `generate_tags` method and updates the fragment's
/// frontmatter with the generated tags. Fire-and-forget from the frontend.
#[tauri::command(async)]
pub async fn curate_fragment(
    rig_state: State<'_, RigState>,
    app_state: State<'_, AppState>,
    fragment_id: String,
) -> Result<Vec<String>, String> {
    // 1. Read fragment content
    let content = {
        let repo = app_state.repo.lock().map_err(|e| e.to_string())?;
        let (_meta, body) = repo.read_fragment(&fragment_id).map_err(|e| e.to_string())?;
        body
    };

    // 2. Get Curator Agent
    let agent = match rig_state.registry.curator_agent().await {
        Ok(a) => a,
        Err(e) => {
            log::info!("[curate_fragment] Curator Agent not available: {}", e);
            return Ok(vec![]);
        }
    };

    // 3. Generate tags
    let tags = agent
        .generate_tags(&content)
        .await
        .unwrap_or_else(|e| {
            log::warn!("[curate_fragment] Tag generation failed: {}", e);
            vec![]
        });

    if tags.is_empty() {
        return Ok(vec![]);
    }

    // 4. Update fragment frontmatter with generated tags
    {
        let repo = app_state.repo.lock().map_err(|e| e.to_string())?;
        // Re-read to get current meta, update tags, and write back
        let (mut meta, body) = repo.read_fragment(&fragment_id).map_err(|e| e.to_string())?;
        meta.tags = tags.clone();
        let document = crate::core::frontmatter::serialize(&meta, &body)
            .map_err(|e| e.to_string())?;
        let file_path = repo.find_fragment_path(&fragment_id).map_err(|e| e.to_string())?;
        std::fs::write(&file_path, &document).map_err(|e| e.to_string())?;
    }

    log::info!("[curate_fragment] Generated tags for {}: {:?}", fragment_id, tags);
    Ok(tags)
}
