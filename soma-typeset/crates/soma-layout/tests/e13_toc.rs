//! E13: table of contents (template-driven, page numbers from layout) and the
//! section-start master.

mod common;

use common::{document, engine, metrics, paragraph, styled};
use soma_doc::block::{Heading, PageBreak, Section};
use soma_doc::node::NodeMeta;
use soma_doc::{Block, Document, Inline, Text};
use soma_layout::{layout, BoxKind, PagedLayout, TextSource};
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

fn section(level: u8) -> Block {
    Block::Section(Section::new(NodeMeta::synthesized(), level))
}

const BASE: &str = r#"
name: toc
page:
  size: custom
  width: 100mm
  height: 50mm
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
      element: heading
    declarations:
      space-before: 0.5em
  - selector:
      element: toc
    declarations:
      font-size: 9pt
      line-height: 1.8
  - selector:
      element: tocTitle
    declarations:
      font-size: 12pt
      space-after: 0.6em
"#;

fn with_toc(extra: &str) -> String {
    format!("{BASE}{extra}")
}

fn build(blocks: Vec<Block>, template: &str) -> PagedLayout {
    let mut doc = Document::new(soma_doc::meta::DocumentMeta::new().with_language("zh-Hans"));
    doc.blocks = blocks;
    doc.renumber();
    let styled = resolve(&doc, &parse_template(template).expect("template")).expect("styles");
    layout(&doc, &styled, &metrics(), &engine()).expect("layout")
}

/// Text of one block's lines.
fn block_text(layout: &PagedLayout, block: &soma_layout::PlacedBlock) -> String {
    let mut out = String::new();
    for placed in &block.lines {
        for atom in &placed.line.boxes {
            out.push_str(layout.text.atom_text(block.node, atom.atom));
        }
    }
    out
}

/// All visible text on a page, in block order.
fn page_text(layout: &PagedLayout, index: usize) -> String {
    layout.pages[index]
        .blocks
        .iter()
        .map(|block| block_text(layout, block))
        .collect::<Vec<_>>()
        .join(" ")
}

#[test]
fn a_toc_lists_headings_with_page_numbers() {
    let blocks = vec![
        section(1),
        heading(1, "第一章"),
        paragraph(&"正文。".repeat(40)),
        Block::PageBreak(PageBreak::new(NodeMeta::synthesized())),
        section(1),
        heading(1, "第二章"),
        paragraph(&"正文。".repeat(40)),
    ];
    let layout = build(blocks, &with_toc("toc:\n  depth: 2\n  title: 目录\n"));

    let report = layout.toc.as_ref().expect("toc report");
    assert_eq!(report.entries, 2, "two level-1 headings");
    assert!(report.passes >= 2, "the page numbers need a second pass");
    assert!(report.stable, "the entries converge");
    assert!(layout.page_count() >= 3, "contents page + chapters");

    let first_page = page_text(&layout, 0);
    assert!(first_page.contains("目录"), "the title is on page 1");
    assert!(first_page.contains("第一章"));
    assert!(first_page.contains("第二章"));

    // Page 2 holds the first chapter; the TOC row for it prints "2".
    assert!(page_text(&layout, 1).contains("第一章"));
    let numbers: Vec<String> = layout.pages[0]
        .blocks
        .iter()
        .filter(|block| {
            block.kind == BoxKind::Paragraph && block.width.0 < layout.page.content_width().0
        })
        .map(|block| block_text(&layout, block))
        .collect();
    assert!(
        numbers.iter().any(|number| number == "2"),
        "the first entry points at page 2: {numbers:?}"
    );
}

#[test]
fn toc_depth_filters_heading_levels() {
    let blocks = vec![
        heading(1, "一级"),
        heading(2, "二级"),
        heading(3, "三级"),
        paragraph("正文。"),
    ];
    let shallow = build(blocks.clone(), &with_toc("toc:\n  depth: 1\n"));
    let deep = build(blocks, &with_toc("toc:\n  depth: 3\n"));

    assert_eq!(shallow.toc.as_ref().unwrap().entries, 1);
    assert_eq!(deep.toc.as_ref().unwrap().entries, 3);
    let shallow_text = page_text(&shallow, 0);
    assert!(shallow_text.contains("一级"));
    assert!(!shallow_text.contains("二级"));
}

#[test]
fn each_toc_row_is_one_flow_item_with_title_and_number() {
    let blocks = vec![heading(1, "第一节"), paragraph("正文。")];
    let layout = build(blocks, &with_toc("toc:\n  depth: 2\n  title: 目录\n"));

    // Page 1 holds: TOC title, the entry (title box + number box).
    let rows: Vec<_> = layout.pages[0]
        .blocks
        .iter()
        .filter(|block| block.kind == BoxKind::Paragraph)
        .collect();
    assert_eq!(rows.len(), 3, "title + entry title + entry number");
    let toc_title = &rows[0];
    let entry = &rows[1];
    let number = &rows[2];

    assert_eq!(block_text(&layout, toc_title), "目录");
    assert_eq!(block_text(&layout, entry), "第一节");
    assert!(
        number.x.0 > entry.x.0,
        "the page number sits to the right ({} > {})",
        number.x.0,
        entry.x.0
    );
    assert_eq!(entry.y, number.y, "both boxes share one row");
    let digits = block_text(&layout, number);
    assert!(
        !digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit()),
        "the number box holds digits: {digits:?}"
    );
}

#[test]
fn a_toc_forces_content_onto_the_next_page() {
    let without = build(vec![heading(1, "标题"), paragraph("正文。")], &with_toc(""));
    let with = build(
        vec![heading(1, "标题"), paragraph("正文。")],
        &with_toc("toc:\n  depth: 2\n"),
    );

    assert_eq!(without.page_count(), 1);
    assert_eq!(with.page_count(), 2, "the TOC occupies its own page");
    assert!(page_text(&without, 0).contains("标题"));
    assert!(!page_text(&with, 0).contains("正文"), "page 1 is the TOC");
    assert!(page_text(&with, 1).contains("正文"));
}

#[test]
fn a_toc_without_a_title_starts_with_the_entries() {
    let blocks = vec![heading(1, "唯一标题"), paragraph("正文。")];
    let layout = build(blocks, &with_toc("toc:\n  depth: 2\n  title:\n"));

    let first = layout.pages[0].blocks.first().expect("a block");
    assert!(
        block_text(&layout, first).contains("唯一标题"),
        "first block is the entry"
    );
}

#[test]
fn toc_entries_use_the_toc_style() {
    let blocks = vec![heading(1, "标题"), paragraph("正文。")];
    let layout = build(blocks, &with_toc("toc:\n  depth: 2\n"));

    let entry = layout.pages[0]
        .blocks
        .iter()
        .find(|block| {
            block.kind == BoxKind::Paragraph
                && block.lines.len() == 1
                && block_text(&layout, block) == "标题"
        })
        .expect("the entry box");
    let body = layout.pages[1]
        .blocks
        .iter()
        .find(|block| block_text(&layout, block) == "正文。")
        .expect("the body paragraph");

    let entry_height = entry.lines[0].line.height;
    let body_height = body.lines[0].line.height;
    assert!(
        entry_height < body_height,
        "9pt/1.8 ({}) is shorter than 10.5pt/1.6 ({})",
        entry_height.0,
        body_height.0
    );
}

#[test]
fn no_toc_section_keeps_the_document_unchanged() {
    let blocks = vec![heading(1, "标题"), paragraph("正文。")];
    let layout = build(blocks, &with_toc(""));
    assert!(layout.toc.is_none());
    assert_eq!(layout.page_count(), 1);
}

#[test]
fn the_section_start_master_drops_the_running_head() {
    let template = r#"
name: chapter-start
page:
  size: custom
  width: 100mm
  height: 50mm
  margin: 8mm
  header: 6mm
  footer: 6mm
  header-text: running-title
  folio: "{page}"
  start-master: opening
masters:
  - name: opening
    header: 0
    footer: 6mm
    folio: "{page}"
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
      space-before: 0.5em
"#;
    let blocks = vec![
        heading(1, "第一章"),
        paragraph(&"正文。".repeat(40)),
        Block::PageBreak(PageBreak::new(NodeMeta::synthesized())),
        section(1),
        heading(1, "第二章"),
        paragraph(&"正文。".repeat(40)),
    ];
    let layout = build(blocks, template);

    // Every page that opens a chapter (a level-1 heading) uses the opening
    // master, including page 1.
    assert!(
        layout.pages[0].header.is_empty(),
        "chapter one opens the document"
    );
    assert_eq!(layout.pages[0].footer.len(), 1, "the folio stays");

    let chapter_two = layout
        .pages
        .iter()
        .position(|page| {
            page.blocks.iter().any(|block| {
                block.kind == BoxKind::Heading && block_text(&layout, block).contains("第二章")
            })
        })
        .expect("the second chapter page");
    assert!(chapter_two > 0, "the section opens a new page");
    assert!(
        layout.pages[chapter_two].header.is_empty(),
        "the chapter opening has no running head"
    );
    assert_eq!(layout.pages[chapter_two].footer.len(), 1, "the folio stays");
}

#[test]
fn the_plain_layout_path_is_unchanged() {
    let doc = document(vec![paragraph("普通段落。")]);
    let styled = styled(&doc, common::TIGHT);
    let layout = layout(&doc, &styled, &metrics(), &engine()).expect("layout");
    assert!(layout.toc.is_none());
    assert_eq!(
        layout.text.atom_text(layout.pages[0].blocks[0].node, 0),
        "普"
    );
}
