// Cognest Core — File Watcher
//
// Monitors capture/ and articles/ directories for .md file changes.
// Uses notify crate with 500ms debounce to batch events, then updates IndexDb.
// Emits "index_updated" event to frontend via Tauri after processing.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use notify::{Event, EventKind, RecursiveMode, Watcher};
use sha2::{Digest, Sha256};

use super::frontmatter;
use super::index::{ArticleRecord, FragmentRecord, IndexDb};
use super::repo::{ArticleMeta, ArticleStatus, FragmentMeta};

/// Errors from watcher operations.
#[derive(Debug, thiserror::Error)]
pub enum WatcherError {
    #[error("Notify 错误: {0}")]
    Notify(#[from] notify::Error),

    #[error("IO 错误: {0}")]
    Io(#[from] std::io::Error),
}

/// Debounced file event categorized by action type.
#[derive(Debug, Clone, PartialEq, Eq)]
enum FileAction {
    Created,
    Modified,
    Deleted,
}

/// Start the file watcher in a background std::thread.
///
/// Uses std::sync::mpsc (no tokio runtime required).
pub fn start_watcher(
    vault_path: PathBuf,
    index: Arc<Mutex<IndexDb>>,
    app_handle: tauri::AppHandle,
) -> Result<WatcherHandle, WatcherError> {
    let (tx, rx) = std::sync::mpsc::channel::<Event>();

    // Set up the notify watcher
    let mut watcher = notify::recommended_watcher(move |res: Result<Event, notify::Error>| {
        if let Ok(event) = res {
            let _ = tx.send(event);
        }
    })?;

    // Watch capture/ and articles/ directories (create if missing)
    let capture_dir = vault_path.join("capture");
    let articles_dir = vault_path.join("articles");

    std::fs::create_dir_all(&capture_dir)?;
    std::fs::create_dir_all(&articles_dir)?;

    watcher.watch(&capture_dir, RecursiveMode::Recursive)?;
    watcher.watch(&articles_dir, RecursiveMode::Recursive)?;

    log::info!(
        "File watcher started: monitoring {:?} and {:?}",
        capture_dir,
        articles_dir
    );

    // Spawn debounce + processing in a background thread (no tokio needed)
    let vault = vault_path.clone();
    let thread_handle = std::thread::spawn(move || {
        debounce_and_process(rx, vault, index, app_handle);
    });

    Ok(WatcherHandle {
        _watcher: watcher,
        _thread_handle: thread_handle,
    })
}

/// Handle to the running watcher. Dropping this stops the watcher.
pub struct WatcherHandle {
    _watcher: notify::RecommendedWatcher,
    _thread_handle: std::thread::JoinHandle<()>,
}

/// Main debounce loop using std::sync::mpsc with recv_timeout.
fn debounce_and_process(
    rx: std::sync::mpsc::Receiver<Event>,
    vault_path: PathBuf,
    index: Arc<Mutex<IndexDb>>,
    app_handle: tauri::AppHandle,
) {
    loop {
        // Wait for the first event (blocking)
        let first_event = match rx.recv() {
            Ok(ev) => ev,
            Err(_) => break, // Channel closed
        };

        // Collect events during the 500ms debounce window
        let mut pending: HashMap<PathBuf, FileAction> = HashMap::new();
        collect_event(&first_event, &vault_path, &mut pending);

        let deadline = Instant::now() + Duration::from_millis(500);
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                break;
            }
            match rx.recv_timeout(remaining) {
                Ok(ev) => collect_event(&ev, &vault_path, &mut pending),
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => break,
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return,
            }
        }

        // Process the collected batch
        if !pending.is_empty() {
            let changed = process_batch(&pending, &vault_path, &index);
            if changed {
                use tauri::Emitter;
                let _ = app_handle.emit("index_updated", ());
            }
        }
    }
}

/// Classify a notify event and add relevant .md file paths to the pending map.
fn collect_event(event: &Event, vault_path: &Path, pending: &mut HashMap<PathBuf, FileAction>) {
    let action = match &event.kind {
        EventKind::Create(_) => FileAction::Created,
        EventKind::Modify(_) => FileAction::Modified,
        EventKind::Remove(_) => FileAction::Deleted,
        _ => return,
    };

    for path in &event.paths {
        // Only process .md files
        if path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }

        // Only process files under capture/ or articles/
        let capture_dir = vault_path.join("capture");
        let articles_dir = vault_path.join("articles");
        if !path.starts_with(&capture_dir) && !path.starts_with(&articles_dir) {
            continue;
        }

        // For the same path, later actions override earlier ones,
        // except: Created then Deleted = remove from pending
        match pending.get(path) {
            Some(FileAction::Created) if action == FileAction::Deleted => {
                pending.remove(path);
            }
            _ => {
                pending.insert(path.clone(), action.clone());
            }
        }
    }
}

/// Process a batch of file events: insert/update/delete index records.
/// Returns true if any index changes were made.
fn process_batch(
    pending: &HashMap<PathBuf, FileAction>,
    vault_path: &Path,
    index: &Arc<Mutex<IndexDb>>,
) -> bool {
    let mut changed = false;

    for (path, action) in pending {
        let result = match action {
            FileAction::Created => handle_created(path, vault_path, index),
            FileAction::Modified => handle_modified(path, vault_path, index),
            FileAction::Deleted => handle_deleted(path, vault_path, index),
        };

        match result {
            Ok(true) => changed = true,
            Ok(false) => {}
            Err(e) => {
                log::warn!("文件监听处理失败 {}: {}", path.display(), e);
            }
        }
    }

    changed
}

/// Handle a newly created .md file: parse and insert into index.
fn handle_created(
    path: &Path,
    vault_path: &Path,
    index: &Arc<Mutex<IndexDb>>,
) -> Result<bool, String> {
    let content = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let hash = compute_hash(content.as_bytes());

    if is_fragment_path(path, vault_path) {
        let parsed = frontmatter::parse::<FragmentMeta>(&content).map_err(|e| e.to_string())?;
        let record = FragmentRecord {
            id: parsed.meta.id,
            content: parsed.body,
            created_at: parsed.meta.created.to_rfc3339(),
            source: parsed.meta.source,
            tags: parsed.meta.tags,
            topics: parsed.meta.topics,
            content_hash: hash,
        };
        let db = index.lock().map_err(|e| e.to_string())?;
        db.insert_fragment(&record).map_err(|e| e.to_string())?;
        log::info!("索引新增碎片: {}", record.id);
        Ok(true)
    } else if is_article_path(path, vault_path) {
        let parsed = frontmatter::parse::<ArticleMeta>(&content).map_err(|e| e.to_string())?;
        let status_str = match parsed.meta.status {
            ArticleStatus::Draft => "draft",
            ArticleStatus::Editing => "editing",
            ArticleStatus::Completed => "completed",
        };
        let record = ArticleRecord {
            id: parsed.meta.id,
            title: parsed.meta.title,
            status: status_str.to_string(),
            created_at: parsed.meta.created.to_rfc3339(),
            updated_at: parsed.meta.updated.to_rfc3339(),
            tags: parsed.meta.tags,
            content_hash: hash,
        };
        let db = index.lock().map_err(|e| e.to_string())?;
        db.insert_article(&record).map_err(|e| e.to_string())?;
        log::info!("索引新增文章: {}", record.id);
        Ok(true)
    } else {
        Ok(false)
    }
}

/// Handle a modified .md file: compare hash, update only if content changed.
fn handle_modified(
    path: &Path,
    vault_path: &Path,
    index: &Arc<Mutex<IndexDb>>,
) -> Result<bool, String> {
    let content = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let new_hash = compute_hash(content.as_bytes());

    if is_fragment_path(path, vault_path) {
        let parsed = frontmatter::parse::<FragmentMeta>(&content).map_err(|e| e.to_string())?;
        let id = &parsed.meta.id;

        // Compare with stored hash
        let db = index.lock().map_err(|e| e.to_string())?;
        let stored_hash = db.get_fragment_hash(id).map_err(|e| e.to_string())?;

        if stored_hash.as_deref() == Some(new_hash.as_str()) {
            // Hash unchanged, skip update
            return Ok(false);
        }

        let record = FragmentRecord {
            id: id.clone(),
            content: parsed.body,
            created_at: parsed.meta.created.to_rfc3339(),
            source: parsed.meta.source,
            tags: parsed.meta.tags,
            topics: parsed.meta.topics,
            content_hash: new_hash,
        };

        if stored_hash.is_some() {
            db.update_fragment(&record).map_err(|e| e.to_string())?;
            log::info!("索引更新碎片: {}", record.id);
        } else {
            // Not in index yet — treat as create
            db.insert_fragment(&record).map_err(|e| e.to_string())?;
            log::info!("索引新增碎片（修改事件）: {}", record.id);
        }
        Ok(true)
    } else if is_article_path(path, vault_path) {
        let parsed = frontmatter::parse::<ArticleMeta>(&content).map_err(|e| e.to_string())?;
        let id = &parsed.meta.id;

        let db = index.lock().map_err(|e| e.to_string())?;
        let stored_hash = db.get_article_hash(id).map_err(|e| e.to_string())?;

        if stored_hash.as_deref() == Some(new_hash.as_str()) {
            return Ok(false);
        }

        let status_str = match parsed.meta.status {
            ArticleStatus::Draft => "draft",
            ArticleStatus::Editing => "editing",
            ArticleStatus::Completed => "completed",
        };
        let record = ArticleRecord {
            id: id.clone(),
            title: parsed.meta.title,
            status: status_str.to_string(),
            created_at: parsed.meta.created.to_rfc3339(),
            updated_at: parsed.meta.updated.to_rfc3339(),
            tags: parsed.meta.tags,
            content_hash: new_hash,
        };

        if stored_hash.is_some() {
            db.update_article(&record).map_err(|e| e.to_string())?;
            log::info!("索引更新文章: {}", record.id);
        } else {
            db.insert_article(&record).map_err(|e| e.to_string())?;
            log::info!("索引新增文章（修改事件）: {}", record.id);
        }
        Ok(true)
    } else {
        Ok(false)
    }
}

/// Handle a deleted .md file: extract ID from filename and remove from index.
fn handle_deleted(
    path: &Path,
    vault_path: &Path,
    index: &Arc<Mutex<IndexDb>>,
) -> Result<bool, String> {
    // Extract the file ID from the filename (stem without extension)
    let id = path
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or_else(|| "无法提取文件 ID".to_string())?
        .to_string();

    let db = index.lock().map_err(|e| e.to_string())?;

    if is_fragment_path(path, vault_path) {
        db.delete_fragment(&id).map_err(|e| e.to_string())?;
        log::info!("索引删除碎片: {}", id);
        Ok(true)
    } else if is_article_path(path, vault_path) {
        db.delete_article(&id).map_err(|e| e.to_string())?;
        log::info!("索引删除文章: {}", id);
        Ok(true)
    } else {
        Ok(false)
    }
}

// ─── Helper Functions ────────────────────────────────────────────────────────

/// Check if a path is under the capture/ directory.
fn is_fragment_path(path: &Path, vault_path: &Path) -> bool {
    path.starts_with(vault_path.join("capture"))
}

/// Check if a path is under the articles/ directory.
fn is_article_path(path: &Path, vault_path: &Path) -> bool {
    path.starts_with(vault_path.join("articles"))
}

/// Compute SHA-256 hash of bytes, returned as lowercase hex string.
fn compute_hash(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    let result = hasher.finalize();
    format!("{:x}", result)
}
