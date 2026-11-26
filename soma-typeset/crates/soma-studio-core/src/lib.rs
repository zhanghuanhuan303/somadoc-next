//! Shared build pipeline for Soma Studio.
//!
//! This crate is the one place where the browser and the Axum server agree on
//! what "building a document" means. It parses the source, resolves the
//! template, selects the engine parameters, measures with the embedded metrics
//! table, runs the page layout, and exposes the finished `PagedLayout` plus
//! HTML page fragments.
//!
//! It is deliberately I/O-free: file reads, image fetches, fonts and PDF
//! writing live in `soma-studio` (native) or in the Leptos UI. The engine
//! crates stay dependency-free; only this shared layer uses `serde`.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use soma_core::{BreakConfig, ParamSet, StyleId, VerticalParams, Width};
use soma_doc::{Block, Document, ImageIntrinsic, ImageRef, Inline};
use soma_engine::EngineConfig;
use soma_layout::{ImageSource, PagedLayout, PaginationStrategy, StyledMetrics};
use soma_metrics::{MetricsTable, FAMILY_FALLBACK, FAMILY_MONO, FAMILY_SANS, FAMILY_SERIF};
use soma_render_html::PageFragment;
use soma_render_svg::SvgPage;
use soma_template::{parse_template, resolve, ComputedStyle, FontWeight, StyledDoc};

/// The frontend selected for a source.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SourceFormat {
    #[default]
    Markdown,
    Html,
    Latex,
    Docx,
}

impl SourceFormat {
    pub fn from_path(path: &str) -> Option<Self> {
        let extension = std::path::Path::new(path)
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        match extension.as_str() {
            "md" | "markdown" => Some(Self::Markdown),
            "html" | "htm" => Some(Self::Html),
            "tex" | "latex" => Some(Self::Latex),
            "docx" => Some(Self::Docx),
            _ => None,
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Markdown => "markdown",
            Self::Html => "html",
            Self::Latex => "latex",
            Self::Docx => "docx",
        }
    }

    pub const fn extension(self) -> &'static str {
        match self {
            Self::Markdown => "md",
            Self::Html => "html",
            Self::Latex => "tex",
            Self::Docx => "docx",
        }
    }
}

/// A parsed source plus media that travelled inside it (a `.docx` archive).
pub struct ParsedDocument {
    pub document: Document,
    pub media: Vec<soma_docx::MediaFile>,
    pub format: SourceFormat,
}

/// Image bytes keyed by the `src` the DocIR carries.
#[derive(Clone, Debug, Default)]
pub struct ImageBytes {
    files: BTreeMap<String, Vec<u8>>,
}

impl ImageBytes {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, src: impl Into<String>, bytes: Vec<u8>) {
        self.files.insert(src.into(), bytes);
    }

    pub fn get(&self, src: &str) -> Option<&[u8]> {
        self.files.get(src).map(Vec::as_slice)
    }

    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }

    pub fn from_media(media: &[soma_docx::MediaFile]) -> Self {
        let mut files = BTreeMap::new();
        for file in media {
            files.insert(file.name.clone(), file.bytes.clone());
        }
        Self { files }
    }
}

/// An `ImageSource` over a map; the browser and the server share it.
pub struct MapImages<'a> {
    files: &'a ImageBytes,
}

impl<'a> MapImages<'a> {
    pub fn new(files: &'a ImageBytes) -> Self {
        Self { files }
    }
}

impl ImageSource for MapImages<'_> {
    fn image(&self, src: &str) -> Option<&[u8]> {
        self.files.get(src)
    }
}

/// What the server needs to report about a native build.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeSummary {
    pub pages: u32,
    pub blocks: usize,
    pub lines: usize,
    pub hash: String,
    pub params_hash: String,
    pub rules_version: u16,
    pub metrics_version: u16,
    pub engine_hash: String,
    pub elapsed_ms: u64,
    /// Lines per physical page, for the wasm/native page-by-page check.
    #[serde(default)]
    pub page_lines: Vec<usize>,
}

/// One character sampled for the browser font probe.
///
/// The expected advance comes from the same `.smt` table the layout used, so
/// the browser can compare its own measurement with the engine without
/// re-deriving which face covers the character.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProbeSample {
    pub text: String,
    /// `serif`, `sans`, `mono` or `fallback`: the role whose family the
    /// browser must measure.
    pub role: String,
    pub bold: bool,
    /// Advance per em in 1/4096 units, from the metrics table.
    pub advance: i32,
}

/// Compile the first diagram in a parsed document to TikZ. The Studio uses
/// the same metrics, template and computed diagram style as the proof, so the
/// exported node coordinates match the natural (unscaled) diagram box.
pub fn diagram_tikz(parsed: &ParsedDocument, options: BuildOptions<'_>) -> Result<String, String> {
    let diagram = find_diagram(&parsed.document.blocks)
        .ok_or_else(|| String::from("文档里没有 Mermaid 或 DOT 图"))?;
    let template =
        parse_template(options.template).map_err(|error| format!("template: {error}"))?;
    let styled =
        resolve(&parsed.document, &template).map_err(|error| format!("template: {error}"))?;
    let table = match options.metrics {
        Some(bytes) => MetricsTable::parse(bytes).map_err(|error| error.to_string())?,
        None => MetricsTable::embedded(),
    };
    let metrics = StyledMetrics::new(&table, &styled);
    let style = styled.style_of(diagram.meta.id).unwrap_or(StyleId(0));
    let computed = styled
        .style(style)
        .cloned()
        .unwrap_or_else(ComputedStyle::base);
    let options = soma_diagram::Options {
        label_size: computed.font_size,
        line_gap: computed.line_height,
        line_width: Width::from_em_frac(1, 25),
        max_label_width: computed.font_size.scale(16, 1),
    };
    let mut text = soma_layout::DiagramText::new(&computed, &metrics, style);
    let layout = soma_diagram::layout(diagram.dialect, &diagram.source, &options, &mut text)
        .map_err(|error| error.to_string())?;
    Ok(soma_diagram::to_tikz(&layout))
}

fn find_diagram(blocks: &[Block]) -> Option<&soma_doc::DiagramBlock> {
    for block in blocks {
        match block {
            Block::Diagram(node) => return Some(node),
            Block::List(node) => {
                for item in &node.items {
                    if let Some(found) = find_diagram(&item.blocks) {
                        return Some(found);
                    }
                }
            }
            Block::BlockQuote(node) => {
                if let Some(found) = find_diagram(&node.blocks) {
                    return Some(found);
                }
            }
            Block::Table(node) => {
                for row in &node.rows {
                    for cell in &row.cells {
                        if let Some(found) = find_diagram(&cell.blocks) {
                            return Some(found);
                        }
                    }
                }
            }
            Block::Generated(node) => {
                if let Some(found) = find_diagram(&node.blocks) {
                    return Some(found);
                }
            }
            _ => {}
        }
    }
    None
}

pub struct Build {
    pub document: Document,
    pub styled: StyledDoc,
    pub pages: PagedLayout,
    pub params: ParamSet,
    pub params_hash: u64,
    /// Media that travelled inside the source (a `.docx` archive). The PDF
    /// backend and the HTML image source both use it.
    pub media: Vec<soma_docx::MediaFile>,
    /// Characters for the browser-side font probe (D101): the engine's
    /// advances versus the browser's measurement of the installed face.
    pub probe: Vec<ProbeSample>,
}

impl Build {
    pub fn fragments(&self, images: &ImageBytes) -> Vec<PageFragment> {
        let source = MapImages::new(images);
        soma_render_html::render_fragments_with(
            &self.pages,
            self.styled.table(),
            &self.pages.text,
            &source,
        )
    }

    /// SVG proof pages: one `<svg class="page">` per page, scaled by the
    /// caller's page slot. Images are inlined data URIs.
    pub fn svg_fragments(&self, images: &ImageBytes) -> Vec<SvgPage> {
        let source = MapImages::new(images);
        soma_render_svg::render_fragments_with(
            &self.pages,
            self.styled.table(),
            &self.pages.text,
            &source,
        )
    }

    pub fn summary(&self, elapsed_ms: u64) -> NativeSummary {
        let blocks = self.pages.pages.iter().map(|page| page.blocks.len()).sum();
        let lines = self
            .pages
            .pages
            .iter()
            .flat_map(|page| page.blocks.iter())
            .map(|block| block.lines.len())
            .sum();
        NativeSummary {
            pages: self.pages.page_count(),
            blocks,
            lines,
            hash: format!("{:016x}", self.pages.hash()),
            params_hash: format!("{:016x}", self.params_hash),
            rules_version: self.pages.rules_version,
            metrics_version: self.pages.metrics_version,
            engine_hash: format!("{:016x}", self.pages.engine_hash),
            elapsed_ms,
            page_lines: self
                .pages
                .pages
                .iter()
                .map(|page| page.blocks.iter().map(|block| block.lines.len()).sum())
                .collect(),
        }
    }
}

/// Inputs to [`build`].
#[derive(Default)]
pub struct BuildOptions<'a> {
    pub template: &'a str,
    pub engine: Option<&'a str>,
    pub metrics: Option<&'a [u8]>,
    pub hyphenation: Option<&'a str>,
}

/// Parse source bytes with the same frontends the CLI uses.
pub fn parse_document(format: SourceFormat, bytes: &[u8]) -> Result<ParsedDocument, String> {
    if format == SourceFormat::Docx {
        let (document, media) =
            soma_docx::parse_docx_with_media(bytes).map_err(|error| error.reason)?;
        return Ok(ParsedDocument {
            document,
            media,
            format,
        });
    }
    let source = std::str::from_utf8(bytes)
        .map_err(|error| format!("input is not valid UTF-8 (at byte {})", error.valid_up_to()))?;
    let document = match format {
        SourceFormat::Markdown => soma_md::parse_markdown(source),
        SourceFormat::Html => soma_html::parse_html(source),
        SourceFormat::Latex => soma_latex::parse_latex(source),
        SourceFormat::Docx => unreachable!("handled above"),
    };
    Ok(ParsedDocument {
        document,
        media: Vec::new(),
        format,
    })
}

/// Fill image intrinsic sizes from bytes the host has (filesystem or fetch).
/// A missing image leaves the layout's aspect fallback alone.
pub fn enrich_images(parsed: &mut ParsedDocument, images: &ImageBytes) {
    let ParsedDocument {
        document, media, ..
    } = parsed;
    enrich_blocks(&mut document.blocks, images, media);
    for footnote in &mut document.footnotes {
        enrich_blocks(&mut footnote.blocks, images, media);
    }
}

/// Local image paths the host must load before [`enrich_images`] / rendering.
pub fn image_sources(parsed: &ParsedDocument) -> Vec<String> {
    let mut set = BTreeSet::new();
    collect_block_images(&parsed.document.blocks, &mut set);
    for footnote in &parsed.document.footnotes {
        collect_block_images(&footnote.blocks, &mut set);
    }
    for file in &parsed.media {
        set.insert(file.name.clone());
    }
    set.into_iter().collect()
}

/// Run the full pipeline. This is the shared browser/server build.
pub fn build(parsed: ParsedDocument, options: BuildOptions<'_>) -> Result<Build, String> {
    let template =
        parse_template(options.template).map_err(|error| format!("template: {error}"))?;
    let styled =
        resolve(&parsed.document, &template).map_err(|error| format!("template: {error}"))?;

    let (engine, vertical) = match options.engine {
        Some(source) => {
            let config = EngineConfig::parse(source).map_err(|error| error.to_string())?;
            (config.break_config(), config.vertical())
        }
        // The Studio is the Chinese-first proposal workbench: its fallback
        // preset charges a short last line up to five characters at twice the
        // book rate, so a 4–5 character tail does not survive (ADR-0115). The
        // command line keeps `latex-v1` (ADR-0069).
        None => (
            BreakConfig::from_params(ParamSet::proposal_v1()),
            VerticalParams::default_v1(),
        ),
    };

    let table = match options.metrics {
        Some(bytes) => MetricsTable::parse(bytes).map_err(|error| error.to_string())?,
        None => MetricsTable::embedded(),
    };
    let metrics = StyledMetrics::new(&table, &styled);
    let patterns = options.hyphenation.map(soma_hyphen::Patterns::parse);

    let pages = soma_layout::layout_with_hyphenation(
        &parsed.document,
        &styled,
        &metrics,
        &engine,
        PaginationStrategy::VerticalKp,
        vertical,
        patterns.as_ref(),
    )
    .map_err(|error| error.to_string())?;

    let probe = probe_samples(&table, &styled);

    Ok(Build {
        document: parsed.document,
        styled,
        pages,
        params: *engine.params(),
        params_hash: engine.param_hash().0,
        media: parsed.media,
        probe,
    })
}

/// Characters probed against the browser font, in reading order.
const PROBE_TEXT: &[&str] = &[
    "永", "国", "中", "文", "排", "版", "，", "。", "、", "…", "—", "A", "g", "0", "W", ")",
];

/// Expected advances for the browser font probe.
///
/// One sample per character, carrying the face role the browser has a CSS
/// family for. A character the preferred face does not cover is attributed to
/// the fallback face, matching `StyledMetrics::face_for`.
fn probe_samples(table: &MetricsTable, styled: &StyledDoc) -> Vec<ProbeSample> {
    let computed = styled
        .style(StyleId(0))
        .cloned()
        .unwrap_or_else(ComputedStyle::base);
    let role = soma_template::family_role(computed.family.as_deref());
    let family = match role {
        soma_template::FamilyRole::Serif => FAMILY_SERIF,
        soma_template::FamilyRole::Sans => FAMILY_SANS,
        soma_template::FamilyRole::Mono => FAMILY_MONO,
    };
    let bold = computed.weight == FontWeight::Bold;
    let faces = table.faces();
    let preferred = faces
        .iter()
        .find(|face| face.family == family && (face.weight != 0) == bold)
        .or_else(|| faces.iter().find(|face| face.family == family))
        .or_else(|| table.face(0));
    let Some(preferred) = preferred else {
        return Vec::new();
    };
    let mut samples = Vec::new();
    for text in PROBE_TEXT {
        let face = if preferred.covers(text) {
            preferred
        } else {
            faces
                .iter()
                .find(|face| face.family == FAMILY_FALLBACK && face.covers(text))
                .unwrap_or(preferred)
        };
        let advance = face.advance(text);
        if advance <= 0 {
            continue;
        }
        let role = match face.family {
            FAMILY_SANS => "sans",
            FAMILY_MONO => "mono",
            FAMILY_FALLBACK => "fallback",
            _ => "serif",
        };
        samples.push(ProbeSample {
            text: (*text).to_string(),
            role: role.to_string(),
            bold: face.weight != 0,
            advance,
        });
    }
    samples
}

fn collect_block_images(blocks: &[Block], out: &mut BTreeSet<String>) {
    for block in blocks {
        match block {
            Block::Figure(node) => collect_image(&node.image.source, out),
            Block::Paragraph(node) => collect_inline_images(&node.inlines, out),
            Block::Heading(node) => collect_inline_images(&node.inlines, out),
            Block::Table(node) => {
                collect_inline_images(&node.caption, out);
                for row in &node.rows {
                    for cell in &row.cells {
                        collect_block_images(&cell.blocks, out);
                    }
                }
            }
            Block::List(node) => {
                for item in &node.items {
                    collect_block_images(&item.blocks, out);
                }
            }
            Block::BlockQuote(node) => collect_block_images(&node.blocks, out),
            Block::Generated(node) => collect_block_images(&node.blocks, out),
            _ => {}
        }
    }
}

fn collect_inline_images(inlines: &[Inline], out: &mut BTreeSet<String>) {
    for inline in inlines {
        match inline {
            Inline::Image(node) => collect_image(&node.source, out),
            Inline::Strong(node)
            | Inline::Emphasis(node)
            | Inline::Underline(node)
            | Inline::Strikethrough(node)
            | Inline::SmallCaps(node)
            | Inline::Superscript(node)
            | Inline::Subscript(node) => collect_inline_images(&node.children, out),
            Inline::Link(node) => collect_inline_images(&node.children, out),
            _ => {}
        }
    }
}

fn collect_image(image: &ImageRef, out: &mut BTreeSet<String>) {
    if image.src.contains("://") || image.src.starts_with("data:") {
        return;
    }
    out.insert(image.src.clone());
}

fn image_lookup<'a>(
    images: &'a ImageBytes,
    media: &'a [soma_docx::MediaFile],
    src: &str,
) -> Option<&'a [u8]> {
    images.get(src).or_else(|| {
        media
            .iter()
            .find(|file| file.name == src)
            .map(|file| file.bytes.as_slice())
    })
}

fn enrich_blocks(blocks: &mut [Block], images: &ImageBytes, media: &[soma_docx::MediaFile]) {
    for block in blocks {
        match block {
            Block::Figure(node) => enrich_image(&mut node.image.source, images, media),
            Block::Paragraph(node) => enrich_inlines(&mut node.inlines, images, media),
            Block::Heading(node) => enrich_inlines(&mut node.inlines, images, media),
            Block::Table(node) => {
                enrich_inlines(&mut node.caption, images, media);
                for row in &mut node.rows {
                    for cell in &mut row.cells {
                        enrich_blocks(&mut cell.blocks, images, media);
                    }
                }
            }
            Block::List(node) => {
                for item in &mut node.items {
                    enrich_blocks(&mut item.blocks, images, media);
                }
            }
            Block::BlockQuote(node) => enrich_blocks(&mut node.blocks, images, media),
            Block::Generated(node) => enrich_blocks(&mut node.blocks, images, media),
            _ => {}
        }
    }
}

fn enrich_inlines(inlines: &mut [Inline], images: &ImageBytes, media: &[soma_docx::MediaFile]) {
    for inline in inlines {
        match inline {
            Inline::Image(node) => enrich_image(&mut node.source, images, media),
            Inline::Strong(node)
            | Inline::Emphasis(node)
            | Inline::Underline(node)
            | Inline::Strikethrough(node)
            | Inline::SmallCaps(node)
            | Inline::Superscript(node)
            | Inline::Subscript(node) => enrich_inlines(&mut node.children, images, media),
            Inline::Link(node) => enrich_inlines(&mut node.children, images, media),
            _ => {}
        }
    }
}

fn enrich_image(image: &mut ImageRef, images: &ImageBytes, media: &[soma_docx::MediaFile]) {
    if image.intrinsic.is_some() || image.src.contains("://") || image.src.starts_with("data:") {
        return;
    }
    let Some(bytes) = image_lookup(images, media, &image.src) else {
        return;
    };
    if let Some((width_px, height_px)) = image_size(bytes) {
        image.intrinsic = Some(ImageIntrinsic {
            width_px,
            height_px,
        });
    }
}

/// PNG IHDR or a JPEG start-of-frame, in pixels.
pub fn image_size(bytes: &[u8]) -> Option<(u32, u32)> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") && bytes.len() >= 24 {
        let width = u32::from_be_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]);
        let height = u32::from_be_bytes([bytes[20], bytes[21], bytes[22], bytes[23]]);
        return (width > 0 && height > 0).then_some((width, height));
    }
    if !bytes.starts_with(&[0xFF, 0xD8]) {
        return None;
    }
    let mut at = 2usize;
    while at + 9 <= bytes.len() {
        if bytes[at] != 0xFF {
            at += 1;
            continue;
        }
        let marker = bytes[at + 1];
        if (0xD0..=0xD9).contains(&marker) || marker == 0x01 || marker == 0xFF {
            at += 2;
            continue;
        }
        let length = u16::from_be_bytes([bytes[at + 2], bytes[at + 3]]) as usize;
        if length < 2 {
            return None;
        }
        let frame = matches!(
            marker,
            0xC0..=0xC3 | 0xC5..=0xC7 | 0xC9..=0xCB | 0xCD..=0xCF
        );
        if frame && at + 9 <= bytes.len() {
            let height = u16::from_be_bytes([bytes[at + 5], bytes[at + 6]]) as u32;
            let width = u16::from_be_bytes([bytes[at + 7], bytes[at + 8]]) as u32;
            return (width > 0 && height > 0).then_some((width, height));
        }
        at += 2 + length;
    }
    None
}

/// The family name from an sfnt `name` table (TTF/OTF, first face of a TTC).
///
/// The Axum server uses this to emit `@font-face` rules for the exact files
/// the PDF backend embeds, so the browser and the PDF name the same family.
pub fn font_family_name(bytes: &[u8]) -> Option<String> {
    let base = if bytes.starts_with(b"ttcf") && bytes.len() >= 16 {
        u32::from_be_bytes([bytes[12], bytes[13], bytes[14], bytes[15]]) as usize
    } else {
        0
    };
    let num_tables = read_u16(bytes, base + 4)? as usize;
    let mut name_offset = None;
    let mut name_length = None;
    for index in 0..num_tables {
        let record = base.checked_add(12 + index * 16)?;
        let tag = read_u32(bytes, record)?;
        if tag == 0x6E61_6D65 {
            name_offset = read_u32(bytes, record + 8).map(|value| value as usize);
            name_length = read_u32(bytes, record + 12).map(|value| value as usize);
            break;
        }
    }
    let offset = name_offset?;
    let length = name_length?;
    let table = bytes.get(offset..offset.checked_add(length)?)?;

    let count = read_u16(table, 2)? as usize;
    let string_offset = read_u16(table, 4)? as usize;
    let mut candidates: Vec<(u16, u16, usize, usize)> = Vec::new();
    for index in 0..count {
        let record = 6 + index * 12;
        let platform = read_u16(table, record)?;
        let _encoding = read_u16(table, record + 2)?;
        let name_id = read_u16(table, record + 6)?;
        let text_length = read_u16(table, record + 8)? as usize;
        let text_offset = read_u16(table, record + 10)? as usize;
        if name_id == 1 || name_id == 16 {
            candidates.push((name_id, platform, text_offset, text_length));
        }
    }
    candidates.sort_by_key(|(name_id, platform, _, _)| {
        let id_rank = if *name_id == 16 { 0 } else { 1 };
        let platform_rank = match platform {
            3 => 0,
            0 => 1,
            1 => 2,
            _ => 3,
        };
        (id_rank, platform_rank)
    });

    for (_, platform, text_offset, text_length) in candidates {
        let start = string_offset.checked_add(text_offset)?;
        let raw = table.get(start..start.checked_add(text_length)?)?;
        let text = if platform == 0 || platform == 3 {
            decode_utf16_be(raw)
        } else {
            Some(
                String::from_utf8_lossy(raw)
                    .trim_end_matches('\0')
                    .to_string(),
            )
        };
        if let Some(text) = text.filter(|text| !text.is_empty()) {
            return Some(text);
        }
    }
    None
}

fn read_u16(bytes: &[u8], at: usize) -> Option<u16> {
    let slice = bytes.get(at..at.checked_add(2)?)?;
    Some(u16::from_be_bytes([slice[0], slice[1]]))
}

fn read_u32(bytes: &[u8], at: usize) -> Option<u32> {
    let slice = bytes.get(at..at.checked_add(4)?)?;
    Some(u32::from_be_bytes([slice[0], slice[1], slice[2], slice[3]]))
}

fn decode_utf16_be(bytes: &[u8]) -> Option<String> {
    if bytes.len() % 2 != 0 {
        return None;
    }
    let units: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|chunk| u16::from_be_bytes([chunk[0], chunk[1]]))
        .collect();
    String::from_utf16(&units).ok()
}

/* ------------------------------------------------------------------ API DTOs */

/// One font the server can serve to the browser.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FontInfo {
    pub role: String,
    pub family: String,
    pub url: String,
    pub weight: String,
    pub style: String,
}

/// A frontend capability row (mirrors `soma-md::CAPABILITIES`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CapabilityInfo {
    pub syntax: String,
    pub status: String,
    pub note: String,
}

/// Everything the UI needs on first paint.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectPayload {
    pub schema: String,
    pub version: u64,
    pub engine_version: String,
    pub root: String,
    pub input: String,
    pub input_relative: String,
    pub format: SourceFormat,
    pub source_text: Option<String>,
    pub template_path: Option<String>,
    pub template: String,
    pub engine_path: Option<String>,
    pub engine: Option<String>,
    pub fonts: Vec<FontInfo>,
    pub capabilities: Vec<CapabilityInfo>,
    pub native: NativeSummary,
    pub native_error: Option<String>,
    pub pdf: bool,
    /// True when the project carries a custom `soma.metrics` table. The UI
    /// fetches it once so the wasm build uses exactly the native table.
    pub custom_metrics: bool,
    /// True when the server loaded hyphenation patterns. The UI fetches the
    /// same text from `/api/hyphenation` before the first build.
    pub custom_hyphenation: bool,
}

/// A request that asks the server to build text the browser currently holds.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildRequest {
    /// `None` means "read the project input from disk" (used for binary DOCX).
    pub source: Option<String>,
    pub template: Option<String>,
    pub engine: Option<String>,
    /// The frontend of the tab being built. `None` keeps the project input's
    /// format, so older clients and the initial load behave as before.
    pub format: Option<SourceFormat>,
}

impl BuildRequest {
    /// The frontend this request should parse with.
    pub fn format_or(&self, fallback: SourceFormat) -> SourceFormat {
        self.format.unwrap_or(fallback)
    }
}

/// A file in the project tree.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileEntry {
    pub path: String,
    pub name: String,
    pub kind: String,
    pub size: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TreeResponse {
    pub root: String,
    pub entries: Vec<FileEntry>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveRequest {
    pub text: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEMPLATE: &str = r#"
name: studio-default
page:
  size: A4
  margin: 20mm
styles:
  - selector:
      element: document
    declarations:
      font-size: 10.5pt
      line-height: 1.6
      align: justify
"#;

    #[test]
    fn the_studio_defaults_to_the_proposal_preset() {
        // ADR-0115: the Studio is the Chinese-first proposal workbench, so its
        // fallback is `proposal-v1`; only the command line keeps `latex-v1`
        // (ADR-0069).
        let parsed =
            parse_document(SourceFormat::Markdown, "中文段落。".as_bytes()).expect("parse");
        let build = build(
            parsed,
            BuildOptions {
                template: TEMPLATE,
                engine: None,
                metrics: None,
                hyphenation: None,
            },
        )
        .expect("build");
        assert_eq!(build.params.preset.0, "proposal-v1");
        assert_eq!(
            build.params.break_params.costs.short_last_threshold,
            Width::from_em(8)
        );
        assert_eq!(
            build.params.break_params.costs.short_last_scale,
            soma_core::Weight(2_000_000)
        );
        assert_eq!(
            build.params.break_params.tolerance,
            soma_core::Permille(1_500)
        );
    }
}
