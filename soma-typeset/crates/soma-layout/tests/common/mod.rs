//! Shared fixtures for the layout tests.

#![allow(dead_code)]

use soma_core::{BreakConfig, StyleId, SyntheticMetrics};
use soma_doc::block::Paragraph;
use soma_doc::meta::DocumentMeta;
use soma_doc::node::NodeMeta;
use soma_doc::{Block, Document, Inline, Text};
use soma_template::{parse_template, resolve, StyledDoc, Template};

/// Narrow page: about 8 lines of body text per page.
pub const TIGHT: &str = r#"
name: tight
page:
  size: custom
  width: 100mm
  height: 34mm
  margin: 4mm
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
      first-indent: 2em
  - selector:
      element: heading
    declarations:
      font-size: 14pt
      space-before: 1.5em
      keep-with-next: true
  - selector:
      element: footnote
    declarations:
      font-size: 8pt
"#;

/// A4 with normal margins.
pub const BOOK: &str = r#"
name: book
page:
  size: A4
  margin: 20mm
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
      first-indent: 2em
  - selector:
      element: heading
    declarations:
      font-size: 20pt
      space-before: 1.5em
      keep-with-next: true
  - selector:
      element: footnote
    declarations:
      font-size: 8pt
"#;

pub fn template(source: &str) -> Template {
    parse_template(source).expect("fixture template must parse")
}

pub fn styled(doc: &Document, source: &str) -> StyledDoc {
    resolve(doc, &template(source)).expect("styles must resolve")
}

pub fn metrics() -> SyntheticMetrics {
    SyntheticMetrics::default()
}

pub fn engine() -> BreakConfig {
    BreakConfig::default()
}

pub fn text(value: &str) -> Inline {
    Text::new(NodeMeta::synthesized(), value).into()
}

pub fn paragraph(value: &str) -> Block {
    Block::Paragraph(Paragraph::new(NodeMeta::synthesized(), vec![text(value)]))
}

pub fn paragraph_with(inlines: Vec<Inline>) -> Block {
    Block::Paragraph(Paragraph::new(NodeMeta::synthesized(), inlines))
}

pub fn document(blocks: Vec<Block>) -> Document {
    let mut doc = Document::new(DocumentMeta::new().with_language("zh-Hans"));
    doc.blocks = blocks;
    doc.renumber();
    doc
}

pub fn style_of(styled: &StyledDoc, node: soma_doc::NodeId) -> StyleId {
    styled.style_of(node).unwrap_or(StyleId(0))
}
