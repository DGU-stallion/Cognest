// Cognest Core — OpenAI-compatible Provider
// Generic provider for any OpenAI API-compatible service (Moonshot, Zhipu, etc.)

use std::pin::Pin;
use std::time::Duration;

use futures::Stream;
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};

use super::{
    ChatMessage, ChatOptions, FinishReason, LlmError, LlmProvider, LlmResponse, Role,
    StreamChunk, TokenUsage,
};

// ─── OpenAI API Request/Response Types ──────────────────────────────────────

#[derive(Debug, Serialize)]
struct ApiRequest {
    model: String,
    messages: Vec<ApiMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    response_format: Option<ResponseFormat>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stream: Option<bool>,
}

#[derive(Debug, Serialize)]
struct ApiMessage {
    role: String,
    content: String,
}

#[derive(Debug, Serialize)]
struct ResponseFormat {
    #[serde(rename = "type")]
    format_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    json_schema: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct ApiResponse {
    choices: Vec<ApiChoice>,
    usage: Option<ApiUsage>,
}

#[derive(Debug, Deserialize)]
struct ApiChoice {
    message: Option<ApiMessageContent>,
    delta: Option<ApiDelta>,
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ApiMessageContent {
    content: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ApiDelta {
    content: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ApiUsage {
    prompt_tokens: u32,
    completion_tokens: u32,
    total_tokens: u32,
}

#[derive(Debug, Deserialize)]
struct ApiErrorResponse {
    error: Option<ApiErrorDetail>,
}

#[derive(Debug, Deserialize)]
struct ApiErrorDetail {
    message: Option<String>,
}

// ─── Provider Implementation ────────────────────────────────────────────────

/// OpenAI-compatible generic provider.
/// Works with any service implementing the OpenAI chat/completions API format
/// (e.g., Moonshot, Zhipu, any OpenAI-proxy endpoint).
pub struct OpenAiCompatProvider {
    client: reqwest::Client,
    endpoint: String,
    api_key: String,
    model: String,
    rt: tokio::runtime::Runtime,
}

impl OpenAiCompatProvider {
    /// Create a new OpenAI-compatible provider instance.
    pub fn new(endpoint: String, api_key: String, model: String) -> Self {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("Failed to create tokio runtime for OpenAI-compat provider");
        Self {
            client: reqwest::Client::new(),
            endpoint: endpoint.trim_end_matches('/').to_string(),
            api_key,
            model,
            rt,
        }
    }

    /// Build the chat completions URL.
    fn chat_url(&self) -> String {
        format!("{}/v1/chat/completions", self.endpoint)
    }

    /// Build the models URL (for validation).
    fn models_url(&self) -> String {
        format!("{}/v1/models", self.endpoint)
    }

    /// Convert internal ChatMessage to API format.
    fn to_api_messages(messages: &[ChatMessage]) -> Vec<ApiMessage> {
        messages
            .iter()
            .map(|m| ApiMessage {
                role: match m.role {
                    Role::System => "system".to_string(),
                    Role::User => "user".to_string(),
                    Role::Assistant => "assistant".to_string(),
                },
                content: m.content.clone(),
            })
            .collect()
    }

    /// Build the request body from messages and options.
    fn build_request_body(
        &self,
        messages: &[ChatMessage],
        options: &ChatOptions,
        stream: bool,
    ) -> ApiRequest {
        let model = options
            .model
            .clone()
            .unwrap_or_else(|| self.model.clone());

        let response_format = options.json_schema.as_ref().map(|schema| ResponseFormat {
            format_type: "json_schema".to_string(),
            json_schema: Some(schema.clone()),
        });

        ApiRequest {
            model,
            messages: Self::to_api_messages(messages),
            temperature: options.temperature,
            max_tokens: options.max_tokens,
            response_format,
            stream: if stream { Some(true) } else { None },
        }
    }

    /// Get the timeout duration from options.
    fn timeout_duration(options: &ChatOptions) -> Duration {
        Duration::from_secs(options.timeout_secs.unwrap_or(30))
    }

    /// Classify a reqwest error into LlmError.
    ///
    /// Privacy: error messages use neutral phrasing that does NOT confirm
    /// whether data reached the remote endpoint (Requirement 9.7).
    fn classify_reqwest_error(err: &reqwest::Error) -> LlmError {
        let provider = "openai_compat".to_string();

        if err.is_timeout() {
            return LlmError::Timeout { provider };
        }
        if err.is_connect() {
            return LlmError::NetworkError {
                provider,
                reason: "请求未能完成".to_string(),
            };
        }
        if let Some(status) = err.status() {
            return Self::classify_status(status, None);
        }

        // Privacy: use neutral message, don't expose raw error details
        // that could reveal whether data was transmitted
        LlmError::NetworkError {
            provider,
            reason: "操作失败，请检查网络后重试".to_string(),
        }
    }

    /// Classify an HTTP status code into LlmError.
    fn classify_status(status: StatusCode, body: Option<&str>) -> LlmError {
        let provider = "openai_compat".to_string();

        match status.as_u16() {
            401 | 403 => LlmError::AuthFailure { provider },
            429 => LlmError::RateLimit { provider },
            408 => LlmError::Timeout { provider },
            _ => {
                let reason = body
                    .and_then(|b| {
                        serde_json::from_str::<ApiErrorResponse>(b)
                            .ok()
                            .and_then(|e| e.error)
                            .and_then(|e| e.message)
                    })
                    .unwrap_or_else(|| format!("HTTP {}", status));
                LlmError::Unknown { provider, reason }
            }
        }
    }

    /// Parse finish reason string to enum.
    fn parse_finish_reason(reason: Option<&str>) -> FinishReason {
        match reason {
            Some("stop") => FinishReason::Stop,
            Some("length") => FinishReason::Length,
            Some("content_filter") => FinishReason::ContentFilter,
            _ => FinishReason::Stop,
        }
    }

    /// Execute a chat completion request asynchronously.
    async fn chat_async(
        &self,
        messages: &[ChatMessage],
        options: &ChatOptions,
    ) -> Result<LlmResponse, LlmError> {
        let body = self.build_request_body(messages, options, false);
        let timeout = Self::timeout_duration(options);

        let response = self
            .client
            .post(&self.chat_url())
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .timeout(timeout)
            .json(&body)
            .send()
            .await
            .map_err(|e| Self::classify_reqwest_error(&e))?;

        let status = response.status();
        if !status.is_success() {
            let body_text = response.text().await.unwrap_or_default();
            return Err(Self::classify_status(status, Some(&body_text)));
        }

        let api_response: ApiResponse = response.json().await.map_err(|e| LlmError::Unknown {
            provider: "openai_compat".to_string(),
            reason: format!("响应解析失败: {}", e),
        })?;

        let choice = api_response
            .choices
            .first()
            .ok_or_else(|| LlmError::Unknown {
                provider: "openai_compat".to_string(),
                reason: "响应中无 choices".to_string(),
            })?;

        let content = choice
            .message
            .as_ref()
            .and_then(|m| m.content.clone())
            .unwrap_or_default();

        let finish_reason = Self::parse_finish_reason(choice.finish_reason.as_deref());

        let usage = api_response
            .usage
            .map(|u| TokenUsage {
                prompt_tokens: u.prompt_tokens,
                completion_tokens: u.completion_tokens,
                total_tokens: u.total_tokens,
            })
            .unwrap_or(TokenUsage {
                prompt_tokens: 0,
                completion_tokens: 0,
                total_tokens: 0,
            });

        Ok(LlmResponse {
            content,
            finish_reason,
            usage,
        })
    }

    /// Execute a streaming chat request asynchronously.
    async fn stream_chat_async(
        &self,
        messages: &[ChatMessage],
        options: &ChatOptions,
    ) -> Result<Pin<Box<dyn Stream<Item = StreamChunk> + Send>>, LlmError> {
        let body = self.build_request_body(messages, options, true);
        let timeout = Self::timeout_duration(options);

        let response = self
            .client
            .post(&self.chat_url())
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .timeout(timeout)
            .json(&body)
            .send()
            .await
            .map_err(|e| Self::classify_reqwest_error(&e))?;

        let status = response.status();
        if !status.is_success() {
            let body_text = response.text().await.unwrap_or_default();
            return Err(Self::classify_status(status, Some(&body_text)));
        }

        let byte_stream = response.bytes_stream();
        let sse_stream = SseParser::new(byte_stream);

        Ok(Box::pin(sse_stream))
    }

    /// Validate connectivity by querying GET /v1/models.
    async fn validate_async(&self) -> Result<(), LlmError> {
        let response = self
            .client
            .get(&self.models_url())
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .timeout(Duration::from_secs(10))
            .send()
            .await
            .map_err(|e| Self::classify_reqwest_error(&e))?;

        let status = response.status();
        if !status.is_success() {
            let body_text = response.text().await.unwrap_or_default();
            return Err(Self::classify_status(status, Some(&body_text)));
        }

        Ok(())
    }
}

impl LlmProvider for OpenAiCompatProvider {
    fn name(&self) -> &str {
        "openai_compat"
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
        self.rt.block_on(self.validate_async())
    }
}

// ─── SSE Stream Parser ──────────────────────────────────────────────────────

/// Server-Sent Events parser that converts a byte stream into StreamChunks.
struct SseParser<S> {
    inner: S,
    buffer: String,
    done: bool,
    partial_tokens: u32,
}

impl<S> SseParser<S> {
    fn new(inner: S) -> Self {
        Self {
            inner,
            buffer: String::new(),
            done: false,
            partial_tokens: 0,
        }
    }

    /// Try to parse a complete SSE event from the buffer.
    fn try_parse_event(&mut self) -> Option<StreamChunk> {
        loop {
            let newline_pos = self.buffer.find('\n')?;
            let line = self.buffer[..newline_pos].trim_end_matches('\r').to_string();
            self.buffer = self.buffer[newline_pos + 1..].to_string();

            // Skip empty lines (SSE event separator)
            if line.is_empty() {
                continue;
            }

            // Parse "data: ..." lines
            if let Some(data) = line.strip_prefix("data: ") {
                let data = data.trim();

                // Check for stream termination
                if data == "[DONE]" {
                    return Some(StreamChunk::Done {
                        usage: TokenUsage {
                            prompt_tokens: 0,
                            completion_tokens: self.partial_tokens,
                            total_tokens: self.partial_tokens,
                        },
                    });
                }

                // Parse the JSON chunk
                match serde_json::from_str::<ApiResponse>(data) {
                    Ok(response) => {
                        if let Some(choice) = response.choices.first() {
                            // Check for finish_reason with content
                            if choice.finish_reason.is_some() {
                                if let Some(ref delta) = choice.delta {
                                    if let Some(ref content) = delta.content {
                                        if !content.is_empty() {
                                            return Some(StreamChunk::Delta {
                                                content: content.clone(),
                                            });
                                        }
                                    }
                                }
                                // Don't emit Done here — wait for [DONE]
                                continue;
                            }

                            // Extract delta content
                            if let Some(ref delta) = choice.delta {
                                if let Some(ref content) = delta.content {
                                    if !content.is_empty() {
                                        return Some(StreamChunk::Delta {
                                            content: content.clone(),
                                        });
                                    }
                                }
                            }
                        }

                        // Empty delta or no relevant content, continue
                        continue;
                    }
                    Err(_) => {
                        // Skip unparseable data lines
                        continue;
                    }
                }
            }

            // Skip other SSE fields (event:, id:, retry:, comments)
            continue;
        }
    }
}

impl<S> Stream for SseParser<S>
where
    S: Stream<Item = Result<bytes::Bytes, reqwest::Error>> + Unpin + Send,
{
    type Item = StreamChunk;

    fn poll_next(
        self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        use std::task::Poll;

        let this = self.get_mut();

        if this.done {
            return Poll::Ready(None);
        }

        loop {
            // Try to extract a complete SSE event from the buffer
            if let Some(chunk) = this.try_parse_event() {
                match &chunk {
                    StreamChunk::Done { .. } | StreamChunk::Error { .. } => {
                        this.done = true;
                    }
                    StreamChunk::Delta { .. } => {
                        this.partial_tokens += 1;
                    }
                }
                return Poll::Ready(Some(chunk));
            }

            // Need more data from the inner stream
            let inner = Pin::new(&mut this.inner);
            match inner.poll_next(cx) {
                Poll::Ready(Some(Ok(bytes))) => {
                    match String::from_utf8(bytes.to_vec()) {
                        Ok(text) => this.buffer.push_str(&text),
                        Err(e) => {
                            this.done = true;
                            return Poll::Ready(Some(StreamChunk::Error {
                                error: LlmError::Unknown {
                                    provider: "openai_compat".to_string(),
                                    reason: format!("UTF-8 解码失败: {}", e),
                                },
                                partial_tokens: this.partial_tokens,
                            }));
                        }
                    }
                }
                Poll::Ready(Some(Err(e))) => {
                    this.done = true;
                    let error = if e.is_timeout() {
                        LlmError::Timeout {
                            provider: "openai_compat".to_string(),
                        }
                    } else {
                        // Privacy: use neutral message (Req 9.7) — don't confirm
                        // whether partial data was transmitted
                        LlmError::NetworkError {
                            provider: "openai_compat".to_string(),
                            reason: "操作失败，请检查网络后重试".to_string(),
                        }
                    };
                    return Poll::Ready(Some(StreamChunk::Error {
                        error,
                        partial_tokens: this.partial_tokens,
                    }));
                }
                Poll::Ready(None) => {
                    // Stream ended without [DONE]
                    this.done = true;
                    return Poll::Ready(Some(StreamChunk::Done {
                        usage: TokenUsage {
                            prompt_tokens: 0,
                            completion_tokens: this.partial_tokens,
                            total_tokens: this.partial_tokens,
                        },
                    }));
                }
                Poll::Pending => return Poll::Pending,
            }
        }
    }
}
