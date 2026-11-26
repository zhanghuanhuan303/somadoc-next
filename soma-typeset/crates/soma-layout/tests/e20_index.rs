//! Index: `\index{term}` markers become a sorted, paginated index at the tail.

mod common;

use common::{document, engine, metrics, paragraph_with, styled, text};
use soma_doc::block::{Cell, Column, PageBreak, Paragraph, Row, Table};
use soma_doc::node::NodeMeta;
use soma_doc::{Block, CellAlign, FootnoteDef, FootnoteRef, IndexRef};
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
      align: center
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
    let line = &title.lines[0].line;
    let expected = (title.width.0 - line.natural_width.0) / 2;
    assert_eq!(
        line.boxes[0].x.0, expected,
        "indexTitle align: center must centre the generated title"
    );
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

/// The index rows on the last page, as `(text, x)`.
fn index_rows(layout: &PagedLayout) -> Vec<(String, i32)> {
    let last = layout.page_count() as usize - 1;
    layout.pages[last]
        .blocks
        .iter()
        .map(|block| (block_text(layout, block), block.x.0))
        .collect()
}

#[test]
fn sub_entries_are_indented_under_their_parent() {
    let blocks = vec![paragraph_with(vec![
        text("正文。"),
        marker("排版"),
        marker("排版!行宽"),
        marker("排版!字面"),
    ])];
    let (layout, _styled) = build(blocks, TEMPLATE);
    let rows = index_rows(&layout);
    let parent = rows
        .iter()
        .find(|(text, _)| text == "排版")
        .expect("parent");
    let width = rows.iter().find(|(text, _)| text == "行宽").expect("行宽");
    let face = rows.iter().find(|(text, _)| text == "字面").expect("字面");

    assert!(parent.1 < width.1, "child is set in: {rows:?}");
    assert!(parent.1 < face.1, "child is set in: {rows:?}");
    // Code-point order puts 字面 before 行宽.
    let face_at = rows.iter().position(|(text, _)| text == "字面");
    let width_at = rows.iter().position(|(text, _)| text == "行宽");
    assert!(face_at < width_at, "siblings are sorted: {rows:?}");
}

#[test]
fn a_sub_entry_whose_parent_is_unmarked_still_gets_its_parent() {
    let blocks = vec![paragraph_with(vec![text("正文。"), marker("排版!行宽")])];
    let (layout, _styled) = build(blocks, TEMPLATE);
    let rows = index_rows(&layout);

    let parent = rows
        .iter()
        .find(|(text, _)| text == "排版")
        .expect("parent");
    let child = rows.iter().find(|(text, _)| text == "行宽").expect("child");
    assert!(parent.1 < child.1, "{rows:?}");
    assert!(
        !parent.0.contains('1'),
        "a bare parent line carries no page number: {rows:?}"
    );
}

#[test]
fn the_template_can_order_entries_by_pinyin() {
    let source = TEMPLATE.replace(
        "index:\n  title: 索引\n",
        "index:\n  title: 索引\n  sort: pinyin\n",
    );
    let blocks = vec![paragraph_with(vec![
        text("正文。"),
        marker("中"),
        marker("八"),
        marker("阿"),
    ])];
    let (layout, _styled) = build(blocks, &source);
    let rows = index_rows(&layout);
    let position = |term: &str| rows.iter().position(|(text, _)| text == term).expect(term);

    assert!(position("阿") < position("八"), "{rows:?}");
    assert!(position("八") < position("中"), "{rows:?}");
}

#[test]
fn a_see_entry_prints_its_target_instead_of_pages() {
    let blocks = vec![paragraph_with(vec![
        text("正文。"),
        marker("排版|see{汉字}"),
    ])];
    let (layout, _styled) = build(blocks, TEMPLATE);
    let last = layout.page_count() as usize - 1;
    let page = page_text(&layout, last);

    assert!(page.contains("见 汉字"), "{page}");
}

#[test]
fn consecutive_pages_become_a_range() {
    let mut blocks = Vec::new();
    for page in 0..4 {
        blocks.push(paragraph_with(vec![text("正文。"), marker("排版")]));
        if page < 3 {
            blocks.push(Block::PageBreak(PageBreak::new(NodeMeta::synthesized())));
        }
    }
    let (layout, _styled) = build(blocks, TEMPLATE);
    let index_page = page_text(&layout, layout.page_count() as usize - 1);

    assert!(
        index_page.contains("1–4"),
        "four pages collapse: {index_page}"
    );
}

#[test]
fn two_consecutive_pages_stay_a_list() {
    let blocks = vec![
        paragraph_with(vec![text("正文。"), marker("排版")]),
        Block::PageBreak(PageBreak::new(NodeMeta::synthesized())),
        paragraph_with(vec![text("续。"), marker("排版")]),
    ];
    let (layout, _styled) = build(blocks, TEMPLATE);
    let index_page = page_text(&layout, layout.page_count() as usize - 1);

    assert!(index_page.contains("1, 2"), "{index_page}");
}

#[test]
fn markers_inside_a_table_cell_are_collected() {
    let paragraph = Paragraph::new(
        NodeMeta::synthesized(),
        vec![text("表格里提到排版"), marker("排版")],
    );
    let cell = Cell::new(NodeMeta::synthesized(), vec![Block::Paragraph(paragraph)]);
    let table = Table::new(
        NodeMeta::synthesized(),
        vec![Column::new(NodeMeta::synthesized(), CellAlign::Default)],
        vec![Row::new(NodeMeta::synthesized(), false, vec![cell])],
    );
    let (layout, _styled) = build(vec![Block::Table(table)], TEMPLATE);
    let report = layout.index.as_ref().expect("index report");

    assert_eq!(report.entries, 1, "the cell marker reached the index");
    let index_page = page_text(&layout, layout.page_count() as usize - 1);
    assert!(
        index_page.contains("排版") && index_page.contains('1'),
        "{index_page}"
    );
}

#[test]
fn markers_inside_a_footnote_page_with_the_reference() {
    let mut doc = document(vec![
        paragraph_with(vec![
            text("正文。"),
            FootnoteRef::new(NodeMeta::synthesized(), "1").into(),
        ]),
        Block::PageBreak(PageBreak::new(NodeMeta::synthesized())),
        paragraph_with(vec![text("末页。")]),
    ]);
    doc.push_footnote(FootnoteDef::new(
        NodeMeta::synthesized(),
        "1",
        vec![Block::Paragraph(Paragraph::new(
            NodeMeta::synthesized(),
            vec![text("注里提到引擎。"), marker("引擎")],
        ))],
    ));
    doc.renumber();
    let styled = styled(&doc, TEMPLATE);
    let layout = layout(&doc, &styled, &metrics(), &engine()).expect("layout");

    assert_eq!(layout.index.as_ref().expect("index report").entries, 1);
    let index_page = page_text(&layout, layout.page_count() as usize - 1);
    assert!(
        index_page.contains("引擎") && index_page.contains('1'),
        "the note's marker is on the page of its reference: {index_page}"
    );
}
