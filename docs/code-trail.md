# 代码链路

## 2026-08-04 S06R-5-2 完整重验收与冻结构建

**触达**:
- `.local/system-acceptance/latest.json:eam-system-acceptance-result-v1` — 被忽略的最终结果记录构建来源、18 个通过步骤、矩阵/隐私/链接计数、安装包元数据和安装生命周期；不进入 Git。
- `target/release/bundle/nsis/evrything-about-me_0.1.0_x64-setup.exe` — 从提交 `1468ca57b9919e8dfc08d428b2885770ae66649a` 生成新的 S32 冻结候选；安装包不进入 Git。
- `docs/system-acceptance-v1.md:§8`、`docs/longitudinal-observation-template.md:构建身份` — 统一固定版本、构建来源、字节数与 SHA-256，旧 S31 构建不再是候选。
- `docs/implementation-slices.md:S06R/S06R-5`、`docs/architecture.md:§9.31` — 关闭 S06R 修订系列并把 S32 门禁切换到新冻结构建。

**入口**：仓库根目录执行 `powershell.exe -NoProfile -ExecutionPolicy Bypass -File .\scripts\run-system-acceptance.ps1 -Mode Full`；S32 复制纵向模板后只能安装并观察本条 SHA 对应的产物。
**测试**：针对 `1468ca57b9919e8dfc08d428b2885770ae66649a` 的最终 Full runner 18/18；33 DET、12 FR、50 ADR、8 THR、5 MIG、33 个证据入口，256 个文本文件隐私扫描与 266 个本地链接全绿；NSIS 5,448,409 bytes，记录及独立复算 SHA-256 均为 `824203ffd36bd2ca32957c10719edd21c4bfaeb39c34b108aa27eda418f87242`，隔离安装、启动、schema v26 首次建库、关窗驻留、退出清理与卸载通过。

## 2026-08-04 S06R-5-1 运行时档案系统验收证据

**触达**:
- `scripts/run-system-acceptance.ps1:Assert-RequiredEvidenceCoverage/Test-AcceptanceMatrix` — 在精确集合与证据外键之外，强制 ADR-0053 及相关 DET/FR/THR/MIG 行显式引用运行时档案证据。
- `scripts/tests/run-system-acceptance.tests.ps1:S06R matrix rejection fixtures` — 覆盖 ADR-0053 整行缺失、ADR-0053 未引用运行时档案证据、schema v26 迁移未引用运行时档案证据三类失败关闭。
- `docs/system-acceptance-v1.md:EV-RUNTIME-PROFILE/DET-30..31/FR-07..10/ADR-0053/THR-01,07/MIG-01,03,05` — 注册档案迁移、write-only Key、恢复、严格测试连接、热切换、重启和并发隔离，并把旧 v25/ADR 计数推进到 v26/50。
- `docs/architecture.md:§9.31` — 同步 S06R 必需证据门禁与完整系统验收数据流。

**入口**：仓库根目录执行 `powershell.exe -NoProfile -ExecutionPolicy Bypass -File .\scripts\run-system-acceptance.ps1 -Mode Validate`；Full 模式在同一矩阵门禁后继续全仓验证、打包与安装烟测。
**测试**：脚本自测通过，并证明三类 S06R 缺证据矩阵会拒绝；Vault 档案 3/3、恢复 1/1、desktop host 运行时档案 6/6、React 设置 5/5 及 `Validate` 全绿。

## 2026-08-04 S06R-4 本地设置面板

**触达**:
- `apps/desktop/src/App.tsx:RuntimeProfileView/RuntimeProfileApiKeyChange/App::{openRuntimeSettings,runtimeProfileDraft,testRuntimeProfile,saveRuntimeProfile}/runtimeKeyStatus/redactRuntimeProfileView` — 在 Vault 就绪的持续对话页增加按需读取模态；只投影白名单视图字段，空 Key、输入新 Key、单独清除确认精确映射 `KEEP/REPLACE/CLEAR`，测试保留草稿，保存成功清空 Key 输入并刷新脱敏视图，失败使用固定脱敏提示。
- `apps/desktop/src/App.tsx:App runtime settings focus effects` — Escape 和关闭按钮丢弃临时 Key 草稿并把焦点返回设置入口，读取完成后聚焦 Base URL；操作进行中禁止关闭。
- `apps/desktop/src/styles.css:.runtime-settings-*` — 为设置字段、Key 状态、清除确认、脱敏错误/成功反馈及窄屏动作换行提供本地模态样式。
- `apps/desktop/src/App.test.tsx:S06R-4 local runtime settings` — 覆盖读取不回显、空输入 `KEEP`、输入 `REPLACE`、单独确认 `CLEAR`、测试零保存且不清草稿、保存成功/失败、固定脱敏错误、Escape 关闭和焦点返回。
- `docs/architecture.md:§3.6/§9.6/§9.7`、`docs/implementation-slices.md:S06R-4` — 同步 write-only Key UI 数据流、已解锁入口、失败语义和切片完成状态。

**入口**：Vault 已解锁后，从持续对话页顶部选择“运行时设置”；WebView 只经 `get_runtime_profile`、`test_runtime_profile({ draft })` 与 `save_runtime_profile({ draft })` 访问宿主。
**测试**：桌面 React 19/19、desktop Rust 30/30、`npm run typecheck` 与 `npm run build` 全绿；desktop all-targets Clippy `-D warnings`、`cargo fmt --all -- --check`、Markdown 本地链接 266/266、256 个 tracked text 隐私扫描、静态权限边界和 `git diff --check` 通过。

## 2026-08-04 S06R-3 宿主热切换与严格测试连接

**触达**:
- `apps/desktop/src-tauri/src/state.rs:RuntimeProfileDraft/RuntimeProfileView/ManagedHost::{get_runtime_profile,test_runtime_profile,save_runtime_profile}/HostCore::open_with_key/runtime_from_profile/save_runtime_profile_from_core` — 宿主从 Vault 单档案启动；测试只发送固定合成严格分类；保存按候选构造、Vault 提交、无失败运行时替换顺序执行。
- `crates/runtime-gateway/src/transport.rs:HttpResponsesTransport::new` — 在任何可能失败的 HTTP client 构造前把候选 bearer 包进清零内存，覆盖测试与热切换候选的失败路径。
- `crates/core/src/memory_loop.rs:MemoryCore::replace_runtime` — 在不移动 repository 与 clock 的前提下提供不可失败的活动运行时交换原语。
- `apps/desktop/src-tauri/src/lib.rs:{get_runtime_profile,test_runtime_profile,save_runtime_profile}`、`apps/desktop/src-tauri/Cargo.toml:zeroize` — 注册三个领域白名单 command；完整 Key 草稿只反序列化进 Rust，并在析构时清零，响应只使用脱敏视图或固定测试结果。
- `apps/desktop/src-tauri/src/state/tests/runtime_profile.rs` — 覆盖测试零持久化/零切换、错误脱敏、保存失败保留旧状态、保存后下一请求与重启、新旧配置并发不混用。
- `docs/architecture.md:§3.6/§9.6/§9.7`、`docs/host-lifecycle-v1.md:§5/§6` — 同步单档案启动、宿主锁顺序、WebView command 与测试门禁。

**入口**：WebView 可调用 `get_runtime_profile`、`test_runtime_profile({ draft })`、`save_runtime_profile({ draft })`；实际密钥读取、严格请求、Vault 更新和运行时替换全部留在 `ManagedHost` 单写锁内。
**测试**：desktop 30/30、Core 42/42、runtime-gateway 27/27、Vault 116/116；四 crate all-targets Clippy `-D warnings` 通过。

## 2026-08-04 S06R-2 SQLCipher 单档案与 write-only 密钥

**触达**:
- `crates/vault/src/schema.rs:MIGRATION_26/migrate_with_hook` — schema v26 创建单例 `runtime_profiles`，迁移默认环回 Base URL、`gpt-oss-20b` 与无 Key，并保持逐版事务回滚。
- `crates/vault/src/repository.rs:RuntimeProfile/RuntimeProfileView/RuntimeProfileKeyAction/VaultRepository::{runtime_profile,runtime_profile_view,update_runtime_profile}` — 分离可信宿主完整读取与命令脱敏视图，以显式 `KEEP/REPLACE/CLEAR` 原子更新档案。
- `crates/runtime-gateway/src/transport.rs:validate_responses_bearer_token` — 抽取 S06R-1 Key 边界；Vault 同时复用 `RuntimeTarget::new`，不复制 Base URL、模型或 Key 校验规则。
- `crates/vault/tests/runtime_profile_persistence.rs:v25_reopens_through_v26_with_one_default_runtime_profile/replace_keep_and_clear_are_durable_while_the_view_stays_redacted` — 覆盖 v25→v26、重启、三态 Key、空白/控制/超限拒绝与视图不回显。
- `crates/vault/tests/backup_recovery.rs:encrypted_snapshot_round_trips_authority_and_rebuilds_indexes` — 合成 Key 随加密 `self.db`/Recovery Set 恢复，且源/恢复数据库、快照与删除头原始字节不可搜索。
- `crates/vault/tests/{capture_persistence,claim_correction_persistence,encrypted_repository,identity_persistence,memory_persistence,reflection_persistence,self_bundle_persistence,shared_experience_persistence,windows_unlock}.rs:schema_version assertions` — 既有重启/恢复路径的预期 schema 从 v25 精确推进到 v26，不改动行为断言。

**入口**：可信宿主打开 `VaultRepository` 后可读取完整档案构造运行时；S06R-3 的 WebView command 必须只暴露 `RuntimeProfileView`，保存必须传入 Base URL、模型和一种显式密钥动作。
**测试**：Vault 116/116、runtime-gateway 27/27；v26 迁移中断、档案事务中断、Recovery Set 恢复与原始字节扫描通过；两 crate all-targets Clippy `-D warnings`、`cargo fmt --all -- --check` 和 `git diff --check` 全绿。

## 2026-08-04 S06R-1 Responses Runtime Contract v2 与可配置目标

**触达**:
- `crates/runtime-gateway/src/transport.rs:RuntimeTarget::new/HttpResponsesTransport::new` — 自有 Base URL/模型，拒绝非环回 HTTP、凭据/query/fragment/空白，并以可选清零 bearer 和无重定向 client 发送。
- `crates/runtime-gateway/src/adapter.rs:OpenAiResponsesRuntime::invoke` — 由规范化 Base URL 追加 `/responses`，把自定义模型写入请求和无凭据外发记录。
- `crates/runtime-gateway/tests/runtime_contract.rs:runtime_target_accepts_remote_https_and_loopback_http_only` — 覆盖远程 HTTPS、环回 HTTP、非法 URL、自定义模型、可选 Key、3xx 拒绝和 v1 夹具等价。
- `apps/desktop/src-tauri/src/state.rs:configured_runtime` — 仅适配 v2 构造 API，保留 S06 旧固定双档案直至 S06R-3。
- `docs/runtime-contract-v2.md:G03 Runtime Contract v2`、`docs/architecture.md:§3.6/§9.6` — 冻结配置、传输、脱敏错误与后续切片边界。

**入口**：`OpenAiResponsesRuntime` 构造合法 `RuntimeTarget` 后，分类或回应调用在适配器内派生 `<base_url>/responses` 并经 `ResponsesTransport` 发送；Bearer Key 只由具体传输持有。
**测试**：`cargo test -p runtime-gateway` 27/27、`cargo test -p desktop-app` 24/24；runtime-gateway Clippy `-D warnings`、desktop all-targets check、Markdown 本地链接 266/266、254 个 tracked text 隐私扫描与静态安全边界、`git diff --check` 全绿。

## 2026-08-04 S06R-0 可配置运行时档案决策与切片

**触达**:
- `CONTEXT.md:推理运行时/运行时后端/运行时档案` — 固定模型执行能力与第二自我身份、个人上下文及 Core 权限的领域边界。
- `docs/adr/0053-vault-backed-configurable-responses-runtime-profile.md`、`docs/adr/0048-openai-responses-runtime-family.md` — 接受 Vault 单档案、write-only Key 与热切换，并只取代旧 ADR 的固定档案/模型部分。
- `docs/implementation-slices.md:S06R-1..S06R-5` — 按 Contract v2、SQLCipher 档案、宿主热切换、设置 UI、系统重验收拆成五个独立门禁。
- `docs/system-acceptance-v1.md:§8`、`SESSION_CHECKPOINT.md` — 把旧 S31 构建降为历史证据并锁定 S32，直到新边界完成重验收。

**入口**：本人确认可自由填写 Responses Base URL、模型 ID 和可选 Bearer Key；本片仅落设计，不改变可执行路径。
**测试**：Markdown 本地链接 263/263、隐私扫描 253 个文本文件零违规、`git diff --check` 通过；旧 S31 矩阵按预期拒绝缺少证据的 ADR-0053，S06R-5 前不得恢复为绿。

## 2026-08-03 S31-2 完整系统验收与可安装构建

**触达**:
- `scripts/run-system-acceptance.ps1:Invoke-SystemAcceptanceMain` — 从矩阵、隐私和静态边界一路执行到全仓门禁、NSIS 构建与隔离安装烟测，18 个步骤全部通过。
- `docs/system-acceptance-v1.md:§8` — 固定最终通过时间、覆盖计数、版本、安装包大小、SHA-256 与安装生命周期结果。
- `target/release/bundle/nsis/evrything-about-me_0.1.0_x64-setup.exe` — 生成供 S32 冻结使用的本地 NSIS 安装程序，不把安装包或本机结果 JSON 纳入 Git。

**入口**：仓库根目录执行 `powershell.exe -NoProfile -ExecutionPolicy Bypass -File .\scripts\run-system-acceptance.ps1 -Mode Full`；结果写入被忽略的 `.local/system-acceptance/latest.json`。
**测试**：针对 S31 构建来源 `425f5ffa3af679818c9fafa6992ec58192282906` 的完整 runner 18/18；Rust workspace、fmt、Clippy、desktop all-targets、React 14/14、浏览扩展 10/10、两套 TypeScript/生产构建、矩阵/隐私/链接、NSIS 构建及安装/启动/关窗保活/退出清理/卸载烟测全部通过；安装包 `0.1.0`、5,424,551 bytes、SHA-256 `8adbb4d0ef0a2880caca62ec40f370d5ec6766eacf801c2c30a01c1f5c02baed`。

## 2026-08-03 S31-1 首次启动 Vault 闭环与烟测纠偏

**触达**:
- `crates/vault/src/key_store.rs:PreparedVault/VaultKeyStore::prepare` — 将首次密钥生成拆成确认前纯内存准备与确认后原子提交，保持 `bundle.meta` v1 不变。
- `apps/desktop/src-tauri/src/state.rs:ManagedHost::{open,initialize_vault,confirm_recovery_key_saved}` — 区分未初始化、待确认、就绪和真实锁定状态，确认前拒绝全部 Core 操作。
- `apps/desktop/src-tauri/src/lib.rs:{initialize_vault,confirm_recovery_key_saved}`、`apps/desktop/src/App.tsx:VaultSetup` — 增加两条白名单命令和一次性恢复密钥确认界面，不开放重读、文件或凭据权限。
- `scripts/run-system-acceptance.ps1:Invoke-InstallerSmoke`、`crates/vault/examples/seed_installer_smoke_vault.rs` — 仅预置 DPAPI 元数据，并要求安装版 exe 实际创建 `self.db`。
- `docs/adr/0052-one-time-recovery-key-webview-ceremony.md`、`docs/architecture.md:§3.7/§9.3/§9.7/§9.31` — 记录 Recovery Key 的窄 WebView 例外、零写入门禁与烟测判据。

**入口**：默认 `%LOCALAPPDATA%/me.evrything.about/vault` 不存在时进入首次创建；已有 `bundle.meta` 仍只走 DPAPI 日常解锁。
**测试**：`cargo test -p vault --test windows_unlock` 6/6；`cargo test -p desktop-app` 24/24；桌面 React 14/14 与 TypeScript 检查通过；验收脚本分支测试和烟测预置示例通过。

## 2026-08-03 S30-4 全仓门禁与实现级收口

**触达**:
- `crates/vault/src/backup.rs:VaultBackup::restore`、`tests/backup_recovery.rs:encrypted_snapshot_round_trips_authority_and_rebuilds_indexes` — 收窄恢复发布边界：历史 Recovery Set 保持字节不变，新 Vault 使用新集合，消除跨目录伪原子窗口。
- `docs/backup-recovery-v1.md:§3/§4`、`docs/architecture.md:§5.8/§6.2/§7/§9.30` — 对齐三代保留、删除头、恢复重放、轮换、安全不变量和失败降级。
- `docs/code-trail.md:S30-1～S30-4` — 以 G09、红测试、加密快照、恢复轮换和全仓验证闭合 S30。

**入口**：S30-1～S30-3 的完整 diff 经恢复攻击矩阵、全仓行为测试、静态检查、桌面/扩展构建和文档链接审计后进入提交。
**测试**：Rust workspace 287/287；Vault 全套 109/109；fmt、workspace Clippy `-D warnings`、desktop all-targets check；桌面 React 13/13、浏览扩展 10/10、两套 TypeScript 与生产构建全部通过；5 个变更 Markdown 的本地链接和 `git diff --check` 通过。

## 2026-08-03 S30-3 恢复重放与 Vault Key 轮换

**触达**:
- `crates/vault/src/backup.rs:VaultBackup::restore` — 在同卷 staging 中认证快照/删除头、校验组和水位、重放新删除意图，只向空目标原子发布且不改写历史 Recovery Set。
- `crates/vault/src/repository.rs:replay_backup_deletion/rebuild_retrieval_after_restore/rotate_after_restore` — 对存在目标执行 S19 闭包、对缺失目标写水位 tombstone，重建检索后 SQLCipher rekey 并重加密对象 ID。
- `crates/vault/src/key_store.rs:install_rotated_metadata` — 用本人原 Recovery Key 包装新 Vault Key，并刷新当前 Windows 用户 DPAPI 副本。
- `crates/vault/tests/backup_recovery.rs:latest_deletion_head_prevents_an_old_backup_from_reviving_forgotten_evidence` — 覆盖旧快照内目标删除、快照外目标 tombstone、current/historical 拒绝和 ID 不复用。

**入口**：可信 Core 以 `snapshot + deletion-head.eam + RecoveryKey + 空目标目录` 调用 `VaultBackup::restore`；本片不新增桌面目录选择 UI。
**测试**：S30 恢复矩阵 4/4、Vault 全套 109/109；正常、snapshot/head 截断与篡改、旧备份、缺对象、外部明文扫描、三代保留、索引重建和轮换均通过。

## 2026-08-03 S30-2 一致性加密快照与删除头

**触达**:
- `crates/backup/src/lib.rs:seal_envelope/encode_snapshot/encode_deletion_head` — 固定有界 XChaCha20-Poly1305 envelope、精确文件摘要、组 ID、代次和有序删除记录协议。
- `crates/vault/src/backup.rs:VaultBackup::create/synchronize_deletions` — 以 SQLite online backup 获取一致数据库，认证所有引用对象，原子发布删除头和不可变快照并保留最近三代。
- `crates/vault/src/repository.rs:create_backup_snapshot/backup_deletion_records`、`crypto.rs:VaultKey::backup_key` — 在单写者边界导出加密数据库/对象，并以 HKDF `info="backup"` 隔离外层密钥。

**入口**：可信 Core 向保险库外的专用 Recovery Set 调用 `VaultBackup::create`；确认遗忘后调用 `synchronize_deletions` 刷新同组删除头。
**测试**：`cargo test -p backup` 2/2；`cargo test -p vault --test backup_recovery` 4/4；相关 Clippy `-D warnings` 通过。

## 2026-08-03 S30-1 G09 恢复协议与攻击矩阵

**触达**:
- `docs/backup-recovery-v1.md:§1～§5` — 冻结三代保留、Backup Key 派生、Recovery Set、删除顺序重放、staging 发布和恢复即轮换。
- `docs/adr/0051-recovery-set-deletion-head.md` — 固定历史快照必须携带同组最新加密删除头，否决独立旧快照与明文墓碑。
- `crates/vault/tests/backup_recovery.rs:S30 recovery matrix` — 先以缺少 `VaultBackup`/`InvalidBackup` 的编译失败锁定新行为，再由实现转绿。

**入口**：S30 实现只能使用 G09 已冻结术语和矩阵，不扩展到平台托管恢复、丢钥匙绕过、增量归档或法证级擦除。
**测试**：红阶段 `cargo test -p vault --test backup_recovery` 按预期因新 API 缺失失败；实现后同命令 4/4 通过。

## 2026-08-03 S29-4 权限审计与全仓收口

**触达**:
- `apps/browser-extension/tests/manifest.test.ts:Manifest V3 permission and origin audit`、`scripts/verify-dist.mjs` — 逐项比对实际 `chrome.*` API、manifest 权限、固定扩展 ID 与生产入口资源。
- `crates/capture-browser/src/http.rs:loopback_origin_is_pinned_to_the_manifest_public_key_id` — 把环回来源锁到 manifest 公钥派生 ID，拒绝生产扩展与宿主常量漂移。
- `crates/vault/tests/{capture_persistence,claim_correction_persistence,encrypted_repository,identity_persistence,memory_persistence,reflection_persistence,self_bundle_persistence,shared_experience_persistence,windows_unlock}.rs` — 随 schema v25 更新既有重启测试的新鲜度断言，不放宽业务行为。
- `docs/architecture.md:§5.3/§6.2/§9.29`、`docs/host-lifecycle-v1.md:§5/§6` — 对齐扩展存储、授权撤销、环回边界与不新增 WebView command 的最终实现。

**入口**：S29-1～S29-3 的完整 diff 经协议、持久化、扩展行为、权限/API、生产目录和全仓门禁后进入提交。
**测试**：Rust 全仓 281/281；fmt、workspace Clippy `-D warnings`、desktop all-targets check；桌面 13/13、扩展 10/10、两套 TypeScript 与生产构建全部通过；独立 Chrome profile 以 unpacked `dist` 启动，CDP 观察到固定 ID 的 `service-worker.js` target。

## 2026-08-03 S29-3 Manifest V3 TypeScript 扩展

**触达**:
- `apps/browser-extension/public/manifest.json` — 固定扩展 ID，只声明 `alarms/scripting/storage/tabs`、环回 host permission 与 HTTP(S) 可选来源权限。
- `apps/browser-extension/src/contracts.ts:webTabSnapshot/finalizeVisit/retainAuthorizedPageContent/enqueueBounded` — 约束浏览元数据、精确来源正文授权/撤销、稳定去重与 128 项/4 MiB 队列上限。
- `apps/browser-extension/src/service-worker.ts:transitionToTab/reconcileStoredAuthorizations/flushQueue` — 会话内追踪聚焦标签页，以本地持久队列承接失败并按宿主收据幂等出队。
- `apps/browser-extension/src/popup.ts`、`tests/contracts.test.ts` — 本人按当前来源显式授权或撤销正文，并覆盖元数据、授权正文、来源撤销、去重和上限。

**入口**：加载 `apps/browser-extension/dist` 后，service worker 监听标签页/窗口边界；popup 只管理当前 HTTP(S) 来源的可选正文权限。
**测试**：Vitest 10/10、TypeScript 通过；Vite 生产构建及 `verify-dist.mjs` 通过，可直接以 unpacked extension 加载 `dist`。

## 2026-08-03 S29-2 浏览事件加密持久化

**触达**:
- `crates/vault/src/schema.rs:MIGRATION_25` — 新增不可改写的浏览访问元数据、当前宿主会话外键与可选正文证据引用。
- `crates/vault/src/repository.rs:VaultRepository::record_browser_submission_with_hook` — 在当前开放宿主会话下原子、幂等写入访问记录，并把授权正文先归档为不可信证据。
- `apps/desktop/src-tauri/src/state.rs:ManagedHost::record_browser_submission`、`src/lib.rs:spawn_browser_capture` — 独立环回线程只借用当前开放宿主会话提交，绑定或扩展断开不阻断 Core。
- `crates/vault/tests/browser_capture_persistence.rs` — 覆盖 SQLCipher 重启、相同提交重试、冲突重试、正文归档和陈旧会话拒绝。

**入口**：桌面宿主独立线程运行固定来源环回适配器，把已校验事件交给 `ManagedHost` 和 `BrowserCaptureRepository`；本片不新增 WebView command，也不接扩展代码。
**测试**：Vault 持久化 2/2、Vault lib 41/41、桌面 Rust 23/23，相关 Clippy 与 desktop all-targets check 通过；覆盖事务回滚/孤儿回收、宿主关闭拒绝和 `core:default` capability 不扩张。

## 2026-08-03 S29-1 固定来源环回会话协议

**触达**:
- `crates/capture-browser/src/domain.rs:BrowserSubmission::from_payload` — 约束 URL、标题、访问/停留区间及按来源授权的不可信页面正文。
- `crates/capture-browser/src/http.rs:handle_http_request/serve_loopback` — 固定扩展来源、仅环回地址与随机宿主会话令牌共同守住有界提交入口。
- `docs/adr/0050-pinned-origin-loopback-browser-capture.md` — 冻结环回会话通道并否决 WebView command、无认证 localhost 与首版 Native Messaging。

**入口**：固定 ID 的 Manifest V3 扩展先从 `/v1/session` 取得仅存于当前宿主进程的令牌，再向 `/v1/browser-events` 提交结构化浏览事件。
**测试**：`cargo test -p capture-browser` 7/7；`cargo clippy -p capture-browser --all-targets -- -D warnings` 通过，覆盖远程、错误来源、伪造令牌、越限、未授权正文拒绝和固定 ID 一致性。

## 2026-08-03 S28-4 全仓门禁与实现级收口

**触达**:
- `crates/capture-windows/src/native.rs:imp` — 按 rustfmt 收拢唯一 Win32 unsafe 适配器的导入排版，不改变采样行为。
- `docs/architecture.md:§4.1/§5.3/§9.28`、`docs/host-lifecycle-v1.md:§5/§6` — 复核 schema v24、采集恢复、白名单 command 与当前代码一致且无需新增 ADR。
- `docs/code-trail.md:S28-1～S28-4` — 以文件、符号、入口和确定性验证结果闭合 S28 三段实现链路。

**入口**：S28-1～S28-3 的完整 diff 经行为测试、静态检查、桌面/前端构建、capability 与本地文档链接审计后进入提交。
**测试**：Rust 全仓 269/269；fmt、Clippy `-D warnings`、桌面 all-targets；React 13/13、TypeScript 与生产构建；5 个变更 Markdown 的 113 个本地文件链接和 `core:default` capability 审计通过。

## 2026-08-03 S28-3 Win32 采样、宿主接线与活动时间线

**触达**:
- `crates/capture-windows/src/native.rs:sample_foreground_activity` — 在唯一 unsafe 边界读取可执行文件名、窗口标题、输入空闲计时与会话锁定状态，不读取输入内容或屏幕。
- `apps/desktop/src-tauri/src/state.rs:record_capture_sample/set_capture_paused/shutdown`、`lib.rs:spawn_capture` — 恢复采集状态、每秒检查点、关窗继续、暂停/恢复及退出空缺进入同一加密时间线。
- `apps/desktop/src/App.tsx:ActivityTimelineEntry/activityTimelineOpen/toggleCapturePause`、`App.test.tsx:S28 Windows activity timeline` — 通过白名单 command 展示活动与有原因空缺，并提供显式暂停/继续。
- `docs/host-lifecycle-v1.md:§5/§6`、`docs/architecture.md:§5.3/§9.28` — 对齐新增 command、Win32 最小权限边界、宿主恢复与端到端数据流。

**入口**：托盘宿主启动 `eam-windows-capture` 线程，将 Win32 最小元数据转换为 `CaptureCheckpoint`；WebView 只能查询投影或切换本人暂停状态。
**测试**：Capture 6/6、Vault capture 4/4、Vault lib 39/39、桌面 Rust 22/22、React 13/13；TypeScript 与生产构建通过。

## 2026-08-03 S28-2 schema v24 活动时间线与崩溃恢复

**触达**:
- `crates/vault/src/schema.rs:MIGRATION_24` — 新增单开放段约束的 `capture_spans`，区分活动元数据与六类采集空缺。
- `crates/vault/src/repository.rs:ActivityTimelineRepository` — 原子合并连续检查点、拒绝陈旧宿主会话，并从最后观测点恢复崩溃空缺。
- `crates/vault/tests/capture_persistence.rs` — 覆盖连续合并、空闲切段、暂停跨重启、崩溃边界与陈旧会话拒绝。

**入口**：宿主先建立加密 `HostSession`，再恢复采集开放段；每个 `CaptureCheckpoint` 只由当前开放宿主会话写入 SQLCipher。
**测试**：`cargo test -p vault --test capture_persistence` 3/3，通过真实关闭、重开与 schema v24 迁移。

## 2026-08-03 S28-1 Windows 活动采集状态机

**触达**:
- `crates/capture-windows/src/domain.rs:ActivitySnapshot/CaptureSpanKind/CaptureGapReason` — 冻结有界前台元数据、活动区间与六类显式采集空缺。
- `crates/capture-windows/src/engine.rs:CaptureStateMachine` — 以确定性转换处理连续活动、窗口切换、空闲、暂停、锁屏、源不可用与退出。
- `crates/capture-windows/tests/state_machine.rs` — 覆盖连续合并边界、空闲切段、暂停拒收、锁屏恢复与停止拒绝。

**入口**：Windows 适配器或确定性测试向 `CaptureStateMachine` 提交前台观测与生命周期信号，得到带边界判定的 `CaptureCheckpoint`。
**测试**：`cargo test -p capture-windows --test state_machine` 5/5；`cargo clippy -p capture-windows --all-targets -- -D warnings` 通过。

## 2026-08-02 S27-4 全仓门禁与实现级收口

**触达**:
- `crates/memory/src/service.rs:validate_maturity_target/validate_maturity_new_support/validate_maturity_counter_evidence/validate_maturity_review/validate_maturity_discussion` — 按资格阶段拆开唯一校验矩阵，保持原拒绝顺序并满足 Clippy 结构门禁。
- `crates/core/src/memory_loop.rs:validate_response_citations`、`crates/vault/src/repository.rs:validate_dispute_revision_link` — 提取既有纯校验步骤，使 S27 接线后继续满足 100 行函数上限。
- `docs/architecture.md:§5.4/§5.6/§9.27`、`docs/code-trail.md:S27-1～S27-4` — 对齐模式形成、成熟、运行时提交、反例修正与遗忘全链路。

**入口**：S27-1～S27-3 的完整实现 diff 经全仓行为测试、静态检查、桌面/前端构建、链接与结构化夹具审计后进入提交。
**测试**：Rust 全仓 257/257、fmt、Clippy `-D warnings`、桌面 all-targets；Runtime contract 23/23、React 12/12、TypeScript 与生产构建通过；2 个变更 Markdown 的 110 个本地文件链接及 5 个成熟 JSON 外层/嵌套载荷通过。

## 2026-08-02 S27-3 Runtime 成熟提议与真实提交

**触达**:
- `crates/runtime-gateway/src/adapter.rs:PatternMaturityOperation/parse_pattern_maturity_operation/pattern_maturity_operation_schema` — 将完整 `propose_pattern_maturity` 加入严格结构化白名单，保留未知操作拒绝。
- `crates/core/src/domain.rs:PatternMaturityReceipt/PatternMaturityWriteRejection/TurnOutcome`、`ports.rs:MemoryRepository::commit_pattern_maturity`、`memory_loop.rs:persist_pattern_maturity_proposals` — 每轮至多提交一个提议，显式返回成功或资格/重复拒绝。
- `crates/memory/src/service.rs:commit_pattern_maturity`、`crates/vault/src/repository.rs:MemoryRepository::commit_pattern_maturity` — 生产 Vault 适配器复用唯一 Memory 资格矩阵并提交 schema v23 后继版本，Core 不反向依赖 Memory。
- `crates/runtime-gateway/tests/runtime_contract.rs`、`tests/fixtures/pattern-maturity-*.json` — 覆盖严格缺字段、未知记忆、双方讨论不合格、重复操作和成功写入。

**入口**：`MemoryCore::run_counterpart_turn` 解析 Runtime 白名单操作后，经可信 Vault 适配器交给 `eam_memory::commit_pattern_maturity`；没有长期记忆适配器的 Core 仓储默认关闭该写入。
**测试**：Runtime contract 23/23；成功路径写 `SUPPORTED_COUNTERPART_VIEW` 版本 2 与一条完整成熟记录，未知/不合格保持 `PROVISIONAL_PATTERN`，重复不产生第三版本。

## 2026-08-02 S27-2 schema v23、成熟记录与遗忘闭包

**触达**:
- `crates/vault/src/schema.rs:MIGRATION_23` — 扩展记忆/争议状态约束，新增初始反例复查、成熟记录、新增支持与分类证据表及来源索引。
- `crates/vault/src/repository.rs:append_pattern_maturity_with_hook/validate_persisted_pattern_maturity/load_pattern_maturity_records` — 事务内重检资格，原子写旧版本取代、稳定看法后继和完整有序提议记录，并在重启时恢复。
- `crates/vault/src/repository.rs:conversation_forget_plan/delete_conversation_memory_closure`、`tests/memory_persistence.rs:pattern_maturity_survives_reopen_with_complete_qualification_evidence` — 把初始/再次复查和讨论证据纳入 S19 删除闭包。

**入口**：Memory 领域校验后的 `ValidatedPatternMaturityProposal` 交给 `LongTermMemoryRepository::append_pattern_maturity`；遗忘任一资格证据从同一 SQLCipher 事务删除完整派生记忆。
**测试**：schema v23 中断恢复、成熟提交前故障回滚、完整记录重启恢复、`SUPPORTED_COUNTERPART_VIEW -> DISPUTED -> WEAKENED` 与遗忘后重启不可见均通过；Vault 全套 93/93。

## 2026-08-02 S27-1 模式门槛与成熟资格状态机

**触达**:
- `crates/memory/src/domain.rs:MemoryStatus/PatternMaturityRecord/ValidatedPatternMaturityProposal` — 增加 `SUPPORTED_COUNTERPART_VIEW`、`WEAKENED`、成熟记录与显式后继版本结构。
- `crates/memory/src/service.rs:validate_initial_pattern/validate_pattern_maturity` — 初始模式要求三个跨时间独立事件和第二自我反例复查；成熟要求新增独立支持、再次复查、双方讨论、精确引用与当前版本。
- `crates/memory/tests/pattern_maturity.rs` — 固定二事件、同源重复、缺复查、资格满足但未提议、逐项资格拒绝、成功成熟与强反例削弱矩阵。

**入口**：`MemoryMaintenance::propose` 只创建 `PROVISIONAL_PATTERN`；只有第二自我显式调用 `MemoryMaintenance::mature_pattern` 才尝试稳定看法后继。
**测试**：模式成熟领域测试 5/5；资格输入本身不改变状态，合法提议始终保留第二自我记忆归属。

## 2026-08-02 S26-5 全仓门禁与实现级收口

**触达**:
- `crates/core/src/memory_loop.rs:immediate_safety_fixture_matches`、`crates/core/tests/reflection_invitations.rs:counterpart_quote_cannot_impersonate_person_immediate_safety_evidence` — 固定即时风险只接受属于本人的精确证据，第二自我同文历史不能越过静默。
- `apps/desktop/src-tauri/src/state.rs:with_reflection_opportunity/person_topic_reentry_reaches_muted_reflection_as_discuss_only` — 证明本人重提从真实桌面入口进入 `discuss_only`，并保持静默状态。
- `docs/reflection-scheduling-v1.md`、`docs/architecture.md`、`docs/code-trail.md` — 对齐 G08 证据归属、生产入口和 S26 完整数据流。

**入口**：S26-1～S26-4 的完整实现 diff 经全仓行为测试、静态检查、桌面/前端构建、链接与夹具审计后进入提交。
**测试**：Rust 全仓 245/245、fmt、Clippy `-D warnings`、桌面 all-targets；React 12/12、TypeScript 与生产构建通过；5 个 Markdown 的 162 个本地链接、JSON 外层/嵌套载荷与 G08 10×9 TSV 通过。

## 2026-08-02 S26-4 桌面延后与静默仪式

**触达**:
- `apps/desktop/src-tauri/src/state.rs:ReflectionInvitationView/with_reflection_opportunity/list_offered_reflection_invitations_from_core/decide_reflection_invitation_from_core` — 可信识别本人直接重提的议题，只投影 `OFFERED` 邀请及逐字依据，并把延后、静默、完成决定交回 Core。
- `apps/desktop/src-tauri/src/lib.rs:list_offered_reflection_invitations/decide_reflection_invitation` — 登记两个最小 Tauri 白名单 command，不向 WebView 暴露仓储能力。
- `apps/desktop/src/App.tsx:decideReflectionInvitation/reflection invitation dialog`、`App.test.tsx:S26 deferrable reflection invitation ceremony` — 展示观察、`why_now`、重要性与证据；第二次提出显示一次静默选择，首次提出不主动询问静默。

**入口**：启动恢复或对话成功后读取所有 `OFFERED` 邀请；本人在仪式中选择稍后、停止主动提起或本次已谈完。
**测试**：桌面 Rust `offered_reflection_projection_requires_a_whitelisted_core_decision`、`person_topic_reentry_reaches_muted_reflection_as_discuss_only` 覆盖可信投影、未知决定拒绝、静默认知保留与本人重提；React 12/12 与 TypeScript 通过。

## 2026-08-02 S26-3 运行时结构化邀请与出站审计

**触达**:
- `crates/runtime-gateway/src/adapter.rs:ReflectionRuntimeInput/parse_reflection_invitation_operation/reflection_invitation_operation_schema` — 向模型发送单个已调度邀请及 offer/discuss-only 语义，严格解析有来源的新邀请操作。
- `crates/runtime-gateway/src/transport.rs:OutboundContextSource::ReflectionInvitation` — 审计外发邀请 ID 与其逐字证据 ID，不暴露仓储能力。
- `crates/runtime-gateway/tests/fixtures/reflection-invitation-response.json`、`runtime_contract.rs:runtime_receives_one_scheduled_reflection_and_emits_a_strict_sourced_invitation` — 固定已有邀请输入、新邀请输出、schema 白名单与精确 disclosure。

**入口**：`MemoryCore::run_counterpart_turn` 把冻结的 `ReflectionRuntimeContext` 交给统一模型网关；模型只能通过 `propose_reflection_invitation` 提交 Core 再验证的候选。
**测试**：Runtime contract 19/19、Runtime Gateway 与 Vault Clippy `-D warnings` 通过。

## 2026-08-02 S26-2 schema v22、重启延后与遗忘闭包

**触达**:
- `crates/vault/src/schema.rs:MIGRATION_22` — 新增反思邀请与逐字证据表、状态约束、到期索引及未解决议题部分唯一索引。
- `crates/vault/src/repository.rs:ReflectionInvitationRepository/load_reflection_invitation/delete_reflection_invitation_closure` — 在 SQLCipher 事务中追加和 CAS 转换邀请，重启恢复完整调度状态，遗忘来源时删除派生邀请。
- `crates/vault/tests/reflection_persistence.rs`、`repository.rs:reflection_forget_failure_rolls_back_evidence_and_invitation_across_reopen` — 覆盖延后重启、议题唯一、证据外键原子回滚、遗忘闭包与提交前故障全回滚。

**入口**：Core 经 `ReflectionInvitationRepository` 提交或转换已验证邀请；Vault 打开时迁移到 schema v22 并恢复所有未遗忘邀请。
**测试**：反思持久化 4/4、遗忘故障注入 1/1、Vault 全套 93/93、`cargo check -p vault --lib` 通过。

## 2026-08-02 S26-1 G08 调度契约与反思邀请 Core 状态机

**触达**:
- `docs/reflection-scheduling-v1.md:G08 Reflection Scheduling Contract v1`、`crates/core/tests/fixtures/g08-reflection-schedule.tsv` — 冻结 7 天延后、24 小时主动频率、单机会一个邀请、32 个未解决预算与固定即时风险边界。
- `crates/core/src/domain.rs:ReflectionInvitation/reflection_delivery/offer_reflection_invitation/decide_reflection_invitation` — 建模 `PENDING -> OFFERED -> DEFERRED | MUTED_BY_PERSON | RESOLVED`，本人重提只允许讨论，固定即时风险可越过静默。
- `crates/core/src/ports.rs:ReflectionInvitationRepository`、`in_memory.rs:ReflectionInvitationRepository` — 定义邀请创建、compare-and-swap 转换、认知保留与遗忘闭包。
- `crates/core/src/memory_loop.rs:run_counterpart_turn/persist_reflection_invitation_proposals/immediate_safety_fixture_matches` — 在冻结自然时机中选择一个邀请，运行时成功后才消费提出机会，并拒绝无来源、模式冒充、重复议题及不属于本人的伪即时风险。
- `crates/core/tests/reflection_invitations.rs` — 覆盖时间重放、无关任务排队、相关话题提出、重复延后、静默、本人重提、固定安全例外、S27 边界与遗忘。

**入口**：`WorkingContext::with_reflection_opportunity` 冻结当前机会；运行时可提交一个结构化 `ReflectionInvitationProposal`，本人通过 `MemoryCore::decide_reflection_invitation` 延后、静默或完成。
**测试**：Core 反思邀请 5/5、Core 全套 45/45、`cargo clippy -p core --all-targets -- -D warnings` 通过。

## 2026-08-02 S25-4 全仓门禁与实现级收口

**触达**:
- `crates/runtime-gateway/src/adapter.rs:parse_shared_experience_operation` — 提取既有共同经历纯解析分支，使新增身份操作后 `parse_turn_response` 继续满足 100 行结构门禁且 contract 不变。
- `docs/architecture.md:§5.4/§9.4/§9.5/§9.25`、`docs/code-trail.md:S25-1～S25-4` — 对齐领域、Self Bundle、运行时、Vault 与桌面只读链路。
- `crates/core/tests/identity_evolution.rs`、`crates/runtime-gateway/tests/runtime_contract.rs`、`crates/vault/tests/identity_persistence.rs`、`apps/desktop/src/App.test.tsx` — 汇总当前前驱、使命拒绝、模型切换、原子回滚与只读界面验收。

**入口**：S25-1～S25-3 的完整实现 diff 经全仓行为测试、静态检查、桌面/前端构建、链接与 JSON 夹具审计后进入提交。
**测试**：Rust 全仓 231/231、fmt、Clippy `-D warnings`、桌面 all-targets、React 10/10、TypeScript 与生产构建通过；`git diff --check`、3 个变更 Markdown 的 104 个本地链接与 1 个 JSON 夹具通过。

## 2026-08-02 S25-3 运行时身份出口与桌面只读版本链

**触达**:
- `crates/runtime-gateway/src/adapter.rs:IdentityRuntimeInput/parse_identity_revision_operation/identity_revision_operation_schema` — 只发送当前身份与版本指针，严格解析六类可选变化、作者、使命、理由和证据；未知操作继续 fail closed。
- `crates/runtime-gateway/src/transport.rs:OutboundContextSource::IdentityState` — 把身份版本作为可审计的结构化出口来源，不暴露仓储能力。
- `apps/desktop/src-tauri/src/state.rs:list_identity_history_from_core/IdentityStateView`、`lib.rs:list_identity_history` — 新增唯一身份 command，固定投影版本、六类身份状态、理由、证据与形成时间。
- `apps/desktop/src/App.tsx:identityHistory`、`App.test.tsx:S25 immutable identity history` — 展示不可改写版本链，只有查看与关闭，无身份写入 command、输入框或编辑控件。

**入口**：每次真实 runtime 响应可提交一个 `propose_identity_revision`；本人从桌面“身份版本”打开可信只读历史。
**测试**：Runtime contract 18/18、桌面 Rust 19/19、React 10/10、TypeScript 与生产构建通过。

## 2026-08-02 S25-2 身份与 Self Bundle 原子持久化

**触达**:
- `crates/identity/src/ports.rs:IdentityRepository::all_identity_states`、`service.rs:identity_history`、`in_memory.rs:append_identity_state` — 把首版身份仓储扩为连续不可改写版本链与完整历史读取。
- `crates/vault/src/repository.rs:IdentityEvolutionRepository/commit_identity_revision` — 复核当前身份、Self Bundle 与宪法指针，在单个 SQLCipher 事务中追加身份并推进 Self Bundle。
- `crates/vault/src/repository.rs:load_identity_states/recheck_identity_revision_versions` — 重启时校验连续前驱链，提交前在事务内再次拒绝过期版本。
- `crates/vault/tests/identity_persistence.rs` — 覆盖身份与 Self Bundle 同链重启、逐字证据恢复及身份证据外键失败时的原子回滚。

**入口**：Core 把已验证 `IdentityRevisionCommit` 交给 Vault；重启和模型切换从 `current_identity_context` 加载同一身份/Self Bundle 指针。
**测试**：Vault 身份持久化 3/3、Core 模型切换回归通过；schema 保持 v21，不新增迁移。

## 2026-08-02 S25-1 身份修订领域契约与 Core 门禁

**触达**:
- `crates/core/src/domain.rs:IdentityRevisionProposal/IdentityStateSnapshot/IdentityRevisionRejectionReason` — 建模当前前驱、六类身份变化、宪法/使命状态、不可改写快照与精确拒绝原因。
- `crates/core/src/ports.rs:IdentityEvolutionRepository`、`in_memory.rs:commit_identity_revision` — 定义当前身份/Self Bundle 读取、原子提交和历史查询边界，并提供确定性内存适配器。
- `crates/core/src/memory_loop.rs:persist_identity_revisions/validate_identity_revision` — 只接受第二自我针对当前版本的单个真实变化，拒绝本人角色卡、旧前驱、宪法变化、放弃反思使命、冒充本人、空理由及越界证据。
- `crates/core/tests/identity_evolution.rs` — 固定正向追加、关键拒绝矩阵与运行时切换后继续同一版本链。

**入口**：`MemoryCore::run_counterpart_turn` 冻结当前身份给 runtime，并在保存第二自我逐字回应后独立校验结构化修订。
**测试**：Core 身份演化 3/3；正例推进身份/Self Bundle，所有关键拒绝均保持原版本链。

## 2026-07-31 S24-4 全仓门禁、迁移基线与实现级收口

**触达**:
- `crates/vault/tests/claim_correction_persistence.rs`、`encrypted_repository.rs`、`identity_persistence.rs`、`memory_persistence.rs`、`self_bundle_persistence.rs`、`windows_unlock.rs` — 将库重启后的 schema 基线同步为 v21，不放宽其余恢复断言。
- `crates/core/src/memory_loop.rs:record_counterpart_agreement_withdrawal` — 提取单次退出落账并把活动约定查找改为显式错误，满足无 panic 与函数长度门禁。
- `crates/runtime-gateway/src/adapter.rs:parse_agreement_withdrawal_operation`、`crates/vault/tests/shared_experience_persistence.rs:persist_person_withdrawal_with_breach` — 分离严格解析与测试准备链路，保持原行为并通过 Clippy。
- `docs/architecture.md:§9.24`、`docs/code-trail.md:S24-1～S24-4` — 对齐领域、schema、运行时、桌面和非对称仪式的完整真实链路。

**入口**：S24-1～S24-3 的完整实现 diff 经全仓行为测试、静态检查、桌面/前端构建、链接与 JSON 夹具审计后进入提交。
**测试**：Rust 全仓 224/224、fmt、Clippy `-D warnings`、桌面 all-targets、React 9/9、TypeScript 与生产构建通过。

## 2026-07-31 S24-3 运行时独立退出与桌面非对称仪式

**触达**:
- `crates/runtime-gateway/src/adapter.rs:parse_turn_response/response_schema` — 白名单解析独立 `withdraw_shared_agreement`，要求活动约定 Claim 与必填理由；缺字段作为无效响应 fail closed，未知操作维持零写入拒绝。
- `apps/desktop/src-tauri/src/state.rs:list_active_shared_agreements_from_core/withdraw_shared_agreement_as_person_from_core/list_shared_experience_ceremonies_from_core` — 只投影当前活动约定，接通本人确认退出，并从可信退出事件展示行为方、生效时间与理由。
- `apps/desktop/src-tauri/src/lib.rs:list_active_shared_agreements/withdraw_shared_agreement_as_person` — 注册两个窄白名单 command，不向 WebView 暴露仓储或通用 Core 能力。
- `apps/desktop/src/App.tsx:openAgreementManager/confirmPersonWithdrawal/acknowledgeCounterpartWithdrawal` — 取消防误触确认时零调用，确认时理由可空；第二自我退出通知只提供“已知悉/继续回应”，没有否决入口。
- `crates/runtime-gateway/tests/runtime_contract.rs`、`apps/desktop/src-tauri/src/state.rs`、`apps/desktop/src/App.test.tsx` — 固定严格 JSON、活动清单、本人确认和第二自我不可否决通知。

**入口**：第二自我在相关冻结工作上下文中提交结构化退出；本人从桌面“管理共同约定”选择活动约定并经确认弹窗调用窄 command。
**测试**：Runtime contract 17/17、桌面 Rust 18/18、React 9/9 与 TypeScript 类型检查通过。

## 2026-07-31 S24-2 schema v21、重启恢复与遗忘闭包

**触达**:
- `crates/vault/src/schema.rs:MIGRATION_21` — 扩展共同经历 kind 并新增退出表，约束本人理由可选、第二自我理由必填，迁移保持单事务回滚。
- `crates/vault/src/repository.rs:commit_agreement_withdrawal/all_shared_experiences` — 原子持久化并恢复退出 actor、生效时间、理由和共享 Claim 证据，重复或非活动目标拒绝。
- `crates/vault/src/repository.rs:delete_conversation_claim_closure` — 遗忘时先删除退出关系，再删除共同经历和原约定 Claim，维持同一递归删除闭包。
- `crates/vault/tests/shared_experience_persistence.rs`、`crates/vault/src/schema.rs` — 覆盖重启、旧违约保留、生效前后投影、遗忘闭包与 v21 中断迁移。

**入口**：Core 通过 `SharedExperienceRepository::commit_agreement_withdrawal` 写入本人确认或第二自我结构化退出；重启由 Vault 还原同一领域事件。
**测试**：Vault 共同经历 7/7、schema 迁移 23/23 通过。

## 2026-07-31 S24-1 独立退出领域契约与未来投影

**触达**:
- `crates/core/src/domain.rs:AgreementWithdrawal/agreement_is_active_at` — 新增独立退出 actor、理由、证据与生效时间模型，并把已生效退出纳入活动约定判定。
- `crates/core/src/memory_loop.rs:withdraw_shared_agreement_as_person/persist_agreement_withdrawals` — 本人仅在确认后退出，第二自我仅凭响应中逐字非空理由立即退出，均写为不可否决共同经历。
- `crates/core/src/in_memory.rs:commit_agreement_withdrawal` — 原子校验活动目标、行为方证据、原约定支持与重复退出，不复用违约或取代路径。
- `crates/retrieval/src/lib.rs:project_active_relational_constraints` — 从退出生效时间起停止原约定未来投影，保留生效前投影与全部历史。
- `crates/core/tests/shared_experiences.rs`、`crates/retrieval/tests/relational_constraints.rs` — 固定未确认不退出、理由缺失拒绝、立即不可否决退出及生效前后投影。

**入口**：本人结构化确认调用 `withdraw_shared_agreement_as_person`；第二自我在冻结工作上下文中提交独立 `AgreementWithdrawalProposal`。
**测试**：Core 共同经历 13/13、Retrieval 关系约束 4/4 通过。

## 2026-07-31 S23-4 全仓门禁、实现级 diff 审计与文档对齐

**触达**:
- `crates/core/src/domain.rs:agreement_terms/conflict_detection_ignores_generic_relational_scope_words` — 固定冲突检测不会因“双方/共同”等通用关系词误判无关范围。
- `crates/core/src/in_memory.rs:collect_shared_agreement_forget_closure` — 提取候选、确认 Claim 与取代边的遗忘闭包收集，保持原行为并满足 Clippy 函数长度门禁。
- `crates/retrieval/tests/relational_constraints.rs:whole_supersession_changes_only_future_projection_and_keeps_compatible_agreements` — 固定替代约定终止后旧约定也不会复活，兼容约定继续并行。
- `docs/architecture.md:§4.1/§5.5/§9.23` — 对齐 schema v20 有序取代边、三阶段活动目标复核、未来投影与桌面可信展示的真实数据流。

**入口**：S23-1～S23-3 的完整实现 diff，经格式化、全仓测试、静态检查、桌面/前端构建、链接与 JSON 夹具审计后进入提交。
**测试**：Rust 全仓 216/216、fmt、Clippy `-D warnings`、桌面 all-targets、React 7/7、TypeScript 与生产构建通过；`git diff --check`、3 个变更 Markdown 的 101 个本地链接和 3 个 JSON 夹具通过。

## 2026-07-31 S23-3 运行时取代契约与桌面完整仪式

**触达**:
- `crates/runtime-gateway/src/adapter.rs:TurnInput/parse_turn_response/response_schema` — 在严格 JSON 中收发 `supersedes_agreement_ids[]`，要求冲突候选重述保留义务并禁止残余推导，同时审计被取代 Claim。
- `apps/desktop/src-tauri/src/state.rs:list_shared_experience_ceremonies_from_core/revise_shared_agreement_from_core` — 从可信候选和共同账本解析旧约定 Claim、表述、范围及结构化起止时间，并把取代清单保留到本人修订的新版本。
- `apps/desktop/src-tauri/src/lib.rs:revise_shared_agreement` — 以白名单 command 传递结构化 Claim ID 清单，不向 WebView 暴露仓储。
- `apps/desktop/src/App.tsx:App` — 签署前展示全部取代目标的 Claim、表述、范围及结构化起止时间，明确生效后无残余义务且历史保留。
- `crates/runtime-gateway/tests/runtime_contract.rs`、`apps/desktop/src-tauri/src/state.rs`、`apps/desktop/src/App.test.tsx` — 固定真实 contract、可信投影、修订保留和界面清单。

**入口**：相关对话把当前活动约定送入运行时；冲突候选携带旧 Agreement Claim ID，经 Core/Vault 校验后成为本人可检查的最终签署仪式。
**测试**：Runtime contract 15/15、桌面 Rust 17/17、React 7/7 与 TypeScript 类型检查通过。

## 2026-07-31 S23-2 schema v20、重启历史与遗忘闭包

**触达**:
- `crates/vault/src/schema.rs:MIGRATION_20` — 以有序候选→旧共同约定 Claim 外键表持久化整份取代关系，并保持迁移单事务回滚。
- `crates/vault/src/repository.rs:SharedExperienceRepository/load_shared_agreement_candidate` — 原子写入、恢复并在最终签署时复核取代目标仍有效。
- `crates/vault/src/repository.rs:plan_conversation_forget/delete_conversation_claim_closure` — 遗忘旧约定支持时沿取代边递归删除依赖候选、确认 Claim 与派生历史，避免悬空或复活。
- `crates/vault/tests/shared_experience_persistence.rs`、`crates/vault/src/schema.rs` — 覆盖重启、取代前后查询、旧违约保留、遗忘闭包与 v20 中断迁移。

**入口**：Core 暂存或修订共同约定候选时把完整取代 Claim ID 清单交给 Vault；最终确认和重启恢复均只信任 schema v20 关系。
**测试**：Vault 共同经历 6/6、schema 迁移 22/22 通过。

## 2026-07-31 S23-1 冲突检测与显式整份取代契约

**触达**:
- `crates/core/src/domain.rs:SharedAgreementRevision/SharedAgreementCandidate/shared_agreements_conflict` — 把被取代约定 Claim ID 固定进不可变候选，并以范围、有效期、义务词项和显式否定极性保守识别直接冲突。
- `crates/core/src/memory_loop.rs:validate_agreement_supersession` — 拒绝未完整声明的冲突及非活动取代目标，禁止从旧约定推导残余义务。
- `crates/retrieval/src/lib.rs:project_active_relational_constraints` — 仅从新约定生效时起整份停止旧约定未来投影，兼容约定继续并行。
- `crates/core/tests/shared_experiences.rs`、`crates/retrieval/tests/relational_constraints.rs` — 固定未声明拒绝、兼容并行、取代前后投影和历史不删除。

**入口**：运行时提出带完整边界的共同约定候选；Core 在暂存前对有效约定执行保守冲突门禁，检索在冻结任务关系约束时应用已签署取代关系。
**测试**：Core 共同经历 11/11、Retrieval 关系约束 3/3 通过。

## 2026-07-31 S22-4 相关性误召回修正与收尾审计

**触达**:
- `crates/retrieval/src/lib.rs:project_active_relational_constraints` — 复合约定范围要求至少两个不同任务词项重合，避免单个通用双字词触发无关关系约束。
- `crates/retrieval/tests/relational_constraints.rs:only_scope_relevant_current_confirmed_agreements_are_projected` — 固定“个人复盘”不得加载“共同项目复盘”约束的回归边界。
- `docs/architecture.md:§5.5` — 对齐真实相关性门禁与 S22 冻结工作上下文路径。

**入口**：桌面对话在检索冻结后投影活动关系约束；复合范围只有满足双词项相关性门禁才进入运行时。
**测试**：Rust 全仓 208/208、fmt、Clippy `-D warnings`、桌面 all-targets、React 6/6、TypeScript 与生产构建通过；`git diff --check` 和 3 个变更 Markdown 的 101 个本地链接通过。

## 2026-07-31 S22-3 schema v19、重启遗忘与桌面可信展示

**触达**:
- `crates/vault/src/schema.rs:MIGRATION_19`、`crates/vault/src/repository.rs:SharedExperienceRepository` — 持久化原约定 Claim、不可空偏离理由和 `AgreementBreach`，保持 Claim/共同经历原子写入及遗忘闭包。
- `apps/desktop/src-tauri/src/state.rs:send_message_with_retrieval/with_relational_constraints/list_shared_experience_ceremonies_from_core` — 检索冻结后附加相关约束，只从可信 Core/Vault 投影偏离通知。
- `apps/desktop/src/App.tsx:App/ceremonyTitle` — 以不可否决仪式展示被偏离的约定 Claim、第二自我理由和双方证据。
- `crates/vault/tests/shared_experience_persistence.rs`、`crates/vault/src/schema.rs`、`apps/desktop/src-tauri/src/state.rs`、`apps/desktop/src/App.test.tsx` — 覆盖重启、遗忘、迁移中断、真实宿主投影与界面关闭语义。

**入口**：相关桌面对话经检索 Context Builder 获得活动约束；有效偏离由 Core/Vault 入账后随现有共同历史仪式队列返回 React。
**测试**：Vault 共同经历 5/5、schema v19 中断迁移、桌面 Rust 定向测试、React 6/6 与 TypeScript 类型检查通过。

## 2026-07-31 S22-2 运行时关系约束与偏离出口

**触达**:
- `crates/runtime-gateway/src/adapter.rs:TurnInput/parse_turn_response/response_schema` — 外发活动约束及固定优先级，审计约定 Claim，并白名单解析带明确理由的偏离操作。
- `crates/core/src/domain.rs:RelationalConstraintDeparture/SharedExperience`、`crates/core/src/memory_loop.rs:MemoryCore::persist_relational_constraint_departures` — 拒绝无理由、非活动或重复偏离，把有效偏离连同原约定双方证据写成 `AgreementBreach` 共同经历。
- `crates/runtime-gateway/tests/runtime_contract.rs`、`crates/runtime-gateway/tests/fixtures/relational-constraint-departure-response.json` — 固定请求层级、结构化出口、审计来源和端到端入账。

**入口**：运行时收到冻结工作上下文中的活动关系约束；确需偏离时在自然响应中逐字说明理由并提交对应约定 Claim ID。
**测试**：Runtime contract 活动约束与带理由偏离定向测试通过；Core 关系约束 3/3 覆盖有效与无理由偏离。

## 2026-07-31 S22-1 活动关系约束投影与 Core 门禁

**触达**:
- `crates/core/src/domain.rs:ActiveRelationalConstraint/WorkingContext` — 固定次于宪法、安全和行动授权的单一优先级，并拒绝快照外有效期或重复约束。
- `crates/retrieval/src/lib.rs:project_active_relational_constraints` — 只把范围与任务相关、已确认、当前有效且可回到共同约定 Claim 的候选投影为活动约束。
- `crates/core/tests/relational_constraints.rs`、`crates/retrieval/tests/relational_constraints.rs` — 覆盖相关/无关任务、有效期、孤儿记录及约定不得授予写入能力。

**入口**：桌面 Context Builder 以当前任务检索查询、约定候选和共同经历账本调用投影，再把结果附加到冻结工作上下文。
**测试**：Core 关系约束 3/3、Retrieval 关系约束 2/2 通过。

## 2026-07-31 S21-4 全仓门禁与实现级 diff 审计

**触达**:
- `crates/core/src/domain.rs`、`crates/core/src/memory_loop.rs` — 收紧候选构造与最终签署错误路径，保持无 panic 的显式拒绝。
- `docs/architecture.md:§4.2/§5.4/§9.21` — 对齐真实候选版本链、RuntimeRequest 精确同意和 S21 已完成边界。
- `docs/code-trail.md:S21-1～S21-4` — 记录 Core、Vault、运行时/桌面和全仓验证四个可接手切片。

**入口**：S21 完整纵向路径从真实运行时约定提议或本人仪式修改进入，经 Core/Vault 双签状态机回到桌面最终签署。
**测试**：Rust 全仓 198/198、Clippy `-D warnings`、fmt、桌面 target、React 5/5、TypeScript 与生产构建通过；`git diff --check` 和 101 个本地 Markdown 链接审计通过。

## 2026-07-31 S21-3 真实运行时与桌面版本双签仪式

**触达**:
- `crates/runtime-gateway/src/adapter.rs:TurnInput/parse_turn_response/response_schema` — 向真实运行时提供待同意候选精确版本，要求约定边界，并白名单解析第二自我精确版本同意操作。
- `apps/desktop/src-tauri/src/state.rs:list_shared_experience_ceremonies_from_core/revise_shared_agreement_from_core` — 从可信 Core 投影候选版本、范围、有效期和终止项，以白名单路径提交本人结构化修订。
- `apps/desktop/src-tauri/src/lib.rs:revise_shared_agreement` — 暴露最小候选修订 command，不向 WebView 提供仓储或任意写能力。
- `apps/desktop/src/App.tsx:App/terminationText`、`apps/desktop/src/styles.css` — 展示全部签署边界，无终止项明确持续条件，并把本人修改提交为等待第二自我同意的新版本。
- `crates/runtime-gateway/tests/runtime_contract.rs`、`apps/desktop/src-tauri/src/state.rs`、`apps/desktop/src/App.test.tsx` — 固定真实 contract、可信投影、修改后重签和持续条件文案。

**入口**：初次运行时约定提议直接形成 v1 最终签署仪式；本人从仪式修改后生成待第二自我同意的 vN+1，后续对话精确同意该版本才恢复最终签署按钮。
**测试**：runtime contract 13/13、桌面 Rust host 15/15、React 5/5 与 TypeScript 类型检查通过。

## 2026-07-31 S21-2 schema v18 与版本双签持久化

**触达**:
- `crates/vault/src/schema.rs:MIGRATION_18` — 增加候选版本链、范围、有效期、终止项、待第二自我同意及双方签署时间，并使旧 S20 无边界候选不可继续签署。
- `crates/vault/src/repository.rs:SharedExperienceRepository` — 原子提交本人修订与旧版退役、绑定第二自我精确同意、按候选有效期签署 Claim，并在遗忘时递归清除版本后继及确认 Claim。
- `crates/vault/tests/shared_experience_persistence.rs` — 覆盖候选边界重启恢复、修订版本链、精确双签和前代证据遗忘闭包。
- `crates/vault/src/schema.rs:schema::tests` — 固定 v18 中断迁移回滚后可重新打开并完成升级。

**入口**：Core 通过扩展后的 `SharedExperienceRepository` 提交候选、修订、精确同意和最终签署；Vault 在同一 SQLCipher 事务内维护候选状态与签署证据。
**测试**：Vault 共同约定持久化 4/4、schema 迁移 20/20 通过。

## 2026-07-31 S21-1 Core 候选版本与精确双签状态机

**触达**:
- `crates/core/src/domain.rs:SharedAgreementCandidate/SharedAgreementRevision/SharedAgreementAssent` — 为候选增加不可变版本链、范围、有效期、终止项及第二自我精确版本同意契约。
- `crates/core/src/memory_loop.rs:MemoryCore::revise_shared_agreement/persist_shared_agreement_assents/resolve_shared_agreement` — 拒绝缺边界候选，本人修改生成待第二自我同意的新版本，精确同意后才允许最终签署。
- `crates/core/src/in_memory.rs:SharedExperienceRepository` — 原子模拟旧版退役、新版暂存、同意证据追加与候选内容/签署状态校验。
- `crates/core/tests/shared_experiences.rs` — 覆盖缺范围/生效时间拒绝、开放终止项、旧版不可改写、错误版本同意拒绝和精确版本双签。

**入口**：运行时初次提出带完整边界的约定，或本人从仪式提交结构化修改；待第二自我候选会随下一轮 `RuntimeRequest` 进入精确同意路径。
**测试**：`cargo test -p core --test shared_experiences --no-fail-fast`，9/9 通过。

## 2026-07-31 S20-4 全仓门禁与实现级 diff 审计

**触达**:
- `crates/runtime-gateway/src/adapter.rs:ORDINARY_RESPONSE_INSTRUCTIONS/HIGH_IMPACT_RESPONSE_INSTRUCTIONS` — 把四类狭义关系事件、双方立场/参与门禁及“移除第二自我后仍成立则排除”判据送入真实运行时分类契约。
- `crates/core/src/in_memory.rs:validate_shared_support` — 与 Vault 对齐，拒绝只由同一参与方多段证据构成的共同历史。
- `apps/desktop/src/App.tsx:App.submitMessage` — 发送失败时同时重载对话和仪式，恢复此前已原子落盘的候选或非否决通知。
- `crates/core/tests/shared_experiences.rs`、`crates/runtime-gateway/tests/runtime_contract.rs`、`apps/desktop/src/App.test.tsx` — 固定同方证据拒绝、运行时分类指令和失败后仪式重发现。

**入口**：真实模型先按狭义关系事件契约决定是否提交结构化提议；Core/Vault 再执行双方逐字证据门禁，桌面失败恢复始终回到可信持久状态。
**测试**：Core 共同经历 7/7、运行时 contract 12/12、React 4/4 与 TypeScript 类型检查通过；最终全仓门禁见本片 checkpoint。

## 2026-07-31 S20-3 桌面分类型仪式可信状态路径

**触达**:
- `apps/desktop/src-tauri/src/state.rs:list_shared_experience_ceremonies_from_core/resolve_shared_agreement_from_core/dismiss_shared_experience_ceremony_from_core` — 只从 Core/Vault 投影候选和已入账通知，解析双方逐字证据并保持关闭通知不删 Claim。
- `apps/desktop/src-tauri/src/lib.rs:list_shared_experience_ceremonies/resolve_shared_agreement/dismiss_shared_experience_ceremony` — 以三个白名单 command 暴露仪式读取、确认/暂缓和非否决关闭。
- `apps/desktop/src/App.tsx:App.resolveCeremony` — 共同约定显示确认入账/暂不记录；实质分歧、关系变化和共同完成只允许知悉关闭。
- `apps/desktop/src/App.test.tsx` — 固定候选与分歧视图，验证准确表述、双方原话、命令参数及无否认入口。

**入口**：应用启动恢复未处理仪式，或 `send_message` 返回本轮新仪式；React 只能提交候选 ID/共享 Claim ID，不能构造或改写领域记录。
**测试**：`cargo test -p desktop-app --lib --no-fail-fast` 14/14、`npm test -- --run` 4/4、`npm run typecheck` 通过。

## 2026-07-31 S20-2 schema v17 与 Vault 共同经历持久化

**触达**:
- `crates/vault/src/schema.rs:MIGRATION_17` — 新增未入共享账本的约定候选、双方逐字支持及已入账共同经历/仪式关闭状态，迁移保持单事务回滚。
- `crates/vault/src/repository.rs:SharedExperienceRepository` — 原子暂存候选、确认或暂缓、直接写共享 Claim、关闭通知且不撤销历史，并在重启后恢复全部状态。
- `crates/vault/src/repository.rs:plan_conversation_forget/delete_conversation_claim_closure` — 把未确认候选和已入账共同经历纳入对话证据遗忘闭包。
- `crates/vault/tests/shared_experience_persistence.rs` — 覆盖候选/确认/分歧/通知/遗忘的 SQLCipher 跨重启语义。

**入口**：Core 只经 `SharedExperienceRepository` 写 schema v17；候选在本人确认前没有 `claims` 行，非约定关系事件以共享 Claim 与独立仪式状态同事务提交。
**测试**：`cargo test -p vault --test shared_experience_persistence --no-fail-fast` 3/3、schema 19/19、S19 遗忘定向与持久化回归通过。

## 2026-07-31 S20-1 共同经历分类与 Core 入账门禁

**触达**:
- `crates/core/src/domain.rs:SharedExperienceKind/SharedExperienceProposal/SharedAgreementCandidate` — 冻结共同约定、实质分歧、关系变化和共同完成重要事情四类领域值及候选/入账结果。
- `crates/core/src/memory_loop.rs:MemoryCore::run_counterpart_turn/resolve_shared_agreement` — 验证本人逐字证据与当前第二自我原话，约定等待本人仪式，其他关系事件直接写共享账本。
- `crates/core/src/in_memory.rs:SharedExperienceRepository` — 原子模拟候选确认、暂缓、共享 Claim 入账与通知关闭不删历史。
- `crates/runtime-gateway/src/adapter.rs:parse_turn_response/response_schema` — 将 `propose_shared_experience` 加入结构化输出白名单并保持未知操作拒绝。

**入口**：模型运行时只能提交四类结构化共同经历提议；Core 解析双方证据后返回候选或已入账共享 Claim，普通自由文本不触发该路径。
**测试**：`cargo test -p core --no-fail-fast` 20/20、`cargo test -p runtime-gateway --test runtime_contract --no-fail-fast` 12/12，覆盖普通问答/本人外部经历拒绝、约定确认与暂缓、实质分歧直接入账、缺失双方证据拒绝及运行时白名单。

## 2026-07-30 S19-3 交叉回归加固与全仓门禁

**触达**:
- `crates/vault/src/repository.rs:plan_conversation_forget/plan_archived_forget` — 先按真实外键依赖收集对话 Claim/记忆/争议/身份闭包与稳定归档来源全版本闭包，再按依赖顺序删除。
- `crates/vault/src/repository.rs:next_identifier_with_deletion_watermark` — 把已提交删除意图纳入对话证据与归档 ID 水位，阻止重启后复用遗忘目标 ID。
- `crates/vault/tests/forget_persistence.rs` — 交叉覆盖理解投影、多版本来源、纠错 Claim 链、长期记忆、争议、current/historical 双关闭、共享对象与跨重启连续性。
- `docs/architecture.md:§9.19` — 固化 S19 文件入口、事务删除流程、对象清理故障边界与 S30 非目标。

**入口**：任一对话证据或归档版本被本人确认遗忘后，Core 只经 `ForgetRepository` 进入单事务闭包删除；陌生会话从架构 §9.19 回到精确符号与测试。
**测试**：全仓 Rust 176/176、Vault 72/72、workspace clippy、fmt、desktop-app all-targets check、3 个变更 Markdown 的 101 个本地链接及 `git diff --check` 全部通过。

## 2026-07-30 S19-2 schema v16 与 Vault 原子遗忘传播

**触达**:
- `crates/vault/src/schema.rs:MIGRATION_16` — 持久化按目标唯一、可供 S30 顺序重放的 `deletion_intents`，迁移整体回滚。
- `crates/vault/src/repository.rs:VaultRepository::forget_with_hook` — 在单一 SQLCipher 事务内删除对话或稳定归档来源闭包、清空可重建索引，并在提交后清理零引用密文。
- `crates/vault/src/repository.rs:delete_conversation_evidence_closure/delete_archived_evidence_closure` — 处理 Claim 取代链、记忆/争议、身份派生、块谱系、理解投影与对象引用依赖顺序。

**入口**：`ForgetRepository::commit_forget` 由 Core 确认门禁调用；`VaultRepository::deletion_intents` 提供恢复重放顺序。
**测试**：`cargo test -p vault --test forget_persistence --no-fail-fast` 5/5，通过 current/historical 双关闭、理解投影、多版本来源、纠错/记忆/争议链、跨重启删除状态和共享对象零引用清理；`cargo test -p vault forget --no-fail-fast` 通过 schema v16 与故障回滚单测。

## 2026-07-30 S19-1 显式遗忘领域契约

**触达**:
- `crates/core/src/domain.rs:ForgetTarget/ForgetRequest/ForgetReceipt` — 区分对话证据与已归档证据目标，并携带本人确认和删除意图回执。
- `crates/core/src/ports.rs:ForgetRepository::commit_forget`、`crates/core/src/memory_loop.rs:MemoryCore::forget` — 冻结幂等原子删除仓储契约与 Core 本人确认门禁。
- `crates/core/src/in_memory.rs:ForgetRepository` — 以内存适配器验证对话证据及其 Claim 版本链删除闭包。

**入口**：本人提交 `ForgetRequest`；Core 拒绝未确认或不存在目标，确认后只通过 `ForgetRepository` 原子提交。
**测试**：`cargo test -p core --test forget --no-fail-fast` 覆盖未确认零写入、确认删除、重复幂等和不存在目标拒绝。

## 2026-07-30 S18-3 交叉回归加固与全仓门禁

**触达**:
- `crates/core/src/memory_loop.rs:MemoryCore::correct_person_fact` — 在分配证据和 Claim ID 前统一拒绝与旧陈述相同的伪纠错。
- `crates/memory/src/service.rs:validate_dispute`、`crates/memory/src/in_memory.rs:append_memory_dispute` — 把 open 争议唯一性绑定到不可变记忆版本，保留旧争议而不阻塞后继版本。
- `crates/vault/src/schema.rs:claim_correction_migration_backfills_existing_claim_and_retrieval_state` — 验证 schema v14 有数据升级后的 Claim 与检索状态回填。
- `crates/vault/tests/claim_correction_persistence.rs:correction_preserves_an_old_open_dispute_without_blocking_the_successor_version` — 覆盖 S17 open 争议与 S18 自动后继记忆的交叉路径。
- `docs/architecture.md:§9.18` — 固化 S18 文件入口、事务传播流程、当前/历史分流及不做边界。

**入口**：伪纠错在 Core 领域门禁失败；真实纠错命中争议记忆时保留旧版本争议，后继记忆仍可通过 `MemoryMaintenance::raise_dispute` 独立进入新争议。
**测试**：全仓 Rust 168/168、workspace clippy、fmt、desktop-app all-targets check、3 个变更 Markdown 的 97 个本地链接及 `git diff --check` 全部通过。

## 2026-07-30 S18-2 schema v15 纠错持久化与局部传播

**触达**:
- `crates/vault/src/schema.rs:MIGRATION_15` — 回填 Claim 当前状态，新增不可变取代事件、按记忆版本隔离的争议约束、记忆传播工作项和 Claim 检索状态。
- `crates/vault/src/repository.rs:ClaimCorrectionRepository::commit_person_fact_correction` — 在一个 SQLCipher 事务内提交纠错证据、Claim 后继链、直接依赖记忆和旧/新两条 Claim 检索投影。
- `crates/memory/src/service.rs:validate_proposal/validate_dispute` — 禁止旧 Claim 新晋升，并让旧版本未复核争议不阻塞后继记忆版本。
- `crates/retrieval/src/context.rs:replay_digest`、`crates/runtime-gateway/src/adapter.rs:RetrievedClaimInput` — 把 Claim 状态和前后继关系纳入快照摘要及历史运行时出口。
- `crates/vault/tests/claim_correction_persistence.rs`、`crates/vault/src/schema.rs:tests` — 覆盖相关/无关记忆、当前/历史召回、跨重启、v14 有数据回填、迁移中断及 S17×S18 跨版本争议。
- `docs/architecture.md:§2/§4.1/§4.2/§5.6/§5.7/§8/§9.18` — 对齐 schema v15、局部传播数据流和 S18 当前边界。

**入口**：`MemoryCore::correct_person_fact` 校验后调用 Vault 原子提交；`retrieve` 按 `SourceScope::Current/Historical` 解析当前 Claim 或完整历史取代链。
**测试**：core 12/12、memory 9/9、Vault 67/67 与 core/memory/retrieval/runtime-gateway/vault 目标 clippy 通过。

## 2026-07-30 S18-1 时间化 Claim 纠错领域契约

**触达**:
- `crates/core/src/domain.rs:Claim/ClaimStatus/ClaimCorrectionReceipt` — 为 Claim 增加当前/已取代状态、显式前驱和传播计数回执。
- `crates/core/src/ports.rs:ClaimCorrectionRepository` — 固定逐字纠错证据、后继本人事实和派生传播必须原子提交的仓储契约。
- `crates/core/src/memory_loop.rs:MemoryCore::correct_person_fact` — 拒绝空文本、无效时间、非本人或已取代 Claim，并构造时间化后继。
- `crates/core/src/in_memory.rs:ClaimCorrectionRepository` — 提供不留部分证据的最小原子实现。
- `crates/core/tests/claim_corrections.rs` — 覆盖合法取代链、历史保留、非本人/已取代/无效时间拒绝。

**入口**：可信调用方以旧本人事实 Claim ID、逐字修正和适用时间调用 `MemoryCore::correct_person_fact`。
**测试**：core 11/11 与 `cargo clippy -p core --all-targets -- -D warnings` 通过。

## 2026-07-30 S17-4 修订关联补全、架构对齐与全仓门禁

**触达**:
- `crates/memory/src/domain.rs:MemoryDispute::revised_version` — 让修订争议在即时结果与跨重启读取中都保留后继版本关联。
- `crates/vault/src/repository.rs:load_memory_dispute` — 恢复并校验修订后继必须直接承接被争议版本，损坏关联失败关闭。
- `crates/vault/tests/memory_persistence.rs:revised_dispute_survives_reopen_with_its_successor_version_link` — 覆盖 schema v14 修订关联跨重启恢复。
- `docs/architecture.md:§2/§4.1/§4.2/§5.5～§5.7/§8/§9.17` — 对齐 schema v14、争议数据流、运行时出口和 S17 当前实现边界。

**入口**：陌生会话从架构 §9.17 定位本人异议、第二自我复核、成对召回与自然/高影响运行时披露的完整路径。
**测试**：全仓 Rust 156/156、fmt、workspace clippy、desktop check、release build、前端 2/2、typecheck、production build、3 个变更 Markdown 的 94 个本地链接及 `git diff --check` 全部通过。

## 2026-07-30 S17-3 自然分层披露与高影响失败关闭

**触达**:
- `crates/core/src/domain.rs:FrozenMemoryDispute/DecisionImpact` — 把完整争议对和普通/高影响级别纳入不可变工作上下文。
- `crates/retrieval/src/context.rs:freeze_working_context/replay_digest` — 优先预算争议对，并让影响级别参与快照重放摘要。
- `crates/runtime-gateway/src/adapter.rs:WorkingContextInput/respond` — 普通模式禁止状态朗读与固定模板，高影响模式要求主动不确定性策略及争议依据入口。
- `crates/runtime-gateway/src/transport.rs:OutboundContextSource::MemoryDispute` — 外发审计记录争议身份、来源 Claim 和双方对话证据 ID。
- `crates/runtime-gateway/tests/runtime_contract.rs` — 覆盖普通自然策略、高影响主动披露和缺少依据入口失败关闭。

**入口**：可信调用方在 `RetrievalQuery::with_decision_impact` 标注高影响；冻结快照把该级别传给运行时，模型不能自行降级披露。
**测试**：runtime-gateway 11/11、retrieval 10/10 与 core/retrieval/runtime-gateway 目标 clippy 通过。

## 2026-07-30 S17-2 schema v14 争议持久化与直接相关成对召回

**触达**:
- `crates/vault/src/schema.rs:MIGRATION_14` — 扩展记忆状态事件并保存争议、本人反证、复核结果、复核依据和直接相关路由词。
- `crates/vault/src/repository.rs:LongTermMemoryRepository` — 原子写入 `DISPUTED/RETRACTED`、保持/撤回/修订结果，并跨重启恢复争议及修订后继版本关联。
- `crates/vault/src/repository.rs:recall_disputed_memories` — 只对当前查询词直接相关的活动争议返回不可拆分双方配对，撤回与修订结果停止召回。
- `crates/retrieval/src/context.rs:freeze_working_context` — 在预算内优先冻结双方立场、来源、反证、复核依据和争议状态。
- `crates/vault/tests/memory_persistence.rs` — 覆盖跨重启保持争议、修订后继版本、相关/非相关召回和撤回关闭全部召回路径。

**入口**：`retrieve` 单独请求 `recall_disputed_memories`；普通长期记忆通道继续排除 `DISPUTED`，Context Builder 只接受完整配对。
**测试**：retrieval 9/9、Vault 62/62 与两 crate 目标 clippy 通过；schema v14 中断迁移保持 v13 可重开。

## 2026-07-30 S17-1 记忆争议与复核领域契约

**触达**:
- `crates/memory/src/domain.rs:MemoryDisputeRequest/MemoryDisputeReview/MemoryDisputeResolution` — 固定本人异议、第二自我复核、双方原文依据和三种复核结果。
- `crates/memory/src/service.rs:MemoryMaintenance::raise_dispute/review_dispute` — 校验版本、逐字反证、复核依据与撤回后新增来源门禁。
- `crates/memory/src/in_memory.rs:InMemoryLongTermMemoryRepository` — 提供 `DISPUTED/RETRACTED` 与修订后继版本的原子领域测试仓储。
- `crates/memory/tests/memory_disputes.rs` — 覆盖保持争议、撤回、修订和缺少反证拒绝。

**入口**：可信 Core 以本人已保留的原文证据调用 `raise_dispute`；第二自我完成复核后调用 `review_dispute`，本人不能直接选择结果。
**测试**：`cargo test -p memory --no-fail-fast` 8/8 与 `cargo clippy -p memory --all-targets -- -D warnings` 通过。

## 2026-07-30 S16-3 架构对齐与全仓门禁

**触达**:
- `docs/architecture.md:§2/§4.1/§5.5/§5.6/§8/§9.16` — 对齐 memory 组件、schema v13、显式提议门禁、版本取代与权威 Claim 召回边界。
- `docs/code-trail.md:S16-1..S16-3` — 保留领域契约、持久化召回和最终验收三个可独立接手的实现切片。

**入口**：陌生会话从架构 §9.16 定位 `crates/memory`、Vault 和 retrieval 的完整路径，再按 S16-1/S16-2 回到精确符号与测试。
**测试**：全仓 Rust 145/145、fmt、workspace clippy、desktop check、release build、前端 2/2、typecheck、production build、2 个变更 Markdown 的 94 个本地链接及 `git diff --check` 全部通过。

## 2026-07-30 S16-2 schema v13 记忆版本持久化与权威召回

**触达**:
- `crates/vault/src/schema.rs:MIGRATION_13` — 保存稳定记忆 ID、不可变版本、三账本来源、追加式状态事件和当前版本路由词。
- `crates/vault/src/repository.rs:LongTermMemoryRepository` — 在单事务内提交初版或显式后继版本，并从 SQLCipher 跨重启恢复完整版本链。
- `crates/vault/src/repository.rs:recall_long_term_memory_candidates` — 只由当前非取代版本的记忆词命中三账本 Claim，随后沿既有权威解析路径进入工作上下文。
- `crates/vault/tests/memory_persistence.rs` — 覆盖跨重启、原子取代、旧版本停止召回、暂定记忆召回与无提议零状态。

**入口**：`MemoryMaintenance::propose` 校验后调用 Vault 版本追加；`retrieve` 合并长期记忆来源 Claim 与其他通道，再统一 `resolve_authoritative`。
**测试**：Vault 58 项测试、memory 4/4 及 memory/retrieval/vault 目标 clippy 通过；schema v13 中断迁移保持 v12 可重开。

## 2026-07-30 S16-1 长期记忆显式提议领域契约

**触达**:
- `crates/memory/src/domain.rs:MemoryProposal/MemoryVersion` — 固定必填字段、三类提议依据、初始状态和显式修订版本目标。
- `crates/memory/src/service.rs:MemoryMaintenance::propose` — 校验来源存在、三账本归属、适用时间、可信度和跨任务保留理由，拒绝静默晋升。
- `crates/memory/src/in_memory.rs:InMemoryLongTermMemoryRepository` — 以稳定记忆 ID、不可变后继版本和前版 `SUPERSEDED` 状态提供最小测试仓储。
- `crates/memory/tests/memory_proposals.rs` — 覆盖直接证据、解释性推断、缺字段、跨账本、无提议零记忆和显式取代。

**入口**：可信 Core 只在第二自我提交完整 `MemoryProposal` 时调用 `MemoryMaintenance::propose`；账本与理解投影没有自动创建长期记忆的入口。
**测试**：`cargo test -p memory --no-fail-fast` 4/4 与 `cargo clippy -p memory --all-targets -- -D warnings` 通过。

## 2026-07-30 S15-4 架构对齐与全仓门禁

**触达**:
- `docs/architecture.md:§2/§4.1/§5.5/§8/§9.15` — 对齐 understanding 组件、schema v12、活动投影路由、谱系失效、权威回读和长期记忆隔离边界。
- `docs/code-trail.md:S15-1..S15-4` — 保留契约、持久化、召回和最终验收四个可独立接手的实现切片。

**入口**：陌生会话先从架构 S15 当前实现边界定位 `crates/understanding`、Vault 和 retrieval 的完整调用链，再按前三个 S15 条目回到精确符号与测试。
**测试**：全仓 Rust 138/138、fmt、workspace clippy、desktop check、release build、前端 2/2、typecheck、production build、2 个变更 Markdown 的 94 个本地链接及 `git diff --check` 全部通过。

## 2026-07-30 S15-3 投影候选路由与权威回读

**触达**:
- `crates/retrieval/src/lib.rs:RecallChannels/RetrievalRepository` — 增加 understanding 候选通道，投影正文不进入检索结果类型。
- `crates/vault/src/repository.rs:recall_understanding_candidates` — 只从活动且 artifact 摘要一致的投影召回来源块，并应用显式时间交集和 128 候选上限。
- `crates/retrieval/tests/context_freeze.rs:vector_memory_and_neighbors_freeze_to_one_replayable_budgeted_snapshot` — 验证理解候选与其他通道合并后仍只冻结权威值。
- `crates/vault/tests/understanding_persistence.rs:active_projection_routes_only_authoritative_evidence_and_invalidated_projection_stops` — 验证投影独有词命中逐字证据、安全前移延续和失效后停止。

**入口**：`retrieve` 在索引候选与长期记忆候选之外请求活动理解候选，随后统一调用 `resolve_authoritative`；运行时出口不接收投影 recipe 或解释文本。
**测试**：retrieval 9/9、understanding persistence 2/2 与 retrieval/vault/understanding 目标 clippy 通过。

## 2026-07-30 S15-2 schema v12 投影持久化与局部失效

**触达**:
- `crates/vault/src/schema.rs:MIGRATION_12` — 在 SQLCipher 内保存投影 recipe、来源、语句、状态事件和可删除路由 artifact。
- `crates/vault/src/repository.rs:UnderstandingRepository` — 认证回读规范证据，原子提交投影，并从 durable recipe 重建缺失 artifact。
- `crates/vault/src/repository.rs:reconcile_understanding_projections` — 只触达引用变化块的活动投影；`UNCHANGED/MOVED` 前移并重建，其余谱系状态失效关闭。
- `crates/vault/tests/understanding_persistence.rs` — 覆盖跨重启、删除重建、安全前移、相关失效和无关投影不变。

**入口**：`materialize_projection` 经 `UnderstandingRepository` 写入；S11 `commit_lineage_batch` 在同一事务内协调受影响投影。
**测试**：Vault 49 个单元/集成测试、understanding 4/4 与两 crate 目标 clippy 通过；schema v12 中断迁移保持 v11 可重开。

## 2026-07-30 S15-1 选择性深度理解投影契约

**触达**:
- `crates/understanding/src/lib.rs:ProjectionTrigger/ProjectionRecipe` — 固定本人指定、反复召回、重要变化、当前任务四类触发及 64 个权威块的有限范围。
- `crates/understanding/src/lib.rs:ProjectionContent/SourcedStatement` — 固定事件链、人物/主题关系和阶段概括三类带来源投影，不授予事实或长期记忆资格。
- `crates/understanding/src/lib.rs:materialize_projection/rebuild_projection` — 只解析显式引用，并以版本化摘要支持派生物删除后同版本重建。
- `crates/understanding/tests/selective_projection.rs` — 覆盖四触发、三投影、非触发拒绝、有限来源和删除重建。

**入口**：可信 Core 以一个已校验 `ProjectionRecipe` 调用 `materialize_projection`；仓储只向构建器暴露点查证据块，不提供全库枚举。
**测试**：`cargo test -p understanding --no-fail-fast` 4/4 与目标 clippy 通过。

## 2026-07-30 S14-4 冻结上下文最小出口与桌面入口

**触达**:
- `crates/runtime-gateway/src/adapter.rs:WorkingContextInput` — 只序列化冻结窗口、账本来源与检索快照，不外发 repository、向量或未选候选。
- `crates/runtime-gateway/src/transport.rs:OutboundContextSource` — 在外发审计中记录证据块或账本的稳定来源引用。
- `apps/desktop/src-tauri/src/state.rs:send_message_with_retrieval` — 以本人当前消息构造 S14 上下文，再交给既有持续对话运行时。
- `crates/runtime-gateway/tests/runtime_contract.rs:response_payload_and_disclosure_contain_only_the_frozen_retrieval_result` — 验证冻结块外发、未选候选隔离和来源审计。

**入口**：WebView `send_message` 白名单 command 经 `ManagedHost` 进入本地 Vault 检索；外部运行时只看到 `RuntimeRequest` 的冻结值。
**测试**：runtime-gateway 9/9、desktop-app 12/12、全仓 Rust 131/131 与 workspace clippy 通过；真实 Vault 对话测试确认 `eam-retrieval-v2` 快照进入运行时，纯 emoji 消息不会被空检索拒绝。

## 2026-07-30 S14-3 schema v11 向量索引与有界邻域

**触达**:
- `crates/vault/src/schema.rs:MIGRATION_11` — 新增模型版本、256 维和 512 字节硬约束的可重建向量表。
- `crates/vault/src/repository.rs:append_vector_hits` — 对加密向量做精确余弦扫描，按阈值和稳定引用保留最多 64 个候选。
- `crates/vault/src/repository.rs:recall_retrieval_neighbors` — 扩展相邻结构块、7 天同来源时间邻居与一跳关系邻居，不递归扩散。
- `crates/vault/src/repository.rs:rebuild_retrieval_index` — 在同一事务重建全文、时间、关系和向量索引并校验统一摘要。
- `crates/vault/tests/retrieval_persistence.rs:vector_windows_and_replay_digest_survive_sqlcipher_reopen` — 覆盖纯向量命中、动态窗口与跨重启相同冻结结果。

**入口**：`RetrievalRepository` 在本地可信边界执行向量召回和邻域查询；每个返回引用仍由 `resolve_authoritative` 认证回读。
**测试**：schema v11 中断回滚、向量摘要损坏重建不改权威数据、Vault 全测和目标 clippy 通过。

## 2026-07-30 S14-2 多通道重排与冻结工作上下文

**触达**:
- `crates/retrieval/src/lib.rs:RecallChannels/retrieve` — 增加 vector 与 long-term-memory 召回位，按通道数、全文分数、向量分数和稳定引用重排。
- `crates/retrieval/src/context.rs:freeze_working_context` — 权威解析种子与邻域，按预算组成完整块窗口并生成 SHA-256 replay digest。
- `crates/core/src/domain.rs:WorkingContext` — 保存冻结窗口、账本项、来源当前性、token 账目和检索/模型版本。
- `crates/retrieval/tests/context_freeze.rs` — 覆盖向量/记忆通道、邻域、可重放冻结及超预算整块跳过。

**入口**：可信 Context Builder 以 `RetrievalQuery + TokenBudget + frozen_at` 调用 `freeze_working_context`；S16 前 Vault 的长期记忆通道稳定为空。
**测试**：retrieval 9/9、core 6/6 与两个 crate 的目标 clippy 通过。

## 2026-07-30 S14-1 G07 固定向量基准

**触达**:
- `docs/retrieval-contract-v2.md:G07 Retrieval Contract v2` — 冻结本地模型、精确索引、重排、邻域、token budget 与性能上限。
- `crates/retrieval/src/vector.rs:embed_text/cosine_similarity_bps` — 实现无下载的 256 维确定性子词特征哈希和 basis-point 余弦分数。
- `crates/retrieval/tests/fixtures/g07-retrieval-benchmark.tsv` — 固定英文形态变化、共享主题和中文措辞变化语料。
- `crates/retrieval/tests/g07_vector_benchmark.rs` — 锁定 3/3 Top-3 覆盖、向量字节重放和 4,096 向量 debug 扫描上限。

**入口**：索引构建与查询必须同时调用 `embed_text`；模型或特征语义变化必须提升契约版本并重建派生索引。
**测试**：固定质量基准 3/3、4,096 向量扫描小于 5 秒，空输入和非法向量字节失败关闭。

## 2026-07-30 S13-2 权威多通道检索与可重建索引

**触达**:
- `crates/vault/src/schema.rs:MIGRATION_10` — 新增全文词项、账本有效期、实体关系、AVAILABLE 投影与双摘要元数据。
- `crates/vault/src/repository.rs:RetrievalRepository` — 实现索引新鲜度校验、原子重建、全文/时间/关系召回和 current/historical 门禁。
- `crates/vault/src/repository.rs:recall_retrieval_candidates` — 将显式时间条件作为全文/关系候选的交集门禁，拒绝仅命中时间的无关结果混入。
- `crates/vault/src/repository.rs:resolve_retrieval_candidate` — 每个候选认证回读规范证据块或校验带逐字来源的账本，拒绝索引片段成为事实。
- `crates/vault/tests/retrieval_persistence.rs:authoritative_multi_channel_retrieval_survives_scope_changes_and_reopen` — 覆盖时间冲突、实体关系、来源移除、历史范围与跨重启。
- `crates/vault/src/repository.rs::tests::corrupt_retrieval_index_rebuilds_without_mutating_authority` — 故障注入验证损坏索引重建不改权威证据。

**入口**：可信 Core 调用 `retrieve(&mut repository, RetrievalQuery)`；查询先确保 `eam-retrieval-v1` 索引有效，再解析为 `EvidenceBlockRef | ClaimId` 对应的权威值。
**测试**：S13 端到端基准 1/1 覆盖跨通道时间交集，损坏重建故障注入与 schema v10 中断回滚通过；全仓 Rust 119/119、fmt/clippy/desktop check、前端 2/2/typecheck/build 及 release 构建通过。

## 2026-07-30 S13-1 检索领域契约

**触达**:
- `crates/retrieval/src/lib.rs:RetrievalQuery/TimeRange` — 固定全文、时间、实体、结果上限和 `current | historical` 输入。
- `crates/retrieval/src/lib.rs:RetrievalRepository/retrieve` — 固定索引只产候选、候选必须权威解析的本地可信边界。
- `crates/retrieval/src/lib.rs:search_terms` — 提供确定性 ASCII 大小写折叠和 Unicode/CJK 整词、字符及双字词项。

**入口**：本地 Context Builder 或测试以 `RetrievalQuery` 调用 `retrieve`；外部模型与 WebView 不获得 repository。
**测试**：`crates/retrieval/src/lib.rs::tests` 覆盖空查询/反向时间/越界结果拒绝及中英文词项稳定性。

## 2026-07-30 S12-3 Obsidian 端到端校准

**触达**:
- `crates/source-obsidian/src/lib.rs:read_scanned_source_file` — 无跟随、有硬上限地读取扫描快照，并在长度/时间变化时终止本轮校准。
- `crates/ingestion/src/service.rs:reconcile_obsidian_source` — 串联完整扫描、稳定归档、S09 解析、S10/S11 物化、移除确认和关系刷新。
- `crates/source-obsidian/tests/fixtures/obsidian-vault` — 固定 Properties、标签、别名、Wikilink、嵌入、附件、配置目录与回收站样本。
- `crates/vault/tests/obsidian_reconciliation.rs:fixed_obsidian_vault_reconciles_end_to_end_without_source_writes` — 覆盖新增、幂等、修改、移动、删除、根离线、恢复和源目录哈希不变。

**入口**：可信宿主定期或收到文件通知后调用 `reconcile_obsidian_source(repository, root_id, root_path, limits, at)`。
**测试**：固定样本端到端测试 1/1、`source-obsidian` 单测 4/4 及 source/ingestion/vault 目标 clippy 通过。

## 2026-07-30 S12-2 Obsidian 来源状态与关系持久化

**触达**:
- `crates/source-obsidian/src/lib.rs:ObsidianSourceRepository` — 定义来源根、稳定记录、归档、离线/移除和关系刷新 port。
- `crates/vault/src/schema.rs:MIGRATION_9` — 新增来源根/状态事件、Obsidian Properties、标签、别名、关系与可重建解析投影。
- `crates/vault/src/repository.rs:ObsidianSourceRepository` — 原子保存移动、移除、恢复和版本，根离线不改变任何子记录。
- `crates/vault/src/repository.rs:persist_obsidian_parse_projection` — 在接受 Markdown 产物的同一事务内保存可查询元数据与关系。
- `crates/vault/tests/obsidian_source_persistence.rs` — 覆盖移动、离线、移除/恢复、跨重启、关系解析和 S11 谱系复用。

**入口**：可信 Core 注册来源根后，通过 `archive_source_file` 与 `finish_source_reconciliation` 提交一次完整校准。
**测试**：Obsidian 持久化集成测试 2/2 通过；schema v9 中断回滚与目标 clippy 通过。

## 2026-07-30 S12-1 Obsidian 只读扫描边界

**触达**:
- `crates/source-obsidian/src/lib.rs:scan_obsidian_root` — 只读递归扫描普通文件，根不可访问时失败关闭为 `SOURCE_UNAVAILABLE`。
- `crates/source-obsidian/src/lib.rs:is_excluded_directory` — 默认排除 `.obsidian` 与 `.trash`，且不跟随符号链接或重解析点。
- `crates/source-obsidian/src/lib.rs::tests::scans_only_ordinary_source_files_without_modifying_the_root` — 固定 Markdown、附件、排除目录和源目录哈希不变。

**入口**：可信 Core 对本人选择的 Obsidian 根目录调用 `scan_obsidian_root(root)`，只消费排序后的相对路径快照。
**测试**：`cargo test -p source-obsidian --no-fail-fast` 3/3 通过；目标 clippy 通过。

## 2026-07-30 S11-2 谱系持久化与相邻修订编排

**触达**:
- `crates/ingestion/src/lineage.rs:BlockLineageRepository` — 定义相邻规范修订读取、原子谱系提交和跨重启恢复 port。
- `crates/ingestion/src/service.rs:materialize_incremental_markdown` — 串联当前修订物化、相邻来源比较、幂等谱系提交与首版工作计划。
- `crates/vault/src/schema.rs:MIGRATION_8` — 新增稳定来源记录/版本、不可变谱系批次、歧义候选和显式增量工作项。
- `crates/vault/src/repository.rs:BlockLineageRepository` — 认证读取相邻规范文本，原子持久化谱系与计划并拒绝部分或冲突重放。
- `crates/vault/tests/evidence_persistence.rs:ambiguous_lineage_and_work_plan_survive_reopen_without_rewriting_history` — 覆盖歧义、幂等重放、跨重启恢复和历史引用不变。

**入口**：可信 Core 在 S09 接受产物上调用 `materialize_incremental_markdown(repository, evidence_id, contract_version, decided_at)`。
**测试**：`cargo test -p ingestion -p vault --no-fail-fast` 全绿；schema v8 中断与谱系提交故障注入均验证完整回滚，目标 clippy 通过。

## 2026-07-30 S11-1 G06 确定性块谱系与增量计划

**触达**:
- `docs/block-lineage-contract-v1.md:确定性匹配` — 冻结唯一定位器/精确指纹、Unicode trigram Dice `7000/1500 bp` 阈值和失败关闭顺序。
- `crates/ingestion/src/lineage.rs:compute_block_lineage` — 生成 `UNCHANGED/MOVED/MODIFIED/REMOVED/AMBIGUOUS` 显式谱系，绝不改写旧引用。
- `crates/ingestion/src/lineage.rs:build_work_plan` — 只为 `UNCHANGED/MOVED` 生成当前投影与索引复用，其余块重建或触发记忆复核。
- `crates/ingestion/tests/lineage_contract.rs` — 固定插入、移动、修改、删除和重复段落歧义基准。

**入口**：可信 Core 以同一 `SourceRecord` 的相邻 `MaterializedExtraction` 和各自规范文本调用 `compute_block_lineage`。
**测试**：`cargo test -p ingestion --no-fail-fast` 18/18 通过；`cargo clippy -p ingestion --all-targets -- -D warnings` 通过。

## 2026-07-30 S10-3 不可变引用读取与原生导航降级

**触达**:
- `crates/ingestion/src/evidence.rs:EvidenceBlockView` — 将一个永久块引用投影为逐字正文、已验证 UTF-8 锚点和临时 UTF-16 UI 范围。
- `crates/ingestion/src/service.rs:open_evidence_block` — 只按 `evidence_id + block_id` 打开精确版本，错误组合稳定拒绝。
- `crates/vault/src/repository.rs:EvidenceBlockQueryRepository` — 从 SQLCipher 恢复块所属契约并认证读取同一归档 Markdown，校验规范摘要后返回。
- `crates/vault/tests/evidence_persistence.rs:extraction_revision_blocks_and_refs_are_stable_across_sqlcipher_reopen` — 覆盖跨重启引用读取、UTF-16 投影、错误引用和定位器失效降级。

**入口**：可信 API 调用 `open_evidence_block(repository, EvidenceBlockRef)`；原生跳转另行调用 `EvidenceBlockView::native_navigation`，不参与规范引用判定。
**测试**：`cargo test -p ingestion -p vault --no-fail-fast` 全绿；中日韩、组合字符与 emoji 的逐字范围和 UTF-16 坐标均由确定性断言覆盖。

## 2026-07-30 S10-2 提取修订与证据块原子持久化

**触达**:
- `crates/vault/src/schema.rs:MIGRATION_7` — 新增绑定 S09 接受产物的不可变提取修订和有序证据块表、复合外键与唯一约束。
- `crates/vault/src/repository.rs:EvidenceExtractionRepository` — 认证读取同一归档 Markdown 与加密解析产物，再分配 Core-owned 修订/块 ID 并原子提交。
- `crates/vault/src/repository.rs:VaultRepository::materialized_extraction` — 从 SQLCipher 恢复同一修订、父子结构、锚点、定位器与块引用。
- `crates/vault/tests/evidence_persistence.rs:extraction_revision_blocks_and_refs_are_stable_across_sqlcipher_reopen` — 覆盖幂等物化、逐字块范围和跨重启 ID/引用稳定性。

**入口**：可信 Core 调用 `materialize_accepted_markdown(repository, evidence_id, eam-markdown-v1)`，只消费状态为 `ACCEPTED` 的 S09 产物。
**测试**：`cargo test -p vault --no-fail-fast` 全绿；repository 故障注入验证提交前失败不留下修订或任一证据块，schema v7 中断回滚保持 v6 可重开。

## 2026-07-30 S10-1 权威证据值与坐标投影

**触达**:
- `crates/ingestion/src/evidence.rs:validate_accepted_markdown` — 把已接受 S09 产物校验为 Core-owned 修订草稿，拒绝非法 UTF-8 范围、顺序和父子关系。
- `crates/ingestion/src/evidence.rs:SourceAnchor::quote` — 只按规范 Markdown 的 UTF-8 半开字节范围逐字取证。
- `crates/ingestion/src/evidence.rs:project_utf8_span_to_utf16` — 在 API 边界从唯一 UTF-8 坐标确定性派生 UTF-16 UI 范围。
- `crates/ingestion/src/evidence.rs:resolve_native_navigation` — 原生定位不可用时稳定返回 `NATIVE_NAVIGATION_UNAVAILABLE`，不影响规范引用。

**入口**：可信 Core 从 S09 `ParsedMarkdownV1` 与同一归档 UTF-8 Markdown 调用 `validate_accepted_markdown`；UI 投影只消费已验证 `SourceAnchor`。
**测试**：`crates/ingestion/src/evidence.rs::tests` 覆盖中日韩、组合字符、emoji、非法字符边界、逐字引用和原生定位降级；`cargo test -p ingestion --no-fail-fast` 12/12 通过。

## 2026-07-30 S09-3 加密解析尝试与归档重处理

**触达**:
- `crates/ingestion/src/domain.rs:MarkdownArchiveRepository` — 定义 `STARTED/ACCEPTED/REJECTED/INTERRUPTED` 尝试与原子接受、拒绝 port。
- `crates/ingestion/src/service.rs:process_archived_markdown` — 在尝试落盘后认证读取归档对象，拒绝无效 UTF-8/解析失败，并阻止同来源版本与同解析器自动重试。
- `crates/vault/src/schema.rs:MIGRATION_6` — 新增解析尝试、加密 JSON 产物和扩展归档状态约束。
- `crates/vault/src/repository.rs:MarkdownArchiveRepository` — 事务化持久化解析结果，并在重开时把遗留 `STARTED` 恢复为 `INTERRUPTED/PARSER_INTERRUPTED`。
- `crates/vault/tests/markdown_persistence.rs:accepted_parse_artifact_and_attempt_survive_sqlcipher_reopen` — 覆盖接受、拒绝、故障遗留恢复和跨重启不重试。
- `apps/desktop/src-tauri/src/state.rs:import_context_file_view` — 穷举 S09 新归档终态，保持既有领域投影编译兼容且不暴露正文。

**入口**：可信 Core 调用 `process_archived_markdown(repository, archive_id, limits, started_at, finished_at)`，只从 S08 已归档认证对象读取原文。
**测试**：`cargo test --workspace --no-fail-fast` 87/87 通过，覆盖正向持久化、无效 UTF-8/资源拒绝、遗留尝试恢复与不自动重试；`cargo clippy --workspace --all-targets -- -D warnings` 通过。

## 2026-07-30 S09-2 Core 内受限 Markdown 解析器

**触达**:
- `crates/markdown/src/lib.rs:parse_markdown` — 以私有硬上限、事件流、UTF-8 半开范围和原子错误实现 `eam-markdown-v1`。
- `crates/markdown/src/lib.rs:parse_properties/parse_wikilink` — 保守规范化 Properties，并固定 Wikilink、嵌入、标签与块定位器消歧。
- `crates/markdown/tests/contract.rs:full_dialect_has_stable_structure_and_verbatim_ranges` — 固定完整方言输出、未知语法降级、扩展隔离和五类资源拒绝。

**入口**：可信 Core 调用 `parse_markdown(source_utf8, ParseLimits)`；类型不接收路径、I/O、repository、网络、运行时或工具句柄。
**测试**：`cargo test -p eam-markdown` 6/6 通过；`cargo clippy -p eam-markdown --all-targets -- -D warnings` 通过。

## 2026-07-30 S09-1 G05 Markdown 契约与固定语料

**触达**:
- `docs/markdown-contract-v1.md:eam-markdown-v1` — 冻结解析库、硬资源上限、块/关系结构、Wikilink 消歧、Properties 降级和定位器规则。
- `crates/markdown/tests/fixtures/full-dialect.md` — 固定 CommonMark/GFM 与 Obsidian 子集正向语料。
- `crates/markdown/tests/fixtures/unknown-syntax.md` — 固定未知语法逐字保留语料。
- `crates/markdown/tests/fixtures/limits.md` — 固定五类资源拒绝语料。

**入口**：S09 实现只允许通过 `parse_markdown(&str, ParseLimits)` 消费这些契约；依赖升级必须保持固定语料输出等价。
**测试**：`git diff --check` 校验契约与语料补丁；解析结果和限额行为由下一子片的 `crates/markdown/tests/contract.rs` 固化。

## 2026-07-30 S08-3 宿主白名单归档入口

**触达**:
- `apps/desktop/src-tauri/src/state.rs:ManagedHost::import_context_file` — 在可信宿主内运行有界摄取并只返回归档状态、原因和去重结果。
- `apps/desktop/src-tauri/src/lib.rs:import_context_file` — 新增单一领域型异步 Tauri command，不暴露通用文件或 repository 能力。
- `apps/desktop/src-tauri/capabilities/main.json:main` — 保持仅 `core:default`，明确无通用文件、shell、HTTP、进程或凭据权限。

**入口**：WebView 只能调用 `import_context_file({ path, approveOversized })`，实际文件观察、认证加密和 SQLCipher 写入均留在内嵌 Core。
**测试**：`apps/desktop/src-tauri/src/state.rs::tests::bounded_import_view_*` 覆盖普通归档、非 Markdown 原因、超限等待和非普通文件拒绝；desktop-app 10 个库测试保持绿色。

## 2026-07-30 S08-2 加密对象库与归档引用

**触达**:
- `crates/vault/src/object_store.rs:ObjectStore` — 以 HMAC-SHA256 内容标识、XChaCha20-Poly1305 认证密文和原子发布实现去重对象库。
- `crates/vault/src/schema.rs:MIGRATION_5` — 新增加密归档证据元数据、状态/原因约束和对象引用索引。
- `crates/vault/src/repository.rs:ArchiveRepository` — 对象先写、SQLCipher 引用后提交，并在启动时清理无引用对象。
- `crates/vault/src/repository.rs:VaultRepository::read_archived_content` — 在可信 Core 内认证解密已归档原件。

**入口**：`ingest_inbox_file` 通过 `ArchiveRepository` 把已稳定字节交给 `VaultRepository`；重开保险库时自动执行对象引用校准。
**测试**：`crates/vault/tests/archive_persistence.rs` 覆盖内容去重、同来源幂等、删除投递原件后的跨重启恢复；`repository::tests::database_failure_leaves_recoverable_orphan_removed_on_reopen` 覆盖 SQLCipher 提交故障与无引用对象清理。

## 2026-07-30 S08-1 Context Inbox 普通文件边界

**触达**:
- `crates/ingestion/src/domain.rs:evaluate_observations` — 固定稳定普通文件、超限批准、重解析点/非普通文件拒绝及归档状态契约。
- `crates/ingestion/src/service.rs:ingest_inbox_file` — 以两次元数据观察、无跟随打开和读取后复核实现先归档前的有界文件摄取。

**入口**：可信宿主向 `ingest_inbox_file` 提供明确路径、导入策略、超限批准和归档时间；该子片只调用抽象 `ArchiveRepository`。
**测试**：`crates/ingestion/src/domain.rs::tests` 覆盖状态转换正反例；`crates/ingestion/src/service.rs::tests` 覆盖 Markdown 仅归档、非 Markdown 固定拒绝原因、目录拒绝与超限等待。

## 2026-07-29 S01 可执行的最小记忆闭环

**触达**:
- `Cargo.toml:workspace` — 建立只包含首个 `core` crate 的 Rust workspace。
- `crates/core/src/domain.rs:WorkingContext` — 定义逐字对话证据、分账 Claim、精确引用、结构化判断和冻结上下文值。
- `crates/core/src/ports.rs:MemoryRepository` — 定义可信 Core 使用的证据与账本仓储 port。
- `crates/core/src/ports.rs:CounterpartRuntime` — 运行时只接收分类输入或 `RuntimeRequest`，不接收 repository。
- `crates/core/src/memory_loop.rs:MemoryCore` — 贯通证据保留、本人事实分类、上下文冻结、引用校验及第二自我判断入账。
- `crates/core/src/in_memory.rs:InMemoryRepository` — 提供按追加顺序读取的 S01 内存适配器。
- `crates/core/src/scripted_runtime.rs:ScriptedRuntime` — 提供可检查输入的确定性分类与响应夹具。

**入口**：`MemoryCore::record_person_turn` 保存并分类本人发言；`MemoryCore::freeze_working_context` 冻结后续会话输入；`MemoryCore::run_counterpart_turn` 校验运行时输出并分账。
**测试**：`crates/core/tests/minimal_memory_loop.rs` 覆盖完整正向闭环、问题/玩笑不入本人账本、无来源及上下文外判断拒绝、自由文本不入第二自我账本、非逐字引用拒绝。

## 2026-07-29 S02 加密结构化存储与重启连续性

**触达**:
- `Cargo.toml:workspace.members` — 加入独立 `vault` crate，并由 `Cargo.lock` 固定 Windows 自包含密码依赖。
- `crates/core/src/domain.rs:ConversationEvidence::restore` — 为可信持久化适配器提供不改变 S01 行为的领域值重建入口。
- `crates/core/src/domain.rs:Claim::restore` — 重建带归属、时间和有序证据引用的不可变账本项。
- `crates/vault/src/crypto.rs:VaultKey` — 实现持钥清零、HKDF-SHA256 用途隔离及 G01 固定向量。
- `crates/vault/src/schema.rs:migrate` — 在单事务内应用版本化 schema，并以故障注入验证中断回滚。
- `crates/vault/src/repository.rs:VaultRepository::open` — 取得唯一写者锁、以 raw DbKey 打开 SQLCipher、校验页 HMAC 并恢复追加 ID。
- `crates/vault/src/repository.rs:MemoryRepository` — 加密持久化对话证据、分账 Claim 与有序引用。
- `crates/vault/src/repository.rs:VaultRepository::close` — checkpoint WAL、关闭 SQLCipher、清零 Vault Key 并释放锁。

**入口**：`VaultRepository::open(vault_root, VaultKey)` 注入 `MemoryCore`；Core 继续只依赖原 `MemoryRepository` port。
**测试**：`crates/vault/tests/encrypted_repository.rs` 覆盖跨重开领域闭环、明文不落盘、错误密钥、第二写者和损坏页拒绝；库单测覆盖 SQLCipher 版本、migration 中断、KDF/对象 AEAD 固定向量及关闭清零。

## 2026-07-29 S03 Windows 解锁与本人自持恢复密钥

**触达**:
- `crates/vault/src/key_store.rs:VaultKeyStore::initialize` — 生成随机 Vault Key/Recovery Key，将相互独立的 DPAPI 与恢复封装原子写入版本化 `bundle.meta`。
- `crates/vault/src/key_store.rs:VaultKeyStore::unlock_local` — 只通过 Windows DPAPI CurrentUser 本机副本产出 Vault Key。
- `crates/vault/src/key_store.rs:VaultKeyStore::unlock_recovery` — 校验 Bech32m 载体并在不读取 DPAPI 字段时认证解封 Vault Key。
- `crates/vault/src/dpapi.rs:protect_current_user` — 以 `CRYPTPROTECT_UI_FORBIDDEN` 调用 DPAPI，且不启用 LocalMachine 范围。
- `crates/vault/src/dpapi.rs:unprotect_current_user` — 复制后清理 `LocalAlloc` 明文并以统一错误关闭失败路径。
- `crates/vault/src/crypto.rs:VaultKey::generate` — 从操作系统安全随机源创建 256-bit Vault Key。
- `crates/vault/src/error.rs:VaultError::UnlockFailed` — 合并错误恢复载体、错误密钥与认证篡改的外部失败面。

**入口**：首次创建调用 `VaultKeyStore::initialize`；日常启动调用 `unlock_local`；独立恢复调用 `unlock_recovery`，两者所得密钥继续只交给 `VaultRepository::open`。
**测试**：`crates/vault/tests/windows_unlock.rs` 覆盖同用户 DPAPI、无 DPAPI 副本恢复、错误密钥/篡改不可区分、Bech32m 校验、元数据无明文密钥及既有数据库保护；`key_store` 单测覆盖元数据截断和尾随字节拒绝。

## 2026-07-29 S04 最小自我介绍与首个身份版本

**触达**:
- `Cargo.toml:workspace.members` — 加入独立 `identity` crate；`crates/vault/Cargo.toml:dependencies` 接入其持久化 port。
- `crates/identity/src/domain.rs:SelfIntroductionCategory` — 固定基本身份与称呼、当前生活、重要人物、长期目标、当前关切、希望被帮助看见部分六类输入。
- `crates/identity/src/domain.rs:IdentityStateVersion` — 定义有来源、带形成时间且不可改写的身份版本值。
- `crates/identity/src/service.rs:IdentityFormation::record_initial_self_introduction` — 在任何写入前验证六类完整、唯一且非空。
- `crates/identity/src/service.rs:IdentityFormation::form_initial_identity` — 只接受第二自我作者、保留反思使命、不冒充本人且来源受限的结构化提议，并只追加版本 1。
- `crates/identity/src/ports.rs:IdentityRuntime` — 运行时只接收类型化初始自述，不获得 repository。
- `crates/identity/src/scripted_runtime.rs:ScriptedIdentityRuntime` — 提供可检查请求的确定性身份形成夹具。
- `crates/vault/src/schema.rs:MIGRATION_2` — 增加六类自述绑定、身份版本及身份来源表，并保持迁移原子性。
- `crates/vault/src/repository.rs:IdentityRepository` — 在一个 SQLCipher 事务内写入六条本人证据/事实并持久化、恢复身份版本。

**入口**：首次引导先调用 `IdentityFormation::record_initial_self_introduction`，成功后调用 `form_initial_identity`；重启通过 `current_identity` 只加载已提交版本 1。
**测试**：`crates/identity/tests/initial_identity.rs` 覆盖缺类、账本归属、本人角色卡、放弃使命、冒充本人和重复形成拒绝；`crates/vault/tests/identity_persistence.rs` 覆盖 SQLCipher 重启同一身份与明文不落盘；`schema` 单测覆盖 v2 中断回滚。

## 2026-07-29 S05 自我包、唤醒与休眠连续性

**触达**:
- `crates/identity/src/self_bundle.rs:SelfBundleState` — 定义宪法/身份版本、第二自我经历引用、信念引用、关系状态和未完成意图组成的完整可迁移状态。
- `crates/identity/src/self_bundle.rs:SelfBundleVersion` — 建立带前驱、提交时间和唤醒结果的不可改写 Self Bundle 版本。
- `crates/identity/src/presence.rs:PresenceCoordinator::initialize_self_bundle` — 只允许围绕当前已形成身份创建首个 Self Bundle。
- `crates/identity/src/presence.rs:PresenceCoordinator::wake` — 固定七状态成功链，令三个工作失败出口都经安全完整提交后休眠，并拒绝候选越权修改宪法或身份版本。
- `crates/identity/src/ports.rs:SelfBundleRepository` — 将完整版本原子追加与当前完整版本加载隔离为可信持久化 port。
- `crates/identity/src/ports.rs:WakeWork` — 让有界观察、思考和回应只交换完整候选状态，不获得 repository。
- `crates/identity/src/in_memory.rs:SelfBundleRepository` — 为状态机测试维护连续且不可改写的内存版本链。
- `crates/vault/src/schema.rs:MIGRATION_3` — 新增 Self Bundle 父版本、经历、信念和未完成意图表，并保持迁移原子性。
- `crates/vault/src/repository.rs:SelfBundleRepository` — 在一个 SQLCipher 事务内校验版本链、追加父行与全部有序子项，并恢复最后完整版本。

**入口**：S04 身份形成后调用 `PresenceCoordinator::initialize_self_bundle`；对话、新证据、定时反思或重要变化调用 `wake(trigger)`，只有 Self Bundle 事务成功后才返回最终休眠状态。
**测试**：`crates/identity/tests/event_driven_presence.rs` 覆盖完整七状态链、`OBSERVE/THINK/RESPOND` 每个失败出口、宪法越权拒绝及身份门禁；`crates/vault/tests/self_bundle_persistence.rs` 覆盖完整状态加密重启恢复，以及子项外键故障导致整包版本回滚；`schema` 单测覆盖 v3 中断回滚。

## 2026-07-29 S06 模型运行时网关与最小数据出口

**触达**:
- `Cargo.toml:workspace.members` — 加入独立 `runtime-gateway` crate 并固定 JSON contract 依赖。
- `crates/core/src/ports.rs:RuntimeErrorKind` — 区分超时、不可用、结构错误与普通运行时失败，限定降级面。
- `crates/core/src/domain.rs:UnsupportedStructuredOperation` — 保留未知操作名和索引供可信 Core 明确拒绝。
- `crates/core/src/memory_loop.rs:MemoryCore::run_counterpart_turn` — 把非白名单操作记录为拒绝且不产生账本写入。
- `crates/runtime-gateway/src/transport.rs:ResponsesTransport` — 定义无 repository 的供应商传输 port、固定模型档案和精确外发检查记录。
- `crates/runtime-gateway/src/transport.rs:HttpResponsesTransport` — 实现强制 HTTPS + bearer 的 Cloud 与无凭据 Local 传输、超时/状态分类、响应上限和 token 清零持有。
- `crates/runtime-gateway/src/adapter.rs:OpenAiResponsesRuntime` — 仅序列化 prompt 与冻结工作上下文，生成 Responses v1 严格 schema 并解析固定结构化输出。
- `crates/runtime-gateway/src/fallback.rs:FallbackRuntime` — 只在 Cloud 超时或不可用时以同一 contract 降级到 Local。
- `docs/runtime-contract-v1.md:G03 Runtime Contract v1` — 冻结首个模型、最小数据出口、错误语义、白名单和固定夹具。
- `docs/adr/0048-openai-responses-runtime-family.md` — 记录首个 OpenAI Responses 单一供应商家族权衡。

**入口**：`MemoryCore` 继续只依赖 `CounterpartRuntime`；宿主以 Cloud `gpt-5.6-terra` 和 Local `gpt-oss-20b` 构造 `OpenAiResponsesRuntime`，再由 `FallbackRuntime` 组合可用性降级。
**测试**：`crates/runtime-gateway/tests/runtime_contract.rs` 覆盖 Local/Cloud 夹具等价、具体 Local HTTP 与 Cloud 明文端点发送前拒绝且凭据不入记录、严格请求字段、未选证据不外发、未知操作由 Core 拒绝、超时/不可用本地降级、结构错误失败关闭、失败尝试可检查，以及运行时不可用后 SQLCipher 证据跨重启仍存在。

## 2026-07-29 S07-1 宿主生命周期与加密运行空缺

**触达**:
- `crates/desktop-host/src/domain.rs:HostSession/HostRuntimeGap` — 定义宿主会话、启动/退出原因、心跳边界和可审计空缺值。
- `crates/desktop-host/src/lifecycle.rs:HostLifecycle` — 固定恢复、前后台显示、显式退出、升级退出和失败关闭转换。
- `crates/desktop-host/src/ports.rs:HostLifecycleRepository` — 隔离会话开始、心跳、结束和空缺查询持久化契约。
- `crates/vault/src/schema.rs:MIGRATION_4` — 新增加密宿主会话与运行空缺表，并保持 migration 原子回滚。
- `crates/vault/src/repository.rs:HostLifecycleRepository` — 原子恢复一次崩溃/退出/升级空缺，拒绝过期会话心跳和重复结束。
- `docs/host-lifecycle-v1.md:G04 Desktop Host Lifecycle v1` — 冻结单实例、自启动、签名升级、心跳空缺、WebView 能力和 Windows 测试方案。

**入口**：Tauri 宿主打开保险库后调用 `begin_host_session`，运行时每 30 秒调用 `heartbeat_host_session`，显式退出或升级前调用 `finish_host_session`；窗口隐藏不结束会话。
**测试**：`crates/desktop-host/src/lifecycle.rs::tests` 覆盖状态转换正反例；`crates/vault/tests/host_lifecycle_persistence.rs` 覆盖崩溃、显式退出、升级、时钟回退和过期会话拒绝的 SQLCipher 跨重启语义；`schema` 单测覆盖 v4 中断回滚。

## 2026-07-29 S07-2 thin Tauri 2 宿主

**触达**:
- `apps/desktop/src-tauri/src/lib.rs:builder` — 首个注册单实例插件，接入当前用户 `--background` 自启动、托盘显示/隐藏、白名单 command 和条件式签名 updater。
- `apps/desktop/src-tauri/src/lib.rs:spawn_heartbeat/shutdown_and_exit` — 每 30 秒提交加密心跳，并在显式退出时即使失败也继续安全清理后终止。
- `apps/desktop/src-tauri/src/state.rs:ManagedHost` — 装配 Vault、Core、运行时与宿主状态机，不把 repository、密钥或凭据交给 WebView。
- `apps/desktop/src-tauri/capabilities/main.json:main` — 仅向主窗口授予 `core:default`，不授予插件、文件、shell、HTTP、进程或凭据权限。
- `apps/desktop/src/App.tsx:App` — 提供只验证宿主渲染的静态前端，明确把持续对话留给下一子片。
- `.gitignore:desktop generated outputs` — 排除 `node_modules`、Vite `dist` 和 Tauri 生成 schema，保留源码、lockfile、capability 与图标资产。

**入口**：`evrything-about-me.exe` 调用 `eam_desktop_app::run`；第二实例只激活首实例，托盘和关闭窗口事件复用同一 `ManagedHost`。
**测试**：`apps/desktop/src-tauri/src/lib.rs::tests` 覆盖 updater 启用条件、失败重开错误和托盘图标；`state.rs::tests` 覆盖退出阶段全部尝试；`apps/desktop/src/App.test.tsx` 覆盖静态宿主边界；`cargo build --bins --features tauri/custom-protocol --release` 验证无 bundle Windows 可执行文件。

## 2026-07-30 S07-3 持续对话 command 与界面

**触达**:
- `apps/desktop/src-tauri/src/state.rs:ManagedHost::list_conversation` — 从 SQLCipher 只投影固定持续会话的双方逐字证据视图。
- `apps/desktop/src-tauri/src/state.rs:ManagedHost::send_message` — 在持久化前校验输入，以最近 32 轮/64 KiB 上限冻结上下文并调用 `MemoryCore::run_counterpart_turn`。
- `apps/desktop/src-tauri/src/lib.rs:list_conversation/send_message` — 注册两个白名单 Tauri command，并把阻塞模型调用移出事件循环线程。
- `apps/desktop/src/App.tsx:App` — 恢复同一段对话，发送新消息，并在运行时失败后重读已落盘的本人原文。
- `apps/desktop/src/styles.css:.conversation-shell` — 建立持续对话、消息归属、忙碌、错误和窄屏布局。

**入口**：React 启动调用 `list_conversation`；本人提交 composer 时调用 `send_message({ verbatim })`，两条路径均只经过 Tauri invoke 白名单进入内嵌 Core。
**测试**：`apps/desktop/src-tauri/src/state.rs::tests::ordinary_conversation_survives_sqlcipher_reopen_without_claims` 覆盖逐字重启恢复与普通问答零 Claim；同模块覆盖输入拒绝和既往上下文冻结；`apps/desktop/src/App.test.tsx` 覆盖恢复、成功发送及运行时失败后的已落盘发言回读。
