//! Bookmarks: every heading becomes an outline entry with its page label.

mod common;

use common::{document, engine, metrics, paragraph, styled};
use soma_doc::block::{Heading, PageBreak};
use soma_doc::node::NodeMeta;
use soma_doc::{Block, Inline, Text};
use soma_layout::{layout, PagedLayout};

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

const TEMPLATE: &str = r#"
name: outline
page:
  size: custom
  width: 160mm
  height: 60mm
  margin: 8mm
  folio: "{page}"
  footer: 6mm
  front-matter: roman
styles:
  - selector:
      element: document
    declarations:
      font-size: 10.5pt
      line-height: 1.4
  - selector:
      element: paragraph
    declarations:
      first-indent: 0
  - selector:
      element: heading
    declarations:
      font-size: 14pt
"#;

fn build(blocks: Vec<Block>) -> PagedLayout {
    let doc = document(blocks);
    let styled = styled(&doc, TEMPLATE);
    layout(&doc, &styled, &metrics(), &engine()).expect("layout")
}

#[test]
fn every_heading_becomes_a_bookmark() {
    let prose = |count: usize| paragraph(&"正文。".repeat(count));
    let layout = build(vec![
        heading(1, "绪论"),
        heading(2, "背景"),
        heading(3, "问题"),
        prose(40),
        Block::PageBreak(PageBreak::new(NodeMeta::synthesized())),
        heading(1, "结论"),
        prose(40),
    ]);
    let outline = &layout.outline;
    assert_eq!(outline.len(), 4);
    assert_eq!(outline[0].level, 1);
    assert_eq!(outline[0].text, "绪论");
    assert_eq!(outline[1].level, 2);
    assert_eq!(outline[2].level, 3);
    assert_eq!(outline[3].text, "结论");
    assert!(
        outline[3].page > outline[0].page,
        "the last heading is on a later page"
    );
    let json = layout.canonical_json();
    assert!(
        json.contains("\"outline\":[{\"level\":1,\"text\":\"绪论\""),
        "{json}"
    );
    for entry in outline {
        assert_eq!(
            entry.printed,
            layout.pages[entry.page as usize - 1].printed,
            "printed label matches the page"
        );
    }
}

#[test]
fn bookmarks_use_printed_labels_from_front_matter() {
    let mut doc = document(vec![heading(1, "第一章"), paragraph(&"正文。".repeat(80))]);
    doc.meta.title = Some(vec![text("书稿")]);
    let styled = styled(
        &doc,
        &TEMPLATE.replace(
            "front-matter: roman",
            "front-matter: roman\ncover:\n  master: default",
        ),
    );
    let layout = layout(&doc, &styled, &metrics(), &engine()).expect("layout");

    assert_eq!(layout.outline[0].page, 2, "the cover owns page 1");
    assert_eq!(layout.outline[0].printed, "1", "the body restarts at 1");
}

#[test]
fn an_empty_document_has_no_bookmarks() {
    let layout = build(vec![paragraph("只有正文。")]);
    assert!(layout.outline.is_empty());
}
