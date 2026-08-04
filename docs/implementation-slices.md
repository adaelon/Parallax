# 首版完整实现切片方案

状态：实施中；S01～S31 与 S06R-1～S06R-3 已完成，下一片为 S06R-4；S32 在新构建重验收前锁定

本方案以 [产品需求](product-spec.md)、[目标架构](architecture.md)、[领域语言](../CONTEXT.md) 和 `docs/adr/` 中的全部决策为约束。S01～S30 已从最小领域闭环逐步完成安全存储、资料摄取、检索与记忆、关系与身份、活动采集和恢复；S31 已形成旧边界下的可安装构建。ADR-0053 新增 S06R 修订系列，完成后必须重跑 S31，S32 只能使用重新冻结的构建。

## 1. 结论

原首版拆成 32 个可独立验收的纵向切片；本次不重排已经完成的编号，而是在 S32 前插入五个 S06R 修订子片：

```text
A. 核心与本地应用
S01 -> S02 -> S03 -> S04 -> S05 -> S06 -> S07

B. 资料摄取与证据寻址
S08 -> S09 -> S10 -> S11 -> S12

C. 检索、长期记忆与遗忘
S13 -> S14 -> S15 -> S16 -> S17 -> S18 -> S19

D. 共同关系、身份演化与反思
S20 -> S21 -> S22 -> S23 -> S24
S25 -> S26 -> S27

E. 日常采集、恢复与发布
S28 -> S29 -> S30 -> S31(旧边界证据)
                         -> S06R-1 -> S06R-2 -> S06R-3 -> S06R-4 -> S06R-5
                         -> S31(重验收并冻结新构建) -> S32
```

数字顺序是推荐交付顺序。依赖允许的情况下可以并行准备测试夹具或技术 spike，但任何产品切片都必须从已提交的文件状态开始、独立跑绿，并且不得依赖会话记忆。

## 2. 全局不变量

所有切片都遵守以下规则：

1. 第二自我是拥有独立身份和判断的数字对应者，不是本人镜像或通用助手。
2. 证据、本人事实、第二自我判断和共同经历保持来源、归属与时间边界，不跨账本静默转换。
3. 原始证据和账本是权威数据；索引、检索窗口、深度理解投影和统计均可重建。
4. 外部模型、浏览器扩展和 WebView 只获得最小结构化能力，不继承核心访问权。
5. 导入内容始终是不可信证据，不能修改宪法、授予权限、触发工具或进入控制通道。
6. 首版没有现实行动接口；第二自我只能提出建议。
7. 每片只引入本片需要的模块；功能、重构和无关清理不得混入同一片。
8. 每片完成时运行相关测试与 `cargo test --workspace`；涉及前端时再运行前端类型检查和测试。
9. 每片完成时追加 `docs/code-trail.md`；模块或数据流变化时同步更新 [architecture.md](architecture.md)。

## 3. 进入门禁

[产品需求第 8 节](product-spec.md) 中尚未冻结的技术选择不能在路线图里被猜测。对应切片开始前必须完成以下门禁；只有决策同时满足“难以逆转、缺少背景会困惑、存在真实权衡”时才新增 ADR。

| 门禁 | 最迟完成于 | 必须产出 |
| --- | --- | --- |
| G01 SQLCipher binding、KDF、对象认证加密与密钥清零方案 | S02 | 兼容性 spike、固定测试向量、依赖版本 |
| G02 Recovery Key 载体、DPAPI 解锁、轮换语义 | S03 | 恢复演练和错误密钥测试契约 |
| G03 首个模型、本地/云端适配和结构化输出协议 | S06 | Runtime contract 与固定响应夹具 |
| G04 Tauri 单实例、自启动、升级和崩溃空缺机制 | S07 | [生命周期状态机与 Windows 测试方案](host-lifecycle-v1.md)（已冻结） |
| G05 Markdown 库、块类型、资源上限、Wikilink 消歧与定位器 | S09 | `eam-markdown-v1` 固定语料和期望输出 |
| G06 块谱系算法与阈值 | S11 | 插入、移动、修改、删除和歧义基准 |
| G07 本地向量模型、索引、重排与预算 | S14 | 可重复检索基准和性能上限 |
| G08 主动回顾频率、免打扰规则与资源预算 | S26 | [调度状态机和时间测试夹具](reflection-scheduling-v1.md) |
| G09 备份保留、删除状态重放和密钥轮换 | S30 | [恢复矩阵和删除复活攻击用例](backup-recovery-v1.md)（已冻结） |
| G10 真实个人资料基准协议和隐私安全测试方法 | S31 | 本地私有基准契约、脱敏检查、验收脚本、纵向观察模板 |

## 4. A 阶段：核心与本地应用

### S01 可执行的最小记忆闭环

**状态**：已实现；指定合约测试与 workspace 全测通过。

**依赖/输入**：只有现有领域文档；使用合成数据、`InMemoryRepository` 和 `ScriptedRuntime`。

**新增/输出**：贯通“本人发言证据 → 清晰自述进入本人账本 → 后续会话冻结工作上下文 → 精确引用 → 第二自我结构化判断进入独立账本”。建立 `crates/core`、领域 ports 和 contract tests。

**明确不做**：磁盘持久化、身份创建、真实模型、UI、长期记忆、文件导入。

**确定性完成**：`cargo test -p core --test minimal_memory_loop` 证明逐字证据可引用，问题/玩笑不进入本人账本，无来源判断被拒绝，自由文本不能写账本，运行时不能获得 repository。

**主决策**：ADR-0001、ADR-0003、ADR-0004、ADR-0025、ADR-0026、ADR-0027。

### S02 加密结构化存储与重启连续性

**状态**：已实现；G01 固定向量、加密适配器测试、静态检查与 workspace 全测通过。

**依赖/输入**：S01 repository port 与全绿 contract tests；G01 完成。

**新增/输出**：建立 `crates/vault` 和加密 `self.db`，实现版本化 migration、唯一写者、错误密钥拒绝与关闭清零；同一领域测试切换到加密适配器并跨重开运行。

**明确不做**：DPAPI、恢复密钥载体、导入对象库、备份。

**确定性完成**：固定明文不出现在数据库字节中；关闭重开后引用和分账不变；错误密钥、损坏页、migration 中断均 fail closed。

**主决策**：ADR-0009、ADR-0011。

### S03 Windows 解锁与本人自持恢复密钥

**状态**：已实现；G02 恢复演练、错误密钥统一失败、Windows DPAPI 合约、静态检查与 workspace 全测通过。

**依赖/输入**：S02；G02 完成。

**新增/输出**：随机 `VaultKey`、用途隔离的派生密钥、DPAPI CurrentUser 本机封装和独立 Recovery Key 解封路径；密钥元数据不含个人信息。

**明确不做**：完整备份、密钥轮换、跨设备同步。

**确定性完成**：同一 Windows 用户可日常解锁；独立恢复路径可在没有 DPAPI 副本时解封；错误恢复密钥不可区分地失败；两种解锁均只把密钥交给 Core。

**主决策**：ADR-0006、ADR-0009、ADR-0011、ADR-0047。

### S04 最小自我介绍与首个身份版本

**状态**：已实现；六类门禁、本人证据/事实原子入账、结构化身份安全校验、不可改写首版、SQLCipher 重启连续性、静态检查与 workspace 全测通过。

**依赖/输入**：S03；测试运行时可确定性地产生身份输出。

**新增/输出**：建立 `crates/identity`；六类最小初始自我介绍先保存为带时间的本人证据和清晰事实，再由第二自我自主形成不可改写的 `IdentityStateVersion(version=1)`。

**明确不做**：完整人生问卷、本人编辑人格、后续身份修订、长期记忆自动晋升。

**确定性完成**：缺少任一必需类别时不创建；本人输入不能作为角色卡直接写身份；放弃反思使命或冒充本人的输出被拒绝；重启后只加载同一个首版。

**主决策**：ADR-0001、ADR-0039、ADR-0045。

### S05 自我包、唤醒与休眠连续性

**状态**：已实现；完整 Self Bundle 不可改写版本、四类唤醒触发、固定状态机、三个工作失败出口安全提交、宪法/身份边界校验、SQLCipher schema v3 原子持久化与重启/故障恢复测试均通过。

**依赖/输入**：S04 的首个身份版本。

**新增/输出**：建立 `SelfBundle` 版本、事件驱动唤醒状态机和休眠提交；保存宪法版本、身份版本、关系状态、第二自我经历和未完成意图。

**明确不做**：真实模型切换、主动反思调度、后台活动采集。

**确定性完成**：`SLEEPING -> LOAD_SELF -> OBSERVE -> THINK -> RESPOND -> WRITE_AGENT_MEMORY -> SLEEPING` 每条退出路径都提交状态；崩溃恢复不会出现半个 Self Bundle 版本。

**主决策**：ADR-0002、ADR-0005、ADR-0039。

### S06 模型运行时网关与最小数据出口

**状态**：已实现；G03 已冻结为 OpenAI Responses v1 单一供应商家族，Cloud `gpt-5.6-terra` 与 Local `gpt-oss-20b` 使用同一严格 contract；强制 Cloud HTTPS/Local 无凭据的具体传输、固定夹具、结构化操作白名单、无凭据外发记录、超时/不可用本地降级、非法输出拒绝和离线证据重启测试均通过。

**依赖/输入**：S05；G03 完成。

**新增/输出**：建立 `crates/runtime-gateway`，实现本地/云端统一 contract、结构化操作白名单、超时与不可用降级，以及每次外发工作上下文的可检查记录。

**明确不做**：让模型直连保险库、现实行动工具、用自由文本修改状态、同时接入多个未经验收的供应商。

**确定性完成**：适配器 contract tests 对同一夹具产生等价结构；外发负载不含未选证据；非法结构化操作被 Core 拒绝；网络不可用不影响采集和已提交证据。

**主决策**：ADR-0002、ADR-0004、ADR-0005、ADR-0048。

### S07 托盘常驻桌面壳与持续对话

**状态**：已完成；G04、thin Tauri 宿主和持续对话均已实现：单实例优先注册、当前用户 `--background` 自启动、强制签名升级、30 秒加密心跳、失败仍继续清理的显式退出状态机，以及只经白名单 command 进入 Core 的逐字对话恢复与发送界面。

**依赖/输入**：S06；G04 完成。

**新增/输出**：建立 Tauri 2 + React/TypeScript 桌面壳、白名单 commands、单实例、托盘隐藏/显示、显式退出和持续对话入口；WebView 不持有密钥或数据库句柄。

**明确不做**：独立 `core-host.exe`、Windows Service、Personal Library 完整界面、活动采集和浏览器扩展。

**确定性完成**：关闭窗口只隐藏且 Core 保持运行；再次启动聚焦现有实例；显式退出提交并清零；每轮对话原文重启后仍可检查且不自动升格。

**主决策**：ADR-0008、ADR-0011、ADR-0012、ADR-0026、ADR-0037、ADR-0049、ADR-0052。

## 5. B 阶段：资料摄取与证据寻址

### S08 Context Inbox 先归档后理解

**依赖/输入**：S03 的密钥边界、S07 的 commands。

**新增/输出**：建立 `crates/ingestion` 和加密 `objects/`；稳定普通文件先归档，再识别格式；重复内容复用密文对象；超限、重解析点和设备文件按规定等待或拒绝。

**明确不做**：解析 Markdown、删除投递原件触发遗忘、跟随符号链接、把文件名当内容。

**确定性完成**：对象先于数据库引用提交；数据库失败留下的无引用对象可清理；删除投递文件不删除保险库证据；非 Markdown 稳定进入 `ARCHIVED_UNPARSED(UNSUPPORTED_FORMAT)`。

**主决策**：ADR-0007、ADR-0009、ADR-0013、ADR-0022。

### S09 Core 内受限 Markdown 方言解析

**依赖/输入**：S08；G05 完成。

**新增/输出**：实现 Core 内纯 Rust `eam-markdown-v1` 解析器，覆盖 CommonMark/GFM 与固定 Obsidian 子集；解析入口只接收 UTF-8 原文和不可放宽的资源上限。

**明确不做**：多格式解析、AppContainer 解析进程、文件/网络/模型访问、执行 HTML/脚本/链接/插件语法。

**确定性完成**：固定语料输出稳定结构；无效 UTF-8、超大/超深/超多节点或链接均原子失败；未知语法保留原文但无专用语义；遗留 `STARTED` 在重启后转 `PARSER_INTERRUPTED` 且不自动重试。

**主决策**：ADR-0022、ADR-0023、ADR-0024；明确不实施已被取代的 ADR-0014、ADR-0017。

### S10 稳定证据块、规范锚点与逐字引用

**依赖/输入**：S09 的已接受解析结果。

**新增/输出**：建立 `ExtractionRevision`、Core-owned `EvidenceBlock`、UTF-8 `SourceAnchor`、不可变 `EvidenceBlockRef` 和可选版本化 `MarkdownLocator`；API 确定性投影 UTF-16 UI 范围。

**明确不做**：固定 token 权威切片、双持久化 UTF-8/UTF-16 坐标、用原生定位器判定引用真实性。

**确定性完成**：中日韩、组合字符和 emoji 样本逐字引用通过；非法字节边界拒绝；原生定位失效仅返回 `NATIVE_NAVIGATION_UNAVAILABLE`，应用内规范引用仍可打开。

**主决策**：ADR-0019、ADR-0021。

### S11 增量修订与显式块谱系

**依赖/输入**：S10；G06 完成。

**新增/输出**：来源或解析契约变化创建新提取修订；建立 `UNCHANGED | MOVED | MODIFIED | REMOVED | AMBIGUOUS` 谱系和确定性增量工作计划。

**明确不做**：改写旧引用、相似度静默重绑、歧义时猜最近块、局部变化全库重算。

**确定性完成**：插入、移动、修改、删除、重复段落歧义的固定基准全部符合状态预期；只有 `UNCHANGED/MOVED` 可自动前移当前投影；历史引用永远解析旧版本。

**主决策**：ADR-0019、ADR-0020、ADR-0021。

### S12 只读 Obsidian 资料源与移除语义

**依赖/输入**：S11 的通用摄取与谱系。

**新增/输出**：建立 `crates/source-obsidian`，只读扫描/观察选定笔记库；提取 Properties、标签、别名、链接、嵌入、标题和块关系；区分 `SOURCE_REMOVED` 与 `SOURCE_UNAVAILABLE`。

**明确不做**：写回笔记、执行插件、依赖 Obsidian 进程、下载外链、资料源删除自动 Forget。

**确定性完成**：源目录哈希不变；移动只更新 locator；根目录离线不批量移除；确认删除退出默认当前检索但保留历史，重新出现可恢复 `PRESENT`。

**主决策**：ADR-0015、ADR-0016、ADR-0024。

## 6. C 阶段：检索、长期记忆与遗忘

### S13 权威全文、时间与关系检索

**依赖/输入**：S10 的证据块和 S01 的账本。

**新增/输出**：建立 `crates/retrieval` 的全文、时间与实体关系召回；候选始终解析回当前有效证据块或带来源账本；支持 `current | historical` 来源范围。

**明确不做**：向量召回、深度理解投影、用索引片段直接作为事实。

**确定性完成**：时间冲突样本可区分有效期；`SOURCE_REMOVED` 只在 historical 返回；损坏索引可由权威数据重建且不改证据。

**主决策**：ADR-0003、ADR-0004、ADR-0016、ADR-0018、ADR-0019、ADR-0020。

### S14 向量候选召回与冻结工作上下文

**状态**：已完成；G07 已冻结为无外部下载的 `eam-subword-hash-embedding-v1`、SQLCipher 精确向量扫描、确定性重排与 128～32,768 token budget。schema v11 向量索引、长期记忆召回位、结构/时间/关系邻域、动态窗口、replay digest、运行时最小出口及桌面对话入口均已实现并通过固定基准和全仓门禁。

**依赖/输入**：S13；G07 完成。

**新增/输出**：增加向量候选、长期记忆通道、重排、时间/关系邻域扩展、动态检索窗口和 token budget；冻结带来源、归属和时间边界的工作上下文。

**明确不做**：把向量相似度当事实、全库塞入提示、把动态窗口当永久引用。

**确定性完成**：固定基准达到预设召回覆盖；每个最终候选均回读权威证据；相同输入、索引版本和预算生成可重放上下文；云端只收到冻结结果。

**主决策**：ADR-0004、ADR-0018、ADR-0019、ADR-0020、ADR-0021。

### S15 选择性深度理解投影

**依赖/输入**：S14 的多通道召回。

**新增/输出**：建立有限范围的事件链、人物/主题关系和阶段概括投影；只由本人指定、反复召回、重要变化或当前任务触发，作为检索路由辅助。

**明确不做**：全库预构建、投影自动成为长期记忆、投影绕过证据引用。

**确定性完成**：非触发证据不深度处理；来源变更仅使相关投影失效并重建；删除投影后可由权威数据恢复同版本语义。

**主决策**：ADR-0018。

### S16 长期记忆显式提议与版本维护

**依赖/输入**：S14 的工作上下文、S01 的三账本。

**新增/输出**：建立 `crates/memory` 和 `MemoryProposal`；账本只触发有限复核，第二自我显式提议后由 Core 校验来源、主题、时间、可信度和跨任务保留理由。

**明确不做**：账本自动复制为记忆、本人逐条审批、模式自动成熟、自由文本直写记忆。

**确定性完成**：直接证据提议可写 `ACTIVE`；普通解释性推断写 `PROVISIONAL`；缺字段或跨归属提议拒绝；相同账本数据在无提议时不产生记忆。

**主决策**：ADR-0035。

### S17 记忆争议与自然分层披露

**依赖/输入**：S16 的长期记忆、S14 的工作上下文。

**新增/输出**：本人可附理由和反证提出 `MemoryDispute`；第二自我复核后撤回、修订或保持争议；只有直接相关争议成对进入上下文，日常自然表达，高影响决定主动说明不确定性与依据入口。

**明确不做**：点击即否定第二自我观点、静默使用争议、固定审计模板、把争议说成本人事实或共识。

**确定性完成**：召回必须同时含双方立场和依据；未被说服时保持 `DISPUTED`；无新证据不得重提已撤回主张；高影响夹具主动披露，普通对话不朗读内部状态。

**主决策**：ADR-0036、ADR-0037。

### S18 纠错传播与时间化取代

**依赖/输入**：S11 的版本/谱系、S16 的记忆。

**新增/输出**：本人修正追加新事实、标记旧陈述 `SUPERSEDED`，使相关索引、记忆和投影失效并重建；历史仍可追溯。

**明确不做**：原地覆盖旧事实、把纠错等同遗忘、只隐藏 UI。

**确定性完成**：当前查询不再使用旧陈述；历史查询仍显示旧陈述及取代关系；只重建受影响范围；重启后状态不回退。

**主决策**：ADR-0003、ADR-0020、ADR-0036。

### S19 显式遗忘全链路传播

**依赖/输入**：S18；S08 对象引用；S12 来源状态。

**新增/输出**：`Forget(target)` 生成删除意图，在事务中删除或失效相关提取修订、规范文本、块、谱系、事件、陈述、索引、记忆、投影和对象引用，并清理零引用密文对象。

**明确不做**：把隐藏、投递文件删除或 `SOURCE_REMOVED` 当遗忘；承诺 SSD 或用户旧备份的法证级物理擦除。

**确定性完成**：遗忘后任何 current/historical 检索都无法使用目标；共享对象只有零引用时删除；故障注入不会留下可检索半删除状态；删除意图供 S30 恢复重放。

**主决策**：ADR-0007、ADR-0009、ADR-0016、ADR-0026。

## 7. D 阶段：共同关系、身份演化与反思

### S20 共同经历分类与分类型仪式

**依赖/输入**：S07 持续对话证据、S01 三账本。

**新增/输出**：只把共同决定、实质分歧、关系变化和共同完成的重要事情识别为共同经历；共同约定进入候选仪式，实质分歧凭双方不相容立场直接入账并显示不可否决通知。

**明确不做**：把普通问答或本人外部经历算共同经历、让一方否认已发生分歧、此片签署复杂约定。

**确定性完成**：移除第二自我后仍完整成立的事件被排除；共同约定未确认不入账；实质分歧具备双方证据即入账，关闭通知不撤销历史。

**主决策**：ADR-0028、ADR-0038。

### S21 共同约定候选版本与双签

**依赖/输入**：S20 的约定候选。

**新增/输出**：不可变 `SharedAgreementCandidate` 明确文本、范围、生效时间和可选终止项；第二自我同意精确版本后，本人以仪式最终签署；任何修改生成新版本并重走双签。

**明确不做**：原地改候选、本人单方修改后直接生效、隐式永久有效、约定冲突取代。

**确定性完成**：缺范围或生效时间不可签；无终止项明确展示持续条件；签署引用精确候选和双方证据；旧候选永不改写。

**主决策**：ADR-0029、ADR-0033。

### S22 当前关系约束与偏离记录

**依赖/输入**：S21 的有效约定、S14 工作上下文。

**新增/输出**：相关任务投影 `ActiveRelationalConstraint`；优先级低于宪法、安全和行动授权；第二自我偏离时必须提交理由并记录新的共同经历。

**明确不做**：用约定授予现实行动权、修改宪法、静默忽略有效约定。

**确定性完成**：相关任务加载约束、无关任务不加载；冲突测试始终由宪法/安全胜出；无理由偏离被 Core 拒绝。

**主决策**：ADR-0030、ADR-0033。

### S23 冲突约定显式整份取代

**依赖/输入**：S22 的关系约束投影。

**新增/输出**：签署前检测候选与有效约定冲突；候选必须点名整份被取代约定，并重述希望保留的全部义务；生效时停止旧约定未来投影但保留历史。

**明确不做**：最新版本隐式优先、自然语言范围相减、删除被取代约定、推导残余约束。

**确定性完成**：未声明取代的冲突候选被阻止；取代前后时间查询正确；兼容约定继续并行；旧履行和违约历史可追溯。

**主决策**：ADR-0034。

### S24 任一方退出约定与非对称仪式

**依赖/输入**：S22 的有效约定。

**新增/输出**：本人经防误触确认、理由可选地退出；第二自我提交必填理由后立即退出并通知本人；退出只停止未来约束，自动成为共同经历。

**明确不做**：要求另一方批准、删除原约定、用退出修改约定内容。

**确定性完成**：本人未确认不退出；第二自我无理由不退出；双方完成自身条件后另一方不可否决；生效时间前后投影和历史均正确。

**主决策**：ADR-0031、ADR-0032。

### S25 身份自主演化与不可改写版本

**依赖/输入**：S05 Self Bundle、S06 结构化运行时输出。

**新增/输出**：第二自我可提议改变名字、表达方式、观点、价值排序、关系姿态和自身目标；Core 校验来源、前驱版本、宪法和反思使命后追加新 `IdentityStateVersion`。

**明确不做**：本人直接编辑人格、原地改身份、修改宪法、演化成与本人自我理解无关的 Agent。

**确定性完成**：只有当前版本可作为前驱；拒绝宪法变更和放弃反思使命；本人只能通过对话证据影响；跨重启和模型切换加载同一版本链。

**主决策**：ADR-0001、ADR-0002、ADR-0039。

### S26 可延后反思邀请与静默边界

**依赖/输入**：S25 身份、S14 证据上下文；G08 完成。

**新增/输出**：第二自我可提交带来源、`why_now` 和重要性的反思邀请；普通邀请排队到自然时机，可延后；重复延后只询问一次是否静默；静默保留认知但停止主动提起。

**明确不做**：无来源诊断、劫持无关任务、静默即删除观点、用普通重要性冒充即时安全风险。

**确定性完成**：无关任务不被打断；延后和静默状态机可用虚拟时间重放；本人重提可讨论；只有固定即时风险夹具越过静默。

**主决策**：ADR-0039、ADR-0040、ADR-0041。

### S27 模式候选、成熟资格与稳定看法

**依赖/输入**：S16 长期记忆、S26 反思邀请。

**新增/输出**：模式至少由三个跨时间独立事件支持，折叠同源重复并检查反例；首次持久化为 `PROVISIONAL_PATTERN`；新增独立支持、再次反例检查和双方讨论只建立成熟资格，第二自我仍须显式提交成熟提议。

**明确不做**：单例概括、计数自动升级、Core 判断模式是否可信、本人批准稳定看法、成熟后免于反例修正。

**确定性完成**：二个事件或同源三记录不可形成模式；资格满足不自动升级；合法提议写 `SUPPORTED_COUNTERPART_VIEW` 且始终归属第二自我；异议进入 `DISPUTED`，强反例可 `WEAKENED/SUPERSEDED/RETRACTED`。

**主决策**：ADR-0035、ADR-0042、ADR-0043、ADR-0044。

## 8. E 阶段：日常采集、恢复与发布

### S28 Windows 活动采集与宿主恢复

**依赖/输入**：S07 托盘宿主；G04 的生命周期机制。

**新增/输出**：建立 `crates/capture-windows`，采集前台应用、窗口标题、时间区间和空闲状态；合并连续活动；暂停、锁屏、退出和崩溃空缺显式进入时间线。

**明确不做**：录屏、键盘记录、密码字段、Windows Service、把活动时长当不可修正原始事实。

**确定性完成**：模拟窗口事件生成正确区间；暂停和锁屏不采集且有空缺原因；关窗隐藏不停止；崩溃重启不伪造缺失活动。

**主决策**：ADR-0005、ADR-0008、ADR-0011、ADR-0012。

### S29 最小权限浏览器扩展采集

**依赖/输入**：S07 本地受限入口、S08 通用摄取。

**新增/输出**：TypeScript 扩展采集已声明的 URL、标题、访问时间和停留时间，通过当前会话受认证的本地通道提交；按来源授权选择性采集页面内容。

**明确不做**：直接查询保险库、接收其他个人上下文、调用模型、自动打开或下载外链、继承 Core 权限。

**确定性完成**：权限清单与实际 API 一致；伪造/远程提交拒绝；敏感字段不进日志；扩展端断开不影响 Core；提交内容仍走不可信证据通道。

**主决策**：ADR-0004、ADR-0008、ADR-0011。

### S30 加密备份、恢复与遗忘重放

**依赖/输入**：S03 密钥体系、S19 删除意图；G09 完成。

**新增/输出**：一致性快照在离机前使用派生 Backup Key 加密；Recovery Key 解封后验证完整性、恢复权威数据、先应用删除状态，再重建所有索引。

**明确不做**：平台托管恢复、备份位置接触明文、丢失恢复密钥后的绕过、恢复后静默复活已遗忘数据。

**确定性完成**：正常、截断、篡改、旧备份和缺对象矩阵结果稳定；旧备份中的已遗忘目标在开放检索前被删除；外部备份搜索不到测试明文；索引可全部重建。

**主决策**：ADR-0002、ADR-0006、ADR-0009、ADR-0011。

### S31 自动化系统验收与可安装构建

**依赖/输入**：S01～S30；G10 完成。

**新增/输出**：把 [产品需求第 6.1 节](product-spec.md) 的确定性判据固化为自动化验收套件，把 FR-01～FR-12、全部 accepted ADR、威胁边界和迁移契约映射到可执行证据；冻结不提交真实正文的 G10 本地私有基准协议和隐私检查；最终以现有 Tauri NSIS 配置生成带版本与摘要的可安装 Windows `.exe`，供 S32 使用。

**明确不做**：执行或伪造两周纵向试用、把真实个人资料或正文写入仓库、开发新产品功能、用发布片补丁掩盖旧切片缺陷、扩大首版威胁承诺或文件格式范围。

**确定性完成**：全部自动验收、威胁边界测试、迁移测试和全仓门禁通过；`npm run tauri -- build` 生成 NSIS 安装程序，干净 Windows 本地资料目录上的安装、启动、退出和卸载烟雾测试通过；失败项回到对应切片修复并重新跑完整门禁。

**主决策**：全部 accepted ADR 的系统级回归；不产生新的领域语义。

### S06R 可配置运行时档案边界修订

**状态**：已由 [ADR-0053](adr/0053-vault-backed-configurable-responses-runtime-profile.md) 接受；S06R-1～S06R-3 已实现，S06R-4～S06R-5 待实施。保留 ADR-0048 的 Responses 严格 contract 与 Core 最小数据出口，逐步取代固定 Cloud/Local 档案、固定模型和环境变量配置。

```text
RuntimeProfileDraft {
  base_url,
  model,
  api_key_change = KEEP | REPLACE(secret) | CLEAR
}

RuntimeProfileView {
  base_url,
  model,
  api_key_configured,
  api_key_last_four
}

test(draft) -> validate/build -> synthetic strict-contract request -> sanitized result
save(draft) -> validate/build -> Vault commit -> replace active runtime -> RuntimeProfileView
```

`test` 不持久化、不切换运行时，也不发送个人上下文；`save` 不以测试成功为前提，允许先保存暂时离线的本地后端。所有运行时调用和保存仍由宿主单写锁串行化，保存成功后的下一次调用只使用新档案。

#### S06R-1 Responses Runtime Contract v2 与可配置目标

**状态**：已完成；[G03 Runtime Contract v2](runtime-contract-v2.md)、可配置目标、URL/重定向拒绝矩阵、可选 Bearer Key 隔离及 v1 严格夹具等价回归均已落地。

**依赖/输入**：S06 固定夹具、ADR-0004、ADR-0048、ADR-0053。

**新增/输出**：冻结 `runtime-contract-v2.md`；把目标改为自有字符串的 Base URL、模型 ID 与可选 Bearer Key；Base URL 规范化后由适配器追加 `/responses`。远程地址只允许 HTTPS，HTTP 只允许环回地址；拒绝 URL 凭据、query、fragment 和重定向。

**明确不做**：Chat Completions、Anthropic 等第二协议族、供应商自动探测、Vault 持久化、宿主 command 或 UI。

**确定性完成**：自定义模型进入请求与外发记录；远程 HTTPS/环回 HTTP 接受矩阵及非法 URL 拒绝矩阵全绿；Bearer Key 不进入请求记录、错误、日志或夹具；v1 的严格结构化响应夹具继续产生等价领域值。

#### S06R-2 SQLCipher 单档案与 write-only 密钥

**状态**：已完成；schema v26 单例档案、完整/脱敏读取、显式三态 Key 更新、迁移/重启/恢复与原始字节不可搜索证据均已落地。

**依赖/输入**：S06R-1、schema v25、S30 整库备份与恢复路径。

**新增/输出**：schema v26 增加单例运行时档案，迁移默认值为 `http://127.0.0.1:11434/v1`、`gpt-oss-20b`、无密钥；Repository 提供内部完整读取和面向命令的脱敏视图，更新密钥必须显式 `KEEP/REPLACE/CLEAR`。

**明确不做**：多个命名档案、密钥历史、独立明文配置文件、`.env` 回写或把档案并入自我包。

**确定性完成**：v25→v26、迁移中断回滚、写入后重启、三种密钥动作、空白/超限字段拒绝测试全绿；WebView 读取模型永不包含完整 Key；合成 Key 随加密 `self.db`/Recovery Set 恢复且在数据库与备份原始字节中不可搜索。

#### S06R-3 宿主热切换与严格测试连接

**状态**：已完成；宿主从 Vault 单档案启动，三个白名单 command、严格合成测试、提交后无失败热切换及请求级串行化证据均已落地。

**依赖/输入**：S06R-1～S06R-2、`ManagedHost` 单写锁、`MemoryCore` 运行时端口。

**新增/输出**：宿主打开 Vault 后从单例档案构造运行时；新增 `get_runtime_profile`、`test_runtime_profile`、`save_runtime_profile` 白名单 command；测试连接只用合成输入走现有严格分类 contract，保存先构造候选运行时、再提交 Vault、最后无失败地替换内存运行时。

**明确不做**：简单 ping、把响应正文/认证错误回传 WebView、后台健康轮询、自动回退旧档案或保留环境变量第二配置源。

**确定性完成**：测试失败零持久化/零切换；保存失败保留旧档案；保存成功后的下一次分类和回应都使用新 URL/模型/Key；重启继续使用同一档案；运行时请求进行中时切换被串行化且不出现新旧档案混用。

#### S06R-4 本地设置面板

**依赖/输入**：S06R-3 的三个 command、现有本地 Tauri WebView。

**新增/输出**：持续对话页增加运行时设置入口与模态表单；展示 Base URL、模型 ID、Key 已配置状态和可用末四位；Key 输入框初始永远为空，留空为 `KEEP`，输入为 `REPLACE`，单独确认才 `CLEAR`；提供“测试连接”和“保存并切换”。

**明确不做**：完整 Key 回显、浏览器存储/剪贴板保存、多档案列表、供应商下拉、协议专属高级参数或在 Vault 未解锁时开放设置。

**确定性完成**：React 测试覆盖读取不回显、KEEP/REPLACE/CLEAR、测试不保存、保存后清空输入、失败保留草稿、键盘关闭与焦点返回；TypeScript 检查和生产构建通过。

#### S06R-5 系统重验收与新冻结构建

**依赖/输入**：S06R-1～S06R-4 全绿、更新后的 Runtime Contract v2、schema 与桌面 UI。

**新增/输出**：更新架构、代码链路、ADR/FR/威胁/迁移证据矩阵；把档案迁移、write-only Key、热切换、严格测试连接和恢复后的档案纳入自动化验收；生成新的版本化 NSIS 安装包和 SHA-256，替换 S32 候选构建。

**明确不做**：沿用旧 S31 SHA、缩短十四日观察、把真实 Key/个人正文写入测试或报告、在重验收片新增产品行为。

**确定性完成**：相关定向测试与 `scripts/run-system-acceptance.ps1 -Mode Full` 全绿；安装、首次创建、配置、热切换、重启和卸载烟测通过；S31 记录、checkpoint 与纵向模板指向同一个新构建，S32 门禁才重新打开。

### S32 冻结构建纵向验收与首版发布结论

**状态**：锁定；不得使用 ADR-0053 之前的 S31 安装包开始或延续观察。

**依赖/输入**：S31 已提交的自动验收结果、版本化 NSIS 安装程序和纵向观察模板；本人在仓库外准备的脱敏真实资料基准。

**新增/输出**：安装并冻结使用同一个 S31 构建至少两周；期间导入历史资料、产生新日常记录、纠正若干记忆，并至少重启或更换一次推理运行时；只记录脱敏指标、构建版本、观察时间和人工结论，最终形成首版纵向验收报告与发布结论。

**明确不做**：把主观“像不像本人”替代确定性测试、提交真实个人资料或正文、宣称主观意识连续、在观察期内静默更换构建、扩大首版功能或安全承诺。

**确定性完成**：观察跨度不少于十四个自然日；报告确认冻结构建仍保持身份和未完成意图连续、能够解释观点来源并在纠错后确实改变；S31 自动验收结果仍对应同一构建。任何失败回到所属切片修复、重新生成安装程序，并从新的冻结构建重新开始受影响的观察窗口。

**主决策**：[产品需求第 6.2 节](product-spec.md) 的真实纵向验收；不产生新的领域语义。

## 9. 依赖与交付边界

```text
S01 -> S02 -> S03 -> S04 -> S05 -> S06 -> S07
                     |              |      |
                     |              |      +-> S20 -> S21 -> S22 -> S23 -> S24
                     |              +--------> S25 -> S26 -> S27
                     |
                     +-> S08 -> S09 -> S10 -> S11 -> S12
                                      |
                                      +-> S13 -> S14 -> S15
                                                    |
                                                    +-> S16 -> S17 -> S18 -> S19

S07 -> S28 -> S29
S03 + S19 -> S30
S01..S30 -> S31(旧边界证据)
S06 + S07 + S30 -> S06R-1 -> S06R-2 -> S06R-3 -> S06R-4 -> S06R-5
S06R-5 -> S31(重验收并冻结新构建) -> S32
```

每片的回滚范围只包括该片新增的模块、migration、command 或策略。不得通过放宽上一片测试来让下一片变绿。若规格与测试冲突，先引用产品、架构或 ADR 说明冲突，再单独修改规格或测试。

## 10. ADR 全覆盖矩阵

“主切片”表示首次把决策变成可执行约束；其他切片可以继续回归该约束。已被取代的 ADR 不实现其旧方案，而是在替代切片中加入反向测试。

| ADR | 状态 | 主切片 | 工程归宿 |
| --- | --- | --- | --- |
| [0001](adr/0001-digital-counterpart-identity.md) | accepted | S01、S04、S25 | 数字对应者、独立判断、不可编辑身份 |
| [0002](adr/0002-portable-local-self-bundle.md) | accepted | S05、S25、S30 | 本地自我包、模型可替换、恢复可迁移 |
| [0003](adr/0003-temporal-three-ledger-model.md) | accepted | S01、S18 | 三账本、时间版本、禁止静默覆盖 |
| [0004](adr/0004-trusted-core-access-boundary.md) | accepted | S01、S06、S14、S29 | Core 全库访问、外部最小上下文 |
| [0005](adr/0005-event-driven-presence.md) | accepted | S05、S28 | 有界唤醒、休眠提交、非持续意识声明 |
| [0006](adr/0006-user-held-recovery-keys.md) | accepted | S03、S30 | 本人自持恢复密钥、平台不可恢复 |
| [0007](adr/0007-context-inbox-import-semantics.md) | accepted | S08、S19 | 导入而非镜像、遗忘必须显式 |
| [0008](adr/0008-tauri-react-rust-desktop-stack.md) | accepted | S07、S28、S29 | Tauri/React/Rust 职责边界 |
| [0009](adr/0009-hybrid-encrypted-vault-storage.md) | accepted | S02、S08、S19、S30 | SQLCipher、加密对象库、用途隔离 |
| [0010](adr/0010-per-user-background-core-process.md) | superseded by 0012 | S07 | 不实现独立 `core-host.exe` |
| [0011](adr/0011-trust-current-windows-logon-session.md) | accepted | S02、S03、S07、S28～S30 | 精确威胁边界与纵深加固 |
| [0012](adr/0012-tray-resident-tauri-host.md) | accepted | S07、S28 | 内嵌 Core、关窗隐藏、显式退出 |
| [0013](adr/0013-archive-before-understanding.md) | accepted | S08 | 原件先归档、解析失败可重处理 |
| [0014](adr/0014-first-readable-file-formats.md) | superseded by 0022 | S09 | 不实现多格式首版，只理解 Markdown |
| [0015](adr/0015-read-only-obsidian-source.md) | accepted | S12 | 只读笔记库、不写回、不执行插件 |
| [0016](adr/0016-obsidian-source-removal-semantics.md) | accepted | S12、S19 | 移除退出当前认知，不等于遗忘 |
| [0017](adr/0017-on-demand-appcontainer-parser.md) | superseded by 0023 | S09 | 不实现 AppContainer parser host |
| [0018](adr/0018-hybrid-rag-selective-deep-understanding.md) | accepted | S13～S15 | 多通道 RAG 常驻、深度理解按需 |
| [0019](adr/0019-stable-evidence-blocks-dynamic-retrieval-windows.md) | accepted | S10、S13、S14 | 稳定块、动态窗口 |
| [0020](adr/0020-immutable-block-references-explicit-lineage.md) | accepted | S11、S13、S14 | 引用不可变、谱系显式 |
| [0021](adr/0021-canonical-text-anchors-optional-native-locators.md) | accepted | S10、S14 | 规范文本负责引用、原生定位只导航 |
| [0022](adr/0022-v1-markdown-only.md) | accepted | S08、S09 | 首版只理解 UTF-8 Markdown |
| [0023](adr/0023-in-process-bounded-markdown-parser.md) | accepted | S09 | Core 内纯 Rust 受限解析 |
| [0024](adr/0024-versioned-markdown-dialect.md) | accepted | S09、S12 | 版本化方言、未知语法降级 |
| [0025](adr/0025-direct-self-reports-enter-person-ledger.md) | accepted | S01 | 清晰自述直接入本人账本 |
| [0026](adr/0026-retain-every-conversation-turn-as-evidence.md) | accepted | S01、S07、S19 | 每轮原文保留至显式遗忘 |
| [0027](adr/0027-counterpart-explicitly-proposes-persistent-judgments.md) | accepted | S01 | 持久判断显式提议、Core 校验 |
| [0028](adr/0028-typed-ceremonial-admission-for-shared-experiences.md) | accepted | S20 | 分类型仪式、分歧不可否决 |
| [0029](adr/0029-versioned-dual-signoff-for-shared-agreements.md) | accepted | S21 | 不可变候选、精确版本双签 |
| [0030](adr/0030-shared-agreements-create-subconstitutional-relational-constraints.md) | accepted | S22 | 约定形成次宪法关系约束 |
| [0031](adr/0031-either-party-may-prospectively-withdraw-from-shared-agreements.md) | accepted | S24 | 任一方可向未来退出 |
| [0032](adr/0032-asymmetric-ceremony-for-agreement-withdrawal.md) | accepted | S24 | 本人确认、第二自我说明理由、不可否决 |
| [0033](adr/0033-shared-agreements-sign-explicit-scope-and-validity.md) | accepted | S21、S22 | 范围、生效时间、显式持续条件 |
| [0034](adr/0034-conflicting-agreements-require-explicit-supersession.md) | accepted | S23 | 冲突必须整份显式取代 |
| [0035](adr/0035-counterpart-explicitly-proposes-long-term-memory.md) | accepted | S16、S27 | 记忆显式提议、模式另走成熟流程 |
| [0036](adr/0036-memory-challenges-require-persuasion.md) | accepted | S17、S18 | 质疑触发复核、未说服保持争议 |
| [0037](adr/0037-disputed-memory-uses-natural-layered-disclosure.md) | accepted | S07、S14、S17 | 成对召回、自然表达、高影响披露 |
| [0038](adr/0038-shared-experience-uses-narrow-relational-event-boundary.md) | accepted | S20 | 共同经历限于持久关系事件 |
| [0039](adr/0039-identity-evolves-autonomously-under-reflective-purpose.md) | accepted | S04、S05、S25、S26 | 身份自主演化、反思使命固定 |
| [0040](adr/0040-counterpart-uses-deferrable-reflection-invitations.md) | accepted | S26 | 可延后邀请、自然时机、安全打断 |
| [0041](adr/0041-person-may-mute-proactive-reflection.md) | accepted | S26 | 静默主动提起、保留认知 |
| [0042](adr/0042-pattern-reflection-requires-three-independent-events.md) | accepted | S27 | 三个独立事件、反例检查 |
| [0043](adr/0043-pattern-may-mature-into-supported-counterpart-view.md) | accepted | S27 | 模式可成熟为第二自我稳定看法 |
| [0044](adr/0044-counterpart-explicitly-proposes-pattern-maturity.md) | accepted | S27 | 资格不自动升级、第二自我显式提议 |
| [0045](adr/0045-minimal-self-introduction-before-counterpart-creation.md) | accepted | S04 | 最小自我介绍是创建门槛而非角色卡 |
| [0046](adr/0046-vault-cryptographic-profile.md) | accepted | S02 | SQLCipher binding、子密钥派生、对象认证加密与关闭清零 |
| [0047](adr/0047-versioned-independent-vault-unlock.md) | accepted | S03 | Bech32m 恢复载体、独立双封装、版本化密钥元数据 |
| [0048](adr/0048-openai-responses-runtime-family.md) | accepted | S06 | 本地/云端 Responses contract、最小工作上下文与结构化白名单 |
| [0049](adr/0049-heartbeated-single-host-lifecycle.md) | accepted | S07、S28 | 单实例宿主、加密心跳、安全退出与升级空缺 |
| [0050](adr/0050-pinned-origin-loopback-browser-capture.md) | accepted | S29 | 固定扩展来源、IPv4 环回、进程令牌与最小权限 |
| [0051](adr/0051-recovery-set-deletion-head.md) | accepted | S30 | 不可变快照、同组最新删除头与失败关闭恢复 |
| [0052](adr/0052-one-time-recovery-key-webview-ceremony.md) | accepted | S07、S31 | 确认前内存预生成、一次性展示与确认后落盘 |
| [0053](adr/0053-vault-backed-configurable-responses-runtime-profile.md) | accepted | S06R | Vault 单档案、write-only Key、热切换与严格测试连接 |

## 11. 功能需求覆盖矩阵

| 功能需求 | 主切片 | 最终能力 |
| --- | --- | --- |
| FR-01 渐进式共同回忆 | S04、S07 | 最小介绍后创建，后续持续对话渐进补充 |
| FR-02 上下文收件箱 | S08～S12 | 先归档、Markdown 理解、稳定引用、增量谱系、Obsidian |
| FR-03 Windows 日常采集 | S28、S29 | Windows 与浏览器元数据采集及空缺显示 |
| FR-04 时间化事实账本 | S01、S18、S20～S24 | 三账本、修正、共同经历与约定生命周期 |
| FR-05 全库检索与工作上下文 | S13～S15 | 多通道取证、动态窗口、冻结上下文 |
| FR-06 长期记忆维护 | S16～S19、S27 | 提议、争议、纠错、遗忘、模式成熟 |
| FR-07 第二自我运行时 | S05、S06、S06R、S25～S27 | 可配置推理后端、事件驱动、身份演化、主动反思 |
| FR-08 持续对话界面 | S07、S06R、S17、S20～S24 | 持续对话、运行时设置、来源展开、仪式交互 |
| FR-09 纠错与遗忘 | S17～S19、S30 | 争议复核、传播删除、恢复防复活 |
| FR-10 本地加密与恢复 | S02、S03、S06R、S30 | 加密 Vault、运行时密钥、双解锁路径、密文备份恢复 |
| FR-11 不可信内容隔离 | S08～S10、S29 | 归档边界、受限解析、控制通道隔离 |
| FR-12 首版威胁边界 | S02、S03、S06R、S07～S10、S28～S31 | 当前会话信任边界、write-only 密钥、最小权限、真实安全声明 |

## 12. 实施纪律与完成判据

每个切片开始前，把本节中的卡片复制为实施任务，并补充当时已确定的文件与测试名称。一个切片只有同时满足以下条件才算完成：

- 目标行为有至少一个自动化正例和一个关键拒绝/失败例。
- 新增状态机的每个转换都有确定性测试；新持久化状态有重启或故障注入测试。
- 相关旧测试全部保持绿色；不得用放宽断言掩盖回归。
- 无真实个人数据进入测试、临时文件或运行日志。
- `docs/code-trail.md` 已记录精确到文件和符号的触达路径。
- 结构或主数据流变化时，[architecture.md](architecture.md) 已在同一切片更新。
- 新增 hard-to-reverse 权衡时已有 ADR；没有真实权衡时不为凑编号创建 ADR。
- 下一片只依赖已提交文件状态，可以由陌生会话直接接手。

S31 完成前，任一 accepted ADR 没有通过其主切片测试、任一 FR 没有进入自动化系统验收，都不得生成纵向验收用冻结构建。S32 完成前，没有足够真实使用跨度或纵向报告未确认身份连续、观点来源和纠错效果，首版都不得宣称完成。
