# Requirements Document

## Introduction

本文档定义 Cognest Phase 2 的 AI 能力层需求。Phase 1 已完成单机 MVP（碎片/文章 CRUD、搜索、Git 同步、编辑器、ViewStack 导航），完全不依赖 AI/LLM。Phase 2 的目标是让 AI 开始工作——包括本地 Embedding 计算、碎片自动聚类与分类、写作辅助对话、生成式视图渲染、后台任务调度、LLM 统一接口层、周期性回顾，以及用户可配置的 AI 模型设置。

核心设计原则：
- Embedding 必须本地计算（高频、隐私、零成本）
- LLM Generation 云端为主（DeepSeek 为首选示例），Ollama 本地兜底（隐私优先）
- Agent 不是常驻角色，是后台 Job
- 聚类用向量算法（本地免费），LLM 只做命名/综述
- 设置页需提供 API 配置 UI，支持多供应商

## Glossary

- **Embedding_Engine**: 本地 Embedding 计算引擎，基于 fastembed-rs 加载 bge-small-zh-v1.5 模型，将文本转为向量
- **LLM_Gateway**: LLM 统一接口层（Rust trait），封装多家供应商 API 的调用细节，对上层 Agent 暴露统一接口
- **Provider**: LLM_Gateway 的具体实现，对应一个 LLM 供应商（如 DeepSeek、OpenAI、Ollama）
- **Curator_Agent**: 碎片分类 Agent，负责向量聚类和 LLM 命名，自动为碎片打标签、关联 Topic
- **Writing_Agent**: 写作辅助 Agent，在创作页 AI 对话面板中为用户提供写作建议
- **Reflection_Agent**: 回顾 Agent，周期性生成每日/周回顾 Feed 卡片
- **Job_Queue**: 后台任务调度系统，持久化到 SQLite jobs 表，支持断点续跑
- **View_Spec**: 生成式视图的 JSON 规范，定义视图类型和数据，前端用固定组件渲染
- **View_Renderer**: 前端组件，将 View_Spec JSON 映射到对应的 React 可视化组件（react-flow / recharts / 自有组件）
- **Settings_Panel**: macOS 偏好设置风格的模态设置面板，含 AI 模型配置 tab
- **Vault**: 用户的知识库根目录（~/CognestVault），存储碎片、文章、Topic 等纯文本文件

## Requirements

### Requirement 1: 本地 Embedding 计算

**User Story:** As a 用户, I want 碎片入库时自动计算语义向量, so that AI 能基于语义相似度进行聚类和关联发现。

#### Acceptance Criteria

1. WHEN a new fragment is created, THE Embedding_Engine SHALL compute a dense vector (512-d float32, matching bge-small-zh-v1.5 output) for the fragment content within 5 seconds on Apple Silicon hardware; IF the fragment content exceeds the model's maximum token window (512 tokens), THEN THE Embedding_Engine SHALL truncate the content to the maximum token window before computing the vector
2. THE Embedding_Engine SHALL store computed vectors as float32 binary data in the local cache file (.cognest/vectors.bin) indexed by fragment ID
3. WHEN the application starts and detects fragments without cached vectors, THE Embedding_Engine SHALL compute missing vectors in a background thread at a rate of at least 10 vectors per second without increasing UI interaction latency beyond 100ms, and SHALL report batch progress (completed count / total count) to the Status_Bar
4. IF the Embedding model file is missing or fails integrity verification via SHA-256 checksum, THEN THE Embedding_Engine SHALL download or re-extract the model before first use, report download progress (percentage) to the Status_Bar, and abort with an error message indicating download failure if the download does not complete within 120 seconds
5. WHEN the cosine similarity function is called with two fragment IDs, THE Embedding_Engine SHALL return a similarity score between -1.0 and 1.0; IF one or both fragment IDs do not have a computed vector in the cache, THEN THE Embedding_Engine SHALL return an error indicating which fragment IDs are missing vectors
6. WHEN a fragment's content is updated via update_fragment, THE Embedding_Engine SHALL recompute and replace the vector for that fragment within 5 seconds on Apple Silicon hardware

### Requirement 2: LLM Gateway 统一接口

**User Story:** As a 开发者, I want a unified LLM interface supporting multiple providers, so that Agent 实现与具体 LLM 供应商解耦。

#### Acceptance Criteria

1. THE LLM_Gateway SHALL expose a Rust trait with methods: chat(messages, options) returning a structured response containing the generated text, finish reason, and token usage counts, and stream_chat(messages, options) returning a token stream that yields individual content chunks and a final summary chunk with token usage counts
2. THE LLM_Gateway SHALL support at least three Provider implementations: DeepSeek (cloud), OpenAI-compatible (cloud), and Ollama (local)
3. WHEN a Provider is called and the API does not respond within 30 seconds or returns a rate-limit or authentication-failure status, THE LLM_Gateway SHALL return a typed error containing the provider name, an error category enum value (Timeout, RateLimit, AuthFailure, NetworkError, Unknown), and a non-empty descriptive string indicating the failure reason
4. WHEN no Provider is configured or all configured Providers fail, THE LLM_Gateway SHALL return a specific "no available provider" error that the UI can translate to a user-friendly prompt directing to Settings_Panel
5. THE LLM_Gateway SHALL support structured output mode where the LLM response is constrained to a caller-specified JSON schema; IF the provider response fails to conform to the specified schema, THEN THE LLM_Gateway SHALL return a typed validation error containing the schema violation details
6. THE LLM_Gateway SHALL read provider configurations (API endpoint, API key, model name, temperature) from a local encrypted settings file (.cognest/settings.enc) at initialization
7. IF the settings file (.cognest/settings.enc) is missing, unreadable, or fails decryption at initialization, THEN THE LLM_Gateway SHALL start in an unconfigured state with zero available providers and return the "no available provider" error on any subsequent call until valid configuration is provided
8. IF a stream_chat session encounters a provider error or timeout after streaming has begun, THEN THE LLM_Gateway SHALL terminate the stream with a typed error chunk containing the error category and the partial token count consumed up to the failure point

### Requirement 3: AI 模型设置页

**User Story:** As a 用户, I want to configure AI model access in a settings panel, so that 我可以选择使用哪个 LLM 供应商并输入 API Key。

#### Acceptance Criteria

1. THE Settings_Panel SHALL include an "AI 模型" tab displaying a list of configurable Providers (maximum 20 entries) with fields: provider name (maximum 64 characters), API endpoint URL (maximum 2048 characters, validated as a well-formed URL), API key (displayed with only the last 4 characters visible and the rest replaced by dot characters), model name (maximum 128 characters), and an enabled/disabled toggle
2. WHEN the user adds a new Provider configuration and triggers validation, THE Settings_Panel SHALL make a lightweight test API call (e.g., list-models or minimal completion request) and display an inline status indicator adjacent to the API key field showing success or failure within 10 seconds
3. THE Settings_Panel SHALL store API keys encrypted at rest using the operating system's keychain (macOS Keychain) and never expose plaintext keys in logs, IPC messages, or frontend state
4. THE Settings_Panel SHALL provide a pre-configured template for DeepSeek (endpoint: api.deepseek.com, default model: deepseek-chat) that the user only needs to fill in an API key to activate
5. THE Settings_Panel SHALL provide a pre-configured template for Ollama (endpoint: localhost:11434) that queries available local models upon tab open and upon user-triggered refresh, requires no API key, and populates the model name field as a selectable list of detected models
6. WHEN the user modifies Provider configurations and clicks save, THE LLM_Gateway SHALL reload configurations and apply them to subsequent Agent requests within 2 seconds without requiring an application restart
7. THE Settings_Panel SHALL display a "隐私说明" notice explaining which data is sent to cloud Providers (fragment content for classification, article context for writing assistance) and that Embedding is always computed locally
8. WHERE the user has configured 2 or more enabled Providers, THE Settings_Panel SHALL allow setting a default Provider and per-Agent Provider overrides (e.g., Curator uses Ollama, Writing uses DeepSeek)
9. IF the Ollama endpoint is unreachable when auto-detection is triggered, THEN THE Settings_Panel SHALL display an inline error message indicating the connection failed and retain any previously detected model list until a successful refresh occurs
10. IF the user attempts to disable or delete the only remaining enabled Provider, THEN THE Settings_Panel SHALL display a warning indicating that at least one Provider must be enabled and prevent the action

### Requirement 4: Job Queue 持久化

**User Story:** As a 用户, I want background AI tasks to persist across app restarts, so that 中断的任务能自动续跑而不丢失进度。

#### Acceptance Criteria

1. THE Job_Queue SHALL persist all job records (id, agent type, status, payload, result, timestamps, retry count) in a SQLite jobs table
2. WHEN the application starts, THE Job_Queue SHALL reset any job in "running" status to "pending" and re-enqueue all "pending" jobs for execution in creation-time ascending order
3. WHEN a job fails due to a runtime error or execution timeout (300 seconds per job), THE Job_Queue SHALL retry up to 3 times with exponential backoff (5s, 25s, 125s) before marking the job as "failed"
4. IF a job is marked as "failed" after exhausting all retry attempts, THEN THE Job_Queue SHALL emit a Tauri event ("job_failed") containing the job ID, agent type, and failure reason so the Status_Bar can notify the user
5. THE Job_Queue SHALL execute at most 2 jobs concurrently in background threads (std::thread::spawn, not tokio) to avoid blocking the UI thread and respect API rate limits
6. WHEN a job's status changes, THE Job_Queue SHALL emit a Tauri event ("job_status_changed") containing the job ID, new status, and progress percentage (0-100 for running jobs, omitted for other statuses) so the Status_Bar can display current activity
7. THE Job_Queue SHALL support job types: curator_classify, curator_recluster, writing_context, reflection_daily, reflection_weekly
8. IF the LLM_Gateway returns a "no available provider" error during job execution, THEN THE Job_Queue SHALL pause only the affected job (leaving other jobs unaffected), set its status to "blocked", and emit a "provider_needed" event prompting the user to configure a Provider

### Requirement 5: Curator Agent — 碎片自动分类

**User Story:** As a 用户, I want new fragments automatically classified and grouped into topics, so that 我不需要手动整理碎片。

#### Acceptance Criteria

1. WHEN a new fragment is created and its embedding is computed, THE Curator_Agent SHALL find the top 5 most similar existing fragments by cosine similarity from the SQLite index within 3 seconds of embedding completion
2. IF the cosine similarity between a new fragment's embedding and an existing topic cluster centroid (arithmetic mean of all fragment embeddings assigned to that topic) exceeds 0.75, THEN THE Curator_Agent SHALL assign the fragment to the highest-scoring topic by updating the fragment's frontmatter `topics` field
3. IF no existing topic has centroid similarity above 0.75 with the new fragment, and at least 5 fragments with an empty `topics` field form a cluster with average pairwise cosine similarity above 0.70, THEN THE Curator_Agent SHALL create a new Topic by invoking the LLM_Gateway to generate a topic title (max 20 characters) and one-sentence summary (max 100 characters)
4. WHEN a new Topic is created, THE Curator_Agent SHALL write a topic page file to `topics/<slug>.md` with frontmatter fields (type: Topic, title, fragment_count, maturity: seed, created, updated) and a body containing the AI-generated summary
5. WHEN a new fragment is created and its embedding is computed, THE Curator_Agent SHALL assign AI-generated tags (1–5 tags per fragment, each tag max 10 characters) to the fragment by updating the fragment's frontmatter `tags` field
6. WHEN the Curator_Agent updates a fragment's frontmatter, THE Curator_Agent SHALL preserve the original content body unchanged (byte-identical below the frontmatter closing `---`)
7. IF the LLM_Gateway call fails during topic naming, THEN THE Curator_Agent SHALL assign a temporary placeholder title "未命名主题-{timestamp}" and enqueue a retry job with a maximum of 3 retry attempts using exponential backoff (delay starting at 30 seconds)
8. IF embedding computation fails for a new fragment, THEN THE Curator_Agent SHALL mark the fragment as unprocessed (ai_processed: false) in the SQLite index and enqueue a retry job with a maximum of 3 attempts

### Requirement 6: Writing Agent — 创作页 AI 对话

**User Story:** As a 用户, I want an AI conversation panel in the compose page, so that 我在写作时可以获得 AI 辅助建议。

#### Acceptance Criteria

1. THE Writing_Agent SHALL render as a collapsible right panel (320px width) in the compose page, hidden by default and expandable via a toggle button
2. WHEN the user sends a message (maximum 2000 characters) in the AI panel, THE Writing_Agent SHALL include the current article content (up to 4000 tokens), the last 5 related fragments (by tag/topic match, ranked by number of shared tags then by recency), and conversation history (last 10 messages) as context for the LLM_Gateway call
3. WHEN the LLM_Gateway returns a response, THE Writing_Agent SHALL display it as a chat bubble in the panel with streaming token rendering (character-by-character appearance)
4. IF the LLM_Gateway call fails or does not return a response within 30 seconds, THEN THE Writing_Agent SHALL display an error message indicating the failure reason in the chat panel, preserve the user's original message in the input field, and provide a "重试" (retry) button
5. THE Writing_Agent SHALL provide three quick-action buttons above the input: "推荐结构" (suggest outline), "扩展段落" (expand current paragraph), "推荐素材" (recommend related fragments)
6. WHEN the "推荐素材" action is triggered, THE Writing_Agent SHALL search for fragments with highest semantic similarity to the current article content and display up to 5 results as clickable reference cards that can be inserted into the editor
7. WHEN the user clicks a recommended fragment card, THE Writing_Agent SHALL insert a reference chip (@[fragment-id]) at the current editor cursor position; IF the editor has no active cursor position, THEN THE Writing_Agent SHALL append the reference chip at the end of the article content
8. IF no Provider is configured, THEN THE Writing_Agent panel SHALL display a message directing the user to the Settings_Panel AI 模型 tab with a direct link button

### Requirement 7: 生成式视图

**User Story:** As a 用户, I want AI-generated interactive views (knowledge graph, timeline, charts), so that 我可以从不同视角浏览我的知识库。

#### Acceptance Criteria

1. THE View_Renderer SHALL support rendering View_Spec JSON for types: graph (react-flow), timeline (custom component), list (custom component), chart (recharts), summary (Markdown renderer)
2. WHEN the user enters a natural language prompt of up to 500 characters in the Discover page generation bar, THE LLM_Gateway SHALL generate a View_Spec JSON conforming to the defined schema within 15 seconds
3. WHEN a View_Spec is generated, THE View_Renderer SHALL validate the JSON structure against the schema before rendering; IF validation fails, THEN THE View_Renderer SHALL display an error state showing the original prompt text and a "重新生成" (regenerate) button, without rendering any partial view content
4. WHEN the user clicks "固定" (pin) on a generated view, THE system SHALL persist the View_Spec to views/<id>.json in the Vault and add it as a permanent card in the Discover feed
5. WHEN a pinned view is loaded, THE View_Renderer SHALL re-query fresh data from the knowledge base (SQLite index) and complete rendering within 2 seconds for datasets up to 200 nodes/items, while preserving the view's layout configuration from the stored View_Spec
6. THE View_Renderer SHALL render within 500ms of receiving a valid View_Spec for datasets up to 200 nodes/items; IF the dataset exceeds 200 nodes/items, THEN THE View_Renderer SHALL truncate the dataset to the 200 most-connected nodes/items and display an indicator showing the total count versus displayed count
7. IF the LLM_Gateway fails to return a valid View_Spec within 15 seconds or returns a network/API error, THEN THE system SHALL display an error state indicating the failure reason (timeout or service unavailability) and provide a "重试" (retry) button, preserving the user's original prompt text in the generation bar

### Requirement 8: Reflection Agent — 回顾卡片生成

**User Story:** As a 用户, I want periodic review cards showing my knowledge growth, so that 我能了解自己的学习趋势和活跃方向。

#### Acceptance Criteria

1. WHEN the clock reaches 22:00 local time daily, THE Reflection_Agent SHALL enqueue a daily review job that generates a summary Feed card containing: fragments created today count, active topics (topics that received at least 1 new fragment linkage during that calendar day), and a single AI-generated insight of no more than 150 characters
2. WHEN Sunday 22:00 local time is reached, THE Reflection_Agent SHALL enqueue a weekly review job that generates a weekly summary card containing: total fragments this week (Monday 00:00 to Sunday 22:00 local time), new topics created, top 3 most active topics ranked by number of new fragment linkages this week, and a 2-3 sentence AI-generated insight (no more than 400 characters) about knowledge growth direction
3. THE Reflection_Agent SHALL output review results as View_Spec JSON (type: summary) and save them to the views/ directory
4. WHEN a review card is generated, THE Reflection_Agent SHALL emit a Tauri event ("new_feed_card") so the Discover page can refresh its feed
5. IF the LLM_Gateway returns an error or does not respond within 30 seconds when the review job runs, THEN THE Reflection_Agent SHALL generate a statistics-only card (counts and topic names without AI insight text) and mark it as "partial"
6. IF the application was not running at the scheduled review time (22:00 daily or Sunday 22:00), THEN THE Reflection_Agent SHALL enqueue the missed review job upon next application launch, provided no review card for that period already exists

### Requirement 9: 数据隐私保护

**User Story:** As a 用户, I want my data to stay private by default, so that 我的碎片内容不会在未经我同意的情况下发送到云端。

#### Acceptance Criteria

1. THE Embedding_Engine SHALL compute all vectors locally without sending any fragment content to external services
2. WHEN a cloud Provider is used, THE LLM_Gateway SHALL send no more than 20 fragments or 8,000 tokens (whichever is smaller) per single request, limited to fragments and article excerpts directly relevant to the current task, and SHALL never batch-upload the entire Vault
3. THE Settings_Panel SHALL display a visible label next to each operation indicating its data routing: "Cloud" for operations that use cloud Providers (classification naming, writing assistance, view generation, review insights) and "Local" for operations that are purely local (embedding, similarity search, clustering algorithm)
4. WHERE the user has configured only Ollama (local) Providers, THE LLM_Gateway SHALL route all generation requests locally without any network calls to external endpoints
5. THE system SHALL never log, persist, or transmit API keys in plaintext outside the OS keychain storage
6. WHEN a user deletes a Provider configuration, THE Settings_Panel SHALL remove the corresponding API key from the OS keychain within 1 second of the deletion action completing
7. IF a cloud Provider request fails or the network connection is interrupted during transmission, THEN THE LLM_Gateway SHALL discard the pending request payload from memory, not retry automatically, and display an error message indicating the operation failed without confirming whether data reached the remote endpoint
8. WHEN the LLM_Gateway sends a request to a cloud Provider, THE system SHALL log the timestamp, target provider name, and token count (but not the content) to a local-only audit record that the user can review from the Settings_Panel

