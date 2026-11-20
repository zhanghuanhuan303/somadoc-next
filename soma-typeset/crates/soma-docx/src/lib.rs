//! Word (`.docx`) reader.
//!
//! A `.docx` is a zip archive of OOXML parts. This crate reads the archive and
//! the subset of WordprocessingML that maps one-to-one onto DocIR. It is
//! import-only: nothing here knows about templates, page geometry, or Word's own
//! layout.
//!
//! Supported (all under `w:body`):
//!
//! - `w:p`, `w:pPr`, `w:pStyle` (`Heading1`..`Heading9`, `heading 1`, `Title`),
//!   `w:numPr`, `w:numId`;
//! - `w:r`, `w:rPr`, `w:t` (with `xml:space="preserve"`), `w:tab`, `w:br`,
//!   `w:cr`, `w:b`, `w:i`, `w:u`, `w:vertAlign`, `w:rStyle`;
//! - `word/styles.xml` paragraph and character styles, including `w:basedOn`
//!   chains (cycles stop at the first repeated style);
//! - `w:hyperlink` (children are kept as text; the relationship target is not
//!   loaded), `w:tbl`, `w:tr`, `w:tc`, `w:trPr`, `w:tblHeader`;
//! - `word/_rels/document.xml.rels` image relationships, `w:drawing` (inline
//!   or anchored) and `w:pict`/`v:imagedata`, with size taken from
//!   `wp:extent`; each image becomes a `Block::Figure`;
//! - `docProps/core.xml`: `dc:title` and `dc:creator`.
//!
//! Everything else is skipped. A skipped element never turns into an error and
//! never inserts fabricated content. The archive reader accepts stored and
//! deflated entries; the deflate decoder covers all three RFC 1951 block types.
//!
//! Not supported: numbering definitions (`numbering.xml`, so a list is ordered
//! iff `w:numId` is present), list continuation across a non-list block,
//! footnotes/endnotes, comments, fields, math, tracked-change semantics,
//! floating position and text wrapping (an anchored image is imported as an
//! ordinary figure), image cropping or format detection (raw bytes are handed
//! to the host), theme colours, and any layout or typography from Word.
//! Headings deeper than 6 are clamped to 6 because DocIR requires `1..=6`.

#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

mod docx;
mod inflate;
mod media;
mod styles;
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

/// One image part referenced by the document.
///
/// `name` is the zip-internal path relative to `word/` (for example
/// `media/image1.png`), identical to the [`soma_doc::meta::ImageRef::src`] of
/// the figure that uses it. `bytes` are handed to the host unchanged: this
/// crate does not decode or validate image formats.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MediaFile {
    pub name: String,
    pub bytes: Vec<u8>,
}

/// Parse a `.docx` archive into DocIR and its referenced media.
///
/// The whole archive is read from memory; no filesystem, command, or external
/// library is involved. `word/document.xml` is required; `word/styles.xml`,
/// `word/_rels/document.xml.rels`, `word/media/*`, and `docProps/core.xml` are
/// optional. A missing or unreadable style, relationship, media, or metadata
/// part degrades to the corresponding absence, never to a failed import.
///
/// The returned document is renumbered and is expected to pass
/// [`Document::validate`]; the import never manufactures ids or spans.
pub fn parse_docx_with_media(bytes: &[u8]) -> Result<(Document, Vec<MediaFile>), DocxError> {
    let archive = zip::Archive::new(bytes)?;
    let document_xml = archive
        .read(b"word/document.xml")?
        .ok_or_else(|| DocxError::new("missing word/document.xml"))?;
    let document_xml = core::str::from_utf8(&document_xml)
        .map_err(|_| DocxError::new("word/document.xml is not UTF-8"))?;

    let styles = styles::Styles::load(&archive);
    let (images, media) = media::load(&archive);
    let blocks = docx::parse_blocks(document_xml, &styles, &images)?;

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
    Ok((document, media))
}

/// Parse a `.docx` archive into DocIR, discarding referenced media.
pub fn parse_docx(bytes: &[u8]) -> Result<Document, DocxError> {
    parse_docx_with_media(bytes).map(|(document, _)| document)
}
