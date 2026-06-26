# Requirements Document

## Introduction

本文档定义 Cognest AI 认知工作台的首个可交付里程碑——技术验证（Phase 0）与单机 MVP（Phase 1）的合并范围。目标是产出一个**无 AI/LLM 依赖**、可日常使用的桌面应用，验证核心信息架构和用户体验。AI 能力将在后续 Phase 2 中增量替换规则逻辑引入。

## Glossary

- **App_Shell**: Tauri v2 + React + Vite 构成的桌面应用骨架，包含窗口管理、IPC 通信和前端渲染
- **Fragment（碎片）**: 用户输入的一条原子化灵感记录，存储为 `capture/yyyy/mm/<uuid-short>.md` 文件，包含 YAML frontmatter 元数据
- **Article（文章）**: 用户创作的长文内容，存储为 `articles/*.md` 文件，支持草稿/已完成状态
- **Vault（仓库）**: Cognest 数据根目录，包含 capture/、articles/、.cognest/ 等子目录的文件系统结构
- **Index_DB**: 位于 `.cognest/index.sqlite` 的 SQLite 数据库，作为文件系统的可丢弃派生索引
- **FTS5**: SQLite 全文检索扩展，用于碎片和文章内容的全文搜索
- **Frontmatter**: Markdown 文件开头的 YAML 元数据块，包含 id、created、source、tags 等字段
- **Sidebar（侧边栏）**: 应用左侧固定导航区域，展开态 248px、收起态 60px
- **View_Stack（视图栈）**: 自有的视图替换 + 返回栈导航模型，替代传统页面路由
- **Quick_Capture_Modal（快速记录弹窗）**: 通过全局快捷键或按钮触发的轻量碎片输入弹窗
- **File_Watcher**: 基于 Rust notify crate 的文件系统监听器，检测 Vault 目录变更并触发索引更新
- **Discover_Feed（发现页）**: 应用默认首页，显示规则触发的统计卡片流
- **TipTap_Editor**: 基于 ProseMirror 的富文本编辑器，支持 Markdown 编辑和碎片引用芯片
- **Reference_Chip（引用芯片）**: 编辑器内的内联组件，表示对某条碎片的 @引用
- **Git_Module**: 基于 git2 Rust 绑定的内置 Git 操作模块
- **Status_Bar（状态栏）**: 窗口底部 28px 信息条，显示后台任务和知识库状态
- **Content_Hash**: 文件内容的 SHA-256 哈希值，用于增量索引更新时判断文件是否真正变化

## Requirements

### 需求 1：应用骨架启动

**用户故事：** 作为开发者，我希望 Tauri v2 + React + Vite 应用骨架能正常编译运行，以便验证桌面框架选型的可行性。

#### 验收标准

1. THE App_Shell SHALL 在 macOS 上通过 `cargo tauri build` 以退出码 0 完成编译，并启动一个包含系统 WebView 的原生窗口
2. THE App_Shell SHALL 通过 Tauri IPC 机制实现前端 React 层与 Rust 后端之间的双向通信：前端调用 Rust 命令并接收返回值（front-to-back），Rust 通过事件机制向前端发送消息且前端成功接收（back-to-front）
3. WHILE App_Shell 处于开发模式（`cargo tauri dev`）时，THE App_Shell SHALL 使用 Vite 作为前端构建工具，在前端源文件保存后 2 秒内将变更反映到运行中的应用界面，且无需整页刷新
4. WHEN App_Shell 启动完成（原生窗口可见）时，THE App_Shell SHALL 在 200ms 内渲染出应用外壳界面，包含可见的侧边栏区域与主内容区域的两栏布局

### 需求 2：数据层与索引

**用户故事：** 作为用户，我希望应用能从文件系统读取碎片数据并建立可搜索的索引，以便快速查找历史记录。

#### 验收标准

1. THE Index_DB SHALL 使用 rusqlite 创建 SQLite 数据库文件于 `.cognest/index.sqlite` 路径
2. THE Index_DB SHALL 创建 fragments 表，包含 id (TEXT PRIMARY KEY)、content (TEXT)、created_at (TEXT)、source (TEXT)、tags (TEXT, JSON array)、topics (TEXT, JSON array)、content_hash (TEXT, sha256 hex string) 字段
3. THE Index_DB SHALL 创建 FTS5 虚拟表 fragments_fts，对 content 和 tags 字段启用全文检索，使用 trigram 分词器
4. THE Index_DB SHALL 创建 articles 表，包含 id (TEXT PRIMARY KEY)、title (TEXT)、status (TEXT)、created_at (TEXT)、updated_at (TEXT)、tags (TEXT, JSON array)、content_hash (TEXT, sha256 hex string) 字段
5. IF Index_DB 文件不存在，WHEN 应用启动时，THE Index_DB SHALL 遍历 Vault 中 capture/ 目录下所有 .md 文件，解析 YAML Frontmatter 和正文，写入 fragments 表和 fragments_fts 表，完成后 fragments 表行数等于 capture/ 目录下有效 .md 文件总数
6. IF Index_DB 文件无法通过 SQLite integrity_check 或文件缺失，WHEN 应用启动时，THE Index_DB SHALL 删除现有数据库文件并从 Vault 文件系统完整重建索引，重建后 fragments 表行数等于 capture/ 目录下有效 .md 文件总数，articles 表行数等于 articles/ 目录下有效 .md 文件总数
7. WHEN 文件系统中某个已索引的 .md 文件内容发生变化时，THE Index_DB SHALL 比较该文件的 sha256 哈希值与 content_hash 字段的存储值，仅在哈希值不同时更新对应索引记录
8. IF .md 文件缺少 YAML Frontmatter 或 Frontmatter 解析失败，THEN THE Index_DB SHALL 跳过该文件不写入索引，并在应用日志中记录该文件路径及失败原因
9. WHEN capture/ 或 articles/ 目录中有新增 .md 文件时，THE Index_DB SHALL 将新文件解析并插入对应索引表；WHEN 已索引文件从文件系统中被删除时，THE Index_DB SHALL 移除对应索引记录

### 需求 3：碎片文件读写

**用户故事：** 作为用户，我希望能记录碎片并将其持久化为 Markdown 文件，以便数据以人可读的纯文本形式长期保存。

#### 验收标准

1. WHEN 用户提交一条包含至少 1 个非空白字符的碎片正文时，THE App_Shell SHALL 在 capture/ 目录下按 `yyyy/mm/<uuid-short>.md` 路径创建新文件，其中 uuid-short 为 8 位十六进制字符，yyyy 和 mm 取碎片创建时间的年份和月份
2. THE App_Shell SHALL 为每条新碎片文件生成包含 id（8 位十六进制）、created（ISO 8601 含时区，精确到秒）、source（值为 "manual"）、tags（空数组）、topics（空数组）字段的 YAML Frontmatter，各字段按上述顺序排列
3. THE App_Shell SHALL 将用户输入的碎片正文原样写入 Frontmatter 结束标记 `---` 之后，正文与结束标记之间保留一个空行
4. IF 碎片文件写入磁盘失败（如权限不足或磁盘空间不足），THEN THE App_Shell SHALL 向用户显示包含失败原因的错误提示，且不向 Index_DB 写入任何记录
5. WHEN 碎片文件创建成功后，THE Index_DB SHALL 在 3 秒内将该碎片的元数据和正文内容写入 fragments 表和 fragments_fts 表，写入完成前不影响用户继续提交新碎片
6. IF Index_DB 写入失败，THEN THE App_Shell SHALL 保留已创建的碎片文件不做删除，并将该碎片标记为待索引状态以便下次启动时重建索引
7. ~~THE App_Shell SHALL 保证碎片文件一旦创建，Frontmatter 结束标记之后的正文内容永不被应用修改或删除（Frontmatter 中的 tags 和 topics 字段允许由 AI Agent 后续更新）~~
   > **⚠️ Phase 1.5 变更：** 碎片正文已改为允许用户编辑（`update_fragment` IPC），不再 immutable。原始 immutable log 约束取消。

### 需求 4：Frontmatter 解析器

**用户故事：** 作为开发者，我希望有可靠的 YAML Frontmatter 解析和序列化能力，以便正确读写碎片和文章的元数据。

#### 验收标准

1. THE App_Shell SHALL 使用 serde_yaml 解析 Markdown 文件开头由第一行 `---` 起始、下一个仅包含 `---` 的行终止的 YAML Frontmatter 块，并将 Frontmatter 之后的内容作为正文返回
2. IF Markdown 文件不包含合法的 Frontmatter 分隔符（首行非 `---` 或缺少结束 `---`），THEN THE App_Shell SHALL 返回解析错误，错误信息中包含文件路径与出错行号
3. IF Frontmatter 分隔符存在但 YAML 内容不符合语法规范，THEN THE App_Shell SHALL 返回解析错误，错误信息中包含文件路径与 serde_yaml 报告的行号
4. THE App_Shell SHALL 将解析后的 Frontmatter 结构体序列化为合法 YAML 文本，并以 `---` 分隔符包裹后与正文拼接写入目标文件
5. THE App_Shell SHALL 保证 round-trip 属性：对任意合法 Frontmatter 结构体执行序列化再解析后，所得结构体与原始结构体所有字段值逐字段相等

### 需求 5：全局快捷键快速记录

**用户故事：** 作为用户，我希望在应用内任何页面都能通过快捷键快速记录碎片，以便零摩擦地捕捉灵感。

#### 验收标准

1. WHEN 用户在应用内按下 ⌘⇧Space 时，THE Quick_Capture_Modal SHALL 在 200ms 内于当前窗口居中弹出，背景显示模糊遮罩
2. WHEN Quick_Capture_Modal 打开后，THE Quick_Capture_Modal SHALL 在 100ms 内将焦点设置到文本输入区域
3. WHEN 用户在 Quick_Capture_Modal 中按下 ⌘↵ 且文本输入区域包含至少 1 个非空白字符时，THE Quick_Capture_Modal SHALL 将输入内容保存为新碎片文件并自动关闭弹窗
4. WHEN 碎片保存成功后，THE App_Shell SHALL 显示 toast 提示「已保存碎片」，持续 2 秒后自动消失
5. WHEN 用户按下 Esc 或点击遮罩区域时，THE Quick_Capture_Modal SHALL 关闭弹窗且丢弃已输入的内容
6. WHILE Quick_Capture_Modal 打开时，THE Quick_Capture_Modal SHALL 保持底层页面的滚动位置、表单输入值和导航状态不变
7. IF 用户在 Quick_Capture_Modal 中按下 ⌘↵ 且文本输入区域为空或仅含空白字符，THEN THE Quick_Capture_Modal SHALL 保持打开状态且不执行保存操作
8. IF 碎片保存过程中发生文件写入失败，THEN THE Quick_Capture_Modal SHALL 保持打开状态并显示 toast 错误提示，且保留用户已输入的内容

### 需求 6：侧边栏导航

**用户故事：** 作为用户，我希望通过侧边栏在发现/创作/碎片/文章四个功能区之间快速切换，以便高效导航。

#### 验收标准

1. THE Sidebar SHALL 展开态宽度为 248px，包含品牌标识、快速记录按钮和四个导航项（发现/创作/碎片/文章），且应用启动时默认选中「发现」导航项
2. WHEN 用户点击侧边栏收起/展开按钮或按下 ⌘\ 时，THE Sidebar SHALL 在展开态（248px）与折叠态（60px，仅显示图标导航）之间切换，切换状态在应用会话间持久保留
3. WHEN 用户点击某个非当前选中的导航项时，THE View_Stack SHALL 将主区域替换为对应的功能页视图，并对当前选中项不重复触发替换
4. THE Sidebar SHALL 对当前选中的导航项显示白底高亮 + 图标变为 accent 色的选中态样式
5. THE Sidebar SHALL 在碎片和文章导航项旁显示对应的条目总数计数，数值超过 999 时显示为「999+」

### 需求 7：视图栈导航模型

**用户故事：** 作为用户，我希望在功能页内点击条目时能进入详情视图，并通过返回按钮回到上一级，以获得流畅的浏览体验。

#### 验收标准

1. WHEN 用户在功能页内点击某个条目时，THE View_Stack SHALL 将主区域内容替换为对应的详情视图
2. WHILE 视图历史记录栈深度大于 0 时，THE View_Stack SHALL 在当前视图左上角显示「← 返回」按钮
3. WHEN 用户点击「← 返回」按钮或按下 Esc 时，THE View_Stack SHALL 恢复为上一级视图；IF 当前已处于功能页顶级视图（栈深度为 0），THEN THE View_Stack SHALL 忽略返回操作且不产生任何视觉变化
4. THE View_Stack SHALL 为每个功能页（发现/创作/碎片/文章）独立维护视图历史记录栈，最大深度为 10 级，支持逐级返回
5. WHEN 用户通过侧边栏切换功能页时，THE View_Stack SHALL 保留原功能页的历史记录栈状态，并显示目标功能页最后停留的视图
6. WHEN 前进导航（进入详情视图）发生时，THE View_Stack SHALL 以新视图从右侧滑入的动画展示，动画时长使用 `var(--motion-base)` (220ms)，曲线使用 `var(--ease-standard)`；WHEN 后退导航（返回上一级）发生时，THE View_Stack SHALL 以当前视图向右侧滑出的动画展示，动画时长和曲线相同

### 需求 8：碎片页（Inbox 时间流）

**用户故事：** 作为用户，我希望在碎片页看到所有历史碎片按时间倒序排列，以便回顾和管理灵感记录。

#### 验收标准

1. THE App_Shell SHALL 在碎片页顶部显示内联输入框（placeholder 文案「写下你的想法…」）和「记录」提交按钮
2. WHEN 用户在输入框中输入文字后按 Enter 或点击「记录」按钮时，THE App_Shell SHALL 将输入内容作为新碎片提交，清空输入框，并显示 toast 提示「已记录」
3. IF 用户在输入框为空或仅含空白字符时按 Enter 或点击「记录」按钮，THEN THE App_Shell SHALL 不执行提交操作且输入框保持当前状态
4. THE App_Shell SHALL 将碎片按日期分组，每组显示日期标签（格式示例：「今天 · 6月24日」），组内碎片按创建时间倒序排列
5. THE App_Shell SHALL 对每条碎片显示正文内容（超过 3 行时截断并显示省略）、创建时间（格式 HH:mm）和 AI 标签列表
6. THE App_Shell SHALL 在碎片页筛选栏提供三个筛选项：「全部」显示所有碎片、「未整理」仅显示未被 AI 归入任何 Topic 的碎片、「已归类」仅显示已被 AI 归入至少一个 Topic 的碎片
7. THE App_Shell SHALL 在筛选栏右侧显示当前筛选结果对应的碎片总数（格式：「共 N 条」）
8. WHEN 用户 hover 某条碎片时，THE App_Shell SHALL 显示「引用到创作」和「查看关联」两个操作按钮
9. WHEN 碎片已被 AI 归入 Topic 且未处于 hover 状态时，THE App_Shell SHALL 在该碎片右侧显示已归类标记图标

### 需求 9：全文搜索

**用户故事：** 作为用户，我希望能通过关键词搜索碎片和文章内容，以便快速定位历史记录。

#### 验收标准

1. WHEN 用户在碎片页搜索框输入关键词且字符数不少于 1 个时，THE Index_DB SHALL 在 300ms 内使用 FTS5 对 fragments_fts 表执行全文检索，并返回最多 50 条匹配结果
2. WHEN 用户在文章页搜索框输入关键词且字符数不少于 1 个时，THE Index_DB SHALL 在 300ms 内使用 FTS5 对文章标题和内容执行全文检索，并返回最多 50 条匹配结果
3. THE Index_DB SHALL 对搜索结果按相关性排序，FTS5 rank 值高的结果排列在前
4. THE Index_DB SHALL 在搜索结果中返回包含匹配关键词的上下文片段，片段长度不超过 150 个字符，并标记匹配关键词的起止位置
5. IF 搜索关键词在碎片或文章中无任何匹配，THEN THE Index_DB SHALL 返回空结果集，且界面显示无结果提示信息

### 需求 10：文章创建与编辑

**用户故事：** 作为用户，我希望能创建文章并使用富文本编辑器撰写内容，以便将碎片化想法发展为系统性文章。

#### 验收标准

1. WHEN 用户点击新建文章按钮或按下 ⌘N 时，THE App_Shell SHALL 在 `articles/` 目录创建一个新文章文件，文件名为 `<uuid-short>.md`，Frontmatter 中 status 字段值为 "draft"
2. THE TipTap_Editor SHALL 在创作页中间区域渲染富文本编辑器，支持 H1-H3 标题、加粗、斜体、行内代码、代码块、引用块、有序列表和无序列表格式
3. THE TipTap_Editor SHALL 在用户停止输入 1 秒后自动将编辑器内容序列化为 Markdown 格式并保存到对应的文章文件
4. THE App_Shell SHALL 在编辑器上方显示文章标题输入框、状态标签、字数统计（以中文字符 + 英文单词为计数单位）和最后编辑时间
5. WHEN 用户修改文章内容后，THE App_Shell SHALL 更新文章文件的 Frontmatter 中 updated 字段为当前时间（ISO 8601 含时区）
6. THE App_Shell SHALL 支持用户手动切换文章状态：草稿 → 修改中 → 已完成，状态变更写入 Frontmatter 的 status 字段
7. IF 文章文件写入失败，THEN THE App_Shell SHALL 在 Status_Bar 显示保存失败提示，保留编辑器中的内容不丢失

### 需求 11：碎片引用（@reference）

**用户故事：** 作为用户，我希望在编辑文章时能引用碎片，以便在写作中追溯灵感来源。

#### 验收标准

1. THE TipTap_Editor SHALL 支持 `@[fragment-id]` 引用语法，在编辑器中将其渲染为不可编辑的内联 Reference_Chip 节点
2. WHEN 用户点击创作页左侧面板中的碎片条目时，IF TipTap_Editor 当前拥有光标焦点，THEN THE TipTap_Editor SHALL 在当前光标位置插入该碎片的 Reference_Chip
3. IF 用户点击创作页左侧面板中的碎片条目时 TipTap_Editor 无光标焦点，THEN THE TipTap_Editor SHALL 将 Reference_Chip 追加到文档末尾
4. THE Reference_Chip SHALL 显示为使用 `var(--accent)` 色调背景的内联标签，内容为碎片的 8 位短 ID（即碎片文件名前缀）
5. WHEN 文章保存为 Markdown 文件时，THE TipTap_Editor SHALL 将 Reference_Chip 序列化为 `@[fragment-id]` 格式
6. WHEN 加载包含 `@[fragment-id]` 语法的 Markdown 文件时，IF 对应的碎片文件存在，THEN THE TipTap_Editor SHALL 将其渲染为对应的 Reference_Chip
7. WHEN 加载包含 `@[fragment-id]` 语法的 Markdown 文件时，IF 对应的碎片文件不存在，THEN THE TipTap_Editor SHALL 将其渲染为带有视觉失效状态的 Reference_Chip 并保留原始 ID 文本

### 需求 12：文章列表与预览

**用户故事：** 作为用户，我希望在文章页浏览所有文章并预览内容，以便管理和查找已创作的作品。

#### 验收标准

1. THE App_Shell SHALL 在文章页左侧区域显示所有文章的列表，按最后编辑时间倒序排列，每行包含标题、摘要（正文前 50 字）、状态标签、字数和日期
2. THE App_Shell SHALL 支持按状态筛选文章：全部/草稿/修改中/已完成，默认显示「全部」
3. THE App_Shell SHALL 支持按标签多选组合筛选文章，多标签筛选取交集（即文章必须包含所有已选标签）
4. WHEN 用户点击文章列表中的某一行时，THE App_Shell SHALL 在右侧 380px 预览面板中显示该文章的标题、状态、字数、标签和 Markdown 渲染后的内容预览
5. THE App_Shell SHALL 在预览面板中提供「编辑」按钮，点击后切换到创作页并打开该文章进行编辑
6. THE App_Shell SHALL 在预览面板中提供「导出」按钮，点击后通过系统文件保存对话框将文章以 .md 格式导出到用户选择的路径
7. WHEN 用户点击预览面板中的「删除」按钮时，THE App_Shell SHALL 弹出确认对话框；用户确认后删除该文章文件及其 Index_DB 记录，并刷新文章列表

### 需求 13：发现页（规则触发 Feed）

**用户故事：** 作为用户，我希望打开应用时能看到基于规则生成的统计卡片，以便快速了解近期碎片记录概况。

#### 验收标准

1. THE Discover_Feed SHALL 作为应用启动后的默认首页显示
2. THE Discover_Feed SHALL 显示「最近碎片统计」卡片，包含过去 7 天的碎片数量、新增标签数量
3. THE Discover_Feed SHALL 显示「高频标签」卡片，列出过去 7 天内出现次数最多的 5 个标签及其碎片计数；IF 过去 7 天内无任何标签，THEN THE Discover_Feed SHALL 在该卡片位置显示空状态提示
4. IF 过去 7 天新增碎片超过 20 条，THEN THE Discover_Feed SHALL 显示一条活跃度提示卡片，内容包含实际碎片数量和与前 7 天的对比增减数值
5. THE Discover_Feed SHALL 按固定优先级从上到下排序显示卡片：活跃度提示 > 最近碎片统计 > 高频标签
6. WHEN 用户点击卡片「×」按钮时，THE Discover_Feed SHALL 以淡出动画移除该卡片，并在当前应用会话内不再显示该卡片；下次应用启动时重新根据规则生成所有卡片
7. WHEN 用户点击卡片中的「查看详情」按钮时，THE View_Stack SHALL 切换为卡片对应的详情视图
8. IF 过去 7 天内无任何碎片记录，THEN THE Discover_Feed SHALL 显示空状态占位图和引导文案，不显示统计卡片

### 需求 14：文件监听与增量索引

**用户故事：** 作为用户，我希望在外部编辑 Vault 文件后应用能自动感知变更并更新索引，以便保持搜索结果与文件系统一致。

#### 验收标准

1. WHEN App_Shell 启动后，THE File_Watcher SHALL 使用 Rust notify crate 递归监听 Vault 目录下 capture/ 和 articles/ 路径中 .md 文件的创建、修改和删除事件
2. WHEN 检测到新 .md 文件创建事件时，THE File_Watcher SHALL 解析新文件的 Frontmatter 和内容并插入 Index_DB 对应表
3. WHEN 检测到 .md 文件修改事件时，THE File_Watcher SHALL 比较 Content_Hash，仅在内容实际变化时更新 Index_DB 对应记录
4. WHEN 检测到 .md 文件删除事件时，THE File_Watcher SHALL 从 Index_DB 中移除对应记录
5. THE File_Watcher SHALL 对高频变更事件进行去抖处理，合并 500ms 内的连续变更为一次索引更新操作
6. IF 新创建或修改的文件 Frontmatter 解析失败或文件不可读，THEN THE File_Watcher SHALL 跳过该文件的索引操作并记录警告日志

### 需求 15：内置 Git 操作

**用户故事：** 作为用户，我希望应用能自动管理 Vault 的 Git 版本控制，以便将数据同步到 GitHub 并保留完整历史。

#### 验收标准

1. THE Git_Module SHALL 使用 git2 Rust 绑定执行 Git 操作，不依赖系统安装的 git 命令行
2. WHEN 用户手动触发同步操作时，THE Git_Module SHALL 将 Vault 中所有未暂存的变更执行 git add、git commit、git push 到远程仓库，其中提交信息格式为「sync: N files changed · YYYY-MM-DD HH:mm」（N 为本次提交包含的变更文件数）
3. IF git push 操作失败（如网络错误或认证失败），THEN THE Git_Module SHALL 在 Status_Bar 显示包含失败原因分类的错误提示，保留本地 commit 不丢弃，且 push 操作超时时间为 30 秒
4. THE Git_Module SHALL 在 `.gitignore` 中排除 `.cognest/` 目录、`*.sqlite` 文件和 `vectors.bin`
5. THE Status_Bar SHALL 显示当前 Git 同步状态：「已同步」表示本地无未推送 commit 且无未暂存变更，「未同步（N 个文件待推送）」表示存在未暂存变更或未推送 commit 时显示涉及的文件数量
6. IF 用户触发同步操作但 Vault 尚未配置 Git 远程仓库，THEN THE Git_Module SHALL 在 Status_Bar 显示提示信息指示需要配置远程仓库，且不执行 commit 和 push 操作
7. IF 用户触发同步操作但 Vault 中无任何未暂存变更且无未推送 commit，THEN THE Git_Module SHALL 不创建空 commit，并在 Status_Bar 保持「已同步」状态

### 需求 16：状态栏

**用户故事：** 作为用户，我希望在窗口底部看到应用运行状态信息，以便了解后台任务进度和知识库概况。

#### 验收标准

1. THE Status_Bar SHALL 固定显示在窗口底部，高度为 28px，不随主区域内容滚动
2. THE Status_Bar SHALL 在左侧显示当前后台任务描述（如「索引更新中…」「同步中…」或无任务时不显示任务文案）
3. THE Status_Bar SHALL 在右侧显示知识库条目总数（碎片数 + 文章数），格式为「知识库 N 条」，该数值在 Index_DB 变更后实时更新
4. THE Status_Bar SHALL 在右侧显示 Git 同步状态图标和文字
5. WHEN 用户点击 Status_Bar 右侧设置按钮时，THE App_Shell SHALL 打开设置面板

### 需求 17：设置面板

**用户故事：** 作为用户，我希望通过设置面板配置 Vault 路径和查看应用信息，以便自定义使用环境。

#### 验收标准

1. WHEN 用户点击状态栏设置按钮时，THE App_Shell SHALL 弹出 macOS 偏好设置风格的模态面板（居中显示，背景模糊遮罩）
2. THE App_Shell SHALL 在设置面板左侧显示 tab 导航，包含：账户、快捷键、知识库、插件四个 tab，默认选中「账户」tab
3. THE App_Shell SHALL 在「知识库」tab 中显示 Vault 存储路径（绝对路径）、碎片总数和文章总数，数值从 Index_DB 实时读取
4. THE App_Shell SHALL 在「快捷键」tab 中以只读方式展示所有已注册的快捷键列表（快捷键名称 + 对应快捷键组合）
5. THE App_Shell SHALL 在「插件」tab 中显示空状态提示「插件生态将在后续版本开放」
6. WHEN 用户按下 Esc 或点击遮罩区域时，THE App_Shell SHALL 关闭设置面板

### 需求 18：设计规范遵循

**用户故事：** 作为用户，我希望应用界面遵循 Apple 设计系统审美和 tokens.css 定义的设计令牌，以获得一致且精致的视觉体验。

#### 验收标准

1. THE App_Shell SHALL 引用 tokens.css 作为唯一的设计令牌来源，不在任何 React 组件或样式文件中重复定义 :root CSS 变量
2. THE App_Shell SHALL 使用 1.5px stroke、18×18 viewBox、currentColor 填充的线性 SVG 作为所有图标，不使用 emoji 或外部图标库
3. THE App_Shell SHALL 确保每屏可见的蓝色 accent（`var(--accent)`）用于可交互元素不超过 2 处（如主按钮、选中态图标）
4. THE App_Shell SHALL 对控件（按钮、输入框）使用 `var(--radius-sm)` (8px) 圆角、卡片使用 `var(--radius-md)` (12px) 到 `var(--radius-lg)` (18px) 圆角、胶囊形元素使用 `var(--radius-pill)` (980px) 圆角
5. THE App_Shell SHALL 对所有视图切换和面板展开动画使用 `var(--ease-standard)` 曲线和 `var(--motion-base)` (220ms) 时长，微交互（hover、focus）使用 `var(--motion-fast)` (150ms)
6. THE App_Shell SHALL 使用 `var(--font-display)` 用于标题、`var(--font-body)` 用于正文、`var(--font-mono)` 用于代码和数据展示

### 需求 19：创作页布局

**用户故事：** 作为用户，我希望创作页提供三栏布局（相关素材面板 + 编辑器 + AI 面板预留位），以便在写作时随时参考相关碎片。

#### 验收标准

1. THE App_Shell SHALL 在创作页左侧渲染 280px 宽的相关素材面板，包含「灵感/话题/文章」三个 tab，默认选中「灵感」tab
2. THE App_Shell SHALL 在「灵感」tab 中列出与当前文章标签匹配的碎片列表，每条碎片显示内容摘要（最多 3 行）、标签和日期，列表最多展示 50 条结果
3. THE App_Shell SHALL 在「话题」tab 中列出与当前文章关联的 Topic（通过共享标签或双链匹配），每条显示话题名称和关联碎片数
4. THE App_Shell SHALL 在「文章」tab 中列出与当前文章关联的其他文章（通过双链或共享 Topic 匹配），每条显示文章标题和状态
5. IF 当前 tab 无匹配结果，THEN THE App_Shell SHALL 显示空状态提示文案，告知用户当前无相关内容
6. THE App_Shell SHALL 在创作页中间区域渲染 TipTap_Editor 编辑器主体，包含文章标题输入区、元信息栏（状态、字数、最后编辑时间）和正文编辑区
7. THE App_Shell SHALL 在创作页右侧预留 320px 宽的 AI 面板区域，MVP 阶段显示占位提示「AI 辅助将在后续版本启用」
8. WHEN 用户按下 ⌘⇧F 时，THE App_Shell SHALL 进入沉浸式模式，隐藏左侧面板和右侧面板，编辑器居中显示且最大宽度为 720px；再次按下 ⌘⇧F 时退出沉浸式模式，恢复三栏布局

### 需求 20：渐进式启动

**用户故事：** 作为用户，我希望应用启动迅速且首屏有内容可看，以便获得流畅的使用体验。

#### 验收标准

1. WHEN App_Shell 启动时，THE App_Shell SHALL 优先渲染应用外壳（侧边栏 + 空主区域框架），不等待索引加载完成
2. IF Index_DB 已存在（非首次启动）且全量索引尚未加载完成，THEN THE App_Shell SHALL 从 Index_DB 读取最近 50 条碎片用于首屏展示
3. IF Index_DB 不存在（首次启动），THEN THE App_Shell SHALL 显示进度指示器（包含已扫描文件数与总文件数），并在后台执行全量索引构建
4. THE App_Shell SHALL 在 tokio 后台线程执行全量索引扫描，期间 UI 主线程保持响应用户输入（点击、滚动、快捷键），输入响应延迟不超过 100ms
5. WHEN 后台索引构建或加载完成时，THE App_Shell SHALL 自动刷新当前视图数据为完整索引结果，并移除进度指示器（如存在）
