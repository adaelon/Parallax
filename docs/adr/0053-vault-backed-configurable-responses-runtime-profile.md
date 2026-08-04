---
status: accepted
---

# 运行时采用 Vault 单档案热切换

**决策**：Vault 单档案热切换 Responses 后端、模型与密钥。

**否决**：
- 固定双档案加环境变量：本人无法在应用内自由配置，且修改必须重启。
- 同时接入多协议族：会把协议差异带入本次边界修订。
- 回显密钥或写入 `.env`：扩大 WebView 读取面与持久明文面。

**命门**：WebView 只写密钥，读取仅得状态与末四位；密钥随加密 `self.db` 和 Recovery Set 迁移。
**何时回头**：本人明确需要多个并存档案，或 Responses 之外的协议族。
**展开**：[S06R 实施切片](../implementation-slices.md#s06r-可配置运行时档案边界修订)
