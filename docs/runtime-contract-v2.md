# G03 Runtime Contract v2

本契约冻结 S06R-1 的可配置 Responses 运行时边界。它取代 v1 的固定 Cloud/Local 目标、固定模型与完整 endpoint；Core 端口、最小数据出口、严格结构化 schema、操作白名单和失败关闭语义保持不变。

## 1. 可配置目标

```text
RuntimeTarget {
  base_url: owned string,
  model: owned string
}

HttpResponsesTransport {
  bearer_token: optional zeroizing secret
}

invoke(target, request):
  endpoint = normalize(target.base_url) + "/responses"
  POST endpoint with optional Authorization: Bearer <secret>
```

`base_url` 与 `model` 由运行时实例自有，不借用设置表单、环境变量或调用方缓冲区。Bearer Key 只归宿主传输层所有；`RuntimeTarget`、Core、结构化请求、外发记录和 provider 响应解析均无读取能力。

## 2. Base URL 接受与拒绝矩阵

| 输入类别 | 结果 |
| --- | --- |
| `https://<remote-host>/<optional-path>` | 接受 |
| `http://localhost[:port]/<optional-path>` | 接受 |
| `http://127.0.0.0/8[:port]/<optional-path>` | 接受 |
| `http://[::1][:port]/<optional-path>` | 接受 |
| 非环回 HTTP | 拒绝 |
| 非 HTTP(S) scheme、相对 URL 或缺失 host | 拒绝 |
| 含 username、password、query 或 fragment | 拒绝 |
| 含空白、控制字符或超过 2048 bytes | 拒绝 |

Base URL 解析后使用 URL 标准形式，并移除路径末尾的 `/`；根路径保持根语义。适配器只追加一个 `/responses`：`https://example.test/openai/v1/` 产生 `https://example.test/openai/v1/responses`。调用方不得传入完整 Responses endpoint。

HTTP client 禁止自动重定向。任何 3xx 都按失败返回，Bearer Key 不会被转发到第二个 origin。

## 3. 模型与认证

- 模型 ID 必须是 1～256 bytes 的非空自有字符串，禁止首尾空白与控制字符；适配器逐字写入请求 `model` 和外发记录。
- Bearer Key 可缺省；存在时必须是 1～8192 bytes 的非空字符串且不含控制字符。
- Bearer Key 仅在实际发送前进入 `Authorization` header，不进入请求 JSON、`OutboundDisclosureRecord`、错误文本、日志或 contract fixture。
- 环回后端也可以使用 Bearer Key；是否认证不再由 Local/Cloud 档案名推断。

## 4. 外发请求与记录

请求 JSON 延续 v1：`store=false`、可信固定 `instructions`、不可信正文只进入 `input`、低 reasoning effort、严格 JSON Schema。现有 `eam_person_turn_classification_v1` 与 `eam_runtime_response_v1` 名称继续表示领域 wire schema v1；运行时配置 contract 升级不放宽该 schema。

每次实际传输前追加：

```text
OutboundDisclosureRecord {
  sequence,
  target = LOCAL | CLOUD,
  model,
  invocation,
  evidence_ids[],
  retrieved_sources[],
  request_json
}
```

`LOCAL` 表示 URL host 是字面环回地址，`CLOUD` 表示非环回 HTTPS；它们只用于披露投递边界，不选择固定模型、凭据或 fallback。

## 5. 错误与响应门禁

- timeout、connect unavailable、其他 request failure 和非成功 HTTP status 只返回分类与脱敏消息，不回传请求 header、Bearer Key 或 provider 正文。
- 3xx 不跟随；408、429 与 5xx 归为 unavailable，其他非成功 status 归为 other。
- provider body 仍限制为 2 MiB、必须是 UTF-8，并通过 v1 严格结构化解析；结构错误不得换目标重试。

## 6. v1 等价回归

`classification-response.json` 与 `turn-response.json` 等固定夹具在任意合法 Base URL、模型和认证选择下必须继续产生与 v1 相同的领域值。S06R-1 不新增协议族、不改变 Core 端口、不持久化运行时档案，也不新增宿主 command 或 UI。
