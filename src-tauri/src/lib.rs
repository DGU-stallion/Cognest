mod commands;
mod core;

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use commands::AppState;
use tauri::{Emitter, Manager};

use crate::core::index::IndexDb;
use crate::core::repo::FileRepo;
use crate::core::watcher;

/// Determine the vault path. For now uses ~/CognestVault as default.
fn default_vault_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("CognestVault")
}

/// Progressive startup: check index integrity, rebuild in background if needed.
/// This runs asynchronously so the UI thread is never blocked.
fn progressive_startup(app: &tauri::App) {
    let app_state: &AppState = app.state::<AppState>().inner();
    let vault_path = {
        let repo = app_state.repo.lock().expect("无法获取 repo 锁");
        repo.vault_path().to_path_buf()
    };
    let index_arc = app_state.index_arc.clone();
    let handle = app.handle().clone();

    // Check integrity and determine if rebuild is needed
    let needs_rebuild = {
        let index = index_arc.lock().expect("无法获取 index 锁");

        // Check database integrity
        let is_valid = index.check_integrity().unwrap_or(false);
        if !is_valid {
            log::warn!("索引数据库完整性检查失败，将重建索引");
            true
        } else {
            // Check if index is empty but vault has files
            let fragment_count = index.fragment_count().unwrap_or(0);
            if fragment_count == 0 {
                let capture_dir = vault_path.join("capture");
                let has_files = capture_dir.exists()
                    && std::fs::read_dir(&capture_dir)
                        .map(|mut entries| entries.next().is_some())
                        .unwrap_or(false);
                if has_files {
                    log::info!("索引为空但 vault 中有文件，将全量构建");
                    true
                } else {
                    false
                }
            } else {
                false
            }
        }
    };

    if needs_rebuild {
        // Emit rebuilding event to frontend (non-blocking)
        let _ = handle.emit("index_rebuilding", ());

        // Spawn background rebuild task in a std thread (no tokio runtime needed)
        let rebuild_vault_path = vault_path.clone();
        let rebuild_index = index_arc.clone();
        let rebuild_handle = handle.clone();

        std::thread::spawn(move || {
            log::info!("开始后台全量索引构建…");

            let result = {
                let repo = FileRepo::new(rebuild_vault_path);
                let index = rebuild_index.lock().map_err(|e| e.to_string());
                match index {
                    Ok(idx) => idx.rebuild_from_vault(&repo).map_err(|e| e.to_string()),
                    Err(e) => Err(e),
                }
            };

            match result {
                Ok(report) => {
                    log::info!(
                        "索引构建完成: {} 碎片, {} 文章, {} 跳过",
                        report.fragments_indexed,
                        report.articles_indexed,
                        report.skipped.len()
                    );
                }
                Err(e) => {
                    log::error!("索引构建失败: {}", e);
                }
            }

            // Notify frontend that index is ready
            let _ = rebuild_handle.emit("index_updated", ());
        });
    } else {
        // Index is valid and populated — emit ready immediately
        let _ = handle.emit("index_updated", ());
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            if cfg!(debug_assertions) {
                app.handle().plugin(
                    tauri_plugin_log::Builder::default()
                        .level(log::LevelFilter::Info)
                        .build(),
                )?;
            }

            // Progressive startup: check index, rebuild in background if needed
            progressive_startup(app);

            // Start the file watcher after AppState is ready
            let app_state: &AppState = app.state::<AppState>().inner();
            let vault_path = {
                let repo = app_state.repo.lock().expect("无法获取 repo 锁");
                repo.vault_path().to_path_buf()
            };
            let index_arc = app_state.index_arc.clone();
            let watcher_handle = app.handle().clone();

            match watcher::start_watcher(vault_path, index_arc, watcher_handle) {
                Ok(handle) => {
                    // Store the watcher handle to keep it alive for the app lifetime
                    app.manage(handle);
                    log::info!("文件监听器启动成功");
                }
                Err(e) => {
                    log::error!("文件监听器启动失败: {}", e);
                }
            }

            Ok(())
        })
        .manage({
            let vault_path = default_vault_path();
            std::fs::create_dir_all(&vault_path).expect("无法创建 vault 目录");

            let repo = FileRepo::new(vault_path.clone());

            let db_path = vault_path.join(".cognest").join("index.sqlite");
            let index = IndexDb::open(&db_path).expect("无法打开索引数据库");
            index.init_schema().expect("无法初始化索引数据库 schema");

            let index_arc = Arc::new(Mutex::new(index));

            AppState {
                repo: Mutex::new(repo),
                index: Mutex::new(
                    IndexDb::open(&db_path).expect("无法打开索引数据库（命令层）"),
                ),
                index_arc: index_arc,
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::create_fragment,
            commands::list_fragments,
            commands::search_fragments,
            commands::update_fragment,
            commands::create_article,
            commands::get_article,
            commands::save_article,
            commands::delete_article,
            commands::export_article,
            commands::search_articles,
            commands::list_articles,
            commands::git_sync,
            commands::git_status,
            commands::get_stats,
            commands::get_top_tags,
            commands::get_counts,
            commands::get_vault_path,
            commands::get_initial_data,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
