# SESSION_CHECKPOINT — 2026-08-04 16:06 +08:00

## 新鲜度自检
- 写入时最新 commit：`281e433 feat: complete S06R-1 runtime contract v2`。
- 读入时对比 `git log --oneline -3`；若仅多出 checkpoint-only 文档提交，以 Git 与当前文件状态为准。

## 当前在做什么
S01～S31 与 S06R-1 已完成并提交；下一片是 S06R-2「SQLCipher 单档案与 write-only 密钥」。S32 继续锁定，直到 S06R-2～S06R-5 完成、完整重跑 S31 并冻结新构建。

## 下一步（可直接接手）
1. 在 `crates/vault/src/schema.rs` 增加 `MIGRATION_26` 单例运行时档案，默认 Base URL `http://127.0.0.1:11434/v1`、模型 `gpt-oss-20b`、无密钥，并接入 `migrate_with_hook`。
2. 在 `crates/vault/src/repository.rs` 定义内部完整档案与脱敏视图，新增读取和 `KEEP/REPLACE/CLEAR` 原子更新 API；复用 S06R-1 的目标/字段验证边界。
3. 新增 Vault 集成测试，覆盖 v25→v26、中断回滚、写入后重启、三种密钥动作、空白/超限拒绝及视图永不返回完整 Key。
4. 扩展 `crates/vault/tests/backup_recovery.rs`，证明合成 Key 随加密 `self.db`/Recovery Set 恢复，且数据库与备份原始字节不可搜索。
5. 运行 Vault 定向测试、runtime 回归、fmt/Clippy，完成 `docs/code-trail.md` 后提交 S06R-2。

## 未提交 / 未完成
- 无；本文件将作为 checkpoint-only 文档提交。
- S06R-2～S06R-5 尚未实现；旧 S31 的 49-ADR 矩阵和 `425f5ff` 安装包只保留历史证据，不具备 S32 候选资格。

## 已验证基线
- `cargo test -p runtime-gateway` 27/27；`cargo test -p desktop-app` 24/24。
- runtime-gateway Clippy `-D warnings`、desktop all-targets check、`cargo fmt --all -- --check` 全绿。
- Markdown 本地链接 266/266；254 个 tracked text 隐私扫描零违规；静态安全边界与 `git diff --check` 通过。
- 未运行完整 S31；其矩阵在 S06R-5 前应继续因 ADR-0053 尚未全链路验收而失败关闭。

## 冷启动读序
1. `docs/implementation-slices.md` S06R-2、S06R-5、§12 — 当前切片边界、后续依赖和完成门禁。
2. `CONTEXT.md`；`docs/adr/0053-vault-backed-configurable-responses-runtime-profile.md`；`docs/runtime-contract-v2.md` — 冻结术语、档案权衡和可复用验证契约。
3. `crates/vault/src/schema.rs`；`crates/vault/src/repository.rs`；`crates/vault/src/backup.rs` — schema v25、Repository 生命周期与整库恢复路径。
4. `crates/runtime-gateway/src/{transport,adapter}.rs`；`crates/runtime-gateway/tests/runtime_contract.rs` — S06R-1 已提交的 Base URL、模型、密钥与拒绝矩阵。
5. `docs/system-acceptance-v1.md` §8；`docs/g10-personal-baseline.md`；`docs/longitudinal-observation-template.md` — 发布锁与新冻结构建要求。
