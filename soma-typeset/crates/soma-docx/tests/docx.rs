//! End-to-end tests: a zip archive in, a validated DocIR document out.

use soma_doc::block::{Block, Figure};
use soma_doc::inline::Inline;
use soma_doc::meta::ImageIntrinsic;
use soma_doc::Document;
use soma_docx::{parse_docx, parse_docx_with_media};

// ---------------------------------------------------------------------------
// A minimal zip writer for fixtures
// ---------------------------------------------------------------------------

struct Entry<'a> {
    name: &'a str,
    method: u16,
    data: &'a [u8],
    compressed: &'a [u8],
}

fn crc32(data: &[u8]) -> u32 {
    let mut value = 0xffff_ffffu32;
    for byte in data {
        value ^= u32::from(*byte);
        for _ in 0..8 {
            value = if value & 1 != 0 {
                0xedb8_8320 ^ (value >> 1)
            } else {
                value >> 1
            };
        }
    }
    !value
}

fn put_u16(out: &mut Vec<u8>, value: u16) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn put_u32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn build_zip(entries: &[Entry<'_>]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut offsets = Vec::new();

    for entry in entries {
        offsets.push(out.len() as u32);
        out.extend_from_slice(b"PK\x03\x04");
        put_u16(&mut out, 20);
        put_u16(&mut out, 0);
        put_u16(&mut out, entry.method);
        put_u16(&mut out, 0);
        put_u16(&mut out, 0);
        put_u32(&mut out, crc32(entry.data));
        put_u32(&mut out, entry.compressed.len() as u32);
        put_u32(&mut out, entry.data.len() as u32);
        put_u16(&mut out, entry.name.len() as u16);
        put_u16(&mut out, 0);
        out.extend_from_slice(entry.name.as_bytes());
        out.extend_from_slice(entry.compressed);
    }

    let directory_offset = out.len();
    for (entry, offset) in entries.iter().zip(&offsets) {
        out.extend_from_slice(b"PK\x01\x02");
        put_u16(&mut out, 20);
        put_u16(&mut out, 20);
        put_u16(&mut out, 0);
        put_u16(&mut out, entry.method);
        put_u16(&mut out, 0);
        put_u16(&mut out, 0);
        put_u32(&mut out, crc32(entry.data));
        put_u32(&mut out, entry.compressed.len() as u32);
        put_u32(&mut out, entry.data.len() as u32);
        put_u16(&mut out, entry.name.len() as u16);
        put_u16(&mut out, 0);
        put_u16(&mut out, 0);
        put_u16(&mut out, 0);
        put_u16(&mut out, 0);
        put_u32(&mut out, 0);
        put_u32(&mut out, *offset);
        out.extend_from_slice(entry.name.as_bytes());
    }
    let directory_size = out.len() - directory_offset;

    out.extend_from_slice(b"PK\x05\x06");
    put_u16(&mut out, 0);
    put_u16(&mut out, 0);
    put_u16(&mut out, entries.len() as u16);
    put_u16(&mut out, entries.len() as u16);
    put_u32(&mut out, directory_size as u32);
    put_u32(&mut out, directory_offset as u32);
    put_u16(&mut out, 0);
    out
}

fn store(files: &[(&str, &[u8])]) -> Vec<u8> {
    let entries: Vec<Entry<'_>> = files
        .iter()
        .map(|(name, data)| Entry {
            name,
            method: 0,
            data,
            compressed: data,
        })
        .collect();
    build_zip(&entries)
}

/// A raw deflate stream made of stored blocks (BTYPE=0). Any bytes that are
/// valid deflate are enough to exercise method 8 without a compressor.
fn deflate_stored(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut remaining = data;
    if remaining.is_empty() {
        out.extend_from_slice(&[0x01, 0x00, 0x00, 0xff, 0xff]);
        return out;
    }
    while !remaining.is_empty() {
        let take = remaining.len().min(65535);
        let chunk = &remaining[..take];
        remaining = &remaining[take..];
        out.push(u8::from(remaining.is_empty()));
        let length = chunk.len() as u16;
        out.extend_from_slice(&length.to_le_bytes());
        out.extend_from_slice(&(!length).to_le_bytes());
        out.extend_from_slice(chunk);
    }
    out
}

fn deflated(files: &[(&str, &[u8])]) -> Vec<u8> {
    let compressed: Vec<Vec<u8>> = files.iter().map(|(_, data)| deflate_stored(data)).collect();
    let entries: Vec<Entry<'_>> = files
        .iter()
        .zip(compressed.iter())
        .map(|((name, data), compressed)| Entry {
            name,
            method: 8,
            data,
            compressed,
        })
        .collect();
    build_zip(&entries)
}

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

const DOCUMENT_XML: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"
            xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">
  <w:body>
    <w:p>
      <w:pPr><w:pStyle w:val="Heading1"/></w:pPr>
      <w:r><w:t>Heading</w:t></w:r>
    </w:p>
    <w:p>
      <w:r><w:t xml:space="preserve">Hello </w:t></w:r>
      <w:r><w:rPr><w:b/></w:rPr><w:t>bold</w:t></w:r>
      <w:r><w:rPr><w:i/></w:rPr><w:t>italic</w:t></w:r>
      <w:r><w:rPr><w:vertAlign w:val="superscript"/></w:rPr><w:t>2</w:t></w:r>
      <w:r><w:rPr><w:u w:val="single"/></w:rPr><w:t>under</w:t></w:r>
    </w:p>
    <w:tbl>
      <w:tr>
        <w:tc><w:p><w:r><w:t>A</w:t></w:r></w:p></w:tc>
        <w:tc><w:p><w:r><w:t>B</w:t></w:r></w:p></w:tc>
      </w:tr>
      <w:tr>
        <w:tc><w:p><w:r><w:t>1</w:t></w:r></w:p></w:tc>
        <w:tc><w:p><w:r><w:t>2</w:t></w:r></w:p></w:tc>
      </w:tr>
    </w:tbl>
    <w:p>
      <w:pPr><w:numPr><w:ilvl w:val="0"/><w:numId w:val="1"/></w:numPr></w:pPr>
      <w:r><w:t>one</w:t></w:r>
    </w:p>
    <w:p>
      <w:pPr><w:numPr><w:ilvl w:val="0"/><w:numId w:val="1"/></w:numPr></w:pPr>
      <w:r><w:t>two</w:t></w:r>
    </w:p>
    <w:p>
      <w:r><w:rPr><w:b w:val="0"/></w:rPr><w:t>plain</w:t></w:r>
      <w:r><w:tab/><w:br/><w:t xml:space="preserve"> tail</w:t></w:r>
    </w:p>
    <w:sectPr><w:pgSz w:w="11906" w:h="16838"/></w:sectPr>
    <w:unexpected><w:p><w:r><w:t>dropped</w:t></w:r></w:p></w:unexpected>
  </w:body>
</w:document>"#;

const CORE_XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<cp:coreProperties xmlns:cp="http://schemas.openxmlformats.org/package/2006/metadata/core-properties"
                   xmlns:dc="http://purl.org/dc/elements/1.1/">
  <dc:title>Sample Title</dc:title>
  <dc:creator>Alice; Bob, Carol</dc:creator>
</cp:coreProperties>"#;

/// The dynamic-Huffman document used by the deflate test: a heading, one
/// paragraph with a bold run, and a one-row table.
const DEFLATED_XML: &str = concat!(
    "<?xml version=\"1.0\"?><w:document><w:body>",
    "<w:p><w:pPr><w:pStyle w:val=\"Heading1\"/></w:pPr>",
    "<w:r><w:t>Deflated</w:t></w:r></w:p>",
    "<w:p><w:r><w:rPr><w:b/></w:rPr>",
    "<w:t>bold text survives a dynamic Huffman block</w:t></w:r></w:p>",
    "<w:tbl><w:tr><w:tc><w:p><w:r><w:t>A</w:t></w:r></w:p></w:tc>",
    "<w:tc><w:p><w:r><w:t>B</w:t></w:r></w:p></w:tc></w:tr></w:tbl>",
    "</w:body></w:document>",
);

/// Raw deflate, one dynamic-Huffman block (BFINAL=1, BTYPE=2).
const DEFLATED_STREAM: &[u8] = &[
    0x75, 0x90, 0x4d, 0x0e, 0x82, 0x30, 0x10, 0x85, 0xaf, 0xd2, 0xf4, 0x00, 0x56, 0xb6, 0x06, 0x4a,
    0x34, 0x2e, 0x58, 0x9a, 0x78, 0x82, 0xfe, 0x61, 0x1a, 0xfb, 0x63, 0xca, 0x50, 0xe0, 0xf6, 0x86,
    0x16, 0x88, 0x46, 0xdc, 0xcc, 0x9b, 0xc9, 0x7c, 0xf3, 0x5e, 0x32, 0x65, 0x3d, 0x5a, 0x83, 0xa2,
    0x0a, 0x9d, 0xf6, 0xae, 0xc2, 0xc5, 0xe1, 0x88, 0x6b, 0x5a, 0x0e, 0x27, 0xe9, 0x45, 0x6f, 0x95,
    0x83, 0xb9, 0xe7, 0x5e, 0x4e, 0xb3, 0xbe, 0x52, 0xb9, 0x85, 0x24, 0x77, 0x98, 0x8c, 0x42, 0xc3,
    0x29, 0x32, 0x53, 0xe1, 0x46, 0x31, 0xa9, 0xdd, 0xa3, 0xc0, 0x84, 0x96, 0x64, 0x63, 0x52, 0x01,
    0x7a, 0x55, 0xad, 0x61, 0xa0, 0xe4, 0xbc, 0x81, 0xb4, 0x0f, 0x99, 0xda, 0x3c, 0x33, 0x9d, 0x8f,
    0x78, 0xb6, 0x58, 0x26, 0xa0, 0xdc, 0x1b, 0x89, 0x40, 0x8d, 0x80, 0xba, 0x3e, 0x44, 0x1d, 0x55,
    0x87, 0x18, 0x92, 0x93, 0x63, 0x56, 0x0b, 0xd4, 0xf4, 0x6d, 0x6b, 0x99, 0x43, 0xdc, 0x78, 0xf1,
    0xdc, 0xf5, 0x07, 0x6e, 0x92, 0x64, 0x37, 0xf1, 0x1d, 0x09, 0xf4, 0xbc, 0x73, 0x44, 0x56, 0xf0,
    0x17, 0xbf, 0xfc, 0xc7, 0x49, 0x0e, 0x21, 0x4b, 0x24, 0x59, 0xdf, 0x46, 0x3e, 0x7e, 0xf9, 0x06,
];

fn plain(document: &Document) -> String {
    let mut out = String::new();
    for block in &document.blocks {
        match block {
            Block::Heading(node) => out.push_str(&text_of(&node.inlines)),
            Block::Paragraph(node) => out.push_str(&text_of(&node.inlines)),
            Block::List(node) => {
                for item in &node.items {
                    if let Some(Block::Paragraph(paragraph)) = item.blocks.first() {
                        out.push_str(&text_of(&paragraph.inlines));
                    }
                }
            }
            _ => {}
        }
        out.push('\n');
    }
    out
}

fn text_of(inlines: &[Inline]) -> String {
    let mut out = String::new();
    for inline in inlines {
        match inline {
            Inline::Text(node) => out.push_str(&node.value),
            Inline::Strong(node)
            | Inline::Emphasis(node)
            | Inline::Underline(node)
            | Inline::Superscript(node)
            | Inline::Subscript(node) => out.push_str(&text_of(&node.children)),
            Inline::Link(node) => out.push_str(&text_of(&node.children)),
            Inline::LineBreak(_) => out.push('\n'),
            _ => {}
        }
    }
    out
}

fn sample() -> Document {
    parse_docx(&store(&[
        ("word/document.xml", DOCUMENT_XML.as_bytes()),
        ("docProps/core.xml", CORE_XML.as_bytes()),
    ]))
    .expect("a readable sample")
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[test]
fn a_stored_archive_becomes_a_valid_document() {
    let document = sample();
    document.validate().expect("valid DocIR");
    assert_eq!(document.blocks.len(), 5);

    match &document.blocks[0] {
        Block::Heading(heading) => {
            assert_eq!(heading.level, 1);
            assert_eq!(text_of(&heading.inlines), "Heading");
        }
        other => panic!("expected a heading, got {other:?}"),
    }

    match &document.blocks[1] {
        Block::Paragraph(paragraph) => {
            assert_eq!(paragraph.inlines.len(), 5);
            assert!(
                matches!(paragraph.inlines[0], Inline::Text(ref text) if text.value == "Hello ")
            );
            assert!(matches!(paragraph.inlines[1], Inline::Strong(_)));
            assert!(matches!(paragraph.inlines[2], Inline::Emphasis(_)));
            assert!(matches!(paragraph.inlines[3], Inline::Superscript(_)));
            assert!(matches!(paragraph.inlines[4], Inline::Underline(_)));
            assert_eq!(text_of(&paragraph.inlines), "Hello bolditalic2under");
        }
        other => panic!("expected a paragraph, got {other:?}"),
    }

    match &document.blocks[2] {
        Block::Table(table) => {
            assert_eq!(table.columns.len(), 2);
            assert_eq!(table.rows.len(), 2);
            assert!(!table.rows[0].header);
            assert_eq!(table.rows[0].cells.len(), 2);
            match &table.rows[0].cells[0].blocks[0] {
                Block::Paragraph(paragraph) => assert_eq!(text_of(&paragraph.inlines), "A"),
                other => panic!("expected a cell paragraph, got {other:?}"),
            }
            match &table.rows[1].cells[1].blocks[0] {
                Block::Paragraph(paragraph) => assert_eq!(text_of(&paragraph.inlines), "2"),
                other => panic!("expected a cell paragraph, got {other:?}"),
            }
        }
        other => panic!("expected a table, got {other:?}"),
    }

    match &document.blocks[3] {
        Block::List(list) => {
            assert!(list.ordered);
            assert_eq!(list.start, 1);
            assert_eq!(list.items.len(), 2);
            match &list.items[0].blocks[0] {
                Block::Paragraph(paragraph) => assert_eq!(text_of(&paragraph.inlines), "one"),
                other => panic!("expected a list paragraph, got {other:?}"),
            }
            match &list.items[1].blocks[0] {
                Block::Paragraph(paragraph) => assert_eq!(text_of(&paragraph.inlines), "two"),
                other => panic!("expected a list paragraph, got {other:?}"),
            }
        }
        other => panic!("expected a list, got {other:?}"),
    }

    match &document.blocks[4] {
        Block::Paragraph(paragraph) => {
            // `w:b w:val="0"` must not produce Strong; tab is a space and br is
            // a hard break.
            assert!(
                matches!(paragraph.inlines[0], Inline::Text(ref text) if text.value == "plain")
            );
            assert_eq!(paragraph.inlines.len(), 4);
            assert!(matches!(paragraph.inlines[1], Inline::Text(ref text) if text.value == " "));
            assert!(matches!(paragraph.inlines[2], Inline::LineBreak(_)));
        }
        other => panic!("expected a paragraph, got {other:?}"),
    }

    assert!(!plain(&document).contains("dropped"));
}

#[test]
fn metadata_is_read_from_core_xml() {
    let document = sample();
    assert_eq!(document.title_text().as_deref(), Some("Sample Title"));
    assert_eq!(
        document
            .meta
            .cover
            .as_ref()
            .and_then(|cover| cover.title.as_deref()),
        Some("Sample Title")
    );
    let authors: Vec<&str> = document
        .meta
        .authors
        .iter()
        .map(|author| author.name.as_str())
        .collect();
    assert_eq!(authors, ["Alice", "Bob", "Carol"]);
}

#[test]
fn missing_metadata_stays_empty() {
    let archive = store(&[("word/document.xml", DOCUMENT_XML.as_bytes())]);
    let document = parse_docx(&archive).expect("a readable document");
    assert!(document.meta.title.is_none());
    assert!(document.meta.cover.is_none());
    assert!(document.meta.authors.is_empty());
    document.validate().expect("valid DocIR");
}

#[test]
fn a_deflated_archive_is_inflated() {
    let archive = build_zip(&[Entry {
        name: "word/document.xml",
        method: 8,
        data: DEFLATED_XML.as_bytes(),
        compressed: DEFLATED_STREAM,
    }]);
    let document = parse_docx(&archive).expect("a readable deflated document");
    document.validate().expect("valid DocIR");

    assert_eq!(document.blocks.len(), 3);
    match &document.blocks[0] {
        Block::Heading(heading) => assert_eq!(text_of(&heading.inlines), "Deflated"),
        other => panic!("expected a heading, got {other:?}"),
    }
    match &document.blocks[1] {
        Block::Paragraph(paragraph) => {
            assert!(matches!(paragraph.inlines[0], Inline::Strong(_)));
            assert_eq!(
                text_of(&paragraph.inlines),
                "bold text survives a dynamic Huffman block"
            );
        }
        other => panic!("expected a paragraph, got {other:?}"),
    }
    match &document.blocks[2] {
        Block::Table(table) => {
            assert_eq!(table.columns.len(), 2);
            assert_eq!(table.rows.len(), 1);
        }
        other => panic!("expected a table, got {other:?}"),
    }
}

#[test]
fn canonical_json_is_stable() {
    let document = sample();
    let first = document.canonical_json();
    let second = document.canonical_json();
    assert_eq!(first, second);
    assert!(first.contains("\"schema\":\"soma.doc/2\""));
}

#[test]
fn broken_archives_are_errors_not_panics() {
    assert!(parse_docx(&[]).is_err());
    assert!(parse_docx(b"this is definitely not a zip").is_err());

    let missing = store(&[("word/other.xml", b"<w:document/>")]);
    assert!(parse_docx(&missing).is_err());

    let truncated = store(&[("word/document.xml", b"<w:document><w:body><w:p")]);
    assert!(parse_docx(&truncated).is_err());

    let unclosed = store(&[(
        "word/document.xml",
        b"<w:document><w:body><w:p><w:r><w:t>oops",
    )]);
    assert!(parse_docx(&unclosed).is_err());

    let not_utf8 = store(&[("word/document.xml", &[0xff, 0xfe, 0x00, 0x00])]);
    assert!(parse_docx(&not_utf8).is_err());

    let empty_xml = store(&[("word/document.xml", b"<w:document/>")]);
    assert!(parse_docx(&empty_xml).is_err());
}

#[test]
fn run_styles_and_hyperlinks_are_mapped() {
    let xml = br#"<w:document><w:body><w:p>
        <w:r><w:rPr><w:rStyle w:val="Strong"/></w:rPr><w:t>a</w:t></w:r>
        <w:r><w:rPr><w:rStyle w:val="Emphasis"/></w:rPr><w:t>b</w:t></w:r>
        <w:r><w:rPr><w:rStyle w:val="Hyperlink"/></w:rPr><w:t>c</w:t></w:r>
        <w:r><w:rPr><w:u w:val="none"/></w:rPr><w:t>d</w:t></w:r>
        <w:r><w:rPr><w:vertAlign w:val="subscript"/></w:rPr><w:t>e</w:t></w:r>
        <w:hyperlink r:id="rId7"><w:r><w:t>f</w:t></w:r></w:hyperlink>
    </w:p></w:body></w:document>"#;
    let archive = store(&[("word/document.xml", xml)]);
    let document = parse_docx(&archive).expect("a readable document");
    document.validate().expect("valid DocIR");

    match &document.blocks[0] {
        Block::Paragraph(paragraph) => {
            assert_eq!(paragraph.inlines.len(), 6);
            assert!(matches!(paragraph.inlines[0], Inline::Strong(_)));
            assert!(matches!(paragraph.inlines[1], Inline::Emphasis(_)));
            // Hyperlink styling and w:hyperlink both stay text; no fabricated href.
            assert!(matches!(paragraph.inlines[2], Inline::Text(ref text) if text.value == "c"));
            assert!(matches!(paragraph.inlines[3], Inline::Text(ref text) if text.value == "d"));
            assert!(matches!(paragraph.inlines[4], Inline::Subscript(_)));
            assert!(matches!(paragraph.inlines[5], Inline::Text(ref text) if text.value == "f"));
            assert_eq!(text_of(&paragraph.inlines), "abcdef");
        }
        other => panic!("expected a paragraph, got {other:?}"),
    }
}

#[test]
fn ragged_tables_and_unordered_lists_stay_valid() {
    let xml = br#"<w:document><w:body>
        <w:tbl>
            <w:tr><w:trPr><w:tblHeader/></w:trPr>
                <w:tc><w:p><w:r><w:t>A</w:t></w:r></w:p></w:tc>
                <w:tc><w:p><w:r><w:t>B</w:t></w:r></w:p></w:tc>
            </w:tr>
            <w:tr><w:tc><w:p><w:r><w:t>1</w:t></w:r></w:p></w:tc></w:tr>
        </w:tbl>
        <w:p><w:pPr><w:numPr><w:ilvl w:val="0"/></w:numPr></w:pPr><w:r><w:t>bullet one</w:t></w:r></w:p>
        <w:p><w:pPr><w:numPr><w:ilvl w:val="0"/></w:numPr></w:pPr><w:r><w:t>bullet two</w:t></w:r></w:p>
    </w:body></w:document>"#;
    let archive = store(&[("word/document.xml", xml)]);
    let document = parse_docx(&archive).expect("a readable document");
    document.validate().expect("valid DocIR");

    match &document.blocks[0] {
        Block::Table(table) => {
            assert_eq!(table.columns.len(), 2);
            assert!(table.rows[0].header);
            // The short row is padded to the first row's width.
            assert_eq!(table.rows[1].cells.len(), 2);
            assert!(table.rows[1].cells[1].blocks.is_empty());
        }
        other => panic!("expected a table, got {other:?}"),
    }

    match &document.blocks[1] {
        Block::List(list) => {
            assert!(!list.ordered, "a numPr without numId cannot be ordered");
            assert_eq!(list.start, 1);
            assert_eq!(list.items.len(), 2);
        }
        other => panic!("expected a list, got {other:?}"),
    }
}

#[test]
fn empty_paragraphs_and_heading_spellings_are_tolerated() {
    let xml = br#"<w:document><w:body>
        <w:p><w:pPr><w:pStyle w:val="heading 3"/></w:pPr></w:p>
        <w:p><w:pPr><w:pStyle w:val="Title"/></w:pPr><w:r><w:t>Title</w:t></w:r></w:p>
        <w:p><w:pPr><w:pStyle w:val="Heading9"/></w:pPr><w:r><w:t>Deep</w:t></w:r></w:p>
        <w:p/>
    </w:body></w:document>"#;
    let archive = store(&[("word/document.xml", xml)]);
    let document = parse_docx(&archive).expect("a readable document");

    match &document.blocks[0] {
        Block::Heading(heading) => assert_eq!(heading.level, 3),
        other => panic!("expected a heading, got {other:?}"),
    }
    match &document.blocks[1] {
        Block::Heading(heading) => assert_eq!(heading.level, 1),
        other => panic!("expected a heading, got {other:?}"),
    }
    match &document.blocks[2] {
        Block::Heading(heading) => assert_eq!(heading.level, 6, "clamped to DocIR's maximum"),
        other => panic!("expected a heading, got {other:?}"),
    }
    assert!(matches!(document.blocks[3], Block::Paragraph(_)));
    document.validate().expect("valid DocIR");
}

// ---------------------------------------------------------------------------
// Stylesheet fixtures
// ---------------------------------------------------------------------------

const STYLES_XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:style w:type="paragraph" w:styleId="Heading1">
    <w:name w:val="heading 1"/>
    <w:basedOn w:val="BodyItalic"/>
    <w:rPr><w:b/></w:rPr>
  </w:style>
  <w:style w:type="paragraph" w:styleId="BodyItalic">
    <w:name w:val="Body Italic"/>
    <w:rPr><w:i/></w:rPr>
  </w:style>
  <w:style w:type="paragraph" w:styleId="FancyHeading">
    <w:name w:val="heading 3"/>
    <w:rPr><w:b/></w:rPr>
  </w:style>
  <w:style w:type="paragraph" w:styleId="BodyStrong">
    <w:name w:val="Body Strong"/>
    <w:rPr><w:b/></w:rPr>
  </w:style>
  <w:style w:type="character" w:styleId="StrongChar">
    <w:name w:val="Strong Char"/>
    <w:basedOn w:val="BaseChar"/>
    <w:rPr><w:i/></w:rPr>
  </w:style>
  <w:style w:type="character" w:styleId="BaseChar">
    <w:name w:val="Base Char"/>
    <w:rPr><w:u/></w:rPr>
  </w:style>
  <w:style w:type="character" w:styleId="NotItalic">
    <w:name w:val="Not Italic"/>
    <w:basedOn w:val="StrongChar"/>
    <w:rPr><w:i w:val="0"/></w:rPr>
  </w:style>
  <w:style w:type="character" w:styleId="CycleA">
    <w:name w:val="Cycle A"/>
    <w:basedOn w:val="CycleB"/>
    <w:rPr><w:b/></w:rPr>
  </w:style>
  <w:style w:type="character" w:styleId="CycleB">
    <w:name w:val="Cycle B"/>
    <w:basedOn w:val="CycleA"/>
    <w:rPr><w:i/></w:rPr>
  </w:style>
</w:styles>"#;

const STYLED_DOCUMENT_XML: &str = r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:body>
    <w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:r><w:t>Styled Heading</w:t></w:r></w:p>
    <w:p><w:pPr><w:pStyle w:val="FancyHeading"/></w:pPr><w:r><w:t>Third Level</w:t></w:r></w:p>
    <w:p><w:pPr><w:pStyle w:val="BodyStrong"/></w:pPr>
      <w:r><w:t>all bold</w:t></w:r>
      <w:r><w:rPr><w:b w:val="0"/></w:rPr><w:t>off</w:t></w:r>
    </w:p>
    <w:p>
      <w:r><w:rPr><w:rStyle w:val="StrongChar"/></w:rPr><w:t>char</w:t></w:r>
      <w:r><w:rPr><w:rStyle w:val="NotItalic"/></w:rPr><w:t>not italic</w:t></w:r>
      <w:r><w:rPr><w:rStyle w:val="CycleA"/></w:rPr><w:t>cycle</w:t></w:r>
    </w:p>
  </w:body>
</w:document>"#;

fn expect_figure(block: &Block) -> &Figure {
    match block {
        Block::Figure(figure) => figure,
        other => panic!("expected a figure, got {other:?}"),
    }
}

#[test]
fn stylesheets_drive_headings_and_run_formatting() {
    let files: [(&str, &[u8]); 2] = [
        ("word/document.xml", STYLED_DOCUMENT_XML.as_bytes()),
        ("word/styles.xml", STYLES_XML.as_bytes()),
    ];

    for archive in [store(&files), deflated(&files)] {
        let document = parse_docx(&archive).expect("a readable styled document");
        document.validate().expect("valid DocIR");
        assert_eq!(document.blocks.len(), 4);

        // `Heading1` is recognised by its style *name*, and inherits italic
        // from its basedOn ancestor while adding bold of its own.
        match &document.blocks[0] {
            Block::Heading(heading) => {
                assert_eq!(heading.level, 1);
                assert_eq!(text_of(&heading.inlines), "Styled Heading");
                match heading.inlines.first() {
                    Some(Inline::Strong(strong)) => {
                        assert!(matches!(strong.children.first(), Some(Inline::Emphasis(_))));
                    }
                    other => panic!("expected strong emphasis, got {other:?}"),
                }
            }
            other => panic!("expected a heading, got {other:?}"),
        }

        match &document.blocks[1] {
            Block::Heading(heading) => assert_eq!(heading.level, 3),
            other => panic!("expected a heading, got {other:?}"),
        }

        // The paragraph style makes every run bold, but a run-level `w:b`
        // with `w:val="0"` turns it off.
        match &document.blocks[2] {
            Block::Paragraph(paragraph) => {
                assert!(matches!(paragraph.inlines.first(), Some(Inline::Strong(_))));
                assert!(matches!(
                    paragraph.inlines.get(1),
                    Some(Inline::Text(text)) if text.value == "off"
                ));
            }
            other => panic!("expected a paragraph, got {other:?}"),
        }

        // Character styles inherit through basedOn; an explicit off wins; and
        // a basedOn cycle terminates instead of looping forever.
        match &document.blocks[3] {
            Block::Paragraph(paragraph) => {
                match paragraph.inlines.first() {
                    Some(Inline::Emphasis(emphasis)) => {
                        assert!(matches!(
                            emphasis.children.first(),
                            Some(Inline::Underline(_))
                        ));
                    }
                    other => panic!("expected emphasis underline, got {other:?}"),
                }
                assert!(matches!(
                    paragraph.inlines.get(1),
                    Some(Inline::Underline(_))
                ));
                match paragraph.inlines.get(2) {
                    Some(Inline::Strong(strong)) => {
                        assert!(matches!(strong.children.first(), Some(Inline::Emphasis(_))));
                    }
                    other => panic!("expected strong emphasis, got {other:?}"),
                }
            }
            other => panic!("expected a paragraph, got {other:?}"),
        }

        assert_eq!(document.canonical_json(), document.canonical_json());
    }
}

// ---------------------------------------------------------------------------
// Image fixtures
// ---------------------------------------------------------------------------

const IMAGE_DOCUMENT_XML: &str = r#"<w:document
    xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"
    xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"
    xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing"
    xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"
    xmlns:pic="http://schemas.openxmlformats.org/drawingml/2006/picture"
    xmlns:v="urn:schemas-microsoft-com:vml">
  <w:body>
    <w:p>
      <w:r><w:t>before</w:t></w:r>
      <w:r><w:drawing><wp:inline>
        <wp:extent cx="914400" cy="457200"/>
        <a:graphic><a:graphicData><pic:pic><pic:blipFill>
          <a:blip r:embed="rId10"/>
        </pic:blipFill></pic:pic></a:graphicData></a:graphic>
      </wp:inline></w:drawing></w:r>
      <w:r><w:t>after</w:t></w:r>
    </w:p>
    <w:p>
      <w:r><w:drawing><wp:anchor>
        <wp:extent cx="1905000" cy="952500"/>
        <a:graphic><a:graphicData><pic:pic><pic:blipFill>
          <a:blip r:embed="rId11"/>
        </pic:blipFill></pic:pic></a:graphicData></a:graphic>
      </wp:anchor></w:drawing></w:r>
    </w:p>
    <w:p>
      <w:r><w:pict><v:shape><v:imagedata r:id="rId12"/></v:shape></w:pict></w:r>
    </w:p>
  </w:body>
</w:document>"#;

const IMAGE_RELS_XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId10" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="media/image1.png"/>
  <Relationship Id="rId11" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="/word/media/image2.jpg"/>
  <Relationship Id="rId12" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="media/image1.png"/>
  <Relationship Id="rId13" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink" Target="https://example.com"/>
</Relationships>"#;

/// A complete 1x1 transparent PNG.
const PNG_1X1: &[u8] = &[
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1f, 0x15, 0xc4,
    0x89, 0x00, 0x00, 0x00, 0x0a, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x63, 0x00, 0x01, 0x00, 0x00,
    0x05, 0x00, 0x01, 0x0d, 0x0a, 0x2d, 0xb4, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae,
    0x42, 0x60, 0x82,
];

/// Arbitrary bytes that start and end like a JPEG; the reader never decodes
/// them, so only the bytes matter.
const JPEG_HEADER: &[u8] = &[
    0xff, 0xd8, 0xff, 0xe0, 0x00, 0x10, 0x4a, 0x46, 0x49, 0x46, 0x00, 0x01, 0xff, 0xd9,
];

#[test]
fn drawings_become_figures_and_media_round_trips() {
    let files: [(&str, &[u8]); 4] = [
        ("word/document.xml", IMAGE_DOCUMENT_XML.as_bytes()),
        ("word/_rels/document.xml.rels", IMAGE_RELS_XML.as_bytes()),
        ("word/media/image1.png", PNG_1X1),
        ("word/media/image2.jpg", JPEG_HEADER),
    ];

    for archive in [store(&files), deflated(&files)] {
        let (document, media) =
            parse_docx_with_media(&archive).expect("a readable illustrated document");
        document.validate().expect("valid DocIR");
        assert_eq!(document.blocks.len(), 5);

        match &document.blocks[0] {
            Block::Paragraph(paragraph) => assert_eq!(text_of(&paragraph.inlines), "before"),
            other => panic!("expected text before the image, got {other:?}"),
        }

        let first = expect_figure(&document.blocks[1]);
        assert_eq!(first.image.source.src, "media/image1.png");
        assert_eq!(first.image.source.alt, None);
        assert_eq!(
            first.image.source.intrinsic,
            Some(ImageIntrinsic {
                width_px: 96,
                height_px: 48,
            })
        );

        match &document.blocks[2] {
            Block::Paragraph(paragraph) => assert_eq!(text_of(&paragraph.inlines), "after"),
            other => panic!("expected text after the image, got {other:?}"),
        }

        // The second paragraph contains only an anchored image: no empty text
        // block surrounds it. 1905000/9525 = 200, 952500/9525 = 100.
        let second = expect_figure(&document.blocks[3]);
        assert_eq!(second.image.source.src, "media/image2.jpg");
        assert_eq!(
            second.image.source.intrinsic,
            Some(ImageIntrinsic {
                width_px: 200,
                height_px: 100,
            })
        );

        // Legacy VML `v:imagedata` resolves through the same relationship
        // table and keeps the paragraph image-only.
        let third = expect_figure(&document.blocks[4]);
        assert_eq!(third.image.source.src, "media/image1.png");
        assert_eq!(third.image.source.intrinsic, None);

        assert_eq!(media.len(), 2);
        assert_eq!(media[0].name, "media/image1.png");
        assert_eq!(media[0].bytes.as_slice(), PNG_1X1);
        assert_eq!(media[1].name, "media/image2.jpg");
        assert_eq!(media[1].bytes.as_slice(), JPEG_HEADER);

        // `parse_docx` keeps its signature and drops media but not figures.
        let plain_document = parse_docx(&archive).expect("a readable document");
        assert_eq!(plain_document.blocks, document.blocks);

        assert_eq!(document.canonical_json(), document.canonical_json());
    }
}

#[test]
fn missing_styles_relationships_and_media_degrade() {
    let drawing = br#"<w:document
        xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"
        xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"
        xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing"
        xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main">
      <w:body>
        <w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:r><w:t>Fallback</w:t></w:r></w:p>
        <w:p><w:r><w:t>kept</w:t></w:r><w:r><w:drawing><wp:inline>
          <wp:extent cx="914400" cy="914400"/>
          <a:graphic><a:graphicData><a:blip r:embed="rId1"/></a:graphicData></a:graphic>
        </wp:inline></w:drawing></w:r></w:p>
      </w:body>
    </w:document>"#;

    // No styles part and no relationships part at all.
    let archive = store(&[("word/document.xml", drawing)]);
    let (document, media) =
        parse_docx_with_media(&archive).expect("missing optional parts are not fatal");
    document.validate().expect("valid DocIR");
    assert!(media.is_empty());
    assert!(matches!(document.blocks[0], Block::Heading(_)));
    assert!(!document
        .blocks
        .iter()
        .any(|block| matches!(block, Block::Figure(_))));
    match &document.blocks[1] {
        Block::Paragraph(paragraph) => assert_eq!(text_of(&paragraph.inlines), "kept"),
        other => panic!("expected the text to survive, got {other:?}"),
    }

    // The relationship exists, but its target has no zip entry.
    let rels = br#"<Relationships>
      <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="media/missing.png"/>
    </Relationships>"#;
    let archive = store(&[
        ("word/document.xml", drawing),
        ("word/_rels/document.xml.rels", rels),
    ]);
    let (document, media) =
        parse_docx_with_media(&archive).expect("a dangling relationship is not fatal");
    document.validate().expect("valid DocIR");
    assert!(media.is_empty());
    assert!(!document
        .blocks
        .iter()
        .any(|block| matches!(block, Block::Figure(_))));
}
