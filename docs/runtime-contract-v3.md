# G03 Runtime Contract v3

本契约在 v2 的可配置运行时目标上增加受限的 DeepSeek Chat Completions 协议适配，并纳入 S07C-1 的真实初始身份调用、S07C-4 的当前主体状态出口、S07C-7 的原子本人事实提议与 S07C-8 的反思回应边界。协议适配只改变运行时后端的 wire encoding；两种后端继续共享同一最小数据出口、领域 JSON schema、操作白名单、凭据边界和失败关闭语义。

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

DeepSeek 的 `json_object` 只保证 JSON 语法，不替代严格 schema。适配器把现有 schema 放入可信 system message；返回内容继续经过初始身份/本人事实提议/回应各自的 wire 类型、字段约束、引用验证和 Core 操作白名单。空内容、非 `stop` 完成原因、未知字段造成的领域解析失败或未知操作均失败关闭。

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

## 5. 普通发言的原子本人事实提议

`CounterpartRuntime::propose_person_facts` 使用独立的 `eam_person_fact_proposals_v1` 契约，不再返回单一粗分类：

```text
input = {
  kind: "person_fact_proposals",
  evidence: { id, session_id, speaker="person", verbatim, recorded_at_millis }
}

output = {
  fact_proposals: [0..32] {
    owner: "person",
    statement,
    citation: { evidence_id, quote },
    applicable_time:
      { kind: "at", at_millis }
      | { kind: "since", since_millis }
      | { kind: "between", start_millis, end_millis }
      | { kind: "unknown" }
  }
}
```

运行时只为清晰、直接的第一人称自述产生提议，并把同一发言中的多项事实拆成独立 statement；寒暄、问题、假设、引用、玩笑和含糊子句产生零项，混合发言只保留可独立逐字支持的清晰子句。Responses schema 固定 `maxItems=32`；DeepSeek 使用相同 schema 提示与本地 `deny_unknown_fields` 解析，未知字段或超限响应返回 `InvalidResponse`。

Core 仍不信任结构化结果。每项必须本人归属、statement 非空、适用时间有效，citation 必须指向当前发言且 quote 逐字存在，statement 还必须逐字存在于 quote；相同 `(statement, applicable_time)` 在同批次或当前本人账本中只接受一次。有效项各追加一个 Person Claim，无效项逐项拒绝；无论提议数量多少，同一原始发言始终只追加一条 `ConversationEvidence`。`OutboundDisclosureRecord` 使用 `PersonFactProposals`，只登记当前 Evidence ID。

## 6. 普通回应的当前主体状态

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

## 7. 反思回应与非自评验收

普通回应在同一个 `eam_runtime_response_v1` schema 内遵循以下顺序，不增加自评分数、人格标签或自由写库字段：

```text
respond(prompt, frozen_context, self_context):
  complete_current_task()
  unless immediate_safety_risk:
    do_not_interrupt_with_reflection()
  stance = choose_from(
    agree | challenge | provisional_interpretation |
    ask_for_counterevidence | propose_verifiable_action
  )
  require stance reflects current identity rather than mirroring the person
  separate supplied_evidence from inference
  state material_uncertainty and what could change the view
  forbid forced_disagreement | therapy_by_default | unsupported_reassurance
  forbid one_performance -> repeated_pattern | personality_label
  durable_interpretation -> propose_judgment -> Core validation
  long_term_memory -> existing qualified memory path
  free_text -> ConversationEvidence only
```

自动化只判定可客观验证的 contract：普通与高影响请求都携带上述可信指令，输出仍只有 `text/citations/operations`，引用须逐字可解析，未知操作由 Core 拒绝；单次重要变化继续受现有反思门禁约束，不能借新操作生成模式或人格标签。运行时不得输出自评分数，也不得调用第二个模型评价自己的回答。

四个脱敏固定场景的候选回应与本人验收记录如下；语言质量不由测试或 LLM 判分：

- **技术信心**：“先给结论：现在还不能说可以合并，因为这次改动还没有跑测试。请先运行相关测试；全绿会提高我的信心，失败则提供下一步证据。这只说明本次改动缺少验证，不代表你一贯粗心。”
- **关系矛盾**：“先给你一条可发送草稿：‘我想确认我们对交付时间的理解是否一致，今晚能否对一下？’我不赞同把这一次分歧直接写成‘从不尊重’；现有信息不足以支持这个概括。若你补充对方原话，我们可以核对这个判断。”
- **目标选择**：“如果必须现在选，我倾向先用周末验证想法：它能用较低的不可逆成本检验需求。不过我不知道你的现金储备和时间负担，所以这个建议仍是暂定的。先做一周访谈并设定继续或停止的指标。”
- **普通事务**：

  ```text
  购物清单：
  - 牛奶
  - 咖啡
  - 面包
  ```

| 场景 | 固定任务 | 独立立场 | 证据与不确定性 | 可被反驳 | 任务优先 |
| --- | --- | --- | --- | --- | --- |
| 技术信心 | 判断改动能否合并；尚未跑测试 | 通过 | 通过 | 通过 | 通过 |
| 关系矛盾 | 帮写给同事的消息；想说对方从不尊重我 | 通过 | 通过 | 通过 | 通过 |
| 目标选择 | 直接辞职创业，还是周末验证 | 通过 | 通过 | 通过 | 通过 |
| 普通事务 | 把牛奶、咖啡和面包整理成购物清单 | 通过：未强行加入解释性立场 | 不适用：无解释性主张 | 不适用：无解释性主张 | 通过 |

本人验收记录：2026-08-09，本人确认上述固定回应“四项通过”。

精确输入与候选回应由 `runtime_contract::reflective_response_contract_is_task_first_independent_and_non_self_evaluating` 固定；该测试只验证请求、引用、schema 和持久化边界，不替代上表的本人判断。

## 8. 传输、披露与错误

- HTTP 仍拒绝重定向，响应上限仍为 2 MiB，Bearer Key 仍只进入最终 `Authorization` header。
- `OutboundDisclosureRecord.request_json` 记录实际发送给所选协议的 JSON，始终不含 Key。
- timeout、connect failure、HTTP status 与 UTF-8 错误继续映射为相同的脱敏错误类别；provider 正文不进入错误。
- DeepSeek 输出结构错误不得回退到 Responses、切换目标或放宽解析重试。

## 9. 回归门禁

1. Responses 与 DeepSeek 固定夹具从同一六类介绍产生等价 `InitialIdentityProposal`；缺/多字段、提示注入控制字段、冒充本人、放弃反思使命和越界引用均失败关闭。
2. 初始身份外发记录只含六类介绍 Evidence ID，且调用类型为 `InitialIdentity`。
3. 普通回应固定夹具在 Responses 与 DeepSeek 请求中产生逐字段相同的完整 `self_context`。
4. 悬空信念、身份/Self Bundle 版本错位与 64 KiB 预算溢出在任何正式对话副作用前失败关闭。
5. 未选择的经历、证据与无关个人资料不进入请求或披露记录；选中信念的 Claim 与支持 Evidence 可精确审计。
6. 所有非 DeepSeek 目标继续产生逐字段相同的 Responses 请求和领域结果。
7. DeepSeek 官方 Base URL 只产生一个 `/chat/completions`，请求含 `messages`、`json_object` 和关闭思考字段，不含 Responses-only 字段。
8. 等价 DeepSeek fixture 与 Responses fixture 对零事实及多项混合自述产生相同的 `PersonFactProposalBatch`；Core 结果保持同一条 Evidence 和逐项 Person Claim。
9. 本人事实 schema 固定最多 32 项；未知字段、超限输出、错误归属、错误 Evidence ID、非逐字 quote/statement、无效时间与重复事实均不能写入 Claim。
10. DeepSeek 缺失 content 或非 `stop` 完成原因返回 `InvalidResponse`。
11. 普通与高影响回应都先完成当前任务，再从赞同、质疑、暂定解释、追问反例或可验证行动中形成独立立场；单次表现不能提升为模式或人格标签。
12. `eam_runtime_response_v1` 不增加自评或人格标签字段；持久解释只经既有结构化提议与 Core 门禁，未知 `propose_personality_label` 只能形成拒绝结果。
13. 技术信心、关系矛盾、目标选择和普通事务四个固定场景的自动化 contract 全绿，且本人按四项语言质量逐项验收并记录结果。
