---
status: accepted
---

# DeepSeek 官方后端使用 Chat Completions 协议适配

**决策**：`api.deepseek.com` 经 Chat Completions 适配，其他后端保持 Responses，Core 严格契约不变。

**否决**：
- 按模型名猜测协议：模型名可由代理复用，不能稳定标识 wire contract。
- 在档案中新增 provider 字段：官方主机已能无歧义识别，不值得迁移 Vault 与扩大 WebView 接口。
- 放宽为自由 JSON：会削弱既有 schema、操作白名单与失败关闭边界。

**命门**：DeepSeek JSON Object 仍由本地严格解析；思考模式关闭以保持 45 秒测试边界。
**何时回头**：DeepSeek 提供 Responses，或本人需要通过自定义代理选择 Chat Completions 时。
**展开**：[G03 Runtime Contract v3](../runtime-contract-v3.md)
