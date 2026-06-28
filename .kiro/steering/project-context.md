---
inclusion: auto
description: Cognest 项目的整体上下文，包括技术栈、架构约束、项目结构和设计规范
---

# Cognest 项目上下文

## 项目概述
Cognest 是一个 AI 认知工作台桌面应用，帮助用户将碎片化灵感演化为系统化认知。
组织原则：「人在两端，AI 在中间」—— 用户负责输入（碎片）和输出（文章），中间整理全交给 AI。

## 技术栈
- 桌面框架：Tauri v2（系统 WebView）
- 前端：React + TypeScript + Vite
- 状态管理：Zustand
- 编辑器：TipTap / ProseMirror + prosemirror-markdown（双向序列化）
- 后端：Rust（Tauri commands + core modules）
- AI 框架：rig-core 0.31（async Agent + tool calling + 多 Provider 路由）
- Embedding：fastembed（本地向量化，无需外部服务）
- 数据库：SQLite + FTS5（可丢弃派生索引）
- 文件监听：notify crate
- Git：git2 crate
- 异步：tokio（AI Agent 调用、CLI 进程管理、Tauri async commands），std::thread（watcher/startup rebuild）
- HTTP：reqwest（LLM Provider API 调用）
- 测试：proptest（Rust 属性测试）、fast-check + vitest（前端属性测试）

## 关键架构约束
1. **文件系统是唯一事实来源** —— SQLite 坏了删掉从文件重建
2. **碎片正文 immutable** —— 创建后永不修改正文，AI 只改 frontmatter
3. **AI Agent 层 async-first** —— 无 block_on 阻塞调用 LLM（启动初始化除外），全部走 tokio async
4. **Tauri event 格式向后兼容** —— writing_chunk、job_status_changed、index_updated 等事件的 payload schema 不可变
5. **编辑器存储格式为纯 Markdown** —— body 以 CommonMark 格式存储，YAML frontmatter 由 Rust 端管理，前端 Serializer 仅处理 body

## 模块边界
- `core/` 中 **基础模块** (repo, index, frontmatter, watcher, git, embedding, settings, jobs) 为纯 Rust，不依赖 Tauri 类型
- `core/rig_agents/` 中 **stream_adapter.rs** 依赖 `tauri::AppHandle`（用于 emit event）
- `core/cli_agents/` 中 **process_manager.rs** 依赖 `tauri::Emitter`（用于转发进程输出）
- `commands/` 为 Tauri IPC 命令薄层，负责状态注入和参数转换

## 项目结构
```
cognest/
├── src-tauri/src/
│   ├── core/
│   │   ├── repo.rs           # 文件仓库（读写碎片/文章）
│   │   ├── index.rs          # SQLite 索引（FTS5 全文搜索）
│   │   ├── frontmatter.rs    # YAML frontmatter 解析/序列化
│   │   ├── watcher.rs        # 文件变更监听（notify）
│   │   ├── git.rs            # Git 操作（git2）
│   │   ├── embedding.rs      # 本地向量化引擎（fastembed）
│   │   ├── settings.rs       # 设置管理（加密存储 + Keychain）
│   │   ├── jobs.rs           # 后台任务队列（SQLite-backed）
│   │   ├── reflection.rs     # 反思调度器（每日/每周回顾）
│   │   ├── rig_agents/       # Rig AI Agent 层
│   │   │   ├── registry.rs   # Agent 注册中心（热重载）
│   │   │   ├── router.rs     # Provider 路由（DeepSeek/OpenAI/Anthropic/Ollama）
│   │   │   ├── writing.rs    # 写作 Agent（流式输出）
│   │   │   ├── curator.rs    # 分类 Agent（tool calling + EmbeddingSearch）
│   │   │   ├── reflection.rs # 反思 Agent（后台洞察生成）
│   │   │   ├── stream_adapter.rs # Rig Stream → Tauri Event 适配
│   │   │   └── types.rs      # 共享类型（StreamChunk, ChatMessage, LlmError）
│   │   └── cli_agents/       # CLI Agent 进程管理
│   │       ├── process_manager.rs  # 检测/spawn/kill（单进程约束）
│   │       └── context.rs    # AGENTS.md 生成器
│   ├── commands/
│   │   ├── mod.rs            # AppState + 通用命令
│   │   ├── ai.rs             # AI 命令（RigState, writing_chat, writing_stream_chat）
│   │   └── cli_agents.rs     # CLI Agent 命令（detect/spawn/kill）
│   ├── lib.rs                # Tauri setup（状态注册、命令注册）
│   └── main.rs              # 入口
├── src/
│   ├── pages/                # React 页面（Discover/Compose/Capture/Articles）
│   ├── components/           # 共享组件（Editor/AgentPanel/Sidebar/ViewStack/…）
│   ├── stores/               # Zustand stores
│   ├── utils/
│   │   ├── markdownSerializer.ts  # ProseMirror → Markdown
│   │   ├── markdownParser.ts      # Markdown → ProseMirror
│   │   └── helpers.ts        # 通用工具函数
│   └── styles/tokens.css     # 设计令牌
├── src/__tests__/            # 前端属性测试 + 单元测试
├── src-tauri/tests/          # Rust 集成/属性测试
└── docs/                     # PRD/技术架构/设计参照
```

## 设计规范
- Apple 设计系统审美（克制、留白、SF Pro 字体系列）
- tokens.css 为唯一设计令牌来源，禁止重复定义 :root 变量
- 图标：1.5px stroke, 18×18, currentColor SVG，禁止 emoji
- 蓝色 accent 每屏最多 2 处
- 圆角分层：控件 8px / 卡片 12-18px / 胶囊 980px

## 数据文件格式
- 碎片：`capture/yyyy/mm/<8位hex>.md`（YAML frontmatter + Markdown 正文，正文 immutable）
- 文章：`articles/<8位hex>.md`（YAML frontmatter + Markdown 正文，body 经 prosemirror-markdown 序列化）
- 索引：`.cognest/index.sqlite`（可丢弃，从文件重建）
- 向量缓存：`.cognest/vectors.bin`（可丢弃，fastembed 重算）

## AI 架构概要
- **AgentRegistry** 管理 Writing/Curator/Reflection 三个 Rig Agent 实例
- **ProviderRouter** 支持 DeepSeek、OpenAI、Anthropic、Ollama 四种 Provider，按配置路由 + 自动回退
- **WritingAgent** — 流式输出写作辅助，注入文章上下文 + embedding 相似碎片
- **CuratorAgent** — 带 tool calling 的分类引擎，可自主调用 EmbeddingSearch 辅助判断
- **CLI Agent** — 检测本地 claude/opencode/kiro CLI，单进程 spawn，stdout/stderr 逐行转发

## Git 规范
- 提交信息格式：`type: description`
- type 枚举：init, feat, fix, refactor, docs, test, chore
- 分支策略：main 为主分支，功能开发在 feature/ 分支
- 不提交：node_modules/, target/, .cognest/, *.sqlite
