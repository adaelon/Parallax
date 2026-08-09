# SESSION_CHECKPOINT — 2026-08-09 16:53 +08:00

## 新鲜度自检
- 写入时最新 commit：`188d3c97018d8f0af71f2ef46fdede0258d36f0b docs: freeze scroll-fix system build`。
- Full 构建来源：`74dd8123600f23a03c9e8e8e49507fa20623fa46 fix(desktop): restore initialization scrolling`。
- 读入时请对比 `git log --oneline -3`；若不一致，以 Git 与当前工作区状态为准。

## 当前在做什么
S07C-6 初始化页面滚动缺陷已修复并重新通过 S31 Full 18/18；新冻结构建已替换旧候选，S32 已解锁但尚未开始，不能结转任何旧观察天数。

## 下一步（可直接接手）
1. 本人明确决定开始 S32 后，先运行 `Get-FileHash -Algorithm SHA256 target/release/bundle/nsis/evrything-about-me_0.1.0_x64-setup.exe`，核对结果为 `f2dcd23f601d7eb8b98a10e47d6cdadd3fc9a503e89dd93629799f5383c767d8`。
2. 创建 `/.local/longitudinal-observations/`，把 `docs/longitudinal-observation-template.md` 复制为私有观察记录，并填写同一构建的安装时间、观察起点和最早结束日。
3. 安装该 NSIS 包并从观察第 1 日开始；按模板逐日记录不透明 ID 与聚合计数，不把真实正文写入仓库。
4. 在同一安装包上完成 LONG-01～LONG-07 与不少于十四个自然日的观察；任何产品行为修复都回到所属切片、重跑 Full 并清零观察窗口。
5. 观察期结束后，按 `docs/longitudinal-observation-template.md:发布结论` 形成 `RELEASE / RETURN_TO_OWNER_SLICE / EXTEND_OBSERVATION` 结论。

## 已验证基线
- 根因：`body { overflow: hidden }` 是会话页所需的全局锁，但保险库与创建外壳只有 `min-height`，内容会撑大容器，导致其 `overflow-y: auto` 无法接管滚动。
- 修复：`.vault-setup-shell` 与 `.counterpart-creation-shell` 均改为 `height: 100vh`；CSS 回归契约先红 2/2，再转绿。
- 真实页面：1280×720、500×600 的初始介绍和 500×200 的保险库页均可滚到底；会话页保持文档滚动为零，仅 `.conversation` 滚动。
- Full：18/18；workspace 347/347、Desktop React 26/26、扩展 10/10、fmt、Clippy `-D warnings`、类型检查与生产构建全绿；隐私 280/280、链接 294/294。
- NSIS：`0.1.0`，5,529,536 bytes，SHA-256 `f2dcd23f601d7eb8b98a10e47d6cdadd3fc9a503e89dd93629799f5383c767d8`；隔离安装、启动、关窗驻留、退出和卸载烟测通过。

## 未提交 / 未完成
- `SESSION_CHECKPOINT.md`：在冻结文档 commit `188d3c9` 后按协议整页刷新，待作为 checkpoint-only commit 纳入最终 push。
- `handoff-*.md` 与 `handoff-*.evidence.json`：用户提供/导出的未跟踪材料，未修改且不纳入产品提交。
- S32 真实纵向观察尚未开始；读取时应重新核对当前分支与 upstream 状态。

## 冷启动读序
1. `docs/implementation-slices.md` S07C-6、S32、§12 — 修复归属、纵向边界与完成门禁。
2. `docs/code-trail.md` 最新两条 — 滚动修复和重新冻结构建的精确链路。
3. `docs/system-acceptance-v1.md` §8；`docs/architecture.md` §9.31 — Full 运行记录与唯一冻结构建。
4. `docs/longitudinal-observation-template.md` — 新 SHA-256、十四日记录和发布结论。
5. `docs/product-spec.md` §6.2；`docs/g10-personal-baseline.md` — 纵向判据与真实资料私隐边界。

## 本会话决策摘要
- 保留会话页的 `body` 滚动锁，由初始化外壳各自拥有视口有界滚动；不把滚动责任重新交回文档。
- 同源修复同时覆盖首次保险库初始化与全部第二自我创建状态，不改业务状态、文案或会话滚动。
- 旧 SHA-256 `73f0137e...5659f` 已失效；S32 只能从 `f2dcd23f...c767d8` 的新安装包第 1 日开始。
