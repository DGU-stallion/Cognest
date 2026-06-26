---
inclusion: auto
---

# Cognest 项目上下文

## 项目概述
Cognest 是一个 AI 认知工作台桌面应用，帮助用户将碎片化灵感演化为系统化认知。
组织原则：「人在两端，AI 在中间」—— 用户负责输入（碎片）和输出（文章），中间整理全交给 AI。

## 技术栈
- 桌面框架：Tauri v2（系统 WebView）
- 前端：React + TypeScript + Vite
- 状态管理：Zustand
- 编辑器：TipTap / ProseMirror
- 后端：Rust Core（纯 Rust crate，不依赖 Tauri 类型）
- 数据库：SQLite + FTS5（可丢弃派生索引）
- 文件监听：notify crate
- Git：git2 crate
- 异步：std::thread（watcher/startup 后台任务），tokio 仅作为 Cargo 依赖但不在 runtime 中使用

## 关键架构约束
1. **Rust Core 不 import 任何 Tauri 类型** —— 纯 Rust crate，Tauri 只是宿主
2. **文件系统是唯一事实来源** —— SQLite 坏了删掉从文件重建
3. **碎片正文 immutable** —— 创建后永不修改正文，AI 只改 frontmatter
4. **MVP 不依赖 AI/LLM** —— 发现页用规则触发，AI 是 Phase 2

## 项目结构
```
cognest/
├── src-tauri/src/core/    # Rust Core（repo/index/frontmatter/watcher/git）
├── src-tauri/src/commands/ # Tauri IPC 命令（薄层转发）
├── src/pages/             # React 页面（Discover/Compose/Capture/Articles）
├── src/components/        # 共享组件（Sidebar/ViewStack/Editor/StatusBar/Modals）
├── src/stores/            # Zustand stores
├── src/styles/tokens.css  # 设计令牌（从 docs/design-ref/ 同步）
└── docs/                  # PRD/技术架构/设计参照/spec
```

## 设计规范
- Apple 设计系统审美（克制、留白、SF Pro 字体系列）
- tokens.css 为唯一设计令牌来源，禁止重复定义 :root 变量
- 图标：1.5px stroke, 18×18, currentColor SVG，禁止 emoji
- 蓝色 accent 每屏最多 2 处
- 圆角分层：控件 8px / 卡片 12-18px / 胶囊 980px

## 数据文件格式
- 碎片：`capture/yyyy/mm/<8位hex>.md`（YAML frontmatter + 正文）
- 文章：`articles/<8位hex>.md`（YAML frontmatter + Markdown 正文）
- 索引：`.cognest/index.sqlite`（可丢弃）

## Git 规范
- 提交信息格式：`type: description`
- type 枚举：init, feat, fix, refactor, docs, test, chore
- 分支策略：main 为主分支，功能开发在 feature/ 分支
- 不提交：node_modules/, target/, .cognest/, *.sqlite
