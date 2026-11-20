# DocIR 规范（`soma-doc`）

状态：**A1 已落地，索引标记已加**（crate `soma-doc`，schema `soma.doc/2`，`docIrVersion = 2`）。
本文件描述冻结后的语义 IR 契约；代码是权威，本文解释意图与不变量。

参考：`docs/architecture-v2.md` §5、ADR-0001（两层 IR）、ADR-0008（DocIR 迁移）、
ADR-0009（语义/呈现边界）。

---

## 1. 定位

| 问题 | 答案 |
|---|---|
| 它是什么 | 「文档是什么」：结构、语义、元数据、脚注、图表、公式源码 |
| 它不是什么 | 不出现宽度、字号、边距、颜色、页高；这些属于模板与盒子树 |
| 谁生产它 | 语法前端（Markdown/HTML/LaTeX/Word）、模板生成的封面/目录 |
| 谁消费它 | `soma-template`（计算样式）、`soma-layout`（盒子树）、诊断与增量重排 |
| 依赖 | **零依赖**：`no_std + alloc`，无 `serde`、无字体、无浮点 |

三条硬不变量：

1. **每个节点** 都有 `NodeId`、`SourceSpan`、`Attrs`（统一放在 `NodeMeta`）。
2. **未知构造不丢**：`RawBlock` / `RawInline` 保留 `format + value + fallback + capability`。
3. **canonical JSON 是唯一序列化**：手写 writer，字段顺序固定，整数单位，native 与
   wasm 字节一致。

## 2. 节点清单

### 文档级

| 类型 | 说明 |
|---|---|
| `Document` | `meta` + `blocks` + `footnotes` + `source_map` |
| `DocumentMeta` | 富文本 `title`、`authors`、`language`、`date`、`keywords`、`template` 引用、`cover` 数据、自由 `fields` |
| `FootnoteDef` | `label` + `blocks`；由 `FootnoteRef.target` 引用 |
| `Section` | **块级标记**（`Block::Section`）：`level`、`numbered`、`page_template`、可选 `title` |

`Section` 用标记而不是嵌套树：分页、running header、增量重排都按文档顺序扫描，
标记让「当前章节」是一次线性遍历即可确定的状态，不存在两套真相。

章节编号、页码、目录条目由模板/排版阶段生成，IR 只记录 `numbered`。

### 块级

| 变体 | 关键字段 |
|---|---|
| `Paragraph` | `inlines` |
| `Heading` | `level`（1..=6）、`inlines` |
| `List` | `ordered`、`start`、`tight`、`items` |
| `ListItem` | `blocks`（可嵌套列表/段落） |
| `BlockQuote` | `blocks` |
| `CodeBlock` | `lang`、`value` |
| `MathBlock` | `source`（交给 `soma-math` 解析成数学盒，内核不解析） |
| `Figure` | `image`、`caption`、`label` |
| `Table` | `columns`、`rows`、`caption`、`label` |
| `Column` | `align`；`share` 是可选的相对宽度千分比（0 = 按内容），不是物理长度 |
| `Row` | `header`、`cells` |
| `Cell` | `colspan`、`rowspan`、`blocks` |
| `ThematicBreak` | — |
| `PageBreak` | 作者要求的强制分页 |
| `Section` | 见上 |
| `GeneratedBlock` | `kind`（toc / lof / lot / index / bibliography / cover / custom）、`slot`、`blocks` |
| `RawBlock` | `format`、`value`、`fallback`、`capability` |

### 行内

| 变体 | 关键字段 |
|---|---|
| `Text` | `value`（非空） |
| `Code` | `value`（非空） |
| `Strong` / `Emphasis` / `Underline` / `Strikethrough` / `SmallCaps` | `children` |
| `Superscript` / `Subscript` | `children` |
| `Link` | `href`、`title`、`children` |
| `Image` | `src`、`alt`、`title`、`intrinsic`（像素，仅用于宽高比） |
| `MathInline` | `source` |
| `FootnoteRef` | `target`、`label`（作者写的标记，不是显示编号） |
| `Index` | `term`（非空；索引词条标记，不占宽度、不产生文本） |
| `Ruby` | `base`、`annotation`（纯字符串：注音不是富文本） |
| `LineBreak` | `kind`（soft / hard；hard 在段落 IR 里变成 `Forced`） |
| `RawInline` | 同 `RawBlock` |

## 3. 身份、来源与属性

```rust
NodeMeta { id: NodeId, span: SourceSpan, attrs: Attrs }
Attrs    { role, classes, anchor, lang }
```

- `NodeId(0)` = 未分配。`Document::renumber()` 按固定前序赋 1..=n。
- `SourceSpan { source, start, end }`，字节偏移。`source == 0` 表示合成内容
  （生成块、测试、模板封面）。`start > end` 非法；已知 `source` 必须在
  `source_map` 里。
- `Attrs.anchor` 是锚点/交叉引用目标，命名为 `anchor` 以免与 `NodeId` 混淆。
- `Attrs` 是内容影响呈现的**唯一**通道：模板按 `role`/`classes`/`anchor` 匹配。

### 遍历顺序契约

`renumber()`、`node_count()`、`node_ids()` 使用同一前序：

1. `meta.title` 的行内；
2. `blocks` 按文档顺序，父节点先于子节点；
3. 表格内部：`columns` → `caption` → `rows`（row → cells → cell blocks）；
4. 图内部：`image` → `caption`；
5. `footnotes` 按文档顺序，定义先于其 blocks。

顺序是契约的一部分，因为 id 出现在 canonical 输出里。

## 4. Canonical JSON

一个文档：

```json
{"schema":"soma.doc/2","docIrVersion":2,"nodeCount":4,
 "meta":{"title":null,"authors":[],"language":"zh-Hans","date":null,
         "keywords":[],"template":null,"cover":null,"fields":[]},
 "blocks":[{"kind":"paragraph","id":1,
            "span":{"source":1,"start":0,"end":9},
            "attrs":{"role":null,"classes":[],"anchor":null,"lang":null},
            "inlines":[
              {"kind":"text","id":2,"span":{"source":1,"start":0,"end":6},
               "attrs":{"role":null,"classes":[],"anchor":null,"lang":null},
               "text":"你好"},
              {"kind":"strong","id":3,"span":{"source":1,"start":6,"end":9},
               "attrs":{"role":null,"classes":[],"anchor":null,"lang":null},
               "inlines":[{"kind":"text","id":4,"span":{"source":1,"start":6,"end":9},
                           "attrs":{"role":null,"classes":[],"anchor":null,"lang":null},
                           "text":"排版"}]}]}],
 "footnotes":[],
 "sourceMap":[{"id":1,"name":"chapter.md","kind":"markdown"}]}
```

规则：

- 字段顺序固定；节点形状统一为 `kind` → `id` → `span` → `attrs` → 载荷。
- 所有数字是整数；没有浮点。
- 字符串只转义 JSON 必需的字符（`"`、`\`、控制字符 `< 0x20`）；CJK 原样输出。
- 布尔写 `true`/`false`；可选值写 `null`。

### 两种模式与两个 hash

| 模式 | 差异 | 用途 |
|---|---|---|
| `Full` | 含 `id`、真实 `span`、`sourceMap`、`meta.template` | 精确复现；`doc_hash()` |
| `Content` | `id` 归零、`span` 归零、`sourceMap` 为空、`template` 为 `null` | 内容身份；`content_hash()` |

- `doc_hash()` 变化 ⇒ 输入或来源变了。
- `content_hash()` 不变 ⇒ 语义内容没变，可以复用解析/布局缓存。
- 两者都是 FNV-1a 64（与 `paramHash` 同一原语）。

## 5. 校验

`Document::validate()` 返回 `DocError { path, reason }`，`path` 形如
`blocks[2].inlines[1].attrs.classes[0]`。覆盖：

| 类别 | 规则 |
|---|---|
| 身份 | id 必须已分配且全文档唯一 |
| 来源 | `start <= end`；已知 `source` 必须存在于 `source_map`；source id 0 保留 |
| 深度 | 块/行内嵌套 ≤ `MAX_DEPTH`（32）；先于任何递归消费者拒绝 |
| 文本 | `Text` / `Code` 非空 |
| 标题/章节 | `level` ∈ 1..=6 |
| 列表 | 无序 `start == 1`；有序 `start >= 1` |
| 表格 | 有行必须有列；行的 `colspan` 之和 == 列数；`colspan/rowspan >= 1` |
| 图表 | `src` 非空；`intrinsic` 两维都 > 0；`label` 是 token |
| 渲染锚点 | `anchor` 全文档唯一 |
| 属性 | `role/class/anchor/lang` 非空 token、无空白/控制字符、长度 ≤ 128；class ≤ 16 且不重复 |
| 公式/原始 | `source`/`format` 非空 token |
| 生成块 | `Custom` 必须有 `slot` |
| 脚注 | `label` 唯一；每个 `FootnoteRef.target` 必须解析到定义 |
| 元数据 | 日期范围；keyword 非空且唯一；field key 非空且唯一；作者名非空；模板名/版本是 token |

## 6. 明确不做

- 不解析 Markdown/HTML/LaTeX（A5 起由前端做）。
- 不计算样式、不选字体、不做断行、不分页。
- 不生成目录/编号/页码（`GeneratedBlock` 只是占位）。
- 不渲染 ruby、不做拼音对齐（数据在 IR，行为在模板/布局）。
- 不做表格 `rowspan` 的网格占位校验（当前只校验每行 `colspan` 之和）。

## 7. 扩展流程

- **加节点/加字段**：additive，`docIrVersion` 不变；canonical 输出必须同时更新
  golden 与 `tests/a1_model.rs` 的 kind 清单。
- **改已有字段语义、改字段顺序、改 hash 输入**：必须升 `docIrVersion` 与
  `SCHEMA`，并新增 ADR。
- 新增物理属性（宽度/字号/颜色）**永不接受**；需要时走 Gate A 报错。
