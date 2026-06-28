# Implementation Plan: Cognest Phase 2 — AI 能力层

## Overview

本任务清单将 Phase 2 AI 设计分解为增量实现步骤。Rust Core 模块先行（embedding → settings → llm → jobs → agents），再扩展 Tauri IPC 层，最后实现 Frontend 组件和 stores。每步构建在前一步基础上，避免孤立代码。

## Tasks

- [x] 1. Rust 依赖与项目骨架
  - [x] 1.1 添加 Cargo 依赖并创建模块文件结构
    - 在 `Cargo.toml` 添加: `fastembed`, `security-framework`, `reqwest` (features: json, stream), `futures`, `tokio-stream`
    - 在 `src-tauri/src/core/` 创建空模块文件: `embedding.rs`, `llm.rs`, `settings.rs`, `jobs.rs`
    - 创建 `src-tauri/src/core/llm/` 目录: `mod.rs`, `deepseek.rs`, `ollama.rs`, `openai_compat.rs`
    - 创建 `src-tauri/src/core/agents/` 目录: `mod.rs`, `curator.rs`, `writing.rs`, `reflection.rs`
    - 更新 `src-tauri/src/core/mod.rs` 声明新模块
    - _Requirements: 2.1, 2.2_

  - [x] 1.2 添加 npm 依赖并创建前端目录结构
    - 安装: `@xyflow/react`, `recharts`, `react-markdown`
    - 创建 `src/stores/settingsStore.ts`, `src/stores/writingStore.ts`, `src/stores/viewStore.ts`
    - 创建 `src/components/WritingPanel.tsx`, `src/components/ViewRenderer.tsx`, `src/components/GenerationBar.tsx`, `src/components/AISettingsTab.tsx`
    - _Requirements: 3.1, 6.1, 7.1_

- [x] 2. EmbeddingEngine 实现
  - [x] 2.1 实现 EmbeddingEngine 核心：模型加载、单文本向量计算、vectors.bin 读写
    - 实现 `EmbeddingEngine::new()` (模型完整性 SHA-256 校验，加载 bge-small-zh-v1.5)
    - 实现 `embed_text()` (512-d float32，超过 512 tokens 截断)
    - 实现 `VectorCache` (vectors.bin 二进制格式: 64 字节 header + 每条 2056 字节记录)
    - 实现 `get_vector()`, `find_unembedded()`
    - _Requirements: 1.1, 1.2, 1.4_

  - [x] 2.2 实现余弦相似度与批量计算
    - 实现 `cosine_similarity()` (返回 -1.0 到 1.0，缺失向量返回错误)
    - 实现 `find_similar()` (top-k 相似碎片)
    - 实现 `compute_centroid()` (质心计算)
    - 实现 `embed_batch()` (后台批处理，进度回调 BatchProgress)
    - _Requirements: 1.3, 1.5, 1.6_

  - [x] 2.3 Property test: Embedding 向量完整性
    - **Property 1: Embedding 向量完整性**
    - 验证计算后 vectors.bin 条目存在且维度 = 512
    - **Validates: Requirements 1.1, 1.2**

  - [x] 2.4 Property test: 余弦相似度数学正确性
    - **Property 2: 余弦相似度数学正确性**
    - 验证 cosine ∈ [-1,1]，self-similarity ≈ 1.0，对称性 sim(a,b) = sim(b,a)
    - **Validates: Requirements 1.5**

- [x] 3. SettingsManager 实现
  - [x] 3.1 实现 SettingsManager：加密配置文件 + Keychain 集成
    - 实现 `SettingsManager::new()`, `load()`, `save()` (AES-256-GCM 加密 .cognest/settings.enc)
    - 实现 `get_api_key()`, `set_api_key()`, `delete_api_key()` (macOS Keychain via security-framework)
    - 定义 `ProviderConfig`, `AppSettings`, `AgentRouting` 类型
    - 处理文件不存在/解密失败场景 → 返回 unconfigured 状态
    - _Requirements: 3.3, 3.6, 9.5, 9.6_

  - [x] 3.2 Property test: API Key 永不明文泄漏
    - **Property 7: API Key 永不明文泄漏**
    - 验证 API Key 只在 Keychain 和进程内存中存在，不出现在日志/文件/IPC
    - **Validates: Requirements 9.5**

- [x] 4. LLM Gateway 与 Provider 实现
  - [x] 4.1 实现 LlmProvider trait 与 LlmGateway 路由
    - 定义 `LlmProvider` trait: `chat()`, `stream_chat()`, `validate()`
    - 定义核心类型: `ChatMessage`, `ChatOptions`, `LlmResponse`, `StreamChunk`, `LlmError`
    - 实现 `LlmGateway::from_config()`, `reload()`, `chat_for_agent()`, `stream_for_agent()`
    - Agent 路由逻辑: default_provider + per-agent overrides
    - 无 Provider 时返回 `LlmError::NoProvider`
    - _Requirements: 2.1, 2.4, 2.6, 2.7_

  - [x] 4.2 实现 DeepSeek Provider
    - 实现 `DeepSeekProvider` struct + `LlmProvider` trait
    - 使用 reqwest 发送 OpenAI-compatible chat/completions 请求
    - 支持 structured output (json_schema in ChatOptions)
    - 超时 30s, 错误分类 (Timeout, RateLimit, AuthFailure, NetworkError)
    - _Requirements: 2.2, 2.3, 2.5_

  - [x] 4.3 实现 Ollama Provider
    - 实现 `OllamaProvider` struct + `LlmProvider` trait
    - `list_models()` (GET /api/tags)
    - Chat via POST /api/chat，流式 via ndjson
    - 无需 API Key，错误处理同上
    - _Requirements: 2.2, 3.5, 9.4_

  - [x] 4.4 实现 OpenAI-compatible Provider
    - 实现 `OpenAiCompatProvider` struct + `LlmProvider` trait
    - 泛化 OpenAI API 格式 (endpoint + api_key + model 可配)
    - 支持 structured output
    - _Requirements: 2.2_

  - [x] 4.5 单元测试: LlmGateway 路由与错误处理
    - 使用 MockLlmProvider 验证路由逻辑
    - 测试 NoProvider / Timeout / AuthFailure 错误分类
    - 测试 stream 中断错误 (StreamChunk::Error with partial_tokens)
    - _Requirements: 2.3, 2.4, 2.8_

- [x] 5. Checkpoint — Rust Core 基础层
  - Ensure all tests pass, ask the user if questions arise.

- [x] 6. Job Queue 实现
  - [x] 6.1 实现 JobQueue：SQLite 持久化 + Worker 线程
    - 创建 `jobs` 表 schema (id, job_type, status, payload, result, timestamps, retry_count, progress, error_message)
    - 实现 `JobQueue::new()`, `enqueue()`, `list_jobs()`, `cancel_job()`
    - 实现 `recover_on_startup()` (running → pending, 按 created_at 排序重新入队)
    - 实现 `start_workers()` (std::thread::spawn, 最多 2 worker)
    - 事件发射通过回调抽象 (`Box<dyn Fn(&str, &str) + Send + Sync>`)，不依赖 Tauri 类型
    - _Requirements: 4.1, 4.2, 4.5, 4.6, 4.7_

  - [x] 6.2 实现重试逻辑与 blocked 状态
    - 指数退避: 5s → 25s → 125s, 最大 3 次重试
    - 单 job 超时 300s
    - LlmError::NoProvider / AuthFailure → 标记 blocked + emit provider_needed
    - 失败后 emit job_failed 事件
    - _Requirements: 4.3, 4.4, 4.8_

  - [x] 6.3 Property test: Job Queue 状态机合法性
    - **Property 3: Job Queue 状态机合法性**
    - 验证只允许合法状态转换 (pending→running, running→{completed,failed,blocked}, etc.)
    - **Validates: Requirements 4.2, 4.3**

  - [x] 6.4 Property test: Job Queue 幂等恢复
    - **Property 8: Job Queue 幂等恢复**
    - 验证崩溃恢复后 running→pending, 无重复执行, 无丢失
    - **Validates: Requirements 4.2**

- [x] 7. Curator Agent 实现
  - [x] 7.1 实现 CuratorAgent: 碎片分类与 Topic 管理
    - 实现 Agent trait for CuratorAgent
    - `classify_fragment()`: 查 top-5 相似碎片 → 检查 centroid > 0.75 → 分配 topic 或创建新 topic
    - 新 Topic 创建: LLM 生成标题 (≤20 chars) + 摘要 (≤100 chars) → 写入 topics/<slug>.md
    - AI 标签生成: 1-5 tags per fragment (每 tag ≤10 chars)
    - 更新 frontmatter (topics, tags 字段)，保留 body 不变
    - 失败时: 占位标题 "未命名主题-{timestamp}", 标记 ai_processed: false, 入队重试
    - _Requirements: 5.1, 5.2, 5.3, 5.4, 5.5, 5.6, 5.7, 5.8_

  - [x] 7.2 Property test: Curator Agent 不修改正文
    - **Property 4: Curator Agent 不修改正文**
    - 验证 Curator 处理前后 body 区域 byte-identical
    - **Validates: Requirements 5.6**

- [x] 8. Writing Agent 实现
  - [x] 8.1 实现 WritingAgent: 写作上下文构建与对话
    - 实现 Agent trait for WritingAgent
    - `build_context()`: 文章内容 (≤4000 tokens) + 相关碎片 (top 5 by tag/topic) + 历史消息 (last 10)
    - `chat()` / `stream_chat()`: 调用 LlmGateway，流式返回
    - `recommend_fragments()`: 按语义相似度推荐 top-5 碎片
    - 快捷动作: outline / expand / recommend 的 system prompt 封装
    - _Requirements: 6.2, 6.3, 6.5, 6.6_

  - [x] 8.2 Property test: 隐私数据量边界
    - **Property 5: 隐私数据量边界**
    - 验证发往 LLM 的请求 fragment_count ≤ 20 且 token_count ≤ 8000
    - **Validates: Requirements 9.2**

- [x] 9. Reflection Agent 实现
  - [x] 9.1 实现 ReflectionAgent: 回顾卡片生成 + 调度器
    - 实现 Agent trait for ReflectionAgent
    - `daily_review()`: 当日碎片数 + 活跃 topic + AI insight (≤150 chars) → ViewSpec (summary)
    - `weekly_review()`: 本周碎片数 + 新 topic + top-3 活跃 topic + AI insight (≤400 chars)
    - LLM 失败时生成纯统计卡片 (partial 标记)
    - 保存 ViewSpec 到 views/<id>.json
    - Emit `new_feed_card` 事件
    - 实现 `ReflectionScheduler`: 独立线程 sleep until 22:00, 入队 daily/weekly job
    - 实现 `check_missed_reviews()`: 启动时补跑遗漏
    - _Requirements: 8.1, 8.2, 8.3, 8.4, 8.5, 8.6_

- [x] 10. Checkpoint — Rust Agents 完成
  - Ensure all tests pass, ask the user if questions arise.

- [x] 11. Tauri IPC 层扩展
  - [x] 11.1 创建 commands/ai.rs 并注册所有 AI 相关命令
    - 创建 `AiState` struct (embedding, llm, jobs, settings 的 Arc 包装)
    - 实现 IPC commands: `get_embedding_status`, `find_similar_fragments`
    - 实现 IPC commands: `writing_chat`, `writing_stream_chat`, `writing_recommend`
    - 实现 IPC commands: `generate_view`, `pin_view`, `list_pinned_views`
    - 实现 IPC commands: `get_ai_settings`, `save_ai_settings`, `validate_provider`, `list_ollama_models`
    - 实现 IPC commands: `list_jobs`, `cancel_job`, `get_audit_log`
    - _Requirements: 2.1, 3.6, 4.6, 6.1, 7.2, 7.4_

  - [x] 11.2 扩展 lib.rs setup() 初始化 AI 子系统
    - 在 setup() 中初始化 `SettingsManager`, `EmbeddingEngine`, `LlmGateway`, `JobQueue`
    - spawn EmbeddingEngine 后台批处理线程 (检测未处理碎片)
    - spawn JobQueue worker 线程 (recover_on_startup + start_workers)
    - spawn ReflectionScheduler 线程
    - 创建 `AiState` 并 manage 到 Tauri
    - 注册新 commands 到 invoke_handler
    - _Requirements: 1.3, 4.2, 8.6_

  - [x] 11.3 创建 audit_log 表并实现审计记录
    - 创建 `audit_log` 表 schema
    - 在 LlmGateway 每次云端请求前后记录: timestamp, provider_name, token_count, operation, success
    - 不记录请求内容
    - _Requirements: 9.8_

- [x] 12. Frontend — Settings Store 与 AI 设置面板
  - [x] 12.1 实现 settingsStore.ts
    - 定义 `ProviderConfig`, `AgentRouting`, `AiSettings` TypeScript 接口
    - 实现 Zustand store: `loadSettings`, `saveSettings`, `validateProvider`, `listOllamaModels`
    - 调用 Tauri invoke 与 Rust 端通信
    - _Requirements: 3.1, 3.6_

  - [x] 12.2 实现 AISettingsTab.tsx 组件
    - Provider 列表 (name, endpoint, masked key, model, enable toggle)
    - 添加 Provider 表单 (预配置模板: DeepSeek + Ollama)
    - 验证按钮 → test API call → 显示 inline success/failure
    - Ollama 模型自动探测 (下拉列表)
    - Agent 路由配置 (default + per-agent overrides)
    - 隐私说明文本
    - 防止禁用唯一 Provider 的警告逻辑
    - 集成到 SettingsModal TABS 数组
    - _Requirements: 3.1, 3.2, 3.4, 3.5, 3.7, 3.8, 3.9, 3.10_

- [x] 13. Frontend — Writing Panel
  - [x] 13.1 实现 writingStore.ts
    - 定义 `ChatMessage`, `RecommendedFragment` 接口
    - 实现 Zustand store: `sendMessage`, `streamMessage`, `retryLast`, `quickAction`, `loadRecommendations`
    - 监听 Tauri event `writing_chunk` 进行流式更新
    - _Requirements: 6.2, 6.3, 6.4_

  - [x] 13.2 实现 WritingPanel.tsx 组件
    - 320px 可折叠右面板
    - 消息气泡列表 (用户/AI，支持 streaming 动画)
    - 快捷动作按钮: 推荐结构 / 扩展段落 / 推荐素材
    - 输入框 + 发送按钮 (max 2000 chars)
    - 错误提示 + 重试按钮
    - 无 Provider 时引导至设置页
    - 推荐素材卡片 → 点击插入 @[fragment-id] reference chip
    - 集成到 Compose.tsx 的 `.ai-side` 区域
    - _Requirements: 6.1, 6.2, 6.3, 6.4, 6.5, 6.6, 6.7, 6.8_

- [x] 14. Frontend — View Renderer 与 Generation Bar
  - [x] 14.1 实现 viewStore.ts
    - 定义 `ViewSpec`, `ViewData`, `GraphData`, `TimelineData`, `ListData`, `ChartData`, `SummaryData` 接口
    - 实现 Zustand store: `generateView`, `pinView`, `unpinView`, `loadPinnedViews`, `clearCurrent`
    - _Requirements: 7.2, 7.4_

  - [x] 14.2 实现 ViewRenderer.tsx 及子组件
    - ViewRenderer: 根据 type 分发到 GraphView / TimelineView / ListView / ChartView / SummaryView
    - GraphView: 使用 @xyflow/react 渲染知识图谱 (nodes + edges)
    - ChartView: 使用 recharts 渲染 bar/line/pie/area
    - SummaryView: 使用 react-markdown 渲染 markdown + stats
    - TimelineView / ListView: 自定义组件
    - 数据截断逻辑: nodes > 200 → 截断 + 显示 "X/Y 已显示" 指示器
    - Schema 验证失败 → 错误状态 + "重新生成" 按钮
    - _Requirements: 7.1, 7.3, 7.5, 7.6_

  - [x] 14.3 实现 GenerationBar.tsx 并集成到 Discover 页
    - 自然语言输入框 (max 500 chars) + 生成按钮
    - Loading 状态显示
    - 集成到 Discover.tsx 顶部
    - 生成结果通过 ViewRenderer 展示
    - 固定按钮 → 保存为 pinned view
    - 错误/超时 → 显示错误 + 重试按钮, 保留原始 prompt
    - _Requirements: 7.2, 7.3, 7.7_

  - [x] 14.4 Property test: View_Spec 数据截断
    - **Property 6: View_Spec 数据截断**
    - 验证渲染时 nodes.length ≤ 200, 超出按 most-connected 截断
    - **Validates: Requirements 7.6**

- [x] 15. Frontend — StatusBar 扩展与 Discover 集成
  - [x] 15.1 扩展 StatusBar 显示 AI 任务状态
    - 监听 Tauri events: `job_status_changed`, `job_failed`, `provider_needed`, `embedding_progress`
    - 显示当前 job 进度 (类型 + 百分比)
    - 失败时显示 Toast 通知
    - provider_needed 时显示引导链接
    - _Requirements: 1.3, 4.4, 4.6, 4.8_

  - [x] 15.2 Discover 页集成 pinned views 和 feed cards
    - 加载并展示 pinned views 列表
    - 监听 `new_feed_card` 事件刷新 feed
    - Reflection 卡片在 feed 中渲染 (ViewRenderer type=summary)
    - _Requirements: 7.4, 7.5, 8.4_

- [x] 16. Data Privacy — 审计与路由标签
  - [x] 16.1 实现隐私标签与审计日志 UI
    - 在 AISettingsTab 中每个操作旁显示 "Cloud" / "Local" 标签
    - 审计日志查看界面: 列表展示 timestamp + provider + operation + token_count + success
    - _Requirements: 9.3, 9.8_

  - [x] 16.2 实现云端请求失败隐私保护逻辑
    - 请求失败时丢弃 payload, 不自动重试
    - 错误消息不确认数据是否到达远端
    - 审计记录 success=false
    - _Requirements: 9.7_

- [x] 17. Final checkpoint — 全部集成完成
  - Ensure all tests pass, ask the user if questions arise.

## Notes

- Tasks marked with `*` are optional and can be skipped for faster MVP
- Each task references specific requirements for traceability
- Checkpoints ensure incremental validation
- Property tests validate universal correctness properties defined in the design
- Unit tests validate specific examples and edge cases
- Rust Core 模块不 import Tauri 类型 (纯 Rust crate 约束)
- 后台线程使用 std::thread::spawn (不使用 tokio in setup)
- 文件系统是 single source of truth，写文件后立即同步 IndexDb

## Task Dependency Graph

```json
{
  "waves": [
    { "id": 0, "tasks": ["1.1", "1.2"] },
    { "id": 1, "tasks": ["2.1", "3.1"] },
    { "id": 2, "tasks": ["2.2", "2.3", "3.2"] },
    { "id": 3, "tasks": ["2.4", "4.1"] },
    { "id": 4, "tasks": ["4.2", "4.3", "4.4"] },
    { "id": 5, "tasks": ["4.5", "6.1"] },
    { "id": 6, "tasks": ["6.2", "6.3", "6.4"] },
    { "id": 7, "tasks": ["7.1", "8.1"] },
    { "id": 8, "tasks": ["7.2", "8.2", "9.1"] },
    { "id": 9, "tasks": ["11.1"] },
    { "id": 10, "tasks": ["11.2", "11.3"] },
    { "id": 11, "tasks": ["12.1", "13.1", "14.1"] },
    { "id": 12, "tasks": ["12.2", "13.2", "14.2"] },
    { "id": 13, "tasks": ["14.3", "14.4", "15.1"] },
    { "id": 14, "tasks": ["15.2", "16.1", "16.2"] }
  ]
}
```
