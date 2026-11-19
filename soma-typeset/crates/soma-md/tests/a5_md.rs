//! A5 Markdown front end: structure, inlines, spans, validity.

use soma_doc::block::{Block, ListItem};
use soma_doc::inline::{Inline, LineBreakKind};
use soma_md::parse_markdown;

#[test]
fn parses_headings_and_paragraphs() {
    let document = parse_markdown("# 第一章\n\n正文第一段。\n\n## 小节\n");
    document
        .validate()
        .expect("markdown must produce valid DocIR");

    assert_eq!(document.blocks.len(), 3);
    match &document.blocks[0] {
        Block::Heading(heading) => {
            assert_eq!(heading.level, 1);
            assert!(!heading.inlines.is_empty());
        }
        other => panic!("expected a heading, got {other:?}"),
    }
    match &document.blocks[2] {
        Block::Heading(heading) => assert_eq!(heading.level, 2),
        other => panic!("expected a heading, got {other:?}"),
    }
}

#[test]
fn parses_emphasis_and_inline_code() {
    let document = parse_markdown("前 **粗体** 与 *斜体* 和 `code` 结束。\n");
    let paragraph = match &document.blocks[0] {
        Block::Paragraph(paragraph) => paragraph,
        other => panic!("expected a paragraph, got {other:?}"),
    };

    assert!(paragraph
        .inlines
        .iter()
        .any(|inline| matches!(inline, Inline::Strong(_))));
    assert!(paragraph
        .inlines
        .iter()
        .any(|inline| matches!(inline, Inline::Emphasis(_))));
    assert!(paragraph
        .inlines
        .iter()
        .any(|inline| matches!(inline, Inline::Code(code) if code.value == "code")));
}

#[test]
fn parses_links_and_images() {
    let document = parse_markdown("见 [链接](https://example.com) 与 ![图](a.png)。\n");
    let paragraph = match &document.blocks[0] {
        Block::Paragraph(paragraph) => paragraph,
        other => panic!("expected a paragraph, got {other:?}"),
    };

    let link = paragraph
        .inlines
        .iter()
        .find_map(|inline| match inline {
            Inline::Link(link) => Some(link),
            _ => None,
        })
        .expect("link");
    assert_eq!(link.href, "https://example.com");

    let image = paragraph
        .inlines
        .iter()
        .find_map(|inline| match inline {
            Inline::Image(image) => Some(image),
            _ => None,
        })
        .expect("image");
    assert_eq!(image.source.src, "a.png");
    assert_eq!(image.source.alt.as_deref(), Some("图"));
}

#[test]
fn parses_lists_and_quotes() {
    let document = parse_markdown("- 第一项\n- 第二项\n\n> 引用文字\n");
    document.validate().expect("valid DocIR");

    match &document.blocks[0] {
        Block::List(list) => {
            assert!(!list.ordered);
            assert_eq!(list.items.len(), 2);
            assert!(matches!(list.items[0], ListItem { .. }));
        }
        other => panic!("expected a list, got {other:?}"),
    }
    assert!(matches!(document.blocks[1], Block::BlockQuote(_)));
}

#[test]
fn parses_ordered_lists_with_start() {
    let document = parse_markdown("3. 第三\n4. 第四\n");
    match &document.blocks[0] {
        Block::List(list) => {
            assert!(list.ordered);
            assert_eq!(list.start, 3);
            assert_eq!(list.items.len(), 2);
        }
        other => panic!("expected a list, got {other:?}"),
    }
}

#[test]
fn parses_fenced_code_and_thematic_breaks() {
    let document = parse_markdown("```rust\nlet x = 1;\n```\n\n---\n");
    document.validate().expect("valid DocIR");

    match &document.blocks[0] {
        Block::CodeBlock(code) => {
            assert_eq!(code.lang.as_deref(), Some("rust"));
            assert_eq!(code.value, "let x = 1;");
        }
        other => panic!("expected code, got {other:?}"),
    }
    assert!(matches!(document.blocks[1], Block::ThematicBreak(_)));
}

#[test]
fn hard_breaks_become_line_breaks() {
    let document = parse_markdown("第一行  \n第二行\\\n第三行\n");
    let paragraph = match &document.blocks[0] {
        Block::Paragraph(paragraph) => paragraph,
        other => panic!("expected a paragraph, got {other:?}"),
    };

    let breaks: Vec<LineBreakKind> = paragraph
        .inlines
        .iter()
        .filter_map(|inline| match inline {
            Inline::LineBreak(node) => Some(node.kind),
            _ => None,
        })
        .collect();
    assert_eq!(breaks, vec![LineBreakKind::Hard, LineBreakKind::Hard]);
}

#[test]
fn parses_footnotes_and_resolves_them() {
    let document = parse_markdown("正文[^1]。\n\n[^1]: 脚注内容。\n");
    document.validate().expect("footnote must resolve");

    assert_eq!(document.footnotes.len(), 1);
    assert_eq!(document.footnotes[0].label, "1");
    let paragraph = match &document.blocks[0] {
        Block::Paragraph(paragraph) => paragraph,
        other => panic!("expected a paragraph, got {other:?}"),
    };
    assert!(paragraph
        .inlines
        .iter()
        .any(|inline| matches!(inline, Inline::FootnoteRef(node) if node.target == "1")));
}

#[test]
fn escapes_are_literal() {
    let document = parse_markdown("\\*not emphasis\\* 和 \\`not code\\`\n");
    let paragraph = match &document.blocks[0] {
        Block::Paragraph(paragraph) => paragraph,
        other => panic!("expected a paragraph, got {other:?}"),
    };
    assert!(!paragraph
        .inlines
        .iter()
        .any(|inline| matches!(inline, Inline::Emphasis(_) | Inline::Code(_))));
}

#[test]
fn every_node_has_an_assigned_id_and_a_span() {
    let document = parse_markdown("# 标题\n\n正文 **粗** `code` [链接](x)\n\n- 项\n");
    let ids = document.node_ids();
    assert_eq!(ids.len() as u32, document.node_count());
    assert!(ids.iter().all(|id| id.0 > 0));
    assert!(document.canonical_json().contains("\"source\":1"));
}

#[test]
fn parsing_is_deterministic() {
    let source = "# 标题\n\n正文 **粗** 与 `code`。\n\n> 引用\n";
    let first = parse_markdown(source);
    let second = parse_markdown(source);

    assert_eq!(first.canonical_json(), second.canonical_json());
    assert_eq!(first.doc_hash(), second.doc_hash());
}

#[test]
fn an_index_extension_becomes_a_marker() {
    let document = parse_markdown("正文 [[index:排版]] 后文\n");
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
    assert_eq!(document.plain_text().trim(), "正文  后文");
    document.validate().expect("valid DocIR");
}
