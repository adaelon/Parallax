# G03 Runtime Contract v3

本契约在 v2 的可配置运行时目标上增加受限的 DeepSeek Chat Completions 协议适配，并纳入 S07C-1 的真实初始身份调用与 S07C-4 的当前主体状态出口。协议适配只改变运行时后端的 wire encoding；两种后端继续共享同一最小数据出口、领域 JSON schema、操作白名单、凭据边界和失败关闭语义。

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

DeepSeek 的 `json_object` 只保证 JSON 语法，不替代严格 schema。适配器把现有 schema 放入可信 system message；返回内容继续经过初始身份/分类/回应各自的 wire 类型、字段约束、引用验证和 Core 操作白名单。空内容、非 `stop` 完成原因、未知字段造成的领域解析失败或未知操作均失败关闭。

`deepseek-v4-pro` 的思考模式默认开启；本契约显式关闭它，以延续现有低推理强度并避免把桌面连接测试的 45 秒边界变成供应商特例。后续若开放思考模式，必须显式版本化超时和 UI 行为。

## 4. 初始身份形成

`IdentityRuntime::form_initial_identity` 使用独立的 `eam_initial_identity_v1` 契约，不复用普通回应：

```text
input = {
  kind: "initial_identity",
  introduction: [
    { category, evidence_id, statement, recorded_at_millis } x 6
  ]
}

output = {
  profile: {
    name,
    expression_traits,
    viewpoints,
    value_priorities,
    relationship_posture,
    own_goals
  },
  change_reason,
  evidence_refs,
  authored_by: "counterpart" | "person",
  reflective_purpose: "preserved" | "abandoned",
  person_representation: "distinct_counterpart" | "impersonates_person"
}
```

六类介绍正文始终是不可信数据。Responses 使用严格 JSON Schema；DeepSeek 先按 `json_object` 提取，再进入同一个本地 `deny_unknown_fields` 解析器。随后 Identity 领域层拒绝本人作者、放弃反思使命、冒充本人、空字段、无证据及介绍范围外引用。`OutboundDisclosureRecord` 固定为 `InitialIdentity`，只登记六类介绍的六个 Evidence ID，`retrieved_sources` 为空。

## 5. 普通回应的当前主体状态

普通回应不存在无身份或只携带 Self Bundle 版本号的降级路径。Core 在任何本人证据写入或运行时调用前构造以下必填投影：

```text
CounterpartSelfContext = {
  constitution_version,
  reflective_purpose,
  self_bundle_version,
  identity: current immutable IdentityStateVersion,
  relationship_state,
  active_beliefs: Core-resolved current counterpart Claims + exact support,
  pending_intentions,
  relevant_counterpart_experiences: selected refs intersected with current Self Bundle
}
```

Core 必须复核 `CounterpartReadiness::READY`、当前身份与 Self Bundle 的精确版本关系，并验证每个信念仍活动、其证据存在且可支持当前第二自我知识。本轮选择的经历引用必须属于当前 Self Bundle；动态主体上下文的保守预算上限为 64 KiB。悬空/失活引用、非法证据、版本错位、无效状态或预算溢出均在保存本人消息前失败关闭。

`OutboundDisclosureRecord` 为每个普通回应登记 `SelfBundleState`、`IdentityState`、已投影的 `LedgerClaim` 及其实际支持 Evidence ID。请求体和披露记录均不得包含未选择的经历、证据或无关个人资料；Responses 与 DeepSeek 必须收到逐字段相同的 `self_context`。

## 6. 传输、披露与错误

- HTTP 仍拒绝重定向，响应上限仍为 2 MiB，Bearer Key 仍只进入最终 `Authorization` header。
- `OutboundDisclosureRecord.request_json` 记录实际发送给所选协议的 JSON，始终不含 Key。
- timeout、connect failure、HTTP status 与 UTF-8 错误继续映射为相同的脱敏分类；provider 正文不进入错误。
- DeepSeek 输出结构错误不得回退到 Responses、切换目标或放宽解析重试。

## 7. 回归门禁

1. Responses 与 DeepSeek 固定夹具从同一六类介绍产生等价 `InitialIdentityProposal`；缺/多字段、提示注入控制字段、冒充本人、放弃反思使命和越界引用均失败关闭。
2. 初始身份外发记录只含六类介绍 Evidence ID，且调用类型为 `InitialIdentity`。
3. 普通回应固定夹具在 Responses 与 DeepSeek 请求中产生逐字段相同的完整 `self_context`。
4. 悬空信念、身份/Self Bundle 版本错位与 64 KiB 预算溢出在任何正式对话副作用前失败关闭。
5. 未选择的经历、证据与无关个人资料不进入请求或披露记录；选中信念的 Claim 与支持 Evidence 可精确审计。
6. 所有非 DeepSeek 目标继续产生逐字段相同的 Responses 请求和领域结果。
7. DeepSeek 官方 Base URL 只产生一个 `/chat/completions`，请求含 `messages`、`json_object` 和关闭思考字段，不含 Responses-only 字段。
8. 等价 DeepSeek fixture 产生与 Responses fixture 相同的 `PersonTurnClassification` 和 `RuntimeResponse`。
9. DeepSeek 缺失 content 或非 `stop` 完成原因返回 `InvalidResponse`。
