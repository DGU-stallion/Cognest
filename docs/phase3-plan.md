# Cognest Phase 3 规划

## 技术决策

- **Agent 层**：引入 Rig 框架重构整个 Agent 层（替代手写 trait + reqwest + block_on）
- **编辑器**：采用 Gravity-UI 双模式架构（TipTap WYSIWYG + prosemirror-markdown 序列化）
- **外部 Agent**：Spawn 本地 CLI Agent（参考 Yume），先做 MCP Server 延后 ACP

---

## 实施计划

### 3.1 Rig 框架重构 Agent 层（3-4 天）

**目标：** 用 Rig 替换当前手写的 LlmGateway + Agent trait，彻底解决 tokio 死锁问题，获得 async + tool calling + 多 provider 统一管理

**为什么不修复而是重构：**
- 当前 `block_on` 架构有结构性死锁问题，修补只是续命
- Rig 是 async-first 设计，与 Tauri v2 的 async commands 天然兼容
- Rig 内置多 Provider 管理（DeepSeek, OpenAI, Anthropic, Ollama），不需手写 LlmGateway
- Tool calling 支持让 Curator Agent 可以自主调用 embedding 搜索

**范围：**
- 替换 `src-tauri/src/core/agents/` 下的 Agent trait 和实现
- 替换 `src-tauri/src/core/llm/` 下的 LlmGateway
- 重写 `writing_stream_chat` 命令为纯 async Rig agent 调用
- 重写 Curator Agent 为 Rig agent + tools

**不做：**
- 不改前端 WritingPanel 的交互逻辑
- 不改 Tauri events 的格式（前端无感知）
- 不引入 langgraph-rust（目前不需要复杂 workflow）

**Rig API 参考：**
```rust
let agent = openai_client
    .agent("deepseek-chat")
    .preamble("You are a helpful writing assistant")
    .tool(EmbeddingSearchTool)
    .build();

let response = agent.prompt("帮我扩展这个段落").await?;
```

### 3.2 Markdown 编辑器升级（3-4 天）

**目标：** 编辑器和预览统一基于 Markdown，消除 HTML/MD 格式混乱

**步骤：**
1. 安装 `prosemirror-markdown` 包
2. 配置 MarkdownSerializer：将 TipTap 的 ProseMirror doc 序列化为 Markdown
3. 配置 MarkdownParser：将 Markdown 解析为 ProseMirror doc
4. 修改 `saveArticle` 命令：存储 Markdown 而非 HTML
5. 修改文章预览：直接用 `react-markdown` 渲染
6. 验证：编辑 → 保存 → 重新打开 → 预览，全链路 Markdown 一致

### 3.3 App 内 Spawn 本地 CLI Agent（3-4 天）

**目标：** 在 Cognest App UI 内调用本地安装的 Claude Code / OpenCode / Kiro CLI 执行更复杂任务

**参考：** Yume（Claude Code 的 Tauri 桌面 UI）

**支持的 CLI Agent：**
| Agent | CLI 命令 | 检测方式 |
|-------|----------|----------|
| Claude Code | `claude` | `which claude` |
| OpenCode | `opencode` | `which opencode` |
| Kiro CLI | `kiro` | `which kiro` |

**架构：**
```
Cognest App UI (AgentPanel)
    │ Tauri async command
    ▼
Rust Backend
    - detect_installed_agents() → 扫描 PATH
    - spawn_agent(cli, args, cwd) → 子进程管理
    - stream stdout/stderr → Tauri events
    │ spawn(cli, [...])
    ▼
Local CLI Agent Process
    - 工作目录: ~/CognestVault
    - 上下文: AGENTS.md + 当前文章内容
```

**实现步骤：**
1. Rust: `detect_agents()` — 扫描 PATH 检测已安装的 CLI agent
2. Rust: `spawn_agent()` — 启动子进程，流式转发 stdout/stderr 为 Tauri events
3. Rust: `kill_agent()` — 终止运行中的 agent 进程
4. Frontend: AgentPanel 组件 — 选择 agent + 输入 prompt + 实时输出显示
5. 集成: agent 工作目录设为 CognestVault，注入 AGENTS.md 作为上下文

### 3.4 索引与数据一致性（1 天）

- App 启动时智能判断是否需要重建索引
- 文章 tags 正确同步到索引
- 热力图对接真实碎片创建频率数据

### 3.5 Topic 系统 UI（1-2 天）

- 创作页"话题"tab：展示当前文章关联的 Topics + 推荐 Topics
- 发现页：Topic 关系图（使用 ViewRenderer graph 组件）

### 3.6 MCP Server（延后）

- 将 Cognest 知识库暴露为 MCP Server，让外部 Agent 可以读写
- 实现 `cognest-mcp` binary + `cognest mcp install <agent>` 命令

---

## 优先级排序

| # | 任务 | 预估 | 说明 |
|---|------|------|------|
| 1 | 3.1 Rig 重构 Agent 层 | 3-4 天 | 解决死锁 + 获得 tool calling |
| 2 | 3.2 Markdown 编辑器升级 | 3-4 天 | 统一存储格式 |
| 3 | 3.3 Spawn 本地 CLI Agent | 3-4 天 | App 内调用 claude/opencode/kiro |
| 4 | 3.4 索引一致性 | 1 天 | 数据同步 |
| 5 | 3.5 Topic UI | 1-2 天 | 话题可视化 |
| 6 | 3.6 MCP Server | 延后 | 开放给外部 Agent |

**执行顺序：** 3.1 → 3.2 → 3.3 → 3.4 → 3.5

---

## 调研结论（参考资料）

### Markdown 编辑器架构

| 方案 | 技术栈 | 适合度 |
|------|--------|--------|
| **Gravity-UI** ✓ | ProseMirror (WYSIWYG) + CodeMirror 6 (Markup) + markdown-it | ★★★★ 最佳参考 |
| Obsidian | CodeMirror 6 + Decorations | ★★★ |
| Tolaria | Tauri + Block Editor | ★★★ |
| Milkdown | ProseMirror + Remark + Y.js | ★★ |

### Agent 框架对比

| 方案 | 解决什么问题 | 推荐度 |
|------|-------------|--------|
| **Rig** ✓ | 死锁 + tool calling + 多 provider | ★★★★ 采用 |
| 纯 async reqwest | 仅修复死锁 | ★★★ 被替代 |
| langgraph-rust | 复杂 workflow | ★★ 过重 |
| tauri-plugin-llm | 离线本地推理 | ★★ 可选补充 |

### Spawn CLI Agent 参考

| 项目 | 做法 | 可借鉴点 |
|------|------|----------|
| **Yume** ✓ | Spawn claude 进程 + 流式 UI | 最直接参考 |
| Open Design | Daemon + 22 种 Agent adapter | 多 agent 管理模式 |

### MCP/ACP 决策

- 先做 MCP Server（stdio transport），覆盖 90% 需求
- ACP 延后（Agent-to-Agent 场景目前不急需）

---

## 待定技术决策

1. MCP Server 实现语言：Rust binary（复用 Core）vs Node.js（生态更丰富）
2. 是否引入 `rmcp` crate vs 手写 stdio JSON-RPC
3. Markdown 编辑器是否需要 Split View（CodeMirror 6 + 实时预览）
4. ACP 的实现时机（当需要 Agent-to-Agent 协作时再做）
