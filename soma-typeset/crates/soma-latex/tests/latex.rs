//! LaTeX subset front end: DocIR shapes, recovery, pipeline.

use soma_core::{BreakConfig, SyntheticMetrics};
use soma_doc::inline::Inline;
use soma_doc::Block;
use soma_latex::parse_latex;
use soma_layout::layout;
use soma_render_html::render;
use soma_template::{parse_template, resolve};

fn visible_text(html: &str) -> String {
    let mut out = String::new();
    let mut in_tag = false;
    for character in html.chars() {
        match character {
            '<' => in_tag = true,
            '>' => in_tag = false,
            '\n' | '\r' => {}
            _ if !in_tag => out.push(character),
            _ => {}
        }
    }
    out
}

fn inline_text(inlines: &[Inline]) -> String {
    let mut out = String::new();
    collect(inlines, &mut out);
    out
}

fn collect(inlines: &[Inline], out: &mut String) {
    for inline in inlines {
        match inline {
            Inline::Text(text) => out.push_str(&text.value),
            Inline::Code(code) => out.push_str(&code.value),
            Inline::Strong(children)
            | Inline::Emphasis(children)
            | Inline::Underline(children)
            | Inline::Strikethrough(children)
            | Inline::SmallCaps(children)
            | Inline::Superscript(children)
            | Inline::Subscript(children) => collect(&children.children, out),
            Inline::Link(link) => collect(&link.children, out),
            Inline::Math(math) => out.push_str(&math.source),
            Inline::Ruby(node) => out.push_str(&node.base),
            Inline::LineBreak(_) => out.push(' '),
            _ => {}
        }
    }
}

fn paragraph_text(block: &Block) -> String {
    let Block::Paragraph(paragraph) = block else {
        panic!("not a paragraph: {block:?}");
    };
    inline_text(&paragraph.inlines)
}

#[test]
fn sections_paragraphs_and_inline_commands_map_to_docir() {
    let source = r#"\section{第一章}
正文 \textbf{加粗}，\emph{斜体}，\underline{下划线}，\texttt{code}，\href{https://example.com}{链接}\footnote{脚注}。

第二段 50\% 与 $x^2$。"#;
    let document = parse_latex(source);
    assert!(matches!(document.blocks[0], Block::Heading(_)));
    assert_eq!(
        paragraph_text(&document.blocks[1]),
        "正文 加粗，斜体，下划线，code，链接。"
    );
    assert_eq!(paragraph_text(&document.blocks[2]), "第二段 50% 与 x^2。");

    let Block::Paragraph(paragraph) = &document.blocks[1] else {
        panic!("paragraph");
    };
    assert!(paragraph
        .inlines
        .iter()
        .any(|inline| matches!(inline, Inline::Strong(_))));
    assert!(paragraph.inlines.iter().any(|inline| match inline {
        Inline::Link(link) => link.href == "https://example.com",
        _ => false,
    }));
    assert_eq!(document.footnotes.len(), 1);
    assert_eq!(document.footnotes[0].label, "1");
    document.validate().expect("valid DocIR");
}

#[test]
fn display_math_becomes_a_block() {
    let source = "前文。\n\n\\[ y = x^2 + 1 \\]\n\n后文。";
    let document = parse_latex(source);
    assert!(document
        .blocks
        .iter()
        .any(|block| matches!(block, Block::MathBlock(_))));
    let Block::MathBlock(math) = document
        .blocks
        .iter()
        .find(|block| matches!(block, Block::MathBlock(_)))
        .expect("math block")
    else {
        unreachable!()
    };
    assert_eq!(math.source, "y = x^2 + 1");
    document.validate().expect("valid DocIR");
}

#[test]
fn lists_quotes_and_verbatim_map_to_docir() {
    let source = r#"\begin{itemize}
\item 第一项
\item 第二项
\end{itemize}

\begin{enumerate}
\item 甲
\item 乙
\end{enumerate}

\begin{quote}
引用一段话。
\end{quote}

\begin{verbatim}
let x = 1;
\end{verbatim}"#;
    let document = parse_latex(source);
    assert!(matches!(document.blocks[0], Block::List(_)));
    let Block::List(list) = &document.blocks[0] else {
        panic!("list");
    };
    assert_eq!(list.items.len(), 2);
    assert!(matches!(document.blocks[1], Block::List(_)));
    assert!(matches!(document.blocks[2], Block::BlockQuote(_)));
    let Block::CodeBlock(code) = &document.blocks[3] else {
        panic!("verbatim");
    };
    assert_eq!(code.value, "let x = 1;");
    document.validate().expect("valid DocIR");
}

#[test]
fn tabular_becomes_a_header_table() {
    let source = r#"\begin{tabular}{lll}
\toprule
项目 & 金额 & 备注 \\
\midrule
合计 & 100 & 跨两列 \\
甲 & 20 & 说明 \\
\bottomrule
\end{tabular}"#;
    let document = parse_latex(source);
    let Block::Table(table) = &document.blocks[0] else {
        panic!("table");
    };
    assert_eq!(table.columns.len(), 3);
    assert_eq!(table.rows.len(), 3);
    assert!(table.rows[0].header);
    assert!(!table.rows[1].header);
    // `\midrule` is eight bytes including the backslash: nothing of the
    // command may leak into the first body cell.
    let first_cell = &table.rows[1].cells[0];
    let Block::Paragraph(paragraph) = &first_cell.blocks[0] else {
        panic!("cell paragraph");
    };
    assert_eq!(inline_text(&paragraph.inlines), "合计");
    document.validate().expect("valid DocIR");
}

#[test]
fn a_table_environment_attaches_its_caption() {
    let source = r#"\begin{table}
\caption{参数表}
\begin{tabular}{ll}
甲 & 乙 \\
1 & 2 \\
\end{tabular}
\end{table}"#;
    let document = parse_latex(source);
    let Block::Table(table) = &document.blocks[0] else {
        panic!("table");
    };
    assert_eq!(table.columns.len(), 2);
    assert_eq!(inline_text(&table.caption), "参数表");
    document.validate().expect("valid DocIR");
}

#[test]
fn a_figure_environment_becomes_a_figure_with_a_caption() {
    let source = r#"\begin{figure}
\centering
\includegraphics[width=0.6\linewidth]{fig/arch.pdf}
\caption{架构图}
\end{figure}"#;
    let document = parse_latex(source);
    let Block::Figure(figure) = &document.blocks[0] else {
        panic!("figure");
    };
    assert_eq!(figure.image.source.src, "fig/arch.pdf");
    assert_eq!(inline_text(&figure.caption), "架构图");
    document.validate().expect("valid DocIR");
}

#[test]
fn a_figure_without_a_path_is_dropped() {
    let document = parse_latex("\\begin{figure}\\caption{无图}\\end{figure}");
    assert!(document.blocks.is_empty());
    document.validate().expect("valid DocIR");
}

#[test]
fn title_author_and_date_fill_the_metadata() {
    let source = r#"\documentclass{article}
\title{中文排版研究}
\author{甲 \and 乙}
\date{2026-11-20}
\begin{document}
\begin{document body}
\end{document}"#;
    let document = parse_latex(source);
    let title = document.meta.title.as_ref().expect("title");
    assert_eq!(inline_text(title), "中文排版研究");
    assert_eq!(document.meta.authors.len(), 2);
    assert_eq!(document.meta.authors[0].name, "甲");
    assert_eq!(document.meta.authors[1].name, "乙");
    let date = document.meta.date.expect("date");
    assert_eq!((date.year, date.month, date.day), (2026, 11, 20));
}

#[test]
fn unknown_commands_keep_their_braced_content() {
    let document = parse_latex(r"前 \unknown{保留} 后 \nothing");
    assert_eq!(paragraph_text(&document.blocks[0]), "前 保留 后");
    document.validate().expect("valid DocIR");
}

#[test]
fn an_item_that_opens_with_an_inline_command_stays_one_paragraph() {
    // `\textbf{...}` at the start of a block is an inline run, not an unknown
    // block command: it must not split the item into two paragraphs.
    let source = r"\begin{itemize}
\item \textbf{甲}：正文
\end{itemize}";
    let document = parse_latex(source);
    let Block::List(list) = &document.blocks[0] else {
        panic!("list");
    };
    assert_eq!(list.items.len(), 1);
    assert_eq!(list.items[0].blocks.len(), 1, "one paragraph per item");
    let Block::Paragraph(paragraph) = &list.items[0].blocks[0] else {
        panic!("paragraph");
    };
    assert!(
        paragraph.inlines.len() >= 2,
        "the bold run and the text that follows it"
    );
}

#[test]
fn malformed_latex_is_recovered_without_panicking() {
    let source = r"\section{未闭合
正文 \textbf{粗体
\begin{itemize}
\item 项
\end{itemize";
    let document = parse_latex(source);
    document.validate().expect("valid DocIR");
    assert!(document.canonical_json().contains("正文"));
}

#[test]
fn parsing_is_deterministic() {
    let source = r"\section{标题}
正文 \emph{重点}。";
    let first = parse_latex(source).canonical_json();
    let second = parse_latex(source).canonical_json();
    assert_eq!(first, second);
    assert!(first.starts_with("{\"schema\":\"soma.doc/3\""));
}

#[test]
fn latex_reaches_the_page_renderer() {
    const TEMPLATE: &str = r#"
name: latex
page:
  size: A5
  margin: 12mm
styles:
  - selector:
      element: document
    declarations:
      font-size: 10.5pt
      line-height: 1.6
      align: justify
  - selector:
      element: paragraph
    declarations:
      first-indent: 2em
  - selector:
      element: heading
    declarations:
      font-size: 16pt
      space-before: 1em
      keep-with-next: true
"#;
    let document = parse_latex(
        r"\section{标题}

正文 \textbf{重点}。",
    );
    let template = parse_template(TEMPLATE).expect("template");
    let styled = resolve(&document, &template).expect("styles");
    let pages = layout(
        &document,
        &styled,
        &SyntheticMetrics::default(),
        &BreakConfig::default(),
    )
    .expect("layout");
    let html = render(&pages, styled.table(), &pages.text);
    let text = visible_text(&html);
    assert!(text.contains("标题"), "{text}");
    assert!(text.contains("正文"), "{text}");
    assert!(text.contains("重点"), "{text}");
}

#[test]
fn a_subtitle_reaches_the_cover_data() {
    let source = r#"\title{书名}
\subtitle{副标题}
\author{作者}
\begin{document}
正文。
\end{document}
"#;
    let doc = parse_latex(source);
    let cover = doc.meta.cover.as_ref().expect("cover data");
    assert_eq!(cover.subtitle.as_deref(), Some("副标题"));
    assert!(doc.meta.title.is_some());
}

#[test]
fn index_commands_become_markers() {
    let document = parse_latex("正文\\index{排版}后文");
    let Block::Paragraph(paragraph) = &document.blocks[0] else {
        panic!("paragraph")
    };
    assert!(
        paragraph
            .inlines
            .iter()
            .any(|inline| matches!(inline, Inline::Index(node) if node.term == "排版")),
        "the marker is in the inline sequence"
    );
    assert_eq!(paragraph_text(&document.blocks[0]), "正文后文");
    document.validate().expect("valid DocIR");
}

#[test]
fn an_empty_index_command_is_dropped() {
    let document = parse_latex("正文\\index{}后文");
    assert_eq!(paragraph_text(&document.blocks[0]), "正文后文");
    document.validate().expect("valid DocIR");
}

#[test]
fn a_ruby_command_carries_base_and_reading() {
    let document = parse_latex("正文\\ruby{汉字}{hànzì}后文");
    let Block::Paragraph(paragraph) = &document.blocks[0] else {
        panic!("paragraph")
    };
    assert!(
        paragraph.inlines.iter().any(|inline| matches!(
            inline,
            Inline::Ruby(node) if node.base == "汉字" && node.annotation == "hànzì"
        )),
        "the annotation is in the inline sequence"
    );
    assert_eq!(paragraph_text(&document.blocks[0]), "正文汉字后文");
    document.validate().expect("valid DocIR");
}

#[test]
fn mermaid_environment_becomes_a_diagram() {
    let document = parse_latex("\\begin{mermaid}\nflowchart TD\n  A --> B\n\\end{mermaid}\n");
    assert!(matches!(document.blocks[0], Block::Diagram(_)));
}
