//! PDF structure, font subsetting, bookmarks and determinism.

use soma_core::{BreakConfig, SyntheticMetrics};
use soma_doc::block::Heading;
use soma_doc::node::NodeMeta;
use soma_doc::{Block, Document, Text};
use soma_layout::{layout, PagedLayout};
use soma_render_pdf::{render, PdfFont, PdfFonts, PdfMetadata};
use soma_template::{parse_template, resolve, StyledDoc};

const FONT: &[u8] = include_bytes!("fixtures/tiny.ttf");

const TEMPLATE: &str = r#"
name: pdf
page:
  size: custom
  width: 120mm
  height: 120mm
  margin: 10mm
  folio: "{page}"
  footer: 6mm
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
      element: heading
    declarations:
      font-size: 16pt
"#;

fn text(value: &str) -> soma_doc::Inline {
    Text::new(NodeMeta::synthesized(), value).into()
}

fn build(blocks: Vec<Block>) -> (PagedLayout, StyledDoc) {
    let mut doc = Document::new(soma_doc::meta::DocumentMeta::new().with_language("zh-Hans"));
    doc.blocks = blocks;
    doc.renumber();
    let styled = resolve(&doc, &parse_template(TEMPLATE).expect("template")).expect("styles");
    let layout = layout(
        &doc,
        &styled,
        &SyntheticMetrics::default(),
        &BreakConfig::default(),
    )
    .expect("layout");
    (layout, styled)
}

fn render_pdf(layout: &PagedLayout, styled: &StyledDoc) -> Vec<u8> {
    let font = PdfFont::from_bytes(FONT.to_vec(), 0).expect("tiny font");
    let fonts = PdfFonts {
        regular: &font,
        bold: None,
        math: None,
    };
    render(
        layout,
        styled,
        &layout.text,
        &fonts,
        &PdfMetadata {
            title: Some("示例"),
            author: Some("作者"),
            language: Some("zh-Hans"),
        },
    )
    .expect("pdf")
}

#[test]
fn the_tiny_fixture_maps_bmp_characters() {
    let font = PdfFont::from_bytes(FONT.to_vec(), 0).expect("tiny font");
    assert_eq!(font.glyph('A' as u32), 1);
    assert_eq!(font.glyph('B' as u32), 2);
    assert_eq!(font.glyph(0x4E2D), 3);
    assert_eq!(font.glyph('Z' as u32), 0, "missing characters use .notdef");
    assert_eq!(font.width_1000(1), 600);
}

#[test]
fn a_subset_is_a_standalone_sfnt() {
    let font = PdfFont::from_bytes(FONT.to_vec(), 0).expect("tiny font");
    let program = font.subset(&[1, 3]);
    assert_eq!(&program[..4], &[0, 1, 0, 0], "sfnt version");
    assert!(program.windows(4).any(|window| window == b"glyf"));
    assert!(program.windows(4).any(|window| window == b"loca"));
    assert!(program.windows(4).any(|window| window == b"head"));
}

#[test]
fn cff_fonts_are_rejected_with_a_clear_error() {
    let mut data = vec![0u8; 64];
    data[..4].copy_from_slice(b"OTTO");
    let error = match PdfFont::from_bytes(data, 0) {
        Ok(_) => panic!("CFF is unsupported"),
        Err(error) => error,
    };
    assert!(error.reason.contains("CFF"), "{}", error.reason);
}

#[test]
fn the_pdf_has_pages_fonts_and_metadata() {
    let blocks = vec![
        Block::Heading(Heading::new(NodeMeta::synthesized(), 1, vec![text("A中B")])),
        Block::Paragraph(soma_doc::block::Paragraph::new(
            NodeMeta::synthesized(),
            vec![text("A B 中")],
        )),
    ];
    let (layout, styled) = build(blocks);
    let pdf = render_pdf(&layout, &styled);
    let text = String::from_utf8_lossy(&pdf);

    assert!(pdf.starts_with(b"%PDF-1.7"), "header");
    assert!(text.contains("/Type /Catalog"), "catalog");
    assert!(text.contains("/Type /Pages"), "pages tree");
    assert!(text.contains("/Type /Page "), "page object");
    assert!(text.contains("/Subtype /Type0"), "embedded text font");
    assert!(text.contains("/FontFile2"), "TrueType program");
    assert!(text.contains("/PageLabels"), "printed page labels");
    assert!(text.contains("/Outlines"), "bookmarks");
    assert!(text.contains("/Title <FEFF"), "UTF-16 title");
    assert!(text.ends_with("%%EOF\n"), "trailer");
}

#[test]
fn rendering_is_byte_deterministic() {
    let blocks = vec![Block::Paragraph(soma_doc::block::Paragraph::new(
        NodeMeta::synthesized(),
        vec![text("A B 中 A")],
    ))];
    let (layout, styled) = build(blocks);
    let first = render_pdf(&layout, &styled);
    let second = render_pdf(&layout, &styled);
    assert_eq!(first, second);
}
