//! Word (`.docx`) reader.
//!
//! A `.docx` is a zip archive of OOXML parts. This crate reads the archive and
//! the subset of WordprocessingML that maps one-to-one onto DocIR. It is
//! import-only: nothing here knows about templates, stylesheets, page geometry,
//! or Word's own layout.
//!
//! Supported (all under `w:body`):
//!
//! - `w:p`, `w:pPr`, `w:pStyle` (`Heading1`..`Heading9`, `heading 1`, `Title`),
//!   `w:numPr`, `w:numId`;
//! - `w:r`, `w:rPr`, `w:t` (with `xml:space="preserve"`), `w:tab`, `w:br`,
//!   `w:cr`, `w:b`, `w:i`, `w:u`, `w:vertAlign`, `w:rStyle`
//!   (`Strong`/`Emphasis`/`Hyperlink`);
//! - `w:hyperlink` (children are kept as text; the relationship target is not
//!   loaded), `w:tbl`, `w:tr`, `w:tc`, `w:trPr`, `w:tblHeader`;
//! - `docProps/core.xml`: `dc:title` and `dc:creator`.
//!
//! Everything else is skipped. A skipped element never turns into an error and
//! never inserts fabricated content. The archive reader accepts stored and
//! deflated entries; the deflate decoder covers all three RFC 1951 block types.
//!
//! Not supported: style-sheet inheritance (`styles.xml`), numbering definitions
//! (`numbering.xml`, so a list is ordered iff `w:numId` is present), list
//! continuation across a non-list block, images, footnotes/endnotes, comments,
//! fields, math, tracked-change semantics, floating objects, and any layout or
//! typography from Word. Headings deeper than 6 are clamped to 6 because DocIR
//! requires `1..=6`.

#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]

extern crate alloc;

mod docx;
mod inflate;
mod xml;
mod zip;

use soma_doc::{Document, DocumentMeta, SourceId, SourceInfo, SourceKind, SourceMap};

/// Format id used in diagnostics and capability tables.
pub const FORMAT_ID: &str = "docx";

/// Syntax id used in diagnostics and capability tables.
pub const SYNTAX_ID: &str = "ooxml";

/// Why a `.docx` could not be read.
///
/// The reason is a short, stable, non-localized string so callers can log it
/// without allocating. `reason` is the complete public surface: an import
/// either produces a validated document or one of these static reasons.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DocxError {
    pub reason: &'static str,
}

impl DocxError {
    pub const fn new(reason: &'static str) -> Self {
        Self { reason }
    }
}

impl core::fmt::Display for DocxError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter.write_str(self.reason)
    }
}

impl core::error::Error for DocxError {}

/// Parse a `.docx` archive into DocIR.
///
/// The whole archive is read from memory; no filesystem, command, or external
/// library is involved. `word/document.xml` is required, `docProps/core.xml` is
/// optional, and a missing or unreadable metadata part simply leaves the
/// metadata empty.
///
/// The returned document is renumbered and is expected to pass
/// [`Document::validate`]; the import never manufactures ids or spans.
pub fn parse_docx(bytes: &[u8]) -> Result<Document, DocxError> {
    let archive = zip::Archive::new(bytes)?;
    let document_xml = archive
        .read(b"word/document.xml")?
        .ok_or_else(|| DocxError::new("missing word/document.xml"))?;
    let document_xml = core::str::from_utf8(&document_xml)
        .map_err(|_| DocxError::new("word/document.xml is not UTF-8"))?;
    let blocks = docx::parse_blocks(document_xml)?;

    let mut meta = DocumentMeta::new();
    // Metadata is decoration: a broken core.xml must not make an otherwise
    // readable document fail, so both the archive read and the XML walk are
    // best-effort here.
    if let Some(core) = archive.read(b"docProps/core.xml").ok().flatten() {
        if let Ok(core_xml) = core::str::from_utf8(&core) {
            meta = docx::parse_core_metadata(core_xml);
        }
    }

    let mut document = Document::new(meta);
    document.blocks = blocks;
    // Nodes are synthesized (no byte spans), but naming the part keeps the
    // provenance of the document itself explicit.
    let mut source_map = SourceMap::new();
    source_map.push(SourceInfo::new(
        SourceId(1),
        "word/document.xml",
        SourceKind::Word,
    ));
    document.source_map = source_map;
    document.renumber();
    Ok(document)
}
