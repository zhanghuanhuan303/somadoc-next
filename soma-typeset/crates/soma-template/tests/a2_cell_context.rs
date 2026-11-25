//! `within:` selectors: cell content can be styled without touching the body.

use soma_core::Width;
use soma_doc::block::{Cell, Column, Paragraph, Row, Table};
use soma_doc::meta::DocumentMeta;
use soma_doc::node::NodeMeta;
use soma_doc::{Block, CellAlign, Document, Text};
use soma_template::{parse_template, resolve};

const TEMPLATE: &str = r#"
name: cells
page:
  size: A4
  margin: 20mm
styles:
  - selector:
      element: document
    declarations:
      font-size: 10.5pt
      line-height: 1.6
      align: justify
  - selector:
      element: paragraph
    declarations:
      first-indent: 2em
  - selector:
      element: paragraph
      within: cell
    declarations:
      first-indent: 0
      indent-left: 0.25em
      indent-right: 0.25em
      font-size: 9pt
"#;

fn cell(value: &str) -> Cell {
    let paragraph = Paragraph::new(
        NodeMeta::synthesized(),
        vec![Text::new(NodeMeta::synthesized(), value).into()],
    );
    Cell::new(NodeMeta::synthesized(), vec![Block::Paragraph(paragraph)])
}

#[test]
fn a_within_cell_rule_overrides_the_body_paragraph_rule() {
    let outside = Paragraph::new(
        NodeMeta::synthesized(),
        vec![Text::new(NodeMeta::synthesized(), "正文").into()],
    );

    let table = Table::new(
        NodeMeta::synthesized(),
        vec![Column::new(NodeMeta::synthesized(), CellAlign::Default)],
        vec![Row::new(
            NodeMeta::synthesized(),
            false,
            vec![cell("单元文字")],
        )],
    );
    let mut document = Document::new(DocumentMeta::new().with_language("zh-Hans"));
    document.push_block(Block::Paragraph(outside));
    document.push_block(Block::Table(table));
    document.renumber();

    let outside_id = document.blocks[0].meta().id;
    let inside_id = match &document.blocks[1] {
        Block::Table(table) => table.rows[0].cells[0].blocks[0].meta().id,
        other => panic!("expected a table, got {other:?}"),
    };

    let template = parse_template(TEMPLATE).expect("template");
    let styled = resolve(&document, &template).expect("resolve");

    let body = styled
        .style(styled.style_of(outside_id).expect("body style"))
        .expect("style exists");
    assert_eq!(body.first_indent, Width(8192), "body keeps the 2em indent");
    assert_eq!(body.indent_left, Width::ZERO);
    assert_eq!(body.font_size, Width::EM);

    let cell_style = styled
        .style(styled.style_of(inside_id).expect("cell paragraph style"))
        .expect("style exists");
    assert_eq!(cell_style.first_indent, Width::ZERO, "no indent in a cell");
    assert_eq!(
        cell_style.indent_left,
        Width(877),
        "0.25em of the cell's 9pt font size (truncated)"
    );
    assert_eq!(cell_style.indent_right, Width(877));
    assert_eq!(cell_style.font_size, Width(3510), "9pt at 10.5pt body");
}
