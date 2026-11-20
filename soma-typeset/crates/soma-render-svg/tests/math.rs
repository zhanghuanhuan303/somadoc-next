//! Display formulas are painted as `<text>` + `<rect>` at layout coordinates.

use soma_core::{BreakConfig, SyntheticMetrics};
use soma_doc::block::MathBlock;
use soma_doc::meta::DocumentMeta;
use soma_doc::node::NodeMeta;
use soma_doc::{Block, Document};
use soma_layout::layout;
use soma_render_svg::render;
use soma_template::{parse_template, resolve};

const TEMPLATE: &str = r#"
name: math
page:
  size: A5
  margin: 10mm
styles:
  - selector:
      element: document
    declarations:
      font-size: 10.5pt
      line-height: 1.6
"#;

fn pages(source: &str) -> soma_layout::PagedLayout {
    let mut document = Document::new(DocumentMeta::new().with_language("zh-Hans"));
    document.push_block(Block::MathBlock(MathBlock::new(
        NodeMeta::synthesized(),
        source,
    )));
    document.renumber();
    let styled = resolve(&document, &parse_template(TEMPLATE).expect("template")).expect("styles");
    layout(
        &document,
        &styled,
        &SyntheticMetrics::default(),
        &BreakConfig::default(),
    )
    .expect("layout")
}

#[test]
fn every_symbol_is_painted_at_its_box_position() {
    let layout = pages("a^2 + b^2 = c^2");
    let svg = render(&layout, &Default::default(), &layout.text);
    let math = layout.pages[0].blocks[0].math.as_ref().expect("math box");

    assert_eq!(
        svg.matches("data-math=").count(),
        math.items.len(),
        "one <text> per math item"
    );

    let block = &layout.pages[0].blocks[0];
    for item in &math.items {
        let x = layout.page.margin_left.0 + block.x.0 + item.x.0;
        let y = layout.page.margin_top.0 + block.y.0 + math.baseline.0 + item.y.0;
        assert!(
            svg.contains(&format!("x=\"{}\" y=\"{}\"", em(x), em(y))),
            "item {} at {x}/{y} is painted",
            item.text
        );
    }
}

#[test]
fn fraction_bars_are_painted_as_rectangles() {
    let layout = pages("\\frac{a}{b}");
    let svg = render(&layout, &Default::default(), &layout.text);
    let math = layout.pages[0].blocks[0].math.as_ref().expect("math box");

    assert_eq!(math.rules.len(), 1);
    assert_eq!(svg.matches("data-rule=\"1\"").count(), 1);
    let block = &layout.pages[0].blocks[0];
    let rule = &math.rules[0];
    let x = layout.page.margin_left.0 + block.x.0 + rule.x.0;
    let y = layout.page.margin_top.0 + block.y.0 + math.baseline.0 + rule.y.0;
    assert!(svg.contains(&format!("x=\"{}\" y=\"{}\"", em(x), em(y))));
}

#[test]
fn superscripts_render_above_the_baseline_and_smaller() {
    let layout = pages("x^2");
    let svg = render(&layout, &Default::default(), &layout.text);
    let math = layout.pages[0].blocks[0].math.as_ref().expect("math box");
    let sup = math
        .items
        .iter()
        .find(|item| item.text == "2")
        .expect("sup");

    assert!(sup.y.0 < 0);
    let block = &layout.pages[0].blocks[0];
    let y = layout.page.margin_top.0 + block.y.0 + math.baseline.0 + sup.y.0;
    let expected = format!("y=\"{}\" font-size=\"{}\"", em(y), em(sup.size.0));
    assert!(svg.contains(&expected), "expected {expected} in {svg}");
}

#[test]
fn a_broken_formula_renders_its_text_placeholder() {
    let layout = pages("\\frac{1}");
    let svg = render(&layout, &Default::default(), &layout.text);

    assert!(layout.pages[0].blocks[0].math.is_none());
    assert_eq!(svg.matches("data-math=").count(), 0);
    assert!(svg.contains("\\frac{1}") || svg.contains("frac"));
}

fn em(value: i32) -> String {
    soma_render_svg::units(soma_core::Width(value))
}
