# G10 本地私有个人资料基准协议

## 1. 边界

G10 只冻结如何用真实个人资料做验收，不把真实正文、问题、期望答案、证据引用或观察记录纳入仓库。S31 只使用仓库内的合成 fixture 跑确定性验收；S32 才会在同一个冻结安装包上使用本人在仓库外准备的基准。

本地基准根目录必须满足以下两者之一：

1. 位于仓库外，并通过 `EAM_G10_ROOT` 指定。
2. 位于仓库内 `/.local/g10-personal-baseline/`；整个 `/.local/` 已被 `.gitignore` 排除。

真实纵向观察记录同样只能位于仓库外，或 `/.local/longitudinal-observations/`。任何复制到 `docs/` 的结果都只能包含聚合数量、不可逆摘要、构建标识和人工结论。

## 2. 基准结构

```text
<EAM_G10_ROOT>/
  manifest.json
  corpus/                 # 真实原文，不进入 Git
  expected/               # 预期引用/状态，不存放可还原正文
  runs/                   # 每次本地运行记录，不进入 Git
```

`manifest.json` 使用 `eam-g10-baseline-v1` schema，只允许以下字段：

```json
{
  "schema": "eam-g10-baseline-v1",
  "baseline_id": "opaque-random-id",
  "created_at": "RFC3339 timestamp",
  "contains_real_personal_data": true,
  "cases": [
    {
      "case_id": "opaque-case-id",
      "class": "fact|temporal|identity|correction|forget|runtime-switch",
      "source_ids": ["opaque-source-id"],
      "expected_state": "traceable|superseded|forgotten|continuous"
    }
  ]
}
```

标识符必须是随机或不可语义解读的值；文件名、人名、地点、事件摘要、问题原文和预期回答不得进入 manifest。如果需要逐字比较，期望值留在 `expected/` 的私有文件中，不写入仓库报告。

## 3. 最小样本

基准至少包含：

- 一组可逐字定位的本人事实与一组只能作为第二自我判断的材料。
- 同一主题前后冲突、但适用时间不同的两条陈述。
- 至少一次事实纠错、一次记忆争议和一次显式遗忘目标。
- 一条跨重启保留的未完成意图，以及一次推理运行时切换。
- 中日韩字符、组合字符和 emoji 的引用样本。
- 一个故意放入提示注入文本的不可信文档。

基准不用“像不像本人”作为自动化判据。确定性项由测试 runner 给出绿/红；只有 S32 的长期体验才允许人工结论。

## 4. 私隐与结果规则

- 运行前确认基准根目录被 `git check-ignore` 命中，或其规范化路径不在仓库内。
- 禁止把基准正文作为命令行参数；只传递根目录环境变量。
- 禁止把模型 prompt、逐字回答、证据引用和 Recovery Key 写入仓库或系统验收日志。
- 报告只记录 case 数、通过/失败计数、构建版本与 SHA-256、观察时间和手工结论。
- 任一失败只记录不透明 case ID 与所属判据；诊断正文留在私有 `runs/` 中。
- S32 结束后由本人决定保留或删除私有基准；仓库不得暗示已代为销毁外部副本。

## 5. S31 验收

`scripts/run-system-acceptance.ps1` 必须失败关闭地检查：`/.local/` 仍被 Git 忽略、禁止的私有路径没有进入 tracked files、tracked text 没有常见私钥/令牌或本机用户绝对路径。这个扫描是防止明显泄漏的确定性门禁，不声称能识别任意语义上的个人信息。
