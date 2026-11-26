---
title: 混合排版样张
subtitle: Markdown 为主，LaTeX 与 HTML 直接进入同一棵 DocIR
author: Soma Typeset
date: 2026-11-26
---

# 混合排版：一份源文，三种语法

Markdown 仍是主语法。行内数学用 `$E=mc^2$`，行间数学用 `$$...$$`；LaTeX 的
`\textbf{粗体}`、`\emph{强调}`、`\underline{下划线}` 与
`\href{https://example.com}{链接}` 会直接变成 DocIR；HTML 的
<u>下划线</u>、<b>粗体</b>、H<sub>2</sub>O、x<sup>2</sup> 与
<a href="https://example.com" title="示例链接">超链接</a> 也一样。

## 行内混排

同一段文字里可以同时出现 **Markdown 粗体**、\textbf{LaTeX 粗体}、
*Markdown 斜体*、\emph{LaTeX 斜体}、<u>HTML 下划线</u> 和
<ruby>汉<rt>hàn</rt></ruby>。行内公式例如 $a^2+b^2=c^2$ 与
\(\int_0^1 x^2\,dx\)，它们都走同一条数学排版路径。

## 行间公式

Markdown 的 `$$` 块：

$$
\sum_{i=1}^{n} i = \frac{n(n+1)}{2}
$$

LaTeX 的 equation 环境：

\begin{equation}
E = mc^2
\end{equation}

## LaTeX 列表

\begin{itemize}
\item 第一项来自 LaTeX 的 itemize
\item 第二项可以包含行内公式 $x \to 0$
\end{itemize}

## HTML 块

<div class="mixed-box">
这是一个 HTML 段落，里面有 <b>粗体</b>、<i>斜体</i> 和
<code>inline code</code>，还有行内公式 $e^{i\pi}+1=0$。

第二段跨过了空行，仍然属于同一个 HTML 块。

<ul>
<li>HTML 列表第一项</li>
<li>HTML 列表第二项</li>
</ul>
</div>

## LaTeX 表格

\begin{tabular}{ll}
\hline
语法 & 结果 \\
\hline
Markdown & 段落与强调 \\
LaTeX & 数学与列表 \\
HTML & 语义标签 \\
\hline
\end{tabular}

## 图

\begin{tikzpicture}
\draw (0,0) -- (2,0) -- (2,1) -- cycle;
\end{tikzpicture}

\begin{mermaid}
flowchart LR
  A[源文] --> B[DocIR]
  B --> C[页面]
\end{mermaid}

## 结尾

未知的 HTML 标签会保持为可见源码，例如 <custom-tag>这里</custom-tag>；
未知的 LaTeX 命令也不会被悄悄丢掉，源文仍然可以在 DocIR 的纯文本投影里找到。
