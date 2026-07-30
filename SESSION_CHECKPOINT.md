# SESSION_CHECKPOINT — 2026-07-30 11:00 +08:00

## 新鲜度自检
- 写入前最新 commit：`868aafb feat: complete S12 read-only Obsidian source`；本次刷新将 amend 进同一提交，因此最终 hash 会变化。
- 读入时对比 `git log --oneline -3`；若 hash 不同但标题一致，以 Git 与当前文件状态为准。

## 当前在做什么
S01～S12 已全部完成并通过门禁；当前没有活动代码切片，下一片为 S13「权威全文、时间与关系检索」。

## 下一步（可直接接手）
1. 阅读 `docs/implementation-slices.md` 的 S13、`docs/product-spec.md` FR-04/FR-05/§6.1 与 `docs/architecture.md` §4.2/§5.5，声明 S13-1 当前/历史检索边界。
2. 建立固定检索基准，覆盖时间冲突、来源归属、`PRESENT | SOURCE_REMOVED` 的 `current | historical` 范围与损坏索引重建。
3. 新建 `crates/retrieval`，实现全文、时间和实体关系候选召回；候选必须解析回权威证据块引用或带来源账本。
4. 接入 schema v9 来源当前性与既有账本，拒绝索引片段直接作为事实；本片不加入向量召回或深度理解投影。
5. 跑相关测试与全仓门禁，更新 `docs/code-trail.md`、`docs/architecture.md` 后提交并刷新 checkpoint。

## 未提交 / 未完成
- 未提交文件：无（本 checkpoint 刷新将 amend 进 S12 完成提交）。
- 未推送：本地 S01～S12 提交尚未发送到 `origin/main`；仅在本人明确要求时 push。
- S13 尚未开始；没有 `crates/retrieval`、真实全文/时间/关系索引或 `current | historical` 查询消费者。
- 环境：C: 当前无可用空间；Cargo 只产生全局缓存 last-use 警告。后续构建继续把 `TEMP/TMP` 与 npm cache 指向 E:。

## 已验证基线
- `cargo test --workspace --no-fail-fast`：114/114 通过，含 S12 扫描、状态持久化、关系、S11 谱系复用和端到端校准。
- `cargo fmt --all -- --check`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo check -p desktop-app --all-targets`：通过。
- `npm test`：持续对话 2/2；`npm run typecheck`、`npm run build`：通过。
- `cargo build --bins --features tauri/custom-protocol --release`：通过。
- `git diff --check`、24 个变更文件高置信秘密模式与 6 个变更 Markdown/87 个本地链接审计：通过。

## 冷启动读序
1. `docs/implementation-slices.md` §2、§3、S13、§12 — 全局不变量、进入门禁、S13 边界与完成判据。
2. `docs/product-spec.md` FR-02、FR-04、FR-05、§6.1 — 来源当前性、时间账本、检索与确定性验收。
3. `CONTEXT.md` — 证据块引用、检索窗口、资料源移除、工作上下文与 RAG 术语。
4. `docs/architecture.md` §1、§2、§4.1、§4.2、§5.2、§5.5、§6、§7、§9.10～§9.12 — 权威数据、schema v9 与检索输入。
5. `docs/code-trail.md`、`docs/adr/0003-temporal-three-ledger-model.md`、`docs/adr/0004-trusted-core-access-boundary.md`、`docs/adr/0016-obsidian-source-removal-semantics.md`、`docs/adr/0018-hybrid-rag-selective-deep-understanding.md`、`docs/adr/0019-stable-evidence-blocks-dynamic-retrieval-windows.md`、`docs/adr/0020-immutable-block-references-explicit-lineage.md`。
6. `crates/ingestion/src/evidence.rs`、`crates/ingestion/src/lineage.rs`、`crates/source-obsidian/src/lib.rs`、`crates/vault/src/repository.rs`、`crates/vault/src/schema.rs`、`crates/core/src/domain.rs`、`crates/core/src/ports.rs` — S13 必须复用的权威引用、来源当前性、账本与持久化边界。

## 本会话决策摘要
- S12 只读边界：仅扫描普通文件，排除配置/回收站且不跟随重解析点；任何不完整校准都禁止提交批量移除，落实 ADR-0015。
- S12 当前性：根离线只记 `SOURCE_UNAVAILABLE`；确认缺失记 `SOURCE_REMOVED`，重新出现恢复 `PRESENT`，历史证据不删除，落实 ADR-0016。
- S12 持久化：schema v9 原子保存来源状态事件与 Obsidian 元数据/原始关系，解析关系为可重建投影；语法边界沿用 ADR-0024。
