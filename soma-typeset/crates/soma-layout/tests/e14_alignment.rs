//! E14: left/center/right alignment.
//!
//! The kernel only justifies or flushes left; centering and right-alignment are
//! applied to the finished lines in the layout layer, so the frozen DP and cost
//! model never see them.

mod common;

use common::{engine, metrics, paragraph_with, styled, text};
use soma_core::Width;
use soma_doc::meta::DocumentMeta;
use soma_doc::{Block, Document};
use soma_layout::{layout, PagedLayout, TextSource};

const BASE: &str = r#"
name: align
page:
  size: custom
  width: 100mm
  height: 60mm
  margin: 8mm
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
      first-indent: 0
"#;

fn template(extra: &str) -> String {
    format!("{BASE}{extra}")
}

fn build_in(blocks: Vec<Block>, template: &str) -> PagedLayout {
    let mut doc = Document::new(DocumentMeta::new().with_language("zh-Hans"));
    doc.blocks = blocks;
    doc.renumber();
    let styled = styled(&doc, template);
    layout(&doc, &styled, &metrics(), &engine()).expect("layout")
}

fn build(blocks: Vec<Block>, extra: &str) -> PagedLayout {
    build_in(blocks, &template(extra))
}

#[test]
fn center_alignment_centres_each_line() {
    let layout = build(
        vec![paragraph_with(vec![text("中文")])],
        "  - selector:\n      element: paragraph\n    declarations:\n      align: center\n",
    );
    let block = &layout.pages[0].blocks[0];
    let line = &block.lines[0].line;
    let width = block.width.0;
    let natural = line.natural_width.0;

    assert_eq!(line.boxes.len(), 2);
    let expected = (width - natural) / 2;
    assert_eq!(
        line.boxes[0].x.0, expected,
        "the first atom starts at half the slack"
    );
    assert_eq!(line.adjustment, 0, "a centered line is not stretched");
    let end = line.boxes.last().unwrap().x.0 + line.boxes.last().unwrap().advance.0;
    assert_eq!(end, expected + natural);
}

#[test]
fn right_alignment_flushes_the_line() {
    let layout = build(
        vec![paragraph_with(vec![text("中文")])],
        "  - selector:\n      element: paragraph\n    declarations:\n      align: right\n",
    );
    let block = &layout.pages[0].blocks[0];
    let line = &block.lines[0].line;
    let last = line.boxes.last().unwrap();

    assert_eq!(
        last.x.0 + last.advance.0,
        block.width.0,
        "the line ends at the right edge"
    );
    assert_eq!(line.boxes[0].x.0, block.width.0 - line.natural_width.0);
}

#[test]
fn left_alignment_stays_ragged_and_unstretched() {
    let layout = build(
        vec![paragraph_with(vec![text("中文")])],
        "  - selector:\n      element: paragraph\n    declarations:\n      align: left\n",
    );
    let block = &layout.pages[0].blocks[0];
    let line = &block.lines[0].line;

    assert_eq!(line.boxes[0].x, Width::ZERO);
    assert_eq!(line.adjustment, 0);
}

#[test]
fn justify_still_stretches_the_non_last_line() {
    let layout = build(vec![paragraph_with(vec![text(&"中文排版".repeat(40))])], "");
    let block = &layout.pages[0].blocks[0];
    let first = &block.lines[0].line;

    assert!(block.lines.len() > 1, "the paragraph wraps");
    assert_ne!(first.adjustment, 0, "justification distributes the slack");
}

#[test]
fn centering_ignores_the_first_line_indent() {
    let layout = build(
        vec![paragraph_with(vec![text("中文")])],
        "  - selector:\n      element: paragraph\n    declarations:\n      align: center\n      first-indent: 2em\n",
    );
    let block = &layout.pages[0].blocks[0];
    let line = &block.lines[0].line;

    assert_eq!(line.indent, Width::from_em(2), "the indent is recorded");
    assert_eq!(
        line.boxes[0].x.0,
        (block.width.0 - line.natural_width.0) / 2,
        "centering uses the full box, not the indented one"
    );
}

#[test]
fn a_centered_heading_can_be_reused_for_titles() {
    let mut heading = soma_doc::block::Heading::new(
        soma_doc::node::NodeMeta::synthesized(),
        1,
        vec![text("居中标题")],
    );
    heading.level = 1;
    let layout = build(
        vec![Block::Heading(heading)],
        "  - selector:\n      element: heading\n    declarations:\n      align: center\n",
    );
    let block = &layout.pages[0].blocks[0];
    let line = &block.lines[0].line;

    assert_eq!(
        line.boxes[0].x.0,
        (block.width.0 - line.natural_width.0) / 2
    );
    // The text index still works for the renderer.
    assert_eq!(layout.text.atom_text(block.node, 0), "居");
}
