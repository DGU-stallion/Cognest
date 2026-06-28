// Cognest Core — CLI Agent integration
//
// Manages detection, spawning, and context injection for local CLI agents
// (Claude Code, OpenCode, Kiro CLI).

pub mod context;
pub mod process_manager;

pub use process_manager::{AgentProcessManager, AgentOutputEvent, CliAgentInfo, ProcessState};
