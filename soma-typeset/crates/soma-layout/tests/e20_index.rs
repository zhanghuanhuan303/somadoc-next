//! Index: `\index{term}` markers become a sorted, paginated index at the tail.

mod common;

use common::{document, engine, metrics, paragraph_with, styled, text};
use soma_doc::block::PageBreak;
use soma_doc::node::NodeMeta;
use soma_doc::{Block, IndexRef};
use soma_layout::{layout, PagedLayout, TextSource};
use soma_template::StyledDoc;

const TEMPLATE: &str = r#"
name: index
page:
  size: custom
  width: 160mm
  height: 90mm
  margin: 10mm
index:
  title: 索引
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
  - selector:
      element: indexEntry
    declarations:
      font-size: 9pt
      line-height: 1.8
  - selector:
      element: indexTitle
    declarations:
      font-size: 12pt
      space-after: 0.6em
"#;

fn marker(term: &str) -> soma_doc::Inline {
    IndexRef::new(NodeMeta::synthesized(), term).into()
}

fn build(blocks: Vec<Block>, source: &str) -> (PagedLayout, StyledDoc) {
    let doc = document(blocks);
    let styled = styled(&doc, source);
    let layout = layout(&doc, &styled, &metrics(), &engine()).expect("layout");
    (layout, styled)
}

fn body() -> Vec<Block> {
    vec![
        paragraph_with(vec![text("第一页提到排版。"), marker("排版")]),
        Block::PageBreak(PageBreak::new(NodeMeta::synthesized())),
        paragraph_with(vec![
            text("第二页再提排版与引擎。"),
            marker("排版"),
            marker("引擎"),
        ]),
    ]
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

fn page_text(layout: &PagedLayout, index: usize) -> String {
    layout.pages[index]
        .blocks
        .iter()
        .map(|block| block_text(layout, block))
        .collect::<Vec<_>>()
        .join(" ")
}

#[test]
fn an_index_is_sorted_and_lists_unique_pages() {
    let (layout, _styled) = build(body(), TEMPLATE);
    let report = layout.index.as_ref().expect("index report");

    assert_eq!(report.entries, 2);
    assert!(report.stable, "the entries converge");
    assert_eq!(layout.page_count(), 3, "two body pages + one index page");

    let index_page = page_text(&layout, 2);
    assert!(index_page.starts_with("索引"), "{index_page}");
    let engine = index_page.find("引擎").expect("term 引擎");
    let typeset = index_page.find("排版").expect("term 排版");
    assert!(engine < typeset, "terms are sorted: {index_page}");
    assert!(
        index_page.contains("1, 2"),
        "排版 lists both pages: {index_page}"
    );
    assert!(index_page.contains('2'), "引擎 is on page 2: {index_page}");
    let json = layout.canonical_json();
    assert!(
        json.contains("\"index\":{\"passes\":2,\"stable\":true,\"entries\":2}"),
        "{json}"
    );
}

#[test]
fn index_entries_use_the_generated_element_style() {
    let (layout, styled) = build(body(), TEMPLATE);
    let entry_style = styled.index_style();
    let entry = layout.pages[2]
        .blocks
        .iter()
        .find(|block| block_text(&layout, block).contains("排版"))
        .expect("index entry");

    assert_eq!(entry.style, entry_style);
    let title = layout.pages[2].blocks.first().expect("index title");
    assert_eq!(title.style, styled.index_title_style());
}

#[test]
fn an_index_starts_on_its_own_page() {
    let (layout, _styled) = build(body(), TEMPLATE);
    let body_page = page_text(&layout, 1);
    assert!(!body_page.contains("索引"), "{body_page}");
    assert!(page_text(&layout, 2).starts_with("索引"));
}

#[test]
fn a_template_without_an_index_section_generates_nothing() {
    let source = TEMPLATE.replace("index:\n  title: 索引\n", "");
    let (layout, _styled) = build(body(), &source);

    assert!(layout.index.is_none());
    assert_eq!(layout.page_count(), 2);
}

#[test]
fn an_index_without_markers_produces_no_page() {
    let source = TEMPLATE;
    let blocks = vec![paragraph_with(vec![text("没有索引项。")])];
    let (layout, _styled) = build(blocks, source);
    let report = layout.index.as_ref().expect("index report");

    assert_eq!(report.entries, 0);
    assert!(report.stable);
    assert_eq!(layout.page_count(), 1, "no empty index page");
}
