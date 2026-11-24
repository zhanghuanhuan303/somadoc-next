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
fn parses_strikethrough() {
    let document = parse_markdown("前 ~~删除~~ 后。\n");
    let paragraph = match &document.blocks[0] {
        Block::Paragraph(paragraph) => paragraph,
        other => panic!("expected a paragraph, got {other:?}"),
    };
    assert!(
        paragraph
            .inlines
            .iter()
            .any(|inline| matches!(inline, Inline::Strikethrough(_))),
        "GFM `~~` is a strikethrough, and the IR carries it"
    );
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

#[test]
fn a_ruby_extension_becomes_an_annotation() {
    let document = parse_markdown("正文 [[ruby:汉字|hànzì]] 后文\n");
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
    assert_eq!(document.plain_text().trim(), "正文 汉字 后文");
    document.validate().expect("valid DocIR");
}

#[test]
fn front_matter_becomes_document_metadata() {
    let source = "---\ntitle: 中文排版案例集\nsubtitle: 九篇语料\nauthor: 张三, 李四\ndate: 2026-11-20\nlanguage: zh-Hans\nkeywords: 排版, 中文\n---\n\n正文第一段。\n";
    let document = parse_markdown(source);

    assert_eq!(document.meta.authors.len(), 2);
    assert_eq!(document.meta.authors[0].name, "张三");
    assert_eq!(document.meta.language.as_deref(), Some("zh-Hans"));
    assert_eq!(document.meta.keywords.len(), 2);
    let cover = document.meta.cover.as_ref().expect("cover data");
    assert_eq!(cover.subtitle.as_deref(), Some("九篇语料"));
    assert_eq!(cover.authors.len(), 2);
    assert!(cover.date.is_some());
    // The body starts after the closing marker, not at the title.
    assert!(!document.blocks.is_empty());
}

#[test]
fn an_unterminated_front_matter_block_is_ordinary_text() {
    let document = parse_markdown("---\ntitle: 没有结束标记\n\n正文。\n");
    assert!(document.meta.title.is_none());
    assert!(document.meta.cover.is_none());
    assert!(!document.blocks.is_empty());
}

#[test]
fn setext_underlines_make_headings() {
    let document = parse_markdown("一级标题\n===\n\n二级标题\n---\n\n正文。\n");
    document.validate().expect("valid DocIR");

    match &document.blocks[0] {
        Block::Heading(heading) => assert_eq!(heading.level, 1),
        other => panic!("expected a level-1 heading, got {other:?}"),
    }
    match &document.blocks[1] {
        Block::Heading(heading) => assert_eq!(heading.level, 2),
        other => panic!("expected a level-2 heading, got {other:?}"),
    }
    assert!(matches!(document.blocks[2], Block::Paragraph(_)));
}

#[test]
fn a_nested_list_is_a_list_inside_an_item() {
    let document = parse_markdown("- 甲\n  - 甲一\n  - 甲二\n- 乙\n");
    document.validate().expect("valid DocIR");
    let Block::List(list) = &document.blocks[0] else {
        panic!("expected a list");
    };
    assert_eq!(list.items.len(), 2);
    let Block::List(nested) = &list.items[0].blocks[1] else {
        panic!(
            "the first item holds a nested list: {:?}",
            list.items[0].blocks
        );
    };
    assert_eq!(nested.items.len(), 2);
    assert!(nested.items[0].meta.id.0 > 0);
    assert!(list.items[1]
        .blocks
        .iter()
        .all(|block| matches!(block, Block::Paragraph(_))));
}

#[test]
fn a_nested_ordered_list_keeps_its_own_numbering() {
    let document = parse_markdown("- 甲\n  3. 三\n  4. 四\n");
    let Block::List(list) = &document.blocks[0] else {
        panic!("expected a list");
    };
    let Block::List(nested) = &list.items[0].blocks[1] else {
        panic!("nested ordered list");
    };
    assert!(nested.ordered);
    assert_eq!(nested.start, 3);
    assert_eq!(nested.items.len(), 2);
}

#[test]
fn a_gfm_table_becomes_a_table_block() {
    let document = parse_markdown("| 名称 | 数值 |\n| :--- | ---: |\n| 甲 | 1 |\n| 乙 | 22 |\n");
    document.validate().expect("valid DocIR");
    let Block::Table(table) = &document.blocks[0] else {
        panic!("expected a table");
    };
    assert_eq!(table.columns.len(), 2);
    assert_eq!(table.columns[0].align, soma_doc::CellAlign::Left);
    assert_eq!(table.columns[1].align, soma_doc::CellAlign::Right);
    assert_eq!(
        (table.columns[0].share, table.columns[1].share),
        (0, 0),
        "equal dashes keep the measured widths"
    );
    assert_eq!(table.rows.len(), 3);
    assert!(table.rows[0].header);
    assert_eq!(table.rows[2].cells.len(), 2);
    assert!(document.plain_text().contains("22"));
}

#[test]
fn table_dashes_become_column_shares() {
    let document = parse_markdown("| 短 | 长 |\n| --- | -------- |\n| 甲 | 乙 |\n");
    let Block::Table(table) = &document.blocks[0] else {
        panic!("expected a table");
    };
    assert_eq!(
        (table.columns[0].share, table.columns[1].share),
        (272, 728),
        "the longer dash run gets the wider column"
    );
}

#[test]
fn a_table_cell_can_hold_an_escaped_bar() {
    let document = parse_markdown("| 表达式 |\n| --- |\n| a \\| b |\n");
    let Block::Table(table) = &document.blocks[0] else {
        panic!("expected a table");
    };
    let cell = &table.rows[1].cells[0];
    let text = cell
        .blocks
        .first()
        .and_then(|block| match block {
            Block::Paragraph(paragraph) => Some(paragraph.inlines.as_slice()),
            _ => None,
        })
        .expect("cell paragraph");
    assert!(matches!(
        &text[0],
        Inline::Text(node) if node.value.contains("a | b")
    ));
}

#[test]
fn a_table_needs_a_delimiter_row() {
    let document = parse_markdown("| 甲 | 乙 |\n| 丙 | 丁 |\n");
    assert!(matches!(document.blocks[0], Block::Paragraph(_)));
}
