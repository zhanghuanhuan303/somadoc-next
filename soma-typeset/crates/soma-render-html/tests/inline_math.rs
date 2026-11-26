//! Inline formulas reach the HTML preview as positioned spans.

use soma_core::{BreakConfig, SyntheticMetrics};
use soma_doc::block::Paragraph;
use soma_doc::inline::MathInline;
use soma_doc::meta::DocumentMeta;
use soma_doc::node::NodeMeta;
use soma_doc::{Block, Document, Inline, Text};
use soma_layout::{layout, PagedLayout};
use soma_render_html::render;
use soma_template::{parse_template, resolve, StyleTable};

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

fn pages(inlines: Vec<Inline>) -> (PagedLayout, StyleTable) {
    let mut document = Document::new(DocumentMeta::new().with_language("zh-Hans"));
    document.push_block(Block::Paragraph(Paragraph::new(
        NodeMeta::synthesized(),
        inlines,
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
fn an_inline_formula_is_drawn_at_the_atom_origin() {
    let (layout, styles) = pages(vec![text("甲"), inline_math("x^2"), text("乙")]);
    let html = render(&layout, &styles, &layout.text);
    let block = &layout.pages[0].blocks[0];
    let formula = &block.inline_math[0];
    let placed = block
        .lines
        .iter()
        .find(|placed| {
            placed
                .line
                .boxes
                .iter()
                .any(|atom| atom.atom == formula.atom)
        })
        .expect("the formula line");
    let atom = placed
        .line
        .boxes
        .iter()
        .find(|atom| atom.atom == formula.atom)
        .expect("the formula atom");

    assert_eq!(
        html.matches("class=\"math\"").count(),
        formula.math.items.len()
    );
    for item in &formula.math.items {
        let x = atom.x.0 + item.x.0;
        let top = placed.y.0 + placed.line.ascent.0 + placed.line.leading.0 / 2 + item.y.0
            - item.size.0 / 8 * 7;
        assert!(
            html.contains(&format!(
                "left:{};top:{};",
                pct(x, block.width.0),
                pct(top, block.height.0)
            )),
            "item {} at {x}/{top} is drawn",
            item.text
        );
    }
    assert!(
        !html.contains('\u{e001}'),
        "the marker atom must never reach the output"
    );
}

#[test]
fn a_malformed_formula_keeps_its_source_text() {
    let (layout, styles) = pages(vec![inline_math("\\frac{1}")]);
    let html = render(&layout, &styles, &layout.text);

    assert!(layout.pages[0].blocks[0].inline_math.is_empty());
    assert_eq!(html.matches("class=\"math\"").count(), 0);
    assert!(html.contains("frac"));
}

fn pct(value: i32, total: i32) -> String {
    let scaled = value as i64 * 1_000_000 / total as i64;
    let sign = if scaled < 0 { "-" } else { "" };
    let absolute = scaled.abs();
    format!("{}{}.{:04}%", sign, absolute / 10_000, absolute % 10_000)
}
