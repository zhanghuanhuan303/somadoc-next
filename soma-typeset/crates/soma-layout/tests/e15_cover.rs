//! E15: generated cover page from document metadata.

mod common;

use common::{engine, metrics, paragraph};
use soma_doc::meta::{CoverData, DocumentMeta};
use soma_doc::Document;
use soma_layout::{layout, BoxKind, PagedLayout, TextSource};
use soma_template::{parse_template, resolve};

const TEMPLATE: &str = r#"
name: cover
page:
  size: custom
  width: 100mm
  height: 60mm
  margin: 8mm
  header: 6mm
  footer: 6mm
  header-text: running-title
  folio: "{page}"
cover:
  master: blank
masters:
  - name: blank
    header: 0
    footer: 0
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
      element: coverTitle
    declarations:
      font-size: 24pt
      line-height: 2
      align: center
      space-before: 20mm
      space-after: 4mm
  - selector:
      element: coverSubtitle
    declarations:
      font-size: 12pt
      align: center
      space-after: 2mm
  - selector:
      element: coverAuthor
    declarations:
      font-size: 11pt
      align: center
  - selector:
      element: coverDate
    declarations:
      font-size: 10pt
      align: center
      space-before: 4mm
"#;

fn document(meta: DocumentMeta) -> Document {
    let mut doc = Document::new(meta);
    doc.push_block(paragraph("正文第一段。"));
    doc.renumber();
    doc
}

fn build(doc: &Document, template: &str) -> PagedLayout {
    let styled = resolve(doc, &parse_template(template).expect("template")).expect("styles");
    layout(doc, &styled, &metrics(), &engine()).expect("layout")
}

fn text_of(layout: &PagedLayout, page: usize) -> String {
    layout.pages[page]
        .blocks
        .iter()
        .map(|block| {
            let mut out = String::new();
            for placed in &block.lines {
                for atom in &placed.line.boxes {
                    out.push_str(layout.text.atom_text(block.node, atom.atom));
                }
            }
            out
        })
        .collect::<Vec<_>>()
        .join(" | ")
}

fn full_meta() -> DocumentMeta {
    let mut cover = CoverData::new();
    cover.title = Some(String::from("中文排版引擎"));
    cover.subtitle = Some(String::from("从断行到成书"));
    cover.authors = vec![String::from("张三"), String::from("李四")];
    cover.date = Some(String::from("2026 年 11 月"));
    DocumentMeta::new()
        .with_language("zh-Hans")
        .with_cover(cover)
}

#[test]
fn a_cover_page_uses_the_cover_metadata() {
    let doc = document(full_meta());
    let layout = build(&doc, TEMPLATE);

    assert!(layout.page_count() >= 2, "cover + body");
    let first = text_of(&layout, 0);
    assert!(first.contains("中文排版引擎"), "{first}");
    assert!(first.contains("从断行到成书"), "{first}");
    assert!(
        first.contains("张三、李四"),
        "authors join with 、: {first}"
    );
    assert!(first.contains("2026年11月"), "{first}");
    assert!(!first.contains("正文第一段"), "the body starts on page 2");
    assert!(text_of(&layout, 1).contains("正文第一段"));
}

#[test]
fn the_cover_uses_its_master() {
    let doc = document(full_meta());
    let layout = build(&doc, TEMPLATE);

    assert!(
        layout.pages[0].header.is_empty() && layout.pages[0].footer.is_empty(),
        "the blank master hides the margin bands"
    );
    assert_eq!(
        layout.pages[1].footer.len(),
        1,
        "the body page gets the folio again"
    );
}

#[test]
fn cover_lines_are_centred() {
    let doc = document(full_meta());
    let layout = build(&doc, TEMPLATE);
    let title = layout.pages[0]
        .blocks
        .iter()
        .find(|block| block.kind == BoxKind::Paragraph)
        .expect("the cover title");

    let line = &title.lines[0].line;
    let slack = title.width.0 - line.natural_width.0;
    assert!(slack > 0, "the title is narrower than the page");
    assert_eq!(
        line.boxes[0].x.0,
        slack / 2,
        "the title starts at half the slack"
    );
}

#[test]
fn the_cover_falls_back_to_document_metadata() {
    let meta = DocumentMeta::new()
        .with_language("en")
        .with_title(vec![soma_doc::Text::new(
            soma_doc::node::NodeMeta::synthesized(),
            "A Small Book",
        )
        .into()]);
    let mut meta = meta;
    meta.authors = vec![soma_doc::meta::Author::new("Ada")];
    let doc = document(meta);

    let layout = build(&doc, TEMPLATE);
    let first = text_of(&layout, 0);
    assert!(
        first.contains("Small"),
        "the title falls back to meta.title: {first}"
    );
    assert!(first.contains("Book"), "{first}");
    assert!(first.contains("Ada"), "{first}");
    assert!(!first.contains("2026"), "no date in the metadata");
}

#[test]
fn a_document_without_cover_data_still_gets_a_cover_when_the_template_asks() {
    let doc = document(
        DocumentMeta::new()
            .with_language("zh-Hans")
            .with_title(vec![soma_doc::Text::new(
                soma_doc::node::NodeMeta::synthesized(),
                "只有标题",
            )
            .into()]),
    );

    let layout = build(&doc, TEMPLATE);
    assert!(layout.page_count() >= 2);
    assert!(text_of(&layout, 0).contains("只有标题"));
}

#[test]
fn no_cover_key_means_no_cover_page() {
    let doc = document(full_meta());
    let without = TEMPLATE.replace("cover:\n  master: blank\n", "");

    let layout = build(&doc, &without);
    assert_eq!(layout.page_count(), 1, "only the body");
    assert!(
        text_of(&layout, 0).contains("正文第一段"),
        "the title never appears"
    );
    assert!(!text_of(&layout, 0).contains("中文排版引擎"));
}

#[test]
fn an_unknown_cover_master_is_a_layout_error() {
    let doc = document(full_meta());
    let styled = resolve(&doc, &parse_template(TEMPLATE).expect("template")).expect("styles");
    let bad = TEMPLATE.replace("master: blank", "master: nope");

    let styled_bad = resolve(&doc, &parse_template(&bad).expect("template")).expect("styles");
    assert_eq!(
        layout(&doc, &styled_bad, &metrics(), &engine())
            .unwrap_err()
            .reason,
        "unknown cover master"
    );
    assert!(layout(&doc, &styled, &metrics(), &engine()).is_ok());
}
