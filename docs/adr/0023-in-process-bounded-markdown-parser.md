---
status: accepted
---

# Markdown 由 Core 内纯 Rust 解析

**决策**:首版在 Core 内以纯 Rust 受限解析 Markdown。

**否决**:
- AppContainer 独立进程：故障隔离更强，但单一纯文本格式不足以抵消部署成本。
- WebView 或 JavaScript 解析：会把不可信证据处理带入界面边界。
- 不提取结构：实现最少，但无法支持稳定证据块和增量谱系。

**命门**:解析故障与宿主共享命运；持久化尝试状态必须阻止坏文件自动崩溃循环。

**何时回头**:新增复杂格式、引入原生依赖，或真实样本出现不可控崩溃、挂起或资源耗尽时。

**取代**:[ADR-0017：按需 AppContainer 解析器](0017-on-demand-appcontainer-parser.md)。

**展开**:[产品 FR-11](../product-spec.md#fr-11-不可信内容隔离)；[架构 3.5](../architecture.md#35-core-内-markdown-解析)。
