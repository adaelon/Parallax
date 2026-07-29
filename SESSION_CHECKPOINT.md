# SESSION_CHECKPOINT — 2026-07-30 05:18 +08:00

## 新鲜度自检
- 写入前最新 commit：`4f63f45 feat: complete S07 continuous conversation`；本次刷新将 amend 进同一提交，因此最终 hash 会变化。
- 读入时对比 `git log --oneline -3`；若 hash 不同但标题一致，以 Git 和当前文件状态为准。

## 当前在做什么
S01～S07 已全部完成并通过门禁；当前没有活动代码切片，下一片为 S08「Context Inbox 先归档后理解」。

## 下一步（可直接接手）
1. 阅读 `docs/implementation-slices.md` 的 S08、`docs/product-spec.md` FR-02/FR-11 与 `docs/architecture.md` §4.1/§5.2，声明只实现普通文件先归档边界的 S08 首个子片。
2. 在新建 `crates/ingestion` 中定义稳定普通文件、重解析点/设备文件拒绝及 `ARCHIVED`/`ARCHIVED_UNPARSED` 状态契约，并先写状态转换正反例。
3. 在 `crates/vault` 接入 HKDF/XChaCha20-Poly1305 加密 `objects/`、内容去重复用、对象先写与无引用对象清理，并做 SQLCipher 故障注入测试。
4. 通过 S07 白名单 command 接入有界导入入口；验证删除投递原件不删除保险库证据，非 Markdown 稳定进入 `ARCHIVED_UNPARSED(UNSUPPORTED_FORMAT)`。

## 未提交 / 未完成
- 未提交文件：无（checkpoint 刷新将 amend 进 S07 完成提交）。
- 未推送：本地 S01～S07 提交尚未发送到 `origin/main`；仅在本人明确要求时 push。
- S08 尚未开始；`crates/ingestion` 与加密对象库均不存在。

## 已验证基线
- `cargo test --workspace --no-fail-fast`：61/61 通过，含 S07 普通问答零 Claim 与 SQLCipher 重启恢复。
- `cargo check -p desktop-app --all-targets`、`cargo fmt --all -- --check`、`cargo clippy --workspace --all-targets -- -D warnings`：通过。
- `npm test`：持续对话 2/2；`npm run typecheck`、`npm run build`：通过。
- Tauri capability：仅 `main` + `core:default`；invoke handler 精确 8 个领域/宿主 command，无通用文件、shell、HTTP、进程、凭据或 repository 能力。
- `cargo build --bins --features tauri/custom-protocol --release`：通过。
- `git diff --check`、秘密模式与变更 Markdown 本地链接审计：通过。

## 冷启动阅读顺序
1. `docs/implementation-slices.md` §2、§3、S08、§12 — 全局不变量、S08 边界与完成门禁。
2. `docs/product-spec.md` FR-02、FR-11、§6.1 — 先归档状态机、不可信内容隔离与验收。
3. `CONTEXT.md` — Context Inbox、证据、保险库、增量更新、遗忘与不可信证据。
4. `docs/architecture.md` §1、§2、§3.2、§4.1、§5.2、§6、§7、§9.7 — 对象库、摄取流、安全与当前宿主边界。
5. `docs/code-trail.md`、`docs/adr/0007-context-inbox-import-semantics.md`、`0009-hybrid-encrypted-vault-storage.md`、`0013-archive-before-understanding.md`、`0022-content-addressed-encrypted-objects.md`、`0046-vault-cryptographic-profile.md`。
6. `crates/vault/src/crypto.rs`、`repository.rs`、`schema.rs`、`apps/desktop/src-tauri/src/lib.rs`、`state.rs` — 可复用密码原语、事务边界与白名单宿主入口。

## 本会话决策摘要
- S07 持续对话沿用既有 ADR：固定会话逐字保留，发送只冻结最近 32 轮/64 KiB 上下文，普通问答不因保留自动入账（`docs/architecture.md` §5.4/§9.7）。
