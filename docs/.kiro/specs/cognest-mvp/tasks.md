# Implementation Plan: Cognest MVP

## Overview

本实施计划覆盖 Cognest MVP（Phase 0 技术验证 + Phase 1 单机 MVP）的完整开发任务，分 4 个 wave 递进执行：基础设施 → 核心模块 → 前端页面 → 集成优化。

## Task Dependency Graph

```json
{
  "waves": [
    {
      "name": "Wave 1: 基础设施",
      "tasks": [1, 2, 3, 4]
    },
    {
      "name": "Wave 2: 核心模块",
      "tasks": [5, 6, 7, 8]
    },
    {
      "name": "Wave 3: 功能页面",
      "tasks": [9, 10, 11, 12, 13, 14]
    },
    {
      "name": "Wave 4: 集成与测试",
      "tasks": [15, 16, 17, 18, 19, 20]
    }
  ]
}
```

## Tasks

- [ ] 1. 项目脚手架初始化：使用 `cargo tauri init` 在项目根目录初始化 Tauri v2 + React + TypeScript + Vite 项目。配置 Cargo.toml 添加 rusqlite、serde、serde_yaml、notify、git2、tokio、chrono、sha2、uuid、thiserror 依赖。从 docs/design-ref/tokens.css 拷贝到 src/styles/tokens.css。验证 `cargo tauri dev` 能启动并显示窗口，验证 IPC 双向通信。 [Requirements: 1, 18]
- [ ] 2. Frontmatter 解析器：实现 src-tauri/src/core/frontmatter.rs 模块。包含 `parse<T>()` 和 `serialize<T>()` 泛型函数，支持 YAML Frontmatter 解析和序列化。实现 FrontmatterError 枚举（MissingDelimiter/YamlParseError/SerializeError）。编写单元测试验证 round-trip 属性和错误处理。 [Requirements: 4]
- [ ] 3. FileRepo 模块：实现 src-tauri/src/core/repo.rs。FileRepo 结构体提供 create_fragment()、read_fragment()、list_fragment_paths()、create_article()、read_article()、save_article()、delete_article()、export_article()、content_hash() 方法。碎片路径 capture/yyyy/mm/<8位hex>.md，文章路径 articles/<8位hex>.md。编写测试验证文件格式和 immutable log。 [Requirements: 3, 10]
- [ ] 4. 前端基础设施：搭建 src/pages/、src/components/、src/stores/、src/styles/ 目录。引入 tokens.css 全局样式，配置 CSS 变量引用。创建 Zustand appStore（currentPage、sidebarExpanded、counts）。实现 App.tsx 基础布局骨架（侧边栏 + 主区域 + 状态栏占位）。确保不重复定义 :root 变量。 [Requirements: 18, 6]
- [ ] 5. SQLite 索引模块：实现 src-tauri/src/core/index.rs。IndexDb 结构体提供 open()、init_schema()、check_integrity()、碎片和文章 CRUD、search_fragments()、search_articles()（FTS5 + trigram）、stats_last_days()、top_tags()、fragment_count()、article_count()、rebuild_from_vault() 方法。创建 fragments + fragments_fts + articles + articles_fts 表。 [Requirements: 2, 9]
- [ ] 6. IPC 命令层：实现 src-tauri/src/commands/ 目录。定义 AppState 管理 FileRepo + IndexDb 实例。注册 Tauri 命令：create_fragment、list_fragments、search_fragments、create_article、get_article、save_article、delete_article、export_article、search_articles、list_articles、git_sync、git_status、get_stats、get_top_tags、get_counts、get_vault_path。每个命令只做序列化转发到 Core。 [Requirements: 1]
- [ ] 7. 侧边栏与导航：实现 src/components/Sidebar.tsx。展开态 248px / 折叠态 60px，4 导航项（发现/创作/碎片/文章）+ 品牌标识 + 快速记录按钮。选中态白底 + accent 图标。条目计数（>999 显示 "999+"）。⌘\ 切换。折叠状态 localStorage 持久化。全部图标 1.5px stroke 18×18 SVG。 [Requirements: 6]
- [ ] 8. 视图栈组件：实现 src/components/ViewStack.tsx 和 viewStackStore.ts。每个功能页独立栈（最大深度 10）。前进动画（右侧滑入 220ms var(--ease-standard)）、后退动画（右侧滑出）。栈深 > 0 显示 "← 返回" 按钮。Esc 后退。侧边栏切换保留原页面栈。 [Requirements: 7]
- [ ] 9. 碎片页：实现 src/pages/Capture.tsx 和 captureStore.ts。顶部输入框 + "记录" 按钮（Enter 提交，空白拒绝）。碎片按日期分组（"今天 · 6月24日" 格式），组内时间倒序。每条显示正文（3 行截断）、时间 HH:mm、标签。筛选栏（全部/未整理/已归类）+ "共 N 条"。Hover 操作按钮。参照 docs/design-ref/capture.html。 [Requirements: 8]
- [ ] 10. 快速记录弹窗：实现 src/components/QuickCaptureModal.tsx。⌘⇧Space 触发居中弹窗（模糊遮罩）。100ms 聚焦 textarea。⌘↵ 保存非空内容 + toast。Esc/点击遮罩关闭。空白不保存。失败时保持打开 + 错误提示。不影响底层页面。 [Requirements: 5]
- [ ] 11. TipTap 编辑器：实现 src/components/Editor.tsx。集成 TipTap StarterKit（H1-H3、加粗、斜体、行内代码、代码块、引用、有序/无序列表）。自定义 ReferenceChip Node（@[fragment-id] 语法、accent 背景不可编辑节点、序列化/反序列化、失效状态处理）。AutoSave 扩展（1s debounce 后保存）。字数统计。 [Requirements: 10, 11]
- [ ] 12. 创作页：实现 src/pages/Compose.tsx 和 composeStore.ts。三栏：左侧 280px 素材面板（灵感/话题/文章 tab，默认灵感）+ 中间编辑器 + 右侧 320px AI 占位。标题输入 + 元信息栏。状态切换 draft→editing→completed。沉浸模式 ⌘⇧F（max-width 720px）。左侧碎片点击插入引用。空状态提示。参照 docs/design-ref/compose.html。 [Requirements: 19, 10, 11]
- [ ] 13. 文章页：实现 src/pages/Articles.tsx 和 articlesStore.ts。左侧列表（编辑时间倒序，标题/摘要50字/状态/字数/日期）+ 右侧 380px 预览面板。状态筛选 + 标签多选交集筛选。预览面板：标题/状态/字数/标签/Markdown 渲染内容 + 编辑/导出/删除按钮。删除确认。参照 docs/design-ref/articles.html。 [Requirements: 12]
- [ ] 14. 发现页：实现 src/pages/Discover.tsx 和 discoverStore.ts。默认首页。规则卡片：最近碎片统计（7天碎片数+标签数）、高频标签 Top 5、活跃度（>20条）。优先级排序。"×" 关闭（会话内）。"查看详情" 进入 ViewStack 详情视图。空状态引导。参照 docs/design-ref/discover.html。 [Requirements: 13]
- [ ] 15. 文件监听模块：实现 src-tauri/src/core/watcher.rs。notify crate 递归监听 capture/ 和 articles/ 下 .md 文件。500ms 去抖合并。Create→解析+插入索引，Modify→hash 比较+更新，Delete→移除记录。解析失败跳过+日志。通过 Tauri emit 通知前端。 [Requirements: 14]
- [ ] 16. Git 模块：实现 src-tauri/src/core/git.rs。git2 crate。open()、sync_status()、sync()、ensure_gitignore()。sync: add → commit("sync: N files changed · YYYY-MM-DD HH:mm") → push(30s 超时)。状态: Synced/Unsynced(file_count)/NoRemote。空变更不 commit。.gitignore 排除 .cognest/、*.sqlite、vectors.bin。 [Requirements: 15]
- [ ] 17. 状态栏：实现 src/components/StatusBar.tsx。固定底部 28px。左侧后台任务（索引更新中/同步中）。右侧知识库条目数("知识库 N 条"实时更新) + Git 状态 + 设置按钮。Zustand 订阅。Tauri 事件监听 index_updated/sync_status 更新。 [Requirements: 16]
- [ ] 18. 设置面板：实现 src/components/SettingsModal.tsx。macOS 偏好设置风格 modal。左侧 tab（账户/快捷键/知识库/插件），默认账户。知识库 tab: Vault 路径 + 碎片/文章总数（IPC 获取）。快捷键 tab: 只读列表。插件 tab: 空状态。Esc/遮罩关闭。 [Requirements: 17]
- [ ] 19. 渐进式启动：优化启动流程。先渲染 shell → 检查 IndexDB → 有效则读最近 50 条填充首屏 → 无效或不存在则显示进度指示器 + tokio 后台全量构建 → 完成后 emit 事件刷新视图。确保 UI 响应 <100ms。 [Requirements: 20, 2]
- [ ] 20. 属性测试：Rust proptest（100+ 迭代）覆盖 Property 1-6, 9。前端 fast-check 覆盖 Property 7-8, 10-14。验证 Frontmatter round-trip、碎片格式、正文不变量、索引计数、hash 检测、空白拒绝、搜索正确性、分组排序、筛选语义、引用 round-trip、标签交集、ViewStack 深度、计数格式。 [Requirements: 设计文档 Correctness Properties]

## Notes

- MVP 阶段不包含任何 AI/LLM 功能，发现页使用纯规则触发
- 所有设计参照 docs/design-ref/ 下的 HTML 原型
- tokens.css 为设计与代码的唯一契约文件
- Rust Core 层绝不依赖 Tauri 类型，保持纯 crate 独立性
