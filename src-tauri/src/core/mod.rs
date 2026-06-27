// Cognest Core — Pure Rust modules (no Tauri dependency)
// This crate handles data layer, file operations, and Git operations.

pub mod frontmatter;
pub mod repo;
pub mod index;
pub mod watcher;
pub mod git;

// Phase 2 — AI capability modules
pub mod embedding;
pub mod llm;
pub mod settings;
pub mod jobs;
pub mod agents;

#[cfg(test)]
mod properties;
