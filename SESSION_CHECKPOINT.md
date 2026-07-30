# SESSION_CHECKPOINT — 2026-07-30 09:51 +08:00

## 新鲜度自检
- 写入前最新 commit：`7323f67 feat: complete S11 explicit block lineage`；本次刷新将 amend 进同一提交，因此最终 hash 会变化。
- 读入时对比 `git log --oneline -3`；若 hash 不同但标题一致，以 Git 与当前文件状态为准。

## 当前在做什么
S01～S11 已全部完成并通过门禁；当前没有活动代码切片，下一片为 S12「只读 Obsidian 资料源与移除语义」。

## 下一步（可直接接手）
1. 阅读 `docs/implementation-slices.md` 的 S12、`docs/product-spec.md` FR-02/§6.1 与 `docs/architecture.md` §3.4/§5.2，声明 S12-1 只读资料源边界。
2. 建立固定样本笔记库与目录哈希基准，覆盖配置目录/回收站排除、普通文件、移动、删除、根目录离线和重新出现。
3. 新建 `crates/source-obsidian`，实现只读扫描/观察与 `PRESENT | SOURCE_REMOVED`、`AVAILABLE | SOURCE_UNAVAILABLE` 状态转换。
4. 接入 schema v8 `SourceRecord`/通用摄取与 S11 谱系，保存 Properties、标签、别名、链接、嵌入、标题和块关系且绝不写回源目录。
5. 跑相关测试与全仓门禁，更新 `docs/code-trail.md`、`docs/architecture.md` 后提交并刷新 checkpoint。

## 未提交 / 未完成
- 未提交文件：无（本 checkpoint 刷新将 amend 进 S11 完成提交）。
- 未推送：本地 S01～S11 提交尚未发送到 `origin/main`；仅在本人明确要求时 push。
- S12 尚未开始；没有 Obsidian 资料源适配器、来源可用性/移除状态机或关系持久化。

## 已验证基线
- `cargo test --workspace --no-fail-fast`：106/106 通过，含 G06 五类基准、稳定定位器修改、schema v8 回填/回滚、谱系故障回滚、幂等与跨重启历史引用。
- `cargo fmt --all -- --check`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo check -p desktop-app --all-targets`：通过。
- `npm test`：持续对话 2/2；`npm run typecheck`、`npm run build`：通过。
- `cargo build --bins --features tauri/custom-protocol --release`：通过。
- `git diff --check`、22 个变更文件秘密模式与 10 个变更 Markdown 本地链接审计：通过。

## 冷启动读序
1. `docs/implementation-slices.md` §2、§3、S12、§12 — 全局不变量、S12 边界与完成判据。
2. `docs/product-spec.md` FR-02、FR-11、§6.1 — Obsidian 只读导入、来源移除/离线、内容隔离与验收。
3. `CONTEXT.md` — Obsidian 笔记库、资料源移除、增量更新、不可信证据术语。
4. `docs/architecture.md` §1、§2、§3.4、§3.5、§4.1、§4.2、§5.2、§6、§7、§9.11 — schema v8/S11 输入与 S12 目标数据流。
5. `docs/code-trail.md`、`docs/adr/0015-read-only-obsidian-source.md`、`0016-obsidian-source-removal-semantics.md`、`0024-versioned-markdown-dialect.md`。
6. `docs/block-lineage-contract-v1.md`、`crates/ingestion/src/lineage.rs`、`service.rs`、`crates/vault/src/repository.rs`、`schema.rs` — S12 必须复用的谱系、来源版本与持久化边界。

## 本会话决策摘要
- G06 匹配：唯一原生定位器/精确指纹优先，Unicode trigram Dice 使用 `7000/1500 bp` 与 ordinal `±2` 双向唯一门禁，落实 `docs/block-lineage-contract-v1.md`。
- S11 投影：只有 `UNCHANGED/MOVED` 可前移当前投影并复用索引负载；`MODIFIED/REMOVED/AMBIGUOUS` 保留历史并进入复核，落实 ADR-0020。
- S11 持久化：schema v8 原子保存稳定来源版本、不可变谱系、歧义候选和增量工作项；重复调用恢复同一批次，不改写旧 `EvidenceBlockRef`。
