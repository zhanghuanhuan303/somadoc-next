//! D56: `footnote.numbering: section` restarts the counter at each section.

mod common;

use common::{document, engine, metrics, paragraph, paragraph_with, styled, text};
use soma_doc::block::{PageBreak, Section};
use soma_doc::inline::FootnoteRef;
use soma_doc::node::NodeMeta;
use soma_doc::{Block, Document, FootnoteDef, Inline};
use soma_layout::{layout, PagedLayout};

const PAGE: &str = r#"
name: sections
page:
  size: custom
  width: 120mm
  height: 140mm
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
  - selector:
      element: footnote
    declarations:
      font-size: 8pt
"#;

fn source(policy: &str) -> String {
    PAGE.replace(
        "name: sections",
        &format!("name: sections\nfootnote:\n  numbering: {policy}"),
    )
}

fn note_ref(label: &str) -> Inline {
    FootnoteRef::new(NodeMeta::synthesized(), label).into()
}

fn section() -> Block {
    Block::Section(Section::new(NodeMeta::synthesized(), 1))
}

fn note(label: &str) -> FootnoteDef {
    FootnoteDef::new(
        NodeMeta::synthesized(),
        label,
        vec![paragraph(&format!("脚注{label}。"))],
    )
}

fn push_notes(doc: &mut Document) {
    for label in ["a", "b", "c", "d"] {
        doc.push_footnote(note(label));
    }
    doc.renumber();
}

fn two_sections_same_page() -> Document {
    let mut doc = document(vec![
        paragraph_with(vec![text("第一节甲"), note_ref("a"), text("。")]),
        paragraph_with(vec![text("第一节乙"), note_ref("b"), text("。")]),
        section(),
        paragraph_with(vec![text("第二节丙"), note_ref("c"), text("。")]),
        paragraph_with(vec![text("第二节丁"), note_ref("d"), text("。")]),
    ]);
    push_notes(&mut doc);
    doc
}

fn two_sections_across_pages() -> Document {
    let mut doc = document(vec![
        paragraph_with(vec![text("第一节甲"), note_ref("a"), text("。")]),
        paragraph_with(vec![text("第一节乙"), note_ref("b"), text("。")]),
        Block::PageBreak(PageBreak::new(NodeMeta::synthesized())),
        section(),
        paragraph_with(vec![text("第二节丙"), note_ref("c"), text("。")]),
        paragraph_with(vec![text("第二节丁"), note_ref("d"), text("。")]),
    ]);
    push_notes(&mut doc);
    doc
}

fn mark_numbers(layout: &PagedLayout) -> Vec<u32> {
    layout
        .pages
        .iter()
        .flat_map(|page| page.blocks.iter())
        .flat_map(|block| block.marks.iter())
        .map(|mark| mark.number)
        .collect()
}

fn page_mark_numbers(layout: &PagedLayout, page: usize) -> Vec<u32> {
    layout.pages[page]
        .blocks
        .iter()
        .flat_map(|block| block.marks.iter())
        .map(|mark| mark.number)
        .collect()
}

#[test]
fn section_numbering_restarts_within_a_page() {
    let doc = two_sections_same_page();
    let styled = styled(&doc, &source("section"));
    let layout = layout(&doc, &styled, &metrics(), &engine()).expect("layout");

    assert_eq!(
        layout.page_count(),
        1,
        "the fixture must keep both sections on one page"
    );
    assert_eq!(mark_numbers(&layout), [1, 2, 1, 2]);
    assert_eq!(
        layout.pages[0].footnote_numbers,
        vec![Some(1), Some(2), Some(1), Some(2)],
        "the note area follows the section-local labels"
    );
}

#[test]
fn section_numbering_restarts_across_pages() {
    let doc = two_sections_across_pages();
    let styled = styled(&doc, &source("section"));
    let layout = layout(&doc, &styled, &metrics(), &engine()).expect("layout");

    assert_eq!(layout.page_count(), 2);
    assert_eq!(page_mark_numbers(&layout, 0), [1, 2]);
    assert_eq!(page_mark_numbers(&layout, 1), [1, 2]);
    assert_eq!(
        layout.pages[1].footnote_numbers,
        vec![Some(1), Some(2)],
        "a section restart does not inherit the previous page's last number"
    );
}

#[test]
fn document_numbering_ignores_sections() {
    let doc = two_sections_same_page();
    let styled = styled(&doc, &source("document"));
    let layout = layout(&doc, &styled, &metrics(), &engine()).expect("layout");

    assert_eq!(mark_numbers(&layout), [1, 2, 3, 4]);
    assert_eq!(
        layout.pages[0].footnote_numbers,
        vec![Some(1), Some(2), Some(3), Some(4)]
    );
}

#[test]
fn canonical_json_carries_the_section_local_numbers() {
    let doc = two_sections_same_page();
    let styled = styled(&doc, &source("section"));
    let layout = layout(&doc, &styled, &metrics(), &engine()).expect("layout");
    let json = layout.canonical_json();

    for block in &layout.pages[0].blocks {
        for mark in &block.marks {
            let serialized = format!(
                "{{\"atom\":{},\"number\":{},\"line\":{}}}",
                mark.atom, mark.number, mark.line
            );
            assert!(
                json.contains(&serialized),
                "canonical output must expose the number actually printed: {serialized}"
            );
        }
    }
    assert!(
        json.contains(&format!(
            "{{\"atom\":{},\"number\":1,\"line\":{}}}",
            layout.pages[0].blocks[2].marks[0].atom, layout.pages[0].blocks[2].marks[0].line
        )),
        "the second section's first marker is numbered 1 in the canonical result"
    );
}

#[test]
fn a_repeated_reference_keeps_the_note_area_in_step() {
    let mut doc = document(vec![paragraph_with(vec![
        text("正文"),
        note_ref("a"),
        text("重引"),
        note_ref("a"),
        text("与"),
        note_ref("b"),
        text("。"),
    ])]);
    for label in ["a", "b"] {
        doc.push_footnote(note(label));
    }
    doc.renumber();

    let styled = styled(&doc, &source("document"));
    let layout = layout(&doc, &styled, &metrics(), &engine()).expect("layout");
    let page = &layout.pages[0];

    assert_eq!(mark_numbers(&layout), [1, 2, 3]);
    assert_eq!(
        page.footnote_numbers,
        vec![Some(1), Some(3)],
        "the note area prints the first visible reference's number, not its slot"
    );
}
