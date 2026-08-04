# S31 首版系统验收矩阵

## 1. 执行契约

`scripts/run-system-acceptance.ps1` 是 S31 的唯一仓库级入口。它依次验证本文档的覆盖完整性、G10 私隐边界、PowerShell 分支自测、Rust workspace、桌面端、浏览器扩展、静态检查、生产构建和 NSIS 安装烟雾。任意一项失败即整体失败，不产生 S32 可用的冻结构建结论。

验收矩阵中的 `automated` 表示证据被 runner 覆盖，不表示可跳过实际运行。矩阵不允许 `pending`、未定义证据 ID、重复 ID，或产品/ADR 中存在但本文档缺失的条目。

## 2. 证据注册表

| ID | 可执行入口 | 锁定的行为 | 类型 |
| --- | --- | --- | --- |
| EV-WORKSPACE | `cargo test --workspace --no-fail-fast` | 全部 Rust 单元、集成、故障注入和跨重启回归 | automated |
| EV-IDENTITY-INIT | `cargo test -p identity --test initial_identity` | 最小介绍门槛、本人证据、首个身份和反冒充 | automated |
| EV-IDENTITY-PRESENCE | `cargo test -p identity --test event_driven_presence` | 唤醒状态机、休眠提交与宪法拒绝 | automated |
| EV-CORE-MEMORY | `cargo test -p core --test minimal_memory_loop` | 证据逐字引用、三账本隔离与工作上下文边界 | automated |
| EV-CORE-IDENTITY | `cargo test -p core --test identity_evolution` | 身份版本、模型切换与自我包主权 | automated |
| EV-SHARED | `cargo test -p core --test shared_experiences`; `cargo test -p vault --test shared_experience_persistence` | 共同经历、双签、取代、退出、历史与遗忘闭包 | automated |
| EV-CONSTRAINTS | `cargo test -p core --test relational_constraints`; `cargo test -p retrieval --test relational_constraints` | 当前关系约束、宪法优先级与未来投影 | automated |
| EV-REFLECTION | `cargo test -p core --test reflection_invitations`; `cargo test -p vault --test reflection_persistence` | 自然时机、延后、静默、安全例外与重启 | automated |
| EV-MEMORY | `cargo test -p memory --test memory_proposals`; `cargo test -p vault --test memory_persistence` | 显式记忆提议、版本与权威来源召回 | automated |
| EV-DISPUTE | `cargo test -p memory --test memory_disputes`; `cargo test -p runtime-gateway --test runtime_contract` | 说服/争议、成对召回、自然表达与高影响披露 | automated |
| EV-PATTERN | `cargo test -p memory --test pattern_maturity` | 三事件门槛、反例、成熟资格、显式提议与强反例 | automated |
| EV-CORRECTION | `cargo test -p core --test claim_corrections`; `cargo test -p vault --test claim_correction_persistence` | 时间化取代、局部传播、当前/历史投影 | automated |
| EV-FORGET | `cargo test -p core --test forget`; `cargo test -p vault --test forget_persistence` | 确认门、全链路删除、幂等和零引用密文清理 | automated |
| EV-INGESTION | `cargo test -p ingestion` | 稳定文件、先归档、格式状态、范围和重解析点拒绝 | automated |
| EV-MARKDOWN | `cargo test -p markdown`; `cargo test -p vault --test markdown_persistence` | `eam-markdown-v1`、资源上限、原文范围与中断回复 | automated |
| EV-LINEAGE | `cargo test -p ingestion --test lineage_contract`; `cargo test -p vault --test evidence_persistence` | 不可变块引用、移动/修改/删除/歧义与工作计划 | automated |
| EV-OBSIDIAN | `cargo test -p source-obsidian`; `cargo test -p vault --test obsidian_reconciliation`; `cargo test -p vault --test obsidian_source_persistence` | 只读扫描、配置排除、移除/离线与结构查询 | automated |
| EV-RETRIEVAL | `cargo test -p retrieval`; `cargo test -p vault --test retrieval_persistence` | 全文/向量/时间/关系/记忆召回、权威回读与冻结窗口 | automated |
| EV-UNDERSTANDING | `cargo test -p understanding`; `cargo test -p vault --test understanding_persistence` | 有限触发、显式来源、可重建投影与失效 | automated |
| EV-RUNTIME | `cargo test -p runtime-gateway --test runtime_contract` | 本地/云端等价 contract、最小输出、HTTPS 与结构化白名单 | automated |
| EV-RUNTIME-PROFILE | `cargo test -p vault --test runtime_profile_persistence`; `cargo test -p vault --test backup_recovery encrypted_snapshot_round_trips_authority_and_rebuilds_indexes -- --exact`; `cargo test -p desktop-app runtime_profile`; `npm test -- App.test.tsx -t "S06R-4 local runtime settings"` in `apps/desktop` | v25→v26 默认档案、`KEEP/REPLACE/CLEAR` 与 write-only Key、Recovery Set 完整恢复、严格合成测试、提交后热切换、重启和并发隔离 | automated |
| EV-VAULT | `cargo test -p vault --test encrypted_repository`; `cargo test -p vault --test windows_unlock`; `cargo test -p vault --lib` | SQLCipher、对象 AEAD、DPAPI/恢复密钥、确认前零写入、错误密钥、清零与故障注入 | automated |
| EV-SCHEMA | `cargo test -p vault schema::tests` | schema v1→v26 逐版事务迁移、中断回滚与数据回填 | automated |
| EV-HOST | `cargo test -p desktop-host`; `cargo test -p desktop-app` | 首次初始化状态机、单实例、关窗隐藏、安全退出、更新失败回复与白名单投影 | automated |
| EV-WIN-CAPTURE | `cargo test -p capture-windows`; `cargo test -p vault --test capture_persistence` | 前台/空闲元数据、暂停、崩溃空缺与不伪造活动 | automated |
| EV-BROWSER | `cargo test -p capture-browser`; `cargo test -p vault --test browser_capture_persistence` | 固定扩展来源、环回、进程令牌、范围与幂等持久化 | automated |
| EV-DESKTOP | `npm test`; `npm run typecheck`; `npm run build` in `apps/desktop` | 一次性恢复密钥确认、React 可信投影、仪式交互、类型和生产 WebView 构建 | automated |
| EV-EXTENSION | `npm test`; `npm run typecheck`; `npm run build` in `apps/browser-extension` | Manifest V3 最小权限、授权正文、失败队列和生产目录 | automated |
| EV-BACKUP | `cargo test -p backup`; `cargo test -p vault --test backup_recovery` | 认证快照、最新删除头、遗忘重放、索引重建与 Vault Key 轮换 | automated |
| EV-STATIC | `cargo fmt --all -- --check`; `cargo clippy --workspace --all-targets -- -D warnings`; `cargo check -p desktop-app --all-targets` | 格式、lint、全目标编译与静态安全边界 | automated |
| EV-PRIVACY | `scripts/run-system-acceptance.ps1 -Mode Validate` | G10 ignore、tracked 私有路径、常见密钥/令牌与本机用户路径扫描 | automated |
| EV-MATRIX | `scripts/run-system-acceptance.ps1 -Mode Validate` | 33 DET、12 FR、50 accepted ADR、8 THR、5 MIG 精确集合、S06R 强制证据与证据外键 | automated |
| EV-PACKAGE | `npm run tauri -- build`; `scripts/run-system-acceptance.ps1 -Mode Smoke` | 版本一致、NSIS 产物、SHA-256、安装版真实创建加密数据库、退出/卸载烟雾 | automated |

## 3. 产品第 6.1 节确定性验收

| ID | 判据 | 自动化证据 | 状态 |
| --- | --- | --- | --- |
| DET-01 | 初始形成 | EV-IDENTITY-INIT | automated |
| DET-02 | 记忆准确 | EV-CORE-MEMORY, EV-RETRIEVAL | automated |
| DET-03 | 时间理解 | EV-CORRECTION, EV-RETRIEVAL | automated |
| DET-04 | 身份隔离 | EV-CORE-MEMORY, EV-CORE-IDENTITY | automated |
| DET-05 | 判断入账 | EV-CORE-MEMORY | automated |
| DET-06 | 长期记忆晋升 | EV-MEMORY, EV-PATTERN | automated |
| DET-07 | 记忆争议 | EV-DISPUTE | automated |
| DET-08 | 争议召回 | EV-DISPUTE, EV-RETRIEVAL | automated |
| DET-09 | 自然表达 | EV-DISPUTE, EV-RUNTIME | automated |
| DET-10 | 主动反思 | EV-REFLECTION | automated |
| DET-11 | 模式门槛 | EV-PATTERN, EV-REFLECTION | automated |
| DET-12 | 模式成熟 | EV-PATTERN, EV-DISPUTE | automated |
| DET-13 | 共同经历 | EV-SHARED | automated |
| DET-14 | 约定约束 | EV-CONSTRAINTS, EV-SHARED | automated |
| DET-15 | 约定退出 | EV-SHARED, EV-CONSTRAINTS, EV-RUNTIME | automated |
| DET-16 | 增量更新 | EV-INGESTION, EV-LINEAGE, EV-OBSIDIAN | automated |
| DET-17 | 块谱系 | EV-LINEAGE | automated |
| DET-18 | 来源坐标 | EV-INGESTION, EV-LINEAGE | automated |
| DET-19 | 延迟理解 | EV-INGESTION, EV-MARKDOWN | automated |
| DET-20 | 格式状态 | EV-INGESTION, EV-MARKDOWN | automated |
| DET-21 | Markdown 方言 | EV-MARKDOWN | automated |
| DET-22 | Obsidian 只读 | EV-OBSIDIAN | automated |
| DET-23 | Obsidian 移除 | EV-OBSIDIAN | automated |
| DET-24 | Markdown 解析边界 | EV-MARKDOWN, EV-INGESTION | automated |
| DET-25 | 后台连续 | EV-HOST, EV-WIN-CAPTURE | automated |
| DET-26 | 纠错传播 | EV-CORRECTION | automated |
| DET-27 | 遗忘传播 | EV-FORGET, EV-BACKUP | automated |
| DET-28 | 身份连续 | EV-IDENTITY-PRESENCE, EV-CORE-IDENTITY, EV-VAULT | automated |
| DET-29 | 对话证据 | EV-CORE-MEMORY, EV-DESKTOP, EV-FORGET | automated |
| DET-30 | 模型迁移 | EV-CORE-IDENTITY, EV-RUNTIME, EV-RUNTIME-PROFILE | automated |
| DET-31 | 数据边界 | EV-VAULT, EV-RUNTIME, EV-RUNTIME-PROFILE | automated |
| DET-32 | 内容隔离 | EV-MARKDOWN, EV-INGESTION, EV-RUNTIME | automated |
| DET-33 | 威胁边界 | EV-VAULT, EV-BROWSER, EV-EXTENSION, EV-PRIVACY | automated |

## 4. 功能需求覆盖

| ID | 需求 | 自动化证据 | 状态 |
| --- | --- | --- | --- |
| FR-01 | 渐进式共同回忆 | EV-IDENTITY-INIT, EV-DESKTOP | automated |
| FR-02 | 上下文收件箱 | EV-INGESTION, EV-MARKDOWN, EV-LINEAGE, EV-OBSIDIAN | automated |
| FR-03 | Windows 日常采集 | EV-WIN-CAPTURE, EV-BROWSER, EV-EXTENSION | automated |
| FR-04 | 时间化事实账本 | EV-CORE-MEMORY, EV-CORRECTION, EV-SHARED | automated |
| FR-05 | 全库检索与工作上下文 | EV-RETRIEVAL, EV-UNDERSTANDING | automated |
| FR-06 | 长期记忆维护 | EV-MEMORY, EV-DISPUTE, EV-PATTERN, EV-CORRECTION, EV-FORGET | automated |
| FR-07 | 第二自我运行时 | EV-IDENTITY-PRESENCE, EV-CORE-IDENTITY, EV-REFLECTION, EV-RUNTIME, EV-RUNTIME-PROFILE | automated |
| FR-08 | 持续对话界面 | EV-DESKTOP, EV-SHARED, EV-DISPUTE, EV-RUNTIME-PROFILE | automated |
| FR-09 | 纠错与遗忘 | EV-CORRECTION, EV-FORGET, EV-BACKUP | automated |
| FR-10 | 本地加密与恢复 | EV-VAULT, EV-BACKUP, EV-RUNTIME-PROFILE | automated |
| FR-11 | 不可信内容隔离 | EV-INGESTION, EV-MARKDOWN, EV-BROWSER, EV-RUNTIME | automated |
| FR-12 | 首版威胁边界 | EV-VAULT, EV-RUNTIME, EV-BROWSER, EV-EXTENSION, EV-PRIVACY | automated |

## 5. accepted ADR 覆盖

| ID | 决策 | 自动化证据 | 状态 |
| --- | --- | --- | --- |
| ADR-0001 | 第二自我是数字对应者 | EV-IDENTITY-INIT, EV-CORE-IDENTITY | automated |
| ADR-0002 | 本地可迁移自我包 | EV-IDENTITY-PRESENCE, EV-CORE-IDENTITY, EV-BACKUP | automated |
| ADR-0003 | 时间化三账本 | EV-CORE-MEMORY, EV-CORRECTION | automated |
| ADR-0004 | 核心访问边界 | EV-RETRIEVAL, EV-RUNTIME | automated |
| ADR-0005 | 事件驱动存在 | EV-IDENTITY-PRESENCE, EV-HOST | automated |
| ADR-0006 | 本人自持恢复密钥 | EV-VAULT, EV-BACKUP | automated |
| ADR-0007 | Inbox 导入而非镜像 | EV-INGESTION, EV-FORGET | automated |
| ADR-0008 | Tauri、React 和 Rust 职责边界 | EV-HOST, EV-DESKTOP, EV-STATIC | automated |
| ADR-0009 | 混合加密保险库 | EV-VAULT, EV-FORGET, EV-BACKUP | automated |
| ADR-0011 | 信任当前 Windows 登录会话 | EV-VAULT, EV-RUNTIME, EV-BROWSER, EV-PRIVACY | automated |
| ADR-0012 | 托盘常驻 Tauri 宿主 | EV-HOST, EV-WIN-CAPTURE, EV-DESKTOP | automated |
| ADR-0013 | 先归档后理解 | EV-INGESTION, EV-MARKDOWN | automated |
| ADR-0015 | 只读 Obsidian 资料源 | EV-OBSIDIAN | automated |
| ADR-0016 | Obsidian 移除保留历史 | EV-OBSIDIAN, EV-RETRIEVAL | automated |
| ADR-0018 | 混合 RAG 与按需深度理解 | EV-RETRIEVAL, EV-UNDERSTANDING | automated |
| ADR-0019 | 稳定结构块与动态窗口 | EV-LINEAGE, EV-RETRIEVAL | automated |
| ADR-0020 | 不可变块引用与显式谱系 | EV-LINEAGE | automated |
| ADR-0021 | 规范文本引用与可选原生定位 | EV-INGESTION, EV-LINEAGE | automated |
| ADR-0022 | 首版只理解 UTF-8 Markdown | EV-INGESTION, EV-MARKDOWN | automated |
| ADR-0023 | Core 内受限纯 Rust Markdown 解析 | EV-MARKDOWN | automated |
| ADR-0024 | 版本化 Markdown 方言 | EV-MARKDOWN | automated |
| ADR-0025 | 清晰本人自述直接入账 | EV-CORE-MEMORY, EV-IDENTITY-INIT | automated |
| ADR-0026 | 每轮对话作为证据保留 | EV-CORE-MEMORY, EV-DESKTOP, EV-FORGET | automated |
| ADR-0027 | 第二自我显式提议持久判断 | EV-CORE-MEMORY, EV-RUNTIME | automated |
| ADR-0028 | 共同经历分类仪式 | EV-SHARED, EV-DESKTOP | automated |
| ADR-0029 | 共同约定候选版本双签 | EV-SHARED | automated |
| ADR-0030 | 次于宪法的关系约束 | EV-CONSTRAINTS, EV-RUNTIME | automated |
| ADR-0031 | 任一方可向未来退出约定 | EV-SHARED, EV-CONSTRAINTS | automated |
| ADR-0032 | 退出采用非对称仪式 | EV-SHARED, EV-RUNTIME, EV-DESKTOP | automated |
| ADR-0033 | 共同约定签署明确边界 | EV-SHARED | automated |
| ADR-0034 | 冲突约定显式整份取代 | EV-SHARED, EV-CONSTRAINTS, EV-RUNTIME | automated |
| ADR-0035 | 长期记忆显式提议 | EV-MEMORY | automated |
| ADR-0036 | 记忆质疑采用说服与争议 | EV-DISPUTE | automated |
| ADR-0037 | 争议记忆自然分层披露 | EV-DISPUTE, EV-RUNTIME | automated |
| ADR-0038 | 狭义关系事件边界 | EV-SHARED | automated |
| ADR-0039 | 身份自主演化受反思使命约束 | EV-IDENTITY-PRESENCE, EV-CORE-IDENTITY | automated |
| ADR-0040 | 可延后的主动反思邀请 | EV-REFLECTION, EV-RUNTIME | automated |
| ADR-0041 | 本人可静默主动反思 | EV-REFLECTION | automated |
| ADR-0042 | 三个独立事件的模式门槛 | EV-PATTERN, EV-REFLECTION | automated |
| ADR-0043 | 模式成熟为稳定第二自我看法 | EV-PATTERN, EV-DISPUTE | automated |
| ADR-0044 | 模式成熟须第二自我显式提议 | EV-PATTERN, EV-RUNTIME | automated |
| ADR-0045 | 创建前需要最小自我介绍 | EV-IDENTITY-INIT | automated |
| ADR-0046 | 用途隔离的保险库密码配置 | EV-VAULT | automated |
| ADR-0047 | 版本化独立双解锁 | EV-VAULT | automated |
| ADR-0048 | OpenAI Responses 运行时家族 | EV-RUNTIME, EV-CORE-IDENTITY | automated |
| ADR-0049 | 加密心跳的单宿主生命周期 | EV-HOST, EV-WIN-CAPTURE | automated |
| ADR-0050 | 固定来源环回浏览器采集 | EV-BROWSER, EV-EXTENSION | automated |
| ADR-0051 | 历史备份携带最新删除头 | EV-BACKUP | automated |
| ADR-0052 | 首次创建一次性展示恢复密钥 | EV-VAULT, EV-HOST, EV-DESKTOP | automated |
| ADR-0053 | 运行时采用 Vault 单档案热切换 | EV-RUNTIME-PROFILE, EV-VAULT, EV-DESKTOP | automated |

## 6. 威胁边界

| ID | 威胁或声明边界 | 自动化证据 | 状态 |
| --- | --- | --- | --- |
| THR-01 | 丢失/脱机复制的保险库与备份不泄漏正文或运行时密钥 | EV-VAULT, EV-BACKUP, EV-RUNTIME-PROFILE, EV-PRIVACY | automated |
| THR-02 | 其他非管理员账户不能用错误密钥或非当前用户封装打开 Vault | EV-VAULT | automated |
| THR-03 | 网络客户端不能远程控制 Core，云端凭据不走明文 HTTP | EV-BROWSER, EV-RUNTIME | automated |
| THR-04 | 外部模型只获得冻结工作上下文，不获得 Vault、密钥或持久身份所有权 | EV-RETRIEVAL, EV-RUNTIME | automated |
| THR-05 | 恶意 Markdown、文件边界和网页内容不执行、不触发工具、不进入控制通道 | EV-INGESTION, EV-MARKDOWN, EV-BROWSER, EV-RUNTIME | automated |
| THR-06 | 浏览器扩展权限等于实际 API 需求，正文只在精确来源授权后读取 | EV-BROWSER, EV-EXTENSION | automated |
| THR-07 | WebView 只有领域 command，运行时密钥只写不回显，关系约束不扩大宪法、安全或现实行动权 | EV-HOST, EV-CONSTRAINTS, EV-DESKTOP, EV-RUNTIME-PROFILE | automated |
| THR-08 | 测试和文档不声称抵抗已控制当前登录会话、管理员、内核或解锁设备物理攻击 | EV-MATRIX, EV-PRIVACY | automated |

## 7. 迁移契约

| ID | 迁移边界 | 自动化证据 | 状态 |
| --- | --- | --- | --- |
| MIG-01 | SQLCipher schema v1→v26 逐版迁移在中断时整体回滚，并以单例默认档案重开 | EV-SCHEMA, EV-RUNTIME-PROFILE | automated |
| MIG-02 | 有数据迁移回填 Claim/谱系/检索状态，派生索引损坏可从权威数据重建 | EV-SCHEMA, EV-VAULT, EV-RETRIEVAL | automated |
| MIG-03 | 热切换运行时后端、模型或 Key 不转移自我包所有权、丢失身份链或混用配置 | EV-CORE-IDENTITY, EV-RUNTIME, EV-RUNTIME-PROFILE | automated |
| MIG-04 | Markdown 解析库变化仍必须保持 `eam-markdown-v1` 等价输出，否则提升契约版本 | EV-MARKDOWN, EV-LINEAGE | automated |
| MIG-05 | 历史备份恢复须先重放最新遗忘、恢复完整运行时档案、重建索引并轮换 Vault Key | EV-BACKUP, EV-FORGET, EV-RUNTIME-PROFILE | automated |

## 8. S31 运行记录

本节只记录最后一次完整通过的 S31 运行；命令输出与中间 JSON 留在被忽略的 `/.local/system-acceptance/`，不携带个人正文进入 Git。下表是完成 S06R-5 后重新冻结的唯一 S32 候选构建。

| 字段 | 结果 |
| --- | --- |
| 状态 | `PASS`（`Full` 模式，18/18 步骤通过） |
| Git head | `1468ca57b9919e8dfc08d428b2885770ae66649a`（S06R-5/S31 构建来源） |
| 执行时间 | 2026-08-04 19:17:11～19:20:44 `+08:00`（212.855 秒） |
| Rust workspace | `cargo fmt`、workspace Clippy `-D warnings`、`cargo test --workspace --no-fail-fast`、desktop all-targets check 全部通过 |
| Desktop React | 19/19，通过 TypeScript 检查与生产构建 |
| Browser extension | 10/10，通过 TypeScript 检查与生产构建 |
| S06R 运行时档案 | `EV-RUNTIME-PROFILE` 覆盖 v25→v26、`KEEP/REPLACE/CLEAR`、write-only Key、恢复、严格合成测试、热切换、下一请求、重启与并发隔离 |
| 静态/隐私/矩阵 | 33 个确定性判据、FR-01～FR-12、50 个 accepted ADR、8 个威胁边界、5 个迁移契约、33 个证据入口完整；256 个文本文件隐私扫描零违规，266 个本地链接零缺失 |
| NSIS 版本 | `0.1.0`；`target/release/bundle/nsis/evrything-about-me_0.1.0_x64-setup.exe`；5,448,409 bytes |
| NSIS SHA-256 | `824203ffd36bd2ca32957c10719edd21c4bfaeb39c34b108aa27eda418f87242` |
| 安装/启动/退出/卸载 | 隔离目录静默安装、启动、关窗保留托盘宿主、进程清理与静默卸载全部通过；仅预置 `bundle.meta` 后，安装版实际创建含 schema v26 默认运行时档案的 `self.db` |

S31 不执行第 6.2 节的十四天纵向验收。该过程只能使用 [S32 观察模板](longitudinal-observation-template.md) 在同一个冻结安装包上完成。
