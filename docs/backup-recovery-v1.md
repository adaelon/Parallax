# G09 加密备份、恢复与遗忘重放

## 1. 固定协议

一个可恢复备份单元是 `Recovery Set = immutable snapshots[] + deletion-head.eam`。单个历史快照不能脱离同组的最新删除头恢复；删除头缺失、损坏或组 ID 不匹配时必须失败关闭。

```text
VaultRepository + RecoveryKey
  -> SQLite online backup 得到一致 self.db
  -> 认证读取所有被引用对象密文
  -> Snapshot{set_id, generation, deletion_watermark, self.db, objects/*}
  -> XChaCha20-Poly1305(BackupKey, random nonce, portable recovery metadata as AAD)
  -> backup-<generation>.eambak

deletion_intents ORDER BY id
  -> DeletionHead{set_id, latest_generation, ordered targets}
  -> 同样以当前 VaultKey 派生的 BackupKey 认证加密
  -> deletion-head.eam（原子覆盖，不进入代际保留）
```

`BackupKey = HKDF-SHA256(VaultKey, salt="evrything-about-me/v1/vault-subkeys", info="backup")`。归档头只携带去掉 DPAPI 字段的可移植恢复封装；它不含个人元数据，并作为 AEAD AAD 认证。外部位置看不到数据库页、对象、文件名、删除目标或索引明文。

## 2. 保留与发布

- 每个 Recovery Set 保留最近 3 个完整快照；删除头不随旧快照裁剪。
- 先原子发布删除头，再发布新快照，最后裁剪第 4 旧的快照；任一步失败不覆盖已有可恢复代际。
- 每次遗忘后必须同步已登记 Recovery Set 的删除头；未同步的集合标记为不可恢复，不能用“只打开旧快照”降级绕过。
- 备份目标必须位于保险库目录之外；符号链接、未知条目、重复路径、超限字段和缺失对象全部拒绝。

## 3. 恢复与轮换

```text
RecoveryKey
  -> 分别认证解封 snapshot / deletion head 的 VaultKey
  -> 验证 set_id、generation、水位、文件集合与长度
  -> 写入同卷 staging 目录
  -> 以旧 VaultKey 打开 SQLCipher，验证 schema 与对象认证
  -> 按 deletion-head 顺序重放 snapshot 水位后的目标
       target exists   -> 执行完整 S19 删除闭包
       target absent   -> 写入零计数 tombstone，推进 ID 水位
  -> 清空并从剩余权威状态重建全部检索索引
  -> 生成新 VaultKey，重加密 SQLCipher 与每个对象并更新对象 ID
  -> 用同一 RecoveryKey 生成新的恢复封装和本机 DPAPI 副本
  -> 原子发布 staging 目录
```

恢复总是轮换 VaultKey；Recovery Key 本身不静默变化，因此同一本人载体仍可打开新保险库和历史 Recovery Set。历史集合在恢复期间保持字节不变并转为只读；新保险库必须创建新集合，避免跨目录原子发布和两个 Vault 分叉写同一删除头。发布前的任何失败只删除 staging，不修改目标路径或备份文件。

## 4. 确定性矩阵

| 场景 | 必须结果 |
| --- | --- |
| 正常快照 | 权威数据与对象恢复；索引重建；新旧 VaultKey 不同。 |
| 截断 snapshot/head | AEAD 或长度校验失败，目标目录不存在。 |
| 篡改 header/ciphertext | 统一返回无效备份，不能区分密钥与内容错误。 |
| 历史快照 + 最新删除头 | 先删除已遗忘目标，再允许检索；current/historical 均不可见。 |
| 删除目标不在历史快照 | 写 tombstone 推进水位，后续 ID 不得复用。 |
| 数据库引用对象缺失 | 创建或恢复失败，不返回半成品。 |
| 删除头缺失或组不匹配 | 失败关闭，不允许独立恢复历史快照。 |
| 外部明文扫描 | 固定测试原文不得出现在 snapshot 或 deletion head 字节中。 |
| 第 4 个新快照发布 | 仅裁剪最旧快照，仍保留 3 个完整代际和删除头。 |

## 5. 边界

不提供平台托管密钥、丢失 Recovery Key 后的旁路、增量/差分归档、跨 Recovery Set 拼接、原地覆盖现有保险库或 SSD/历史副本的法证级擦除。首版恢复 API 面向可信 Core；桌面选择目录与进度 UI 不属于 S30。
