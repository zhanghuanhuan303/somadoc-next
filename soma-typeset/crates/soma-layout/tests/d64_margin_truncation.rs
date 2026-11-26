//! D64: header/footer text is one line and long text is truncated with `…`,
//! never wrapped into the content box.

mod common;

use common::{document, engine, metrics, paragraph, styled, text};
use soma_core::Width;
use soma_doc::block::Heading;
use soma_doc::node::NodeMeta;
use soma_doc::Block;
use soma_layout::{layout, PagedLayout, PlacedBlock, TextSource};

const LONG_LITERAL: &str =
    "这一行页眉文字故意写得非常长用来验证页眉带的截断策略而不是让它压进正文版心";
const LONG_RUNNING: &str =
    "一个非常长的章节标题用来验证运行页眉不会折行也不会越过正文版心的左右边界";

const STYLES: &str = r#"styles:
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
      font-size: 12pt
      space-before: 0
"#;

fn source(header_text: &str, folio: bool) -> String {
    let folio = if folio { "  folio: \"{page}\"\n" } else { "" };
    format!(
        "name: margin\npage:\n  size: custom\n  width: 100mm\n  height: 70mm\n  margin: 8mm\n  header: 8mm\n  footer: 8mm\n  header-text: {header_text}\n{folio}{STYLES}"
    )
}

fn heading(level: u8, value: &str) -> Block {
    Block::Heading(Heading::new(
        NodeMeta::synthesized(),
        level,
        vec![text(value)],
    ))
}

fn margin_text(layout: &PagedLayout, blocks: &[PlacedBlock]) -> String {
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

fn assert_band_is_inside_the_content_box(layout: &PagedLayout, block: &PlacedBlock) {
    let content_width = layout.page.content_width();
    assert!(block.width.0 > 0, "the band still prints something");
    assert!(
        block.width.0 <= content_width.0,
        "the line fits the measure"
    );
    assert!(block.x.0 >= 0, "the line does not start in the left margin");
    assert!(
        block.x.0 + block.width.0 <= content_width.0,
        "the line does not enter the right margin"
    );
    for line in &block.lines {
        assert!(line.y.0 >= 0, "the line stays inside its band");
        assert!(
            line.y.0 + line.line.height.0 <= block.height.0,
            "the line box stays inside the band"
        );
    }
}

fn assert_centred(layout: &PagedLayout, block: &PlacedBlock) {
    assert_eq!(
        block.x.0,
        (layout.page.content_width().0 - block.width.0) / 2,
        "a margin line stays centred in the content column"
    );
}

fn assert_truncated(full: &str, printed: &str) {
    assert!(
        printed.ends_with('…'),
        "long margin text ends with an explicit ellipsis: {printed}"
    );
    let kept = printed.trim_end_matches('…');
    assert!(
        full.starts_with(kept),
        "truncation only removes a suffix: {printed}"
    );
    assert!(
        kept.chars().count() < full.chars().count(),
        "the fixture really is longer than the band"
    );
}

#[test]
fn a_long_literal_header_is_truncated_before_the_folio() {
    let doc = document(vec![paragraph("正文。")]);
    let styled = styled(&doc, &source(LONG_LITERAL, true));
    let layout = layout(&doc, &styled, &metrics(), &engine()).expect("layout");
    let page = &layout.pages[0];

    let header = page.header.first().expect("a header block");
    let printed = margin_text(&layout, &page.header);
    assert_truncated(LONG_LITERAL, &printed);
    assert_band_is_inside_the_content_box(&layout, header);
    assert_centred(&layout, header);
    assert_eq!(header.y + header.height, Width::ZERO);
    assert_eq!(header.lines[0].y, Width::ZERO);

    let footer = page.footer.first().expect("a folio block");
    assert_eq!(margin_text(&layout, &page.footer), "1");
    assert_band_is_inside_the_content_box(&layout, footer);
    assert_centred(&layout, footer);
    assert_eq!(footer.y, layout.page.content_height());
}

#[test]
fn a_long_literal_header_fits_without_a_folio() {
    let doc = document(vec![paragraph("正文。")]);
    let styled = styled(&doc, &source(LONG_LITERAL, false));
    let layout = layout(&doc, &styled, &metrics(), &engine()).expect("layout");
    let page = &layout.pages[0];

    let header = page.header.first().expect("a header block");
    let printed = margin_text(&layout, &page.header);
    assert_truncated(LONG_LITERAL, &printed);
    assert_band_is_inside_the_content_box(&layout, header);
    assert_centred(&layout, header);
    assert!(
        page.footer.is_empty(),
        "no folio means no footer block at all"
    );
}

#[test]
fn a_long_running_title_is_truncated_before_the_folio() {
    let doc = document(vec![heading(1, LONG_RUNNING), paragraph("正文。")]);
    let styled = styled(&doc, &source("running-title", true));
    let layout = layout(&doc, &styled, &metrics(), &engine()).expect("layout");
    let page = &layout.pages[0];

    let header = page.header.first().expect("a running header");
    let printed = margin_text(&layout, &page.header);
    assert_truncated(LONG_RUNNING, &printed);
    assert_band_is_inside_the_content_box(&layout, header);

    let footer = page.footer.first().expect("a folio block");
    assert_eq!(margin_text(&layout, &page.footer), "1");
    assert_band_is_inside_the_content_box(&layout, footer);
    assert_centred(&layout, footer);
}

#[test]
fn a_long_running_title_fits_without_a_folio() {
    let doc = document(vec![heading(1, LONG_RUNNING), paragraph("正文。")]);
    let styled = styled(&doc, &source("running-title", false));
    let layout = layout(&doc, &styled, &metrics(), &engine()).expect("layout");
    let page = &layout.pages[0];

    let header = page.header.first().expect("a running header");
    let printed = margin_text(&layout, &page.header);
    assert_truncated(LONG_RUNNING, &printed);
    assert_band_is_inside_the_content_box(&layout, header);
    assert!(page.footer.is_empty());
}

#[test]
fn a_short_header_is_not_touched() {
    let doc = document(vec![paragraph("正文。")]);
    let styled = styled(&doc, &source("短页眉", false));
    let layout = layout(&doc, &styled, &metrics(), &engine()).expect("layout");
    let page = &layout.pages[0];

    let header = page.header.first().expect("a header block");
    assert_eq!(margin_text(&layout, &page.header), "短页眉");
    assert_band_is_inside_the_content_box(&layout, header);
    assert_centred(&layout, header);
}
