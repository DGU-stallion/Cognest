# Requirements Document

## Introduction

Cognest Phase 3 对应用的 AI Agent 层、编辑器存储格式、以及外部 CLI Agent 集成三个核心模块进行升级重构。主要目标：

1. **Rig 框架重构 Agent 层** — 用 Rig 替换手写的 LlmGateway + Agent trait，解决 tokio `block_on` 死锁问题，获得 async-first + tool calling + 多 Provider 统一管理能力。
2. **Markdown 编辑器升级** — 采用 Gravity-UI 双模式架构思路，集成 prosemirror-markdown 实现 ProseMirror ↔ Markdown 双向序列化，统一存储格式为纯 Markdown。
3. **App 内 Spawn 本地 CLI Agent** — 在应用 UI 内检测并调用本地安装的 Claude Code / OpenCode / Kiro CLI 执行复杂任务，参考 Yume 实现。

## Glossary

- **Rig_Agent**: 基于 Rig 框架构建的 async Agent 实例，替代当前手写的 Agent trait 实现
- **Rig_Client**: Rig 框架提供的 LLM Provider 客户端（如 `rig::providers::openai::Client`），统一管理 API 密钥和配置
- **Agent_Registry**: 管理所有 Rig_Agent 实例的注册中心，负责 Agent 的创建、配置热更新和生命周期管理
- **Tool_Trait**: Rig 框架定义的 Tool trait，用于实现 Agent 可调用的工具函数（如 embedding 搜索）
- **Provider_Router**: 基于设置中的路由规则将 Agent 请求分发到对应 LLM Provider 的路由器
- **Markdown_Serializer**: 基于 prosemirror-markdown 的序列化器，将 ProseMirror Document 转换为 Markdown 字符串
- **Markdown_Parser**: 基于 prosemirror-markdown 的解析器，将 Markdown 字符串转换为 ProseMirror Document
- **Editor_Mode**: 编辑器的显示模式，包含 WYSIWYG 模式（TipTap 富文本）和 Source 模式（原始 Markdown 文本）
- **CLI_Agent**: 用户本地安装的命令行 AI Agent 工具（Claude Code、OpenCode、Kiro CLI）
- **Agent_Process**: Rust 后端管理的 CLI_Agent 子进程，负责 spawn、流式输出转发和生命周期管理
- **Agent_Panel**: 前端展示 CLI_Agent 交互界面的 React 组件
- **CognestVault**: 用户的知识库根目录，作为 CLI_Agent 的工作目录

## Requirements

### Requirement 1: Rig Agent 注册与生命周期管理

**User Story:** As a 开发者, I want Agent 层基于 Rig 框架统一构建和管理, so that 所有 Agent 调用都是 async-first 且无死锁风险。

#### Acceptance Criteria

1. WHEN 应用启动时, THE Agent_Registry SHALL 根据当前 Provider 配置创建所有 Rig_Agent 实例
2. WHEN Provider 配置发生变更时, THE Agent_Registry SHALL 在 2 秒内完成 Agent 实例的热重载，热重载完成的标志为新 Agent 实例已就绪可接受请求且旧实例已释放
3. THE Agent_Registry SHALL 支持注册至少三种 Rig_Agent：Writing Agent、Curator Agent、Reflection Agent
4. IF Rig_Client 创建失败（如 API 密钥无效或 endpoint 不可达）, THEN THE Agent_Registry SHALL 记录错误日志并将对应 Agent 标记为不可用状态
5. THE Agent_Registry SHALL 提供 async 方法获取指定 Agent 的引用，无需 Mutex lock 阻塞
6. IF 调用方请求获取处于不可用状态的 Agent, THEN THE Agent_Registry SHALL 返回明确的 AgentUnavailable 错误，包含不可用原因描述
7. WHILE Agent 实例热重载进行中, THE Agent_Registry SHALL 继续使用旧 Agent 实例处理请求直到新实例就绪，不中断正在执行的调用

### Requirement 2: Rig Provider 路由

**User Story:** As a 用户, I want 不同 Agent 可以灵活使用不同的 LLM Provider, so that 我可以为写作用高质量模型而分类用经济模型。

#### Acceptance Criteria

1. THE Provider_Router SHALL 支持 DeepSeek、OpenAI、Anthropic、Ollama 四种 Provider 类型
2. WHEN 用户在设置中配置 Agent 到 Provider 的映射时, THE Provider_Router SHALL 将对应 Agent 的请求路由到指定 Provider
3. IF Agent 未配置显式 Provider 映射, THEN THE Provider_Router SHALL 使用 AgentRouting.defaultProvider 作为路由目标
4. IF 指定的 Provider 不可用（API 调用超时 30 秒或返回 5xx 错误）, THEN THE Provider_Router SHALL 回退到默认 Provider 并记录警告日志
5. IF 所有 Provider 均不可用, THEN THE Provider_Router SHALL 返回明确的 NoProvider 错误
6. WHEN Provider 回退发生时, THE Provider_Router SHALL 通过 Tauri event 通知前端当前使用了回退 Provider

### Requirement 3: Writing Agent 异步流式输出

**User Story:** As a 用户, I want 写作助手的响应实时流式显示, so that 我无需等待完整响应即可开始阅读。

#### Acceptance Criteria

1. WHEN 用户在 WritingPanel 发送消息时, THE Rig_Agent SHALL 以 async stream 方式返回 token chunks，首个 chunk 须在请求发出后 30 秒内到达前端
2. WHILE 流式输出进行中, THE Rig_Agent SHALL 通过 Tauri event（事件名 `writing_chunk`）将每个 StreamChunk 转发至前端，payload 格式保持 `{type: "Delta"|"Done"|"Error", content?, usage?, error?}` 不变，单个 chunk 从后端接收到 emit 的延迟不超过 100ms
3. WHILE 流式输出进行中, THE Rig_Agent SHALL 保持可取消状态；WHEN 用户触发取消操作时, THE Rig_Agent SHALL 在 2 秒内终止 LLM 请求并发送 Done chunk，前端保留已接收的部分内容
4. WHEN 构建 Writing Agent prompt 时, THE Rig_Agent SHALL 在上下文中注入当前文章内容（截取前 4000 字符）和通过 embedding 相似度检索的相关碎片（最多 5 条，相似度阈值 ≥ 0.5）
5. IF 流式输出中途发生网络错误或 Provider 返回异常, THEN THE Rig_Agent SHALL 发送 Error chunk（包含错误类别描述），前端保留已累积的 Delta 内容不清除
6. IF 首个 StreamChunk 在 30 秒内未到达, THEN THE Rig_Agent SHALL 发送 Error chunk 指示超时，并终止本次请求

### Requirement 4: Curator Agent Tool Calling

**User Story:** As a 用户, I want Curator Agent 能自主调用 embedding 搜索工具, so that 分类和聚类决策更加智能。

#### Acceptance Criteria

1. THE Rig_Agent（Curator）SHALL 注册 EmbeddingSearch 作为 Rig Tool_Trait 实现
2. WHEN Curator Agent 执行分类任务时, THE Rig_Agent SHALL 通过 Rig 框架的 tool calling 机制调用 EmbeddingSearch 工具查找相似碎片，调用决策由 LLM 自主完成
3. THE EmbeddingSearch Tool SHALL 接受不超过 2000 字符的查询文本参数，并返回 top-5 相似碎片的 ID 和相似度分数（范围 0.0 到 1.0）
4. WHEN Curator Agent 完成分类时, THE Rig_Agent SHALL 以合并方式（不产生重复项）更新碎片 frontmatter 的 topics 字段，并生成 1 至 5 个标签（每个标签不超过 10 个字符）写入 tags 字段
5. THE Rig_Agent（Curator）SHALL 保留现有的聚类阈值常量（TOPIC_ASSIGN_THRESHOLD=0.75, CLUSTER_FORM_THRESHOLD=0.70）
6. IF EmbeddingSearch 工具调用失败（如 embedding 索引不可用或向量未计算）, THEN THE Rig_Agent SHALL 跳过相似度匹配步骤，将碎片保留为未分类状态，并记录警告日志

### Requirement 5: 删除旧 LlmGateway 和 Agent trait

**User Story:** As a 开发者, I want 旧的手写抽象被完全移除, so that 代码库不存在重复的 LLM 调用路径。

#### Acceptance Criteria

1. THE 系统 SHALL 不再包含 `src-tauri/src/core/llm/` 目录（含 `mod.rs`、`deepseek.rs`、`ollama.rs`、`openai_compat.rs` 等所有文件须删除）
2. THE 系统 SHALL 不再包含 `src-tauri/src/core/agents/mod.rs` 中的 `Agent` trait 定义及 `AgentError` 枚举；该目录下旧的 `curator.rs`、`writing.rs`、`reflection.rs` 实现文件须删除
3. THE 系统 SHALL 不包含任何通过 `Runtime::block_on` 或独立 tokio Runtime 同步阻塞调用 LLM API 的代码路径（对 `src-tauri/src/` 执行 `block_on` 文本搜索结果为零匹配）
4. THE 系统 SHALL 不包含对已删除模块（`core::llm`、旧 `core::agents` trait）的 `mod`、`use` 或 `pub mod` 引用，且项目须通过 `cargo check` 无错误
5. THE 系统 SHALL 保持以下 Tauri event 的名称和 payload JSON schema 不变：`writing_chunk`（StreamChunk 格式：Delta/Done/Error 三种 variant）、`job_status_changed`（JobStatusEvent 格式）、`index_updated`（无 payload）
6. IF 新 Rig_Agent 实现需要发送流式 chunk, THEN THE 系统 SHALL 使用与旧 `StreamChunk` 相同的 serde tag 格式（`{"type":"delta","content":"..."}` / `{"type":"done","usage":{...}}` / `{"type":"error","error":{...},"partial_tokens":N}`）

### Requirement 6: Markdown Serializer（PM → MD）

**User Story:** As a 用户, I want 编辑器中的富文本内容能正确序列化为 Markdown, so that 文件系统中存储的是标准 Markdown 格式。

#### Acceptance Criteria

1. THE Markdown_Serializer SHALL 将 ProseMirror Document 序列化为符合 CommonMark 规范（附加 GFM strikethrough 扩展）的 Markdown 字符串
2. THE Markdown_Serializer SHALL 支持以下 node 类型的序列化：heading (h1-h6)、paragraph、blockquote、code_block（保留语言标识符）、bullet_list、ordered_list（支持至少 4 层嵌套）、horizontal_rule、image（包含 alt 文本和 URL）
3. THE Markdown_Serializer SHALL 支持以下 mark 类型的序列化：bold（`**`）、italic（`*`）、code（`` ` ``）、link（`[text](url)`）、strikethrough（`~~`）
4. WHEN 文章自动保存触发时（编辑器内容变更后 1 秒无新输入）, THE Editor 组件 SHALL 调用 Markdown_Serializer 将当前文档转为 Markdown 后传递给 Tauri 命令存储
5. THE Markdown_Serializer SHALL 仅接收并处理文章 body 内容（不含 YAML frontmatter），frontmatter 的拼接与解析由 Rust 端负责
6. IF ProseMirror Document 包含 Serializer 不支持的 node 或 mark 类型（如自定义扩展节点）, THEN THE Markdown_Serializer SHALL 将该节点的纯文本内容作为 paragraph 输出，不丢弃内容

### Requirement 7: Markdown Parser（MD → PM）

**User Story:** As a 用户, I want 打开已有的 Markdown 文件时编辑器能正确还原富文本显示, so that 编辑体验流畅。

#### Acceptance Criteria

1. THE Markdown_Parser SHALL 将 CommonMark Markdown 字符串解析为 ProseMirror Document
2. THE Markdown_Parser SHALL 支持解析与 Markdown_Serializer 相同的所有 node 和 mark 类型：heading (h1-h6)、paragraph、blockquote、code_block、bullet_list、ordered_list、horizontal_rule、image、bold、italic、code、link、strikethrough
3. WHEN 用户打开文章时, THE Editor 组件 SHALL 调用 Markdown_Parser 将 Markdown 正文转为 ProseMirror Document 加载到编辑器中，解析耗时不超过 500ms（对于 100KB 以内的文档）
4. IF Markdown 内容包含 Parser 不支持的语法块, THEN THE Markdown_Parser SHALL 将该块级元素的原始文本作为纯文本 paragraph 节点保留，不丢弃内容
5. IF Markdown 正文为空字符串或仅包含空白字符, THEN THE Markdown_Parser SHALL 返回仅含一个空 paragraph 节点的 ProseMirror Document

### Requirement 8: Markdown 往返一致性（Round-Trip）

**User Story:** As a 用户, I want 编辑保存再打开后内容完全一致, so that 不丢失格式信息。

#### Acceptance Criteria

1. IF ProseMirror Document 仅包含 Markdown_Serializer 支持的 node 类型（heading, paragraph, blockquote, code_block, bullet_list, ordered_list, horizontal_rule, image）和 mark 类型（bold, italic, code, link, strikethrough）, THEN 执行 serialize → parse → serialize SHALL 产生与第一次 serialize 逐字符相同的 Markdown 字符串（round-trip 一致性），包括尾部换行符
2. WHEN 系统对碎片文件（capture/*.md）执行 parse 时, THE Markdown_Parser SHALL 仅解析第二个 `---` 分隔符之后的 body 部分，忽略 YAML frontmatter 区块
3. IF 文章内容仅包含 Serializer 支持的元素, THEN THE round-trip 转换 SHALL 不引入额外空行、不改变空行数量、不改变缩进或列表标记符号
4. IF ProseMirror Document 包含 Serializer 不支持的 node 或 mark 类型, THEN THE round-trip 转换 SHALL 将该内容以纯文本 paragraph 保留，后续 round-trip 对该纯文本部分保持稳定不再变化

### Requirement 9: 编辑器双模式切换

**User Story:** As a 用户, I want 在富文本编辑和原始 Markdown 之间自由切换, so that 我可以精确控制格式。

#### Acceptance Criteria

1. THE Editor 组件 SHALL 提供 WYSIWYG 模式和 Source 模式的切换按钮，并以视觉高亮指示当前激活的模式
2. WHEN 用户从 WYSIWYG 模式切换到 Source 模式时, THE Editor 组件 SHALL 调用 Markdown_Serializer 将当前文档（含未保存编辑）转为 Markdown 文本显示
3. WHEN 用户从 Source 模式切换回 WYSIWYG 模式时, THE Editor 组件 SHALL 调用 Markdown_Parser 将 Markdown 文本解析回 ProseMirror Document
4. IF Markdown_Parser 解析 Source 模式中的文本失败, THEN THE Editor 组件 SHALL 保持当前 Source 模式不变，并向用户显示解析错误提示信息
5. WHILE 用户处于 Source 模式时, THE Editor 组件 SHALL 提供等宽字体的纯文本编辑区域
6. WHEN 模式切换完成时, THE Editor 组件 SHALL 将光标定位到对应位置（如果原位置无法在目标模式中映射，则定位到文档开头）

### Requirement 10: 检测已安装的 CLI Agent

**User Story:** As a 用户, I want 应用自动检测我本地安装了哪些 CLI Agent, so that 我无需手动配置即可使用。

#### Acceptance Criteria

1. WHEN 用户打开 Agent_Panel 时, THE 系统 SHALL 扫描 PATH 环境变量检测已安装的 CLI_Agent，整个检测流程（含版本查询）SHALL 在 10 秒内完成
2. THE 系统 SHALL 检测以下 CLI_Agent：`claude`（Claude Code）、`opencode`（OpenCode）、`kiro`（Kiro CLI）
3. THE 系统 SHALL 对每个检测到的 CLI_Agent 返回名称、CLI 命令绝对路径和版本字符串（通过执行 `<command> --version` 获取的第一行输出）
4. IF 某个 CLI_Agent 的可执行文件未在 PATH 中找到, THEN THE Agent_Panel SHALL 将该 Agent 显示为不可用状态并展示该 Agent 的官方安装链接
5. IF 某个 CLI_Agent 的可执行文件存在但执行 `--version` 失败或超时（单个命令超时 5 秒）, THEN THE 系统 SHALL 将该 Agent 标记为已安装但版本未知，并在版本字段显示"版本未知"
6. WHEN 用户在 Agent_Panel 点击刷新按钮时, THE 系统 SHALL 重新执行 CLI_Agent 检测流程并更新显示状态

### Requirement 11: Spawn 和管理 CLI Agent 进程

**User Story:** As a 用户, I want 在应用内启动 CLI Agent 执行任务, so that 我无需切换到终端。

#### Acceptance Criteria

1. WHEN 用户选择 CLI_Agent 并提交 prompt 时, THE 系统 SHALL spawn 对应的 CLI 进程作为子进程，将用户 prompt 作为命令行参数传递给 CLI_Agent
2. THE Agent_Process SHALL 以 CognestVault 目录作为工作目录
3. THE Agent_Process SHALL 将 stdout 和 stderr 逐行读取并以 Tauri events 转发至前端，每行作为一个独立事件发送
4. WHEN 用户点击停止按钮时, THE 系统 SHALL 向 Agent_Process 发送 SIGTERM 信号终止进程
5. IF Agent_Process 在 SIGTERM 后 5 秒未退出, THEN THE 系统 SHALL 发送 SIGKILL 强制终止
6. IF 已有一个 Agent_Process 正在运行时用户尝试启动新进程, THEN THE 系统 SHALL 拒绝启动并向前端返回错误提示，说明当前有进程正在运行
7. IF CLI_Agent 进程 spawn 失败（如命令不存在或权限不足）, THEN THE 系统 SHALL 向前端发送包含失败原因的错误事件
8. WHEN Agent_Process 退出时, THE 系统 SHALL 向前端发送包含退出状态码的完成事件

### Requirement 12: CLI Agent 上下文注入

**User Story:** As a 用户, I want CLI Agent 能获得当前工作上下文, so that Agent 理解我的知识库结构和当前任务。

#### Acceptance Criteria

1. WHEN spawn CLI_Agent 时, THE 系统 SHALL 在 CognestVault 工作目录中生成或更新 `AGENTS.md` 文件，并在文件写入完成后再启动 CLI 子进程
2. WHEN 用户在文章编辑页发起 CLI Agent 请求时, THE 系统 SHALL 将当前文章的完整 Markdown 内容（含 frontmatter）作为 prompt 前缀注入 CLI_Agent 的启动参数中
3. IF 用户发起 CLI Agent 请求时未处于文章编辑页, THEN THE 系统 SHALL 仅依赖 AGENTS.md 提供上下文，不注入额外文章内容
4. THE AGENTS.md 文件 SHALL 包含：CognestVault 顶层目录结构（capture/、articles/ 及其子目录，深度不超过 2 层）、碎片和文章的 YAML frontmatter 字段说明、当前所有 topics 名称列表
5. IF AGENTS.md 文件生成或写入失败, THEN THE 系统 SHALL 记录错误日志并继续 spawn CLI_Agent（以无上下文文件的降级模式运行）

### Requirement 13: Agent Panel 前端界面

**User Story:** As a 用户, I want 一个专属的面板来与 CLI Agent 交互, so that 输入输出清晰可见。

#### Acceptance Criteria

1. THE Agent_Panel SHALL 展示已检测到的 CLI_Agent 列表，支持选择要使用的 Agent
2. THE Agent_Panel SHALL 提供文本输入区域用于输入 prompt，最大输入长度为 10,000 字符
3. WHILE Agent_Process 运行中, THE Agent_Panel SHALL 在接收到 Tauri event 后 200ms 内渲染 stdout/stderr 输出（支持 ANSI 颜色转义），输出区域自动滚动至最新内容，最多保留最近 5,000 行输出
4. WHILE Agent_Process 运行中, THE Agent_Panel SHALL 显示停止按钮并禁用 prompt 提交功能
5. WHEN Agent_Process 结束时, THE Agent_Panel SHALL 显示退出状态码和总运行时长（精确到秒）
6. IF 用户提交空白 prompt 或未选择 CLI_Agent, THEN THE Agent_Panel SHALL 禁用提交按钮，不发起 spawn 请求
