//! Stream 适配层 — 将 Rig stream 转换为 Tauri event payload
//!
//! 核心函数 `stream_to_tauri_events()` 负责：
//! - 消费 WritingRigAgent::stream_chat() 返回的 StreamChunkItem 流
//! - 转换为与旧版 StreamChunk 格式兼容的 JSON payload
//! - 通过 tauri::AppHandle emit `writing_chunk` 事件
//! - 支持 CancellationToken 取消（2s 内终止）
//! - 30s 首个 chunk 超时保护

use std::pin::Pin;
use std::time::Duration;

use futures::{Stream, StreamExt};
use tauri::Emitter;
use tokio_util::sync::CancellationToken;

use crate::core::rig_agents::types::{LlmError, StreamChunk, TokenUsage};
use crate::core::rig_agents::AgentError;

use super::writing::StreamChunkItem;

/// 首个 chunk 最大等待时间
const FIRST_CHUNK_TIMEOUT: Duration = Duration::from_secs(30);

/// 流式适配结果
#[derive(Debug, Clone)]
pub struct StreamResult {
    /// 累计的完整文本内容
    pub content: String,
    /// 流是否被取消
    pub cancelled: bool,
}

/// 将 WritingRigAgent 的流式输出转换为 Tauri `writing_chunk` 事件
///
/// 参数：
/// - `stream`: WritingRigAgent::stream_chat() 返回的 StreamChunkItem 流
/// - `app`: Tauri AppHandle，用于 emit 事件
/// - `cancel_token`: 取消令牌，取消后 2s 内终止流
///
/// 行为：
/// - 每个 Text chunk 发出 `{"type":"delta","content":"..."}`
/// - 流正常结束发出 `{"type":"done","usage":{...}}`
/// - 错误发出 `{"type":"error","error":{...},"partial_tokens":0}`
/// - 30s 内无首个 chunk 发出超时 Error chunk
/// - 取消时发出 Done chunk 后终止
pub async fn stream_to_tauri_events(
    stream: Pin<Box<dyn Stream<Item = Result<StreamChunkItem, AgentError>> + Send>>,
    app: &tauri::AppHandle,
    cancel_token: CancellationToken,
) -> StreamResult {
    let mut stream = stream;
    let mut total_content = String::new();
    // 等待首个 chunk，带超时保护
    let first_result = tokio::select! {
        chunk = stream.next() => chunk,
        _ = tokio::time::sleep(FIRST_CHUNK_TIMEOUT) => {
            // 30s 超时，发送 Error chunk
            let payload = StreamChunk::Error {
                error: LlmError::Timeout {
                    provider: "rig".into(),
                },
                partial_tokens: 0,
            };
            emit_chunk(app, &payload);
            return StreamResult {
                content: total_content,
                cancelled: false,
            };
        }
        _ = cancel_token.cancelled() => {
            // 等待首个 chunk 期间被取消
            let payload = StreamChunk::Done {
                usage: TokenUsage::zero(),
            };
            emit_chunk(app, &payload);
            return StreamResult {
                content: total_content,
                cancelled: true,
            };
        }
    };

    // 处理首个 chunk
    match first_result {
        Some(Ok(item)) => {
            if process_chunk_item(item, &mut total_content, app) {
                return StreamResult {
                    content: total_content,
                    cancelled: false,
                };
            }
        }
        Some(Err(e)) => {
            emit_error(app, &e);
            return StreamResult {
                content: total_content,
                cancelled: false,
            };
        }
        None => {
            // Stream 立即结束，发送 Done
            let payload = StreamChunk::Done {
                usage: TokenUsage::zero(),
            };
            emit_chunk(app, &payload);
            return StreamResult {
                content: total_content,
                cancelled: false,
            };
        }
    }

    // 主循环：处理后续 chunks
    loop {
        tokio::select! {
            chunk = stream.next() => {
                match chunk {
                    Some(Ok(item)) => {
                        if process_chunk_item(item, &mut total_content, app) {
                            break;
                        }
                    }
                    Some(Err(e)) => {
                        emit_error(app, &e);
                        break;
                    }
                    None => {
                        // Stream 正常结束但未收到 Done item
                        let payload = StreamChunk::Done {
                            usage: TokenUsage::zero(),
                        };
                        emit_chunk(app, &payload);
                        break;
                    }
                }
            }
            _ = cancel_token.cancelled() => {
                // 取消 — 发送 Done chunk 后终止
                let payload = StreamChunk::Done {
                    usage: TokenUsage::zero(),
                };
                emit_chunk(app, &payload);
                return StreamResult {
                    content: total_content,
                    cancelled: true,
                };
            }
        }
    }

    StreamResult {
        content: total_content,
        cancelled: false,
    }
}

/// 处理单个 StreamChunkItem，返回 true 表示流结束
fn process_chunk_item(
    item: StreamChunkItem,
    total_content: &mut String,
    app: &tauri::AppHandle,
) -> bool {
    match item {
        StreamChunkItem::Text(text) => {
            total_content.push_str(&text);
            let payload = StreamChunk::Delta { content: text };
            emit_chunk(app, &payload);
            false
        }
        StreamChunkItem::Done => {
            let payload = StreamChunk::Done {
                usage: TokenUsage::zero(),
            };
            emit_chunk(app, &payload);
            true
        }
    }
}

/// 发送错误 chunk
fn emit_error(app: &tauri::AppHandle, error: &AgentError) {
    let payload = StreamChunk::Error {
        error: LlmError::Unknown {
            provider: "rig".into(),
            reason: error.to_string(),
        },
        partial_tokens: 0,
    };
    emit_chunk(app, &payload);
}

/// 发射单个 StreamChunk 事件到前端
fn emit_chunk(app: &tauri::AppHandle, chunk: &StreamChunk) {
    // 序列化为 JSON 字符串后 emit
    if let Ok(json) = serde_json::to_string(chunk) {
        let _ = app.emit("writing_chunk", &json);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_token_usage_zero() {
        let usage = TokenUsage::zero();
        assert_eq!(usage.prompt_tokens, 0);
        assert_eq!(usage.completion_tokens, 0);
        assert_eq!(usage.total_tokens, 0);
    }

    #[test]
    fn test_stream_chunk_delta_serialization() {
        let chunk = StreamChunk::Delta {
            content: "hello".to_string(),
        };
        let json = serde_json::to_string(&chunk).unwrap();
        assert_eq!(json, r#"{"type":"delta","content":"hello"}"#);
    }

    #[test]
    fn test_stream_chunk_done_serialization() {
        let chunk = StreamChunk::Done {
            usage: TokenUsage {
                prompt_tokens: 10,
                completion_tokens: 20,
                total_tokens: 30,
            },
        };
        let json = serde_json::to_string(&chunk).unwrap();
        assert!(json.contains(r#""type":"done""#));
        assert!(json.contains(r#""prompt_tokens":10"#));
        assert!(json.contains(r#""completion_tokens":20"#));
        assert!(json.contains(r#""total_tokens":30"#));
    }

    #[test]
    fn test_stream_chunk_error_serialization() {
        let chunk = StreamChunk::Error {
            error: LlmError::Timeout {
                provider: "rig".into(),
            },
            partial_tokens: 0,
        };
        let json = serde_json::to_string(&chunk).unwrap();
        assert!(json.contains(r#""type":"error""#));
        assert!(json.contains(r#""partial_tokens":0"#));
    }
}
