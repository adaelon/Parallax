# 代码链路

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
