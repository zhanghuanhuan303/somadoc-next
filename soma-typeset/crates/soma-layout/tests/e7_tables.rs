//! Rowspan grids and repeated table headers.

mod common;

use common::{document, engine, metrics, styled, text, TIGHT};
use soma_doc::block::{Cell, Column, Paragraph, Row, Table};
use soma_doc::node::NodeMeta;
use soma_doc::{Block, CellAlign};
use soma_layout::{layout, BoxKind, TextSource};

fn cell(value: &str, colspan: u16, rowspan: u16) -> Cell {
    cell_with_id(value, colspan, rowspan).0
}

/// The placed block's node is the paragraph inside the cell, not the cell.
fn cell_with_id(value: &str, colspan: u16, rowspan: u16) -> (Cell, soma_doc::NodeId) {
    let paragraph = Paragraph::new(NodeMeta::synthesized(), vec![text(value)]);
    let id = paragraph.meta.id;
    (
        Cell::new(NodeMeta::synthesized(), vec![Block::Paragraph(paragraph)])
            .with_spans(colspan, rowspan),
        id,
    )
}

fn columns(count: usize) -> Vec<Column> {
    (0..count)
        .map(|_| Column::new(NodeMeta::synthesized(), CellAlign::Default))
        .collect()
}

fn block_text(layout: &soma_layout::PagedLayout, block: &soma_layout::PlacedBlock) -> String {
    let mut out = String::new();
    for placed in &block.lines {
        for atom in &placed.line.boxes {
            out.push_str(layout.text.atom_text(block.node, atom.atom));
        }
    }
    out
}

#[test]
fn a_rowspan_cell_covers_its_rows_without_overlap() {
    let tall_text = "跨行内容".repeat(12);
    let (tall, _) = cell_with_id(&tall_text, 1, 2);
    let (top, _) = cell_with_id("右上", 1, 1);
    let (bottom, _) = cell_with_id("右下", 1, 1);

    let table = Table::new(
        NodeMeta::synthesized(),
        columns(2),
        vec![
            Row::new(NodeMeta::synthesized(), false, vec![tall, top]),
            Row::new(NodeMeta::synthesized(), false, vec![bottom]),
        ],
    );
    let doc = document(vec![Block::Table(table)]);
    let styled = styled(&doc, TIGHT);
    let layout = layout(&doc, &styled, &metrics(), &engine()).expect("layout");

    let blocks = &layout.pages[0].blocks;
    let find = |value: &str| {
        blocks
            .iter()
            .find(|block| block.kind == BoxKind::TableCell && block_text(&layout, block) == value)
            .unwrap_or_else(|| panic!("cell block {value}"))
    };
    let tall = find(&tall_text);
    let top = find("右上");
    let bottom = find("右下");

    assert_eq!(block_text(&layout, tall), tall_text);
    assert_eq!(tall.x.0, 0);
    assert_eq!(top.x.0, layout.page.content_width().0 / 2);
    assert_eq!(bottom.x.0, layout.page.content_width().0 / 2);
    assert!(
        bottom.y.0 >= top.y.0 + top.height.0,
        "row 1 must start below row 0"
    );
    assert!(
        tall.y.0 + tall.height.0 >= bottom.y.0 + bottom.height.0,
        "the spanning cell must cover the rows it spans"
    );
    let spanning_blocks = layout
        .pages
        .iter()
        .flat_map(|page| page.blocks.iter())
        .filter(|block| block.kind == BoxKind::TableCell)
        .filter(|block| block_text(&layout, block) == tall_text)
        .count();
    assert_eq!(
        spanning_blocks, 1,
        "the spanning cell is emitted once, not once per row"
    );
}

#[test]
fn a_repeated_header_appears_on_every_table_page() {
    let (header, _) = cell_with_id("表头", 1, 1);
    let mut rows = vec![Row::new(NodeMeta::synthesized(), true, vec![header])];
    for index in 0..40 {
        rows.push(Row::new(
            NodeMeta::synthesized(),
            false,
            vec![cell(&format!("第{index}行"), 1, 1)],
        ));
    }
    let table = Table::new(NodeMeta::synthesized(), columns(1), rows);
    let doc = document(vec![Block::Table(table)]);
    let styled = styled(&doc, TIGHT);
    let layout = layout(&doc, &styled, &metrics(), &engine()).expect("layout");

    assert!(layout.page_count() >= 3, "the table must span pages");
    for page in &layout.pages {
        let cells: Vec<_> = page
            .blocks
            .iter()
            .filter(|block| block.kind == BoxKind::TableCell)
            .collect();
        assert!(!cells.is_empty(), "page {} has no table cells", page.number);
        let header = cells
            .iter()
            .find(|block| block_text(&layout, block) == "表头")
            .unwrap_or_else(|| panic!("page {} has no repeated header", page.number));
        assert_eq!(header.y.0, 0, "the header sits at the top of the page");
        assert_eq!(block_text(&layout, header), "表头");
        let first_content = cells
            .iter()
            .find(|block| block_text(&layout, block) != "表头")
            .expect("content row");
        assert!(
            first_content.y.0 >= header.height.0,
            "content must start below the repeated header"
        );
    }

    assert!(layout.pages.iter().all(|page| !page.overflow));
    assert!(layout
        .pages
        .iter()
        .all(|page| page.used_height.0 <= layout.page.content_height().0));
}
