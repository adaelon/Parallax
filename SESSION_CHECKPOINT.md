# SESSION_CHECKPOINT — 2026-07-30 08:35 +08:00

## 新鲜度自检
- 写入前最新 commit：`8126aa4 feat: complete S09 bounded markdown parsing`；本次刷新将 amend 进同一提交，因此最终 hash 会变化。
- 读入时对比 `git log --oneline -3`；若 hash 不同但标题一致，以 Git 和当前文件状态为准。

## 当前在做什么
S01～S09 已全部完成并通过门禁；当前没有活动代码切片，下一片为 S10「稳定证据块、规范锚点与逐字引用」。

## 下一步（可直接接手）
1. 阅读 `docs/implementation-slices.md` 的 S10、`docs/product-spec.md` FR-02/§6.1 与 `docs/architecture.md` §3.5/§4.2/§5.2，声明 S10 第一子片。
2. 在可信 Core 定义 `ExtractionRevision`、Core-owned `EvidenceBlock`、UTF-8 `SourceAnchor`、不可变 `EvidenceBlockRef` 与版本化 `MarkdownLocator`，先写中日韩、组合字符、emoji 和非法边界测试。
3. 将 S09 已接受的加密解析产物转换并原子持久化为提取修订与有序证据块；正文必须由同一归档 Markdown 的字节范围逐字取得。
4. 实现 UTF-8 到 UTF-16 UI 范围的确定性投影；原生定位失效只返回 `NATIVE_NAVIGATION_UNAVAILABLE`，不得影响规范引用。
5. 跑相关测试与全仓门禁，更新 `docs/code-trail.md`、`docs/architecture.md` 后提交并刷新 checkpoint。

## 未提交 / 未完成
- 未提交文件：无（checkpoint 刷新将 amend 进 S09 完成提交）。
- 未推送：本地 S01～S09 提交尚未发送到 `origin/main`；仅在本人明确要求时 push。
- S10 尚未开始；没有提取修订、权威证据块、不可变块引用或 UTF-16 投影实现。

## 已验证基线
- `cargo test --workspace --no-fail-fast`：87/87 通过，含固定 Markdown 方言、未知语法降级、五类资源拒绝、SQLCipher 接受/拒绝、故障遗留恢复与跨重启不重试。
- `cargo fmt --all -- --check`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo check -p desktop-app --all-targets`：通过。
- `npm test`：持续对话 2/2；`npm run typecheck`、`npm run build`：通过。
- `cargo build --bins --features tauri/custom-protocol --release`：通过。
- `git diff --check`、变更文件秘密模式与变更 Markdown 本地链接审计：通过。

## 冷启动读序
1. `docs/implementation-slices.md` §2、§3、S10、§12 — 全局不变量、S10 边界与完成判据。
2. `docs/product-spec.md` FR-02、FR-11、§6.1 — 证据块、规范文本、来源坐标与不可信内容边界。
3. `CONTEXT.md` — 证据、证据块、规范文本、来源锚点、原生定位器与不可变引用术语。
4. `docs/architecture.md` §1、§2、§3.5、§4.1、§4.2、§5.2、§6、§7、§9.9 — 解析产物到权威证据块的数据流与 S09 现状。
5. `docs/code-trail.md`、`docs/markdown-contract-v1.md`、`docs/adr/0019-stable-evidence-blocks-dynamic-retrieval-windows.md`、`0021-canonical-text-anchors-optional-native-locators.md`。
6. `crates/markdown/src/lib.rs`、`crates/ingestion/src/domain.rs`、`service.rs`、`crates/vault/src/repository.rs`、`schema.rs`、`crates/vault/tests/markdown_persistence.rs` — S10 输入契约、持久化边界与可复用测试夹具。

## 本会话决策摘要
- G05 Markdown 依赖与方言：事件流 `pulldown-cmark` + literal autolink + 保守 YAML，已落档到 `docs/markdown-contract-v1.md`。
- S09 持久化边界：schema v6 保存版本化解析尝试和加密解析产物，不提前实现 S10 权威证据块，已落档到 `docs/architecture.md` §9.9。
