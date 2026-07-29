# SESSION_CHECKPOINT — 2026-07-29 22:36 +08:00

## 新鲜度自检
- 写入前最新 commit：`ccfd9ae feat: implement S06 runtime gateway`；本次刷新将 amend 进同一提交，因此最终 hash 会变化。
- 读入时对比 `git log --oneline -3`；若 hash 不同但标题一致，以 Git 和当前文件状态为准。

## 当前在做什么
S01～S06 已提交并通过全量门禁；当前没有活动代码切片，下一片为 S07 托盘常驻桌面壳与持续对话。

## 下一步（可直接接手）
1. 阅读 `docs/implementation-slices.md` 的 S07、G04，以及 ADR-0008、ADR-0011、ADR-0012、ADR-0026、ADR-0037，冻结 Tauri 单实例、自启动、升级、崩溃空缺和显式退出状态机。
2. 检查 `crates/identity/src/presence.rs`、`crates/runtime-gateway/src/transport.rs` 与 `adapter.rs`，声明 WebView 不持有密钥、repository、模型凭据或通用文件能力的 S07 切片。
3. 仅在本人明确要求时运行 `git push origin main`；当前本地 `main` 在 checkpoint amend 前领先远端三个功能提交。

## 未提交 / 未完成
- 未提交文件：无（checkpoint 刷新将 amend 进 S06 提交）。
- 未推送：本地 S01～S06 功能提交尚未发送到 `origin/main`。
- S07：未开始；G04 尚未执行，当前没有红测或未验证代码。

## 已验证基线
- `cargo test -p runtime-gateway`：8/8 通过（适配等价、最小出口、Core 拒绝、HTTP/TLS、降级与持久证据）。
- `cargo test --workspace --no-fail-fast`：44/44 通过。
- `cargo fmt --all -- --check`：通过。
- `cargo clippy --workspace --all-targets -- -D warnings`：通过。
- `git diff --cached --check`：S06 提交前通过。
- 变更文档本地 Markdown 链接检查：通过。

## 冷启动阅读顺序
1. `docs/implementation-slices.md` §2、§3、S06、S07、§12 — 全局不变量、已完成/下一切片、G04 和完成门禁。
2. `CONTEXT.md` — 领域术语，重点是对话证据、事件驱动存在、自我包、工作上下文、核心访问权与不可信证据。
3. `docs/architecture.md` §1、§2、§3.1、§3.2、§3.6、§5.1、§5.4、§6、§7、§9.5、§9.6 — 桌面宿主、运行时网关、生命周期、安全和已实现边界。
4. `docs/runtime-contract-v1.md`、`docs/code-trail.md` — G03 固定 contract 与 S01～S06 精确触达。
5. `docs/adr/0008-tauri-react-rust-desktop-stack.md`、`0011-trust-current-windows-logon-session.md`、`0012-tray-resident-tauri-host.md`、`0026-retain-every-conversation-turn-as-evidence.md`、`0037-disputed-memory-uses-natural-layered-disclosure.md`、`0048-openai-responses-runtime-family.md` — S07 宿主、对话和运行时边界。
6. `crates/identity/src/presence.rs`、`ports.rs`、`crates/runtime-gateway/src/transport.rs`、`adapter.rs`、`fallback.rs`、`crates/core/src/memory_loop.rs` — 当前唤醒、最小数据出口、降级和证据提交契约。

## 本会话决策摘要
- §G03 首个运行时家族：Cloud `gpt-5.6-terra` 与 Local `gpt-oss-20b` 统一采用 OpenAI Responses v1 严格 contract（ADR-0048；`docs/runtime-contract-v1.md`）。
- Cloud 传输强制 HTTPS、非空清零 bearer 且禁止重定向；Local 不携带 Cloud 凭据，认证不进入外发检查记录。
