//! AgentProcessManager — CLI Agent 子进程的 spawn、输出转发和生命周期管理
//!
//! 设计要点：
//! - 单进程约束：同时只能运行一个 CLI Agent 子进程
//! - stdout/stderr 逐行读取并以 Tauri event 转发
//! - SIGTERM → 5s → SIGKILL 优雅终止策略
//! - detect_agents() 扫描 PATH 检测已安装 CLI Agent

use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::Mutex;
use tokio::time::{timeout, Duration};

use crate::core::rig_agents::AgentError;

/// CLI Agent 信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CliAgentInfo {
    /// Agent 显示名称 (e.g. "Claude Code")
    pub name: String,
    /// CLI 命令名 (e.g. "claude")
    pub command: String,
    /// 可执行文件绝对路径
    pub path: String,
    /// 版本字符串 (--version 输出第一行) 或 "版本未知"
    pub version: String,
    /// 是否可用
    pub available: bool,
}

/// 进程状态
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "state")]
pub enum ProcessState {
    Idle,
    Running { pid: u32, started_at: String },
    Finished { exit_code: i32, duration_secs: u64 },
}

/// CLI Agent 输出事件 — 通过 Tauri event 发送至前端
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type")]
pub enum AgentOutputEvent {
    /// stdout/stderr 行输出
    Line { content: String, stream: String },
    /// 进程退出
    Exit { code: i32, duration_secs: u64 },
    /// 错误（spawn 失败等）
    Error { reason: String },
}

/// 运行中的进程内部表示
struct RunningProcess {
    child: Child,
    pid: u32,
    started_at: Instant,
}

/// Agent 进程管理器（单进程限制）
pub struct AgentProcessManager {
    current: Arc<Mutex<Option<RunningProcess>>>,
}

impl AgentProcessManager {
    pub fn new() -> Self {
        Self {
            current: Arc::new(Mutex::new(None)),
        }
    }

    /// 检测已安装的 CLI Agent
    ///
    /// 扫描 PATH 检测 claude/opencode/kiro 命令，执行 `--version` 获取版本。
    /// 单命令超时 5s，整体检测超时 10s。
    pub async fn detect_agents() -> Vec<CliAgentInfo> {
        let agents_to_detect = vec![
            ("Claude Code", "claude"),
            ("OpenCode", "opencode"),
            ("Kiro CLI", "kiro"),
        ];

        let total_timeout = Duration::from_secs(10);
        let results = timeout(total_timeout, async {
            let mut results = Vec::new();
            for (name, command) in &agents_to_detect {
                let info = detect_single_agent(name, command).await;
                results.push(info);
            }
            results
        })
        .await;

        match results {
            Ok(agents) => agents,
            Err(_) => {
                // Total timeout expired — return what we have as unavailable
                agents_to_detect
                    .iter()
                    .map(|(name, command)| CliAgentInfo {
                        name: name.to_string(),
                        command: command.to_string(),
                        path: String::new(),
                        version: "检测超时".to_string(),
                        available: false,
                    })
                    .collect()
            }
        }
    }

    /// Spawn CLI Agent 子进程
    ///
    /// - 单进程约束：已有运行中进程则返回 ProcessAlreadyRunning
    /// - CognestVault 作为工作目录
    /// - stdout/stderr 逐行读取并以 Tauri event 转发
    pub async fn spawn(
        &self,
        cli_command: &str,
        prompt: &str,
        cwd: &Path,
        app: &tauri::AppHandle,
    ) -> Result<(), AgentError> {
        // 单进程约束检查
        {
            let guard = self.current.lock().await;
            if guard.is_some() {
                return Err(AgentError::ProcessAlreadyRunning);
            }
        }

        // 构建命令
        let mut cmd = Command::new(cli_command);
        cmd.arg(prompt);
        cmd.current_dir(cwd);
        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::piped());

        // Spawn 子进程
        let mut child = cmd.spawn().map_err(|e| {
            AgentError::ProcessSpawn(format!("无法启动 {}: {}", cli_command, e))
        })?;

        let pid = child.id().unwrap_or(0);

        // 获取 stdout/stderr
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();

        // 存储运行中的进程
        {
            let mut guard = self.current.lock().await;
            *guard = Some(RunningProcess {
                child,
                pid,
                started_at: Instant::now(),
            });
        }

        // 在后台任务中逐行转发输出
        let app_handle = app.clone();
        let current = self.current.clone();

        tokio::spawn(async move {
            let app_for_stdout = app_handle.clone();
            let app_for_stderr = app_handle.clone();

            // stdout reader
            let stdout_task = tokio::spawn(async move {
                if let Some(stdout) = stdout {
                    let reader = BufReader::new(stdout);
                    let mut lines = reader.lines();
                    while let Ok(Some(line)) = lines.next_line().await {
                        let event = AgentOutputEvent::Line {
                            content: line,
                            stream: "stdout".to_string(),
                        };
                        let _ = tauri::Emitter::emit(&app_for_stdout, "agent_output", &event);
                    }
                }
            });

            // stderr reader
            let stderr_task = tokio::spawn(async move {
                if let Some(stderr) = stderr {
                    let reader = BufReader::new(stderr);
                    let mut lines = reader.lines();
                    while let Ok(Some(line)) = lines.next_line().await {
                        let event = AgentOutputEvent::Line {
                            content: line,
                            stream: "stderr".to_string(),
                        };
                        let _ = tauri::Emitter::emit(&app_for_stderr, "agent_output", &event);
                    }
                }
            });

            // 等待两个 reader 完成
            let _ = tokio::join!(stdout_task, stderr_task);

            // 等待进程退出并获取状态
            let mut guard = current.lock().await;
            if let Some(mut process) = guard.take() {
                let duration_secs = process.started_at.elapsed().as_secs();
                let exit_code = match process.child.wait().await {
                    Ok(status) => status.code().unwrap_or(-1),
                    Err(_) => -1,
                };

                let exit_event = AgentOutputEvent::Exit {
                    code: exit_code,
                    duration_secs,
                };
                let _ = tauri::Emitter::emit(&app_handle, "agent_output", &exit_event);
            }
        });

        Ok(())
    }

    /// 终止当前进程
    ///
    /// 策略：SIGTERM → 5s 等待 → SIGKILL
    pub async fn kill(&self) -> Result<(), AgentError> {
        let mut guard = self.current.lock().await;
        if let Some(ref mut process) = *guard {
            let pid = process.pid;

            // 发送 SIGTERM
            #[cfg(unix)]
            {
                unsafe {
                    libc::kill(pid as i32, libc::SIGTERM);
                }
            }
            #[cfg(not(unix))]
            {
                // Windows: 直接 kill
                let _ = process.child.kill().await;
                *guard = None;
                return Ok(());
            }

            // 等待最多 5 秒
            let wait_result = timeout(
                Duration::from_secs(5),
                process.child.wait(),
            )
            .await;

            match wait_result {
                Ok(_) => {
                    // 进程已正常退出
                    *guard = None;
                }
                Err(_) => {
                    // 5s 超时，发送 SIGKILL
                    let _ = process.child.kill().await;
                    let _ = process.child.wait().await;
                    *guard = None;
                }
            }

            Ok(())
        } else {
            // 没有运行中的进程，静默成功
            Ok(())
        }
    }

    /// 返回当前进程状态
    pub async fn status(&self) -> ProcessState {
        let guard = self.current.lock().await;
        match &*guard {
            Some(process) => ProcessState::Running {
                pid: process.pid,
                started_at: format!("{:?}", process.started_at),
            },
            None => ProcessState::Idle,
        }
    }

    /// Check if a spawn would be rejected due to single-process constraint.
    ///
    /// Returns Ok(()) if spawning is allowed, Err(ProcessAlreadyRunning) if not.
    /// This method is exposed for property-based testing of the constraint logic.
    pub async fn check_spawn_guard(&self) -> Result<(), AgentError> {
        let guard = self.current.lock().await;
        if guard.is_some() {
            Err(AgentError::ProcessAlreadyRunning)
        } else {
            Ok(())
        }
    }

    /// Simulate a running process for testing purposes.
    ///
    /// Sets the internal state as if a process is running with the given PID.
    pub async fn set_running_for_test(&self, pid: u32) {
        let mut guard = self.current.lock().await;
        *guard = Some(RunningProcess {
            child: Command::new("sleep")
                .arg("9999")
                .spawn()
                .expect("failed to spawn test process"),
            pid,
            started_at: Instant::now(),
        });
    }

    /// Clear the running process state for testing purposes.
    pub async fn clear_for_test(&self) {
        let mut guard = self.current.lock().await;
        if let Some(mut process) = guard.take() {
            let _ = process.child.kill().await;
            let _ = process.child.wait().await;
        }
    }
}

/// 检测单个 CLI Agent — 查找路径 + 执行 --version
async fn detect_single_agent(name: &str, command: &str) -> CliAgentInfo {
    let single_timeout = Duration::from_secs(5);

    // 使用 `which` 命令检测可执行文件路径
    let path = find_executable_path(command).await;

    match path {
        Some(exec_path) => {
            // 执行 --version 获取版本信息
            let version = timeout(single_timeout, get_version(command)).await;
            let version_str = match version {
                Ok(Some(v)) => v,
                _ => "版本未知".to_string(),
            };

            CliAgentInfo {
                name: name.to_string(),
                command: command.to_string(),
                path: exec_path,
                version: version_str,
                available: true,
            }
        }
        None => CliAgentInfo {
            name: name.to_string(),
            command: command.to_string(),
            path: String::new(),
            version: String::new(),
            available: false,
        },
    }
}

/// 在 PATH 中查找可执行文件路径
async fn find_executable_path(command: &str) -> Option<String> {
    let output = Command::new("which")
        .arg(command)
        .output()
        .await
        .ok()?;

    if output.status.success() {
        let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if path.is_empty() {
            None
        } else {
            Some(path)
        }
    } else {
        None
    }
}

/// 执行 command --version 获取第一行版本输出
async fn get_version(command: &str) -> Option<String> {
    let output = Command::new(command)
        .arg("--version")
        .output()
        .await
        .ok()?;

    if output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        stdout.lines().next().map(|s| s.trim().to_string())
    } else {
        // 某些工具即使 --version 也可能返回非零退出码但有输出
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        let combined = if !stdout.is_empty() { stdout } else { stderr };
        combined.lines().next().map(|s| s.trim().to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_process_manager_starts_idle() {
        let pm = AgentProcessManager::new();
        let state = pm.status().await;
        assert!(matches!(state, ProcessState::Idle));
    }

    #[tokio::test]
    async fn test_detect_agents_returns_vec() {
        // detect_agents should always return 3 items (one per known agent)
        let agents = AgentProcessManager::detect_agents().await;
        assert_eq!(agents.len(), 3);
        assert_eq!(agents[0].command, "claude");
        assert_eq!(agents[1].command, "opencode");
        assert_eq!(agents[2].command, "kiro");
    }
}
