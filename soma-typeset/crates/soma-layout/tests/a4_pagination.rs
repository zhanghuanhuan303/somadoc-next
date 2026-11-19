//! A4: deterministic pagination, page breaks, keep-with-next, overflow.

mod common;

use common::{document, engine, metrics, paragraph, styled, TIGHT};
use soma_doc::block::{Heading, PageBreak};
use soma_doc::node::NodeMeta;
use soma_doc::Block;
use soma_layout::{
    layout, layout_with, BoxKind, PaginationKind, PaginationStrategy, VerticalParams,
};

fn body(count: usize) -> Vec<Block> {
    (0..count)
        .map(|index| {
            paragraph(&format!(
                "第{index}段。中文排版需要断行、对齐与标点处理，这一段足够长以占用多行。"
            ))
        })
        .collect()
}

#[test]
fn many_paragraphs_span_multiple_pages() {
    let doc = document(body(8));
    let styled = styled(&doc, TIGHT);
    let layout = layout(&doc, &styled, &metrics(), &engine()).expect("layout");

    assert!(layout.page_count() >= 2, "content must paginate");
    let content_height = layout.page.content_height();
    for page in &layout.pages {
        assert_eq!(page.blocks.first().map(|block| block.y.0), Some(0));
        for block in &page.blocks {
            assert!(
                block.y.0 + block.height.0 <= content_height.0 || page.overflow,
                "block exceeds the content box on page {}",
                page.number
            );
        }
    }
}

#[test]
fn page_break_starts_a_new_page() {
    let doc = document(vec![
        paragraph("第一页内容。"),
        Block::PageBreak(PageBreak::new(NodeMeta::synthesized())),
        paragraph("第二页内容。"),
    ]);
    let styled = styled(&doc, TIGHT);
    let layout = layout(&doc, &styled, &metrics(), &engine()).expect("layout");

    assert_eq!(layout.page_count(), 2);
    assert_eq!(layout.pages[0].blocks.len(), 1);
    assert_eq!(layout.pages[1].blocks.len(), 1);
    assert_eq!(layout.pages[1].blocks[0].y.0, 0);
}

#[test]
fn a_heading_never_ends_a_page() {
    let mut blocks = Vec::new();
    for index in 0..4 {
        blocks.push(Block::Heading(Heading::new(
            NodeMeta::synthesized(),
            2,
            vec![common::text(&format!("小节 {index}"))],
        )));
        blocks.push(paragraph(
            "正文。中文排版需要断行、对齐与标点处理，这一段足够长以占用多行。",
        ));
    }
    let doc = document(blocks);
    let styled = styled(&doc, TIGHT);
    let layout = layout(&doc, &styled, &metrics(), &engine()).expect("layout");

    assert!(layout.page_count() >= 2, "content must paginate");
    for page in &layout.pages {
        if let Some(last) = page.blocks.last() {
            assert_ne!(
                last.kind,
                BoxKind::Heading,
                "heading was stranded at the bottom of page {}",
                page.number
            );
        }
    }
}

#[test]
fn blocks_start_at_the_top_of_every_page() {
    let doc = document(body(6));
    let styled = styled(&doc, TIGHT);
    let layout = layout(&doc, &styled, &metrics(), &engine()).expect("layout");

    for page in &layout.pages {
        assert_eq!(
            page.blocks.first().map(|block| block.y.0),
            Some(0),
            "space-before must be dropped at the page top"
        );
    }
}

#[test]
fn a_block_taller_than_the_page_is_marked_as_overflow() {
    let long = "长".repeat(400);
    let doc = document(vec![paragraph(&long)]);
    let styled = styled(&doc, TIGHT);
    let layout = layout(&doc, &styled, &metrics(), &engine()).expect("layout");

    assert_eq!(layout.page_count(), 1);
    assert!(layout.pages[0].overflow, "overflow must be reported");
    assert!(layout.pages[0].blocks[0].height.0 > layout.page.content_height().0);
}

#[test]
fn pagination_is_deterministic() {
    let doc = document(body(9));
    let styled = styled(&doc, TIGHT);
    let first = layout(&doc, &styled, &metrics(), &engine()).expect("layout");
    let second = layout(&doc, &styled, &metrics(), &engine()).expect("layout");

    assert_eq!(first.canonical_json(), second.canonical_json());
    assert_eq!(first.page_count(), second.page_count());
}

#[test]
fn an_empty_document_still_produces_one_page() {
    let doc = document(Vec::new());
    let styled = styled(&doc, TIGHT);
    let layout = layout(&doc, &styled, &metrics(), &engine()).expect("layout");

    assert_eq!(layout.page_count(), 1);
    assert!(layout.pages[0].blocks.is_empty());
}

#[test]
fn vertical_pagination_never_costs_more_than_greedy() {
    let doc = document(body(9));
    let styled = styled(&doc, TIGHT);
    let vertical = layout_with(
        &doc,
        &styled,
        &metrics(),
        &engine(),
        PaginationStrategy::VerticalKp,
        VerticalParams::default_v1(),
    )
    .expect("vertical");
    let greedy = layout_with(
        &doc,
        &styled,
        &metrics(),
        &engine(),
        PaginationStrategy::Greedy,
        VerticalParams::default_v1(),
    )
    .expect("greedy");

    assert_eq!(vertical.pagination, PaginationKind::VerticalKp);
    assert_eq!(greedy.pagination, PaginationKind::Greedy);
    assert!(
        vertical.pagination_cost <= greedy.pagination_cost,
        "vertical {} must not cost more than greedy {}",
        vertical.pagination_cost,
        greedy.pagination_cost
    );
    assert!(vertical.page_count() <= greedy.page_count());
}

#[test]
fn vertical_pagination_balances_a_ragged_tail() {
    // Two short blocks and one long one: greedy leaves a nearly empty page,
    // vertical K-P can pull the last block back.
    let mut blocks = body(3);
    blocks.push(paragraph(
        "这一段很长，用来把内容推到第二页，并让贪心留下一个很空的尾页。中文排版需要断行与对齐。",
    ));
    let doc = document(blocks);
    let styled = styled(&doc, TIGHT);

    let vertical = layout_with(
        &doc,
        &styled,
        &metrics(),
        &engine(),
        PaginationStrategy::VerticalKp,
        VerticalParams::default_v1(),
    )
    .expect("vertical");
    let greedy = layout_with(
        &doc,
        &styled,
        &metrics(),
        &engine(),
        PaginationStrategy::Greedy,
        VerticalParams::default_v1(),
    )
    .expect("greedy");

    assert!(vertical.pagination_cost <= greedy.pagination_cost);
}

#[test]
fn vertical_pagination_respects_forced_breaks() {
    let doc = document(vec![
        paragraph("第一页内容。"),
        Block::PageBreak(PageBreak::new(NodeMeta::synthesized())),
        paragraph("第二页内容。"),
        Block::PageBreak(PageBreak::new(NodeMeta::synthesized())),
        paragraph("第三页内容。"),
    ]);
    let styled = styled(&doc, TIGHT);
    let layout = layout(&doc, &styled, &metrics(), &engine()).expect("layout");

    assert_eq!(layout.page_count(), 3);
    assert_eq!(layout.pages[0].blocks.len(), 1);
    assert_eq!(layout.pages[2].blocks.len(), 1);
}

#[test]
fn vertical_pagination_keeps_headings_with_next() {
    let mut blocks = Vec::new();
    for index in 0..4 {
        blocks.push(Block::Heading(Heading::new(
            NodeMeta::synthesized(),
            2,
            vec![common::text(&format!("小节 {index}"))],
        )));
        blocks.push(paragraph(
            "正文。中文排版需要断行、对齐与标点处理，这一段足够长以占用多行。",
        ));
    }
    let doc = document(blocks);
    let styled = styled(&doc, TIGHT);
    let layout = layout(&doc, &styled, &metrics(), &engine()).expect("layout");

    for page in &layout.pages {
        if let Some(last) = page.blocks.last() {
            assert_ne!(last.kind, BoxKind::Heading, "page {}", page.number);
        }
    }
}

#[test]
fn vertical_pagination_is_deterministic_and_reports_its_cost() {
    let doc = document(body(10));
    let styled = styled(&doc, TIGHT);
    let first = layout(&doc, &styled, &metrics(), &engine()).expect("layout");
    let second = layout(&doc, &styled, &metrics(), &engine()).expect("layout");

    assert_eq!(first.canonical_json(), second.canonical_json());
    assert!(first.pagination_cost >= 0);
    assert_eq!(first.footnote_passes, 1);
}
