# Cognest 技术架构文档

> 版本: 1.0 | 日期: 2026-06-25 | 状态: 已确认方案，待实施

本文档是对话中技术讨论的正式整理，作为开发实施的指导文件。与 `prd.md` 互补——PRD 定义「做什么」，本文档定义「怎么做」。

---

## 1. 架构总览

### 1.1 设计原则

产品架构由四个特征倒逼：

1. **本地优先 + immutable log** → 文件是唯一事实来源，数据库是可丢弃的派生索引
2. **"AI 在中间"是后台重活** → 需要持久化的任务编排层，不是临时调 API
3. **生成式视图是核心差异化** → AI 输出受约束的 view spec，前端用固定组件渲染
4. **Mac → Windows → 移动端导入** → UI 层和 core 层解耦，但移动端只做数据导入

### 1.2 分层架构

```
┌─────────────────────────────────────────────────────────────┐
│  React UI 层                                                │
│  TipTap 编辑器 · 视图渲染器(react-flow/recharts) · 返回栈路由 │
├─────────────────────────────────────────────────────────────┤
│  Tauri IPC 层                                               │
│  薄命令层，只做序列化转发                                      │
├─────────────────────────────────────────────────────────────┤
│  Rust Core（不依赖 Tauri 类型）                               │
│  FileRepo · SQLite Index · Embedding · Job Queue · LLM GW   │
├─────────────────────────────────────────────────────────────┤
│  外部服务                                                    │
│  云端 LLM API | 可选 Ollama | MCP Server (对外暴露知识库)      │
└─────────────────────────────────────────────────────────────┘
```

**关键纪律：Rust Core 不 import 任何 Tauri 类型。** 它是纯 Rust crate，Tauri 只是宿主之一。

---

## 2. 数据层设计

### 2.1 核心决策

| 决策 | 选择 | 理由 |
|------|------|------|
| 事实来源 | **纯文本文件（`.md` + YAML frontmatter）** | git 友好、可备份、人可读、符合 immutable log |
| 数据库角色 | **SQLite = 可丢弃的本地索引** | 坏了删掉重建即可，不是事实来源 |
| 向量存储 | **不用向量数据库** | 单用户几万条规模，embedding 当 BLOB 列存 SQLite 或旁路 `.bin` |
| 同步方式 | **git 同步到 GitHub**（面向程序员/懂科技群体 + agent） | 未来可加云端同步服务 |
| 文章存储 | **文件系统（Markdown 文件）** | 天然支持 git diff、版本历史 |

### 2.2 仓库目录结构

```
cognest-vault/                     # git 仓库根目录
├── AGENTS.md                      # Agent 约定：类型枚举、工作流、权限
├── index.md                       # 内容目录：每页一行摘要（AI 维护）
├── log.md                         # append-only 时间线（AI 维护）
│
├── capture/                       # 碎片层（append-only，一条一文件）
│   └── 2026/06/
│       ├── <uuid-short>.md
│       └── <uuid-short>.md
│
├── topics/                        # AI 孵化的主题页（wiki 层，AI 维护）
│   ├── agent-architecture.md
│   └── context-engineering.md
│
├── articles/                      # 用户创作的文章（知识层）
│   ├── agent-memory.md
│   └── rag-patterns.md
│
├── views/                         # 固定的生成式视图 spec（JSON）
│   └── my-agent-dashboard.json
│
├── .meta/                         # AI 派生的关联数据（可重建，可选入 git）
│   └── relations.jsonl
│
└── .gitignore                     # 忽略本地缓存

本地缓存（.gitignore，随时可从上面重建）
└── .cognest/
    ├── index.sqlite               # FTS5 全文检索 + frontmatter 索引
    └── vectors.bin                # embedding 缓存（float32 BLOB）
```

### 2.3 碎片文件格式

```markdown
---
id: a1b2c3d4
created: 2026-06-25T10:30:00+08:00
source: manual
tags: []          # AI 后续填充，不改原文正文
topics: []        # AI 后续填充
---

Agent 其实像一个控制系统，有输入、处理、输出、反馈四个环节
```

**规则：**
- 碎片文件一旦创建，正文永不修改（immutable log）
- AI 的标签/topic 关联写到 frontmatter 的 `tags`/`topics` 字段，或写到 `.meta/relations.jsonl`
- 文件名使用 UUID 短前缀（8 位），按年月分目录

### 2.4 文章文件格式

```markdown
---
id: x9y8z7w6
title: Agent Memory 架构设计
status: draft | editing | completed
created: 2026-06-20T14:00:00+08:00
updated: 2026-06-25T16:00:00+08:00
tags: [agent, memory, architecture]
related: [[context-engineering], [rag-patterns]]
---

# Agent Memory 架构设计

正文内容...

引用碎片：@[a1b2c3d4]
```

### 2.5 Topic 页面格式（AI 维护）

```markdown
---
type: Topic
title: Agent Architecture
fragment_count: 43
maturity: growing    # seed | growing | mature
created: 2026-05-01
updated: 2026-06-25
related_to: [[context-engineering], [[rag-patterns]]
---

# Agent Architecture

## 综述
用户持续关注 Agent 架构方向，涵盖记忆系统、工具调用、反馈回路等子主题...

## 关键碎片
- @[a1b2c3d4] — Agent 像控制系统
- @[e5f6g7h8] — MCP 是跨链路由器
...

## 演化历史
- [2026-06-25] 新增 3 条碎片，与 Context Engineering 建立关联
- [2026-06-20] 主题首次被 Curator 识别
```

### 2.6 View Spec 格式（生成式视图）

```json
{
  "id": "view-uuid",
  "type": "graph",
  "title": "Agent 相关知识图谱",
  "query": "topics related to agent",
  "created": "2026-06-25T15:00:00+08:00",
  "pinned": true,
  "config": {
    "layout": "force-directed",
    "highlight": ["agent-architecture"]
  },
  "data": {
    "nodes": [...],
    "edges": [...]
  }
}
```

**允许的 type 枚举：** `graph | timeline | list | chart | board | summary`

前端固定组件映射：
| type | 渲染组件 |
|------|----------|
| graph | react-flow |
| timeline | 自有时间线组件 |
| list | 自有列表/看板组件 |
| chart | recharts |
| board | 自有看板组件 |
| summary | Markdown 渲染 |

### 2.7 索引策略

SQLite 索引（`.cognest/index.sqlite`）结构：

```sql
-- 碎片索引
CREATE TABLE fragments (
  id TEXT PRIMARY KEY,
  content TEXT,
  created_at TEXT,
  source TEXT,
  tags TEXT,          -- JSON array
  topics TEXT,        -- JSON array
  content_hash TEXT,  -- sha256，用于增量更新判断
  embedding BLOB      -- float32 向量，可为 NULL（MVP 不填）
);

-- FTS5 全文检索
CREATE VIRTUAL TABLE fragments_fts USING fts5(
  content, tags, tokenize='trigram'
);

-- 文章索引
CREATE TABLE articles (
  id TEXT PRIMARY KEY,
  title TEXT,
  status TEXT,
  created_at TEXT,
  updated_at TEXT,
  tags TEXT,
  content_hash TEXT
);

-- 关联表
CREATE TABLE relations (
  source_id TEXT,
  target_id TEXT,
  type TEXT,          -- reference | similarity | evolution
  strength REAL,
  discovered_by TEXT, -- curator | librarian | user
  created_at TEXT
);
```

**增量更新策略：** 用 `content_hash`（sha256）判断文件是否真的变了，避免 git 操作改 mtime 导致无意义重算。

### 2.8 什么进 git，什么不进

| 进 git | 不进 git（.gitignore） |
|--------|----------------------|
| `capture/**/*.md` | `.cognest/` 整个目录 |
| `articles/**/*.md` | `vectors.bin` |
| `topics/**/*.md` | `*.sqlite` |
| `views/**/*.json` | `node_modules/` |
| `.meta/relations.jsonl` | `target/` |
| `AGENTS.md` / `index.md` / `log.md` | |


---

## 3. AI Agent 设计

### 3.1 核心决策

| 决策 | 选择 | 理由 |
|------|------|------|
| Agent 框架 | **不用框架**（不用 LangChain/CrewAI/AutoGen） | 场景是单 agent 单任务、确定性触发，不需要复杂编排 |
| 实现方式 | **prompt + 受限工具 + 结构化输出** | 每个 Agent = 一个后台 Job 类型 |
| 外部 Agent 接入 | **不用 ACP 接 Claude Code/Codex** | 你的 Agent 不是 coding agent，是知识整理流水线 |
| MCP 方向 | **你暴露接口给外部 agent**（而非你调外部 agent） | 外部 agent 查你的知识库，不是替你执行 |
| Embedding | **本地计算，不走云端** | 高频、隐私敏感、成本控制 |
| LLM Generation | **云端为主，Ollama 兜底** | 低频、重质量 |

### 3.2 Agent 不是"常驻角色"，是后台任务流水线

每个 Agent 的实现模式：

```
Agent = System Prompt
      + 输入（被选中的碎片/文件子集）
      + Tool definitions（受限的写回工具）
      + 一次 LLM 调用
      + 结构化输出 → 写回文件系统
```

### 3.3 四个 Agent 的具体设计

#### Curator Agent（分类/聚类/关联发现）

```
触发：新碎片入库
输入：新碎片 + 最近相关的 10-15 条碎片（通过 FTS/embedding 检索）
工具：
  - assign_tags(fragment_id, tags[])
  - assign_topic(fragment_id, topic_id)
  - create_topic(title, summary, fragment_ids[])
  - link_topics(topic_a, topic_b, reason)
输出：更新 frontmatter tags/topics + 可能创建 topics/*.md
```

**关键：聚类用向量算法（本地免费），LLM 只负责给聚出来的簇"起名 + 写一句解释"。** 不要每条碎片都调 LLM。

#### Writing Agent（写作辅助）

```
触发：用户在创作页显式激活
输入：当前文章内容 + 相关碎片 + 相关 topic
工具：
  - suggest_outline(sections[])
  - recommend_fragments(fragment_ids[], reason)
  - expand_section(section_title, content)
输出：对话式响应 + 可选的结构化建议
```

#### Librarian Agent（知识库维护）

```
触发：每周定时 + 碎片累积超阈值
输入：全量 index.md + 变更节点的 1-2 度邻居（不是整库）
工具：
  - update_index(entries[])
  - flag_duplicate(fragment_a, fragment_b, reason)
  - update_relations(relations[])
  - update_topic_page(topic_id, new_content)
输出：更新 index.md / .meta/relations.jsonl / topics/*.md
```

**关键：不做整库 LLM 扫描（context 爆炸）。** 只扫"变更节点 + 其 1-2 度邻居"这个子图。

#### Reflection Agent（回顾/发现长期兴趣）

```
触发：每日 22:00 / 每周日 / 每月 1 号
输入：时间窗口内的碎片 + topic 变化统计
工具：
  - generate_review(period, stats, insights[])
  - highlight_growing_topic(topic_id, reason)
输出：生成 Feed 卡片（view spec JSON）
```

### 3.4 Job Queue 设计

Rust 侧一个轻量 job queue：`tokio` worker + SQLite `jobs` 表持久化。

```sql
CREATE TABLE jobs (
  id TEXT PRIMARY KEY,
  agent TEXT,           -- curator | writing | librarian | reflection
  status TEXT,          -- pending | running | completed | failed
  payload TEXT,         -- JSON：输入参数
  result TEXT,          -- JSON：输出结果
  created_at TEXT,
  started_at TEXT,
  completed_at TEXT,
  retry_count INTEGER DEFAULT 0
);
```

触发关系：

| 触发事件 | 入队 Job | 频率 |
|----------|----------|------|
| 新碎片入库 | `curator:tag_and_cluster` | 每次 |
| 用户打开创作页 | `writing:preload_context` | 按需 |
| cron 每日 22:00 | `reflection:daily` | 每日 |
| cron 每周日 | `librarian:consistency_scan` | 每周 |
| 碎片累积 > 50 条未处理 | `curator:full_recluster` | 低频 |

**持久化是重点：** App 关掉重开，没跑完的 job 从 `pending`/`running` 状态续上。

### 3.5 LLM Gateway 设计

```
LLM Gateway（Rust trait）
├── CloudProvider（Claude / GPT / DeepSeek）
│   └── 用户配置 API Key
├── OllamaProvider（本地）
│   └── HTTP 调 localhost:11434
└── 统一接口：
    - chat(messages, tools, response_format) → Result
    - embed(text) → Vec<f32>
```

**Embedding 与 Generation 分离：**
- Embedding：必须本地（`fastembed-rs` 或 `onnxruntime` 跑 `bge-small-zh`），每条碎片入库即算
- Generation：云端为主（用户配 key），可选 Ollama 兜底

### 3.6 AI 成本控制策略

从 Karpathy gist 社区的生产教训提炼：

1. **不要每条碎片都调 LLM**——聚类用向量算法（本地免费），LLM 只给结果起名
2. **不要整库加载 context**——"搜索 → 展开目录一层 → 只读需要的那一节"，比灌整篇便宜 ~15 倍
3. **矛盾检测分层**：入库时只比对 8-15 页；commit gate 用 grep 扫确定性标记；周期 lint 只扫变更子图
4. **Topic 页更新用 append**（加 History 段），不覆盖——避免"自信但过时"的记忆让 AI 变差

### 3.7 MCP Server 设计

方向：**你暴露接口给外部 agent**（Cursor / Claude / 其他）。

```
MCP Server（stdio/SSE 接口，跑在 Rust Core 之上）
├── Tools:
│   ├── search_fragments(query, limit) → Fragment[]
│   ├── get_topics(filter?) → Topic[]
│   ├── get_relations(node_id) → Relation[]
│   ├── get_article(id) → Article
│   └── get_review(period) → ReviewSummary
├── Resources:
│   ├── knowledge://index → index.md 内容
│   ├── knowledge://topics → topic 列表
│   └── knowledge://recent → 最近 N 条碎片
└── 安全:
    ├── 显式开关（用户手动启用）
    ├── 默认只读
    └── 不暴露原始全文除非用户授权
```


---

## 4. 技术选型

### 4.1 确认的技术栈

| 层面 | 选择 | 版本/备注 |
|------|------|-----------|
| 桌面框架 | **Tauri v2** | 系统 WebView，Mac 原生体验，包体小 |
| 前端框架 | **React + TypeScript** | 复杂交互，类型安全 |
| 构建工具 | **Vite** | 快速 HMR，Tauri 官方推荐 |
| 编辑器 | **TipTap / ProseMirror** | 富文本 + Markdown 双模式 |
| 图谱可视化 | **react-flow** | 知识图谱渲染 |
| 图表 | **recharts** | 趋势/统计图表 |
| 本地数据库 | **SQLite**（通过 `rusqlite`） | 可丢弃索引，FTS5 全文检索 |
| 本地 Embedding | **fastembed-rs** 或 **candle** | 跑 `bge-small-zh` / `multilingual-e5` |
| LLM 调用 | **云端 API**（Claude/GPT） + 可选 Ollama | 用户配 key |
| 文件监听 | **notify** (Rust crate) | 监听 vault 文件变化触发索引更新 |
| Git 操作 | **git2** (libgit2 Rust binding) | 内置 git 客户端 |
| 异步运行时 | **tokio** | Job queue + 并发任务 |
| 序列化 | **serde** + **serde_json** | IPC + 文件解析 |
| YAML 解析 | **serde_yaml** | frontmatter 解析 |
| 状态管理(前端) | **Zustand** | 轻量，TypeScript 友好 |
| 路由(前端) | **自有返回栈**（非 react-router） | 视图替换模型，不是页面路由 |

### 4.2 不用的技术（及理由）

| 不用 | 理由 |
|------|------|
| 向量数据库（LanceDB/Pinecone） | 单用户几万条，暴力余弦够用，不引入额外基础设施 |
| Electron | 自带 Chromium 太重，原生感差，不符合"Mac 原生体验优先" |
| CRDT / Yjs | MVP 阶段无多端协同需求，git 已覆盖同步；未来云端同步再引入 |
| LangChain / CrewAI / AutoGen | Agent 场景简单（单任务确定性触发），框架是过度杀伤 |
| ACP / 外部 coding agent | 四个 Agent 是知识整理流水线，不是代码执行 |
| Docker | 纯本地桌面 app，无服务端依赖，无需容器 |
| Node sidecar | Tauri v2 + Rust 足够，不需要额外 Node 进程 |

### 4.3 为什么选 Tauri 而不是 Electron

| 维度 | Tauri | Electron |
|------|-------|----------|
| 包体 | ~10MB | ~150MB+ |
| 内存 | 使用系统 WebView，省 | 自带 Chromium，重 |
| 启动速度 | 快（WebView 已在内存） | 慢（拉起 Chromium） |
| 原生能力 | 全局快捷键、原生菜单、通知 = 一等公民 | 需要额外配置 |
| Rust 生态 | 直接用 rusqlite/git2/fastembed | 需要 N-API binding |
| 验证 | Tolaria 已验证 Tauri + 1万笔记可行 | — |
| 代价 | 需要 Rust 基础 | 全 TS，学习成本低 |

决定因素：**"Mac 原生体验优先"** + Tolaria 已验证 = Tauri。

---

## 5. 开发环境搭建

### 5.1 Mac 环境要求

```bash
# 1. Xcode Command Line Tools（原生模块编译必需）
xcode-select --install

# 2. Rust 工具链
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
rustup default stable
rustup target add aarch64-apple-darwin  # Apple Silicon

# 3. Node.js（用 fnm 管理版本）
curl -fsSL https://fnm.vercel.app/install | bash
fnm install --lts
fnm use lts-latest

# 4. pnpm（包管理器）
npm install -g pnpm

# 5. Tauri CLI
cargo install tauri-cli

# 6. 可选：Ollama（本地 LLM 兜底）
# 从 https://ollama.ai 下载 Mac 原生 app，不用 Docker
```

### 5.2 不需要 Docker

Docker 解决的是"服务端依赖一致性 / 多服务编排"。Cognest 是纯本地桌面 app：
- SQLite 是嵌入库，不是服务
- Embedding 在进程内计算
- LLM 要么调云端 API，要么本地 Ollama 原生 app
- 没有数据库服务、没有消息队列、没有微服务

**整条链路不需要容器。**

### 5.3 项目结构

```
cognest/                           # 仓库根目录
├── src-tauri/                     # Rust 后端
│   ├── Cargo.toml
│   ├── src/
│   │   ├── main.rs               # Tauri 入口
│   │   ├── commands/             # IPC 命令（薄层，转发到 core）
│   │   └── core/                 # 纯 Rust Core（不依赖 Tauri）
│   │       ├── mod.rs
│   │       ├── repo.rs           # 文件仓库操作
│   │       ├── index.rs          # SQLite 索引
│   │       ├── embedding.rs      # 本地 embedding
│   │       ├── agents/           # 四个 Agent 实现
│   │       │   ├── curator.rs
│   │       │   ├── writing.rs
│   │       │   ├── librarian.rs
│   │       │   └── reflection.rs
│   │       ├── jobs.rs           # Job queue
│   │       ├── llm.rs            # LLM Gateway trait
│   │       ├── mcp.rs            # MCP Server
│   │       └── git.rs            # Git 操作
│   └── tauri.conf.json
├── src/                           # React 前端
│   ├── main.tsx
│   ├── App.tsx
│   ├── pages/
│   │   ├── Discover.tsx
│   │   ├── Compose.tsx
│   │   ├── Capture.tsx
│   │   └── Articles.tsx
│   ├── components/
│   │   ├── Sidebar.tsx
│   │   ├── ViewRenderer.tsx      # view spec → 组件映射
│   │   ├── Editor.tsx            # TipTap 编辑器
│   │   └── ...
│   ├── stores/                   # Zustand 状态
│   └── styles/
│       └── tokens.css            # 从 Open Design 同步来的设计令牌
├── package.json
├── vite.config.ts
├── tsconfig.json
└── README.md
```

### 5.4 Kiro IDE 衔接 Open Design 设计文件

**问题：** Open Design 里的原型（`capture.html`/`compose.html` 等）和真实代码仓库怎么桥接？

**方案：**

1. **在 `/Users/a19150/Project/Cognest` 目录 `git init`**，作为真实 app 仓库
2. **把 Open Design 的 `assets/tokens.css` 作为设计↔代码的契约文件**：
   - 拷贝到仓库的 `src/styles/tokens.css`
   - 或建软链保持同步：设计侧调色/改间距，代码侧自动跟着变
3. **在 Open Design 中将链接代码目录指向 `/Users/a19150/Project/Cognest`**
4. **用 Kiro 打开 `/Users/a19150/Project/Cognest` 来写代码**
5. **HTML 原型作为视觉参照**：逐个对照 `capture.html`/`compose.html`/`discover.html` 重建为 React 组件

```
Open Design（设计侧）          Kiro IDE（代码侧）
├── capture.html    ──参照──→   src/pages/Capture.tsx
├── compose.html    ──参照──→   src/pages/Compose.tsx
├── discover.html   ──参照──→   src/pages/Discover.tsx
├── assets/tokens.css ══同步══→ src/styles/tokens.css
└── 链接代码目录 ──────────────→ /Users/a19150/Project/Cognest
```


---

## 6. 开发路径（按可验证的里程碑切）

### Phase 0 · 技术验证 Spike（1-2 周）

**目标：** 证伪选型，跑通最小链路。

```
验证项：
├── Tauri v2 + React 骨架能跑起来
├── rusqlite + FTS5 全文检索能用
├── 从 capture/ 目录读 .md 文件 → 解析 frontmatter → 写入 SQLite 索引
├── 一次 LLM API 调用（Claude/GPT）能返回结构化 JSON
└── 可选：fastembed-rs 跑一个 embedding 模型成功
```

**不做：** UI 设计、完整功能、美化。只做技术验证。

### Phase 1 · 单机 MVP（4-6 周）

**目标：** 自己能天天用。发现 feed 先用规则触发，不上 AI。

```
功能：
├── 碎片 CRUD（append-only，文件写入 capture/）
├── 全局快捷键弹出捕捉框（Tauri 全局快捷键）
├── SQLite 索引 + FTS5 全文检索
├── 文章创作（TipTap 编辑器 + Markdown 导出）
├── @引用碎片（编辑器内芯片）
├── 侧边栏导航（发现/创作/碎片/文章）
├── 发现 feed（规则触发卡片：最近碎片统计、高频标签）
├── 文件监听（notify）→ 增量更新索引
└── 内置 git（commit/push 到 GitHub）
```

**关键：** MVP 完全不上 LLM/embedding，先验证 IA 和核心体验。AI 是增量替换规则的，不是前置依赖。

### Phase 2 · AI 能力（4-6 周）

**目标：** AI 开始工作。

```
功能：
├── 本地 Embedding（fastembed-rs + bge-small-zh）
├── Curator Agent：向量聚类 + LLM 命名
├── Writing Agent：创作页 AI 对话面板
├── 生成式视图（view spec + 固定组件渲染）
├── Job Queue 持久化（tokio + SQLite jobs 表）
├── LLM Gateway（云端 API + Ollama 兜底）
└── Reflection Agent：每日/周回顾 → Feed 卡片
```

### Phase 3 · MCP Server + 插件（2-3 周）

**目标：** 对外暴露知识库。

```
功能：
├── MCP Server（stdio 接口，暴露 search/get_topics/get_relations）
├── 安全：显式开关 + 默认只读 + 权限控制
├── AGENTS.md 自动生成（告诉外部 agent 你的约定）
├── 碎片导入插件框架（adapter 模式）
└── flomo 导入插件（Markdown 导出 → capture/*.md）
```

### Phase 4 · Windows + 优化（2-4 周）

**目标：** 跨桌面平台。

```
功能：
├── Windows 构建（Tauri 换 build target，基本免费）
├── 性能优化（大量碎片下的索引/检索速度）
├── Librarian Agent：全量一致性扫描 + 知识图谱维护
└── 用户设置面板（AI 模型配置、API Key、快捷键）
```

### Phase 5 · 云端同步 + 移动端导入（未来）

**目标：** 多终端。

```
功能：
├── 云端同步服务（打通 PC/Pad/Phone）
├── 移动端数据导入插件：
│   ├── Apple 备忘录 → 快捷指令导出 → capture/*.md
│   ├── flomo API/导出 → capture/*.md
│   └── 其他备忘录 adapter
└── 端到端加密
```

---

## 7. 参考架构来源

| 来源 | 拿走什么 | 落在哪一层 |
|------|----------|-----------|
| **Tolaria** | Tauri 架构、原生 git、内置 MCP、AGENTS.md、vault 级 agent 权限 | 基础设施层 |
| **Karpathy LLM Wiki** | "编译而非检索"、index.md+log.md、三层结构、不要向量库 | AI 孵化/知识演化层 |
| **Portent** | 最小类型词汇 + 两种关系 + Capture→Organize→Archive 生命周期 | frontmatter schema |
| **flomo** | 低摩擦捕捉、卡片流、每日回顾 | capture 层 + Reflection Agent |
| **Obsidian** | 双链 = 图的边、graph view、文件即库 | topics/articles 关系与可视化 |

### 7.1 Karpathy LLM Wiki 模型在本产品中的映射

```
Karpathy 模型           →  Cognest 映射
─────────────────────────────────────────
Raw（外部文档）         →  capture/（用户碎片，但 immutable）
Wiki Page（LLM 维护）   →  topics/（AI 孵化的主题页）
index.md（目录）        →  index.md（AI 维护的内容目录）
log.md（操作日志）      →  log.md（append-only 时间线）
query 操作              →  写空间（创作页 + AI 对话）
ingest 操作             →  碎片入库 + Curator Agent
lint 操作               →  Librarian Agent 一致性扫描
```

### 7.2 与 Tolaria 的差异化（你做不同的地方）

| Tolaria | Cognest |
|---------|---------|
| 键盘流 power user | 低摩擦，flomo 式捕捉 |
| 用户手动组织 | AI 自动组织，人不分类不打标签 |
| 固定视图 | 生成式视图（AI 按需生成看板） |
| 无发现 feed | 发现页 = AI 主动推送工作成果 |
| 8 种 Portent 类型全实现 | 最小类型起步（碎片/文章/topic） |

---

## 8. 关键风险与缓解

| 风险 | 缓解策略 |
|------|----------|
| Tauri 首次打开慢（Tolaria 的问题） | 渐进式启动：先渲染壳 + 最近 50 条，后台异步全量扫描 |
| Rust 学习成本 | Core 层薄（文件 IO + git + SQLite），可参考 Tolaria 源码 |
| LLM 账单失控 | 聚类用本地向量算法免费；LLM 只做命名/综述；context 加载分层 |
| AI 生成 taxonomy drift | type 字段约束为枚举，写进 AGENTS.md；定期 Librarian 清理 |
| 生成式视图表达力不足 | view spec 的 type 枚举可逐步扩展；MVP 先 5 种够用 |
| git 同步冲突 | 碎片 append-only = 几乎无冲突；文章级冲突靠 git merge 工具 |

---

## 9. 首次启动优化策略

针对 Tolaria 暴露的首次加载慢问题，Cognest 的防护设计：

1. **渐进式启动**：先渲染 App Shell + 从 SQLite 读最近 50 条（亚毫秒），用户第一帧有内容
2. **增量索引**：维护 `last_scan_sha`，只扫 git diff 里变动的文件，不全量遍历
3. **后台异步**：全量扫描/embedding 补算在 tokio 后台线程，不阻塞 UI
4. **首次安装/大型导入**：给进度条（"扫描了 xxx/10000 条"），不假装不慢
5. **内容哈希判断变化**：用 sha256 而非 mtime，避免 git 操作触发无意义重算

---

*本文档与 `prd.md` 互补：PRD 定义产品形态，本文档指导技术实施。随开发推进持续更新。*
