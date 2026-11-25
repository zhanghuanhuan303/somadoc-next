//! E5: baseline grid.

mod common;

use common::{document, engine, metrics, paragraph, styled, BOOK};
use soma_layout::layout;

fn grid_template() -> String {
    BOOK.replace(
        "line-height: 1.6",
        "line-height: 1.6\n      baseline-grid: 12pt",
    )
}

fn body() -> Vec<soma_doc::Block> {
    let unit = "中文排版的核心是把规则变成可调的代价模型。混合 English 与数字 123。";
    (0..6).map(|_| paragraph(&unit.repeat(3))).collect()
}

#[test]
fn body_lines_snap_to_the_grid() {
    let doc = document(body());
    let source = grid_template();
    let styled = styled(&doc, &source);
    let layout = layout(&doc, &styled, &metrics(), &engine()).expect("layout");

    let mut checked = 0;
    for page in &layout.pages {
        for block in page.blocks.iter().filter(|block| !block.lines.is_empty()) {
            let grid = styled
                .style(block.style)
                .map(|style| style.baseline_grid.0)
                .unwrap_or(0);
            assert!(grid > 0, "the fixture must enable the grid");
            for placed in &block.lines {
                assert_eq!(
                    placed.y.0 % grid,
                    0,
                    "line y {} is off the {grid}-unit grid",
                    placed.y.0
                );
                checked += 1;
            }
            assert_eq!(block.height.0 % grid, 0);
        }
    }
    assert!(checked > 10, "the fixture must produce several lines");
}

#[test]
fn the_grid_changes_line_pitch_but_not_the_text() {
    let doc = document(body());
    let plain = styled(&doc, BOOK);
    let grid = styled(&doc, &grid_template());

    let plain_layout = layout(&doc, &plain, &metrics(), &engine()).expect("layout");
    let grid_layout = layout(&doc, &grid, &metrics(), &engine()).expect("layout");

    assert_eq!(
        plain_layout.text.blocks(),
        grid_layout.text.blocks(),
        "the grid must not change the text"
    );

    let plain_blocks: Vec<_> = plain_layout
        .pages
        .iter()
        .flat_map(|page| page.blocks.iter())
        .map(|block| (block.node, block.lines.len(), block.height.0))
        .collect();
    let grid_blocks: Vec<_> = grid_layout
        .pages
        .iter()
        .flat_map(|page| page.blocks.iter())
        .map(|block| (block.node, block.lines.len(), block.height.0))
        .collect();
    assert_ne!(
        plain_blocks.iter().map(|(_, _, h)| *h).collect::<Vec<_>>(),
        grid_blocks.iter().map(|(_, _, h)| *h).collect::<Vec<_>>(),
        "the grid must change the measured pitch"
    );
    assert_eq!(
        plain_blocks
            .iter()
            .map(|(node, lines, _)| (*node, *lines))
            .collect::<Vec<_>>(),
        grid_blocks
            .iter()
            .map(|(node, lines, _)| (*node, *lines))
            .collect::<Vec<_>>(),
        "line breaking itself stays put"
    );
}

#[test]
fn a_grid_wider_than_the_line_height_adds_leading() {
    let doc = document(vec![paragraph("中文。")]);
    let source = BOOK.replace(
        "line-height: 1.6",
        "line-height: 1.6\n      baseline-grid: 40pt",
    );
    let styled = styled(&doc, &source);
    let layout = layout(&doc, &styled, &metrics(), &engine()).expect("layout");
    let block = &layout.pages[0].blocks[0];
    let grid = styled
        .style(block.style)
        .map(|style| style.baseline_grid.0)
        .unwrap_or(0);
    assert!(grid > 0);
    assert_eq!(block.height.0, grid, "one line occupies at least one cell");
}
