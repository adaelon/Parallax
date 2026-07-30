# G05 `eam-markdown-v1` 契约

## 1. 结论

S09 使用 `pulldown-cmark 0.13.4`（`default-features = false`）消费 CommonMark/GFM 事件与 UTF-8 字节范围，使用 `gfm-autolinks 0.2.0` 补齐 GFM literal autolink，使用 `saphyr 0.0.11` 保守读取文件开头的 YAML Properties。三者都是纯 Rust 库；解析入口不接收路径、文件、数据库、网络、模型、WebView 或工具句柄。

候选比较：

| 候选 | 源定位 | 资源拒绝时机 | GFM / Obsidian | 结论 |
| --- | --- | --- | --- | --- |
| `pulldown-cmark 0.13.4` | 事件自带半开 UTF-8 字节范围 | 消费事件时可立即停止 | CommonMark、表格、删除线、任务列表、Wikilink；literal autolink 外补 | 采用 |
| `comrak 0.54.0` | AST 为行列坐标，需重算绝对范围 | 完整 AST 已建立后 | GFM 完整，Wikilink 仍需扩展 | 不采用 |
| `markdown 1.0.0` | AST 带 offset | 完整 AST 已建立后 | GFM 完整，Wikilink 仍需扩展 | 不采用 |

依赖选择不新增 ADR：解析库被本文件的版本化输入/输出契约包裹，可在固定语料保持等价时替换；行为变化必须提升方言版本。

## 2. 唯一入口与硬上限

```rust
pub const CONTRACT_VERSION: &str = "eam-markdown-v1";

pub struct ParseLimits { /* 字段私有，只能收紧 */ }

pub fn parse_markdown(
    source_utf8: &str,
    limits: ParseLimits,
) -> Result<ParsedMarkdownV1, MarkdownParseError>;
```

`ParseLimits::new` 只能创建不超过下列硬上限的值；调用者可以收紧，不能放宽：

| 资源 | 默认值 / 硬上限 | 计数规则 |
| --- | --- | --- |
| 原文 | 16 MiB | `source_utf8.len()`，创建底层解析器前检查 |
| 块 | 50,000 | 每个输出块一次；超出后丢弃全部结果 |
| 嵌套 | 64 | 事件流中所有可包含子事件的 `Start` 层数 |
| 元数据 | 256 KiB | Properties 原始主体字节数；解析 YAML 前检查 |
| 链接 | 10,000 | Markdown link、image、autolink、Wikilink 和 embed 合计 |

任一上限为零仍是有效的收紧值。超出上限返回稳定的 `RESOURCE_LIMIT(<resource>)`；不返回半成品。`&str` 保证独立解析入口只接收 UTF-8；归档字节转为 `&str` 失败由可信摄取协调器记录为 `INVALID_ENCODING`。

## 3. 输出结构

```text
ParsedMarkdownV1 {
  contract_version,
  properties: [{ name, values: [scalar], source_span }],
  blocks: [{
    local_id, parent_local_id?, ordinal, kind, source_span,
    heading_level?, list_start?, task_checked?, info_string?,
    native_locator?: Heading(text) | BlockId(id)
  }],
  relations: [{
    kind: LINK | IMAGE | AUTOLINK | WIKILINK | EMBED,
    target, alias?, heading?, block_id?, source_span
  }],
  tags: [{ value, source_span }]
}
```

- `source_span` 是 `[start_byte, end_byte)`；两端必须是 UTF-8 字符边界且原文切片成功。
- `local_id` 只是本次解析内从 1 开始的先序编号，不是 S10 的权威证据块 ID。
- `ordinal` 是全局输出顺序；`parent_local_id` 只指向同一结果中的容器块。
- 块类型固定为 `PARAGRAPH`、`HEADING`、`BLOCK_QUOTE`、`LIST`、`LIST_ITEM`、`CODE_BLOCK`、`TABLE`、`TABLE_HEAD`、`TABLE_ROW`、`TABLE_CELL`、`HTML_BLOCK`、`THEMATIC_BREAK` 和 `METADATA_BLOCK`。
- 强调、粗体、删除线、任务标记、行内代码、软/硬换行保留在父块原文范围中；任务状态附在最近的 `LIST_ITEM`，不单独成为块。
- HTML、脚本、数学、Mermaid 和插件源码只可能成为 `HTML_BLOCK`、`CODE_BLOCK` 或父块原文，不渲染、不执行、不获取。

## 4. 方言与消歧

底层只启用表格、删除线、任务列表、GFM blockquote kind、YAML metadata block 和 Wikilink；不启用智能标点、标题属性、数学、定义列表、上下标或其他未列扩展。

Wikilink/嵌入按下列顺序解析：

```text
![[target|alias]]       -> EMBED
[[target|alias]]        -> WIKILINK
target#^block           -> target + block_id
target#heading          -> target + heading（保留后续 #，支持多级标题文本）
#heading                -> 当前文档标题目标
#^block                 -> 当前文档块目标
```

- 第一个 `|` 分隔别名；第一个 `#` 分隔文件目标与局部目标；紧随 `#` 的 `^` 表示块，否则表示标题。
- 空目标只有在存在标题或块目标时有效；空别名、空标题、空块 ID或含 `# | ^ : %% [[ ]]` 的文件目标不产生专用关系，原文仍保留。
- Markdown link/image 不转换为 Wikilink；外部目标只保存字符串，不打开或下载。
- GFM literal autolink 只在普通文本事件中识别，代码、HTML、显式 link/image 与 Wikilink/embed 内不二次扫描。

行内标签从普通文本事件识别：`#` 必须位于行首或空白/`*_~(` 之后；标签只含字母、数字、`_`、`-`、`/` 与常用 Unicode 非空白字符，且至少含一个非数字字符。代码、HTML、链接目标与 Properties 原始文本不重复扫描；Properties 的 `tags` 标量或标量列表另行规范化为标签。

块 ID 只接受 ASCII 字母、数字和 `-`。简单段落或列表项末尾的 ` ^id` 以及独占一行的 `^id` 可产生 `BlockId(id)` 定位器；该定位器失效不改变原文范围。

## 5. Properties

- 只识别文件第一个字节开始的 `---` 块，结束符为独占一行的 `---` 或 `...`。
- YAML 顶层必须是 mapping；只有字符串键对应的 scalar 或 `list<scalar>` 会进入 `properties`。
- `tags`、`aliases` 遵守同一标量规则；单个 scalar 规范化为单元素列表。
- nested mapping/sequence、非字符串键、自定义 tag、alias、anchor、`<<` merge key 和无法解析的 YAML 都保留在 `METADATA_BLOCK` 原文范围，但不产生对应结构化 property。
- 某个不支持项不使整篇文档失败；元数据原始主体超限才原子拒绝。

## 6. 固定语料与验收

固定语料位于 `crates/markdown/tests/fixtures/`：

| 输入 | 唯一期望 |
| --- | --- |
| `full-dialect.md` | 块顺序覆盖标题/段落/列表/任务/引用/代码/表格/HTML；只规范化受支持 Properties；识别 Markdown/GFM/Wikilink/embed/tag/block ID；所有范围逐字有效 |
| `unknown-syntax.md` | callout、数学、Dataview 与自定义 YAML 原样留在所属范围，不产生未列专用结构 |
| `limits.md` | 同一短语料分别以 source/block/depth/metadata/link 的收紧上限触发稳定原子拒绝 |

升级 `pulldown-cmark`、`gfm-autolinks` 或 `saphyr` 前必须运行固定语料；块、属性、关系、标签或范围出现非等价变化时，不得继续声称 `eam-markdown-v1`。
