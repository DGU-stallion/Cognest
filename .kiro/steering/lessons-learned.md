---
inclusion: auto
---

# Cognest 开发经验教训（AI 必读）

本文件记录开发过程中踩过的坑和解决方案，避免重复犯错。

## 1. Tauri v2 没有 tokio runtime（关键）

**问题：** Tauri 的 `setup()` 闭包运行在主线程，不在 tokio runtime 内。在 setup 或 IPC 命令中调用 `tokio::spawn`、`tokio::time::sleep`、tokio channel 会 panic：`there is no reactor running`。

**解决：** 后台任务用 `std::thread::spawn` + `std::sync::mpsc` + `recv_timeout` 实现，不依赖 tokio runtime。File watcher 和 progressive startup 都改用 std 线程。

**规则：** Rust Core 的后台任务一律用 `std::thread`，除非显式创建了 tokio runtime。

## 2. IPC 数据契约必须前后端严格一致（导致白屏的根因）

**问题：** 前端 `Article` 类型期望 `content`、`word_count` 字段，status 值为 `"done"`；但后端 `list_articles` 返回的 `ArticleRecord`（来自 SQLite 索引）**没有** `content`/`word_count`，且 status 值是 `"completed"`。前端直接访问 `article.content.replace()`、`count.toLocaleString()` → `undefined is not an object` → 整个 WebView 白屏。

**解决：** 在 store 的 load 函数里做**数据规范化层**——把后端返回的原始结构映射成前端完整类型，所有可能缺失的字段补默认值（`?? ''`、`?? 0`、`?? []`），枚举值做映射（`completed` → `done`）。

**规则：**
- 定义 IPC 返回的 `RawXxxRecord` 类型（字段全部可选），在 store 里映射成前端完整类型
- 任何 `.replace()`、`.toLocaleString()`、`.toLowerCase()`、`.split()` 调用前，确保对象非 undefined
- 后端 `ArticleRecord`（索引）≠ `ArticleResponse`（完整读取）。列表用前者（无正文），详情用后者（有正文），前端要区分对待

## 3. 浏览器能跑 ≠ Tauri WebView 能跑

**现象：** `localhost:5173` 浏览器正常，但 Tauri 桌面端白屏。

**原因：** 浏览器里 `invoke()` 因无 Tauri runtime 直接 reject，被 try/catch 吞掉，组件不会拿到真实数据，崩溃路径没被触发。Tauri WebView 里 `invoke()` 真的返回数据，触发完整渲染，才暴露 undefined 字段崩溃。

**规则：** 调试 Tauri 应用必须在桌面端验证，不能只看浏览器。白屏时第一步开 DevTools（tauri.conf.json 里 `"devtools": true`，或 ⌘⌥I）看 Console 红色报错。

## 4. 写文件必须同步更新 SQLite 索引

**问题：** `create_article`/`save_article` 只写 `.md` 文件，依赖 File Watcher 异步更新索引。但 watcher 有延迟，导致刚创建的文章在文章页（从索引读）看不到。

**解决：** IPC 命令写文件后**立即同步** `INSERT OR REPLACE` 到 IndexDb，不等 watcher。watcher 作为兜底（处理外部编辑）。

**规则：** 任何修改 vault 文件的命令，写完文件后同步更新索引，保证 UI 立即可见。

## 5. 命令行执行注意事项

- `cargo tauri dev`、`pnpm dev` 是长驻进程，不要用阻塞式命令跑（会卡住）。让用户手动运行，或用后台进程工具。
- cargo 命令前先 `source "$HOME/.cargo/env"`。
- 验证编译用 `cargo build`（Rust）和 `npx tsc --noEmit`（前端类型检查，快），不要跑长时间的完整 build。

## 6. 上下文压缩（compaction）

对话很长时上下文会被自动压缩，压缩后需重新读取 tasks.md 等确认当前进度，不要依赖记忆。
