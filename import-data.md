# Cognest 导入数据

从 OpenDesign 原型中提取的碎片和文章数据，用于导入到 MVP 展示和测试。

---

## 碎片 (Fragments / Captures)

### 2026-06-24

```yaml
---
type: capture
id: frag-001
title: ""
tags: [MCP, Agent 架构]
created_at: 2026-06-24T10:32:00+08:00
source: manual
---
MCP 本质上是一个 USB-C 协议——标准化了外设的发现和注册
```

```yaml
---
type: capture
id: frag-002
title: ""
tags: [产品设计]
created_at: 2026-06-24T08:15:00+08:00
source: manual
---
知识管理软件的核心不是「管理」，是让知识自己生长
```

### 2026-06-23

```yaml
---
type: capture
id: frag-003
title: ""
tags: [Agent Memory, Agent 架构]
created_at: 2026-06-23T22:15:00+08:00
source: manual
---
Agent 的 Memory 本质上是一个持续演化的 context window
```

```yaml
---
type: capture
id: frag-004
title: ""
tags: [产品设计]
created_at: 2026-06-23T14:08:00+08:00
source: manual
---
产品设计的第一性原则：用户的认知负担最小化
```

```yaml
---
type: capture
id: frag-005
title: ""
tags: [Context Engineering]
created_at: 2026-06-23T09:41:00+08:00
source: manual
---
Context Engineering 的核心是让模型在对的时间看到对的信息
```

### 2026-06-22

```yaml
---
type: capture
id: frag-006
title: ""
tags: [产品设计]
created_at: 2026-06-22T16:55:00+08:00
source: manual
---
双空间不对——应该是人在两端、AI 在中间
```

```yaml
---
type: capture
id: frag-007
title: ""
tags: [RAG, Context Engineering]
created_at: 2026-06-22T11:23:00+08:00
source: manual
---
RAG 的瓶颈不是检索精度，是检索到的上下文如何被模型有效利用
```

---

## 文章 (Articles)

### 1. Context Engineering 方法论

- **状态**: 草稿
- **字数**: 2,847
- **Tags**: AI, 工程, Context, LLM
- **最后编辑**: 2026-06-24 14:32

> 从 Prompt Engineering 到 Context Engineering 的范式跃迁

#### 从 Prompt Engineering 到 Context Engineering

过去一年，我们对 LLM 应用开发的理解经历了一次范式跃迁。早期的「Prompt Engineering」本质上是在优化单次输入；而「Context Engineering」关注的是如何在对的时间，让模型看到对的信息。

> RAG 的瓶颈不是检索精度，是检索到的上下文如何被模型有效利用。

#### Context Pipeline 的三个组件

一个完整的 Context Pipeline 包括三个部分：RAG、Agent Memory、Tool Results。这三者的组装、排序、裁剪——就是 Context Engineering 的核心工作。

Prompt Engineering 的核心假设是「只要指令写得好，模型就能回答好」。Context Engineering 的核心假设是「模型的输出质量，取决于它接收到的上下文质量」。

---

### 2. MCP 协议设计哲学

- **状态**: 草稿
- **字数**: 1,204
- **Tags**: AI, 工程, MCP
- **最后编辑**: 2026-06-23 22:10

> 为什么 Model Context Protocol 选择了这种约束式设计

#### 约束即自由

MCP 选择了极度约束的设计——不试图做一个万能 API，而是定义一个最小化的「工具发现 + 调用」协议。这种约束看似限制了灵活性，实则降低了集成成本。

任何 AI Agent 只要实现 MCP client，就能调用任何 MCP-compatible 工具——无需学习每个工具的独特 API。

---

### 3. Agent Memory 的本质

- **状态**: 已完成
- **字数**: 4,312
- **Tags**: AI, Agent, Memory
- **最后编辑**: 2026-06-20

> 记忆不是存储，是在有限窗口中还原历史语义的能力

#### 记忆 ≠ 存储

Memory 的核心挑战不是存储，而是召回。它本质是长期 context 的压缩表示——需要在有限的 window 中还原出足够的历史语义。

#### 三种记忆层次

短期（当前对话 context）、中期（session-level 摘要）、长期（跨 session 持久化）。每一层的压缩策略不同。

---

### 4. 知识管理的终局：从文件夹到语义图

- **状态**: 已完成
- **字数**: 3,876
- **Tags**: 产品, AI, 知识管理
- **最后编辑**: 2026-06-15

> 为什么传统知识管理软件注定需要 AI 重构

#### 文件夹是 20 世纪的隐喻

我们用文件夹管理知识已经 40 年了。但人脑从来不是按文件夹组织信息的——它是网状的、关联的、按语义而非路径寻址的。

AI Native 的知识管理应该模仿人脑：自动建立关联、按语义检索、随时间演化。

---

### 5. RAG 不是答案，Context Pipeline 才是

- **状态**: 修改中
- **字数**: 2,156
- **Tags**: AI, 工程, RAG
- **最后编辑**: 2026-06-12

> 单纯的向量检索无法解决上下文质量问题

#### RAG 的局限性

单纯的向量检索只解决了「找到相关文档」这一步。但把文档塞进 prompt 不等于模型能有效利用它——这是两个完全不同的问题。

Context Pipeline 在 RAG 之上加入了排序、裁剪、格式化、上下文窗口管理。

---

### 6. 决策疲劳与认知卸载

- **状态**: 已完成
- **字数**: 1,890
- **Tags**: 思维, 产品
- **最后编辑**: 2026-06-08

> 为什么把组织工作交给 AI 不只是效率问题

#### 整理本身就是消耗

每次你决定「这条笔记放在哪个文件夹」「给它什么标签」，你消耗的不是时间，是认知资源。这些微决策累积成决策疲劳。

把组织工作交给 AI 不只是效率提升——是释放人类最稀缺的资源：判断力。

---

### 7. Karpathy LLM Wiki 理念解读

- **状态**: 修改中
- **字数**: 2,543
- **Tags**: AI, 产品, Wiki
- **最后编辑**: 2026-06-05

> 从 Raw Memory 到 Knowledge Graph 的知识演化路径

#### 从 Raw Memory 到 Knowledge Graph

Karpathy 提出的演化路径：Raw Memory → Topic → Note → Article → Wiki Page → Knowledge Graph。每一步都是对原始信息的一次结构化提升。

AI 在这个链路中的角色是自动维护结构——用户只需要输入和审核，不需要手动搬运。

---

> 数据来源：OpenDesign 原型文件 `capture.html` 和 `articles.html`
> 导出时间：2026-06-27
