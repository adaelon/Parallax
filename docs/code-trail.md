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
