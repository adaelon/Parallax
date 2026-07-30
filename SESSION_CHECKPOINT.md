# SESSION_CHECKPOINT — 2026-07-30 14:32 +08:00

## 新鲜度自检
- 写入前最新 commit：`5ec4d3b feat: complete S14 frozen working context`；本次刷新将 amend 进同一提交，因此最终 hash 会变化。
- 读入时对比 `git log --oneline -3`；若 hash 不同但标题一致，以 Git 与当前文件状态为准。

## 当前在做什么
S01～S14 已全部完成并通过门禁；当前没有活动代码切片，下一片为 S15「选择性深度理解投影」。

## 下一步（可直接接手）
1. 阅读 `docs/implementation-slices.md` 的 S15、§12，`docs/product-spec.md` FR-05/FR-06，及 ADR-0018，声明 S15-1 投影契约切片。
2. 对照 `crates/retrieval`、S11 增量工作项与 S14 冻结上下文，固定本人指定、反复召回、重要变化和当前任务四类触发输入。
3. 冻结版本化投影、来源引用、失效和可重建契约；若出现难以逆转的真实权衡再新增 ADR。
4. 以有限证据范围实现事件链、人物/主题关系和阶段概括投影，不允许投影取得事实或长期记忆资格。
5. 跑相关测试与全仓门禁，更新 `docs/code-trail.md`、结构变化对应的 `docs/architecture.md` 后提交并刷新 checkpoint。

## 未提交 / 未完成
- 未提交文件：无（本 checkpoint 刷新将 amend 进 S14 完成提交）。
- 未推送：本地 S01～S14 提交尚未发送到 `origin/main`；仅在本人明确要求时 push。
- S15 尚未开始；没有选择性深度理解投影、触发账本、投影失效或重建实现。
- 环境：构建继续把 `TEMP/TMP` 指向 `E:\temp\eam-codex`，避免 C: 空间不足。

## 已验证基线
- `cargo test --workspace --no-fail-fast`：131/131 通过，含 G07 3/3、schema v11、向量损坏重建、邻域/预算、跨重启 replay 和真实桌面 Vault 入口。
- `cargo fmt --all -- --check`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo check -p desktop-app --all-targets`：通过。
- `git diff --check` 与 4 个变更 Markdown 的 149 个本地链接审计：通过。

## 冷启动读序
1. `docs/implementation-slices.md` §2、§3、S15、§12 — 全局不变量、S15 边界与完成判据。
2. `docs/product-spec.md` FR-05、FR-06、§6.1 — 深度理解的检索/记忆边界与确定性验收。
3. `CONTEXT.md` — 工作上下文、RAG、长期记忆与选择性深度理解术语。
4. `docs/architecture.md` §1、§2、§4.1、§5.5、§5.6、§6、§7、§8、§9.14 — schema v11、S14 数据流与 S15 输入边界。
5. `docs/code-trail.md`、`docs/adr/0018-hybrid-rag-selective-deep-understanding.md`、`docs/adr/0019-stable-evidence-blocks-dynamic-retrieval-windows.md`、`docs/adr/0020-immutable-block-references-explicit-lineage.md`。
6. `crates/retrieval/src/lib.rs`、`crates/retrieval/src/context.rs`、`crates/ingestion/src/lineage.rs`、`crates/ingestion/src/service.rs` 的增量物化路径、`crates/vault/src/repository.rs` 的 retrieval/工作项路径、`crates/core/src/domain.rs` 的 `WorkingContext`。

## 本会话决策摘要
- G07 不新增 ADR：向量模型与索引是版本化、可重建且可替换的候选发现派生物；已落档到 `docs/retrieval-contract-v2.md`。
- S14 冻结边界：所有候选和邻域权威回读后才按预算组窗，运行时与外发审计只接收冻结结果。
