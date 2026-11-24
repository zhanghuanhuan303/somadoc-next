//! PDF structure, font subsetting, bookmarks and determinism.

use soma_core::{BreakConfig, SyntheticMetrics};
use soma_doc::block::{Cell, Column, Figure, Heading, Row, Table};
use soma_doc::inline::Image;
use soma_doc::meta::{ImageIntrinsic, ImageRef};
use soma_doc::node::NodeMeta;
use soma_doc::{Block, Container, Document, Inline, Link, Text};
use soma_layout::{layout, PagedLayout, TextSource};
use soma_render_pdf::{render, ImageSource, PdfFont, PdfFonts, PdfMetadata, ProgramKind};
use soma_template::{parse_template, resolve, StyledDoc};

const FONT: &[u8] = include_bytes!("fixtures/tiny.ttf");
const CFF: &[u8] = include_bytes!("fixtures/tiny-cff.otf");
const CFF_NAME: &[u8] = include_bytes!("fixtures/tiny-cff-name.otf");
const CFF_SEAC: &[u8] = include_bytes!("fixtures/tiny-cff-seac.otf");

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
    render_pdf_with_assets(layout, styled, &soma_render_pdf::NoImages)
}

fn render_pdf_with_assets(
    layout: &PagedLayout,
    styled: &StyledDoc,
    assets: &dyn ImageSource,
) -> Vec<u8> {
    render_pdf_with(
        layout,
        styled,
        &PdfFont::from_bytes(FONT.to_vec(), 0).expect("tiny font"),
        assets,
    )
}

fn render_pdf_with(
    layout: &PagedLayout,
    styled: &StyledDoc,
    font: &PdfFont,
    assets: &dyn ImageSource,
) -> Vec<u8> {
    let fonts = PdfFonts {
        fallback: None,
        regular: font,
        bold: None,
        sans: None,
        sans_bold: None,
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
        assets,
    )
    .expect("pdf")
}

/// One image, addressed by the source string a figure wrote.
struct Assets {
    src: &'static str,
    bytes: Vec<u8>,
}

impl ImageSource for Assets {
    fn image(&self, src: &str) -> Option<&[u8]> {
        (src == self.src).then_some(self.bytes.as_slice())
    }
}

/// A PNG a reader would accept, written with one stored deflate block so the
/// test does not need the encoder.
fn tiny_png(width: u32, height: u32, rgb: [u8; 3]) -> Vec<u8> {
    let mut raw = Vec::new();
    for _ in 0..height {
        raw.resize(raw.len() + 1, 0);
        for _ in 0..width {
            raw.extend_from_slice(&rgb);
        }
    }
    let mut stream = vec![0x78u8, 0x01];
    stream.push(0x01);
    stream.extend_from_slice(&(raw.len() as u16).to_le_bytes());
    stream.extend_from_slice(&(!(raw.len() as u16)).to_le_bytes());
    stream.extend_from_slice(&raw);
    let (mut low, mut high) = (1u32, 0u32);
    for byte in &raw {
        low = (low + u32::from(*byte)) % 65521;
        high = (high + low) % 65521;
    }
    stream.extend_from_slice(&((high << 16) | low).to_be_bytes());

    let mut out = Vec::new();
    out.extend_from_slice(b"\x89PNG\r\n\x1a\n");
    let mut chunk = |kind: &[u8; 4], body: &[u8]| {
        out.extend_from_slice(&(body.len() as u32).to_be_bytes());
        out.extend_from_slice(kind);
        out.extend_from_slice(body);
        out.extend_from_slice(&[0, 0, 0, 0]);
    };
    let mut header = Vec::new();
    header.extend_from_slice(&width.to_be_bytes());
    header.extend_from_slice(&height.to_be_bytes());
    header.extend_from_slice(&[8, 2, 0, 0, 0]);
    chunk(b"IHDR", &header);
    chunk(b"IDAT", &stream);
    chunk(b"IEND", &[]);
    out
}

fn figure(src: &str, alt: &str) -> Block {
    Block::Figure(Figure::new(
        NodeMeta::synthesized(),
        Image::new(
            NodeMeta::synthesized(),
            ImageRef {
                src: String::from(src),
                alt: Some(String::from(alt)),
                title: None,
                intrinsic: Some(ImageIntrinsic {
                    width_px: 4,
                    height_px: 3,
                }),
            },
        ),
    ))
}

#[test]
fn the_tiny_fixture_maps_bmp_characters() {
    let font = PdfFont::from_bytes(FONT.to_vec(), 0).expect("tiny font");
    assert_eq!(font.glyph('A' as u32), 1);
    assert_eq!(font.glyph('B' as u32), 2);
    assert_eq!(font.glyph(0x4E2D), 3);
    assert_eq!(font.glyph('Z' as u32), 0, "missing characters use .notdef");
    assert_eq!(font.width_1000(1), 600);
    assert!(font.has_glyph('A' as u32));
    assert!(!font.has_glyph('Z' as u32));
    assert_eq!(font.advance_4096('A' as u32), Some(2458), "600/1000 em");
    assert_eq!(font.advance_4096('Z' as u32), None);
}

#[test]
fn a_figure_embeds_its_image_as_an_xobject() {
    let blocks = vec![figure("fig/test.png", "流程示意")];
    let (layout, styled) = build(blocks);
    let assets = Assets {
        src: "fig/test.png",
        bytes: tiny_png(4, 3, [0x12, 0x34, 0x56]),
    };
    let pdf = render_pdf_with_assets(&layout, &styled, &assets);
    let text = String::from_utf8_lossy(&pdf);
    assert!(
        text.contains("/Subtype /Image"),
        "an image XObject is written"
    );
    assert!(text.contains("/Width 4 /Height 3"));
    assert!(text.contains("/ColorSpace /DeviceRGB"));
    assert!(text.contains("/XObject <<"), "the page names its images");
    assert!(
        text.contains("FEFF6D417A0B793A610F"),
        "the figure's alt text reaches the structure tree"
    );
}

#[test]
fn a_missing_asset_keeps_the_placeholder() {
    let blocks = vec![figure("fig/gone.png", "缺失")];
    let (layout, styled) = build(blocks);
    let pdf = render_pdf(&layout, &styled);
    let text = String::from_utf8_lossy(&pdf);
    assert!(
        !text.contains("/Subtype /Image"),
        "an unresolved figure must not invent an image"
    );
    assert!(text.contains("/S /Figure"), "it is still a tagged figure");
}

#[test]
fn the_page_is_tagged_for_accessibility() {
    let blocks = vec![
        Block::Heading(Heading::new(NodeMeta::synthesized(), 1, vec![text("标题")])),
        Block::Paragraph(soma_doc::block::Paragraph::new(
            NodeMeta::synthesized(),
            vec![text("正文")],
        )),
    ];
    let (layout, styled) = build(blocks);
    let pdf = render_pdf(&layout, &styled);
    let text = String::from_utf8_lossy(&pdf);
    assert!(text.contains("/MarkInfo << /Marked true >>"));
    assert!(text.contains("/Type /StructTreeRoot"));
    assert!(text.contains("/S /Document"));
    assert!(text.contains("/S /H1"), "a level-1 heading is /H1");
    assert!(text.contains("/S /P"));
    assert!(text.contains("/ParentTree"));
    assert!(text.contains("/StructParents 0"));
}

#[test]
fn a_table_is_tagged_with_rows_and_cells() {
    let cell = |value: &str| {
        Cell::new(
            NodeMeta::synthesized(),
            vec![Block::Paragraph(soma_doc::block::Paragraph::new(
                NodeMeta::synthesized(),
                vec![text(value)],
            ))],
        )
    };
    let columns = vec![
        Column::new(NodeMeta::synthesized(), Default::default()),
        Column::new(NodeMeta::synthesized(), Default::default()),
    ];
    let mut header = cell("头一");
    header.colspan = 2;
    let blocks = vec![Block::Table(Table::new(
        NodeMeta::synthesized(),
        columns,
        vec![
            Row::new(NodeMeta::synthesized(), true, vec![header]),
            Row::new(NodeMeta::synthesized(), false, vec![cell("甲"), cell("乙")]),
        ],
    ))];
    let (layout, styled) = build(blocks);
    let pdf = render_pdf(&layout, &styled);
    let text = String::from_utf8_lossy(&pdf);

    assert!(
        text.contains("/S /Table"),
        "the table is a structure element"
    );
    assert_eq!(text.matches("/S /TR").count(), 2, "one row each");
    assert_eq!(text.matches("/S /TD").count(), 3, "one cell each");
    assert!(
        text.contains("/O /Table /RowSpan 1 /ColSpan 2"),
        "the spanning header cell says so"
    );
    assert!(!text.contains("/S /P"), "a cell is a /TD, not a paragraph");
}

#[test]
fn a_subset_is_a_standalone_sfnt() {
    let font = PdfFont::from_bytes(FONT.to_vec(), 0).expect("tiny font");
    let subset = font.subset(&[1, 3]).expect("subset");
    assert_eq!(subset.kind, ProgramKind::TrueType);
    let program = subset.program;
    assert_eq!(&program[..4], &[0, 1, 0, 0], "sfnt version");
    assert!(program.windows(4).any(|window| window == b"glyf"));
    assert!(program.windows(4).any(|window| window == b"loca"));
    assert!(program.windows(4).any(|window| window == b"head"));
}

#[test]
fn a_cid_keyed_cff_is_read_like_any_other_face() {
    let font = PdfFont::from_bytes(CFF.to_vec(), 0).expect("cff font");
    assert_eq!(font.program_kind(), ProgramKind::CidCff);
    assert_eq!(font.glyph('A' as u32), 1);
    assert_eq!(font.glyph(0x4E2D), 3);
    assert_eq!(font.glyph('Z' as u32), 0, "missing characters use .notdef");
    assert_eq!(font.width_1000(3), 1000);
    assert_eq!(font.cid_system().ordering, "Identity");
}

#[test]
fn a_name_keyed_cff_is_promoted_to_cid_keyed() {
    let font = PdfFont::from_bytes(CFF_NAME.to_vec(), 0).expect("name-keyed cff");
    let subset = font.subset(&[1, 2]).expect("subset");
    assert_eq!(subset.kind, ProgramKind::CidCff);
    assert!(
        subset.program.windows(5).any(|window| window == b"Adobe"),
        "the ROS registry has to be a string of the program"
    );
    assert!(
        subset
            .program
            .windows(8)
            .any(|window| window == b"Identity"),
        "the ROS ordering has to be a string of the program"
    );
}

#[test]
fn a_seac_composite_is_embedded_as_one_outline() {
    // The fixture maps U+00C1 to a glyph drawn from two glyphs with `endchar`.
    let blocks = vec![Block::Paragraph(soma_doc::block::Paragraph::new(
        NodeMeta::synthesized(),
        vec![text("A\u{c1}B")],
    ))];
    let (layout, styled) = build(blocks);
    let font = PdfFont::from_bytes(CFF_SEAC.to_vec(), 0).expect("seac font");
    let pdf = render_pdf_with(&layout, &styled, &font, &soma_render_pdf::NoImages);
    let text = String::from_utf8_lossy(&pdf);
    assert!(text.contains("/Subtype /CIDFontType0C"));
    assert!(
        text.contains("/ToUnicode"),
        "the composite keeps a text mapping"
    );
}

#[test]
fn a_cff_face_is_embedded_as_a_cidfonttype0() {
    let blocks = vec![Block::Paragraph(soma_doc::block::Paragraph::new(
        NodeMeta::synthesized(),
        vec![text("A B 中")],
    ))];
    let (layout, styled) = build(blocks);
    let font = PdfFont::from_bytes(CFF.to_vec(), 0).expect("cff font");
    let pdf = render_pdf_with(&layout, &styled, &font, &soma_render_pdf::NoImages);
    let text = String::from_utf8_lossy(&pdf);
    assert!(text.contains("/FontFile3"), "CFF is a FontFile3");
    assert!(
        text.contains("/Subtype /CIDFontType0C"),
        "Type 2 charstrings"
    );
    assert!(text.contains("/Subtype /CIDFontType0 "), "CIDFontType0");
    assert!(!text.contains("/FontFile2"), "no TrueType program");
}

#[test]
fn cff2_is_rejected_with_a_clear_error() {
    let mut data = CFF_NAME.to_vec();
    let at = data
        .windows(4)
        .position(|window| window == b"CFF ")
        .expect("cff table tag");
    data[at..at + 4].copy_from_slice(b"CFF2");
    let error = match PdfFont::from_bytes(data, 0) {
        Ok(_) => panic!("CFF2 is unsupported"),
        Err(error) => error,
    };
    assert!(error.reason.contains("CFF2"), "{}", error.reason);
}

#[test]
fn underlines_and_links_reach_the_page() {
    let link = Link::new(
        NodeMeta::synthesized(),
        "https://example.com/a(b)",
        vec![text("链接")],
    );
    let underline = Container::new(NodeMeta::synthesized(), vec![text("下划线")]);
    let blocks = vec![Block::Paragraph(soma_doc::block::Paragraph::new(
        NodeMeta::synthesized(),
        vec![Inline::Link(link), text(" "), Inline::Underline(underline)],
    ))];
    let (layout, styled) = build(blocks);
    let pdf = render_pdf(&layout, &styled);
    let text = String::from_utf8_lossy(&pdf);
    assert!(text.contains("/Subtype /Link"), "a link is an annotation");
    assert!(
        text.contains("/URI (https://example.com/a\\(b\\))"),
        "the URI is an escaped PDF string"
    );
    // The content stream is deflated, so the rule itself is checked by the
    // painter's unit test; here the decoration only has to reach the renderer.
    let underlined = layout
        .pages
        .iter()
        .flat_map(|page| page.blocks.iter())
        .any(|block| {
            (block.line_offset..block.line_offset + 8).any(|atom| {
                layout
                    .text
                    .atom_decorations(block.node, atom)
                    .contains(soma_core::Decorations::UNDERLINE)
            })
        });
    assert!(underlined, "the underline reaches the text index");
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
