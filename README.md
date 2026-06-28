# 🧠 Cognest

Cognest 是一个 AI 认知工作台桌面应用，帮助用户将碎片化灵感演化为系统化认知。

**核心理念：人在两端，AI 在中间** — 用户负责输入（碎片）和输出（文章），中间的整理、发现、归档、关联全部交给 AI。

<img width="1000" alt="Cognest Preview" src="src/assets/hero.png" />

## 设计原则

- 📝 **人只管 I/O** — 用户不分类、不打标签、不建文件夹。记录碎片和创作文章，其余交给 AI。
- 📁 **文件系统为真** — 所有数据是 Markdown + YAML frontmatter 文件。SQLite 索引可丢弃重建，文件永远是唯一事实来源。
- 🔌 **Git 原生** — 每个 vault 是 git 仓库，版本历史完整，零依赖云服务。
- 🧠 **AI 是管家不是代笔** — AI 整理、推荐、发现关联，不替你写结论。
- 🔍 **视图是生成式的** — 知识图谱、时间线、列表不是固定页面，而是 AI 按自然语言描述按需生成。
- 📦 **碎片不可变** — 每条灵感永久保存，即使被合并/总结，原始记录不可修改。
- 🔒 **离线优先，零锁定** — 无账号、无订阅、无云依赖。数据永远在本地。

## 功能

| 页面 | 用途 | 特色 |
|------|------|------|
| **发现** | 浏览 AI 的工作成果 | 自然语言生成知识图谱/时间线/列表/图表视图 |
| **创作** | 写作（文章输出） | TipTap 富文本编辑器 + AI 写作助手 + 沉浸模式 |
| **碎片** | 快速记录灵感 | ⌘⇧Space 全局呼出，AI 自动标签 |
| **文章** | 管理已完成作品 | 搜索/筛选/预览/导出 |

## 技术栈

- **桌面框架**: Tauri v2（系统 WebView）
- **前端**: React 19 + TypeScript + Vite
- **状态管理**: Zustand
- **编辑器**: TipTap / ProseMirror
- **后端**: Rust（纯 core crate，Tauri 只是宿主）
- **数据库**: SQLite + FTS5（可丢弃派生索引）
- **AI**: Rig 框架（支持 DeepSeek / OpenAI / Ollama）
- **Embedding**: 本地 ONNX 模型（离线向量化）

## 快速开始

### 前置条件

- Node.js 20+
- pnpm 9+
- Rust stable（rustup）
- macOS（开发主力环境）

### 安装与运行

```bash
# 安装前端依赖
pnpm install

# 启动开发模式（前端 + Tauri）
cargo tauri dev

# 仅前端开发（无 Rust 后端）
pnpm dev
```

### 构建

```bash
# 生产构建
cargo tauri build
```

## 项目结构

```
cognest/
├── src/                    # React 前端
│   ├── pages/              # 页面（Discover/Compose/Capture/Articles）
│   ├── components/         # 共享组件（Editor/Sidebar/ViewStack/WritingPanel）
│   ├── stores/             # Zustand 状态管理
│   ├── styles/tokens.css   # 设计令牌
│   └── utils/              # Markdown 解析/序列化
├── src-tauri/              # Rust 后端
│   └── src/
│       ├── core/           # 核心逻辑（repo/index/settings/embedding/rig_agents）
│       └── commands/       # Tauri IPC 命令层
├── docs/                   # PRD/技术架构/设计参照
└── public/                 # 静态资源
```

## 数据格式

碎片和文章都是 Markdown 文件，带 YAML frontmatter：

```markdown
---
id: a1b2c3d4
created: 2026-06-28T12:00:00Z
source: manual
tags: [认知科学, 碎片化]
topics: []
---

今天读到一个有趣的观点：人类的记忆并非线性存储…
```

默认 vault 路径: `~/CognestVault`

## 键盘快捷键

| 快捷键 | 功能 |
|--------|------|
| `⌘⇧Space` | 快速记录碎片 |
| `⌘N` | 新建文章 |
| `⌘⇧F` | 沉浸式写作模式 |
| `⌘\` | 切换侧边栏 |

## 测试

```bash
# 前端单元测试
pnpm test

# Rust 单元测试
cd src-tauri && cargo test
```

## 技术文档

- 📐 [技术架构](docs/tech-architecture.md) — 系统设计与模块划分
- 📋 [产品需求文档](docs/prd.md) — 完整 PRD（v4.0）
- 🎨 [设计参照](docs/design-ref/) — HTML 原型 + 设计令牌

## 许可证

MIT
