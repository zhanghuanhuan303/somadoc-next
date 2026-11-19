//! E12: inline math participates in line breaking as an object atom.
//!
//! The atom text is a private-use marker; the formula geometry travels in
//! `PlacedBlock.inline_math`. These tests lock the three things the rest of the
//! engine depends on: one atom per formula, a line box that grows to fit, and a
//! placeholder when the source does not parse.

mod common;

use common::{document, engine, metrics, paragraph, paragraph_with, styled, text, TIGHT};
use soma_core::Width;
use soma_doc::block::MathBlock;
use soma_doc::inline::MathInline;
use soma_doc::node::NodeMeta;
use soma_doc::{Block, Inline};
use soma_layout::{layout, math::MARKER, BoxKind, PagedLayout, TextSource};

fn math(source: &str) -> Inline {
    Inline::Math(MathInline::new(NodeMeta::synthesized(), source))
}

fn layout_of(blocks: Vec<Block>) -> PagedLayout {
    let doc = document(blocks);
    let styled = styled(&doc, TIGHT);
    layout(&doc, &styled, &metrics(), &engine()).expect("layout")
}

#[test]
fn an_inline_formula_becomes_one_object_atom() {
    let layout = layout_of(vec![paragraph_with(vec![
        text("设"),
        math("x^2"),
        text("为单位。"),
    ])]);
    let block = &layout.pages[0].blocks[0];

    assert_eq!(block.inline_math.len(), 1, "one box per formula");
    let inline = &block.inline_math[0];
    assert!(inline.math.width.0 > 0 && inline.math.height.0 > 0);
    assert_eq!(inline.source, "x^2");

    let atom = layout.text.atom_text(block.node, inline.atom);
    assert!(
        atom.starts_with(MARKER),
        "the atom text is a marker, not the source: {atom:?}"
    );
    assert!(
        !atom.contains("x^2"),
        "the formula source must not be drawn as text"
    );
}

#[test]
fn surrounding_atoms_keep_their_order() {
    let layout = layout_of(vec![paragraph_with(vec![
        text("甲"),
        math("a+b"),
        text("乙"),
    ])]);
    let block = &layout.pages[0].blocks[0];
    let line = &block.lines[0].line;
    let boxes = &line.boxes;

    assert_eq!(boxes.len(), 3, "three atoms on the line");
    let formula = block.inline_math[0].atom as usize;
    assert_eq!(formula, 1, "the formula sits between the two characters");
    assert!(boxes[0].x.0 < boxes[formula].x.0);
    assert!(boxes[formula].x.0 < boxes[2].x.0);
    assert!(
        boxes[2].x.0 - boxes[formula].x.0 >= block.inline_math[0].math.width.0,
        "the next atom starts after the formula box"
    );
}

#[test]
fn a_tall_formula_grows_the_line_box() {
    let plain = layout_of(vec![paragraph("正文")]);
    let tall = layout_of(vec![paragraph_with(vec![
        text("正文"),
        math("\\frac{\\frac{a}{b}}{\\frac{c}{d}}"),
    ])]);

    let plain_height = plain.pages[0].blocks[0].lines[0].line.height;
    let tall_line = &tall.pages[0].blocks[0].lines[0].line;
    let formula = &tall.pages[0].blocks[0].inline_math[0].math;

    assert!(
        tall_line.ascent >= formula.baseline,
        "the line ascent covers the formula ascent ({} >= {})",
        tall_line.ascent.0,
        formula.baseline.0
    );
    assert!(
        tall_line.height > plain_height,
        "a 2-em formula must raise the line box ({} > {})",
        tall_line.height.0,
        plain_height.0
    );
}

#[test]
fn inline_style_does_not_stack_sum_limits() {
    let inline_doc = layout_of(vec![paragraph_with(vec![math("\\sum_{i=1}^{n} i")])]);
    let display_doc = layout_of(vec![Block::MathBlock(MathBlock::new(
        NodeMeta::synthesized(),
        "\\sum_{i=1}^{n} i",
    ))]);

    let inline = &inline_doc.pages[0].blocks[0].inline_math[0].math;
    let display = display_doc.pages[0].blocks[0]
        .math
        .as_ref()
        .expect("display box");

    assert!(
        inline.height.0 < display.height.0,
        "text-style limits stay to the right: {} < {}",
        inline.height.0,
        display.height.0
    );
    assert!(
        inline.width.0 > display.width.0,
        "so the inline formula is wider: {} > {}",
        inline.width.0,
        display.width.0
    );
}

#[test]
fn a_malformed_inline_formula_keeps_its_source() {
    let layout = layout_of(vec![paragraph_with(vec![
        text("见"),
        math("\\frac{1}"),
        text("处。"),
    ])]);
    let block = &layout.pages[0].blocks[0];

    assert!(block.inline_math.is_empty(), "no box for a broken formula");
    let text_of_line: String = block
        .lines
        .iter()
        .flat_map(|placed| placed.line.boxes.iter())
        .map(|atom| layout.text.atom_text(block.node, atom.atom))
        .collect();
    assert!(
        text_of_line.contains("\\frac{1}"),
        "the source stays visible: {text_of_line:?}"
    );
}

#[test]
fn multiple_formulas_keep_distinct_markers() {
    let layout = layout_of(vec![paragraph_with(vec![
        math("x"),
        text("与"),
        math("y"),
        text("不同。"),
    ])]);
    let block = &layout.pages[0].blocks[0];

    assert_eq!(block.inline_math.len(), 2);
    assert_ne!(block.inline_math[0].atom, block.inline_math[1].atom);
    assert_ne!(
        layout.text.atom_text(block.node, block.inline_math[0].atom),
        layout.text.atom_text(block.node, block.inline_math[1].atom)
    );
}

#[test]
fn an_inline_formula_inside_emphasis_still_lays_out() {
    let emphasis = Inline::Emphasis(soma_doc::inline::Container::new(
        NodeMeta::synthesized(),
        vec![text("强调"), math("a+b")],
    ));
    let layout = layout_of(vec![paragraph_with(vec![emphasis])]);
    let block = &layout.pages[0].blocks[0];

    assert_eq!(block.inline_math.len(), 1);
    assert!(block.inline_math[0].math.width.0 > 0);
}

#[test]
fn canonical_json_carries_inline_math() {
    let layout = layout_of(vec![paragraph_with(vec![text("甲"), math("x^2")])]);
    let json = layout.canonical_json();

    assert!(json.contains("\"schema\":\"soma.pages/6\""));
    assert!(json.contains("\"inlineMath\":[{\"atom\":"));
    assert!(json.contains("\"source\":\"x^2\""));
    assert!(json.contains("\"math\":{\"width\":"));
    // A block without formulas still writes the empty array.
    let plain = layout_of(vec![paragraph("普通段落。")]);
    assert!(plain.canonical_json().contains("\"inlineMath\":[]"));
}

#[test]
fn a_formula_wider_than_the_line_still_breaks() {
    let mut inlines = vec![text(&"前".repeat(20))];
    inlines.push(math("\\frac{a+b+c+d+e+f}{g+h+i+j+k+l+m}"));
    inlines.push(text(&"后".repeat(20)));
    let layout = layout_of(vec![paragraph_with(inlines)]);
    let block = &layout.pages[0].blocks[0];

    assert!(
        block.lines.len() > 1,
        "a 3-em formula cannot follow 20em of text on a 25em line"
    );
    let formula = block.inline_math[0].atom;
    let lines_with_formula = block
        .lines
        .iter()
        .filter(|placed| placed.line.boxes.iter().any(|atom| atom.atom == formula))
        .count();
    assert_eq!(lines_with_formula, 1, "an object atom is never split");

    let line = block
        .lines
        .iter()
        .find(|placed| placed.line.boxes.iter().any(|atom| atom.atom == formula))
        .expect("the formula line");
    assert!(
        line.line.natural_width.0 >= block.inline_math[0].math.width.0,
        "the line is at least as wide as the formula"
    );
}

#[test]
fn math_blocks_stay_separate_from_inline_math() {
    let layout = layout_of(vec![
        paragraph_with(vec![math("x")]),
        Block::MathBlock(MathBlock::new(NodeMeta::synthesized(), "y")),
    ]);

    let blocks = &layout.pages[0].blocks;
    assert_eq!(blocks.len(), 2);
    assert_eq!(blocks[0].kind, BoxKind::Paragraph);
    assert_eq!(blocks[0].inline_math.len(), 1);
    assert!(blocks[0].math.is_none());
    assert_eq!(blocks[1].kind, BoxKind::Math);
    assert!(blocks[1].inline_math.is_empty());
    assert!(blocks[1].math.is_some());
}

#[test]
fn an_empty_paragraph_of_only_math_is_still_a_paragraph() {
    let layout = layout_of(vec![paragraph_with(vec![math("x^2")])]);
    let block = &layout.pages[0].blocks[0];

    assert_eq!(block.kind, BoxKind::Paragraph);
    assert_eq!(block.inline_math.len(), 1);
    assert_eq!(block.lines_total, 1);
    assert!(block.height.0 > Width::ZERO.0);
}
