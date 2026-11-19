//! Table rules reach the HTML renderer as positioned boxes.

use soma_core::{BreakConfig, SyntheticMetrics};
use soma_doc::block::{Cell, Column, Paragraph, Row, Table};
use soma_doc::meta::DocumentMeta;
use soma_doc::node::NodeMeta;
use soma_doc::{Block, CellAlign, Document, Text};
use soma_layout::layout;
use soma_render_html::render;
use soma_template::{parse_template, resolve};

const TEMPLATE: &str = r##"
name: ruled
page:
  size: A5
  margin: 10mm
table:
  rule-width: 0.4pt
  header-rule-width: 0.6pt
  outer-rule-width: 0.8pt
  rule-color: "#336699"
styles:
  - selector:
      element: document
    declarations:
      font-size: 10.5pt
      line-height: 1.6
      align: justify
"##;

fn cell(value: &str) -> Cell {
    Cell::new(
        NodeMeta::synthesized(),
        vec![Block::Paragraph(Paragraph::new(
            NodeMeta::synthesized(),
            vec![Text::new(NodeMeta::synthesized(), value).into()],
        ))],
    )
}

#[test]
fn a_ruled_table_emits_positioned_rule_boxes() {
    let table = Table::new(
        NodeMeta::synthesized(),
        vec![Column::new(NodeMeta::synthesized(), CellAlign::Default)],
        vec![
            Row::new(NodeMeta::synthesized(), true, vec![cell("表头")]),
            Row::new(NodeMeta::synthesized(), false, vec![cell("正文")]),
        ],
    );
    let mut document = Document::new(DocumentMeta::new().with_language("zh-Hans"));
    document.push_block(Block::Table(table));
    document.renumber();

    let styled = resolve(&document, &parse_template(TEMPLATE).expect("template")).expect("styles");
    let pages = layout(
        &document,
        &styled,
        &SyntheticMetrics::default(),
        &BreakConfig::default(),
    )
    .expect("layout");
    let html = render(&pages, styled.table(), &pages.text);

    // Top rule, header rule and the outer bottom rule.
    assert_eq!(
        html.matches("class=\"table-rule\"").count(),
        3,
        "one box per rule: {html}"
    );
    assert!(html.contains("height:0."), "the rule height is painted");
    assert!(
        html.contains("background:#336699"),
        "the rule color is painted"
    );

    let rules = pages.pages[0]
        .blocks
        .iter()
        .filter(|block| block.kind == soma_layout::BoxKind::TableRule)
        .count();
    assert_eq!(rules, 3);
}
