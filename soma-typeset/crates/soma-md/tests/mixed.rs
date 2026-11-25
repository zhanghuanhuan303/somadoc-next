//! Mixed-syntax Markdown: Markdown + embedded LaTeX + embedded HTML.
//!
//! These tests assert the semantic seam, not rendering: every construct must
//! reach DocIR, and unknown source must remain visible instead of vanishing.

use soma_doc::{Block, Inline};
use soma_md::parse_markdown;

fn paragraph_inlines(source: &str) -> Vec<Inline> {
    let document = parse_markdown(source);
    document.validate().expect("valid DocIR");
    let block = document
        .blocks
        .iter()
        .find(|block| matches!(block, Block::Paragraph(_)))
        .expect("paragraph");
    let Block::Paragraph(paragraph) = block else {
        unreachable!()
    };
    paragraph.inlines.clone()
}

fn has_container(inlines: &[Inline], pick: fn(&Inline) -> bool) -> bool {
    inlines.iter().any(|inline| {
        pick(inline)
            || inline
                .children()
                .iter()
                .any(|child| has_container(core::slice::from_ref(child), pick))
    })
}

#[test]
fn markdown_latex_and_html_live_in_one_paragraph() {
    let inlines =
        paragraph_inlines("Markdown **粗**、\\textbf{LaTeX 粗}、<u>HTML 下划线</u>、$E=mc^2$。\n");
    assert!(has_container(&inlines, |inline| matches!(
        inline,
        Inline::Strong(_)
    )));
    assert!(
        has_container(&inlines, |inline| matches!(inline, Inline::Underline(_))),
        "inline HTML must become a semantic underline"
    );
    assert!(
        inlines
            .iter()
            .any(|inline| matches!(inline, Inline::Math(_))),
        "inline maths must become MathInline"
    );
}

#[test]
fn inline_html_parses_markdown_in_its_children() {
    let inlines = paragraph_inlines("前 <u>下划线 **粗** 尾</u> 后\n");
    let underline = inlines
        .iter()
        .find_map(|inline| match inline {
            Inline::Underline(node) => Some(node),
            _ => None,
        })
        .expect("underline");
    assert!(
        underline
            .children
            .iter()
            .any(|inline| matches!(inline, Inline::Strong(_))),
        "Markdown inside an HTML container must still parse"
    );
}

#[test]
fn inline_html_maps_links_images_breaks_and_scripts() {
    let inlines = paragraph_inlines(
        "A <a href=\"/x?a=1&amp;b=2\" title=\"T\">链接</a> <img src=\"a.png\" alt=\"图\"> <br> H<sub>2</sub>O x<sup>2</sup> <ruby>汉<rt>hàn</rt></ruby>\n",
    );
    assert!(inlines.iter().any(|inline| match inline {
        Inline::Link(node) => node.href == "/x?a=1&b=2" && node.title.as_deref() == Some("T"),
        _ => false,
    }));
    assert!(inlines.iter().any(|inline| match inline {
        Inline::Image(node) =>
            node.source.src == "a.png" && node.source.alt.as_deref() == Some("图"),
        _ => false,
    }));
    assert!(inlines
        .iter()
        .any(|inline| matches!(inline, Inline::LineBreak(_))));
    assert!(inlines
        .iter()
        .any(|inline| matches!(inline, Inline::Subscript(_))));
    assert!(inlines
        .iter()
        .any(|inline| matches!(inline, Inline::Superscript(_))));
    assert!(inlines
        .iter()
        .any(|inline| matches!(inline, Inline::Ruby(_))));
}

#[test]
fn unknown_inline_html_stays_visible() {
    let document = parse_markdown("a <foo>x</foo> b\n");
    document.validate().expect("valid DocIR");
    assert!(document.plain_text().contains("<foo>"));
}

#[test]
fn latex_inline_commands_delegate_to_the_latex_frontend() {
    let inlines = paragraph_inlines(
        "\\textbf{粗 \\emph{斜}}、\\underline{下划线}、\\href{https://example.com}{链接}、\\url{https://example.com}、\\ruby{汉}{hàn}、\\index{词}、\\LaTeX\n",
    );
    assert!(has_container(&inlines, |inline| matches!(
        inline,
        Inline::Strong(_)
    )));
    assert!(has_container(&inlines, |inline| matches!(
        inline,
        Inline::Emphasis(_)
    )));
    assert!(has_container(&inlines, |inline| matches!(
        inline,
        Inline::Underline(_)
    )));
    assert!(inlines
        .iter()
        .any(|inline| matches!(inline, Inline::Link(_))));
    assert!(inlines
        .iter()
        .any(|inline| matches!(inline, Inline::Ruby(_))));
    assert!(inlines
        .iter()
        .any(|inline| matches!(inline, Inline::Index(_))));
    assert!(document_text(&inlines).contains("LaTeX"));
}

fn document_text(inlines: &[Inline]) -> String {
    let mut out = String::new();
    for inline in inlines {
        match inline {
            Inline::Text(node) => out.push_str(&node.value),
            Inline::Code(node) => out.push_str(&node.value),
            Inline::Strong(_)
            | Inline::Emphasis(_)
            | Inline::Underline(_)
            | Inline::Strikethrough(_)
            | Inline::SmallCaps(_)
            | Inline::Superscript(_)
            | Inline::Subscript(_) => out.push_str(&document_text(inline.children())),
            Inline::Link(node) => out.push_str(&document_text(&node.children)),
            Inline::Math(node) => out.push_str(&node.source),
            Inline::Ruby(node) => out.push_str(&node.base),
            Inline::Raw(node) => out.push_str(&node.content.fallback),
            _ => {}
        }
    }
    out
}

#[test]
fn dollar_math_does_not_eat_currency() {
    let inlines = paragraph_inlines("价格 $5 和 $10，公式 $x+1$ 结束。\n");
    assert_eq!(
        inlines
            .iter()
            .filter(|inline| matches!(inline, Inline::Math(_)))
            .count(),
        1,
        "only the complete formula is maths"
    );
    assert!(document_text(&inlines).contains("$5 和 $10"));
}

#[test]
fn display_math_and_latex_environments_become_blocks() {
    let source = r#"前文

$$
\int_0^1 x^2\,dx
$$

\begin{equation}
E = mc^2
\end{equation}

\begin{itemize}
\item 甲
\item 乙
\end{itemize}

\begin{tabular}{ll}
A & B \\
1 & 2 \\
\end{tabular}
"#;
    let document = parse_markdown(source);
    document.validate().expect("valid DocIR");
    let math = document
        .blocks
        .iter()
        .filter(|block| matches!(block, Block::MathBlock(_)))
        .count();
    assert_eq!(math, 2, "dollar and equation blocks");
    assert!(document.blocks.iter().any(|block| match block {
        Block::MathBlock(node) => node.source.contains("\\int_0^1"),
        _ => false,
    }));
    assert!(document.blocks.iter().any(|block| match block {
        Block::List(node) => node.items.len() == 2,
        _ => false,
    }));
    assert!(document
        .blocks
        .iter()
        .any(|block| matches!(block, Block::Table(_))));
}

#[test]
fn html_blocks_are_semantic_and_can_span_blank_lines() {
    let source = "<div class=\"box\">\n第一段\n\n第二段 <b>粗</b>\n\n<ul>\n<li>甲</li>\n<li>乙</li>\n</ul>\n</div>\n\n正文\n";
    let document = parse_markdown(source);
    document.validate().expect("valid DocIR");
    assert!(
        document
            .blocks
            .iter()
            .any(|block| matches!(block, Block::Paragraph(_))),
        "the HTML paragraphs must survive"
    );
    assert!(
        document
            .blocks
            .iter()
            .any(|block| matches!(block, Block::List(_))),
        "the HTML list must become a List"
    );
    assert!(
        !document
            .blocks
            .iter()
            .any(|block| matches!(block, Block::Raw(_))),
        "supported HTML must not stay raw"
    );
    let text = document.plain_text();
    assert!(text.contains("第一段") && text.contains("第二段") && text.contains("正文"));
    assert!(!text.contains("<div"));
}

#[test]
fn html_blocks_parse_embedded_markdown_math_and_latex() {
    let source = "<div>\n段落里有 $x^2$ 与 \\textbf{LaTeX 粗体}。\n\n<ul>\n<li>列表里也有 $y$</li>\n</ul>\n</div>\n";
    let document = parse_markdown(source);
    document.validate().expect("valid DocIR");
    let has_math = document.blocks.iter().any(|block| match block {
        Block::Paragraph(node) => node
            .inlines
            .iter()
            .any(|inline| matches!(inline, Inline::Math(_))),
        Block::List(node) => node.items.iter().any(|item| {
            item.blocks.iter().any(|block| match block {
                Block::Paragraph(node) => node
                    .inlines
                    .iter()
                    .any(|inline| matches!(inline, Inline::Math(_))),
                _ => false,
            })
        }),
        _ => false,
    });
    assert!(has_math, "math inside an HTML block must reach DocIR");
    assert!(
        document.plain_text().contains("LaTeX 粗体"),
        "LaTeX text inside an HTML block must survive"
    );
}

#[test]
fn details_is_a_transparent_block_and_keeps_its_content() {
    let document =
        parse_markdown("<details>\n<summary>点击展开</summary>\n\n折叠内容 $x$。\n\n</details>\n");
    document.validate().expect("valid DocIR");
    let text = document.plain_text();

    assert!(text.contains("点击展开"), "{text}");
    assert!(text.contains("折叠内容"), "{text}");
    assert!(!text.contains("<details>"), "no raw tags: {text}");
    assert!(
        document.blocks.iter().any(|block| match block {
            Block::Paragraph(node) => node
                .inlines
                .iter()
                .any(|inline| matches!(inline, Inline::Math(_))),
            _ => false,
        }),
        "math inside the details body survives"
    );
}

#[test]
fn inline_latex_footnote_becomes_a_definition() {
    let document = parse_markdown("前文 \\footnote{一条注\\index{脚注词条}} 后文\n");
    document.validate().expect("valid DocIR");

    assert_eq!(document.footnotes.len(), 1, "one footnote definition");
    assert!(
        document.footnotes[0]
            .blocks
            .iter()
            .any(|block| match block {
                Block::Paragraph(node) => {
                    soma_doc::text::inlines_text(&node.inlines).contains("一条注")
                }
                _ => false,
            }),
        "the note body is in the definition"
    );
    assert!(
        document.footnotes[0]
            .blocks
            .iter()
            .any(|block| block_has_index(block, "脚注词条")),
        "an index inside the note is collected"
    );
    let paragraph = document
        .blocks
        .iter()
        .find_map(|block| match block {
            Block::Paragraph(node) => Some(node),
            _ => None,
        })
        .expect("paragraph");
    assert!(
        paragraph.inlines.iter().any(inline_is_footnote_ref),
        "the paragraph carries a real reference"
    );
    assert!(!document.plain_text().contains("\\footnote"));
}

fn inline_is_footnote_ref(inline: &Inline) -> bool {
    match inline {
        Inline::FootnoteRef(_) => true,
        _ => inline.children().iter().any(inline_is_footnote_ref),
    }
}

fn block_has_index(block: &Block, term: &str) -> bool {
    match block {
        Block::Paragraph(node) => node
            .inlines
            .iter()
            .any(|inline| inline_has_index(inline, term)),
        _ => false,
    }
}

fn inline_has_index(inline: &Inline, term: &str) -> bool {
    match inline {
        Inline::Index(node) => node.term == term,
        _ => inline
            .children()
            .iter()
            .any(|child| inline_has_index(child, term)),
    }
}

#[test]
fn a_latex_section_command_becomes_a_heading() {
    let document = parse_markdown("# 总标题\n\n\\section{脚注与索引}\n\n正文。\n");
    document.validate().expect("valid DocIR");

    let headings: Vec<_> = document
        .blocks
        .iter()
        .filter_map(|block| match block {
            Block::Heading(node) => Some(node),
            _ => None,
        })
        .collect();
    assert_eq!(headings.len(), 2);
    assert_eq!(headings[1].level, 1);
    assert_eq!(
        soma_doc::text::inlines_text(&headings[1].inlines),
        "脚注与索引"
    );
    assert!(!document.plain_text().contains("\\section"));
}

#[test]
fn html_entities_do_not_become_a_second_markdown_parse() {
    let document = parse_markdown("<p>&lt;b&gt;x&lt;/b&gt;</p>\n");
    document.validate().expect("valid DocIR");
    let Block::Paragraph(paragraph) = &document.blocks[0] else {
        panic!("paragraph");
    };
    assert!(!paragraph
        .inlines
        .iter()
        .any(|inline| matches!(inline, Inline::Strong(_))));
    assert_eq!(document.plain_text().trim(), "<b>x</b>");
}

#[test]
fn tikz_and_mermaid_environments_reuse_the_existing_blocks() {
    let source = r#"```mermaid
flowchart LR
A --> B
```

\begin{tikzpicture}
\draw (0,0) -- (1,0);
\end{tikzpicture}
"#;
    let document = parse_markdown(source);
    document.validate().expect("valid DocIR");
    assert!(document
        .blocks
        .iter()
        .any(|block| matches!(block, Block::Diagram(_))));
    assert!(document.blocks.iter().any(|block| match block {
        Block::MathBlock(node) => node.source.contains("\\begin{tikzpicture}"),
        _ => false,
    }));
}
