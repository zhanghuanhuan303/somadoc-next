# 内容能力矩阵（Gate C）

状态：**A5/A6 之后冻结的第一版**。代码是权威：`soma_md::CAPABILITIES`。
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
| `inline_code` | supported | `` `x` `` → `Code`，一个不可断 atom |
| `link` | supported | `[text](url)` → `Link` |
| `image` | supported | `![alt](src)` → `Image` |
| `fenced_code` | supported | ``` ``` ``` / `~~~` → `CodeBlock` |
| `list_unordered` | supported | `-`/`*`/`+`，只支持平铺列表 |
| `list_ordered` | supported | `1.` / `1)`，保留起始编号 |
| `blockquote` | supported | `>` 行合并为一个 `BlockQuote` 段落 |
| `thematic_break` | supported | `---`/`***`/`___` → `ThematicBreak` |
| `footnote_def` | supported | `[^label]: text` → `FootnoteDef` |
| `footnote_ref` | supported | `[^label]` → `FootnoteRef` |
| `escape` | supported | 反斜杠转义下一个字符 |
| `nested_list` | degraded | 缩进项并入父项文本 |
| `setext_heading` | degraded | 下划线标题退化为段落 + 分隔线 |
| `table` | degraded | 竖线表格保持文本；要真表格请构造 `Table` |
| `html_block` | degraded | 原样保留为文本，永不执行 |
| `html_inline` | degraded | 原样保留为文本，永不执行 |
| `link_reference` | degraded | `[label]: url` 定义保持文本 |
| `multi_line_code_span` | degraded | 未闭合的行内代码保持文本 |

## 安全口径

- 前端不执行任何内容：HTML 块/行内保留为文本。
- 渲染器只输出绝对定位的 `span`/`div`，不输出 `<script>`、`<a href>`、`<img src>`。
- 图是占位框；链接只画文本。
- 输出里的文本全部转义（`& < > "`）。

## 降级断言

对每个 `degraded` 构造，`crates/soma-md/tests/capabilities.rs` 都有一条断言：
源文本必须出现在 DocIR 的纯文本投影里，证明“没有丢内容”。
