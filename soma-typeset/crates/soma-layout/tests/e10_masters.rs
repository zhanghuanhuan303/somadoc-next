//! Page masters selected by sections: margin bands and running text.

mod common;

use common::{document, engine, metrics, paragraph, styled, TIGHT};
use soma_doc::block::Section;
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
