//! D76: front-matter numbering. The cover and contents may be unnumbered or
//! use roman numerals; the body always restarts at 1.

mod common;

use common::{engine, metrics, paragraph};
use soma_doc::block::Heading;
use soma_doc::node::NodeMeta;
use soma_doc::{Block, Document, Inline, Text};
use soma_layout::{layout, PagedLayout, TextSource};
use soma_template::{parse_template, resolve};

fn text(value: &str) -> Inline {
    Text::new(NodeMeta::synthesized(), value).into()
}

fn heading(level: u8, value: &str) -> Block {
    Block::Heading(Heading::new(
        NodeMeta::synthesized(),
        level,
        vec![text(value)],
    ))
}

fn base(front_matter: &str) -> String {
    format!(
        r#"
name: front
page:
  size: custom
  width: 160mm
  height: 120mm
  margin: 10mm
  footer: 8mm
  folio: "{{page}}/{{pages}}"
  front-matter: {front_matter}
cover:
  master: default
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
  - selector:
      element: heading
    declarations:
      font-size: 14pt
  - selector:
      element: toc
    declarations:
      font-size: 9pt
toc:
  depth: 1
  title: 目录
  leaders: false
"#
    )
}

fn build(front_matter: &str) -> PagedLayout {
    let mut doc = Document::new(soma_doc::meta::DocumentMeta::new().with_language("zh-Hans"));
    doc.meta.title = Some(vec![text("书稿")]);
    doc.blocks = vec![heading(1, "绪论"), paragraph("正文。")];
    doc.renumber();
    let styled = resolve(
        &doc,
        &parse_template(&base(front_matter)).expect("template"),
    )
    .expect("styles");
    layout(&doc, &styled, &metrics(), &engine()).expect("layout")
}

fn block_text(layout: &PagedLayout, block: &soma_layout::PlacedBlock) -> String {
    let mut out = String::new();
    for placed in &block.lines {
        for atom in &placed.line.boxes {
            out.push_str(layout.text.atom_text(block.node, atom.atom));
        }
    }
    out
}

fn footer_text(layout: &PagedLayout, index: usize) -> String {
    layout.pages[index]
        .footer
        .iter()
        .map(|block| block_text(layout, block))
        .collect::<Vec<_>>()
        .join(" ")
}

#[test]
fn continuous_numbering_counts_front_matter() {
    let layout = build("continuous");

    assert!(layout.page_count() >= 3, "cover + contents + body");
    assert_eq!(layout.pages[0].printed, "1");
    assert_eq!(layout.pages[1].printed, "2");
    assert_eq!(layout.pages[2].printed, "3");
    let total = layout.page_count();
    assert_eq!(footer_text(&layout, 0), format!("1/{total}"));
    assert_eq!(footer_text(&layout, 2), format!("3/{total}"));
}

#[test]
fn roman_numbering_labels_front_matter_and_restarts_the_body() {
    let layout = build("roman");

    assert!(layout.page_count() >= 3, "cover + contents + body");
    assert_eq!(layout.pages[0].printed, "i", "the cover is page i");
    assert_eq!(layout.pages[1].printed, "ii", "the contents are page ii");
    assert_eq!(layout.pages[2].printed, "1", "the body restarts at 1");

    // `{pages}` counts body pages only.
    let body = layout.page_count() - 2;
    assert_eq!(footer_text(&layout, 0), "i/1");
    assert_eq!(footer_text(&layout, 2), format!("1/{body}"));
}

#[test]
fn hidden_front_matter_prints_no_folio() {
    let layout = build("hidden");

    assert!(layout.page_count() >= 3, "cover + contents + body");
    assert_eq!(layout.pages[0].printed, "");
    assert_eq!(layout.pages[1].printed, "");
    assert_eq!(layout.pages[2].printed, "1");
    assert_eq!(footer_text(&layout, 0), "", "no folio on the cover");
    assert_eq!(footer_text(&layout, 1), "", "no folio on the contents");
    assert_eq!(footer_text(&layout, 2), "1/1");
}

#[test]
fn a_body_page_can_be_page_one_when_there_is_no_front_matter() {
    let mut doc = Document::new(soma_doc::meta::DocumentMeta::new().with_language("zh-Hans"));
    doc.blocks = vec![heading(1, "绪论"), paragraph("正文。")];
    doc.renumber();
    let source = r#"
name: plain
page:
  size: custom
  width: 160mm
  height: 120mm
  margin: 10mm
  footer: 8mm
  folio: "{page}/{pages}"
  front-matter: roman
styles:
  - selector:
      element: document
    declarations:
      font-size: 10.5pt
      line-height: 1.6
  - selector:
      element: paragraph
    declarations:
      first-indent: 0
"#;
    let styled = resolve(&doc, &parse_template(source).expect("template")).expect("styles");
    let layout = layout(&doc, &styled, &metrics(), &engine()).expect("layout");

    assert_eq!(layout.pages[0].printed, "1");
    assert_eq!(footer_text(&layout, 0), "1/1");
}

#[test]
fn a_continuous_document_without_front_matter_matches_plain_numbers() {
    let mut doc = Document::new(soma_doc::meta::DocumentMeta::new().with_language("zh-Hans"));
    doc.blocks = vec![paragraph("正文。")];
    doc.renumber();
    let source = r#"
name: plain
page:
  size: custom
  width: 160mm
  height: 120mm
  margin: 10mm
styles:
  - selector:
      element: document
    declarations:
      font-size: 10.5pt
      line-height: 1.6
"#;
    let styled = resolve(&doc, &parse_template(source).expect("template")).expect("styles");
    let layout = layout(&doc, &styled, &metrics(), &engine()).expect("layout");

    assert_eq!(layout.pages[0].printed, "1");
}
