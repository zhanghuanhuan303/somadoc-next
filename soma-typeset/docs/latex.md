# LaTeX 前端（`soma-latex`）

状态：**v1 已落地**（输出 `soma.doc/3`）。代码是权威；本文说明支持的子集与限制。

参考：ADR-0004（语法前端）、ADR-0009（语义/呈现边界）、ADR-0025（本前端）。

---

## 1. 入口

```rust
use soma_latex::parse_latex;

let document = parse_latex(r"\section{标题}
正文 \textbf{重点}\footnote{脚注}。");
```

- 零依赖、`no_std + alloc`、`forbid(unsafe_code)`。
- 只产出 DocIR；`\newcommand` 等宏在解析前展开（ADR-0067），不执行 `\usepackage`，不做真实公式排版。
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
| `\index{...}` | `Inline::Index`（索引词条标记，不占宽度；空词条丢弃） |
| `$...$` / `\(...\)` | `Inline::Math`（源码原样） |
| `\[...\]` / `$$...$$` / `equation` | `MathBlock`（源码原样） |
| `itemize` / `enumerate` | `List` |
| `quote` / `quotation` | `BlockQuote` |
| `center` / `flushleft` / `flushright` | 透明容器 |
| `verbatim` | `CodeBlock` |
| `tabular`（`\\` 分行、`&` 分列、`\hline`/`\toprule`/`\midrule`/`\bottomrule`、`\multicolumn`） | `Table` |
| `table` / `table*`（内部 `\caption{...}`） | `Table.caption` |
| `figure` / `figure*`（`\includegraphics[opts]{path}` + `\caption{...}`） | `Figure`（`ImageRef`，无 intrinsic 时按 3:2 占位）；无路径时丢弃 |
| `tikzpicture` 环境 | `MathBlock` 保留环境源码；布局交给 `soma-tikz` 解析核心 PGF 绘图语言（ADR-0092，子集见 `docs/tikz.md`），失败时回退为源码文本 |
| `\newpage` / `\clearpage` | `PageBreak` |
| `\title` / `\author`（`\and` 分隔）/ `\date`（`YYYY-MM-DD`） | `DocumentMeta` |

## 3. 降级与恢复

- 未知命令：有 `{...}` 参数时保留参数文本；无参数时丢弃。
- 未知环境：保留其中的块。
- 未闭合的组/环境：读到末尾并把剩余内容作为文本保留。
- `\label`/`\cite`/`\ref`/`\pageref`：丢弃（交叉引用尚未建模）；`\index` 生成索引标记。
- 符号转义：`\% \& \_ \# \$ \{ \}`；`\,`/`\;`/`\:`/`\ ` 变成空格；`\!` 丢弃。
- `\ldots`/`\dots` → `…`，`\LaTeX`/`\TeX` 保留名称。

## 4. 表格规范化

- 列数 = 最宽一行（考虑 `\multicolumn` 的 span）。
- 行补齐为空单元格，span 夹到网格内。
- `\midrule` 或“行后的 `\hline`”之前的行标为表头；表头必须是前缀。
- 单元格内容 v1 是纯文本（不解析 `\textbf` 等）；`---`/`--` 转成破折号。
- `table`/`figure` 环境里的 `\caption` 会从正文中移除并挂到第一个 `Table`（或新建的
  `Figure`）上；题注本身按行内语法解析，其中的脚注照常进入文档。

## 5. 限制（见 `docs/debt.md` D70）

- 宏只在本文档内定义：`\newcommand`/`\renewcommand`/`\providecommand`/`\def`（0–9 个参数），计数器 `\newcounter`/`\setcounter`/`\stepcounter` 与 `\arabic`/`\roman`/`\Roman`/`\alph`/`\Alph`/`\value`/`\the<名称>`。不做 `\newcommand*`、可选参数、`\let`、环境定义、交叉引用与参考文献。`\includegraphics` 只得到路径，不读取图片尺寸。
- 行间公式的源码原样进入 `MathBlock.source`，随后由 `soma-math` 排成数学盒（ADR-0026）；
  行内公式同样交给 `soma-math`，作为对象 atom 参与断行（ADR-0027）。
- 无 `\item[label]`、无 `description` 环境、无 `\multicolumn` 以外的列格式。
- 无 `\maketitle` 布局：标题进元数据，占位标题页由模板决定。
- 无源位置（`SourceSpan` 为空）。

## 6. 测试

`crates/soma-latex/tests/latex.rs` 覆盖：

- 章节/段落/强调/链接/脚注；
- 行内与行间公式；
- 列表、引用、verbatim；
- tabular 的表头与网格，并通过 `validate`；
- 元数据（标题/作者/日期）；导言区的 `\subtitle{...}` 进封面，`\language{...}`
  进元数据并写入 PDF `/Lang`，`\colophon{...}` 进封面字段。
- 未知命令保留参数；
- 畸形输入不 panic；
- canonical 确定性；
- LaTeX → 模板 → 分页 → HTML 渲染端到端。
