# SESSION_CHECKPOINT — 2026-07-29 23:49 +08:00

## 新鲜度自检
- 写入前最新 commit：`fa8ce8a feat: establish S07 tray-resident desktop host`；本次刷新将 amend 进同一提交，因此最终 hash 会变化。
- 读入时对比 `git log --oneline -3`；若 hash 不同但标题一致，以 Git 和当前文件状态为准。

## 当前在做什么
S01～S06 与 S07 的 G04、加密宿主生命周期、thin Tauri 2 宿主均已提交；当前没有活动代码切片，S07 下一子片为持续对话 command 与界面。

## 下一步（可直接接手）
1. 阅读 `docs/implementation-slices.md` 的 S07、`docs/architecture.md` §5.4/§9.7、ADR-0026 与 ADR-0037，声明只接入逐字对话证据的 S07-3 切片。
2. 在 `apps/desktop/src-tauri/src/state.rs` 以有界白名单方法封装 `MemoryCore::run_counterpart_turn` 与对话读取，不向 WebView 暴露 repository、密钥、凭据或通用文件能力。
3. 在 `apps/desktop/src-tauri/src/lib.rs` 注册 `list_conversation`/`send_message`，并在 `apps/desktop/src` 用 React 接入持续对话；普通问答不得自动产生 Claim。
4. 增加 SQLCipher 重启与前端交互测试，运行 Rust workspace、前端、capability 和 Windows release 门禁后提交。

## 未提交 / 未完成
- 未提交文件：无（checkpoint 刷新将 amend 进 S07 宿主提交）。
- 未推送：本地 S01～S07 宿主功能提交尚未发送到 `origin/main`；仅在本人明确要求时 push。
- S07：thin 宿主完成；持续对话 command、重启可读对话和产品界面尚未实现。

## 已验证基线
- `cargo test -p desktop-host -p desktop-app`：宿主 9/9 通过。
- `cargo test --workspace --no-fail-fast`：58/58 通过，含 4 个 SQLCipher 宿主生命周期重启场景。
- `cargo check -p desktop-app --all-targets`、`cargo fmt --all -- --check`：通过。
- `cargo clippy --workspace --all-targets -- -D warnings`：通过。
- `npm test`：静态宿主 1/1；`npm run typecheck`、`npm run build`：通过。
- Tauri capability 审计：仅 `main` + `core:default`，无插件/文件/shell/HTTP/进程/凭据权限。
- `cargo build --bins --features tauri/custom-protocol --release`：通过，产出无 bundle Windows executable。
- `git diff --cached --check`、秘密模式与变更 Markdown 本地链接审计：通过。

## 冷启动阅读顺序
1. `docs/implementation-slices.md` §2、§3、S07、§12 — 全局不变量、S07 边界、G04 和完成门禁。
2. `CONTEXT.md` — 对话证据、宿主运行空缺、事件驱动存在、工作上下文、核心访问权与不可信证据。
3. `docs/architecture.md` §1、§2、§3.1、§3.2、§3.6、§5.1、§5.4、§6、§7、§9.7 — 宿主、对话、生命周期、安全和当前实现边界。
4. `docs/host-lifecycle-v1.md`、`docs/code-trail.md` — G04 固定 contract 与 S01～S07-2 精确触达。
5. `docs/adr/0008-tauri-react-rust-desktop-stack.md`、`0011-trust-current-windows-logon-session.md`、`0012-tray-resident-tauri-host.md`、`0026-retain-every-conversation-turn-as-evidence.md`、`0037-disputed-memory-uses-natural-layered-disclosure.md`、`0049-heartbeated-single-host-lifecycle.md`。
6. `apps/desktop/src-tauri/src/lib.rs`、`state.rs`、`crates/desktop-host/src/lifecycle.rs`、`crates/core/src/memory_loop.rs`、`crates/runtime-gateway/src/adapter.rs` — 当前 native 入口、Core 与运行时边界。

## 本会话决策摘要
- §G04 单宿主生命周期：单实例 Tauri 宿主以加密心跳恢复空缺，并在 Core 安全关闭后安装签名升级（ADR-0049；`docs/host-lifecycle-v1.md`）。
- S07-2 updater：仅在运行时同时提供 HTTPS endpoint 与非空公钥时注册；本片使用静态前端验证宿主，不实现持续对话。
