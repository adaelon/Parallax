# G03 Runtime Contract v3

本契约在 v2 的可配置运行时目标上增加一个受限的 DeepSeek Chat Completions 协议适配。它只改变运行时后端的 wire encoding；Core 端口、最小数据出口、领域 JSON schema、操作白名单、凭据边界和失败关闭语义保持不变。

## 1. 协议选择与 endpoint

```text
RuntimeTarget::new(base_url, model):
  normalized = validate_https_or_loopback_http(base_url)
  protocol = if normalized.host == "api.deepseek.com"
             then DEEPSEEK_CHAT_COMPLETIONS
             else OPENAI_RESPONSES

RuntimeTarget::endpoint():
  OPENAI_RESPONSES          -> normalized.path + "/responses"
  DEEPSEEK_CHAT_COMPLETIONS -> normalized.path + "/chat/completions"
```

协议只按 URL 解析后的精确 host 选择；不按模型名、Key 形态或 host 后缀猜测。`https://api.deepseek.com` 产生 `https://api.deepseek.com/chat/completions`。调用方仍须填写 Base URL，不得填写完整 endpoint。其他 DeepSeek-compatible 代理默认继续按 Responses 处理，直到档案契约显式加入协议选择。

## 2. Responses 编码

非 DeepSeek 目标保持 v2 请求与响应：

```text
request = {
  model,
  store: false,
  instructions,
  input,
  reasoning: { effort: "low" },
  text: { format: { type: "json_schema", name, strict: true, schema } }
}

structured_output = response.output[*].content[*]
  where type == "output_text"
```

## 3. DeepSeek Chat Completions 编码

```text
system_content = instructions
  + explicit JSON-only requirement
  + schema name
  + serialized existing JSON Schema

request = {
  model,
  messages: [
    { role: "system", content: system_content },
    { role: "user", content: input }
  ],
  response_format: { type: "json_object" },
  thinking: { type: "disabled" },
  stream: false
}

require choices[0].finish_reason == "stop"
structured_output = choices[0].message.content
```

DeepSeek 的 `json_object` 只保证 JSON 语法，不替代严格 schema。适配器把现有 schema 放入可信 system message；返回内容继续经过既有分类/回应 wire 类型、字段约束、引用验证和 Core 操作白名单。空内容、非 `stop` 完成原因、未知字段造成的领域解析失败或未知操作均失败关闭。

`deepseek-v4-pro` 的思考模式默认开启；本契约显式关闭它，以延续现有低推理强度并避免把桌面连接测试的 45 秒边界变成供应商特例。后续若开放思考模式，必须显式版本化超时和 UI 行为。

## 4. 传输、披露与错误

- HTTP 仍拒绝重定向，响应上限仍为 2 MiB，Bearer Key 仍只进入最终 `Authorization` header。
- `OutboundDisclosureRecord.request_json` 记录实际发送给所选协议的 JSON，始终不含 Key。
- timeout、connect failure、HTTP status 与 UTF-8 错误继续映射为相同的脱敏分类；provider 正文不进入错误。
- DeepSeek 输出结构错误不得回退到 Responses、切换目标或放宽解析重试。

## 5. 回归门禁

1. 所有非 DeepSeek 目标继续产生逐字段相同的 Responses 请求和领域结果。
2. DeepSeek 官方 Base URL 只产生一个 `/chat/completions`，请求含 `messages`、`json_object` 和关闭思考字段，不含 Responses-only 字段。
3. 等价 DeepSeek fixture 产生与 Responses fixture 相同的 `PersonTurnClassification` 和 `RuntimeResponse`。
4. DeepSeek 缺失 content 或非 `stop` 完成原因返回 `InvalidResponse`。
