//! Table rules are painted as SVG rectangles at the layout coordinates.

use soma_core::{BreakConfig, SyntheticMetrics};
use soma_doc::block::{Cell, Column, Paragraph, Row, Table};
use soma_doc::meta::DocumentMeta;
use soma_doc::node::NodeMeta;
use soma_doc::{Block, CellAlign, Document, Text};
use soma_layout::layout;
use soma_render_svg::render;
use soma_template::{parse_template, resolve};

const TEMPLATE: &str = r#"
name: ruled
page:
  size: A5
  margin: 10mm
table:
  rule-width: 0.4pt
  outer-rule-width: 0.8pt
styles:
  - selector:
      element: document
    declarations:
      font-size: 10.5pt
      line-height: 1.6
      align: justify
"#;

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
fn rules_are_painted_at_layout_coordinates() {
    let table = Table::new(
        NodeMeta::synthesized(),
        vec![Column::new(NodeMeta::synthesized(), CellAlign::Default)],
        vec![
            Row::new(NodeMeta::synthesized(), false, vec![cell("一")]),
            Row::new(NodeMeta::synthesized(), false, vec![cell("二")]),
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
    let svg = render(&pages, styled.table(), &pages.text);

    // Outer top, one internal rule and the outer bottom rule.
    assert_eq!(
        svg.matches("fill=\"#333\"").count(),
        3,
        "one rect per rule: {svg}"
    );

    let rules: Vec<_> = pages.pages[0]
        .blocks
        .iter()
        .filter(|block| block.kind == soma_layout::BoxKind::TableRule)
        .collect();
    assert_eq!(rules.len(), 3);
    for rule in rules {
        let x = pages.page.margin_left.0 + rule.x.0;
        let y = pages.page.margin_top.0 + rule.y.0;
        assert!(
            svg.contains(&format!("x=\"{}\" y=\"{}\"", em(x), em(y))),
            "rule at {x}/{y} is painted"
        );
    }
}

fn em(value: i32) -> String {
    let sign = if value < 0 { "-" } else { "" };
    let absolute = value.abs();
    format!(
        "{}{}.{:03}em",
        sign,
        absolute / 4096,
        (absolute % 4096) * 1000 / 4096
    )
}
