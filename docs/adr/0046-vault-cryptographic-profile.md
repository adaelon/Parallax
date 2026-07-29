---
status: accepted
---

# 保险库密码配置采用用途隔离的固定算法组合

**决策**: DbKey 用 HKDF-SHA256 派生并作为 SQLCipher raw key；对象用 XChaCha20-Poly1305；持钥包装器关闭时清零。

**否决**:
- 系统 SQLCipher/OpenSSL：Windows 安装状态不可重复，无法形成自包含构建。
- SQLCipher passphrase KDF：Vault Key 已是高熵随机密钥，重复口令派生没有收益。
- `cipher_memory_security=ON`：Windows `VirtualLock` 配额失败可导致进程崩溃。

**命门**: 固定 `rusqlite 0.40.1`、`libsqlite3-sys 0.38.1`/SQLCipher 4.14.0、`hkdf 0.13.0`、`chacha20poly1305 0.11.0`、`zeroize 1.9.0`。
**何时回头**: 依赖停止维护、算法出现实质弱点，或 Windows 构建/关闭测试不再通过时。
**展开**: [架构 §4.1](../architecture.md#41-权威存储布局)；固定向量见 `crates/vault/src/crypto.rs`。
