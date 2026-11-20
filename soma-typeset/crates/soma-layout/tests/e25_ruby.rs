//! Ruby annotations: the base breaks as one atom, the reading rides above it,
//! and the line grows upward to make room (ADR-0060).

mod common;

use common::{document, engine, metrics, paragraph_with, styled, text};
use soma_doc::inline::Ruby;
use soma_doc::node::NodeMeta;
use soma_layout::{layout, PagedLayout, TextSource};

const TEMPLATE: &str = r#"
name: ruby
page:
  size: custom
  width: 120mm
  height: 80mm
  margin: 10mm
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

fn ruby(base: &str, annotation: &str) -> soma_doc::Inline {
    Ruby::new(NodeMeta::synthesized(), base, annotation).into()
}

fn build(inlines: Vec<soma_doc::Inline>) -> PagedLayout {
    let doc = document(vec![paragraph_with(inlines)]);
    let styled = styled(&doc, TEMPLATE);
    layout(&doc, &styled, &metrics(), &engine()).expect("layout")
}

#[test]
fn the_base_is_one_atom_and_carries_its_reading() {
    let layout = build(vec![text("甲"), ruby("汉字", "hànzì"), text("乙")]);
    let block = &layout.pages[0].blocks[0];

    assert_eq!(layout.text.atom_text(block.node, 1), "汉字");
    assert_eq!(block.ruby.len(), 1);
    assert_eq!(block.ruby[0].atom, 1);
    assert_eq!(block.ruby[0].text, "hànzì");
    // Half the base em, and the shift clears the base ascent.
    assert!(block.ruby[0].size.0 > 0);
    assert!(block.ruby[0].shift.0 > 0);
}

#[test]
fn the_line_grows_upward_for_the_annotation() {
    let plain = build(vec![text("甲"), text("汉"), text("乙")]);
    let annotated = build(vec![text("甲"), ruby("汉", "hàn")]);
    let plain_ascent = plain.pages[0].blocks[0].lines[0].line.ascent.0;
    let annotated_line = &annotated.pages[0].blocks[0].lines[0].line;

    assert!(
        annotated_line.ascent.0 > plain_ascent,
        "ruby reserves space above the base: {} vs {plain_ascent}",
        annotated_line.ascent.0
    );
    assert!(
        annotated_line.height.0
            >= annotated_line.ascent.0 + annotated_line.descent.0 + annotated_line.leading.0
    );
}

#[test]
fn canonical_bytes_name_the_annotation() {
    let layout = build(vec![ruby("汉字", "hànzì")]);
    let json = layout.canonical_json();

    assert!(
        json.contains("\"ruby\":[{\"atom\":0,\"text\":\"hànzì\""),
        "{json}"
    );
}

#[test]
fn a_document_without_ruby_has_no_ruby_key() {
    let layout = build(vec![text("没有注音。")]);
    let json = layout.canonical_json();

    assert!(!json.contains("\"ruby\""), "{json}");
}
