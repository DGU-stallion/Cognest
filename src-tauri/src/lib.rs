mod commands;
mod core;

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use commands::AppState;
use commands::ai::AiState;
use tauri::{Emitter, Manager};

use crate::core::agents::reflection::{ReflectionAgent, ReflectionScheduler};
use crate::core::embedding::EmbeddingEngine;
use crate::core::index::IndexDb;
use crate::core::jobs::{JobQueue, WorkerContext};
use crate::core::llm::LlmGateway;
use crate::core::repo::FileRepo;
use crate::core::settings::SettingsManager;
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

            match watcher::start_watcher(vault_path.clone(), index_arc.clone(), watcher_handle) {
                Ok(handle) => {
                    // Store the watcher handle to keep it alive for the app lifetime
                    app.manage(handle);
                    log::info!("文件监听器启动成功");
                }
                Err(e) => {
                    log::error!("文件监听器启动失败: {}", e);
                }
            }

            // ─── AI Subsystem Initialization ────────────────────────────────
            log::info!("正在初始化 AI 子系统…");

            // 1. SettingsManager
            let settings_manager = SettingsManager::new(&vault_path);
            let settings = Arc::new(Mutex::new(settings_manager));

            // 2. EmbeddingEngine (failure is non-fatal — AI is optional)
            let model_dir = vault_path.join(".cognest").join("models");
            let vectors_bin_path = vault_path.join(".cognest").join("vectors.bin");
            let _ = std::fs::create_dir_all(&model_dir);

            let embedding = match EmbeddingEngine::new(&model_dir, &vectors_bin_path) {
                Ok(engine) => {
                    log::info!("EmbeddingEngine 初始化成功");
                    Arc::new(Mutex::new(engine))
                }
                Err(e) => {
                    log::error!(
                        "EmbeddingEngine 初始化失败 (AI 功能不可用): {}",
                        e
                    );
                    // AI is optional — skip entire AI subsystem initialization.
                    // The app continues to function without AI features.
                    log::info!("AI 子系统跳过初始化，应用正常运行");
                    return Ok(());
                }
            };

            // 3. LlmGateway
            let llm = {
                let settings_guard = settings.lock().expect("无法获取 settings 锁");
                match LlmGateway::from_config(&settings_guard) {
                    Ok(gateway) => {
                        log::info!("LlmGateway 初始化成功");
                        Arc::new(Mutex::new(gateway))
                    }
                    Err(e) => {
                        log::info!(
                            "LlmGateway 初始化为空 (未配置 Provider): {}",
                            e
                        );
                        // Create an empty gateway — NoProvider errors will be returned on use
                        Arc::new(Mutex::new(LlmGateway::empty()))
                    }
                }
            };

            // 4. JobQueue — reuse the SQLite connection from AppState
            let db_path = vault_path.join(".cognest").join("index.sqlite");
            let job_db = Arc::new(Mutex::new(
                rusqlite::Connection::open(&db_path)
                    .expect("无法打开 SQLite 数据库 (job queue)"),
            ));

            let app_handle_for_emitter = app.handle().clone();
            let event_emitter: Box<dyn Fn(&str, &str) + Send + Sync> =
                Box::new(move |event: &str, payload: &str| {
                    let _ = app_handle_for_emitter.emit(event, payload.to_string());
                });

            let job_queue = Arc::new(JobQueue::new(job_db, event_emitter));
            log::info!("JobQueue 初始化成功");

            // 5. Create AiState and manage it
            let ai_state = AiState {
                embedding: embedding.clone(),
                llm: llm.clone(),
                jobs: job_queue.clone(),
                settings: settings.clone(),
            };
            app.manage(ai_state);
            log::info!("AiState 已注册到 Tauri");

            // ─── Background Threads ─────────────────────────────────────────

            // Thread 1: Embedding batch processing — polls for unembedded fragments
            let emb_thread_embedding = embedding.clone();
            let emb_thread_index = index_arc.clone();
            let emb_thread_jobs = job_queue.clone();
            std::thread::spawn(move || {
                log::info!("Embedding 后台批处理线程启动");
                loop {
                    std::thread::sleep(std::time::Duration::from_secs(30));

                    // Get all fragment IDs from the index
                    let all_ids = {
                        let idx = match emb_thread_index.lock() {
                            Ok(idx) => idx,
                            Err(_) => continue,
                        };
                        match idx.all_fragment_ids() {
                            Ok(ids) => ids,
                            Err(_) => continue,
                        }
                    };

                    if all_ids.is_empty() {
                        continue;
                    }

                    // Find unembedded fragments
                    let unembedded = {
                        let engine = match emb_thread_embedding.lock() {
                            Ok(e) => e,
                            Err(_) => continue,
                        };
                        engine.find_unembedded(&all_ids)
                    };

                    if unembedded.is_empty() {
                        continue;
                    }

                    log::info!("发现 {} 个未计算向量的碎片，开始批处理", unembedded.len());

                    // Read content for each unembedded fragment and compute embeddings
                    for frag_id in &unembedded {
                        let content = {
                            let idx = match emb_thread_index.lock() {
                                Ok(idx) => idx,
                                Err(_) => break,
                            };
                            idx.get_fragment_content(frag_id).unwrap_or_else(|_| String::new())
                        };

                        if content.is_empty() {
                            continue;
                        }

                        let mut engine = match emb_thread_embedding.lock() {
                            Ok(e) => e,
                            Err(_) => break,
                        };

                        match engine.embed_text(&content) {
                            Ok(vector) => {
                                if let Err(e) = engine.store_vector(frag_id, &vector) {
                                    log::error!(
                                        "存储向量失败 (fragment {}): {}",
                                        frag_id,
                                        e
                                    );
                                }
                            }
                            Err(e) => {
                                log::error!(
                                    "计算向量失败 (fragment {}): {}",
                                    frag_id,
                                    e
                                );
                            }
                        }
                    }

                    // After embedding, enqueue curator classify jobs for newly embedded fragments
                    for frag_id in &unembedded {
                        let payload = serde_json::json!({
                            "fragment_id": frag_id
                        });
                        if let Err(e) = emb_thread_jobs.enqueue(
                            crate::core::jobs::JobType::CuratorClassify,
                            payload,
                        ) {
                            log::error!("入队 curator_classify 失败: {}", e);
                        }
                    }
                }
            });

            // Thread 2 & 3: JobQueue workers (recover on startup + start workers)
            let recovered = job_queue.recover_on_startup().unwrap_or_else(|e| {
                log::error!("JobQueue 恢复失败: {}", e);
                0
            });
            if recovered > 0 {
                log::info!("JobQueue 恢复了 {} 个中断的 job", recovered);
            }

            let repo_for_workers = Arc::new(Mutex::new(FileRepo::new(vault_path.clone())));
            let worker_context = Arc::new(WorkerContext {
                embedding: embedding.clone(),
                llm: llm.clone(),
                repo: repo_for_workers,
                index: index_arc.clone(),
            });
            job_queue.start_workers(worker_context.clone());
            log::info!("JobQueue worker 线程已启动");

            // Thread 4: ReflectionScheduler
            let scheduler = ReflectionScheduler::new(job_queue.clone());
            scheduler.start();
            log::info!("ReflectionScheduler 线程已启动");

            // Check for missed reviews on startup
            let missed_review_context = worker_context.clone();
            let missed_review_jobs = job_queue.clone();
            std::thread::spawn(move || {
                let agent = ReflectionAgent;
                match agent.check_missed_reviews(&*missed_review_context) {
                    Ok(missed) => {
                        for job_type in missed {
                            let payload = serde_json::json!({
                                "job_type": format!("{:?}", job_type)
                            });
                            if let Err(e) = missed_review_jobs.enqueue(job_type, payload) {
                                log::error!("入队遗漏回顾失败: {}", e);
                            }
                        }
                    }
                    Err(e) => {
                        log::error!("检查遗漏回顾失败: {}", e);
                    }
                }
            });

            log::info!("AI 子系统初始化完成");

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
            commands::delete_fragment,
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
            // AI commands
            commands::ai::get_embedding_status,
            commands::ai::find_similar_fragments,
            commands::ai::writing_chat,
            commands::ai::writing_stream_chat,
            commands::ai::writing_recommend,
            commands::ai::generate_view,
            commands::ai::pin_view,
            commands::ai::list_pinned_views,
            commands::ai::get_ai_settings,
            commands::ai::save_ai_settings,
            commands::ai::validate_provider,
            commands::ai::list_ollama_models,
            commands::ai::list_jobs,
            commands::ai::cancel_job,
            commands::ai::get_audit_log,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
