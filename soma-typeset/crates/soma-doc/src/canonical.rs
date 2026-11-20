//! Canonical JSON for DocIR.
//!
//! The format is part of the contract: field order is fixed, numbers are
//! integers, and no map-backed serializer may be introduced. Two documents with
//! equal canonical bytes are the same document for every downstream stage.
//!
//! Two modes exist:
//!
//! - [`CanonicalMode::Full`] writes ids, spans, the source map, and the template
//!   reference. This is the reproducibility form.
//! - [`CanonicalMode::Content`] normalises ids and spans to zero, empties the
//!   source map, and drops the template reference. This is the content-identity
//!   form used for caching.

use alloc::string::String;
use core::fmt::{self, Write};

use crate::block::{Block, GeneratedBlock, List, Row, Section, Table};
use crate::doc::{Document, FootnoteDef, DOC_IR_VERSION, SCHEMA};
use crate::inline::Inline;
use crate::meta::{CoverData, DocumentMeta, ImageRef, MetaField};
use crate::node::NodeMeta;
use crate::raw::RawContent;

/// Which canonical form to emit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CanonicalMode {
    /// Ids, spans, source map, and template reference included.
    Full,
    /// Provenance normalised away; semantic content only.
    Content,
}

pub(crate) fn canonical_json(doc: &Document, mode: CanonicalMode) -> String {
    let mut out = String::new();
    write_document(&mut out, doc, mode).expect("writing canonical JSON into a String cannot fail");
    out
}

pub(crate) fn write_document(
    out: &mut impl Write,
    doc: &Document,
    mode: CanonicalMode,
) -> fmt::Result {
    out.write_str("{\"schema\":\"")?;
    out.write_str(SCHEMA)?;
    write!(out, "\",\"docIrVersion\":{}", DOC_IR_VERSION)?;
    write!(out, ",\"nodeCount\":{}", doc.node_count())?;
    out.write_str(",\"meta\":")?;
    write_document_meta(out, &doc.meta, mode)?;
    out.write_str(",\"blocks\":")?;
    write_blocks(out, &doc.blocks, mode)?;
    out.write_str(",\"footnotes\":")?;
    write_footnotes(out, &doc.footnotes, mode)?;
    out.write_str(",\"sourceMap\":")?;
    write_source_map(out, doc, mode)?;
    out.write_str("}")
}

fn write_source_map(out: &mut impl Write, doc: &Document, mode: CanonicalMode) -> fmt::Result {
    if mode == CanonicalMode::Content {
        return out.write_str("[]");
    }
    out.write_char('[')?;
    for (index, entry) in doc.source_map.entries.iter().enumerate() {
        if index > 0 {
            out.write_char(',')?;
        }
        write!(out, "{{\"id\":{},\"name\":", entry.id.0)?;
        write_string(out, &entry.name)?;
        out.write_str(",\"kind\":\"")?;
        out.write_str(entry.kind.as_str())?;
        out.write_str("\"}")?;
    }
    out.write_char(']')
}

fn write_document_meta(
    out: &mut impl Write,
    meta: &DocumentMeta,
    mode: CanonicalMode,
) -> fmt::Result {
    out.write_str("{\"title\":")?;
    match &meta.title {
        Some(inlines) => write_inlines(out, inlines, mode)?,
        None => out.write_str("null")?,
    }

    out.write_str(",\"authors\":[")?;
    for (index, author) in meta.authors.iter().enumerate() {
        if index > 0 {
            out.write_char(',')?;
        }
        out.write_str("{\"name\":")?;
        write_string(out, &author.name)?;
        out.write_str(",\"affiliation\":")?;
        write_opt_str(out, author.affiliation.as_deref())?;
        out.write_str(",\"email\":")?;
        write_opt_str(out, author.email.as_deref())?;
        out.write_str(",\"orcid\":")?;
        write_opt_str(out, author.orcid.as_deref())?;
        out.write_char('}')?;
    }
    out.write_str("],\"language\":")?;
    write_opt_str(out, meta.language.as_deref())?;

    out.write_str(",\"date\":")?;
    match meta.date {
        Some(date) => write!(
            out,
            "{{\"year\":{},\"month\":{},\"day\":{}}}",
            date.year, date.month, date.day
        )?,
        None => out.write_str("null")?,
    }

    out.write_str(",\"keywords\":[")?;
    for (index, keyword) in meta.keywords.iter().enumerate() {
        if index > 0 {
            out.write_char(',')?;
        }
        write_string(out, keyword)?;
    }
    out.write_str("],\"template\":")?;
    match (&meta.template, mode) {
        (Some(template), CanonicalMode::Full) => {
            out.write_str("{\"name\":")?;
            write_string(out, &template.name)?;
            out.write_str(",\"version\":")?;
            write_opt_str(out, template.version.as_deref())?;
            out.write_char('}')?;
        }
        _ => out.write_str("null")?,
    }

    out.write_str(",\"cover\":")?;
    match &meta.cover {
        Some(cover) => write_cover(out, cover)?,
        None => out.write_str("null")?,
    }
    out.write_str(",\"fields\":")?;
    write_fields(out, &meta.fields)?;
    out.write_char('}')
}

fn write_cover(out: &mut impl Write, cover: &CoverData) -> fmt::Result {
    out.write_str("{\"title\":")?;
    write_opt_str(out, cover.title.as_deref())?;
    out.write_str(",\"subtitle\":")?;
    write_opt_str(out, cover.subtitle.as_deref())?;
    out.write_str(",\"authors\":[")?;
    for (index, author) in cover.authors.iter().enumerate() {
        if index > 0 {
            out.write_char(',')?;
        }
        write_string(out, author)?;
    }
    out.write_str("],\"date\":")?;
    write_opt_str(out, cover.date.as_deref())?;
    out.write_str(",\"logo\":")?;
    match &cover.logo {
        Some(logo) => write_image_ref_object(out, logo)?,
        None => out.write_str("null")?,
    }
    out.write_str(",\"fields\":")?;
    write_fields(out, &cover.fields)?;
    out.write_char('}')
}

fn write_fields(out: &mut impl Write, fields: &[MetaField]) -> fmt::Result {
    out.write_char('[')?;
    for (index, field) in fields.iter().enumerate() {
        if index > 0 {
            out.write_char(',')?;
        }
        out.write_str("{\"key\":")?;
        write_string(out, &field.key)?;
        out.write_str(",\"value\":")?;
        write_string(out, &field.value)?;
        out.write_char('}')?;
    }
    out.write_char(']')
}

fn write_footnotes(
    out: &mut impl Write,
    footnotes: &[FootnoteDef],
    mode: CanonicalMode,
) -> fmt::Result {
    out.write_char('[')?;
    for (index, definition) in footnotes.iter().enumerate() {
        if index > 0 {
            out.write_char(',')?;
        }
        out.write_str("{\"kind\":\"footnoteDef\",")?;
        write_node_head(out, &definition.meta, mode)?;
        out.write_str(",\"label\":")?;
        write_string(out, &definition.label)?;
        out.write_str(",\"blocks\":")?;
        write_blocks(out, &definition.blocks, mode)?;
        out.write_char('}')?;
    }
    out.write_char(']')
}

fn write_node_head(out: &mut impl Write, meta: &NodeMeta, mode: CanonicalMode) -> fmt::Result {
    let id = match mode {
        CanonicalMode::Full => meta.id.0,
        CanonicalMode::Content => 0,
    };
    write!(out, "\"id\":{},", id)?;
    match mode {
        CanonicalMode::Full => write!(
            out,
            "\"span\":{{\"source\":{},\"start\":{},\"end\":{}}},",
            meta.span.source.0, meta.span.start, meta.span.end
        )?,
        CanonicalMode::Content => {
            out.write_str("\"span\":{\"source\":0,\"start\":0,\"end\":0},")?
        }
    }
    out.write_str("\"attrs\":{\"role\":")?;
    write_opt_str(out, meta.attrs.role.as_deref())?;
    out.write_str(",\"classes\":[")?;
    for (index, class) in meta.attrs.classes.iter().enumerate() {
        if index > 0 {
            out.write_char(',')?;
        }
        write_string(out, class)?;
    }
    out.write_str("],\"anchor\":")?;
    write_opt_str(out, meta.attrs.anchor.as_deref())?;
    out.write_str(",\"lang\":")?;
    write_opt_str(out, meta.attrs.lang.as_deref())?;
    out.write_char('}')
}

fn write_blocks(out: &mut impl Write, blocks: &[Block], mode: CanonicalMode) -> fmt::Result {
    out.write_char('[')?;
    for (index, block) in blocks.iter().enumerate() {
        if index > 0 {
            out.write_char(',')?;
        }
        write_block(out, block, mode)?;
    }
    out.write_char(']')
}

fn write_block(out: &mut impl Write, block: &Block, mode: CanonicalMode) -> fmt::Result {
    out.write_str("{\"kind\":\"")?;
    out.write_str(block.kind_str())?;
    out.write_str("\",")?;
    write_node_head(out, block.meta(), mode)?;

    match block {
        Block::Paragraph(node) => {
            out.write_str(",\"inlines\":")?;
            write_inlines(out, &node.inlines, mode)?;
        }
        Block::Heading(node) => {
            write!(out, ",\"level\":{}", node.level)?;
            out.write_str(",\"inlines\":")?;
            write_inlines(out, &node.inlines, mode)?;
        }
        Block::List(node) => write_list_block(out, node, mode)?,
        Block::BlockQuote(node) => {
            out.write_str(",\"blocks\":")?;
            write_blocks(out, &node.blocks, mode)?;
        }
        Block::CodeBlock(node) => {
            out.write_str(",\"lang\":")?;
            write_opt_str(out, node.lang.as_deref())?;
            out.write_str(",\"text\":")?;
            write_string(out, &node.value)?;
        }
        Block::MathBlock(node) => {
            out.write_str(",\"source\":")?;
            write_string(out, &node.source)?;
        }
        Block::Figure(node) => {
            out.write_str(",\"image\":")?;
            write_image_node(out, &node.image, mode)?;
            out.write_str(",\"caption\":")?;
            write_inlines(out, &node.caption, mode)?;
            out.write_str(",\"label\":")?;
            write_opt_str(out, node.label.as_deref())?;
        }
        Block::Table(node) => write_table(out, node, mode)?,
        Block::ThematicBreak(_) | Block::PageBreak(_) => {}
        Block::Section(node) => write_section(out, node, mode)?,
        Block::Generated(node) => write_generated(out, node, mode)?,
        Block::Raw(node) => write_raw(out, &node.content)?,
    }

    out.write_char('}')
}

fn write_inlines(out: &mut impl Write, inlines: &[Inline], mode: CanonicalMode) -> fmt::Result {
    out.write_char('[')?;
    for (index, inline) in inlines.iter().enumerate() {
        if index > 0 {
            out.write_char(',')?;
        }
        write_inline(out, inline, mode)?;
    }
    out.write_char(']')
}

fn write_inline(out: &mut impl Write, inline: &Inline, mode: CanonicalMode) -> fmt::Result {
    out.write_str("{\"kind\":\"")?;
    out.write_str(inline.kind_str())?;
    out.write_str("\",")?;
    write_node_head(out, inline.meta(), mode)?;

    match inline {
        Inline::Text(node) => {
            out.write_str(",\"text\":")?;
            write_string(out, &node.value)?;
        }
        Inline::Code(node) => {
            out.write_str(",\"text\":")?;
            write_string(out, &node.value)?;
        }
        Inline::Strong(_)
        | Inline::Emphasis(_)
        | Inline::Underline(_)
        | Inline::Strikethrough(_)
        | Inline::SmallCaps(_)
        | Inline::Superscript(_)
        | Inline::Subscript(_) => {
            out.write_str(",\"inlines\":")?;
            write_inlines(out, inline.children(), mode)?;
        }
        Inline::Link(node) => {
            out.write_str(",\"href\":")?;
            write_string(out, &node.href)?;
            out.write_str(",\"title\":")?;
            write_opt_str(out, node.title.as_deref())?;
            out.write_str(",\"inlines\":")?;
            write_inlines(out, &node.children, mode)?;
        }
        Inline::Image(node) => write_image_fields(out, &node.source)?,
        Inline::Math(node) => {
            out.write_str(",\"source\":")?;
            write_string(out, &node.source)?;
        }
        Inline::FootnoteRef(node) => {
            out.write_str(",\"target\":")?;
            write_string(out, &node.target)?;
            out.write_str(",\"label\":")?;
            write_opt_str(out, node.label.as_deref())?;
        }
        Inline::Index(node) => {
            out.write_str(",\"term\":")?;
            write_string(out, &node.term)?;
        }
        Inline::Ruby(node) => {
            out.write_str(",\"base\":")?;
            write_string(out, &node.base)?;
            out.write_str(",\"annotation\":")?;
            write_string(out, &node.annotation)?;
        }
        Inline::LineBreak(node) => {
            out.write_str(",\"break\":\"")?;
            out.write_str(node.kind.as_str())?;
            out.write_char('"')?;
        }
        Inline::Raw(node) => write_raw(out, &node.content)?,
    }

    out.write_char('}')
}

fn write_image_node(
    out: &mut impl Write,
    image: &crate::inline::Image,
    mode: CanonicalMode,
) -> fmt::Result {
    out.write_str("{\"kind\":\"image\",")?;
    write_node_head(out, &image.meta, mode)?;
    write_image_fields(out, &image.source)?;
    out.write_char('}')
}

fn write_image_ref_object(out: &mut impl Write, image: &ImageRef) -> fmt::Result {
    out.write_char('{')?;
    out.write_str("\"src\":")?;
    write_string(out, &image.src)?;
    out.write_str(",\"alt\":")?;
    write_opt_str(out, image.alt.as_deref())?;
    out.write_str(",\"title\":")?;
    write_opt_str(out, image.title.as_deref())?;
    out.write_str(",\"intrinsic\":")?;
    write_intrinsic(out, image)?;
    out.write_char('}')
}

fn write_image_fields(out: &mut impl Write, image: &ImageRef) -> fmt::Result {
    out.write_str(",\"src\":")?;
    write_string(out, &image.src)?;
    out.write_str(",\"alt\":")?;
    write_opt_str(out, image.alt.as_deref())?;
    out.write_str(",\"title\":")?;
    write_opt_str(out, image.title.as_deref())?;
    out.write_str(",\"intrinsic\":")?;
    write_intrinsic(out, image)
}

fn write_intrinsic(out: &mut impl Write, image: &ImageRef) -> fmt::Result {
    match image.intrinsic {
        Some(intrinsic) => write!(
            out,
            "{{\"widthPx\":{},\"heightPx\":{}}}",
            intrinsic.width_px, intrinsic.height_px
        ),
        None => out.write_str("null"),
    }
}

fn write_list_block(out: &mut impl Write, list: &List, mode: CanonicalMode) -> fmt::Result {
    write!(
        out,
        ",\"ordered\":{},\"start\":{},\"tight\":{},\"items\":[",
        list.ordered, list.start, list.tight
    )?;
    for (index, item) in list.items.iter().enumerate() {
        if index > 0 {
            out.write_char(',')?;
        }
        out.write_str("{\"kind\":\"listItem\",")?;
        write_node_head(out, &item.meta, mode)?;
        out.write_str(",\"blocks\":")?;
        write_blocks(out, &item.blocks, mode)?;
        out.write_char('}')?;
    }
    out.write_char(']')
}

fn write_table(out: &mut impl Write, table: &Table, mode: CanonicalMode) -> fmt::Result {
    out.write_str(",\"columns\":[")?;
    for (index, column) in table.columns.iter().enumerate() {
        if index > 0 {
            out.write_char(',')?;
        }
        out.write_str("{\"kind\":\"column\",")?;
        write_node_head(out, &column.meta, mode)?;
        out.write_str(",\"align\":\"")?;
        out.write_str(column.align.as_str())?;
        // Only a column with a width hint writes the key, so every existing
        // document keeps its canonical bytes.
        if column.share > 0 {
            write!(out, ",\"share\":{}", column.share)?;
        }
        out.write_str("\"}")?;
    }
    out.write_str("],\"caption\":")?;
    write_inlines(out, &table.caption, mode)?;
    out.write_str(",\"label\":")?;
    write_opt_str(out, table.label.as_deref())?;
    out.write_str(",\"rows\":[")?;
    for (index, row) in table.rows.iter().enumerate() {
        if index > 0 {
            out.write_char(',')?;
        }
        write_row(out, row, mode)?;
    }
    out.write_char(']')
}

fn write_row(out: &mut impl Write, row: &Row, mode: CanonicalMode) -> fmt::Result {
    out.write_str("{\"kind\":\"row\",")?;
    write_node_head(out, &row.meta, mode)?;
    write!(out, ",\"header\":{},\"cells\":[", row.header)?;
    for (index, cell) in row.cells.iter().enumerate() {
        if index > 0 {
            out.write_char(',')?;
        }
        out.write_str("{\"kind\":\"cell\",")?;
        write_node_head(out, &cell.meta, mode)?;
        write!(
            out,
            ",\"colspan\":{},\"rowspan\":{},\"blocks\":",
            cell.colspan, cell.rowspan
        )?;
        write_blocks(out, &cell.blocks, mode)?;
        out.write_char('}')?;
    }
    out.write_str("]}")
}

fn write_section(out: &mut impl Write, section: &Section, mode: CanonicalMode) -> fmt::Result {
    write!(
        out,
        ",\"level\":{},\"numbered\":{},\"pageTemplate\":",
        section.level, section.numbered
    )?;
    write_opt_str(out, section.page_template.as_deref())?;
    out.write_str(",\"title\":")?;
    match &section.title {
        Some(title) => write_inlines(out, title, mode)?,
        None => out.write_str("null")?,
    }
    Ok(())
}

fn write_generated(
    out: &mut impl Write,
    generated: &GeneratedBlock,
    mode: CanonicalMode,
) -> fmt::Result {
    out.write_str(",\"generatedKind\":\"")?;
    out.write_str(generated.kind.as_str())?;
    out.write_str("\",\"slot\":")?;
    write_opt_str(out, generated.slot.as_deref())?;
    out.write_str(",\"blocks\":")?;
    write_blocks(out, &generated.blocks, mode)
}

fn write_raw(out: &mut impl Write, raw: &RawContent) -> fmt::Result {
    out.write_str(",\"format\":")?;
    write_string(out, &raw.format)?;
    out.write_str(",\"value\":")?;
    write_string(out, &raw.value)?;
    out.write_str(",\"fallback\":")?;
    write_string(out, &raw.fallback)?;
    out.write_str(",\"capability\":\"")?;
    out.write_str(raw.capability.as_str())?;
    out.write_char('"')
}

fn write_opt_str(out: &mut impl Write, value: Option<&str>) -> fmt::Result {
    match value {
        Some(value) => write_string(out, value),
        None => out.write_str("null"),
    }
}

/// JSON string escaping. Control characters below 0x20 are written as `\u00XX`.
fn write_string(out: &mut impl Write, value: &str) -> fmt::Result {
    out.write_char('"')?;
    for character in value.chars() {
        match character {
            '"' => out.write_str("\\\"")?,
            '\\' => out.write_str("\\\\")?,
            '\n' => out.write_str("\\n")?,
            '\r' => out.write_str("\\r")?,
            '\t' => out.write_str("\\t")?,
            character if (character as u32) < 0x20 => {
                write!(out, "\\u{:04x}", character as u32)?;
            }
            character => out.write_char(character)?,
        }
    }
    out.write_char('"')
}
