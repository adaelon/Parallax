---
status: accepted
---

# 首个模型运行时采用 OpenAI Responses 家族

**决策**：首个本地与云端运行时统一采用 OpenAI Responses 严格结构化协议。

**否决**：
- 同时接入不同供应商 SDK：会在首片引入不可比较的协议差异。
- 让 Core 理解供应商响应：会污染可信领域边界并扩大替换成本。
- 依赖自由文本提取操作：无法形成可拒绝、可测试的状态修改白名单。

**命门**：两端只交换 G03 v1 contract；固定双档案与模型由 [ADR-0053](0053-vault-backed-configurable-responses-runtime-profile.md) 取代。
**何时回头**：任一模型无法稳定实现同一严格结构化夹具，或 Responses 兼容层泄漏供应商状态时。
**后续**：[ADR-0054](0054-deepseek-chat-completions-protocol-adapter.md) 为 DeepSeek 官方后端接受受限 Chat Completions 适配；其他后端仍受本决策约束。
**展开**：[G03 Runtime Contract v1](../runtime-contract-v1.md)
