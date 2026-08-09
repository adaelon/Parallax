---
status: accepted
---

# 任意 Markdown 目录作为显式只读资料源

**决策**：本人可指定一个任意本地目录作为只读 Markdown 资料源；存在 Obsidian 结构时按兼容语义理解。

**否决**：
- 仅允许 Obsidian 笔记库：把通用 Markdown 能力绑定到非必需应用。
- 赋予 Agent 任意文件系统读取权：越过显式根目录与 Core 最小披露边界。
- 复用 Context Inbox：一次性导入与持续来源当前性的删除语义冲突。

**命门**：访问不得越过本人选择的根目录；资料源移除不等于遗忘，非 Markdown 内容不得进入 Agent 工作上下文。
**何时回头**：需要同时监控多个根目录或增加 Markdown 之外的可理解格式时。
**展开**：[产品 FR-02](../product-spec.md#fr-02-上下文收件箱)；[架构 5.2](../architecture.md#52-文件与只读资料源增量导入)；[ADR-0016](0016-obsidian-source-removal-semantics.md)；[ADR-0024](0024-versioned-markdown-dialect.md)。
