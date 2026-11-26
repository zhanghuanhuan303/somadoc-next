//! Page masters selected by sections: margin bands and running text.

mod common;

use common::{document, engine, metrics, paragraph, styled, text, TIGHT};
use soma_core::Width;
use soma_doc::block::{Heading, Section};
use soma_doc::node::NodeMeta;
use soma_doc::Block;
use soma_layout::layout;
use soma_layout::TextSource;

const MASTERS: &str = r#"
name: masters
page:
  size: custom
  width: 100mm
  height: 60mm
  margin: 8mm
  header: 6mm
  footer: 6mm
  header-text: running-title
  folio: "{page}"
masters:
  - name: chapter
    header: 4mm
    footer: 0
    header-text: 章节页
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

fn section(name: &str) -> Block {
    let mut section = Section::new(NodeMeta::synthesized(), 1);
    section.page_template = Some(String::from(name));
    Block::Section(section)
}

fn heading(level: u8, value: &str) -> Block {
    Block::Heading(Heading::new(
        NodeMeta::synthesized(),
        level,
        vec![text(value)],
    ))
}

const RUNNING: &str = r#"
name: running
page:
  size: custom
  width: 100mm
  height: 60mm
  margin: 8mm
  header: 6mm
  header-text: running-title
styles:
  - selector:
      element: document
    declarations:
      font-size: 9pt
      line-height: 1.5
      align: justify
  - selector:
      element: paragraph
    declarations:
      first-indent: 0
  - selector:
      element: heading
    declarations:
      font-size: 12pt
      space-before: 0.5em
"#;

#[test]
fn a_running_head_keeps_the_chapter_when_a_section_starts() {
    let doc = document(vec![
        heading(1, "第一章"),
        paragraph("开篇。"),
        heading(2, "第一节"),
        paragraph(&"续文".repeat(160)),
    ]);
    let styled = styled(&doc, RUNNING);
    let layout = layout(&doc, &styled, &metrics(), &engine()).expect("layout");

    assert!(layout.page_count() >= 2, "the paragraph spans pages");
    let last = layout.pages.last().expect("a page");
    assert_eq!(margin_text(&layout, &last.header), "第一章");
}

#[test]
fn sample_mode_keeps_chapters_in_one_flow() {
    let blocks = vec![
        heading(1, "第一章"),
        paragraph("正文。"),
        heading(1, "第二章"),
        paragraph("正文。"),
    ];
    let sample = RUNNING.replace("styles:", "layout:\n  mode: sample\nstyles:");
    let sample = sample.replace(
        "space-before: 0.5em",
        "space-before: 0.5em\n      page-break-before: true",
    );
    let doc = document(blocks.clone());
    let sample_styled = styled(&doc, &sample);
    let flow = layout(&doc, &sample_styled, &metrics(), &engine()).expect("sample layout");
    assert_eq!(flow.page_count(), 1, "chapters follow one another");

    let book = RUNNING.replace(
        "space-before: 0.5em",
        "space-before: 0.5em\n      page-break-before: true",
    );
    let doc = document(blocks);
    let book_styled = styled(&doc, &book);
    let paged = layout(&doc, &book_styled, &metrics(), &engine()).expect("book layout");
    assert_eq!(paged.page_count(), 2, "book mode opens each chapter");
}

fn margin_text(layout: &soma_layout::PagedLayout, blocks: &[soma_layout::PlacedBlock]) -> String {
    let mut out = String::new();
    for block in blocks {
        for line in &block.lines {
            for atom in &line.line.boxes {
                out.push_str(layout.text.atom_text(block.node, atom.atom));
            }
        }
    }
    out
}

#[test]
fn a_section_switches_the_page_master_on_a_new_page() {
    let doc = document(vec![
        paragraph("前言"),
        section("chapter"),
        paragraph(&"正文".repeat(220)),
    ]);
    let styled = styled(&doc, MASTERS);
    let layout = layout(&doc, &styled, &metrics(), &engine()).expect("layout");

    assert!(layout.page_count() >= 2, "the section starts a page");
    assert_eq!(margin_text(&layout, &layout.pages[0].header), "");
    assert_eq!(margin_text(&layout, &layout.pages[0].footer), "1");
    assert_eq!(layout.pages[0].header.len(), 0, "running title is empty");

    for page in &layout.pages[1..] {
        assert_eq!(
            margin_text(&layout, &page.header),
            "章节页",
            "page {} uses the chapter master",
            page.number
        );
        assert!(
            page.footer.is_empty(),
            "page {} has no footer band",
            page.number
        );
    }

    assert!(
        layout.pages[1].blocks.iter().any(|block| {
            block.lines.iter().any(|line| {
                line.line
                    .boxes
                    .iter()
                    .any(|atom| layout.text.atom_text(block.node, atom.atom).contains('正'))
            })
        }),
        "the section content starts on the new page"
    );
    assert!(!layout.pages[1].header.is_empty());
    assert!(layout.pages.iter().all(|page| !page.overflow));
}

#[test]
fn the_folio_owns_the_bottom_of_its_band() {
    const BANDS: &str = r#"
name: bands
page:
  size: custom
  width: 100mm
  height: 60mm
  margin: 8mm
  header: 6mm
  footer: 6mm
  header-text: 页眉
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
"#;
    let doc = document(vec![paragraph("正文")]);
    let styled = styled(&doc, BANDS);
    let layout = layout(&doc, &styled, &metrics(), &engine()).expect("layout");
    let page = &layout.pages[0];

    let header = page.header.first().expect("a header block");
    assert_eq!(
        header.y + header.height,
        Width::ZERO,
        "the header band hangs above the content origin"
    );
    assert_eq!(
        header.lines[0].y,
        Width::ZERO,
        "a header fills from the top"
    );

    let footer = page.footer.first().expect("a footer block");
    assert_eq!(
        footer.y,
        layout.page.content_height(),
        "the footer band starts at the bottom of the text block"
    );
    assert_eq!(
        footer.lines[0].y + footer.lines[0].line.height,
        footer.height,
        "the folio sits on the last line of its band"
    );
}

#[test]
fn masters_do_not_change_the_content_box() {
    let doc = document(vec![paragraph(&"正文".repeat(220))]);
    let styled = styled(&doc, MASTERS);
    let layout = layout(&doc, &styled, &metrics(), &engine()).expect("layout");
    assert_eq!(layout.page.content_height(), styled.page().content_height());
    assert_eq!(layout.page.content_width(), styled.page().content_width());
    assert_eq!(styled.masters().len(), 2);
    assert_ne!(
        styled.masters()[1].header_height,
        styled.masters()[0].header_height
    );
    assert_eq!(styled.masters()[1].footer_height.0, 0);
}

#[test]
fn an_unknown_master_is_an_error() {
    let doc = document(vec![section("missing"), paragraph("正文")]);
    let styled = styled(&doc, MASTERS);
    let error = layout(&doc, &styled, &metrics(), &engine()).expect_err("unknown master");
    assert_eq!(error.reason, "unknown page master");
}

#[test]
fn a_plain_template_has_only_the_default_master() {
    let doc = document(vec![paragraph("正文")]);
    let styled = styled(&doc, TIGHT);
    assert_eq!(styled.masters().len(), 1);
    assert_eq!(styled.masters()[0].name, "default");
    assert_eq!(styled.master_index("default"), Some(0));
    assert_eq!(styled.master_index("chapter"), None);
}
