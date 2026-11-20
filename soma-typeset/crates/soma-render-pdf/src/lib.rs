//! PDF renderer: consumes `PagedLayout` + `StyledDoc`/`TextSource` and writes a
//! deterministic PDF with embedded TrueType subsets, bookmarks and page labels.
//!
//! It never breaks lines, never paginates, and never reads DocIR. Fonts are
//! supplied by the caller (the CLI reads them from disk); the renderer keeps
//! only the glyphs the layout actually uses.

#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]

extern crate alloc;

mod cff;
mod deflate;
mod font;
mod standard_encoding;

pub use cff::CidSystem;
pub use font::{PdfError, PdfFont, PdfFontDescriptor, ProgramKind, Subset};

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::fmt::Write;

use soma_layout::{BoxKind, MathBox, OutlineEntry, PagedLayout, PlacedBlock, TextSource};
use soma_template::{ComputedStyle, FontWeight, StyledDoc};

const UNITS_PER_EM: i64 = 4096;
const FOOTNOTE_MARKER: char = '\u{e000}';

/// Fonts available to the PDF renderer. Bold and math fall back to `regular`.
pub struct PdfFonts<'a> {
    pub regular: &'a PdfFont,
    pub bold: Option<&'a PdfFont>,
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
    Math,
    Fallback,
}

impl Role {
    const ALL: [Role; 4] = [Role::Regular, Role::Bold, Role::Math, Role::Fallback];

    fn resource(self) -> &'static str {
        match self {
            Role::Regular => "F1",
            Role::Bold => "F2",
            Role::Math => "F3",
            Role::Fallback => "F4",
        }
    }
}

/// Render every page to one PDF file.
pub fn render(
    layout: &PagedLayout,
    styles: &StyledDoc,
    text: &dyn TextSource,
    fonts: &PdfFonts<'_>,
    metadata: &PdfMetadata<'_>,
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

    let mut page_ids = Vec::with_capacity(layout.pages.len());
    for page in &layout.pages {
        let content = paint_page(layout, styles, text, fonts, scale, page);
        let content_id = pdf.reserve();
        pdf.set(content_id, stream_flate(b"", &content));

        let mut font_resources = String::new();
        for (role, font) in &embedded {
            let _ = write!(font_resources, "/{} {} 0 R ", role.resource(), font.type0);
        }
        let page_id = pdf.reserve();
        let body = format!(
            "<< /Type /Page /Parent {pages_object} 0 R /MediaBox [0 0 {} {}] /Resources << /Font << {font_resources}>> >> /Contents {content_id} 0 R >>",
            number(scale.milli(layout.page.width.0)),
            number(scale.milli(layout.page.height.0))
        );
        pdf.set(page_id, body.into_bytes());
        page_ids.push(page_id);
    }

    let outline = pdf_outline(&mut pdf, &layout.outline, &page_ids, layout, scale);
    let labels = page_labels(&mut pdf, &layout.pages);

    let info = pdf.reserve();
    pdf.set(info, info_object(metadata));

    let mut catalog_body = format!("<< /Type /Catalog /Pages {pages_object} 0 R");
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

    let mut kids = String::new();
    for id in &page_ids {
        let _ = write!(kids, "{id} 0 R ");
    }
    pdf.set(
        pages_object,
        format!(
            "<< /Type /Pages /Kids [{kids}] /Count {} >>",
            page_ids.len()
        )
        .into_bytes(),
    );

    Ok(pdf.finish(catalog, info))
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
            Role::Math => &mut self.math,
            Role::Fallback => &mut self.fallback,
        }
    }

    fn has(&self, role: Role, fonts: &PdfFonts<'_>) -> bool {
        match role {
            Role::Regular => !self.regular.is_empty(),
            Role::Bold => fonts.bold.is_some() && !self.bold.is_empty(),
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
            Role::Math => self.math.unwrap_or(self.regular),
            Role::Fallback => self.fallback.unwrap_or(self.regular),
            Role::Regular => self.regular,
        }
    }

    fn role_for(&self, style: &ComputedStyle, fallback: bool) -> Role {
        if fallback && self.fallback.is_some() {
            Role::Fallback
        } else if style.weight == FontWeight::Bold && self.bold.is_some() {
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
        for placed in &block.lines {
            for atom in &placed.line.boxes {
                if let Some(inline) = block.inline_math.iter().find(|math| math.atom == atom.atom) {
                    collect_math(&inline.math, fonts, used);
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
            collect_math(math, fonts, used);
        }
    }
}

fn collect_math(math: &MathBox, fonts: &PdfFonts<'_>, used: &mut Used) {
    let role = fonts.math_role();
    for item in &math.items {
        for character in item.text.chars() {
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
    layout: &PagedLayout,
    styles: &StyledDoc,
    text: &dyn TextSource,
    fonts: &PdfFonts<'_>,
    scale: Scale,
    page: &soma_layout::Page,
) -> Vec<u8> {
    let mut painter = Painter {
        out: Vec::new(),
        scale,
        fill: None,
        text_open: false,
        font: None,
        size: 0,
    };
    for block in &page.blocks {
        paint_block(&mut painter, layout, styles, text, fonts, block, 0);
    }
    for block in &page.header {
        paint_block(&mut painter, layout, styles, text, fonts, block, 0);
    }
    for block in &page.footer {
        paint_block(&mut painter, layout, styles, text, fonts, block, 0);
    }
    let footnote_top = layout.page.content_height().0 - page.footnote_height.0;
    for block in &page.footnotes {
        paint_block(
            &mut painter,
            layout,
            styles,
            text,
            fonts,
            block,
            footnote_top.max(0),
        );
    }
    painter.end_text();
    painter.out
}

struct Painter {
    out: Vec<u8>,
    scale: Scale,
    fill: Option<u32>,
    text_open: bool,
    font: Option<Role>,
    size: i64,
}

impl Painter {
    fn emit(&mut self, value: &str) {
        self.out.extend_from_slice(value.as_bytes());
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
}

fn paint_block(
    painter: &mut Painter,
    layout: &PagedLayout,
    styles: &StyledDoc,
    text: &dyn TextSource,
    fonts: &PdfFonts<'_>,
    block: &PlacedBlock,
    extra_y: i32,
) {
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
            return;
        }
        BoxKind::Math if block.math.is_some() => {
            let math = block.math.as_ref().unwrap();
            let left = layout.page.margin_left.0 + block.x.0;
            let top = layout.page.margin_top.0 + block.y.0 + extra_y;
            paint_math(
                painter,
                layout,
                fonts,
                math,
                left,
                top + math.baseline.0,
                style.color.unwrap_or(0),
            );
            return;
        }
        _ => {}
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
        }
    }
}

fn paint_math(
    painter: &mut Painter,
    layout: &PagedLayout,
    fonts: &PdfFonts<'_>,
    math: &MathBox,
    left: i32,
    baseline: i32,
    paint: u32,
) {
    let role = fonts.math_role();
    let font = fonts.font(role);
    for item in &math.items {
        let x = left + item.x.0;
        let y = layout.page.height.0 - (baseline + item.y.0);
        let size = painter.scale.milli(item.size.0);
        painter.set_fill(paint);
        painter.start_text();
        painter.set_font(role, size);
        let glyphs: Vec<u16> = item
            .text
            .chars()
            .map(|character| font.glyph(character as u32))
            .collect();
        painter.show(painter.scale.milli(x), painter.scale.milli(y), &glyphs);
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
