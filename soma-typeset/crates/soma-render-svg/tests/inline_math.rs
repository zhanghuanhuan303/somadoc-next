//! Inline formulas are painted at the atom origin, on the line baseline.

use soma_core::{BreakConfig, SyntheticMetrics};
use soma_doc::inline::MathInline;
use soma_doc::meta::DocumentMeta;
use soma_doc::node::NodeMeta;
use soma_doc::{Block, Document, Inline, Text};
use soma_layout::layout;
use soma_render_svg::render;
use soma_template::{parse_template, resolve};

const TEMPLATE: &str = r#"
name: inline-math
page:
  size: A5
  margin: 10mm
styles:
  - selector:
      element: document
    declarations:
      font-size: 10.5pt
      line-height: 1.6
      align: justify
"#;

fn inline_math(source: &str) -> Inline {
    Inline::Math(MathInline::new(NodeMeta::synthesized(), source))
}

fn text(value: &str) -> Inline {
    Text::new(NodeMeta::synthesized(), value).into()
}

fn document(inlines: Vec<Inline>) -> Document {
    let mut document = Document::new(DocumentMeta::new().with_language("zh-Hans"));
    document.push_block(Block::Paragraph(soma_doc::block::Paragraph::new(
        NodeMeta::synthesized(),
        inlines,
    )));
    document.renumber();
    document
}

#[test]
fn inline_symbols_are_painted_on_the_line_baseline() {
    let doc = document(vec![text("甲"), inline_math("x^2"), text("乙")]);
    let styled = resolve(&doc, &parse_template(TEMPLATE).expect("template")).expect("styles");
    let pages = layout(
        &doc,
        &styled,
        &SyntheticMetrics::default(),
        &BreakConfig::default(),
    )
    .expect("layout");
    let svg = render(&pages, styled.table(), &pages.text);
    let block = &pages.pages[0].blocks[0];
    let poly = &block.inline_math[0];
    let placed = block
        .lines
        .iter()
        .find(|placed| placed.line.boxes.iter().any(|atom| atom.atom == poly.atom))
        .expect("the formula line");
    let atom = placed
        .line
        .boxes
        .iter()
        .find(|atom| atom.atom == poly.atom)
        .expect("the formula atom");

    let base_x = pages.page.margin_left.0 + block.x.0 + atom.x.0;
    let base_y = pages.page.margin_top.0 + block.y.0 + placed.y.0 + placed.line.ascent.0;

    assert_eq!(
        svg.matches("data-math=").count(),
        poly.math.items.len(),
        "one <text> per formula item"
    );
    for item in &poly.math.items {
        let x = base_x + item.x.0;
        let y = base_y + item.y.0;
        assert!(
            svg.contains(&format!("x=\"{}\" y=\"{}\"", em(x), em(y))),
            "item {} at {x}/{y} is painted",
            item.text
        );
    }
    assert!(
        !svg.contains('\u{e001}'),
        "the marker atom must never reach the output"
    );
}

#[test]
fn a_malformed_formula_keeps_its_source_text() {
    let doc = document(vec![inline_math("\\frac{1}")]);
    let styled = resolve(&doc, &parse_template(TEMPLATE).expect("template")).expect("styles");
    let pages = layout(
        &doc,
        &styled,
        &SyntheticMetrics::default(),
        &BreakConfig::default(),
    )
    .expect("layout");
    let svg = render(&pages, styled.table(), &pages.text);

    assert!(pages.pages[0].blocks[0].inline_math.is_empty());
    assert_eq!(svg.matches("data-math=").count(), 0);
    assert!(svg.contains("frac"), "the source is still visible");
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
