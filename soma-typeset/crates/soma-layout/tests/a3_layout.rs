//! A3: box tree, paragraph boxes over the frozen kernel, text resolution.

mod common;

use common::{document, engine, metrics, paragraph, paragraph_with, styled, text, BOOK};
use soma_core::{break_paragraph, ParagraphStyle, StyleId, Width};
use soma_doc::block::Heading;
use soma_doc::node::NodeMeta;
use soma_doc::{Block, Inline};
use soma_layout::{layout, BoxKind, TextSource};
use soma_rules::{paragraph_from_spans, InlineSpan};

#[test]
fn paragraph_box_matches_the_inline_kernel() {
    let doc = document(vec![paragraph(
        "中文English混排，带标点。第二句更长一些用来换行。",
    )]);
    let styled = styled(&doc, BOOK);
    let layout = layout(&doc, &styled, &metrics(), &engine()).expect("layout");

    let block = &layout.pages[0].blocks[0];
    assert_eq!(block.kind, BoxKind::Paragraph);

    // Rebuild the same paragraph from the public seams and compare lines.
    let style_id = styled.style_of(block.node).expect("style");
    let computed = styled.style(style_id).expect("computed");
    let content_width = styled.page().content_width();
    let inner =
        content_width.saturating_sub(computed.indent_left.saturating_add(computed.indent_right));
    let paragraph_style = ParagraphStyle {
        width: inner,
        first_indent: computed.first_indent,
        hang: Width::ZERO,
        tracking: Width::ZERO,
        line_height: computed.line_height,
        align: computed.align,
    };
    let text_node = match &doc.blocks[0] {
        Block::Paragraph(node) => node.inlines[0].meta().id,
        other => panic!("expected a paragraph, got {other:?}"),
    };
    let span_style = styled.style_of(text_node).unwrap_or(StyleId(0));
    let spans = [InlineSpan::text(
        match &doc.blocks[0] {
            Block::Paragraph(node) => match &node.inlines[0] {
                Inline::Text(text) => text.value.as_str(),
                other => panic!("expected text, got {other:?}"),
            },
            _ => unreachable!(),
        },
        span_style,
    )];
    let rebuilt = paragraph_from_spans(&spans, paragraph_style).expect("paragraph");
    let direct = break_paragraph(&rebuilt, &metrics(), &engine()).expect("break");

    assert_eq!(block.lines.len(), direct.lines.len());
    for (placed, expected) in block.lines.iter().zip(direct.lines.iter()) {
        assert_eq!(&placed.line, expected, "line differs from the kernel");
    }
}

#[test]
fn text_index_resolves_atoms() {
    let doc = document(vec![paragraph("中文abc，标点。")]);
    let styled = styled(&doc, BOOK);
    let layout = layout(&doc, &styled, &metrics(), &engine()).expect("layout");

    let block = &layout.pages[0].blocks[0];
    let atoms: Vec<&str> = (0..6)
        .map(|index| layout.text.atom_text(block.node, index))
        .collect();

    assert_eq!(atoms[0], "中");
    assert_eq!(atoms[1], "文");
    assert_eq!(atoms[2], "abc");
    assert_eq!(atoms[3], "，");
    assert_eq!(atoms[4], "标");
    assert_eq!(atoms[5], "点");
}

#[test]
fn blocks_are_stacked_with_spacing() {
    let doc = document(vec![
        Block::Heading(Heading::new(
            NodeMeta::synthesized(),
            1,
            vec![text("第一章")],
        )),
        paragraph("正文第一段。"),
        paragraph("正文第二段。"),
    ]);
    let source = BOOK.replace(
        "      first-indent: 2em",
        "      first-indent: 2em\n      space-before: 1em",
    );
    let styled = styled(&doc, &source);
    let layout = layout(&doc, &styled, &metrics(), &engine()).expect("layout");
    let page = &layout.pages[0];

    assert_eq!(page.blocks.len(), 3);
    assert_eq!(page.blocks[0].kind, BoxKind::Heading);
    assert_eq!(page.blocks[1].kind, BoxKind::Paragraph);

    let first_bottom = page.blocks[0].y.0 + page.blocks[0].height.0;
    assert!(
        page.blocks[1].y.0 >= first_bottom,
        "the second block must start below the first"
    );
    assert_eq!(
        page.blocks[1].y.0 - first_bottom,
        soma_core::Width::EM.0,
        "the paragraph's space-before must apply"
    );
}

#[test]
fn page_geometry_and_versions_are_carried() {
    let doc = document(vec![paragraph("一点点文字。")]);
    let styled = styled(&doc, BOOK);
    let engine = engine();
    let layout = layout(&doc, &styled, &metrics(), &engine).expect("layout");

    assert_eq!(layout.page, styled.page());
    assert_eq!(layout.page_count(), 1);
    assert_eq!(layout.engine_hash, engine.param_hash().0);
    assert_eq!(layout.rules_version, engine.rules_version());
    assert_eq!(layout.metrics_version, engine.metrics_version());
    assert_eq!(layout.pagination.as_str(), "verticalKp");
}

#[test]
fn layout_is_deterministic() {
    let doc = document(vec![paragraph("确定性：同样的输入，同样的字节。")]);
    let styled = styled(&doc, BOOK);
    let first = layout(&doc, &styled, &metrics(), &engine()).expect("layout");
    let second = layout(&doc, &styled, &metrics(), &engine()).expect("layout");

    assert_eq!(first.canonical_json(), second.canonical_json());
    assert_eq!(first.hash(), second.hash());
}

#[test]
fn empty_paragraphs_are_skipped() {
    let doc = document(vec![
        paragraph_with(vec![]),
        paragraph_with(vec![text("有内容")]),
    ]);
    let styled = styled(&doc, BOOK);
    let layout = layout(&doc, &styled, &metrics(), &engine()).expect("layout");

    assert_eq!(layout.pages[0].blocks.len(), 1);
}

#[test]
fn inline_objects_stay_unbreakable() {
    let doc = document(vec![paragraph_with(vec![
        text("前"),
        Inline::Code(soma_doc::Code::new(NodeMeta::synthesized(), "let x = 1;")),
        text("后"),
    ])]);
    let styled = styled(&doc, BOOK);
    let layout = layout(&doc, &styled, &metrics(), &engine()).expect("layout");
    let block = &layout.pages[0].blocks[0];

    assert!(block
        .lines
        .iter()
        .any(|line| !line.line.boxes.is_empty() && block.lines.len() == 1));
}
