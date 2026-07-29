# SESSION_CHECKPOINT — 2026-07-30 06:37 +08:00

## 新鲜度自检
- 写入前最新 commit：`374e064 feat: complete S08 context inbox archiving`；本次刷新将 amend 进同一提交，因此最终 hash 会变化。
- 读入时对比 `git log --oneline -3`；若 hash 不同但标题一致，以 Git 和当前文件状态为准。

## 当前在做什么
S01～S08 已全部完成并通过门禁；当前没有活动代码切片，下一片为 S09「Core 内受限 Markdown 方言解析」，其进入门禁 G05 尚未完成。

## 下一步（可直接接手）
1. 阅读 `docs/implementation-slices.md` 的 G05/S09、`docs/product-spec.md` FR-02/FR-11 与 `docs/architecture.md` §3.5/§5.2，先声明 G05 spike，不直接写解析器。
2. 用无个人数据的固定语料比较候选纯 Rust CommonMark/GFM 解析库，冻结块类型、资源上限、Wikilink 消歧和定位器契约到 `docs/markdown-contract-v1.md`。
3. G05 通过后创建受限解析 crate，入口只接收 UTF-8 原文与 `ParseLimits`，先写固定语料、未知语法和资源拒绝测试。
4. 将 `MarkdownParseAttempt(STARTED/ACCEPTED/REJECTED/INTERRUPTED)` 接入 SQLCipher；故障注入验证遗留 `STARTED` 重启后转 `PARSER_INTERRUPTED` 且不自动重试。
5. 从 S08 已归档对象重处理 `.md`；保持非 Markdown 为 `ARCHIVED_UNPARSED(UNSUPPORTED_FORMAT)`，不实现 S10 证据块与索引。

## 未提交 / 未完成
- 未提交文件：无（checkpoint 刷新将 amend 进 S08 完成提交）。
- 未推送：本地 S01～S08 提交尚未发送到 `origin/main`；仅在本人明确要求时 push。
- S09 尚未开始；G05 Markdown 契约与解析 crate 均不存在。

## 已验证基线
- `cargo test --workspace --no-fail-fast`：76/76 通过，含 S08 稳定/超限/设备拒绝、对象认证去重、SQLCipher 故障孤儿清理、删除投递原件后跨重启恢复及缺失对象失败关闭。
- `cargo check -p desktop-app --all-targets`、`cargo fmt --all -- --check`、`cargo clippy --workspace --all-targets -- -D warnings`：通过。
- `npm test`：持续对话 2/2；`npm run typecheck`、`npm run build`：通过。
- Tauri capability：仅 `main` + `core:default`；invoke handler 精确 9 个领域/宿主 command，无通用文件、shell、HTTP、进程、凭据或 repository 能力。
- `cargo build --bins --features tauri/custom-protocol --release`：通过。
- `git diff --check`、秘密模式与变更 Markdown 本地链接审计：通过。

## 冷启动阅读顺序
1. `docs/implementation-slices.md` §2、§3、S09、§12 — 全局不变量、G05 进入门禁、S09 边界与完成判据。
2. `docs/product-spec.md` FR-02、FR-11、§6.1 — Markdown-only、解析状态、资源隔离与确定性验收。
3. `CONTEXT.md` — 证据、规范文本、证据块、来源锚点、保险库与不可信证据。
4. `docs/architecture.md` §1、§2、§3.5、§4.1、§5.2、§6、§7、§9.8 — 解析边界、先归档流、安全不变量与 S08 现状。
5. `docs/code-trail.md`、`docs/adr/0013-archive-before-understanding.md`、`0014-first-readable-file-formats.md`、`0017-on-demand-appcontainer-parser.md`、`0022-v1-markdown-only.md`、`0023-in-process-bounded-markdown-parser.md`、`0024-versioned-markdown-dialect.md`。
6. `crates/ingestion/src/domain.rs`、`service.rs`、`crates/vault/src/object_store.rs`、`repository.rs`、`schema.rs`、`apps/desktop/src-tauri/src/state.rs` — S09 可复用归档状态、对象读取、故障边界与宿主入口。
