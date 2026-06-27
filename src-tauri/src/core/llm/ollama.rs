// Cognest Core — Ollama Provider
// Local LLM via Ollama REST API (localhost:11434)

use std::pin::Pin;
use std::time::Duration;

use futures::stream::{self, Stream, StreamExt};
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};

use super::{
    ChatMessage, ChatOptions, FinishReason, LlmError, LlmProvider, LlmResponse, Role,
    StreamChunk, TokenUsage,
};

/// Ollama local LLM provider (no API key required)
pub struct OllamaProvider {
    client: reqwest::Client,
    endpoint: String,
    model: String,
    rt: tokio::runtime::Runtime,
}

// ─── Ollama API Request/Response Types ──────────────────────────────────────

#[derive(Debug, Serialize)]
struct OllamaChatRequest {
    model: String,
    messages: Vec<OllamaChatMessage>,
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    options: Option<OllamaOptions>,
}

#[derive(Debug, Serialize)]
struct OllamaChatMessage {
    role: String,
    content: String,
}

#[derive(Debug, Serialize)]
struct OllamaOptions {
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
}

/// Response from GET /api/tags
#[derive(Debug, Deserialize)]
struct OllamaTagsResponse {
    models: Vec<OllamaModelInfo>,
}

#[derive(Debug, Deserialize)]
struct OllamaModelInfo {
    name: String,
}

/// Non-streaming response from POST /api/chat
#[derive(Debug, Deserialize)]
struct OllamaChatResponse {
    message: OllamaResponseMessage,
    #[serde(default)]
    done: bool,
    #[serde(default)]
    eval_count: Option<u32>,
    #[serde(default)]
    prompt_eval_count: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct OllamaResponseMessage {
    content: String,
}

/// Streaming chunk from POST /api/chat (ndjson line)
#[derive(Debug, Deserialize)]
struct OllamaStreamChunk {
    #[serde(default)]
    message: Option<OllamaResponseMessage>,
    done: bool,
    #[serde(default)]
    eval_count: Option<u32>,
    #[serde(default)]
    prompt_eval_count: Option<u32>,
}

// ─── Implementation ─────────────────────────────────────────────────────────

impl OllamaProvider {
    /// Create a new Ollama provider instance.
    pub fn new(endpoint: String, model: String) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(300))
            .connect_timeout(Duration::from_secs(10))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("Failed to create tokio runtime for Ollama provider");

        Self {
            client,
            endpoint: endpoint.trim_end_matches('/').to_string(),
            model,
            rt,
        }
    }

    /// Query available models from Ollama (GET /api/tags).
    pub fn list_models(&self) -> Result<Vec<String>, LlmError> {
        self.rt.block_on(self.list_models_async())
    }

    async fn list_models_async(&self) -> Result<Vec<String>, LlmError> {
        let url = format!("{}/api/tags", self.endpoint);

        let response = self
            .client
            .get(&url)
            .timeout(Duration::from_secs(10))
            .send()
            .await
            .map_err(|e| self.classify_error(e))?;

        if !response.status().is_success() {
            return Err(LlmError::Unknown {
                provider: "ollama".to_string(),
                reason: format!("HTTP {}", response.status()),
            });
        }

        let tags: OllamaTagsResponse = response.json().await.map_err(|e| LlmError::Unknown {
            provider: "ollama".to_string(),
            reason: format!("Failed to parse response: {}", e),
        })?;

        Ok(tags.models.into_iter().map(|m| m.name).collect())
    }

    /// Convert messages to Ollama format
    fn convert_messages(messages: &[ChatMessage]) -> Vec<OllamaChatMessage> {
        messages
            .iter()
            .map(|m| OllamaChatMessage {
                role: match m.role {
                    Role::System => "system".to_string(),
                    Role::User => "user".to_string(),
                    Role::Assistant => "assistant".to_string(),
                },
                content: m.content.clone(),
            })
            .collect()
    }

    /// Build chat request body
    fn build_request(
        &self,
        messages: &[ChatMessage],
        options: &ChatOptions,
        stream: bool,
    ) -> OllamaChatRequest {
        let model = options
            .model
            .clone()
            .unwrap_or_else(|| self.model.clone());

        let ollama_options = if options.temperature.is_some() {
            Some(OllamaOptions {
                temperature: options.temperature,
            })
        } else {
            None
        };

        OllamaChatRequest {
            model,
            messages: Self::convert_messages(messages),
            stream,
            options: ollama_options,
        }
    }

    /// Get the timeout duration from options
    fn get_timeout(&self, options: &ChatOptions) -> Duration {
        Duration::from_secs(options.timeout_secs.unwrap_or(300))
    }

    /// Classify reqwest errors into LlmError variants
    fn classify_error(&self, error: reqwest::Error) -> LlmError {
        if error.is_timeout() {
            LlmError::Timeout {
                provider: "ollama".to_string(),
            }
        } else if error.is_connect() {
            LlmError::NetworkError {
                provider: "ollama".to_string(),
                reason: "Connection refused — is Ollama running?".to_string(),
            }
        } else {
            LlmError::NetworkError {
                provider: "ollama".to_string(),
                reason: error.to_string(),
            }
        }
    }

    /// Async implementation of chat
    async fn chat_async(
        &self,
        messages: &[ChatMessage],
        options: &ChatOptions,
    ) -> Result<LlmResponse, LlmError> {
        let url = format!("{}/api/chat", self.endpoint);
        let body = self.build_request(messages, options, false);
        let timeout = self.get_timeout(options);

        let response = self
            .client
            .post(&url)
            .json(&body)
            .timeout(timeout)
            .send()
            .await
            .map_err(|e| self.classify_error(e))?;

        let status = response.status();
        if !status.is_success() {
            let error_text = response.text().await.unwrap_or_default();
            return Err(self.classify_http_error(status, &error_text));
        }

        let chat_response: OllamaChatResponse =
            response.json().await.map_err(|e| LlmError::Unknown {
                provider: "ollama".to_string(),
                reason: format!("Failed to parse response: {}", e),
            })?;

        let completion_tokens = chat_response.eval_count.unwrap_or(0);
        let prompt_tokens = chat_response.prompt_eval_count.unwrap_or(0);

        Ok(LlmResponse {
            content: chat_response.message.content,
            finish_reason: FinishReason::Stop,
            usage: TokenUsage {
                prompt_tokens,
                completion_tokens,
                total_tokens: prompt_tokens + completion_tokens,
            },
        })
    }

    /// Classify HTTP status errors
    fn classify_http_error(&self, status: StatusCode, body: &str) -> LlmError {
        match status.as_u16() {
            408 | 504 => LlmError::Timeout {
                provider: "ollama".to_string(),
            },
            _ => LlmError::Unknown {
                provider: "ollama".to_string(),
                reason: format!("HTTP {}: {}", status, body),
            },
        }
    }

    /// Async implementation of stream_chat
    async fn stream_chat_async(
        &self,
        messages: &[ChatMessage],
        options: &ChatOptions,
    ) -> Result<Pin<Box<dyn Stream<Item = StreamChunk> + Send>>, LlmError> {
        let url = format!("{}/api/chat", self.endpoint);
        let body = self.build_request(messages, options, true);
        let timeout = self.get_timeout(options);

        let response = self
            .client
            .post(&url)
            .json(&body)
            .timeout(timeout)
            .send()
            .await
            .map_err(|e| self.classify_error(e))?;

        let status = response.status();
        if !status.is_success() {
            let error_text = response.text().await.unwrap_or_default();
            return Err(self.classify_http_error(status, &error_text));
        }

        // Read the byte stream and parse ndjson lines
        let byte_stream = response.bytes_stream();

        let chunk_stream = byte_stream
            .map(|result| match result {
                Ok(bytes) => {
                    let text = String::from_utf8_lossy(&bytes).to_string();
                    text
                }
                Err(_) => String::new(),
            })
            // Buffer lines: Ollama sends one JSON object per line
            .flat_map(|text| {
                let lines: Vec<String> = text
                    .lines()
                    .filter(|line| !line.trim().is_empty())
                    .map(|s| s.to_string())
                    .collect();
                stream::iter(lines)
            })
            // Parse each line as JSON
            .filter_map(|line| async move {
                let chunk: OllamaStreamChunk = serde_json::from_str(&line).ok()?;

                if chunk.done {
                    // Final chunk with usage stats
                    let completion_tokens = chunk.eval_count.unwrap_or(0);
                    let prompt_tokens = chunk.prompt_eval_count.unwrap_or(0);
                    Some(StreamChunk::Done {
                        usage: TokenUsage {
                            prompt_tokens,
                            completion_tokens,
                            total_tokens: prompt_tokens + completion_tokens,
                        },
                    })
                } else if let Some(message) = chunk.message {
                    if !message.content.is_empty() {
                        Some(StreamChunk::Delta {
                            content: message.content,
                        })
                    } else {
                        None
                    }
                } else {
                    None
                }
            });

        Ok(Box::pin(chunk_stream))
    }
}

impl LlmProvider for OllamaProvider {
    fn name(&self) -> &str {
        "ollama"
    }

    fn chat(
        &self,
        messages: &[ChatMessage],
        options: &ChatOptions,
    ) -> Result<LlmResponse, LlmError> {
        self.rt.block_on(self.chat_async(messages, options))
    }

    fn stream_chat(
        &self,
        messages: &[ChatMessage],
        options: &ChatOptions,
    ) -> Result<Pin<Box<dyn Stream<Item = StreamChunk> + Send>>, LlmError> {
        self.rt.block_on(self.stream_chat_async(messages, options))
    }

    fn validate(&self) -> Result<(), LlmError> {
        // Validate by calling list_models — success means connection works
        self.list_models().map(|_| ())
    }
}
