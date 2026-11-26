//! D92: page badness is normalized by the body line height, not the height of
//! whichever block opens the page.

mod common;

use common::{document, engine, metrics, paragraph, styled, TIGHT};
use soma_core::{StyleId, Width};
use soma_doc::block::Figure;
use soma_doc::inline::Image;
use soma_doc::meta::ImageRef;
use soma_doc::node::NodeMeta;
use soma_doc::Block;
use soma_layout::{layout, layout_with, PaginationStrategy, VerticalParams};

fn body(count: usize) -> Vec<Block> {
    (0..count)
        .map(|index| {
            paragraph(&format!(
                "第{index}段。中文排版需要断行、对齐与标点处理，这一段足够长以占用多行。"
            ))
        })
        .collect()
}

fn figure_and_body() -> Vec<Block> {
    let figure = Figure::new(
        NodeMeta::synthesized(),
        Image::new(
            NodeMeta::synthesized(),
            ImageRef::new("wide.png").with_intrinsic(400, 100),
        ),
    );
    let mut blocks = vec![Block::Figure(figure)];
    blocks.extend(body(18));
    blocks
}

fn with_body_height(body_height: Width) -> VerticalParams {
    VerticalParams {
        body_height,
        ..VerticalParams::default_v1()
    }
}

#[test]
fn the_default_body_height_is_the_resolved_document_line_height() {
    let doc = document(figure_and_body());
    let styled = styled(&doc, TIGHT);
    let line_height = styled
        .style(StyleId(0))
        .expect("the document style")
        .line_height;
    assert!(line_height.0 > 0);

    let automatic = layout(&doc, &styled, &metrics(), &engine()).expect("automatic layout");
    assert!(automatic.page_count() >= 2, "the fixture must paginate");
    let first_item = automatic.pages[0].blocks[0].height;
    assert_ne!(
        first_item, line_height,
        "the page opens with a figure, so the first item is not a body line"
    );

    let explicit = layout_with(
        &doc,
        &styled,
        &metrics(),
        &engine(),
        PaginationStrategy::VerticalKp,
        with_body_height(line_height),
    )
    .expect("explicit layout");

    assert_eq!(
        automatic.pagination_cost, explicit.pagination_cost,
        "the default must resolve to the template's document line height, not the figure"
    );
    assert_eq!(
        automatic.pages, explicit.pages,
        "the resolved default and an explicit equal height choose the same pages"
    );
}

#[test]
fn a_different_body_height_changes_the_vertical_cost() {
    let doc = document(figure_and_body());
    let styled = styled(&doc, TIGHT);
    let line_height = styled
        .style(StyleId(0))
        .expect("the document style")
        .line_height;

    let automatic = layout(&doc, &styled, &metrics(), &engine()).expect("automatic layout");
    let doubled = layout_with(
        &doc,
        &styled,
        &metrics(),
        &engine(),
        PaginationStrategy::VerticalKp,
        with_body_height(Width(line_height.0 * 2)),
    )
    .expect("doubled layout");

    assert_ne!(
        automatic.pagination_cost, doubled.pagination_cost,
        "the body height is the badness denominator, so it must reach the score"
    );
}

#[test]
fn an_explicit_body_height_enters_the_vertical_hash() {
    let line_height = Width::from_em(2);
    assert_ne!(
        VerticalParams::default_v1().hash(),
        with_body_height(line_height).hash(),
        "an explicit badness denominator is part of the cost model"
    );
    assert_eq!(
        VerticalParams::default_v1().hash(),
        VerticalParams::from(soma_core::VerticalParams::default_v1()).hash(),
        "the automatic body height keeps the kernel's default hash"
    );
}
