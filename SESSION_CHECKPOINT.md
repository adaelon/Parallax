# SESSION_CHECKPOINT — 2026-07-30 12:51 +08:00

## 新鲜度自检
- 写入前最新 commit：`ba804ca feat: complete S13 authoritative retrieval`；本次刷新将 amend 进同一提交，因此最终 hash 会变化。
- 读入时对比 `git log --oneline -3`；若 hash 不同但标题一致，以 Git 与当前文件状态为准。

## 当前在做什么
S01～S13 已全部完成并通过门禁；当前没有活动代码切片，下一片为 S14「向量候选召回与冻结工作上下文」，开始前必须完成 G07。

## 下一步（可直接接手）
1. 阅读 `docs/implementation-slices.md` 的 G07、S14、§12，及 `docs/product-spec.md` FR-05/§6.1，声明 S14-1 G07 固定检索基准切片。
2. 以 S13 的全文/时间/关系结果为基线，冻结本地向量模型、索引、重排和 token budget 的可重复质量/性能上限。
3. 若 G07 形成难以逆转的真实权衡，按现有 ADR 模板落档；否则只记录版本化检索契约与固定夹具。
4. 在 `crates/retrieval` 扩展向量/长期记忆通道、邻域与动态窗口，所有最终候选继续回读权威证据或带来源账本。
5. 跑相关测试与全仓门禁，更新 `docs/code-trail.md`、`docs/architecture.md` 后提交并刷新 checkpoint。

## 未提交 / 未完成
- 未提交文件：无（本 checkpoint 刷新将 amend 进 S13 完成提交）。
- 未推送：本地 S01～S13 提交尚未发送到 `origin/main`；仅在本人明确要求时 push。
- S14 尚未开始；没有向量候选、长期记忆通道、重排、动态检索窗口、token budget 或冻结工作上下文消费者。
- 环境：构建继续把 `TEMP/TMP` 与 npm cache 指向 E:，避免 C: 空间不足。

## 已验证基线
- `cargo test --workspace --no-fail-fast`：119/119 通过，含 current/historical、跨通道时间交集、关系召回、跨重启和损坏索引重建。
- `cargo fmt --all -- --check`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo check -p desktop-app --all-targets`：通过。
- `npm test`：持续对话 2/2；`npm run typecheck`、`npm run build`：通过。
- `cargo build --bins --features tauri/custom-protocol --release`：通过。
- `git diff --check`、14 个变更文件高置信秘密模式与 2 个变更 Markdown/93 个本地链接审计：通过。

## 冷启动读序
1. `docs/implementation-slices.md` §2、§3、G07、S14、§12 — 全局不变量、向量门禁、S14 边界与完成判据。
2. `docs/product-spec.md` FR-05、§6.1 — 多路 RAG、权威回读、工作上下文与确定性验收。
3. `CONTEXT.md` — 检索窗口、工作上下文、RAG、向量召回和选择性深度理解术语。
4. `docs/architecture.md` §1、§2、§4.1、§5.5、§6、§7、§8、§9.13 — schema v10、检索编排、来源门禁与 S14 输入边界。
5. `docs/code-trail.md`、`docs/adr/0004-trusted-core-access-boundary.md`、`docs/adr/0018-hybrid-rag-selective-deep-understanding.md`、`docs/adr/0019-stable-evidence-blocks-dynamic-retrieval-windows.md`、`docs/adr/0020-immutable-block-references-explicit-lineage.md`、`docs/adr/0021-canonical-text-anchors-optional-native-locators.md`。
6. `crates/retrieval/src/lib.rs`、`crates/vault/src/repository.rs` 的 retrieval 路径、`crates/vault/src/schema.rs` MIGRATION_10、`crates/vault/tests/retrieval_persistence.rs`、`crates/core/src/domain.rs` 的 `WorkingContext`、`crates/runtime-gateway/src/adapter.rs` — S14 必须扩展和复用的边界。

## 本会话决策摘要
- S13 新边界：`crates/retrieval` 只负责编排与权威候选契约，schema v10 索引由 `VaultRepository` 持久化并可重建，落实 ADR-0003/0004/0018/0019/0020。
- S13 事实资格：索引只返回 `EvidenceBlockRef | ClaimId`；证据认证回读规范文本，账本校验逐字来源后才可返回。
- S13 范围与时间：`current` 仅接受 `PRESENT` 最新版本，`historical` 可返回旧版或 `SOURCE_REMOVED`；显式时间条件与全文/关系通道严格求交集。
