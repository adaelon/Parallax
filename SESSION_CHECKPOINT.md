# SESSION_CHECKPOINT — 2026-07-30 09:08 +08:00

## 新鲜度自检
- 写入前最新 commit：`2266169 feat: complete S10 stable evidence references`；本次刷新将 amend 进同一提交，因此最终 hash 会变化。
- 读入时对比 `git log --oneline -3`；若 hash 不同但标题一致，以 Git 和当前文件状态为准。

## 当前在做什么
S01～S10 已全部完成并通过门禁；当前没有活动代码切片，下一片为 S11「增量修订与显式块谱系」，开始前必须完成 G06。

## 下一步（可直接接手）
1. 阅读 `docs/implementation-slices.md` 的 S11/G06、`docs/product-spec.md` FR-02/§6.1 与 `docs/architecture.md` §4.2/§5.2，声明 G06 第一子片。
2. 建立插入、移动、修改、删除和重复段落歧义固定基准，比较确定性块谱系算法与阈值并冻结 G06 契约。
3. 在可信 Core 定义 `BlockLineage` 与 `UNCHANGED | MOVED | MODIFIED | REMOVED | AMBIGUOUS`，确保旧 `EvidenceBlockRef` 永不改写。
4. 产生只允许 `UNCHANGED/MOVED` 前移的增量工作计划，持久化谱系并覆盖跨重启与歧义测试。
5. 跑相关测试与全仓门禁，更新 `docs/code-trail.md`、`docs/architecture.md` 后提交并刷新 checkpoint。

## 未提交 / 未完成
- 未提交文件：无（本 checkpoint 刷新将 amend 进 S10 完成提交）。
- 未推送：本地 S01～S10 提交尚未发送到 `origin/main`；仅在本人明确要求时 push。
- S11/G06 尚未开始；没有跨修订块谱系、当前来源投影或增量工作计划。

## 已验证基线
- `cargo test --workspace --no-fail-fast`：95/95 通过，含 UTF-8/UTF-16 多语言坐标、非法边界、原子物化、故障回滚、跨重启引用与导航降级。
- `cargo fmt --all -- --check`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo check -p desktop-app --all-targets`：通过。
- `npm test`：持续对话 2/2；`npm run typecheck`、`npm run build`：通过。
- `cargo build --bins --features tauri/custom-protocol --release`：通过。
- `git diff --check`、变更文件秘密模式与变更 Markdown 本地链接审计：通过。

## 冷启动读序
1. `docs/implementation-slices.md` §2、§3、S11、§12 — 全局不变量、G06 门禁、S11 边界与完成判据。
2. `docs/product-spec.md` FR-02、FR-11、§6.1 — 块谱系、历史引用、增量更新与不可信内容边界。
3. `CONTEXT.md` — 证据块、证据块引用、块谱系、规范文本和来源锚点术语。
4. `docs/architecture.md` §1、§2、§3.5、§4.1、§4.2、§5.2、§6、§7、§9.10 — S10 权威块现状与 S11 目标数据流。
5. `docs/code-trail.md`、`docs/adr/0019-stable-evidence-blocks-dynamic-retrieval-windows.md`、`0020-immutable-block-references-explicit-lineage.md`、`0021-canonical-text-anchors-optional-native-locators.md`。
6. `crates/ingestion/src/evidence.rs`、`service.rs`、`crates/vault/src/repository.rs`、`schema.rs`、`crates/vault/tests/evidence_persistence.rs` — S11 输入契约、持久化边界与多语言夹具。

## 本会话决策摘要
- S10 权威引用：规范 Markdown 的唯一 UTF-8 范围负责真实性，UTF-16 只在 API 边界投影，落实 ADR-0021。
- S10 块身份：schema v7 原子保存 Core-owned 修订与有序证据块，重复物化恢复同一不可变引用，落实 ADR-0019。
