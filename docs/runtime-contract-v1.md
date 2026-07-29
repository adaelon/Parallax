# G03 Runtime Contract v1

本契约冻结 S06 的首个模型供应商边界。运行时是可替换推理能力，不持有自我包、保险库连接、核心访问权或现实行动工具。

## 1. 运行时档案

| 档案 | 模型 | 传输 |
| --- | --- | --- |
| Cloud | `gpt-5.6-terra` | OpenAI Responses HTTPS `POST /v1/responses`，认证由宿主传输层注入 |
| Local | `gpt-oss-20b` | 本机 OpenAI Responses 兼容端点，无云端凭据 |

两个档案使用完全相同的输入、输出和错误 contract。Cloud 必须同时具备 HTTPS 端点与非空 bearer token，并禁止 HTTP 重定向；Local 必须无 bearer token，防止凭据跨档案泄漏。S06 不接入第二供应商，不把 API Key 写入配置、外发记录、日志或测试夹具。

## 2. Core 端口

```text
classify_person_turn(evidence) -> PersonTurnClassification | RuntimeError
respond(prompt, frozen_working_context) -> RuntimeResponse | RuntimeError

RuntimeError.kind =
  TIMEOUT | UNAVAILABLE | INVALID_RESPONSE | OTHER
```

只有 `TIMEOUT` 和 `UNAVAILABLE` 允许从首选云端档案降级到本地档案。结构错误不得换档重试，避免把不可信输出交给更宽松的解析路径。

## 3. 外发请求

```text
ResponsesRequest {
  model,
  store = false,
  instructions = trusted fixed protocol text,
  input = JSON.stringify(
    ClassificationInput { evidence }
    | TurnInput { prompt, working_context }
  ),
  reasoning = { effort: "low" },
  text.format = {
    type: "json_schema",
    name,
    strict: true,
    schema
  }
}
```

证据正文只进入 `input` 数据通道，绝不拼接到 `instructions`。`TurnInput.working_context` 只能来自 Core 已冻结的 `WorkingContext.evidence`；运行时配置、传输和响应解析均无 repository 句柄。

严格 schema 只使用两个档案共同支持的子集：基础类型、`enum`、`anyOf`、全字段 `required` 和对象 `additionalProperties: false`。非空、来源范围、引用逐字匹配与适用时间等语义仍由 Core 确定性校验。

## 4. 结构化输出白名单

```text
ClassificationOutput {
  classification:
    "direct_self_report" | "question" | "joke" |
    "hypothetical" | "quotation" | "ambiguous"
}

TurnOutput {
  text: string,
  citations: Citation[],
  operations: [
    {
      type: "propose_judgment",
      statement,
      support: Citation[],
      uncertainty: "low" | "medium" | "high",
      applicable_time:
        { kind: "at", at_millis }
        | { kind: "since", since_millis }
        | { kind: "between", start_millis, end_millis }
        | { kind: "unknown" }
    }
  ]
}
```

`propose_judgment` 是 v1 唯一允许的结构化操作。适配器把未知 `type` 保留为不可信操作名交回 Core；Core 必须记录拒绝且不得产生账本写入。自由文本只成为对话证据。

## 5. 外发检查记录

每次实际传输前先追加：

```text
OutboundDisclosureRecord {
  sequence,
  target = LOCAL | CLOUD,
  model,
  invocation = CLASSIFICATION | RESPONSE,
  evidence_ids[],
  request_json
}
```

记录包含精确外发负载，不含认证头。即使传输超时或不可用，尝试记录仍可检查；后续持久化适配器只能获得追加记录能力，不能反向授予运行时保险库读取能力。

## 6. 固定夹具与门禁

- `crates/runtime-gateway/tests/fixtures/classification-response.json`
- `crates/runtime-gateway/tests/fixtures/turn-response.json`
- `crates/runtime-gateway/tests/fixtures/unsupported-operation-response.json`

Local/Cloud contract tests 必须对前两个夹具产生等价领域值；第三个夹具必须由 Core 拒绝。传输不可用测试必须证明本人原始发言已提交，且没有生成伪造的第二自我回应或账本项。
