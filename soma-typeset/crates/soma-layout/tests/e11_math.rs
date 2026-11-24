//! E11: display math placed by the layout pipeline.
//!
//! The formula geometry comes from `soma-math`; these tests only check the
//! contract the rest of the engine relies on: one centred box, a baseline that
//! renderers can position from, and no effect on the surrounding text flow.

mod common;

use common::{document, engine, metrics, paragraph, styled, BOOK, TIGHT};
use soma_core::{Metrics, Width};
use soma_doc::block::MathBlock;
use soma_doc::node::NodeMeta;
use soma_doc::Block;
use soma_layout::{layout, BoxKind, PagedLayout};

fn math_layout(source: &str) -> PagedLayout {
    let doc = document(vec![Block::MathBlock(MathBlock::new(
        NodeMeta::synthesized(),
        source,
    ))]);
    let styled = styled(&doc, TIGHT);
    layout(&doc, &styled, &metrics(), &engine()).expect("layout")
}

#[test]
fn display_formula_is_centred_on_the_content_box() {
    let layout = math_layout("a^2 + b^2 = c^2");
    let block = &layout.pages[0].blocks[0];
    let math = block.math.as_ref().expect("math box");

    assert_eq!(block.kind, BoxKind::Math);
    assert_eq!(block.width, math.width);
    assert_eq!(block.height, math.height);
    let content = layout.page.content_width();
    assert_eq!(block.x, Width((content.0 - math.width.0).max(0) / 2));
    assert!(
        math.baseline.0 > 0 && math.baseline.0 < math.height.0,
        "baseline {} inside height {}",
        math.baseline.0,
        math.height.0
    );
    assert!(math.width.0 > 0 && math.height.0 > 0);
}

#[test]
fn consecutive_display_formulas_keep_the_default_skip() {
    let doc = document(vec![
        Block::MathBlock(MathBlock::new(NodeMeta::synthesized(), r"\frac{a}{b}")),
        Block::MathBlock(MathBlock::new(NodeMeta::synthesized(), r"\sqrt{x}")),
    ]);
    let styled = styled(&doc, BOOK);
    let layout = layout(&doc, &styled, &metrics(), &engine()).expect("layout");
    let first = &layout.pages[0].blocks[0];
    let second = &layout.pages[0].blocks[1];
    let gap = second.y.0 - (first.y.0 + first.height.0);
    let body = styled.table().styles()[0].font_size.0;
    assert!(
        gap >= body * 9 / 10,
        "display skips: gap {gap}, body {body}"
    );
}

#[test]
fn superscript_sits_above_the_baseline_and_is_smaller() {
    let layout = math_layout("x^2");
    let math = layout.pages[0].blocks[0].math.as_ref().expect("math box");

    let base = math
        .items
        .iter()
        .find(|item| item.text == "x")
        .expect("base symbol");
    let sup = math
        .items
        .iter()
        .find(|item| item.text == "2")
        .expect("superscript");

    assert_eq!(base.y, Width::ZERO, "the base sits on the baseline");
    assert!(sup.y.0 < 0, "superscript lifts above the baseline");
    assert!(sup.size.0 < base.size.0, "scripts are set smaller");
    assert!(sup.x.0 >= base.x.0 + metrics().advance("x", soma_core::StyleId(0)).0);
    // The raised script must be counted in the ascent, not clipped: the box
    // top is above the superscript's own baseline.
    assert!(
        math.baseline.0 > -sup.y.0,
        "box top {} vs sup lift {}",
        math.baseline.0,
        -sup.y.0
    );
}

#[test]
fn subscript_sits_below_the_baseline() {
    let layout = math_layout("a_1");
    let math = layout.pages[0].blocks[0].math.as_ref().expect("math box");
    let sub = math
        .items
        .iter()
        .find(|item| item.text == "1")
        .expect("subscript");

    assert!(sub.y.0 > 0, "subscript drops below the baseline");
    assert!(sub.size.0 < Width::EM.0);
    assert!(
        math.height.0 > math.baseline.0,
        "the depth below the baseline is part of the box"
    );
}

#[test]
fn fraction_draws_a_rule_between_its_parts() {
    let layout = math_layout("\\frac{a}{b}");
    let math = layout.pages[0].blocks[0].math.as_ref().expect("math box");

    assert_eq!(math.rules.len(), 1, "one fraction bar");
    let rule = &math.rules[0];
    assert!(rule.width.0 > 0 && rule.thickness.0 > 0);

    let numerator = math
        .items
        .iter()
        .find(|item| item.text == "a")
        .expect("numerator");
    let denominator = math
        .items
        .iter()
        .find(|item| item.text == "b")
        .expect("denominator");

    assert!(
        numerator.y.0 < rule.y.0,
        "numerator {} above rule {}",
        numerator.y.0,
        rule.y.0
    );
    assert!(
        denominator.y.0 > rule.y.0,
        "denominator {} below rule {}",
        denominator.y.0,
        rule.y.0
    );
}

#[test]
fn math_keeps_the_paragraph_flow_intact() {
    let doc = document(vec![
        paragraph("中文段落一。"),
        Block::MathBlock(MathBlock::new(NodeMeta::synthesized(), "E = mc^2")),
        paragraph("中文段落二。"),
    ]);
    let styled = styled(&doc, TIGHT);
    let layout = layout(&doc, &styled, &metrics(), &engine()).expect("layout");
    let kinds: Vec<BoxKind> = layout.pages[0]
        .blocks
        .iter()
        .map(|block| block.kind)
        .collect();

    assert_eq!(
        kinds,
        vec![BoxKind::Paragraph, BoxKind::Math, BoxKind::Paragraph]
    );
    let math = &layout.pages[0].blocks[1];
    assert!(math.lines.is_empty(), "a formula has no text lines");
    assert_eq!(math.lines_total, 0);
}

#[test]
fn canonical_json_carries_the_formula() {
    let layout = math_layout("\\frac{a}{b}");
    let json = layout.canonical_json();

    assert!(json.contains("\"schema\":\"soma.pages/11\""));
    assert!(json.contains("\"math\":{\"width\":"));
    assert!(json.contains("\"inlineMath\":[]"));
    assert!(json.contains("\"baseline\":"));
    assert!(json.contains("\"text\":\"a\""));
    assert!(json.contains("\"rules\":[{\"x\":"));
    assert!(json.contains("\"thickness\":"));
}

#[test]
fn a_block_without_math_serializes_as_null() {
    let doc = document(vec![paragraph("普通段落。")]);
    let styled = styled(&doc, TIGHT);
    let layout = layout(&doc, &styled, &metrics(), &engine()).expect("layout");

    assert!(layout.canonical_json().contains("\"math\":null"));
}

#[test]
fn math_height_does_not_change_the_line_count() {
    let plain = document(vec![paragraph("甲。乙。丙。")]);
    let with_math = document(vec![
        paragraph("甲。乙。丙。"),
        Block::MathBlock(MathBlock::new(NodeMeta::synthesized(), "a+b")),
    ]);
    let plain_styled = styled(&plain, TIGHT);
    let math_styled = styled(&with_math, TIGHT);
    let plain_layout = layout(&plain, &plain_styled, &metrics(), &engine()).expect("layout");
    let math_layout = layout(&with_math, &math_styled, &metrics(), &engine()).expect("layout");

    let plain_lines: u32 = plain_layout.pages[0]
        .blocks
        .iter()
        .map(|block| block.lines_total)
        .sum();
    let math_lines: u32 = math_layout.pages[0]
        .blocks
        .iter()
        .filter(|block| block.kind != BoxKind::Math)
        .map(|block| block.lines_total)
        .sum();
    assert_eq!(plain_lines, math_lines);
}

#[test]
fn a_real_math_face_selects_design_variants() {
    use soma_layout::StyledMetrics;
    use soma_metrics::MetricsTable;

    let doc = document(vec![Block::MathBlock(MathBlock::new(
        NodeMeta::synthesized(),
        r"\sum_{i=1}^{n} a_i + \sqrt{\frac{a}{b}}",
    ))]);
    let styled = styled(&doc, TIGHT);
    let table = MetricsTable::embedded();
    let metrics = StyledMetrics::new(&table, &styled);
    let layout = layout(&doc, &styled, &metrics, &engine()).expect("layout");
    let math = layout.pages[0].blocks[0].math.as_ref().expect("math box");

    let sum = math
        .items
        .iter()
        .find(|item| item.text == "∑")
        .expect("summation");
    assert!(
        sum.variant().is_some(),
        "the embedded Noto MATH table has a display sum: {sum:?}"
    );
    assert_eq!(
        sum.design,
        styled.style(soma_core::StyleId(0)).unwrap().font_size,
        "a variant is drawn at the formula's design size"
    );
    assert!(sum.size.0 > sum.design.0, "the text fallback is larger");

    let radical = math
        .items
        .iter()
        .find(|item| item.text == "√")
        .expect("radical");
    assert!(radical.variant().is_some(), "a tall radicand has a variant");

    let json = layout.canonical_json();
    assert!(
        json.contains("\"design\":"),
        "the page result names the size"
    );
    assert!(json.contains("\"glyph\":1") || json.contains("\"glyph\":2"));
}
