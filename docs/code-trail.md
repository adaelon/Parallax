# 代码链路

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
