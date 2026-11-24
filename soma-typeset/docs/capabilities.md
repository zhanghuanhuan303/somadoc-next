# 内容能力矩阵（Gate C）

状态：**Gate C 维护中**。代码是权威：`soma_md::CAPABILITIES`（当前 29 项）。
本文件必须与代码逐条对应；`scripts/gate-c.sh` 会检查这一点。

图例：
- **supported** —— 建模成 DocIR 结构；
- **degraded** —— 内容以文本保留，结构不建模（**绝不丢内容**）；
- **rejected** —— 不接受（当前没有）。

| syntax | status | 说明 |
|---|---|---|
| `atx_heading` | supported | `#`..`######` → `Heading` |
| `paragraph` | supported | 空行分隔的块 |
| `soft_line_join` | supported | 连续行用空格合并 |
| `hard_break_spaces` | supported | 行尾两个空格 → `LineBreak::Hard` |
| `hard_break_backslash` | supported | 行尾反斜杠 → `LineBreak::Hard` |
| `emphasis` | supported | `*x*` / `_x_` → `Emphasis` |
| `strong` | supported | `**x**` / `__x__` → `Strong` |
| `strikethrough` | supported | `~~x~~` → `Strikethrough`，PDF 里画成横线（ADR-0044） |
| `inline_code` | supported | `` `x` `` → `Code`，一个不可断 atom |
| `link` | supported | `[text](url)` → `Link` |
| `image` | supported | `![alt](src)` → `Image` |
| `fenced_code` | supported | ``` ``` ``` / `~~~` → `CodeBlock`；info string 的第一个 token 选择语法着色（rust/c/cpp/shell/markdown/latex 等，ADR-0091） |
| `list_unordered` | supported | `-`/`*`/`+`，只支持平铺列表 |
| `list_ordered` | supported | `1.` / `1)`，保留起始编号 |
| `blockquote` | supported | `>` 行合并为一个 `BlockQuote` 段落 |
| `thematic_break` | supported | `---`/`***`/`___` → `ThematicBreak` |
| `footnote_def` | supported | `[^label]: text` → `FootnoteDef` |
| `footnote_ref` | supported | `[^label]` → `FootnoteRef` |
| `ruby_marker` | supported | `[[ruby:base\|reading]]` → `Inline::Ruby` |
| `index_marker` | supported | `[[index:term]]` → `Inline::Index` |
| `escape` | supported | 反斜杠转义下一个字符 |
| `nested_list` | supported | 缩进记号成为父项内的 `List` |
| `setext_heading` | supported | 段落下方的 `===` / `---` 成为一/二级 `Heading` |
| `table` | supported | 带分隔行的 GFM 竖线表格成为 `Table`；`\|` 是字面竖线；分隔行的破折号数量成为相对列宽 |
| `html_block` | supported | 块成为 `RawBlock`，fallback 为文本；永不执行 |
| `html_inline` | supported | 标签成为 `RawInline`，fallback 为文本；永不执行 |
| `link_reference` | supported | `[label]: url` 定义解析 `[text][label]` / `[text][]` 链接与图片；定义行不进入正文 |
| `multi_line_code_span` | supported | 代码跨度可以跨软换行，换行规范化为空格 |
| `unterminated_code_span` | degraded | 未闭合的反引号保持为文本 |

TikZ 不是 Markdown 构造：`tikzpicture` 由 LaTeX 前端作为 `MathBlock` 显示对象进入，
再由 `soma-tikz` 解释核心 PGF 绘图语言（ADR-0092，子集见 `docs/tikz.md`），
因此不在本表与 `soma_md::CAPABILITIES` 的对账范围内。

## 安全口径

- 前端不执行任何内容：HTML 块/行内成为 `RawBlock`/`RawInline`，布局用 fallback 文本绘制。
- 渲染器只输出绝对定位的 `span`/`div`；不输出 `<script>`、`<a href>`。图片可以以 `data:` URI 内联（`<img src="data:…">` / `<image href="data:…">`），因为那仍然是一个自包含文档；除此之外任何 `src`/`href` 都不会出现（ADR-0048）。
- 图是占位框；链接只画文本。
- 输出里的文本全部转义（`& < > "`）。

## 降级断言

对每个 `degraded` 构造，`crates/soma-md/tests/capabilities.rs` 都有一条断言：
源文本必须出现在 DocIR 的纯文本投影里，证明“没有丢内容”。
