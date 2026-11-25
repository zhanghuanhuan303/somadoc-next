//! Display formulas become absolutely positioned spans plus painted rules.

use soma_core::{BreakConfig, SyntheticMetrics};
use soma_doc::block::MathBlock;
use soma_doc::meta::DocumentMeta;
use soma_doc::node::NodeMeta;
use soma_doc::{Block, Document};
use soma_layout::{layout, PagedLayout};
use soma_render_html::render;
use soma_template::{parse_template, resolve, StyleTable};

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

fn pages(source: &str) -> (PagedLayout, StyleTable) {
    let mut document = Document::new(DocumentMeta::new().with_language("zh-Hans"));
    document.push_block(Block::MathBlock(MathBlock::new(
        NodeMeta::synthesized(),
        source,
    )));
    document.renumber();
    let styled = resolve(&document, &parse_template(TEMPLATE).expect("template")).expect("styles");
    let pages = layout(
        &document,
        &styled,
        &SyntheticMetrics::default(),
        &BreakConfig::default(),
    )
    .expect("layout");
    (pages, styled.table().clone())
}

#[test]
fn symbols_become_positioned_spans() {
    let (layout, styles) = pages("a+b");
    let html = render(&layout, &styles, &layout.text);
    let math = layout.pages[0].blocks[0].math.as_ref().expect("math box");

    assert_eq!(
        html.matches("class=\"math\"").count(),
        math.items.len(),
        "one span per item"
    );
    for item in &math.items {
        assert!(
            html.contains(&format!(">{}</span>", item.text)),
            "item {} is drawn: {html}",
            item.text
        );
        assert!(
            html.contains(&format!("font-size:{};", em(item.size.0))),
            "item {} carries its size",
            item.text
        );
    }
}

#[test]
fn the_block_top_is_offset_by_the_formula_baseline() {
    let (layout, styles) = pages("x");
    let html = render(&layout, &styles, &layout.text);
    let block = &layout.pages[0].blocks[0];
    let math = block.math.as_ref().expect("math box");
    let item = &math.items[0];

    let left = layout.page.margin_left.0 + block.x.0 + item.x.0;
    let top =
        layout.page.margin_top.0 + block.y.0 + math.baseline.0 + item.y.0 - item.size.0 / 8 * 7;
    assert!(
        html.contains(&format!("left:{};", em(left)))
            && html.contains(&format!("top:{};", em(top))),
        "expected left {} top {} in {html}",
        em(left),
        em(top)
    );
}

#[test]
fn fraction_bars_become_divs() {
    let (layout, styles) = pages("\\frac{a}{b}");
    let html = render(&layout, &styles, &layout.text);
    let math = layout.pages[0].blocks[0].math.as_ref().expect("math box");
    let block = &layout.pages[0].blocks[0];
    let rule = &math.rules[0];

    assert_eq!(html.matches("class=\"math-rule\"").count(), 1);
    let left = layout.page.margin_left.0 + block.x.0 + rule.x.0;
    let top = layout.page.margin_top.0 + block.y.0 + math.baseline.0 + rule.y.0;
    assert!(
        html.contains(&format!("left:{};", em(left)))
            && html.contains(&format!("top:{};", em(top))),
        "the bar is painted at the layout coordinates"
    );
}

#[test]
fn a_broken_formula_renders_its_text_placeholder() {
    let (layout, styles) = pages("\\frac{1}");
    let html = render(&layout, &styles, &layout.text);

    assert!(layout.pages[0].blocks[0].math.is_none());
    assert_eq!(html.matches("class=\"math\"").count(), 0);
    assert!(html.contains("frac"));
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
