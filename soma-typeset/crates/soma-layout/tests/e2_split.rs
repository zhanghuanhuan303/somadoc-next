//! E2: line-level vertical layout, widows and orphans, glue, footnote lines.

mod common;

use common::{document, engine, metrics, paragraph, styled, text, TIGHT};
use soma_doc::block::{Heading, Paragraph};
use soma_doc::inline::FootnoteRef;
use soma_doc::node::NodeMeta;
use soma_doc::{Block, Document, FootnoteDef};
use soma_layout::{layout, BoxKind};
use soma_layout::{layout_with, PaginationKind, PaginationStrategy, VerticalParams};

fn long_document(paragraphs: usize) -> Document {
    let mut blocks = Vec::new();
    for index in 0..paragraphs {
        let units = 40 + (index * 37) % 120;
        blocks.push(paragraph(&"中".repeat(units)));
    }
    document(blocks)
}

#[test]
fn paragraphs_split_and_line_offsets_cover_every_line_once() {
    let doc = long_document(6);
    let styled = styled(&doc, TIGHT);
    let layout = layout(&doc, &styled, &metrics(), &engine()).expect("layout");

    assert!(layout.page_count() > 1);
    let mut seen: Vec<(u32, u32)> = Vec::new();
    for page in &layout.pages {
        for block in page.blocks.iter().filter(|block| !block.lines.is_empty()) {
            for offset in 0..block.lines.len() {
                seen.push((block.node.0, block.line_offset + offset as u32));
            }
        }
    }
    let before = seen.len();
    seen.sort_unstable();
    seen.dedup();
    assert_eq!(seen.len(), before, "no line may appear twice");
}

#[test]
fn widows_and_orphans_are_avoided() {
    let doc = long_document(12);
    let styled = styled(&doc, TIGHT);
    let layout = layout(&doc, &styled, &metrics(), &engine()).expect("layout");
    assert!(layout.page_count() > 1);

    for page in &layout.pages {
        for block in page.blocks.iter().filter(|block| !block.lines.is_empty()) {
            if block.lines_total < 3 {
                continue;
            }
            assert!(
                block.line_offset + 1 != block.lines_total,
                "widow: page starts with the last line of node {}",
                block.node.0
            );
            assert!(
                !(block.line_offset == 0 && block.lines.len() == 1),
                "orphan: page ends with the first line of node {}",
                block.node.0
            );
        }
    }
}

#[test]
fn a_footnote_lands_on_the_page_of_its_marker_line() {
    let mut inlines = vec![text(&"中".repeat(300))];
    inlines.push(FootnoteRef::new(NodeMeta::synthesized(), "1").into());
    inlines.push(text(&"文".repeat(160)));

    let mut doc = document(vec![Block::Paragraph(Paragraph::new(
        NodeMeta::synthesized(),
        inlines,
    ))]);
    doc.push_footnote(FootnoteDef::new(
        NodeMeta::synthesized(),
        "1",
        vec![Block::Paragraph(Paragraph::new(
            NodeMeta::synthesized(),
            vec![text("脚注内容。")],
        ))],
    ));
    doc.renumber();

    let styled = styled(&doc, TIGHT);
    let layout = layout(&doc, &styled, &metrics(), &engine()).expect("layout");
    assert!(layout.page_count() > 1, "the paragraph must span pages");

    let mut marker_pages = 0;
    for page in &layout.pages {
        let has_mark = page.blocks.iter().any(|block| !block.marks.is_empty());
        if has_mark {
            marker_pages += 1;
            assert!(
                !page.footnotes.is_empty(),
                "the page holding the marker must carry the note"
            );
        } else {
            assert!(
                page.footnotes.is_empty(),
                "a page without the marker must not carry the note"
            );
        }
    }
    assert_eq!(marker_pages, 1);
}

#[test]
fn keep_with_next_moves_the_heading_with_the_next_block() {
    let mut blocks = Vec::new();
    for _ in 0..7 {
        blocks.push(paragraph(
            "这一段用来占满页面的前几行，排版需要断行与对齐。",
        ));
    }
    blocks.push(Block::Heading(Heading::new(
        NodeMeta::synthesized(),
        2,
        vec![text("小节标题")],
    )));
    blocks.push(paragraph("标题后的正文内容，必须与标题同页。"));

    let doc = document(blocks);
    let styled = styled(&doc, TIGHT);
    let layout = layout(&doc, &styled, &metrics(), &engine()).expect("layout");
    assert!(layout.page_count() > 1);

    for page in &layout.pages {
        if let Some(last) = page.blocks.last() {
            assert_ne!(
                last.kind,
                BoxKind::Heading,
                "a heading must not be the last block on a page"
            );
        }
    }
}

#[test]
fn vertical_glue_fills_the_page_before_leaving_a_gap() {
    let source = TIGHT.replace(
        "first-indent: 2em",
        "first-indent: 2em\n      space-before: 1em\n      space-before-plus: 1em",
    );
    let doc = document(vec![
        paragraph("第一段。"),
        paragraph("第二段。"),
        paragraph("第三段。"),
    ]);
    let styled = styled(&doc, &source);
    let layout = layout(&doc, &styled, &metrics(), &engine()).expect("layout");

    let page = &layout.pages[0];
    assert!(page.blocks.len() >= 3);
    for pair in page.blocks.windows(2) {
        let gap = pair[1].y.0 - (pair[0].y.0 + pair[0].height.0);
        assert!(
            gap > 4096,
            "natural space is 1em; glue must stretch it, got {gap}"
        );
    }
    assert!(page.used_height.0 <= layout.page.content_height().0);
}

#[test]
fn the_greedy_strategy_still_works_and_is_recorded() {
    let doc = long_document(4);
    let styled = styled(&doc, TIGHT);
    let layout = layout_with(
        &doc,
        &styled,
        &metrics(),
        &engine(),
        PaginationStrategy::Greedy,
        VerticalParams::default_v1(),
    )
    .expect("layout");
    assert_eq!(layout.pagination, PaginationKind::Greedy);
    assert!(layout.page_count() >= 1);
}

#[test]
fn the_vertical_hash_is_part_of_the_canonical_result() {
    let doc = long_document(3);
    let styled = styled(&doc, TIGHT);
    let mut params = VerticalParams::default_v1();
    let first = layout_with(
        &doc,
        &styled,
        &metrics(),
        &engine(),
        PaginationStrategy::VerticalKp,
        params,
    )
    .expect("layout");
    params.widow_penalty += 1;
    let second = layout_with(
        &doc,
        &styled,
        &metrics(),
        &engine(),
        PaginationStrategy::VerticalKp,
        params,
    )
    .expect("layout");

    assert_ne!(first.vertical_hash, second.vertical_hash);
    assert!(first
        .canonical_json()
        .contains(&format!("\"verticalHash\":{}", first.vertical_hash)));
}
