// Cognest IPC Command Layer — CLI Agent Commands
//
// Async commands for detecting, spawning, and killing CLI Agent subprocesses.
// Generates AGENTS.md context before spawn and optionally injects article content.

use std::path::PathBuf;

use tauri::State;

use crate::commands::AppState;
use crate::core::cli_agents::context::{generate_agents_md, write_agents_md};
use crate::core::cli_agents::process_manager::{AgentProcessManager, CliAgentInfo};

// ─── CLI Agent State ────────────────────────────────────────────────────────

/// Tauri managed state for CLI Agent process management.
pub struct CliAgentState {
    pub manager: AgentProcessManager,
}

// ─── CLI Agent Commands ─────────────────────────────────────────────────────

/// Detect installed CLI agents by scanning PATH.
///
/// Returns a list of known CLI agents with availability and version info.
/// Entire detection completes within 10 seconds.
#[tauri::command(async)]
pub async fn detect_cli_agents() -> Result<Vec<CliAgentInfo>, String> {
    let agents = AgentProcessManager::detect_agents().await;
    Ok(agents)
}

/// Spawn a CLI Agent subprocess.
///
/// Workflow:
/// 1. Generate/update AGENTS.md in the vault (graceful degradation on failure)
/// 2. Optionally prepend article content (Markdown + frontmatter) to the prompt
/// 3. Spawn the CLI process with the assembled prompt
///
/// The process output (stdout/stderr) is forwarded as `agent_output` Tauri events.
#[tauri::command(async)]
pub async fn spawn_cli_agent(
    state: State<'_, CliAgentState>,
    app_state: State<'_, AppState>,
    app: tauri::AppHandle,
    command: String,
    prompt: String,
    article_content: Option<String>,
) -> Result<(), String> {
    // 1. Determine vault path
    let vault_path: PathBuf = {
        let repo = app_state.repo.lock().map_err(|e| e.to_string())?;
        repo.vault_path().to_path_buf()
    };

    // 2. Gather topics from index for AGENTS.md context
    let topics = gather_topics_from_index(&app_state);

    // 3. Generate and write AGENTS.md (graceful degradation per Req 12.5)
    let agents_md_content = generate_agents_md(&vault_path, &topics);
    if let Err(e) = write_agents_md(&vault_path, &agents_md_content) {
        log::warn!("AGENTS.md 写入失败，降级继续: {}", e);
    }

    // 4. Assemble final prompt with optional article content injection (Req 12.2)
    let final_prompt = match article_content {
        Some(content) if !content.is_empty() => {
            format!(
                "Below is the current article content for context:\n\n{}\n\n---\n\n{}",
                content, prompt
            )
        }
        _ => prompt,
    };

    // 5. Spawn the CLI agent process
    state
        .manager
        .spawn(&command, &final_prompt, &vault_path, &app)
        .await
        .map_err(|e| e.to_string())
}

/// Kill the currently running CLI Agent process.
///
/// Uses SIGTERM → 5s timeout → SIGKILL strategy.
#[tauri::command(async)]
pub async fn kill_cli_agent(
    state: State<'_, CliAgentState>,
) -> Result<(), String> {
    state
        .manager
        .kill()
        .await
        .map_err(|e| e.to_string())
}

// ─── Helper Functions ───────────────────────────────────────────────────────

/// Gather all unique topics from indexed fragments.
///
/// Best-effort: returns empty vec on failure (topics are non-critical context).
fn gather_topics_from_index(app_state: &State<'_, AppState>) -> Vec<String> {
    let index = match app_state.index.lock() {
        Ok(idx) => idx,
        Err(_) => return Vec::new(),
    };

    index.all_topics().unwrap_or_default()
}
