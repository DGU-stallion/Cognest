// Cognest IPC Command Layer
//
// Thin #[tauri::command] functions that serialize/forward to Core modules.
// No business logic lives here — only state extraction, deserialization, and forwarding.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::core::git::{GitModule, SyncStatus};
use crate::core::index::{
    ArticleFilter, ArticleRecord, FragmentFilter, FragmentRecord, IndexDb, SearchResult,
    StatsResult,
};
use crate::core::repo::{ArticleMeta, ArticleStatus, FileRepo};

// ─── Application State ──────────────────────────────────────────────────────

/// Shared application state holding Core instances behind Mutex guards.
pub struct AppState {
    pub repo: Mutex<FileRepo>,
    pub index: Mutex<IndexDb>,
    /// Arc-wrapped IndexDb shared with the file watcher background task.
    pub index_arc: Arc<Mutex<IndexDb>>,
}

// ─── Response Types ─────────────────────────────────────────────────────────

/// Article response returned to frontend (meta + body).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArticleResponse {
    pub id: String,
    pub title: String,
    pub status: String,
    pub created: String,
    pub updated: String,
    pub tags: Vec<String>,
    pub body: String,
}

/// Counts response for the Discover page.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CountsResponse {
    pub fragments: u64,
    pub articles: u64,
}

/// Git sync result response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitSyncResponse {
    pub files_changed: usize,
    pub commit_sha: String,
}

/// Tag count entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TagCount {
    pub tag: String,
    pub count: u32,
}

// ─── Fragment Commands ───────────────────────────────────────────────────────

/// Create a new fragment with the given content.
#[tauri::command]
pub fn create_fragment(
    state: State<'_, AppState>,
    content: String,
) -> Result<String, String> {
    let repo = state.repo.lock().map_err(|e| e.to_string())?;
    repo.create_fragment(&content).map_err(|e| e.to_string())
}

/// List fragments from the index with optional filter and pagination.
#[tauri::command]
pub fn list_fragments(
    state: State<'_, AppState>,
    filter: Option<String>,
    offset: Option<u64>,
    limit: Option<u64>,
) -> Result<Vec<FragmentRecord>, String> {
    let index = state.index.lock().map_err(|e| e.to_string())?;
    let filt = match filter.as_deref() {
        Some("uncategorized") => FragmentFilter::Uncategorized,
        Some("categorized") => FragmentFilter::Categorized,
        _ => FragmentFilter::All,
    };
    index
        .list_fragments(filt, offset.unwrap_or(0), limit.unwrap_or(50))
        .map_err(|e| e.to_string())
}

/// Full-text search fragments by query string.
#[tauri::command]
pub fn search_fragments(
    state: State<'_, AppState>,
    query: String,
    limit: Option<usize>,
) -> Result<Vec<SearchResult>, String> {
    let index = state.index.lock().map_err(|e| e.to_string())?;
    index
        .search_fragments(&query, limit.unwrap_or(20))
        .map_err(|e| e.to_string())
}

/// Update a fragment's body content (keeps frontmatter, replaces body).
#[tauri::command]
pub fn update_fragment(
    state: State<'_, AppState>,
    id: String,
    content: String,
) -> Result<(), String> {
    let repo = state.repo.lock().map_err(|e| e.to_string())?;
    repo.update_fragment_content(&id, &content)
        .map_err(|e| e.to_string())
}

// ─── Article Commands ────────────────────────────────────────────────────────

/// Create a new article with the given title.
#[tauri::command]
pub fn create_article(
    state: State<'_, AppState>,
    title: String,
) -> Result<String, String> {
    let repo = state.repo.lock().map_err(|e| e.to_string())?;
    let id = repo.create_article(&title).map_err(|e| e.to_string())?;

    // Immediately update IndexDb so article is visible in list
    let index = state.index.lock().map_err(|e| e.to_string())?;
    let (meta, body) = repo.read_article(&id).map_err(|e| e.to_string())?;
    let status_str = match meta.status {
        ArticleStatus::Draft => "draft",
        ArticleStatus::Editing => "editing",
        ArticleStatus::Completed => "completed",
    };
    let content_for_hash = format!("{}{}", meta.title, body);
    let content_hash = FileRepo::content_hash(content_for_hash.as_bytes());
    let tags_json = serde_json::to_string(&meta.tags).unwrap_or_else(|_| "[]".to_string());
    let _ = tags_json; // used below
    let record = crate::core::index::ArticleRecord {
        id: id.clone(),
        title: meta.title,
        status: status_str.to_string(),
        created_at: meta.created.to_rfc3339(),
        updated_at: meta.updated.to_rfc3339(),
        tags: meta.tags,
        content_hash,
    };
    let _ = index.insert_article(&record);

    Ok(id)
}

/// Get a single article by ID (returns meta + body).
#[tauri::command]
pub fn get_article(
    state: State<'_, AppState>,
    id: String,
) -> Result<ArticleResponse, String> {
    let repo = state.repo.lock().map_err(|e| e.to_string())?;
    let (meta, body) = repo.read_article(&id).map_err(|e| e.to_string())?;
    let status_str = match meta.status {
        ArticleStatus::Draft => "draft",
        ArticleStatus::Editing => "editing",
        ArticleStatus::Completed => "completed",
    };
    Ok(ArticleResponse {
        id: meta.id,
        title: meta.title,
        status: status_str.to_string(),
        created: meta.created.to_rfc3339(),
        updated: meta.updated.to_rfc3339(),
        tags: meta.tags,
        body,
    })
}

/// Save (update) an article's content and metadata.
#[tauri::command]
pub fn save_article(
    state: State<'_, AppState>,
    id: String,
    title: String,
    status: String,
    tags: Vec<String>,
    body: String,
) -> Result<(), String> {
    let repo = state.repo.lock().map_err(|e| e.to_string())?;
    // Read existing to preserve created timestamp
    let (existing_meta, _) = repo.read_article(&id).map_err(|e| e.to_string())?;
    let new_status = match status.as_str() {
        "editing" => ArticleStatus::Editing,
        "completed" => ArticleStatus::Completed,
        _ => ArticleStatus::Draft,
    };
    let meta = ArticleMeta {
        id: id.clone(),
        title,
        status: new_status,
        created: existing_meta.created,
        updated: chrono::Utc::now(),
        tags,
    };
    repo.save_article(&id, &meta, &body)
        .map_err(|e| e.to_string())?;

    // Immediately update IndexDb so changes are reflected in article list
    let index = state.index.lock().map_err(|e| e.to_string())?;
    let content_for_hash = format!("{}{}", meta.title, body);
    let content_hash = FileRepo::content_hash(content_for_hash.as_bytes());
    let status_str = match meta.status {
        ArticleStatus::Draft => "draft",
        ArticleStatus::Editing => "editing",
        ArticleStatus::Completed => "completed",
    };
    let record = crate::core::index::ArticleRecord {
        id: id.clone(),
        title: meta.title,
        status: status_str.to_string(),
        created_at: meta.created.to_rfc3339(),
        updated_at: meta.updated.to_rfc3339(),
        tags: meta.tags,
        content_hash,
    };
    let _ = index.insert_article(&record); // insert_article uses INSERT OR REPLACE

    Ok(())
}

/// Delete an article by ID.
#[tauri::command]
pub fn delete_article(
    state: State<'_, AppState>,
    id: String,
) -> Result<(), String> {
    let repo = state.repo.lock().map_err(|e| e.to_string())?;
    repo.delete_article(&id).map_err(|e| e.to_string())
}

/// Export an article to a destination path.
#[tauri::command]
pub fn export_article(
    state: State<'_, AppState>,
    id: String,
    dest: String,
) -> Result<(), String> {
    let repo = state.repo.lock().map_err(|e| e.to_string())?;
    let dest_path = PathBuf::from(dest);
    repo.export_article(&id, &dest_path)
        .map_err(|e| e.to_string())
}

/// Full-text search articles by query string.
#[tauri::command]
pub fn search_articles(
    state: State<'_, AppState>,
    query: String,
    limit: Option<usize>,
) -> Result<Vec<SearchResult>, String> {
    let index = state.index.lock().map_err(|e| e.to_string())?;
    index
        .search_articles(&query, limit.unwrap_or(20))
        .map_err(|e| e.to_string())
}

/// List articles with optional status filter.
#[tauri::command]
pub fn list_articles(
    state: State<'_, AppState>,
    status: Option<String>,
) -> Result<Vec<ArticleRecord>, String> {
    let index = state.index.lock().map_err(|e| e.to_string())?;
    let filter = match status {
        Some(s) if !s.is_empty() => ArticleFilter::ByStatus(s),
        _ => ArticleFilter::All,
    };
    index.list_articles(filter).map_err(|e| e.to_string())
}

// ─── Git Commands ────────────────────────────────────────────────────────────

/// Trigger a git sync operation (add + commit + push).
#[tauri::command]
pub fn git_sync(state: State<'_, AppState>) -> Result<GitSyncResponse, String> {
    let repo = state.repo.lock().map_err(|e| e.to_string())?;
    let vault_path = repo.vault_path().to_path_buf();
    drop(repo); // Release lock before git operations

    let git = GitModule::open(&vault_path).map_err(|e| e.to_string())?;
    let result = git.sync().map_err(|e| e.to_string())?;
    Ok(GitSyncResponse {
        files_changed: result.files_changed,
        commit_sha: result.commit_sha,
    })
}

/// Get current git sync status.
#[tauri::command]
pub fn git_status(state: State<'_, AppState>) -> Result<SyncStatus, String> {
    let repo = state.repo.lock().map_err(|e| e.to_string())?;
    let vault_path = repo.vault_path().to_path_buf();
    drop(repo); // Release lock before git operations

    let git = match GitModule::open(&vault_path) {
        Ok(g) => g,
        Err(_) => {
            // If vault is not a git repo, report NoRemote
            return Ok(SyncStatus::NoRemote);
        }
    };
    git.sync_status().map_err(|e| e.to_string())
}

// ─── Statistics Commands ─────────────────────────────────────────────────────

/// Get activity statistics (fragment/article counts + daily activity).
#[tauri::command]
pub fn get_stats(
    state: State<'_, AppState>,
    days: Option<u32>,
) -> Result<StatsResult, String> {
    let index = state.index.lock().map_err(|e| e.to_string())?;
    index
        .stats_last_days(days.unwrap_or(30))
        .map_err(|e| e.to_string())
}

/// Get top N tags by frequency within the past N days.
#[tauri::command]
pub fn get_top_tags(
    state: State<'_, AppState>,
    days: Option<u32>,
    limit: Option<usize>,
) -> Result<Vec<TagCount>, String> {
    let index = state.index.lock().map_err(|e| e.to_string())?;
    let results = index
        .top_tags(days.unwrap_or(30), limit.unwrap_or(10))
        .map_err(|e| e.to_string())?;
    Ok(results
        .into_iter()
        .map(|(tag, count)| TagCount { tag, count })
        .collect())
}

/// Get total counts of fragments and articles.
#[tauri::command]
pub fn get_counts(
    state: State<'_, AppState>,
) -> Result<CountsResponse, String> {
    let index = state.index.lock().map_err(|e| e.to_string())?;
    let fragments = index.fragment_count().map_err(|e| e.to_string())?;
    let articles = index.article_count().map_err(|e| e.to_string())?;
    Ok(CountsResponse {
        fragments,
        articles,
    })
}

/// Get the vault root path.
#[tauri::command]
pub fn get_vault_path(
    state: State<'_, AppState>,
) -> Result<String, String> {
    let repo = state.repo.lock().map_err(|e| e.to_string())?;
    Ok(repo.vault_path().to_string_lossy().to_string())
}

// ─── Startup Commands ────────────────────────────────────────────────────────

/// Initial data response for quick first-screen render.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InitialDataResponse {
    pub fragments: Vec<FragmentRecord>,
    pub fragment_count: u64,
    pub article_count: u64,
}

/// Get the most recent 50 fragments for quick first-screen render.
/// This should resolve in <100ms when the index already exists.
#[tauri::command]
pub fn get_initial_data(
    state: State<'_, AppState>,
) -> Result<InitialDataResponse, String> {
    let index = state.index.lock().map_err(|e| e.to_string())?;
    let fragments = index
        .list_fragments(FragmentFilter::All, 0, 50)
        .map_err(|e| e.to_string())?;
    let fragment_count = index.fragment_count().map_err(|e| e.to_string())?;
    let article_count = index.article_count().map_err(|e| e.to_string())?;
    Ok(InitialDataResponse {
        fragments,
        fragment_count,
        article_count,
    })
}
