# Implementation Plan: Cognest Phase 3

## Overview

本实现计划将 Phase 3 三大模块（Rig Agent 层重构、Markdown 编辑器升级、CLI Agent 集成）拆解为增量式编码任务。每个任务基于前序任务构建，最终通过集成步骤连接所有模块。执行优先级：Rig 框架集成 → 删除旧代码 → Markdown 编辑器 → CLI Agent。

## Tasks

- [x] 1. Rig 框架基础设施与核心接口
  - [x] 1.1 添加 Rig 依赖并创建模块结构
    - 在 `Cargo.toml` 添加 `rig-core = { version = "0.31", features = ["derive"] }` 和 `tokio-util = { version = "0.7", features = ["rt"] }`
    - 创建 `src-tauri/src/core/rig_agents/mod.rs` 模块，声明子模块：`registry`、`router`、`writing`、`curator`、`reflection`、`stream_adapter`
    - 定义 `AgentError` 枚举（AgentUnavailable、NoProvider、ProviderFallback、LlmFailure、ToolFailure、Timeout、Cancelled、Embedding、ProcessSpawn、ProcessAlreadyRunning）
    - 定义 `AgentStatus` 枚举（Available、Unavailable { reason }、Reloading）
    - _Requirements: 1.1, 1.4, 1.6_

  - [x] 1.2 实现 ProviderRouter
    - 创建 `src-tauri/src/core/rig_agents/router.rs`
    - 实现 `RigProvider` 枚举（DeepSeek、OpenAI、Anthropic、Ollama — 均使用 rig openai::Client 兼容接口）
    - 实现 `ProviderRouter::from_config()` 从 AppSettings 构建所有 Provider Client
    - 实现 `resolve()` 方法：按 overrides map 解析 Agent 到 Provider 的映射
    - 实现 `resolve_with_fallback()`：目标 Provider 不可用时回退到 defaultProvider
    - _Requirements: 2.1, 2.2, 2.3, 2.4, 2.5_

  - [x] 1.3 编写 ProviderRouter 属性测试
    - **Property 2: Provider 路由正确解析** — 验证 overrides 存在时返回映射 Provider，否则返回 defaultProvider
    - **Property 3: Provider 回退逻辑** — 验证目标不可用时回退到 default，所有不可用时返回 NoProvider
    - **Validates: Requirements 2.2, 2.3, 2.4, 2.5**

  - [x] 1.4 实现 AgentRegistry
    - 创建 `src-tauri/src/core/rig_agents/registry.rs`
    - 使用 `Arc<RwLock<RegistryInner>>` 实现无阻塞 async 访问
    - 实现 `AgentRegistry::new()` — 从配置初始化所有 Agent
    - 实现 `reload()` — 热重载逻辑（旧实例继续服务直到新实例就绪）
    - 实现 `writing_agent()`、`curator_agent()`、`reflection_agent()` 获取方法
    - 不可用 Agent 返回 `AgentError::AgentUnavailable` 含原因
    - _Requirements: 1.1, 1.2, 1.3, 1.5, 1.6, 1.7_

  - [x] 1.5 编写 AgentRegistry 属性测试
    - **Property 1: Agent 不可用返回明确错误** — 验证标记为不可用的 Agent 返回 AgentUnavailable 错误
    - **Validates: Requirements 1.6**

- [x] 2. Writing Agent 与流式适配
  - [x] 2.1 实现 WritingRigAgent
    - 创建 `src-tauri/src/core/rig_agents/writing.rs`
    - 使用 Rig `client.agent(model).preamble(...).build()` 构建 Agent
    - 实现 `stream_chat()` — 组装 prompt（文章上下文截取前 4000 字符 + 最多 5 条相似碎片，相似度 ≥ 0.5）+ 历史消息，调用 Rig stream
    - 实现 `chat()` — 同步完整响应
    - _Requirements: 3.1, 3.4_

  - [x] 2.2 实现 StreamAdapter（Rig Stream → Tauri Event）
    - 创建 `src-tauri/src/core/rig_agents/stream_adapter.rs`
    - 实现 `stream_to_tauri_events()` — 将 Rig `StreamedAssistantContent` 转为 `writing_chunk` Tauri event
    - 使用 `CancellationToken` 支持取消（2s 内终止）
    - 保持 StreamChunk 的 serde 格式不变：`{"type":"delta","content":"..."}`、`{"type":"done","usage":{...}}`、`{"type":"error",...}`
    - 处理超时（30s 无首个 chunk → Error chunk）
    - _Requirements: 3.1, 3.2, 3.3, 3.5, 3.6, 5.5, 5.6_

  - [x] 2.3 编写 StreamChunk 格式属性测试
    - **Property 4: StreamChunk 格式兼容性** — 验证 Delta/Done/Error 序列化后 JSON 格式符合规定 schema
    - **Validates: Requirements 3.2, 5.5, 5.6**

  - [x] 2.4 编写写作上下文注入属性测试
    - **Property 9: 写作上下文注入约束** — 验证注入的文章上下文不超过 4000 字符、碎片最多 5 条且相似度 ≥ 0.5
    - **Validates: Requirements 3.4**

- [x] 3. Checkpoint — 确保所有测试通过
  - 确保所有测试通过，ask the user if questions arise.

- [x] 4. Curator Agent 与 Tool Calling
  - [x] 4.1 实现 EmbeddingSearchTool
    - 创建 `src-tauri/src/core/rig_agents/curator.rs`
    - 实现 Rig `Tool` trait for `EmbeddingSearchTool`
    - 输入：查询文本（截取前 2000 字符）
    - 输出：top-5 相似碎片的 ID 和相似度分数（0.0-1.0）
    - 集成现有 `EmbeddingEngine` 和 `IndexDb`
    - _Requirements: 4.1, 4.2, 4.3_

  - [x] 4.2 实现 CuratorRigAgent
    - 使用 Rig `client.agent(model).preamble(...).tool(embedding_search_tool).build()` 构建
    - 实现 `classify_fragment()` — 调用 Agent 执行分类（LLM 自主决定是否调用 EmbeddingSearch）
    - 实现 `generate_tags()` — 生成 1-5 个标签（每个 ≤ 10 字符）
    - 保留聚类阈值常量：TOPIC_ASSIGN_THRESHOLD=0.75, CLUSTER_FORM_THRESHOLD=0.70
    - frontmatter topics 合并无重复
    - EmbeddingSearch 失败时降级为未分类状态
    - _Requirements: 4.1, 4.2, 4.4, 4.5, 4.6_

  - [x] 4.3 编写 EmbeddingSearch Tool 属性测试
    - **Property 10: EmbeddingSearch Tool 输入输出约束** — 验证仅处理前 2000 字符、返回最多 5 条、similarity 在 [0.0, 1.0]
    - **Validates: Requirements 4.3**

  - [x] 4.4 编写 Frontmatter 合并属性测试
    - **Property 11: Frontmatter 合并无重复** — 验证 topics 合并无重复、tags 1-5 个且每个 ≤ 10 字符
    - **Validates: Requirements 4.4**

- [x] 5. Reflection Agent 与 Tauri Command 层改造
  - [x] 5.1 实现 ReflectionRigAgent
    - 创建 `src-tauri/src/core/rig_agents/reflection.rs`
    - 基于 Rig 框架构建 Reflection Agent（无 tool calling）
    - 用于 JobQueue 触发的后台反思任务
    - _Requirements: 1.3_

  - [x] 5.2 改造 Tauri Command 层为 async
    - 修改 `src-tauri/src/commands/ai.rs` 中的命令为 async
    - 新增 `RigState` Tauri 管理状态（包含 AgentRegistry）
    - 实现 `writing_stream_chat` 命令 — 获取 WritingAgent → 调用 stream_chat → stream_to_tauri_events
    - 实现 `writing_chat` 命令 — 获取 WritingAgent → 调用 chat
    - 实现 Provider 回退时通知前端（Tauri event）
    - 确保不含 `block_on` 调用
    - _Requirements: 2.6, 3.1, 3.2, 5.3_

  - [x] 5.3 集成 AgentRegistry 到应用启动流程
    - 修改 `main.rs` 或 `lib.rs` 中的 Tauri Builder，注册 `RigState`
    - 应用启动时初始化 AgentRegistry
    - 设置保存命令触发 AgentRegistry reload
    - _Requirements: 1.1, 1.2_

- [x] 6. 删除旧 LlmGateway 和 Agent trait
  - [x] 6.1 删除旧模块并清理引用
    - 删除 `src-tauri/src/core/llm/` 整个目录（mod.rs、deepseek.rs、ollama.rs、openai_compat.rs）
    - 删除 `src-tauri/src/core/agents/` 目录下的旧文件（curator.rs、writing.rs、reflection.rs、mod.rs）
    - 清理所有 `mod`、`use`、`pub mod` 对已删除模块的引用
    - 更新 `core/mod.rs` 引用新的 `rig_agents` 模块
    - 确保 `cargo check` 通过且无 `block_on` 文本搜索结果
    - _Requirements: 5.1, 5.2, 5.3, 5.4, 5.5, 5.6_

- [x] 7. Checkpoint — 确保 Rust 编译通过且所有测试通过
  - 确保所有测试通过，ask the user if questions arise.

- [x] 8. Markdown Serializer 与 Parser
  - [x] 8.1 安装前端依赖
    - 安装 `prosemirror-markdown`、`markdown-it`、`ansi-to-react`
    - 安装 dev 依赖 `fast-check`
    - _Requirements: 6.1, 7.1_

  - [x] 8.2 实现 MarkdownSerializer
    - 创建 `src/utils/markdownSerializer.ts`
    - 使用 `prosemirror-markdown` 的 `MarkdownSerializer` 类
    - 支持 node 序列化：heading (h1-h6)、paragraph、blockquote、code_block（保留语言标识符）、bullet_list、ordered_list（≥4 层嵌套）、horizontal_rule、image（含 alt + URL）、list_item、hard_break
    - 支持 mark 序列化：bold(`**`)、italic(`*`)、code(`` ` ``)、link(`[text](url)`)、strikethrough(`~~`)
    - 支持自定义节点：referenceChip → `@[fragmentId]`
    - 不支持的 node/mark 降级为纯文本 paragraph 输出
    - _Requirements: 6.1, 6.2, 6.3, 6.5, 6.6_

  - [x] 8.3 实现 MarkdownParser
    - 创建 `src/utils/markdownParser.ts`
    - 使用 `prosemirror-markdown` 的 `MarkdownParser` + `markdown-it`
    - 启用 CommonMark + GFM strikethrough
    - 实现自定义 inline rule 解析 `@[hex8]` 为 referenceChip 节点
    - 支持与 Serializer 相同的所有 node 和 mark 类型
    - 不支持的语法块保留为纯文本 paragraph
    - 空字符串或纯空白输入返回仅含一个空 paragraph 的 Document
    - Frontmatter 分离：仅解析第二个 `---` 之后的 body
    - _Requirements: 7.1, 7.2, 7.3, 7.4, 7.5, 8.2_

  - [x] 8.4 编写 Markdown Round-Trip 属性测试
    - **Property 5: Markdown Round-Trip 一致性** — 验证 supported nodes 的 serialize → parse → serialize 逐字符相同
    - **Property 6: 不支持节点的文本保留** — 验证不支持节点的纯文本内容不丢弃
    - **Property 7: 不支持内容的 Round-Trip 稳定性** — 验证二次 round-trip 结果稳定
    - **Property 8: Frontmatter 分离** — 验证 YAML frontmatter 不进入 ProseMirror Document
    - **Validates: Requirements 6.1, 6.2, 6.3, 6.6, 7.1, 7.2, 7.4, 8.1, 8.2, 8.3, 8.4**

- [x] 9. 编辑器双模式支持
  - [x] 9.1 改造 Editor 组件支持双模式切换
    - 修改 `src/components/Editor.tsx`
    - 新增 state：`mode: 'wysiwyg' | 'source'`
    - 添加模式切换按钮（视觉高亮当前模式）
    - WYSIWYG → Source：调用 MarkdownSerializer 转换当前文档
    - Source → WYSIWYG：调用 MarkdownParser 解析 Markdown 文本
    - Source 模式：等宽字体纯文本编辑区域
    - 解析失败时保持 Source 模式并显示错误提示
    - 切换后光标定位（无法映射时定位到文档开头）
    - _Requirements: 9.1, 9.2, 9.3, 9.4, 9.5, 9.6_

  - [x] 9.2 改造自动保存逻辑
    - 编辑器内容变更后 1s 无新输入 → 调用 MarkdownSerializer → 传递给 Tauri 命令
    - Serializer 仅处理 body（不含 frontmatter）
    - _Requirements: 6.4, 6.5_

  - [x] 9.3 编写编辑器模式切换单元测试
    - 测试 WYSIWYG → Source → WYSIWYG round-trip
    - 测试 Source 模式解析失败不切换
    - 测试空内容和边界情况
    - _Requirements: 9.2, 9.3, 9.4_

- [x] 10. Checkpoint — 确保前端编译通过且测试通过
  - 确保所有测试通过，ask the user if questions arise.

- [x] 11. CLI Agent 检测与进程管理
  - [x] 11.1 实现 AgentProcessManager
    - 创建 `src-tauri/src/core/cli_agents/mod.rs` 及 `process_manager.rs`
    - 实现 `detect_agents()` — 扫描 PATH 检测 claude/opencode/kiro 命令，执行 `--version`（单命令超时 5s，总检测 10s 内完成）
    - 实现 `spawn()` — 单进程约束（已有运行中返回 ProcessAlreadyRunning）、CognestVault 为 cwd、stdout/stderr 逐行转发 Tauri event
    - 实现 `kill()` — SIGTERM → 5s → SIGKILL
    - 实现 `status()` — 返回当前进程状态
    - _Requirements: 10.1, 10.2, 10.3, 10.5, 11.1, 11.2, 11.3, 11.4, 11.5, 11.6, 11.7, 11.8_

  - [x] 11.2 实现 ContextGenerator
    - 创建 `src-tauri/src/core/cli_agents/context.rs`
    - 实现 `generate_agents_md()` — 包含 vault 目录结构（depth ≤ 2）、frontmatter 字段说明、topics 列表
    - spawn 前写入/更新 AGENTS.md，写入失败则降级继续
    - _Requirements: 12.1, 12.3, 12.4, 12.5_

  - [x] 11.3 编写 CLI Agent 属性测试
    - **Property 12: 单进程约束** — 验证运行中尝试 spawn 返回 ProcessAlreadyRunning
    - **Property 14: AGENTS.md 内容完整性** — 验证生成内容包含目录结构、字段说明、topics
    - **Validates: Requirements 11.6, 12.4**

  - [x] 11.4 实现 CLI Agent Tauri 命令
    - 新增 `detect_cli_agents` async 命令
    - 新增 `spawn_cli_agent` async 命令（含文章内容注入：编辑页时注入完整 Markdown + frontmatter 为 prompt 前缀）
    - 新增 `kill_cli_agent` async 命令
    - 注册 `CliAgentState`（包含 AgentProcessManager）到 Tauri
    - _Requirements: 10.1, 11.1, 11.4, 12.1, 12.2, 12.3_

- [x] 12. Agent Panel 前端界面
  - [x] 12.1 实现 AgentPanel 组件
    - 创建 `src/components/AgentPanel.tsx` 和 `AgentPanel.css`
    - 展示已检测 CLI Agent 列表（可选择）
    - 不可用 Agent 显示官方安装链接
    - prompt 文本输入区（最大 10,000 字符）
    - 空白 prompt 或未选择 Agent 时禁用提交按钮
    - 运行中显示停止按钮、禁用提交
    - 输出区域：支持 ANSI 颜色（使用 ansi-to-react）、自动滚动、最多保留 5000 行
    - 进程结束后显示退出码和运行时长（精确到秒）
    - 刷新按钮重新检测 Agent
    - _Requirements: 10.4, 10.6, 13.1, 13.2, 13.3, 13.4, 13.5, 13.6_

  - [x] 12.2 编写 AgentPanel 输出缓冲区属性测试
    - **Property 15: Agent Panel 输出缓冲区限制** — 验证超过 5000 行时仅保留最近 5000 行
    - **Validates: Requirements 13.3**

  - [x] 12.3 编写 CLI Agent 输出转发属性测试
    - **Property 13: 进程输出事件忠实转发** — 验证每行 stdout/stderr 对应一个 Tauri event 且内容匹配
    - **Validates: Requirements 11.3, 11.8**

- [x] 13. 最终集成与清理
  - [x] 13.1 集成所有模块并端到端验证
    - 确保 Rig Agent 层 → StreamAdapter → Tauri Event → 前端 WritingPanel 链路完整
    - 确保 Editor 双模式 → MarkdownSerializer/Parser → Tauri save 链路完整
    - 确保 AgentPanel → spawn_cli_agent → ProcessManager → output events 链路完整
    - 验证无 `block_on` 残留
    - `cargo check` 和 `pnpm build` 均通过
    - _Requirements: 5.3, 5.4_

- [x] 14. Final Checkpoint — 确保所有测试通过
  - 确保所有测试通过，ask the user if questions arise.

## Notes

- 标记 `*` 的子任务为可选测试任务，可跳过以加速 MVP
- 每个任务引用了对应的 Requirements 编号以保证可追溯性
- Checkpoints 确保增量验证，避免后期大规模回归
- 属性测试验证普遍正确性，单元测试验证具体场景和边界条件
- Rig 框架使用 openai::Client 兼容 DeepSeek/Ollama（共用 OpenAI 兼容 API），减少实现复杂度
- 删除旧代码安排在新代码完成后（Task 6），确保平滑过渡

## Task Dependency Graph

```json
{
  "waves": [
    { "id": 0, "tasks": ["1.1", "8.1"] },
    { "id": 1, "tasks": ["1.2", "8.2", "8.3"] },
    { "id": 2, "tasks": ["1.3", "1.4", "8.4"] },
    { "id": 3, "tasks": ["1.5", "2.1", "4.1"] },
    { "id": 4, "tasks": ["2.2", "4.2", "5.1"] },
    { "id": 5, "tasks": ["2.3", "2.4", "4.3", "4.4", "5.2"] },
    { "id": 6, "tasks": ["5.3"] },
    { "id": 7, "tasks": ["6.1"] },
    { "id": 8, "tasks": ["9.1", "9.2", "11.1", "11.2"] },
    { "id": 9, "tasks": ["9.3", "11.3", "11.4"] },
    { "id": 10, "tasks": ["12.1"] },
    { "id": 11, "tasks": ["12.2", "12.3"] },
    { "id": 12, "tasks": ["13.1"] }
  ]
}
```
