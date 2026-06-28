// Cognest Core — Pure Rust modules (no Tauri dependency)
// This crate handles data layer, file operations, and Git operations.

pub mod frontmatter;
pub mod repo;
pub mod index;
pub mod watcher;
pub mod git;

// Phase 2 — AI capability modules
pub mod embedding;
pub mod settings;
pub mod jobs;

// Phase 3 — Rig Agent 层 (async-first, replaces old llm + agents modules)
pub mod rig_agents;

// Phase 3 — CLI Agent 进程管理
pub mod cli_agents;

// Reflection agent — 从 agents/ 迁出的独立模块
pub mod reflection;

#[cfg(test)]
mod properties;
