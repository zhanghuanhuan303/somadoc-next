# LaTeX 前端（`soma-latex`）

状态：**v1 已落地**（输出 `soma.doc/1`）。代码是权威；本文说明支持的子集与限制。

参考：ADR-0004（语法前端）、ADR-0009（语义/呈现边界）、ADR-0025（本前端）。

---

## 1. 入口

```rust
use soma_latex::parse_latex;

let document = parse_latex(r"\section{标题}
正文 \textbf{重点}\footnote{脚注}。");
```

- 零依赖、`no_std + alloc`、`forbid(unsafe_code)`。
- 只产出 DocIR；不解析宏、不执行 `\usepackage`、不做真实公式排版。
- `%` 注释会被去掉；`\%` 保留。
- 正文从 `\begin{document}` 之后开始；没有该标记时解析全文。
- 未闭合的 `{`/环境保留剩余文本，不 panic。

## 2. 支持的语义

| LaTeX | DocIR |
|---|---|
| `\section` / `\subsection` / `\subsubsection` / `\paragraph` / `\subparagraph` | `Heading`（1–5 级，接受 `*`） |
| 空行、`\par` | 段落 |
| `\\` / `\newline` | 硬换行 |
| `\textbf` | `Strong` |
| `\emph` / `\textit` / `\textsl` | `Emphasis` |
| `\underline` / `\textsc` | `Underline` / `SmallCaps` |
| `\textsuperscript` / `\textsubscript` | `Superscript` / `Subscript` |
| `\texttt` / `\verb|...|` | `Inline::Code` |
| `\href{url}{text}` / `\url{url}` | `Link` |
| `\footnote{...}` | `FootnoteRef` + 文档末尾的 `FootnoteDef`（编号从 1） |
| `$...$` / `\(...\)` | `Inline::Math`（源码原样） |
| `\[...\]` / `$$...$$` / `equation` | `MathBlock`（源码原样） |
| `itemize` / `enumerate` | `List` |
| `quote` / `quotation` | `BlockQuote` |
| `center` / `flushleft` / `flushright` | 透明容器 |
| `verbatim` | `CodeBlock` |
| `tabular`（`\\` 分行、`&` 分列、`\hline`/`\toprule`/`\midrule`/`\bottomrule`、`\multicolumn`） | `Table` |
| `\newpage` / `\clearpage` | `PageBreak` |
| `\title` / `\author`（`\and` 分隔）/ `\date`（`YYYY-MM-DD`） | `DocumentMeta` |

## 3. 降级与恢复

- 未知命令：有 `{...}` 参数时保留参数文本；无参数时丢弃。
- 未知环境：保留其中的块。
- 未闭合的组/环境：读到末尾并把剩余内容作为文本保留。
- `\label`/`\index`/`\cite`/`\ref`/`\pageref`：丢弃（交叉引用尚未建模）。
- 符号转义：`\% \& \_ \# \$ \{ \}`；`\,`/`\;`/`\:`/`\ ` 变成空格；`\!` 丢弃。
- `\ldots`/`\dots` → `…`，`\LaTeX`/`\TeX` 保留名称。

## 4. 表格规范化

- 列数 = 最宽一行（考虑 `\multicolumn` 的 span）。
- 行补齐为空单元格，span 夹到网格内。
- `\midrule` 或“行后的 `\hline`”之前的行标为表头；表头必须是前缀。
- 单元格内容 v1 是纯文本（不解析 `\textbf` 等）；`---`/`--` 转成破折号。

## 5. 限制（见 `docs/debt.md` D70）

- 无宏、计数器、交叉引用、参考文献、`\includegraphics`。
- 行间公式的源码原样进入 `MathBlock.source`，随后由 `soma-math` 排成数学盒（ADR-0026）；
  行内公式仍是占位文本（D54）。
- 无 `\item[label]`、无 `description` 环境、无 `\multicolumn` 以外的列格式。
- 无 `\maketitle` 布局：标题进元数据，占位标题页由模板决定。
- 无源位置（`SourceSpan` 为空）。

## 6. 测试

`crates/soma-latex/tests/latex.rs` 覆盖：

- 章节/段落/强调/链接/脚注；
- 行内与行间公式；
- 列表、引用、verbatim；
- tabular 的表头与网格，并通过 `validate`；
- 元数据（标题/作者/日期）；
- 未知命令保留参数；
- 畸形输入不 panic；
- canonical 确定性；
- LaTeX → 模板 → 分页 → HTML 渲染端到端。
