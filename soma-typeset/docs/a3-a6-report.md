# A3–A6 report（layout / pagination / Markdown / HTML / footnotes）

状态：**A3–A6 一次交付。** 工作区 **237 tests**，`scripts/gates.sh`、
`scripts/gate-a.sh`、`scripts/wasm-hash.sh` 全绿；native 与 `wasm32-wasip1`
的全管线 canonical 字节一致（**8,979 bytes**）。

新增 crate：

| crate | 作用 |
|---|---|
| `soma-layout` | DocIR + 样式 → 盒子 → 页面；脚注；`soma.pages/1` |
| `soma-md` | Markdown 子集 → DocIR |
| `soma-render-html` | `PagedLayout` → 绝对定位 HTML |

同时按 ADR-0011 给 `soma-rules` 增加了一个入口 `paragraph_from_spans`（带样式的行内
序列），并把 `Line` 的 canonical 序列化抽成 `Line::write_canonical`，供页面结果复用。

---

## 1. A3：布局与盒子

- `layout(doc, styled, metrics, engine) -> PagedLayout`。
- 每个块编译成 `PlacedBlock { node, kind, style, x, y, width, height, lines, marks, stage }`；
  行是冻结内核的 `Line`，`y` 相对块顶。
- **段落盒与直接调 K-P 逐行一致**：`paragraph_box_matches_the_inline_kernel` 用公开
  seam 重建同一个 `Paragraph`，逐行比较 `Line`。
- `TextIndex`：按 `NodeId` 保存每个 atom 的文本与 `StyleId`，实现 `TextSource`；
  渲染器不再需要 DocIR（ADR-0010 §2）。
- `PagedLayout` 携带 `page`、`engine_hash`（`paramHash`）、`rules_version`、
  `metrics_version`、`footnote_passes`。
- canonical：`soma.pages/1`，整数 + 固定字段序；`TextIndex` 不进 canonical（它是
  从 DocIR 派生的）。

## 2. A4：分页

- 确定性贪心：按块高填充页面，`PageBreak` 与 `page_break_before` 强制换页，
  页顶丢弃 `space_before`。
- `keep-with-next`：页末块若要求与下一块同页，则把上一页的它移到下一页（不掏空页面）。
- 单块高于内容区 → 独占一页并标记 `overflow`。
- 脚注区高度参与排版：先无脚注分页，再按每页脚注高度重排，收敛上限 8 次，
  `footnote_passes` 记录实际次数。

**未做**：垂直 K-P（用确定性贪心，ADR-0005 允许的回退路径）、段落跨页拆分
（因此没有 widow/orphan 行级处理）、页面母版 / 页眉页脚 / 页码绘制。

## 3. A5：Markdown 与 HTML

- `soma-md`：ATX 标题、段落（软换行合并）、硬换行（两空格 / 反斜杠）、
  `**强**`、`*强调*`、行内代码、链接、图片、围栏代码、平铺列表（有序/无序）、
  引用、分隔线、脚注定义与引用、反斜杠转义。
- 所有节点带 `SourceSpan`，输出经 `Document::validate()` 校验（测试里就是这么做的）。
- `soma-render-html`：`render(&PagedLayout, &StyleTable, &dyn TextSource)`；
  每个 atom 绝对定位，`em` 长度用整数换算（3 位小数），文本 HTML 转义，
  脚注标记画成编号，`<br>` 永不出现。

## 4. A6：脚注、图、表、公式、全链路哈希

- 脚注：整块移到当前页脚注区，按模板策略编号（`document` / `page`）；标记 atom 是
  `FOOTNOTE_MARKER`，渲染器按 `Mark.number` 画数字。
- 图：按 `intrinsic` 宽高比定高，缺省 3:2；只画占位框（`BoxKind::Figure`）。
- 表：按列数均分内容宽，每个单元格是一个 `TableCell` 盒，定位在列偏移上；
  没有行线、没有 colspan 网格求解、没有行高对齐。
- 公式：作为单个不可断对象，文本是 `source`（无 Math 适配器）。
- 全链路哈希：`soma-wasm` fixture 现在输出 `docIr` + `layout` + `pages`，
  native 与 wasm 完全一致。

## 5. Gate 覆盖

| Gate | 内容 | 状态 |
|---|---|---|
| Gate A | `soma-doc` 零依赖、`soma-template` 白名单、layout/md/render 直接依赖白名单、四个 crate `no_std`、无浮点、4 个 schema 常量 | ✅ |
| Gate T | 行内差分、canonical 字节、native/wasm（现在含页面结果） | ✅ |
| Gate A3 | 段落盒与直接 K-P 逐行一致 | ✅ |
| Gate C | 未知语法降级为文本（Markdown 前端不丢内容） | 部分（无能力矩阵） |
| Gate R | 渲染器不改坐标、不重排 | 部分（无跨渲染器比对） |

## 6. 测试分布（新增）

| suite | 数量 |
|---|---:|
| `a3_layout.rs` | 7 |
| `a4_pagination.rs` | 7 |
| `a6_blocks.rs` | 10 |
| `a5_md.rs` | 11 |
| `pipeline.rs`（MD → HTML） | 3 |
| `a5_render.rs` | 6 |
| `soma-rules/tests/spans.rs` | 6 |

## 7. 已知限制（已记账）

- 无垂直 K-P、无段落跨页、无页眉页脚与页码（D48、D49、D50）。
- 盒子树是扁平的：没有嵌套 VBox/HBox；表格没有行线、colspan 网格、表头重复（D51）。
- 公式是文本占位，没有 Math 适配器（D52）。
- Markdown 子集不含嵌套列表、setext 标题、表格、HTML 块、链接引用定义（D53）。
- 行内代码/对象使用 `AtomKind::Other`，没有独立的等宽度量（D54）。
- 图只画占位框，不加载图片；ruby 未渲染（D55）。
- `Section` 级脚注编号回退为全文连续（D56）。
- `EngineConfig` 的 YAML 形式仍未实现（D29 保留）。
