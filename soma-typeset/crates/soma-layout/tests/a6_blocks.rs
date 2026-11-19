//! A6: code, math, figures, tables, rules, raw blocks, footnotes.

mod common;

use common::{document, engine, metrics, paragraph, paragraph_with, styled, text, TIGHT};
use soma_core::Width;
use soma_doc::block::{
    Cell, CodeBlock, Column, Figure, MathBlock, Paragraph, RawBlock, Row, Table, ThematicBreak,
};
use soma_doc::inline::{FootnoteRef, Image};
use soma_doc::meta::ImageRef;
use soma_doc::node::NodeMeta;
use soma_doc::{Block, CellAlign, FootnoteDef};
use soma_layout::{layout, BoxKind, TextSource, FOOTNOTE_MARKER};

#[test]
fn code_block_keeps_source_lines() {
    let doc = document(vec![Block::CodeBlock(CodeBlock::new(
        NodeMeta::synthesized(),
        Some("rust".into()),
        "let a = 1;\nlet b = 2;\n",
    ))]);
    let styled = styled(&doc, TIGHT);
    let layout = layout(&doc, &styled, &metrics(), &engine()).expect("layout");
    let block = &layout.pages[0].blocks[0];

    assert_eq!(block.kind, BoxKind::Code);
    assert_eq!(block.lines.len(), 2, "one line per source line");
    let first = layout.text.atom_text(block.node, 0);
    assert!(first.starts_with("let"), "atom text: {first}");
}

#[test]
fn math_block_becomes_a_display_box() {
    let doc = document(vec![Block::MathBlock(MathBlock::new(
        NodeMeta::synthesized(),
        "a^2 + b^2 = c^2",
    ))]);
    let styled = styled(&doc, TIGHT);
    let layout = layout(&doc, &styled, &metrics(), &engine()).expect("layout");
    let block = &layout.pages[0].blocks[0];

    assert_eq!(block.kind, BoxKind::Math);
    let math = block.math.as_ref().expect("display math box");
    assert_eq!(math.width, block.width);
    assert!(math.items.iter().any(|item| item.text == "a"));
    assert!(
        math.items.len() > 3,
        "symbols plus scripts: {:?}",
        math.items.len()
    );
    assert!(
        math.items.iter().any(|item| item.size.0 < Width::EM.0),
        "superscripts are set smaller than the base size"
    );
    assert!(
        block.lines.is_empty(),
        "a display formula carries no text lines"
    );
    assert!(block.x.0 > 0, "a narrow formula is centred: {}", block.x.0);
    assert!(block.height.0 > 0);
}

#[test]
fn malformed_math_falls_back_to_a_text_placeholder() {
    let source = "\\frac{1}";
    let doc = document(vec![Block::MathBlock(MathBlock::new(
        NodeMeta::synthesized(),
        source,
    ))]);
    let styled = styled(&doc, TIGHT);
    let layout = layout(&doc, &styled, &metrics(), &engine()).expect("layout");
    let block = &layout.pages[0].blocks[0];

    assert_eq!(block.kind, BoxKind::Math);
    assert!(block.math.is_none(), "no math box for a broken formula");
    assert_eq!(layout.text.atom_text(block.node, 0), source);
    assert_eq!(block.lines.len(), 1, "the placeholder is one text line");
}

#[test]
fn figure_uses_the_intrinsic_aspect_ratio() {
    let figure = Figure::new(
        NodeMeta::synthesized(),
        Image::new(
            NodeMeta::synthesized(),
            ImageRef::new("photo.png").with_intrinsic(200, 100),
        ),
    );
    let doc = document(vec![Block::Figure(figure)]);
    let styled = styled(&doc, TIGHT);
    let layout = layout(&doc, &styled, &metrics(), &engine()).expect("layout");
    let block = &layout.pages[0].blocks[0];

    assert_eq!(block.kind, BoxKind::Figure);
    assert_eq!(block.height.0, block.width.0 / 2);
}

#[test]
fn a_figure_without_intrinsic_size_uses_three_by_two() {
    let figure = Figure::new(
        NodeMeta::synthesized(),
        Image::new(NodeMeta::synthesized(), ImageRef::new("photo.png")),
    );
    let doc = document(vec![Block::Figure(figure)]);
    let styled = styled(&doc, TIGHT);
    let layout = layout(&doc, &styled, &metrics(), &engine()).expect("layout");
    let block = &layout.pages[0].blocks[0];

    assert_eq!(block.height.0, block.width.0 * 2 / 3);
}

#[test]
fn table_rows_place_cells_at_column_offsets() {
    let columns = vec![
        Column::new(NodeMeta::synthesized(), CellAlign::Left),
        Column::new(NodeMeta::synthesized(), CellAlign::Right),
    ];
    let row = Row::new(
        NodeMeta::synthesized(),
        false,
        vec![
            Cell::new(
                NodeMeta::synthesized(),
                vec![Block::Paragraph(Paragraph::new(
                    NodeMeta::synthesized(),
                    vec![text("甲")],
                ))],
            ),
            Cell::new(
                NodeMeta::synthesized(),
                vec![Block::Paragraph(Paragraph::new(
                    NodeMeta::synthesized(),
                    vec![text("乙")],
                ))],
            ),
        ],
    );
    let doc = document(vec![Block::Table(Table::new(
        NodeMeta::synthesized(),
        columns,
        vec![row],
    ))]);
    let styled = styled(&doc, TIGHT);
    let layout = layout(&doc, &styled, &metrics(), &engine()).expect("layout");
    let blocks = &layout.pages[0].blocks;

    assert_eq!(blocks.len(), 2, "one box per cell");
    assert_eq!(blocks[0].kind, BoxKind::TableCell);
    assert_eq!(blocks[0].x.0, 0);
    let column_width = layout.page.content_width().0 / 2;
    assert_eq!(blocks[1].x.0, column_width);
    assert_eq!(blocks[0].y.0, blocks[1].y.0, "cells share the row baseline");
}

#[test]
fn thematic_break_is_a_rule_box() {
    let doc = document(vec![
        paragraph("前文。"),
        Block::ThematicBreak(ThematicBreak::new(NodeMeta::synthesized())),
        paragraph("后文。"),
    ]);
    let styled = styled(&doc, TIGHT);
    let layout = layout(&doc, &styled, &metrics(), &engine()).expect("layout");
    let rule = layout.pages[0]
        .blocks
        .iter()
        .find(|block| block.kind == BoxKind::Rule)
        .expect("rule box");

    assert!(rule.lines.is_empty());
    assert!(rule.height.0 > 0);
}

#[test]
fn raw_blocks_fall_back_to_text() {
    let doc = document(vec![Block::Raw(RawBlock::new(
        NodeMeta::synthesized(),
        soma_doc::RawContent::new("html", "<hr>").with_fallback("[rule]"),
    ))]);
    let styled = styled(&doc, TIGHT);
    let layout = layout(&doc, &styled, &metrics(), &engine()).expect("layout");
    let block = &layout.pages[0].blocks[0];

    assert_eq!(layout.text.atom_text(block.node, 0), "[rule]");
}

fn footnote_document() -> soma_doc::Document {
    let mut doc = document(vec![paragraph_with(vec![
        text("正文"),
        FootnoteRef::new(NodeMeta::synthesized(), "1").into(),
        text("继续。"),
    ])]);
    doc.push_footnote(FootnoteDef::new(
        NodeMeta::synthesized(),
        "1",
        vec![Block::Paragraph(Paragraph::new(
            NodeMeta::synthesized(),
            vec![text("脚注内容。")],
        ))],
    ));
    doc.renumber();
    doc
}

#[test]
fn footnotes_move_to_the_page_area() {
    let doc = footnote_document();
    let styled = styled(&doc, TIGHT);
    let layout = layout(&doc, &styled, &metrics(), &engine()).expect("layout");
    let page = &layout.pages[0];

    assert_eq!(page.footnotes.len(), 1);
    assert_eq!(page.footnotes[0].kind, BoxKind::Footnote);
    assert!(page.footnote_height.0 > 0);
    assert!(page.used_height.0 + page.footnote_height.0 <= layout.page.content_height().0);
}

#[test]
fn footnote_markers_are_marked_for_renderers() {
    let doc = footnote_document();
    let styled = styled(&doc, TIGHT);
    let layout = layout(&doc, &styled, &metrics(), &engine()).expect("layout");
    let block = &layout.pages[0].blocks[0];

    assert_eq!(block.marks.len(), 1);
    let mark = block.marks[0];
    assert_eq!(mark.number, 1);
    let marker = FOOTNOTE_MARKER.encode_utf8(&mut [0u8; 4]).to_string();
    assert_eq!(layout.text.atom_text(block.node, mark.atom), marker);
}

#[test]
fn footnote_numbering_resets_per_page() {
    let source = TIGHT.replace("name: tight", "name: tight\nfootnote:\n  numbering: page");
    let mut blocks = Vec::new();
    for index in 0..8 {
        blocks.push(paragraph_with(vec![
            text(&format!("第{index}段。")),
            FootnoteRef::new(NodeMeta::synthesized(), "note").into(),
        ]));
        blocks.push(paragraph(
            "这一段足够长，用来把内容推到下一页。中文排版需要断行与对齐。",
        ));
    }
    let mut doc = document(blocks);
    doc.push_footnote(FootnoteDef::new(
        NodeMeta::synthesized(),
        "note",
        vec![Block::Paragraph(Paragraph::new(
            NodeMeta::synthesized(),
            vec![text("同一条脚注。")],
        ))],
    ));
    doc.renumber();

    let styled = styled(&doc, &source);
    let layout = layout(&doc, &styled, &metrics(), &engine()).expect("layout");
    assert!(layout.page_count() >= 2);

    for page in &layout.pages {
        for block in &page.blocks {
            for mark in &block.marks {
                assert_eq!(
                    mark.number, 1,
                    "page numbering must restart on page {}",
                    page.number
                );
            }
        }
    }
}

#[test]
fn colspan_cells_span_their_columns() {
    let columns = (0..3)
        .map(|_| Column::new(NodeMeta::synthesized(), CellAlign::Default))
        .collect();
    let mut wide = Cell::new(
        NodeMeta::synthesized(),
        vec![Block::Paragraph(Paragraph::new(
            NodeMeta::synthesized(),
            vec![text("跨两列")],
        ))],
    );
    wide.colspan = 2;
    let row = Row::new(
        NodeMeta::synthesized(),
        true,
        vec![
            wide,
            Cell::new(
                NodeMeta::synthesized(),
                vec![Block::Paragraph(Paragraph::new(
                    NodeMeta::synthesized(),
                    vec![text("第三列")],
                ))],
            ),
        ],
    );
    let doc = document(vec![Block::Table(Table::new(
        NodeMeta::synthesized(),
        columns,
        vec![row],
    ))]);
    let styled = styled(&doc, TIGHT);
    let layout = layout(&doc, &styled, &metrics(), &engine()).expect("layout");
    let blocks = &layout.pages[0].blocks;

    assert_eq!(blocks.len(), 2);
    let column_width = layout.page.content_width().0 / 3;
    assert_eq!(blocks[0].x.0, 0);
    assert_eq!(
        blocks[0].width.0,
        column_width * 2,
        "a colspan=2 cell must be two columns wide"
    );
    assert_eq!(blocks[1].x.0, column_width * 2);
    assert_eq!(blocks[1].width.0, column_width);
}
