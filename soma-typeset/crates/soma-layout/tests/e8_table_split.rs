//! Table row bands split across pages.
//!
//! A table is one flow item whose parts are runs of rows that no rowspan cell
//! crosses. A page break can only fall on a part boundary, so a continuation
//! page is assembled by shifting whole cell blocks: no cell is ever clipped.

mod common;

use common::{document, engine, metrics, styled, text, TIGHT};
use soma_doc::block::{Cell, Column, Paragraph, Row, Table};
use soma_doc::node::NodeMeta;
use soma_doc::{Block, CellAlign};
use soma_layout::{layout, BoxKind, PagedLayout, PlacedBlock, TextSource};

fn cell(value: &str, colspan: u16, rowspan: u16) -> Cell {
    let paragraph = Paragraph::new(NodeMeta::synthesized(), vec![text(value)]);
    Cell::new(NodeMeta::synthesized(), vec![Block::Paragraph(paragraph)])
        .with_spans(colspan, rowspan)
}

fn columns(count: usize) -> Vec<Column> {
    (0..count)
        .map(|_| Column::new(NodeMeta::synthesized(), CellAlign::Default))
        .collect()
}

fn block_text(layout: &PagedLayout, block: &PlacedBlock) -> String {
    let mut out = String::new();
    for placed in &block.lines {
        for atom in &placed.line.boxes {
            out.push_str(layout.text.atom_text(block.node, atom.atom));
        }
    }
    out
}

fn cell_blocks(layout: &PagedLayout, page: usize) -> Vec<&PlacedBlock> {
    layout.pages[page]
        .blocks
        .iter()
        .filter(|block| block.kind == BoxKind::TableCell)
        .collect()
}

fn page_of(layout: &PagedLayout, value: &str) -> usize {
    layout
        .pages
        .iter()
        .position(|page| {
            page.blocks
                .iter()
                .any(|block| block.kind == BoxKind::TableCell && block_text(layout, block) == value)
        })
        .unwrap_or_else(|| panic!("cell {value} is missing"))
}

/// Every body row appears exactly once, in page order.
fn assert_row_conservation(layout: &PagedLayout, rows: &[String]) {
    let mut seen: Vec<String> = Vec::new();
    for page in &layout.pages {
        for block in page.blocks.iter().filter(|b| b.kind == BoxKind::TableCell) {
            seen.push(block_text(layout, block));
        }
    }
    for row in rows {
        let count = seen.iter().filter(|text| *text == row).count();
        assert_eq!(count, 1, "row {row} must appear exactly once, saw {count}");
    }
}

#[test]
fn a_long_table_splits_across_pages_without_losing_rows() {
    let mut rows = vec![Row::new(
        NodeMeta::synthesized(),
        true,
        vec![cell("表头", 1, 1)],
    )];
    let mut body = Vec::new();
    for index in 0..40 {
        let value = format!("第{index:02}行");
        rows.push(Row::new(
            NodeMeta::synthesized(),
            false,
            vec![cell(&value, 1, 1)],
        ));
        body.push(value);
    }
    let table = Table::new(NodeMeta::synthesized(), columns(1), rows);
    let doc = document(vec![Block::Table(table)]);
    let styled = styled(&doc, TIGHT);
    let layout = layout(&doc, &styled, &metrics(), &engine()).expect("layout");

    assert!(layout.page_count() >= 3, "the table must continue");
    assert_row_conservation(&layout, &body);

    for (page_index, page) in layout.pages.iter().enumerate() {
        let cells = cell_blocks(&layout, page_index);
        let headers: Vec<_> = cells
            .iter()
            .filter(|block| block_text(&layout, block) == "表头")
            .collect();
        assert_eq!(
            headers.len(),
            1,
            "page {} repeats the header once",
            page.number
        );
        assert_eq!(headers[0].y.0, 0, "the header sits at the top of the page");
        let first_body = cells
            .iter()
            .find(|block| block_text(&layout, block) != "表头")
            .expect("content row");
        assert!(
            first_body.y.0 >= headers[0].height.0,
            "page {} starts below the header",
            page.number
        );
    }

    assert!(layout.pages.iter().all(|page| !page.overflow));
    assert!(layout
        .pages
        .iter()
        .all(|page| page.used_height.0 <= layout.page.content_height().0));
}

#[test]
fn a_spanning_cell_keeps_its_rows_on_one_page() {
    let mut rows = Vec::new();
    for index in 0..60 {
        let row = if index == 10 {
            Row::new(
                NodeMeta::synthesized(),
                false,
                vec![cell("跨行内容", 1, 3), cell("行10", 1, 1)],
            )
        } else if index == 11 {
            Row::new(NodeMeta::synthesized(), false, vec![cell("行11", 1, 1)])
        } else if index == 12 {
            Row::new(NodeMeta::synthesized(), false, vec![cell("行12", 1, 1)])
        } else {
            let value = format!("行{index:02}");
            Row::new(
                NodeMeta::synthesized(),
                false,
                vec![cell(&value, 1, 1), cell(&value, 1, 1)],
            )
        };
        rows.push(row);
    }
    let table = Table::new(NodeMeta::synthesized(), columns(2), rows);
    let doc = document(vec![Block::Table(table)]);
    let styled = styled(&doc, TIGHT);
    let layout = layout(&doc, &styled, &metrics(), &engine()).expect("layout");

    assert!(layout.page_count() >= 3, "the table must continue");
    let spanning: Vec<String> = layout
        .pages
        .iter()
        .flat_map(|page| page.blocks.iter())
        .filter(|block| block.kind == BoxKind::TableCell)
        .map(|block| block_text(&layout, block))
        .filter(|value| value == "跨行内容")
        .collect();
    assert_eq!(spanning.len(), 1, "the spanning cell is emitted once");

    let span_page = page_of(&layout, "跨行内容");
    for value in ["行10", "行11", "行12"] {
        assert_eq!(
            page_of(&layout, value),
            span_page,
            "row {value} must stay on the page that holds the spanning cell"
        );
    }

    assert!(layout.pages.iter().all(|page| !page.overflow));
}

#[test]
fn a_table_without_a_header_splits_too() {
    let mut rows = Vec::new();
    let mut body = Vec::new();
    for index in 0..50 {
        let value = format!("单元格{index:02}");
        rows.push(Row::new(
            NodeMeta::synthesized(),
            false,
            vec![cell(&value, 1, 1)],
        ));
        body.push(value);
    }
    let table = Table::new(NodeMeta::synthesized(), columns(1), rows);
    let doc = document(vec![Block::Table(table)]);
    let styled = styled(&doc, TIGHT);
    let layout = layout(&doc, &styled, &metrics(), &engine()).expect("layout");

    assert!(layout.page_count() >= 3, "the table must continue");
    assert_row_conservation(&layout, &body);
    assert!(layout.pages.iter().all(|page| !page.overflow));
    assert!(layout
        .pages
        .iter()
        .all(|page| page.used_height.0 <= layout.page.content_height().0));
}

#[test]
fn a_tall_cell_is_moved_whole_not_clipped() {
    let tall = "高单元格".repeat(30);
    let mut rows = vec![Row::new(
        NodeMeta::synthesized(),
        false,
        vec![cell(&tall, 1, 1)],
    )];
    for index in 0..12 {
        rows.push(Row::new(
            NodeMeta::synthesized(),
            false,
            vec![cell(&format!("短{index:02}"), 1, 1)],
        ));
    }
    let table = Table::new(NodeMeta::synthesized(), columns(1), rows);
    let doc = document(vec![Block::Table(table)]);
    let styled = styled(&doc, TIGHT);
    let layout = layout(&doc, &styled, &metrics(), &engine()).expect("layout");

    let mut seen = 0usize;
    for page in &layout.pages {
        for block in page.blocks.iter().filter(|b| b.kind == BoxKind::TableCell) {
            if block_text(&layout, block) == tall {
                seen += 1;
                assert_eq!(
                    block.lines.len(),
                    block.lines_total as usize,
                    "a tall cell is never sliced"
                );
                assert_eq!(block.line_offset, 0);
            }
        }
    }
    assert_eq!(seen, 1, "the tall cell appears once and whole");
}
