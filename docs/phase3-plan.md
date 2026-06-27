# Cognest Phase 3 规划

## 当前已知问题

### P0 — 阻塞核心功能
1. **Writing Agent 超时** — DeepSeek API 调用在 60s 内超时，怀疑是 `spawn_blocking` + 独立 tokio runtime 的 reqwest client 在 Tauri 环境下的兼容性问题。需要彻底重构为纯 async IPC command（去掉 spawn_blocking + agent 层的同步调用链）
2. **Markdown 编辑器/预览不完善** — TipTap 存储 HTML，预览用 react-markdown 渲染 Markdown，两者格式不匹配。删除 H1 后预览异常。

### P1 — 体验问题
3. **文章预览不显示标签** — 索引中 article tags 为空（需要重建索引或修复入库逻辑）
4. **热力图数据是随机的** — 需要对接真实写作频率数据

### P2 — 功能缺失
5. **ACP / MCP 接入** — 将 Cognest 暴露为 MCP Server 或 ACP Agent，让 Claude Code / OpenCode / Kiro CLI 可以调用
6. **Topic 页面** — 创作页"话题"tab 显示占位文字，需要实现 Topic 列表和关联

---

## Phase 3 任务规划

### 3.1 Markdown 编辑器升级（参考 Tolaria）
- 替换 TipTap 为支持原生 Markdown 的编辑器方案
- 选项 A：TipTap + prosemirror-markdown（序列化/反序列化为 MD 而非 HTML）
- 选项 B：使用 Milkdown（基于 ProseMirror 的 Markdown-first 编辑器）
- 选项 C：参考 Tolaria 的 CodeMirror + Markdown 预览分屏方案
- 目标：存储格式统一为 YAML frontmatter + Markdown body，编辑器原生读写 Markdown

### 3.2 Writing Agent 修复
- 将 `writing_chat` / `writing_stream_chat` 重构为纯 async 实现（类似 validate_provider 的直接 reqwest 调用）
- 去掉 WritingAgent → LlmGateway → Provider 的多层锁链路
- 或者：将 LlmProvider trait 改为 async trait（需要 `async-trait` crate）

### 3.3 ACP / MCP 接入
- 实现 `cognest-mcp` CLI binary：独立编译，链接 Core crate
- 暴露 tools：`search_fragments`、`create_fragment`、`find_similar`、`list_articles`、`get_article`
- 配置方式：在 Claude Code / Kiro 的 `mcp.json` 中添加
- 后续：ACP HTTP Server（localhost:9820）支持 Agent 发现和双向通信

### 3.4 索引与数据一致性
- 修复文章索引 tags 不同步问题
- App 启动时智能判断是否需要重建索引（file hash 对比）
- 热力图对接真实数据（从 fragments created_at 统计每日碎片数）

### 3.5 Topic 系统 UI
- 创作页"话题"tab：展示当前文章关联的 Topics + 推荐 Topics
- 发现页：Topic 关系图（使用已有的 ViewRenderer graph 组件）

---

## 优先级排序

| 优先级 | 任务 | 预估工时 |
|--------|------|----------|
| 1 | 3.2 Writing Agent 修复 | 1 天 |
| 2 | 3.1 Markdown 编辑器升级 | 3-4 天 |
| 3 | 3.3 ACP/MCP 接入 | 2-3 天 |
| 4 | 3.4 索引一致性 | 1 天 |
| 5 | 3.5 Topic UI | 1-2 天 |

---

## 技术决策待定

- Markdown 编辑器选型（TipTap+prosemirror-markdown vs Milkdown vs CodeMirror）
- MCP Server 运行方式（Tauri sidecar vs 独立 binary vs 内嵌 HTTP server）
- 是否引入 async-trait 重构 LlmProvider trait
