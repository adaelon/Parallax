# G06 Block Lineage Contract v1

## 1. 冻结边界

`eam-block-lineage-v1` 只比较同一 `SourceRecord` 的相邻已物化提取修订。输入必须是可信 Core 已验证的不可变 `EvidenceBlock`、规范 UTF-8 Markdown 与有序父子结构；输出只包含显式谱系和增量工作计划，不改写任何 `EvidenceBlockRef`。

## 2. 确定性匹配

按以下顺序消费候选；前一步已经消费的块不再进入后一步：

1. **唯一原生定位器**：版本、类型和值完全相同，且在前后修订中各只出现一次、块类型相同。
2. **唯一精确指纹**：`kind + metadata + canonical quote` 完全相同，且在前后修订中各只出现一次。
3. **受限修改候选**：块类型相同、父块均为空或父块已唯一对应、ordinal 距离不超过 `2`；以 Unicode scalar trigram Dice 系数计算分数，双向唯一最佳分数至少 `7000 bp`，且相对各自第二名至少领先 `1500 bp`。少于三个 scalar 的正文只接受精确或定位器匹配。
4. **失败关闭**：精确候选不唯一，或修改候选达到阈值但不满足双向唯一与领先差，记为 `AMBIGUOUS`；没有候选才记为 `REMOVED`。

唯一定位器或唯一精确指纹匹配后，ordinal 与已映射父块位置都未变化为 `UNCHANGED`，否则为 `MOVED`。受限修改候选只产生 `MODIFIED`；它即使分数很高也不得自动前移旧引用。

## 3. 增量工作计划

```text
UNCHANGED | MOVED
  -> ADVANCE_CURRENT_PROJECTION(from_ref, to_ref)
  -> REUSE_INDEX_PAYLOAD(from_ref, to_ref)

MODIFIED
  -> REBUILD_INDEX(to_ref)
  -> REVIEW_MEMORY(from_ref, MODIFIED)

REMOVED | AMBIGUOUS
  -> REVIEW_MEMORY(from_ref, status)

current block without consumed predecessor
  -> REBUILD_INDEX(to_ref)
```

只有 `UNCHANGED/MOVED` 能产生 `ADVANCE_CURRENT_PROJECTION`。`AMBIGUOUS` 保存全部候选引用但 `to_ref = None`；候选新块仍分别按无前驱新证据重建索引。

## 4. 固定基准

基准位于 `crates/ingestion/tests/fixtures/lineage/`：

| 变化 | 预期 |
| --- | --- |
| `baseline.md -> inserted.md` | 新段落无前驱；被后移的精确块为 `MOVED` |
| `baseline.md -> moved.md` | 调换的精确块为 `MOVED` |
| `baseline.md -> modified.md` | 唯一近似正文为 `MODIFIED`，不产生当前投影 |
| `baseline.md -> deleted.md` | 缺失正文为 `REMOVED` |
| `ambiguous-from.md -> ambiguous-to.md` | 重复段落候选为 `AMBIGUOUS`，不得按最近位置猜测 |

## 5. 版本升级

改变指纹字段、候选约束、`7000/1500 bp` 阈值或状态生成顺序必须提升规则版本并重新运行全部固定基准。真实资料若显示误配或长期高歧义，优先失败关闭并升级规则，不在原版本内漂移阈值。
