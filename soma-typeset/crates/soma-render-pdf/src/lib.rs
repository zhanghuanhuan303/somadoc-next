//! PDF renderer: consumes `PagedLayout` + `StyledDoc`/`TextSource` and writes a
//! deterministic PDF with embedded TrueType subsets, bookmarks, page labels,
//! image XObjects and a tagged structure tree.
//!
//! It never breaks lines, never paginates, and never reads DocIR. Fonts and
//! images are supplied by the caller (the CLI reads them from disk); the
//! renderer keeps only the glyphs the layout actually uses and the images a
//! figure actually points at.

#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]

extern crate alloc;

mod cff;
mod deflate;
mod font;
mod image;
mod inflate;
mod standard_encoding;

pub use cff::CidSystem;
pub use font::{PdfError, PdfFont, PdfFontDescriptor, ProgramKind, Subset};

/// The image seam is shared with the other renderers; re-exported so a caller
/// only needs this crate's name.
pub use soma_layout::{ImageSource, NoImages};

use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use core::fmt::Write;

use soma_core::Decorations;
use soma_layout::{
    BoxKind, MathBox, OutlineEntry, PagedLayout, PlacedBlock, TableCell, TextSource,
};
use soma_template::{ComputedStyle, FontWeight, StyledDoc};

const UNITS_PER_EM: i64 = 4096;
const FOOTNOTE_MARKER: char = '\u{e000}';

/// Fonts available to the PDF renderer.
///
/// A style picks its face by `font-family` and `font-weight` (the same rule the
/// layout uses to measure), so a book can set its body in a serif face and its
/// headings in a sans face. Anything missing falls back to `regular`.
pub struct PdfFonts<'a> {
    pub regular: &'a PdfFont,
    pub bold: Option<&'a PdfFont>,
    /// The sans family: `font-family: sans` (or `hei`, `gothic`, `mono`).
    pub sans: Option<&'a PdfFont>,
    pub sans_bold: Option<&'a PdfFont>,
    pub math: Option<&'a PdfFont>,
    /// A rescue face for glyphs the body face lacks. Omit it and those glyphs
    /// render as `.notdef`; layout will still have measured them correctly.
    pub fallback: Option<&'a PdfFont>,
}

/// Document metadata written to the PDF Info dictionary.
#[derive(Clone, Copy, Default)]
pub struct PdfMetadata<'a> {
    pub title: Option<&'a str>,
    pub author: Option<&'a str>,
    pub language: Option<&'a str>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Role {
    Regular,
    Bold,
    Sans,
    SansBold,
    Math,
    Fallback,
}

impl Role {
    const ALL: [Role; 6] = [
        Role::Regular,
        Role::Bold,
        Role::Sans,
        Role::SansBold,
        Role::Math,
        Role::Fallback,
    ];

    fn resource(self) -> &'static str {
        match self {
            Role::Regular => "F1",
            Role::Bold => "F2",
            Role::Sans => "F3",
            Role::SansBold => "F4",
            Role::Math => "F5",
            Role::Fallback => "F6",
        }
    }
}

/// Render every page to one PDF file.
///
/// `assets` resolves the source strings figures carry to bytes; pass
/// [`NoImages`] when there is nothing to read and the figures keep their
/// placeholder boxes.
pub fn render(
    layout: &PagedLayout,
    styles: &StyledDoc,
    text: &dyn TextSource,
    fonts: &PdfFonts<'_>,
    metadata: &PdfMetadata<'_>,
    assets: &dyn ImageSource,
) -> Result<Vec<u8>, PdfError> {
    let scale = Scale {
        num: styles.body_pt().num(),
        den: styles.body_pt().den(),
    };
    if scale.den <= 0 || scale.num <= 0 {
        return Err(PdfError::new("body font size must be positive"));
    }

    let mut used = Used::default();
    for page in &layout.pages {
        collect_blocks(&page.blocks, styles, text, fonts, &mut used);
        collect_blocks(&page.header, styles, text, fonts, &mut used);
        collect_blocks(&page.footer, styles, text, fonts, &mut used);
        collect_blocks(&page.footnotes, styles, text, fonts, &mut used);
    }

    let mut pdf = Pdf::new();
    let catalog = pdf.reserve();
    let pages_object = pdf.reserve();
    let struct_root = pdf.reserve();
    let document_elem = pdf.reserve();

    let mut embedded: Vec<(Role, EmbeddedFont)> = Vec::new();
    for role in Role::ALL {
        if !used.has(role, fonts) {
            continue;
        }
        let font = fonts.font(role);
        let entries = used.entries(role);
        let gids: Vec<u16> = entries.iter().map(|(gid, _)| *gid).collect();
        let subset = font.subset(&gids)?;
        embedded.push((role, embed_font(&mut pdf, font, role, &subset, &entries)));
    }

    // One XObject per distinct source, in first-appearance order. A source that
    // does not resolve, or a format this backend cannot read, is skipped: the
    // figure keeps its placeholder instead of failing the page.
    let images = collect_images(&mut pdf, layout, text, assets);

    let scene = Scene {
        layout,
        styles,
        text,
        fonts,
        images: &images,
    };

    let mut page_ids = Vec::with_capacity(layout.pages.len());
    // Structure elements per page, in MCID order, built after the page looks
    // exist because each element points back at its page.
    // Every structure element that hangs off the Document, in reading order.
    let mut roots: Vec<u32> = Vec::new();
    let mut structure: Vec<Vec<(u32, u32)>> = Vec::with_capacity(layout.pages.len());
    for (index, page) in layout.pages.iter().enumerate() {
        let mut tags = PageTags::default();
        let mut page_images = Vec::new();
        let (content, links) = paint_page(&scene, scale, page, &mut tags, &mut page_images);
        let content_id = pdf.reserve();
        pdf.set(content_id, stream_flate(b"", &content));

        let mut font_resources = String::new();
        for (role, font) in &embedded {
            let _ = write!(font_resources, "/{} {} 0 R ", role.resource(), font.type0);
        }
        let mut xobject_resources = String::new();
        for used_index in &page_images {
            let object = &images[*used_index];
            let _ = write!(xobject_resources, "/{} {} 0 R ", object.name, object.object);
        }
        let xobjects = if xobject_resources.is_empty() {
            String::new()
        } else {
            format!("/XObject << {xobject_resources}>> ")
        };
        let annotations = link_annotations(&links);
        let tagged = !tags.roots.is_empty();
        let parents = if tagged {
            format!(" /StructParents {index}")
        } else {
            String::new()
        };
        let page_id = pdf.reserve();
        let body = format!(
            "<< /Type /Page /Parent {pages_object} 0 R /MediaBox [0 0 {} {}] /Resources << /Font << {font_resources}>> {xobjects}>> /Contents {content_id} 0 R{annotations}{parents} >>",
            number(scale.milli(layout.page.width.0)),
            number(scale.milli(layout.page.height.0))
        );
        pdf.set(page_id, body.into_bytes());
        page_ids.push(page_id);

        // One ParentTree entry per MCID, keyed by MCID. `owners` comes back in
        // tree order, which is reading order; sort so a cell that owns several
        // sequences still lines up.
        let mut owners: Vec<(u32, u32)> = Vec::new();
        let mut elements = Vec::with_capacity(tags.roots.len());
        for root in &tags.roots {
            elements.push(write_struct(
                &mut pdf,
                root,
                document_elem,
                page_id,
                &mut owners,
            ));
        }
        owners.sort_by_key(|(mcid, _)| *mcid);
        structure.push(owners);
        roots.extend(elements);
    }

    let outline = pdf_outline(&mut pdf, &layout.outline, &page_ids, layout, scale);
    let labels = page_labels(&mut pdf, &layout.pages);

    let info = pdf.reserve();
    pdf.set(info, info_object(metadata));

    // The structure tree: a Document element holding every page's roots in
    // reading order, and a parent tree keyed by `StructParents`. The parent
    // tree is indexed by MCID, so it lists the owner of each sequence — not
    // only the roots.
    let mut numbers = String::new();
    for (index, owners) in structure.iter().enumerate() {
        if owners.is_empty() {
            continue;
        }
        let _ = write!(numbers, "{index} [");
        for (_, owner) in owners {
            let _ = write!(numbers, "{owner} 0 R ");
        }
        numbers.push_str("] ");
    }
    let parent_tree = pdf.reserve();
    pdf.set(parent_tree, format!("<< /Nums [{numbers}] >>").into_bytes());

    let mut kids = String::new();
    for root in &roots {
        let _ = write!(kids, "{root} 0 R ");
    }
    pdf.set(
        document_elem,
        format!("<< /Type /StructElem /S /Document /P {struct_root} 0 R /K [{kids}] >>")
            .into_bytes(),
    );
    pdf.set(
        struct_root,
        format!(
            "<< /Type /StructTreeRoot /K [{document_elem} 0 R] /ParentTree {parent_tree} 0 R /ParentTreeNextKey {} >>",
            layout.pages.len()
        )
        .into_bytes(),
    );

    let mut catalog_body = format!(
        "<< /Type /Catalog /Pages {pages_object} 0 R /MarkInfo << /Marked true >> /StructTreeRoot {struct_root} 0 R"
    );
    if let Some(outline) = outline {
        let _ = write!(
            catalog_body,
            " /Outlines {outline} 0 R /PageMode /UseOutlines"
        );
    }
    if let Some(labels) = labels {
        let _ = write!(catalog_body, " /PageLabels {labels} 0 R");
    }
    if let Some(language) = metadata.language.filter(|value| !value.is_empty()) {
        let _ = write!(catalog_body, " /Lang {}", pdf_string(language));
    }
    catalog_body.push_str(" >>");
    pdf.set(catalog, catalog_body.into_bytes());

    let mut page_kids = String::new();
    for id in &page_ids {
        let _ = write!(page_kids, "{id} 0 R ");
    }
    pdf.set(
        pages_object,
        format!(
            "<< /Type /Pages /Kids [{page_kids}] /Count {} >>",
            page_ids.len()
        )
        .into_bytes(),
    );

    Ok(pdf.finish(catalog, info))
}

/// Write one structure element and its subtree, returning its object number.
///
/// Every MCID it owns is appended to `owners`, so the caller can build the
/// parent tree from the same walk that wrote the tree.
fn write_struct(
    pdf: &mut Pdf,
    node: &TagNode,
    parent: u32,
    page: u32,
    owners: &mut Vec<(u32, u32)>,
) -> u32 {
    let id = pdf.reserve();
    let mut kids: Vec<u32> = Vec::with_capacity(node.children.len());
    for child in &node.children {
        kids.push(write_struct(pdf, child, id, page, owners));
    }
    let mut body = format!("<< /Type /StructElem /S /{} /P {parent} 0 R", node.tag);
    if kids.is_empty() {
        let _ = write!(body, " /Pg {page} 0 R /K ");
        match node.mcids.as_slice() {
            [] => body.push_str("-1"),
            [only] => {
                let _ = write!(body, "{only}");
            }
            many => {
                body.push('[');
                for mcid in many {
                    let _ = write!(body, "{mcid} ");
                }
                body.push(']');
            }
        }
    } else {
        let _ = write!(body, " /Pg {page} 0 R /K [");
        for kid in &kids {
            let _ = write!(body, "{kid} 0 R ");
        }
        body.push(']');
    }
    if let Some((row_span, column_span)) = node.span {
        if row_span > 1 || column_span > 1 {
            let _ = write!(
                body,
                " /A << /O /Table /RowSpan {row_span} /ColSpan {column_span} >>"
            );
        }
    }
    if let Some(alt) = node.alt.as_deref() {
        let _ = write!(body, " /Alt {}", pdf_string(alt));
    }
    body.push_str(" >>");
    pdf.set(id, body.into_bytes());
    for mcid in &node.mcids {
        owners.push((*mcid, id));
    }
    id
}

/// Build one image XObject per distinct figure source, in reading order.
fn collect_images(
    pdf: &mut Pdf,
    layout: &PagedLayout,
    text: &dyn TextSource,
    assets: &dyn ImageSource,
) -> Vec<ImageObject> {
    let mut objects: Vec<ImageObject> = Vec::new();
    for page in &layout.pages {
        for block in &page.blocks {
            if block.kind != BoxKind::Figure {
                continue;
            }
            let Some(src) = text.block_image(block.node) else {
                continue;
            };
            if objects.iter().any(|object| object.src == src) {
                continue;
            }
            let Some(bytes) = assets.image(src) else {
                continue;
            };
            let Ok(decoded) = image::decode(bytes) else {
                continue;
            };
            let name = format!("Im{}", objects.len() + 1);
            let object = pdf.reserve();
            let header = format!(
                "/Type /XObject /Subtype /Image /Width {} /Height {} /ColorSpace {} /BitsPerComponent 8",
                decoded.width,
                decoded.height,
                decoded.color_space.name()
            );
            let stream = match decoded.filter {
                image::Filter::Flate => stream_flate(header.as_bytes(), &decoded.data),
                image::Filter::Dct => {
                    let full = format!("{header} /Filter /DCTDecode");
                    stream(full.as_bytes(), &decoded.data)
                }
            };
            pdf.set(object, stream);
            objects.push(ImageObject {
                src: String::from(src),
                name,
                object,
            });
        }
    }
    objects
}

// ---------------------------------------------------------------------------
// Geometry
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
struct Scale {
    num: i64,
    den: i64,
}

impl Scale {
    /// Convert layout units (1/4096 em) to milli-points.
    fn milli(self, units: i32) -> i64 {
        units as i64 * self.num * 1000 / (UNITS_PER_EM * self.den)
    }

    fn size(self, style: &ComputedStyle) -> i64 {
        self.milli(style.font_size.0)
    }
}

fn number(value: i64) -> String {
    let sign = if value < 0 { "-" } else { "" };
    let absolute = value.abs();
    format!("{}{}.{:03}", sign, absolute / 1000, absolute % 1000)
}

fn color(number: u32) -> (i64, i64, i64) {
    (
        ((number >> 16) & 0xFF) as i64 * 1000 / 255,
        ((number >> 8) & 0xFF) as i64 * 1000 / 255,
        (number & 0xFF) as i64 * 1000 / 255,
    )
}

// ---------------------------------------------------------------------------
// Used glyphs
// ---------------------------------------------------------------------------

#[derive(Default)]
struct Used {
    regular: Vec<(u16, u32)>,
    bold: Vec<(u16, u32)>,
    sans: Vec<(u16, u32)>,
    sans_bold: Vec<(u16, u32)>,
    math: Vec<(u16, u32)>,
    fallback: Vec<(u16, u32)>,
}

impl Used {
    fn add(&mut self, role: Role, font: &PdfFont, character: char) {
        if character == '\u{fffc}' || character == FOOTNOTE_MARKER {
            return;
        }
        let gid = font.glyph(character as u32);
        // `.notdef` gets a replacement character, never a random missing one.
        let codepoint = if gid == 0 { 0xFFFD } else { character as u32 };
        self.list(role).push((gid, codepoint));
    }

    fn list(&mut self, role: Role) -> &mut Vec<(u16, u32)> {
        match role {
            Role::Regular => &mut self.regular,
            Role::Bold => &mut self.bold,
            Role::Sans => &mut self.sans,
            Role::SansBold => &mut self.sans_bold,
            Role::Math => &mut self.math,
            Role::Fallback => &mut self.fallback,
        }
    }

    fn has(&self, role: Role, fonts: &PdfFonts<'_>) -> bool {
        match role {
            Role::Regular => !self.regular.is_empty(),
            Role::Bold => fonts.bold.is_some() && !self.bold.is_empty(),
            Role::Sans => fonts.sans.is_some() && !self.sans.is_empty(),
            Role::SansBold => fonts.sans_bold.is_some() && !self.sans_bold.is_empty(),
            Role::Math => fonts.math.is_some() && !self.math.is_empty(),
            Role::Fallback => fonts.fallback.is_some() && !self.fallback.is_empty(),
        }
    }

    /// Sorted and unique; the first Unicode scalar wins when several map to
    /// one glyph.
    fn entries(&self, role: Role) -> Vec<(u16, u32)> {
        let mut list = match role {
            Role::Regular => self.regular.clone(),
            Role::Bold => self.bold.clone(),
            Role::Sans => self.sans.clone(),
            Role::SansBold => self.sans_bold.clone(),
            Role::Math => self.math.clone(),
            Role::Fallback => self.fallback.clone(),
        };
        list.sort();
        list.dedup_by_key(|(gid, _)| *gid);
        list
    }
}

impl PdfFonts<'_> {
    fn font(&self, role: Role) -> &PdfFont {
        match role {
            Role::Bold => self.bold.unwrap_or(self.regular),
            Role::Sans => self.sans.unwrap_or(self.regular),
            Role::SansBold => self.sans_bold.or(self.sans).unwrap_or(self.regular),
            Role::Math => self.math.unwrap_or(self.regular),
            Role::Fallback => self.fallback.unwrap_or(self.regular),
            Role::Regular => self.regular,
        }
    }

    fn role_for(&self, style: &ComputedStyle, fallback: bool) -> Role {
        if fallback && self.fallback.is_some() {
            return Role::Fallback;
        }
        let bold = style.weight == FontWeight::Bold;
        if soma_template::family_is_sans(style.family.as_deref()) {
            match (bold, self.sans_bold, self.sans) {
                (true, Some(_), _) => Role::SansBold,
                (_, _, Some(_)) => Role::Sans,
                _ => Role::Regular,
            }
        } else if bold && self.bold.is_some() {
            Role::Bold
        } else {
            Role::Regular
        }
    }

    fn math_role(&self) -> Role {
        if self.math.is_some() {
            Role::Math
        } else {
            Role::Regular
        }
    }
}

fn collect_blocks(
    blocks: &[PlacedBlock],
    styles: &StyledDoc,
    text: &dyn TextSource,
    fonts: &PdfFonts<'_>,
    used: &mut Used,
) {
    for block in blocks {
        let block_style = styles
            .style(block.style)
            .cloned()
            .unwrap_or_else(ComputedStyle::base);
        let block_role = fonts.role_for(&block_style, false);
        for placed in &block.lines {
            for atom in &placed.line.boxes {
                if let Some(inline) = block.inline_math.iter().find(|math| math.atom == atom.atom) {
                    collect_math(&inline.math, fonts, used, block_role);
                    continue;
                }
                let style_id = text
                    .atom_style(block.node, atom.atom)
                    .unwrap_or(block.style);
                let style = styles
                    .style(style_id)
                    .cloned()
                    .unwrap_or_else(ComputedStyle::base);
                let role = fonts.role_for(&style, text.uses_fallback(block.node, atom.atom));
                for character in text.atom_text(block.node, atom.atom).chars() {
                    used.add(role, fonts.font(role), character);
                }
            }
        }
        if let Some(math) = &block.math {
            collect_math(math, fonts, used, block_role);
        }
    }
}

/// The face that draws `character` inside a formula: the math face when it has
/// the glyph, else the text face (a `\text{...}` run in Han, say), else the
/// rescue face.
fn math_face(fonts: &PdfFonts<'_>, character: char, text_role: Role) -> Role {
    let math = fonts.math_role();
    if math == text_role {
        return text_role;
    }
    if fonts.font(math).glyph(character as u32) != 0 {
        return math;
    }
    if let Some(fallback) = fonts.fallback {
        if fallback.glyph(character as u32) != 0 {
            return Role::Fallback;
        }
    }
    text_role
}

fn collect_math(math: &MathBox, fonts: &PdfFonts<'_>, used: &mut Used, text_role: Role) {
    for item in &math.items {
        for character in item.text.chars() {
            let role = math_face(fonts, character, text_role);
            used.add(role, fonts.font(role), character);
        }
    }
}

// ---------------------------------------------------------------------------
// Fonts in the PDF
// ---------------------------------------------------------------------------

struct EmbeddedFont {
    type0: u32,
}

fn embed_font(
    pdf: &mut Pdf,
    font: &PdfFont,
    role: Role,
    subset: &Subset,
    entries: &[(u16, u32)],
) -> EmbeddedFont {
    let descriptor = font.descriptor();
    let name = match role {
        Role::Regular => "SomaSerif",
        Role::Bold => "SomaSerif-Bold",
        Role::Sans => "SomaSans",
        Role::SansBold => "SomaSans-Bold",
        Role::Math => "SomaMath",
        Role::Fallback => "SomaSerif-Fallback",
    };

    let file = pdf.reserve();
    let program = &subset.program;
    let stream_dict = match subset.kind {
        ProgramKind::TrueType => format!("/Length1 {}", program.len()),
        ProgramKind::CidCff => String::from("/Subtype /CIDFontType0C"),
    };
    pdf.set(file, stream_flate(stream_dict.as_bytes(), program));

    let unicode = pdf.reserve();
    pdf.set(unicode, to_unicode(entries));

    let mut widths = String::new();
    for (gid, _) in entries {
        let _ = write!(widths, "{} [{}] ", gid, font.width_1000(*gid));
    }

    let file_key = match subset.kind {
        ProgramKind::TrueType => "FontFile2",
        ProgramKind::CidCff => "FontFile3",
    };
    let descriptor_id = pdf.reserve();
    pdf.set(
        descriptor_id,
        format!(
            "<< /Type /FontDescriptor /FontName /{name} /Flags {} /FontBBox [{} {} {} {}] /ItalicAngle {} /Ascent {} /Descent {} /CapHeight {} /StemV 80 /{file_key} {file} 0 R >>",
            descriptor.flags,
            descriptor.bbox[0],
            descriptor.bbox[1],
            descriptor.bbox[2],
            descriptor.bbox[3],
            descriptor.italic_angle,
            descriptor.ascent,
            descriptor.descent,
            descriptor.cap_height
        )
        .into_bytes(),
    );

    let system = font.cid_system();
    let (subtype, glyph_map) = match subset.kind {
        ProgramKind::TrueType => ("CIDFontType2", " /CIDToGIDMap /Identity"),
        // A CIDFontType0 reads the CID-to-glyph mapping out of the embedded
        // font's charset, which the subsetter rewrote to identity.
        ProgramKind::CidCff => ("CIDFontType0", ""),
    };
    let cid = pdf.reserve();
    pdf.set(
        cid,
        format!(
            "<< /Type /Font /Subtype /{subtype} /BaseFont /{name} /CIDSystemInfo << /Registry ({}) /Ordering ({}) /Supplement {} >> /FontDescriptor {descriptor_id} 0 R /DW 1000 /W [{widths}]{glyph_map} >>",
            system.registry, system.ordering, system.supplement
        )
        .into_bytes(),
    );

    let type0 = pdf.reserve();
    pdf.set(
        type0,
        format!(
            "<< /Type /Font /Subtype /Type0 /BaseFont /{name} /Encoding /Identity-H /DescendantFonts [{cid} 0 R] /ToUnicode {unicode} 0 R >>"
        )
        .into_bytes(),
    );
    EmbeddedFont { type0 }
}

/// `/Annots` entries for a page's links, or an empty string when it has none.
fn link_annotations(links: &[Link]) -> String {
    if links.is_empty() {
        return String::new();
    }
    let mut out = String::from(" /Annots [");
    for link in links {
        let _ = write!(
            out,
            "<< /Type /Annot /Subtype /Link /Rect [{} {} {} {}] /Border [0 0 0] /A << /S /URI /URI {} >> >> ",
            number(link.rect[0]),
            number(link.rect[1]),
            number(link.rect[2]),
            number(link.rect[3]),
            pdf_string(&link.uri)
        );
    }
    out.push(']');
    out
}

fn to_unicode(entries: &[(u16, u32)]) -> Vec<u8> {
    let mut body = String::new();
    body.push_str(
        "/CIDInit /ProcSet findresource begin\n12 dict begin\nbegincmap\n\
         /CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def\n\
         /CMapName /Adobe-Identity-UCS def\n/CMapType 2 def\n\
         1 begincodespacerange\n<0000> <FFFF>\nendcodespacerange\n",
    );
    for chunk in entries.chunks(100) {
        let _ = writeln!(body, "{} beginbfchar", chunk.len());
        for (gid, codepoint) in chunk {
            let mut utf16 = String::new();
            if let Some(character) = char::from_u32(*codepoint) {
                let mut buffer = [0u16; 2];
                for unit in character.encode_utf16(&mut buffer) {
                    let _ = write!(utf16, "{:04X}", unit);
                }
            }
            let _ = writeln!(body, "<{:04X}> <{}>", gid, utf16);
        }
        body.push_str("endbfchar\n");
    }
    body.push_str("endcmap\nCMapName currentdict /CMap defineresource pop\nend\nend\n");
    stream_flate(b"", body.as_bytes())
}

// ---------------------------------------------------------------------------
// Content streams
// ---------------------------------------------------------------------------

fn paint_page(
    scene: &Scene<'_>,
    scale: Scale,
    page: &soma_layout::Page,
    tags: &mut PageTags,
    used_images: &mut Vec<usize>,
) -> (Vec<u8>, Vec<Link>) {
    let mut painter = Painter {
        out: Vec::new(),
        scale,
        fill: None,
        text_open: false,
        font: None,
        size: 0,
        links: Vec::new(),
    };
    for block in &page.blocks {
        paint_block(scene, &mut painter, block, 0, Some(tags), used_images);
    }
    // Running heads and folios are not part of the reading order, so they are
    // painted but not tagged.
    for block in &page.header {
        paint_block(scene, &mut painter, block, 0, None, used_images);
    }
    for block in &page.footer {
        paint_block(scene, &mut painter, block, 0, None, used_images);
    }
    let footnote_top = scene.layout.page.content_height().0 - page.footnote_height.0;
    for block in &page.footnotes {
        paint_block(
            scene,
            &mut painter,
            block,
            footnote_top.max(0),
            None,
            used_images,
        );
    }
    painter.end_text();
    (painter.out, painter.links)
}

/// The document-level inputs a page needs. Bundled so that `paint_block` keeps
/// a readable parameter list.
struct Scene<'a> {
    layout: &'a PagedLayout,
    styles: &'a StyledDoc,
    text: &'a dyn TextSource,
    fonts: &'a PdfFonts<'a>,
    images: &'a [ImageObject],
}

impl Scene<'_> {
    /// Index into `images` for a figure block, when the asset decoded.
    fn image_for(&self, block: &PlacedBlock) -> Option<usize> {
        let src = self.text.block_image(block.node)?;
        self.images.iter().position(|object| object.src == src)
    }
}

/// One image XObject, addressed by the source string the document wrote.
struct ImageObject {
    src: String,
    /// Resource name inside a page's `/XObject` dictionary.
    name: String,
    /// Object number of the image stream.
    object: u32,
}

/// One structure element.
///
/// A container (`Table`, `TR`) owns children and has no marked content of its
/// own; a leaf owns one or more marked-content sequences. `/K` is written from
/// whichever of the two the element has.
struct TagNode {
    tag: &'static str,
    alt: Option<String>,
    /// MCIDs this element owns, in order.
    mcids: Vec<u32>,
    /// `/RowSpan` and `/ColSpan`, for a table cell that spans more than one.
    span: Option<(u16, u16)>,
    children: Vec<TagNode>,
}

/// The structure elements of one page, in reading order. The MCID of a
/// sequence is the order it was opened in, which is what the content stream
/// marks.
#[derive(Default)]
struct PageTags {
    roots: Vec<TagNode>,
    next_mcid: u32,
    /// Index into `roots` of the table being built, if the last block was a
    /// cell.
    table: Option<usize>,
    /// `(row, column)` of the cell currently accumulating blocks, so a second
    /// paragraph in the same cell lands under the same `/TD`.
    cell: Option<(u16, u16)>,
}

impl PageTags {
    /// Start a marked-content sequence. `cell` places it in a table grid; a
    /// block that is not in a table closes the one that was open.
    fn open(&mut self, tag: &'static str, alt: Option<&str>, cell: Option<TableCell>) -> u32 {
        let mcid = self.next_mcid;
        self.next_mcid += 1;
        let Some(cell) = cell else {
            self.table = None;
            self.cell = None;
            self.roots.push(TagNode {
                tag,
                alt: alt.map(String::from),
                mcids: vec![mcid],
                span: None,
                children: Vec::new(),
            });
            return mcid;
        };
        if self.table.is_none() {
            self.roots.push(TagNode {
                tag: "Table",
                alt: None,
                mcids: Vec::new(),
                span: None,
                children: Vec::new(),
            });
            self.table = Some(self.roots.len() - 1);
            self.cell = None;
        }
        let table = self.table.unwrap();
        // Cells arrive row by row and left to right, so the current row is the
        // table's last child.
        let rows = &mut self.roots[table].children;
        if rows.last().map(|row| row.tag) != Some("TR")
            || self.cell.map(|(row, _)| row) != Some(cell.row)
        {
            rows.push(TagNode {
                tag: "TR",
                alt: None,
                mcids: Vec::new(),
                span: None,
                children: Vec::new(),
            });
            self.cell = None;
        }
        let row = rows.last_mut().expect("a row was just added");
        if self.cell != Some((cell.row, cell.column)) {
            row.children.push(TagNode {
                tag,
                alt: alt.map(String::from),
                mcids: vec![mcid],
                span: Some((cell.row_span, cell.column_span)),
                children: Vec::new(),
            });
            self.cell = Some((cell.row, cell.column));
        } else {
            row.children
                .last_mut()
                .expect("the cell exists")
                .mcids
                .push(mcid);
        }
        mcid
    }
}

/// A link rectangle on the page, in milli-points.
struct Link {
    rect: [i64; 4],
    uri: String,
}

struct Painter {
    out: Vec<u8>,
    scale: Scale,
    fill: Option<u32>,
    text_open: bool,
    font: Option<Role>,
    size: i64,
    links: Vec<Link>,
}

impl Painter {
    fn emit(&mut self, value: &str) {
        self.out.extend_from_slice(value.as_bytes());
    }

    /// Fill a rectangle in layout units, in the current fill colour.
    fn rule(&mut self, x: i32, y: i32, width: i32, height: i32) {
        if width <= 0 || height <= 0 {
            return;
        }
        self.end_text();
        let line = format!(
            "{} {} {} {} re f\n",
            number(self.scale.milli(x)),
            number(self.scale.milli(y)),
            number(self.scale.milli(width)),
            number(self.scale.milli(height))
        );
        self.emit(&line);
    }

    /// Record a link rectangle, merging with the previous one when the two run
    /// on and point at the same target.
    fn link(&mut self, rect: [i64; 4], uri: &str) {
        if let Some(last) = self.links.last_mut() {
            // Advances round to the milli-point, so a run's pieces can overlap
            // by a unit; treat anything that close as touching.
            if last.uri == uri
                && last.rect[1] == rect[1]
                && last.rect[3] == rect[3]
                && (rect[0] - last.rect[2]).abs() <= 128
            {
                last.rect[2] = rect[2];
                return;
            }
        }
        self.links.push(Link {
            rect,
            uri: String::from(uri),
        });
    }

    fn set_fill(&mut self, value: u32) {
        if self.fill == Some(value) {
            return;
        }
        self.end_text();
        self.fill = Some(value);
        let (red, green, blue) = color(value);
        let line = format!("{} {} {} rg\n", number(red), number(green), number(blue));
        self.emit(&line);
    }

    fn start_text(&mut self) {
        if !self.text_open {
            self.emit("BT\n");
            self.text_open = true;
            self.font = None;
        }
    }

    fn end_text(&mut self) {
        if self.text_open {
            self.emit("ET\n");
            self.text_open = false;
            self.font = None;
        }
    }

    fn set_font(&mut self, role: Role, size: i64) {
        if self.font == Some(role) && self.size == size {
            return;
        }
        self.font = Some(role);
        self.size = size;
        let line = format!("/{} {} Tf\n", role.resource(), number(size));
        self.emit(&line);
    }

    /// Show a run, optionally stretched horizontally (a brace spanning a
    /// body). `stretch` is permille, 1000 meaning "as is".
    fn show_stretched(&mut self, x: i64, y: i64, stretch: u16, glyphs: &[u16]) {
        if stretch == 1000 {
            self.show(x, y, glyphs);
            return;
        }
        let mut hex = String::with_capacity(glyphs.len() * 4);
        for gid in glyphs {
            let _ = write!(hex, "{:04X}", gid);
        }
        let line = format!(
            "{} 0 0 1 {} {} Tm <{}> Tj\n",
            number(i64::from(stretch)),
            number(x),
            number(y),
            hex
        );
        self.emit(&line);
    }

    fn show(&mut self, x: i64, y: i64, glyphs: &[u16]) {
        let mut hex = String::with_capacity(glyphs.len() * 4);
        for gid in glyphs {
            let _ = write!(hex, "{:04X}", gid);
        }
        let line = format!("1 0 0 1 {} {} Tm <{}> Tj\n", number(x), number(y), hex);
        self.emit(&line);
    }

    fn rect(&mut self, x: i64, y: i64, width: i64, height: i64) {
        self.end_text();
        let line = format!(
            "{} {} {} {} re f\n",
            number(x),
            number(y),
            number(width),
            number(height)
        );
        self.emit(&line);
    }

    fn stroke_rect(&mut self, x: i64, y: i64, width: i64, height: i64) {
        self.end_text();
        let line = format!(
            "0.800 0.800 0.800 RG 0.5 w {} {} {} {} re S\n",
            number(x),
            number(y),
            number(width),
            number(height)
        );
        self.emit(&line);
    }

    /// Open a marked-content sequence and remember which structure element it
    /// belongs to (ADR-0047).
    fn begin_marked(&mut self, tag: &str, mcid: u32) {
        self.end_text();
        let line = format!("/{tag} << /MCID {mcid} >> BDC\n");
        self.emit(&line);
    }

    fn end_marked(&mut self) {
        self.end_text();
        self.emit("EMC\n");
    }

    /// Draw an image XObject into the box at `(x, y)` with the given size, all
    /// in layout units. `y` is the bottom edge, measured from the page bottom.
    fn image(&mut self, name: &str, x: i32, y: i32, width: i32, height: i32) {
        if width <= 0 || height <= 0 {
            return;
        }
        self.end_text();
        let line = format!(
            "q {} 0 0 {} {} {} cm /{name} Do Q\n",
            number(self.scale.milli(width)),
            number(self.scale.milli(height)),
            number(self.scale.milli(x)),
            number(self.scale.milli(y))
        );
        self.emit(&line);
    }
}

/// The rectangle an underline or a strikethrough occupies, in layout units,
/// anchored at the PDF's bottom-left corner.
///
/// `offset` is the distance from the baseline to the top of the rule, positive
/// downwards, which is how the font's `post` table states the underline
/// position. Baselines count down from the page top, while a PDF rectangle
/// grows up from the page bottom, so the vertical pass is flipped here — the
/// one place that knows both conventions.
#[allow(clippy::too_many_arguments)]
fn decoration_rect(
    em: i32,
    thickness_1000: i32,
    x: i32,
    advance: i32,
    baseline: i32,
    page_height: i32,
    offset: i32,
) -> [i32; 4] {
    let thickness = (em as i64 * thickness_1000 as i64 / 1000).max(1) as i32;
    let top = baseline + offset;
    [x, page_height - top - thickness, advance, thickness]
}

/// Paint one placed block.
///
/// `tags` is `Some` for the main reading order (body blocks) and `None` for
/// running heads, folios and footnote areas, which stay untagged: a structure
/// tree that repeated the header on every page would be worse than none.
fn paint_block(
    scene: &Scene<'_>,
    painter: &mut Painter,
    block: &PlacedBlock,
    extra_y: i32,
    mut tags: Option<&mut PageTags>,
    used_images: &mut Vec<usize>,
) {
    let layout = scene.layout;
    let styles = scene.styles;
    let text = scene.text;
    let fonts = scene.fonts;
    let style = styles
        .style(block.style)
        .cloned()
        .unwrap_or_else(ComputedStyle::base);
    match block.kind {
        BoxKind::Rule | BoxKind::TableRule => {
            painter.set_fill(style.color.unwrap_or(0x333333));
            let x = layout.page.margin_left.0 + block.x.0;
            let top = layout.page.margin_top.0 + block.y.0 + extra_y;
            let y = layout.page.height.0 - top - block.height.0;
            painter.rect(
                painter.scale.milli(x),
                painter.scale.milli(y),
                painter.scale.milli(block.width.0),
                painter.scale.milli(block.height.0),
            );
            return;
        }
        BoxKind::Figure => {
            let x = layout.page.margin_left.0 + block.x.0;
            let top = layout.page.margin_top.0 + block.y.0 + extra_y;
            let y = layout.page.height.0 - top - block.height.0;
            let mcid = tags
                .as_deref_mut()
                .map(|tags| tags.open("Figure", text.block_alt(block.node), None));
            if let Some(mcid) = mcid {
                painter.begin_marked("Figure", mcid);
            }
            match scene.image_for(block) {
                Some(index) => {
                    let object = &scene.images[index];
                    if !used_images.contains(&index) {
                        used_images.push(index);
                    }
                    painter.image(&object.name, x, y, block.width.0, block.height.0);
                }
                None => {
                    // No asset, or a format the backend cannot read: keep the
                    // placeholder so the layout stays honest about what the
                    // figure occupies.
                    painter.set_fill(0xEEEEEE);
                    painter.rect(
                        painter.scale.milli(x),
                        painter.scale.milli(y),
                        painter.scale.milli(block.width.0),
                        painter.scale.milli(block.height.0),
                    );
                    painter.stroke_rect(
                        painter.scale.milli(x),
                        painter.scale.milli(y),
                        painter.scale.milli(block.width.0),
                        painter.scale.milli(block.height.0),
                    );
                }
            }
            if mcid.is_some() {
                painter.end_marked();
            }
            return;
        }
        BoxKind::Math if block.math.is_some() => {
            let math = block.math.as_ref().unwrap();
            let left = layout.page.margin_left.0 + block.x.0;
            let top = layout.page.margin_top.0 + block.y.0 + extra_y;
            let mcid = tags.as_mut().map(|tags| tags.open("Formula", None, None));
            if let Some(mcid) = mcid {
                painter.begin_marked("Formula", mcid);
            }
            paint_math(
                painter,
                layout,
                fonts,
                math,
                left,
                top + math.baseline.0,
                style.color.unwrap_or(0),
                fonts.role_for(&style, false),
            );
            if mcid.is_some() {
                painter.end_marked();
            }
            return;
        }
        _ => {}
    }

    // Everything else is flowed text: paragraph, heading, code, a table cell.
    let tag = block_tag(scene, block);
    let mcid = match (tag, tags) {
        (Some(tag), Some(tags)) => {
            let alt = if tag == "Figure" {
                text.block_alt(block.node)
            } else {
                None
            };
            Some(tags.open(tag, alt, text.block_cell(block.node)))
        }
        _ => None,
    };
    if let (Some(tag), Some(mcid)) = (tag, mcid) {
        painter.begin_marked(tag, mcid);
    }

    for placed in &block.lines {
        let baseline =
            layout.page.margin_top.0 + block.y.0 + extra_y + placed.y.0 + placed.line.ascent.0;
        for atom in &placed.line.boxes {
            if let Some(inline) = block.inline_math.iter().find(|math| math.atom == atom.atom) {
                let left = layout.page.margin_left.0 + block.x.0 + atom.x.0;
                paint_math(
                    painter,
                    layout,
                    fonts,
                    &inline.math,
                    left,
                    baseline,
                    style.color.unwrap_or(0),
                    fonts.role_for(&style, false),
                );
                continue;
            }
            let text_value = text.atom_text(block.node, atom.atom);
            if text_value.is_empty() {
                continue;
            }
            let mark = block.marks.iter().find(|mark| mark.atom == atom.atom);
            // A footnote marker has no glyph of its own: it is drawn as its
            // number, like every other renderer does.
            if mark.is_none()
                && text_value
                    .chars()
                    .all(|character| character == '\u{fffc}' || character == FOOTNOTE_MARKER)
            {
                continue;
            }
            let style_id = text
                .atom_style(block.node, atom.atom)
                .unwrap_or(block.style);
            let atom_style = styles
                .style(style_id)
                .cloned()
                .unwrap_or_else(|| style.clone());
            let role = fonts.role_for(&atom_style, text.uses_fallback(block.node, atom.atom));
            let font = fonts.font(role);
            // A footnote reference is a superscript: smaller and raised.
            let full = painter.scale.size(&atom_style);
            let size = if mark.is_some() { full * 2 / 3 } else { full };
            let x = layout.page.margin_left.0 + block.x.0 + atom.x.0;
            let mut y = layout.page.height.0 - baseline;
            if mark.is_some() {
                y += (full / 3) as i32;
            }
            painter.set_fill(atom_style.color.unwrap_or(0));
            painter.start_text();
            painter.set_font(role, size);
            let glyphs: Vec<u16> = match mark {
                Some(mark) => format!("{}", mark.number)
                    .chars()
                    .map(|character| font.glyph(character as u32))
                    .collect(),
                None => text_value
                    .chars()
                    .map(|character| font.glyph(character as u32))
                    .collect(),
            };
            painter.show(painter.scale.milli(x), painter.scale.milli(y), &glyphs);

            // Decorations and links are decided by the source, not the style:
            // the renderer only asks the text index what the author asked for.
            let decorations = text.atom_decorations(block.node, atom.atom);
            if !decorations.is_empty() {
                // Rules are painted in layout units, so the font's 1000-unit
                // underline metrics scale by the atom's em, never by the
                // milli-point size (ADR-0044).
                let descriptor = font.descriptor();
                let em = atom_style.font_size.0;
                let advance = atom.advance.0;
                let page_height = layout.page.height.0;
                if decorations.contains(Decorations::UNDERLINE) {
                    let rect = decoration_rect(
                        em,
                        descriptor.underline_thickness,
                        x,
                        advance,
                        baseline,
                        page_height,
                        descriptor.underline_position.unsigned_abs() as i32,
                    );
                    painter.rule(rect[0], rect[1], rect[2], rect[3]);
                }
                if decorations.contains(Decorations::STRIKETHROUGH) {
                    let rect = decoration_rect(
                        em,
                        descriptor.underline_thickness,
                        x,
                        advance,
                        baseline,
                        page_height,
                        -em * 3 / 10,
                    );
                    painter.rule(rect[0], rect[1], rect[2], rect[3]);
                }
            }
            if let Some(uri) = text.atom_link(block.node, atom.atom) {
                let top = baseline - placed.line.ascent.0;
                let bottom = baseline + placed.line.descent.0;
                painter.link(
                    [
                        painter.scale.milli(x),
                        painter.scale.milli(layout.page.height.0 - bottom),
                        painter.scale.milli(x + atom.advance.0),
                        painter.scale.milli(layout.page.height.0 - top),
                    ],
                    uri,
                );
            }
        }
    }
    if mcid.is_some() {
        painter.end_marked();
    }
}

/// The structure type a block maps to, when it belongs in the reading order.
fn block_tag(scene: &Scene<'_>, block: &PlacedBlock) -> Option<&'static str> {
    match block.kind {
        BoxKind::Paragraph => Some("P"),
        BoxKind::Heading => Some(match scene.text.block_heading_level(block.node) {
            Some(1) => "H1",
            Some(2) => "H2",
            Some(3) => "H3",
            Some(4) => "H4",
            Some(5) => "H5",
            Some(_) => "H6",
            None => "P",
        }),
        BoxKind::Code => Some("Code"),
        // A table cell would need a `/Table` structure above it; tagging it as
        // a paragraph keeps the tree valid without inventing a hierarchy the
        // box tree does not have (a cell is a placed block, not a row of one).
        BoxKind::TableCell => Some("TD"),
        BoxKind::Footnote => Some("Note"),
        BoxKind::Figure | BoxKind::Math => None,
        BoxKind::Rule | BoxKind::TableRule => None,
    }
}

#[allow(clippy::too_many_arguments)]
fn paint_math(
    painter: &mut Painter,
    layout: &PagedLayout,
    fonts: &PdfFonts<'_>,
    math: &MathBox,
    left: i32,
    baseline: i32,
    paint: u32,
    text_role: Role,
) {
    let math_role = fonts.math_role();
    for item in &math.items {
        let x = left + item.x.0;
        let y = layout.page.height.0 - (baseline + item.y.0);
        let size = painter.scale.milli(item.size.0);
        painter.set_fill(item.color.unwrap_or(paint));
        painter.start_text();
        if item.stretch != 1000 {
            // Only braces stretch, and they are single glyphs: one face.
            let font = fonts.font(math_role);
            painter.set_font(math_role, size);
            let glyphs: Vec<u16> = item
                .text
                .chars()
                .map(|character| font.glyph(character as u32))
                .collect();
            painter.show_stretched(
                painter.scale.milli(x),
                painter.scale.milli(y),
                item.stretch,
                &glyphs,
            );
            continue;
        }
        // A formula's text is mostly math-face glyphs, but a `\text{...}` run
        // may hold characters only the text face carries. Draw contiguous
        // runs of one face, advancing by each glyph's own width so the run
        // starts where the layout put it.
        let mut runs: Vec<(Role, i64, Vec<u16>)> = Vec::new();
        let mut offset: i64 = 0;
        for character in item.text.chars() {
            let role = math_face(fonts, character, text_role);
            let glyph = fonts.font(role).glyph(character as u32);
            match runs.last_mut() {
                Some((last, _, glyphs)) if *last == role => glyphs.push(glyph),
                _ => runs.push((role, offset, vec![glyph])),
            }
            offset += i64::from(fonts.font(role).width_1000(glyph)) * size / 1000;
        }
        for (role, advance, glyphs) in runs {
            painter.set_font(role, size);
            painter.show(
                painter.scale.milli(x) + advance,
                painter.scale.milli(y),
                &glyphs,
            );
        }
    }
    for rule in &math.rules {
        let x = left + rule.x.0;
        let top = baseline + rule.y.0;
        let y = layout.page.height.0 - top - rule.thickness.0;
        painter.set_fill(paint);
        painter.rect(
            painter.scale.milli(x),
            painter.scale.milli(y),
            painter.scale.milli(rule.width.0),
            painter.scale.milli(rule.thickness.0),
        );
    }
}

// ---------------------------------------------------------------------------
// PDF objects
// ---------------------------------------------------------------------------

struct Pdf {
    objects: Vec<Vec<u8>>,
}

impl Pdf {
    fn new() -> Self {
        Self {
            objects: Vec::new(),
        }
    }

    fn reserve(&mut self) -> u32 {
        self.objects.push(Vec::new());
        self.objects.len() as u32
    }

    fn set(&mut self, id: u32, body: Vec<u8>) {
        self.objects[id as usize - 1] = body;
    }

    fn finish(self, catalog: u32, info: u32) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n");
        let mut offsets = Vec::with_capacity(self.objects.len());
        for (index, body) in self.objects.iter().enumerate() {
            offsets.push(out.len());
            out.extend_from_slice(format!("{} 0 obj\n", index + 1).as_bytes());
            out.extend_from_slice(body);
            out.push(b'\n');
            out.extend_from_slice(b"endobj\n");
        }
        let xref = out.len();
        out.extend_from_slice(format!("xref\n0 {}\n", self.objects.len() + 1).as_bytes());
        out.extend_from_slice(b"0000000000 65535 f \n");
        for offset in &offsets {
            out.extend_from_slice(format!("{:010} 00000 n \n", offset).as_bytes());
        }
        out.extend_from_slice(
            format!(
                "trailer\n<< /Size {} /Root {catalog} 0 R /Info {info} 0 R >>\nstartxref\n{xref}\n%%EOF\n",
                self.objects.len() + 1
            )
            .as_bytes(),
        );
        out
    }
}

/// A stream, deflated when that is smaller. The dictionary carries the filter
/// so callers never have to remember it.
fn stream_flate(dict: &[u8], data: &[u8]) -> Vec<u8> {
    match deflate::zlib(data) {
        Some(packed) => {
            let mut full = Vec::with_capacity(dict.len() + 24);
            full.extend_from_slice(dict);
            if !dict.is_empty() {
                full.push(b' ');
            }
            full.extend_from_slice(b"/Filter /FlateDecode");
            stream(&full, &packed)
        }
        None => stream(dict, data),
    }
}

fn stream(dict: &[u8], data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(dict.len() + data.len() + 32);
    out.extend_from_slice(b"<< ");
    out.extend_from_slice(dict);
    // The EOL before `endstream` is part of the stream data, so it is counted.
    out.extend_from_slice(format!(" /Length {}", data.len() + 1).as_bytes());
    out.extend_from_slice(b" >>\nstream\n");
    out.extend_from_slice(data);
    out.extend_from_slice(b"\nendstream");
    out
}

fn pdf_string(value: &str) -> String {
    if value.is_ascii() {
        let mut out = String::from("(");
        for character in value.chars() {
            match character {
                '(' | ')' | '\\' => {
                    out.push('\\');
                    out.push(character);
                }
                character => out.push(character),
            }
        }
        out.push(')');
        out
    } else {
        utf16_hex(value)
    }
}

fn utf16_hex(value: &str) -> String {
    let mut out = String::from("<FEFF");
    for unit in value.encode_utf16() {
        let _ = write!(out, "{:04X}", unit);
    }
    out.push('>');
    out
}

fn info_object(metadata: &PdfMetadata<'_>) -> Vec<u8> {
    let mut out = String::from("<< /Producer (soma-typeset)");
    if let Some(title) = metadata.title.filter(|value| !value.is_empty()) {
        let _ = write!(out, " /Title {}", utf16_hex(title));
    }
    if let Some(author) = metadata.author.filter(|value| !value.is_empty()) {
        let _ = write!(out, " /Author {}", utf16_hex(author));
    }
    out.push_str(" >>");
    out.into_bytes()
}

// ---------------------------------------------------------------------------
// Bookmarks and page labels
// ---------------------------------------------------------------------------

struct OutlineNode {
    entry: usize,
    parent: Option<usize>,
    children: Vec<usize>,
    first: Option<u32>,
    last: Option<u32>,
    prev: Option<u32>,
    next: Option<u32>,
    object: u32,
}

fn pdf_outline(
    pdf: &mut Pdf,
    entries: &[OutlineEntry],
    page_ids: &[u32],
    layout: &PagedLayout,
    scale: Scale,
) -> Option<u32> {
    if entries.is_empty() || page_ids.is_empty() {
        return None;
    }
    let mut nodes: Vec<OutlineNode> = Vec::with_capacity(entries.len());
    let mut roots: Vec<usize> = Vec::new();
    let mut stack: Vec<usize> = Vec::new();
    for (index, entry) in entries.iter().enumerate() {
        while let Some(&top) = stack.last() {
            if entries[nodes[top].entry].level >= entry.level {
                stack.pop();
            } else {
                break;
            }
        }
        let node = nodes.len();
        nodes.push(OutlineNode {
            entry: index,
            parent: stack.last().copied(),
            children: Vec::new(),
            first: None,
            last: None,
            prev: None,
            next: None,
            object: 0,
        });
        if let Some(&parent) = stack.last() {
            nodes[parent].children.push(node);
        } else {
            roots.push(node);
        }
        stack.push(node);
    }

    for node in &mut nodes {
        node.object = pdf.reserve();
    }
    let root_object = pdf.reserve();
    for index in 0..nodes.len() {
        let child_count = nodes[index].children.len();
        for position in 0..child_count {
            let child = nodes[index].children[position];
            nodes[child].parent = Some(index);
            let object = nodes[child].object;
            if position == 0 {
                nodes[index].first = Some(object);
            }
            if position + 1 == child_count {
                nodes[index].last = Some(object);
            }
            if position > 0 {
                nodes[child].prev = Some(nodes[nodes[index].children[position - 1]].object);
            }
            if position + 1 < child_count {
                nodes[child].next = Some(nodes[nodes[index].children[position + 1]].object);
            }
        }
    }
    for position in 0..roots.len() {
        let root = roots[position];
        if position > 0 {
            nodes[root].prev = Some(nodes[roots[position - 1]].object);
        }
        if position + 1 < roots.len() {
            nodes[root].next = Some(nodes[roots[position + 1]].object);
        }
    }

    let page_height = number(scale.milli(layout.page.height.0));
    let mut bodies = Vec::with_capacity(nodes.len());
    for (node_index, node) in nodes.iter().enumerate() {
        let entry = &entries[node.entry];
        let page = page_ids
            .get(entry.page.saturating_sub(1) as usize)
            .copied()
            .or_else(|| page_ids.first().copied())
            .unwrap_or(1);
        let mut body = String::from("<< /Title ");
        body.push_str(&utf16_hex(&entry.text));
        let parent = node
            .parent
            .and_then(|parent| nodes.get(parent))
            .map(|parent| parent.object)
            .unwrap_or(root_object);
        let _ = write!(body, " /Parent {parent} 0 R");
        if let Some(prev) = node.prev {
            let _ = write!(body, " /Prev {prev} 0 R");
        }
        if let Some(next) = node.next {
            let _ = write!(body, " /Next {next} 0 R");
        }
        if let Some(first) = node.first {
            let _ = write!(body, " /First {first} 0 R");
        }
        if let Some(last) = node.last {
            let _ = write!(body, " /Last {last} 0 R");
        }
        if !node.children.is_empty() {
            let _ = write!(body, " /Count {}", descendants(&nodes, node_index));
        }
        let _ = write!(body, " /Dest [{page} 0 R /XYZ 0 {page_height} null] >>");
        bodies.push(body);
    }
    for (node, body) in nodes.iter().zip(bodies) {
        pdf.set(node.object, body.into_bytes());
    }

    let mut root = String::from("<< /Type /Outlines");
    if let Some(first) = roots.first() {
        let _ = write!(root, " /First {} 0 R", nodes[*first].object);
    }
    if let Some(first) = roots.first() {
        let _ = write!(root, " /Last {} 0 R", nodes[*first].object);
    }
    let _ = write!(root, " /Count {} >>", entries.len());
    pdf.set(root_object, root.into_bytes());
    Some(root_object)
}

fn descendants(nodes: &[OutlineNode], index: usize) -> usize {
    nodes[index]
        .children
        .iter()
        .map(|child| 1 + descendants(nodes, *child))
        .sum()
}

fn page_labels(pdf: &mut Pdf, pages: &[soma_layout::Page]) -> Option<u32> {
    let mut ranges: Vec<(usize, char, u32)> = Vec::new();
    for (index, page) in pages.iter().enumerate() {
        if let Some(value) = decimal(&page.printed) {
            if ranges
                .last()
                .map(|(_, kind, _)| *kind != 'D')
                .unwrap_or(true)
            {
                ranges.push((index, 'D', value));
            }
        } else if let Some(value) = roman(&page.printed) {
            if ranges
                .last()
                .map(|(_, kind, _)| *kind != 'r')
                .unwrap_or(true)
            {
                ranges.push((index, 'r', value));
            }
        }
    }
    if ranges.is_empty() {
        return None;
    }
    let mut body = String::from("<< /Nums [");
    for (index, kind, start) in ranges {
        let _ = write!(body, "{index} << /S /{kind} /St {start} >> ");
    }
    body.push_str("] >>");
    let id = pdf.reserve();
    pdf.set(id, body.into_bytes());
    Some(id)
}

fn decimal(value: &str) -> Option<u32> {
    if value.is_empty() {
        return None;
    }
    value.parse::<u32>().ok()
}

fn roman(value: &str) -> Option<u32> {
    if value.is_empty() {
        return None;
    }
    let mut total = 0u32;
    let mut previous = 0u32;
    for character in value.chars().rev() {
        let current = match character {
            'i' => 1,
            'v' => 5,
            'x' => 10,
            'l' => 50,
            'c' => 100,
            'd' => 500,
            'm' => 1000,
            _ => return None,
        };
        if current < previous {
            total -= current;
        } else {
            total += current;
            previous = current;
        }
    }
    if total == 0 {
        None
    } else {
        Some(total)
    }
}

#[cfg(test)]
mod painter_tests {
    use super::{decoration_rect, Painter, Scale};

    fn painter() -> Painter {
        Painter {
            out: Vec::new(),
            scale: Scale { num: 1, den: 1 },
            fill: None,
            text_open: false,
            font: None,
            size: 0,
            links: Vec::new(),
        }
    }

    #[test]
    fn a_rule_is_a_filled_rectangle() {
        let mut painter = painter();
        // One em is 4096 layout units, so these are 10, 20, 30 and 2 points.
        let em = soma_core::UNITS_PER_EM;
        painter.rule(em * 10, em * 20, em * 30, em * 2);
        assert_eq!(
            String::from_utf8_lossy(&painter.out),
            "10.000 20.000 30.000 2.000 re f\n",
            "an underline is a filled rule, not a stroke"
        );
    }

    #[test]
    fn an_underline_sits_below_the_baseline() {
        // A body-em atom with a 50/1000 underline under a baseline 10000 units
        // down a 40000-unit page. The rule must land inside the text band, not
        // at the page bottom (ADR-0044).
        let rect = decoration_rect(4096, 50, 700, 3000, 10_000, 40_000, 512);
        assert_eq!(rect[0], 700);
        assert_eq!(rect[2], 3000);
        assert_eq!(rect[3], 204, "50/1000 of one em, in layout units");
        // The rectangle's top edge is baseline + offset, measured from the page
        // top, so it flips to 40000 - 10512 - 204 from the bottom.
        assert_eq!(rect[1], 40_000 - 10_512 - 204);
        assert_eq!(rect[1] + rect[3], 40_000 - 10_512);
    }

    #[test]
    fn a_strikeout_sits_above_the_baseline() {
        let rect = decoration_rect(4096, 50, 0, 1000, 10_000, 40_000, -4096 * 3 / 10);
        assert_eq!(rect[1] + rect[3], 40_000 - 10_000 + 1228);
    }

    #[test]
    fn a_hairline_underline_is_at_least_one_unit() {
        // A font that reports 0 thickness still gets a drawable rule.
        let rect = decoration_rect(4096, 0, 0, 1000, 100, 10_000, 100);
        assert_eq!(rect[3], 1);
    }

    #[test]
    fn adjacent_links_with_one_target_merge() {
        let mut painter = painter();
        painter.link([0, 0, 10, 12], "https://a");
        painter.link([10, 0, 25, 12], "https://a");
        painter.link([0, 40, 10, 52], "https://a");
        painter.link([0, 0, 5, 12], "https://b");
        assert_eq!(painter.links.len(), 3);
        assert_eq!(painter.links[0].rect, [0, 0, 25, 12]);
    }
}
