//! Layout: DocIR + styles → box tree → pages.
//!
//! Responsibilities (architecture v2 §8, ADR-0010):
//!
//! - compile DocIR inlines into a styled inline sequence and hand it to the
//!   rules layer (ADR-0011), then to the frozen inline K-P kernel;
//! - stack block boxes into pages with a deterministic greedy paginator;
//! - move footnotes to the page footnote area by whole blocks;
//! - own all result data: `PagedLayout` has no lifetime and no reference into
//!   `Document`. Renderers resolve text through [`TextSource`].
//!
//! Not implemented in v1 (tracked in `docs/debt.md`): vertical K-P, splitting a
//! paragraph across pages, hyphenation, floating bodies, column balancing,
//! math layout adapters, nested footnote references.

#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]

extern crate alloc;

pub mod canonical;
mod diagram;
pub mod index;
pub mod math;

pub use diagram::DiagramText;
pub mod metrics;
mod pagination;

pub use metrics::{StyledMetrics, TableMetrics};

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

use soma_core::{
    break_paragraph as break_paragraph_core, Align, BoundaryKind, BreakConfig, Cost, Decorations,
    FallbackStage, Line, Metrics, Paragraph, ParagraphStyle, PunctClass, StyleId, SyntaxClass,
    VerticalParams as KernelVerticalParams, Width,
};
use soma_doc::{Block, Document, Inline, NodeId};
use soma_rules::{
    paragraph_from_code_line, paragraph_from_spans, paragraph_from_text, InlineSpan, RuleError,
    SpanKind,
};
use soma_template::{ComputedStyle, ResolvedPage, StyledDoc};

pub use soma_tikz::{
    Path as TikzPath, Point as TikzPoint, Segment as TikzSegment, Stroke as TikzStroke,
};

pub const SCHEMA: &str = "soma.pages/11";

/// Break one paragraph with the layout's space rule applied.
///
/// A space may give up at most a third of its advance. The rule table is
/// font-blind, so a fixed 1/8 em shrink is a different fraction of the space
/// in every face — 43% of a 0.29 em space, 26% of a 0.38 em one. TeX's space
/// glue shrinks by a third of its own width; applying that here keeps a
/// compressed line readable whatever the face is (D86).
pub fn break_paragraph(
    paragraph: &Paragraph,
    metrics: &dyn Metrics,
    config: &BreakConfig,
) -> Result<soma_core::LayoutResult, soma_core::BreakError> {
    let clamped = clamp_space_glue(paragraph, metrics);
    let paragraph = clamped.as_ref().unwrap_or(paragraph);
    break_paragraph_core(paragraph, metrics, config)
}

/// Shrink spaces by at most a third of their advance; `None` when nothing
/// changes, so a paragraph without spaces is not copied.
fn clamp_space_glue(paragraph: &Paragraph, metrics: &dyn Metrics) -> Option<Paragraph> {
    let mut boundaries = paragraph.boundaries.clone();
    let mut changed = false;
    for (index, boundary) in boundaries.iter_mut().enumerate() {
        if boundary.kind != BoundaryKind::Space {
            continue;
        }
        let style = paragraph.atoms[index].style;
        let third = metrics.space_width(style).0 / 3;
        if boundary.glue.shrink.0 > third {
            boundary.glue.shrink = Width(third);
            changed = true;
        }
    }
    if !changed {
        return None;
    }
    let mut clamped = paragraph.clone();
    clamped.boundaries = boundaries;
    Some(clamped)
}

/// Placeholder atom for a footnote marker. Renderers replace the marked atom
/// with [`Mark::number`].
pub const FOOTNOTE_MARKER: char = '\u{e000}';
/// The marker as a string, for metrics adapters that must recognise it.
pub const FOOTNOTE_MARKER_TEXT: &str = "\u{e000}";

/// Thickness of the rule above the footnote area, in layout units.
pub const FOOTNOTE_RULE_THICKNESS: Width = Width::from_em_frac(1, 32);
/// Space between that rule and the first footnote line.
pub const FOOTNOTE_RULE_GAP: Width = Width::from_em_frac(1, 4);
/// Width of the footnote rule, as a fraction of the content width.
pub fn footnote_rule_width(content_width: Width) -> Width {
    Width(content_width.0 / 3)
}
/// Left column reserved for the note number; the note text starts here.
pub fn footnote_number_column(font_size: Width) -> Width {
    Width(font_size.0 * 3 / 2)
}

/// How a page was filled. Only the deterministic greedy path exists in v1.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaginationKind {
    /// Vertical Knuth-Plass: page breaks minimise summed page badness.
    VerticalKp,
    /// Deterministic greedy pass, used for `PaginationStrategy::Greedy` and as
    /// the fallback when the vertical DP finds no feasible break sequence.
    Greedy,
}

impl PaginationKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::VerticalKp => "verticalKp",
            Self::Greedy => "greedy",
        }
    }
}

/// Which paginator to run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaginationStrategy {
    /// Vertical K-P over flow items (ADR-0005 preferred path).
    VerticalKp,
    /// Deterministic greedy fill.
    Greedy,
}

/// Vertical pagination parameters.
///
/// The kernel owns the base cost model; the layout layer adds the body line
/// height used to normalize leftover page space (D92). A page short by one
/// body line then costs `badness_scale` regardless of what kind of block
/// happens to start the page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VerticalParams {
    /// Weight applied for a page whose fill leaves no freedom. `0` keeps the
    /// page count neutral; the DP then optimises badness only.
    pub page_weight: i64,
    /// Scale for the cubic page badness. A page short by one body line costs
    /// `badness_scale` (100 by default).
    pub badness_scale: i64,
    /// Cost of a page that must hold one block taller than the content area.
    pub overflow_penalty: i64,
    /// Cost of breaking a paragraph after its first line (a one-line page foot).
    pub club_penalty: i64,
    /// Cost of breaking a paragraph before its last line (a one-line page head).
    pub widow_penalty: i64,
    /// Body line height used as the badness denominator. `Width::ZERO` means
    /// "resolve from the document style"; a positive value overrides it.
    pub body_height: Width,
}

impl VerticalParams {
    pub const fn default_v1() -> Self {
        Self {
            page_weight: 0,
            badness_scale: 100,
            overflow_penalty: 1_000_000,
            club_penalty: 10_000,
            widow_penalty: 10_000,
            body_height: Width::ZERO,
        }
    }

    /// Hash of the vertical cost model in a fixed field order. The automatic
    /// body height (`Width::ZERO`) is derived from the template, so it does not
    /// enter the parameter hash; an explicit override does.
    pub fn hash(&self) -> u64 {
        let kernel = KernelVerticalParams {
            page_weight: self.page_weight,
            badness_scale: self.badness_scale,
            overflow_penalty: self.overflow_penalty,
            club_penalty: self.club_penalty,
            widow_penalty: self.widow_penalty,
        };
        let mut state = kernel.hash();
        if self.body_height.0 > 0 {
            for byte in self.body_height.0.to_le_bytes() {
                state ^= u64::from(byte);
                state = state.wrapping_mul(0x0000_0100_0000_01b3);
            }
        }
        state
    }
}

impl Default for VerticalParams {
    fn default() -> Self {
        Self::default_v1()
    }
}

impl From<KernelVerticalParams> for VerticalParams {
    fn from(params: KernelVerticalParams) -> Self {
        Self {
            page_weight: params.page_weight,
            badness_scale: params.badness_scale,
            overflow_penalty: params.overflow_penalty,
            club_penalty: params.club_penalty,
            widow_penalty: params.widow_penalty,
            body_height: Width::ZERO,
        }
    }
}

impl From<VerticalParams> for KernelVerticalParams {
    fn from(params: VerticalParams) -> Self {
        Self {
            page_weight: params.page_weight,
            badness_scale: params.badness_scale,
            overflow_penalty: params.overflow_penalty,
            club_penalty: params.club_penalty,
            widow_penalty: params.widow_penalty,
        }
    }
}

/// What one placed box represents.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BoxKind {
    Paragraph,
    Heading,
    Code,
    Math,
    Rule,
    Figure,
    TableCell,
    /// A horizontal line drawn by the table's rule widths.
    TableRule,
    Footnote,
}

impl BoxKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Paragraph => "paragraph",
            Self::Heading => "heading",
            Self::Code => "codeBlock",
            Self::Math => "mathBlock",
            Self::Rule => "thematicBreak",
            Self::Figure => "figure",
            Self::TableCell => "cell",
            Self::TableRule => "tableRule",
            Self::Footnote => "footnote",
        }
    }
}

/// A visible number drawn in place of a marker atom.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mark {
    /// Atom index inside the block's atom sequence.
    pub atom: u32,
    /// Visible number, after the template's numbering policy.
    pub number: u32,
    /// Line index inside the block, so a split paragraph keeps its marker on
    /// the page that holds the line.
    pub line: u32,
    /// Section ordinal of the paragraph that carries this marker.
    /// `section` numbering restarts at each change of this value (D56).
    pub(crate) section: u32,
    /// Definition index of the note this marker refers to. The note area is
    /// numbered from these marks, not from a positional guess, so a repeated
    /// reference cannot shift the printed note numbers.
    pub(crate) note: Option<usize>,
}

/// One line inside a placed block. `y` is relative to the block top.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlacedLine {
    pub y: Width,
    /// Extra width the last atom hangs past the target (hanging punctuation).
    pub hang: i32,
    pub line: Line,
}

/// A block box positioned on a page. `x`/`y` are relative to the page content
/// origin.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlacedBlock {
    pub node: NodeId,
    pub kind: BoxKind,
    /// Style resolved for this block; renderers use it for font metrics.
    pub style: StyleId,
    pub x: Width,
    pub y: Width,
    pub width: Width,
    pub height: Width,
    pub lines: Vec<PlacedLine>,
    pub marks: Vec<Mark>,
    /// Display math content (only for `BoxKind::Math`).
    pub math: Option<MathBox>,
    /// Inline formulas in this block, in atom order.
    pub inline_math: Vec<InlineMathBox>,
    /// Ruby annotations in this block, in atom order (ADR-0060).
    pub ruby: Vec<RubySpan>,
    /// Index of the first line in this page slice. Non-zero means the block is
    /// continued from the previous page.
    pub line_offset: u32,
    /// Total number of lines in the unsplit block.
    pub lines_total: u32,
    pub stage: FallbackStage,
}

/// One ruby annotation over an atom (furigana / 注音).
///
/// The annotation is not part of the atom text: line breaking measures the
/// base only, and the annotation is drawn above it. `shift` and `size` are
/// resolved here, so every renderer places it the same way without needing
/// font metrics of its own.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RubySpan {
    /// Atom the annotation sits over.
    pub atom: u32,
    pub text: String,
    /// Distance from the base line's baseline up to the annotation baseline.
    pub shift: Width,
    /// Annotation em size.
    pub size: Width,
}

/// A laid-out display formula, positioned from the block top.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MathBox {
    pub width: Width,
    /// Total height (`baseline` above the top, `depth` below).
    pub height: Width,
    /// Distance from the block top to the formula baseline. Item and rule `y`
    /// values are relative to this baseline, matching `soma_math`.
    pub baseline: Width,
    pub items: Vec<soma_math::MathItem>,
    pub rules: Vec<soma_math::MathRule>,
    /// Stroked/filled vector paths (TikZ, ADR-0092). Empty for formulas.
    pub paths: Vec<TikzPath>,
}

/// One inline formula inside a block's atom stream.
///
/// `atom` indexes the block's text stream (the same index used by
/// `PlacedLine.line.boxes`). The atom's text is a private-use marker, so
/// renderers must draw `math` instead of the atom text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InlineMathBox {
    pub atom: u32,
    /// Formula source, kept for diagnostics and text extraction.
    pub source: String,
    /// Geometry; `items`/`rules` are relative to the formula baseline, which
    /// sits on the line's baseline.
    pub math: MathBox,
}

/// One page.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Page {
    /// Physical page index, 1-based. Always counts every page.
    pub number: u32,
    /// Number printed on the page (arabic or roman), or empty when the page
    /// has no folio.
    pub printed: String,
    pub blocks: Vec<PlacedBlock>,
    pub footnotes: Vec<PlacedBlock>,
    /// Note number for each block in `footnotes`, aligned by index. Only the
    /// first block of a note carries `Some`; continuation blocks are `None`.
    /// Derived from the marks on the referring lines, so it is not part of the
    /// canonical page result.
    pub footnote_numbers: Vec<Option<u32>>,
    /// Definition index for each block in `footnotes`, aligned by index.
    /// Used to assign `footnote_numbers`; not canonical.
    pub(crate) footnote_notes: Vec<usize>,
    /// Running header, positioned above the content box (negative `y`).
    pub header: Vec<PlacedBlock>,
    /// Footer and folio, positioned below the content box.
    pub footer: Vec<PlacedBlock>,
    pub used_height: Width,
    pub footnote_height: Width,
    /// True when a single box was taller than the content area.
    pub overflow: bool,
}

/// One table-of-contents row: heading level, text, and resolved page number.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TocEntry {
    pub level: u8,
    pub text: String,
    /// The label printed on the page: `"3"`, `"ii"`, or `""` when the page has
    /// no folio.
    pub page: String,
}

/// Generated lists gathered by one pass: headings, figures, tables, index.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Generated {
    headings: Vec<TocEntry>,
    figures: Vec<TocEntry>,
    tables: Vec<TocEntry>,
    index: Vec<TocEntry>,
}

/// One bookmark: a heading and the page it opens.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OutlineEntry {
    /// Heading level, `1..=5`.
    pub level: u8,
    pub text: String,
    /// Physical page index, 1-based.
    pub page: u32,
    /// Printed label (`"3"`, `"ii"`, or empty).
    pub printed: String,
}

/// What the index did during layout.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IndexReport {
    pub passes: u8,
    pub stable: bool,
    pub entries: u32,
}

/// One caption waiting for its page number.
#[derive(Clone, Debug, PartialEq, Eq)]
struct CaptionRef {
    item: usize,
    text: String,
}

/// What the table of contents did during layout.
///
/// `passes` is how many full layouts ran (1 = the entries were already stable),
/// and `stable` is `false` when the pass cap was reached before the entries
/// stopped moving — in that case the printed page numbers come from the
/// previous pass.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TocReport {
    pub passes: u8,
    pub stable: bool,
    pub entries: u32,
    /// Figures listed after the chapters (`0` when the template lists none).
    pub figures: u32,
    /// Tables listed after the chapters.
    pub tables: u32,
}

/// The complete, owned layout result.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PagedLayout {
    pub page: ResolvedPage,
    pub pagination: PaginationKind,
    pub pages: Vec<Page>,
    pub text: TextIndex,
    pub engine_hash: u64,
    pub rules_version: u16,
    pub metrics_version: u16,
    /// Content hash of the metrics source. `0` for adapters without one.
    pub metrics_hash: u64,
    /// Hash of the vertical cost model used for this pagination.
    pub vertical_hash: u64,
    /// Footnote reflow passes actually used (1 = no iteration).
    pub footnote_passes: u32,
    /// Summed vertical cost of the chosen pagination. Comparable only between
    /// runs with the same `VerticalParams`.
    pub pagination_cost: i64,
    /// Present when the template asks for a table of contents.
    pub toc: Option<TocReport>,
    /// Present when the template asks for an index.
    pub index: Option<IndexReport>,
    /// One entry per heading, in document order. A PDF backend can turn this
    /// into the bookmark tree without re-reading the document.
    pub outline: Vec<OutlineEntry>,
}

impl PagedLayout {
    pub fn page_count(&self) -> u32 {
        self.pages.len() as u32
    }

    pub fn canonical_json(&self) -> String {
        canonical::json(self)
    }

    pub fn hash(&self) -> u64 {
        canonical::hash(self)
    }
}

/// Resolve the text of one atom. The renderer must not see DocIR.
pub trait TextSource {
    fn atom_text(&self, block: NodeId, atom: u32) -> &str;

    /// Style of one atom, when the source knows it. Renderers fall back to the
    /// block style when this returns .
    fn atom_style(&self, _block: NodeId, _atom: u32) -> Option<StyleId> {
        None
    }

    /// Underline and friends, as the source wrote them (ADR-0044).
    fn atom_decorations(&self, _block: NodeId, _atom: u32) -> Decorations {
        Decorations::NONE
    }

    /// Link target of one atom, when it came from one.
    fn atom_link(&self, _block: NodeId, _atom: u32) -> Option<&str> {
        None
    }

    /// Highlighted class of one code atom (ADR-0091). `None` uses the atom's
    /// own style color.
    fn atom_syntax(&self, _block: NodeId, _atom: u32) -> Option<SyntaxClass> {
        None
    }

    /// True when the atom is drawn by a rescue face rather than its style's
    /// own. Layout measured it with the same rule, so the renderer and the
    /// breaker agree (docs/mixed-font.md).
    fn uses_fallback(&self, _block: NodeId, _atom: u32) -> bool {
        false
    }

    /// Image source of a figure block, exactly as the document wrote it.
    ///
    /// The renderer does not open files; it hands this string to the caller's
    /// [`ImageSource`] so the same layout can be rendered from another asset
    /// root (ADR-0047).
    fn block_image(&self, _block: NodeId) -> Option<&str> {
        None
    }

    /// Alt text of a figure, for the tagged-PDF `/Alt` entry.
    fn block_alt(&self, _block: NodeId) -> Option<&str> {
        None
    }

    /// Heading level of a heading block; `None` for every other block. The
    /// tagged PDF maps it to `/H1`…`/H6`.
    fn block_heading_level(&self, _block: NodeId) -> Option<u8> {
        None
    }

    /// Where a table cell sits in its grid. `None` for every block that is not
    /// part of a table. The tagged PDF needs it to build `/Table`, `/TR` and
    /// `/TD` instead of a flat list of paragraphs (ADR-0050).
    fn block_cell(&self, _block: NodeId) -> Option<TableCell> {
        None
    }
}

/// A cell's place in the table grid, for the structure tree.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TableCell {
    pub row: u16,
    pub column: u16,
    pub row_span: u16,
    pub column_span: u16,
}

/// Image bytes for a figure, supplied by the caller.
///
/// A document says `fig/pipeline.png`; where that lives is the caller's
/// business. Keeping this seam here, next to [`TextSource`], is what lets every
/// renderer share it without any of them touching the file system.
pub trait ImageSource {
    fn image(&self, src: &str) -> Option<&[u8]>;
}

/// An [`ImageSource`] with no assets: figures keep their placeholder.
pub struct NoImages;

impl ImageSource for NoImages {
    fn image(&self, _src: &str) -> Option<&[u8]> {
        None
    }
}

/// A `data:` URI for an image, or `None` when the bytes are not a format the
/// renderers can declare.
///
/// This is how the HTML and SVG renderers show a figure without ever naming an
/// external resource: the image travels inside the document. PNG and JPEG are
/// the formats `soma-render-pdf` also embeds.
pub fn data_uri(bytes: &[u8]) -> Option<String> {
    let mime = if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        "image/png"
    } else if bytes.starts_with(&[0xff, 0xd8]) {
        "image/jpeg"
    } else {
        return None;
    };
    let mut out = String::with_capacity(bytes.len() * 4 / 3 + 24);
    out.push_str("data:");
    out.push_str(mime);
    out.push_str(";base64,");
    base64(bytes, &mut out);
    Some(out)
}

const BASE64: [u8; 64] = *b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn base64(data: &[u8], out: &mut String) {
    for chunk in data.chunks(3) {
        let first = chunk[0];
        let second = chunk.get(1).copied();
        let third = chunk.get(2).copied();
        out.push(BASE64[(first >> 2) as usize] as char);
        out.push(BASE64[(((first & 0x03) << 4) | (second.unwrap_or(0) >> 4)) as usize] as char);
        match second {
            Some(second) => out.push(
                BASE64[(((second & 0x0f) << 2) | (third.unwrap_or(0) >> 6)) as usize] as char,
            ),
            None => out.push('='),
        }
        match third {
            Some(third) => out.push(BASE64[(third & 0x3f) as usize] as char),
            None => out.push('='),
        }
    }
}

/// Owned atom text per DocIR block. Built while laying out, so the renderer
/// does not recompile anything.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TextIndex {
    entries: Vec<(NodeId, Vec<TextEntry>)>,
    /// Atoms drawn by a rescue face, `(node, atom)`, sorted at `finish`.
    fallbacks: Vec<(u32, u32)>,
    /// Figures: `(node, source, alt text)`, sorted at `finish`.
    figures: Vec<(NodeId, String, Option<String>)>,
    /// Heading levels, `(node, level)`, sorted at `finish`.
    headings: Vec<(NodeId, u8)>,
    /// Table cell placement, `(node, cell)`, sorted at `finish`.
    cells: Vec<(NodeId, TableCell)>,
    /// Highest node id inserted so far. Tracked explicitly: `entries` is only
    /// sorted at `finish`, so the last element is not the maximum while blocks
    /// are still being added (the TOC adds ids above the document's).
    max: u32,
}

impl TextIndex {
    pub fn blocks(&self) -> &[(NodeId, Vec<TextEntry>)] {
        &self.entries
    }

    fn insert(&mut self, node: NodeId, atoms: Vec<TextEntry>) {
        self.max = self.max.max(node.0);
        self.entries.push((node, atoms));
    }

    fn mark_fallback(&mut self, node: NodeId, atom: u32) {
        self.fallbacks.push((node.0, atom));
    }

    fn mark_figure(&mut self, node: NodeId, src: &str, alt: Option<&str>) {
        self.figures
            .push((node, String::from(src), alt.map(String::from)));
    }

    fn mark_heading(&mut self, node: NodeId, level: u8) {
        self.headings.push((node, level));
    }

    fn mark_cell(&mut self, node: NodeId, cell: TableCell) {
        self.cells.push((node, cell));
    }

    fn finish(&mut self) {
        self.entries.sort_by_key(|(node, _)| *node);
        self.fallbacks.sort_unstable();
        self.fallbacks.dedup();
        self.figures.sort_by_key(|(node, _, _)| *node);
        self.headings.sort_by_key(|(node, _)| *node);
        self.cells.sort_by_key(|(node, _)| *node);
    }

    fn max_node(&self) -> u32 {
        self.max
    }

    /// Atoms drawn by a rescue face (diagnostics).
    pub fn fallback_count(&self) -> usize {
        self.fallbacks.len()
    }
}

impl TextSource for TextIndex {
    fn atom_text(&self, block: NodeId, atom: u32) -> &str {
        match self.entries.binary_search_by_key(&block, |(node, _)| *node) {
            Ok(index) => self.entries[index]
                .1
                .get(atom as usize)
                .map(|entry| entry.text.as_str())
                .unwrap_or(""),
            Err(_) => "",
        }
    }

    fn atom_style(&self, block: NodeId, atom: u32) -> Option<StyleId> {
        self.entry(block, atom).map(|entry| entry.style)
    }

    fn atom_decorations(&self, block: NodeId, atom: u32) -> Decorations {
        self.entry(block, atom)
            .map(|entry| entry.decorations)
            .unwrap_or(Decorations::NONE)
    }

    fn atom_link(&self, block: NodeId, atom: u32) -> Option<&str> {
        self.entry(block, atom)
            .and_then(|entry| entry.link.as_deref())
    }

    fn atom_syntax(&self, block: NodeId, atom: u32) -> Option<SyntaxClass> {
        self.entry(block, atom).and_then(|entry| entry.syntax)
    }

    fn uses_fallback(&self, block: NodeId, atom: u32) -> bool {
        self.fallbacks.binary_search(&(block.0, atom)).is_ok()
    }

    fn block_image(&self, block: NodeId) -> Option<&str> {
        self.figures
            .binary_search_by_key(&block, |(node, _, _)| *node)
            .ok()
            .map(|index| self.figures[index].1.as_str())
    }

    fn block_alt(&self, block: NodeId) -> Option<&str> {
        self.figures
            .binary_search_by_key(&block, |(node, _, _)| *node)
            .ok()
            .and_then(|index| self.figures[index].2.as_deref())
    }

    fn block_heading_level(&self, block: NodeId) -> Option<u8> {
        self.headings
            .binary_search_by_key(&block, |(node, _)| *node)
            .ok()
            .map(|index| self.headings[index].1)
    }

    fn block_cell(&self, block: NodeId) -> Option<TableCell> {
        self.cells
            .binary_search_by_key(&block, |(node, _)| *node)
            .ok()
            .map(|index| self.cells[index].1)
    }
}

impl TextIndex {
    fn entry(&self, block: NodeId, atom: u32) -> Option<&TextEntry> {
        match self.entries.binary_search_by_key(&block, |(node, _)| *node) {
            Ok(index) => self.entries[index].1.get(atom as usize),
            Err(_) => None,
        }
    }
}

/// One atom as the renderers see it: text, style, and what the source asked
/// for on top of the style.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextEntry {
    pub text: String,
    pub style: StyleId,
    pub decorations: Decorations,
    pub link: Option<alloc::sync::Arc<str>>,
    /// Token class from the code lexer; `None` for ordinary text.
    pub syntax: Option<SyntaxClass>,
}

/// One layout failure.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LayoutError {
    pub node: Option<NodeId>,
    pub reason: &'static str,
}

impl LayoutError {
    fn new(node: Option<NodeId>, reason: &'static str) -> Self {
        Self { node, reason }
    }
}

impl core::fmt::Display for LayoutError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self.node {
            Some(node) => write!(f, "node {}: {}", node.0, self.reason),
            None => f.write_str(self.reason),
        }
    }
}

/// Placeholder style used before the first resolved style is known.
const ROOT_STYLE: StyleId = StyleId(0);

/// Body line height for page badness (D92).
///
/// An explicit positive `body_height` wins. Otherwise the document style's
/// resolved template line height is used; if the style or its line height is
/// unavailable, the font's line metrics are the last fallback. `None` tells
/// pagination to keep the old first-item height, so an exotic host without
/// either source still paginates as before.
fn resolved_body_height(
    vertical: VerticalParams,
    styled: &StyledDoc,
    metrics: &dyn Metrics,
) -> Option<Width> {
    if vertical.body_height.0 > 0 {
        return Some(vertical.body_height);
    }
    if let Some(style) = styled.style(ROOT_STYLE) {
        if style.line_height.0 > 0 {
            return Some(style.line_height);
        }
    }
    let height = metrics.line_metrics(ROOT_STYLE).height();
    (height.0 > 0).then_some(height)
}

/// Lay out a document into pages with the default strategy (vertical K-P).
pub fn layout(
    doc: &Document,
    styled: &StyledDoc,
    metrics: &dyn Metrics,
    engine: &BreakConfig,
) -> Result<PagedLayout, LayoutError> {
    layout_with(
        doc,
        styled,
        metrics,
        engine,
        PaginationStrategy::VerticalKp,
        VerticalParams::default_v1(),
    )
}

/// One layout pass. The TOC driver repeats it with the entries found by the
/// previous pass; everything else passes the same values.
struct Pass<'a> {
    doc: &'a Document,
    styled: &'a StyledDoc,
    metrics: &'a dyn Metrics,
    engine: &'a BreakConfig,
    strategy: PaginationStrategy,
    vertical: VerticalParams,
    /// Entries printed by this pass (from the previous pass's pagination).
    front: &'a Generated,
    /// Template TOC settings; `None` disables TOC generation.
    toc: Option<&'a soma_template::TocSpec>,
    /// Index entries from the previous pass (term + page labels).
    index: &'a [TocEntry],
    /// Optional hyphenation patterns; `None` keeps every Latin run unbreakable.
    hyphen: Option<&'a soma_hyphen::Patterns>,
}

/// Lay out with an explicit strategy and vertical cost model.
///
/// `vertical` accepts either the layout's [`VerticalParams`] (which adds
/// `body_height` for D92) or the kernel's parameter struct, which gets the
/// automatic body height.
///
/// When the template asks for a table of contents, this runs the layout again
/// until the entry page numbers stop moving (at most [`TOC_MAX_PASSES`]).
/// Everything else is a single pass.
pub fn layout_with(
    doc: &Document,
    styled: &StyledDoc,
    metrics: &dyn Metrics,
    engine: &BreakConfig,
    strategy: PaginationStrategy,
    vertical: impl Into<VerticalParams>,
) -> Result<PagedLayout, LayoutError> {
    layout_with_hyphenation(
        doc,
        styled,
        metrics,
        engine,
        strategy,
        vertical.into(),
        None,
    )
}

/// Lay out with an explicit strategy, vertical cost model, and hyphenation
/// pattern set. Patterns are host data: an engine without a dictionary behaves
/// exactly like [`layout_with`].
pub fn layout_with_hyphenation(
    doc: &Document,
    styled: &StyledDoc,
    metrics: &dyn Metrics,
    engine: &BreakConfig,
    strategy: PaginationStrategy,
    vertical: impl Into<VerticalParams>,
    hyphen: Option<&soma_hyphen::Patterns>,
) -> Result<PagedLayout, LayoutError> {
    let vertical = vertical.into();
    let toc = styled.toc();
    let index = styled.index();
    if toc.is_none() && index.is_none() {
        let (pages, _) = layout_once(&Pass {
            doc,
            styled,
            metrics,
            engine,
            strategy,
            vertical,
            front: &Generated::default(),
            toc: None,
            index: &[],
            hyphen,
        })?;
        return Ok(pages);
    }

    let mut front = Generated::default();
    let mut passes: u8 = 0;
    loop {
        passes += 1;
        let (mut pages, found) = layout_once(&Pass {
            doc,
            styled,
            metrics,
            engine,
            strategy,
            vertical,
            front: &front,
            toc,
            index: &front.index,
            hyphen,
        })?;
        let stable = found == front;
        if stable || passes >= TOC_MAX_PASSES {
            if toc.is_some() {
                pages.toc = Some(TocReport {
                    passes,
                    stable,
                    entries: front.headings.len() as u32,
                    figures: front.figures.len() as u32,
                    tables: front.tables.len() as u32,
                });
            }
            if index.is_some() {
                pages.index = Some(IndexReport {
                    passes,
                    stable,
                    entries: front.index.len() as u32,
                });
            }
            return Ok(pages);
        }
        front = found;
    }
}

/// Bound on TOC fixed-point iterations. Each pass is a full layout, so a stuck
/// document is reported as unstable instead of looping.
const TOC_MAX_PASSES: u8 = 4;

fn layout_once(pass: &Pass<'_>) -> Result<(PagedLayout, Generated), LayoutError> {
    let Pass {
        doc,
        styled,
        metrics,
        engine,
        strategy,
        vertical,
        front,
        toc,
        index,
        hyphen,
    } = *pass;
    let page = styled.page();
    let content_width = page.content_width();
    if content_width.0 <= 0 || page.content_height().0 <= 0 {
        return Err(LayoutError::new(None, "page content box must be positive"));
    }

    let generated_base = doc.node_ids().iter().map(|id| id.0).max().unwrap_or(0) + 1;
    let mut builder = Builder {
        styled,
        metrics,
        engine,
        hyphen,
        content_width,
        page_height: page.content_height(),
        text: TextIndex::default(),
        notes: NoteMap::build(doc),
        headings: Vec::new(),
        tables: Vec::new(),
        current_master: 0,
        generated: generated_base,
        section_start_pending: false,
        current_section: 0,
        toc_depth: toc.map(|spec| spec.depth).unwrap_or(0),
        toc_entries: front.headings.clone(),
        numbering: styled.numbering().clone(),
        heading_counts: [0; 3],
        figure_count: 0,
        equation_count: 0,
        extra_left: Width::ZERO,
        extra_right: Width::ZERO,
        table_count: 0,
        front_matter_items: 0,
        pending_index: Vec::new(),
        index_terms: Vec::new(),
        note_index_terms: note_index_terms(doc),
        pending_ruby: Vec::new(),
        suppress_hang: false,
        defer_index: false,
        index_entries: index.to_vec(),
        last_line_short: false,
        figure_refs: Vec::new(),
        table_refs: Vec::new(),
    };
    let mut items = Vec::new();
    if styled.cover().is_some() {
        builder.cover_rows(doc, &mut items)?;
    }
    if let Some(spec) = toc {
        builder.toc_rows(spec, front, &mut items)?;
    }
    builder.front_matter_items = items.len();
    builder.flow_into(&doc.blocks, ROOT_STYLE, &mut items, true)?;
    builder.compile_notes(doc)?;
    if let Some(spec) = styled.index() {
        builder.index_rows(spec, &mut items)?;
    }

    let page_height = page.content_height();
    let body_height = resolved_body_height(vertical, styled, metrics);
    // Pagination sees the resolved body height; the public parameters keep the
    // caller's value so the automatic height stays out of `verticalHash`.
    let pagination_vertical = VerticalParams {
        body_height: body_height.unwrap_or(Width::ZERO),
        ..vertical
    };
    let (vertical_items, segments) = pagination::build_stream(&items, pagination_vertical);
    let mut passes = 1u32;

    let (ends, kind) = match strategy {
        PaginationStrategy::VerticalKp => match pagination::place_kp(
            &vertical_items,
            &segments,
            page_height,
            &builder.notes,
            pagination_vertical,
        ) {
            Some(ends) => (ends, PaginationKind::VerticalKp),
            None => (
                pagination::greedy_with_notes(
                    &vertical_items,
                    &segments,
                    page_height,
                    &builder.notes,
                    &mut passes,
                ),
                PaginationKind::Greedy,
            ),
        },
        PaginationStrategy::Greedy => (
            pagination::greedy_with_notes(
                &vertical_items,
                &segments,
                page_height,
                &builder.notes,
                &mut passes,
            ),
            PaginationKind::Greedy,
        ),
    };

    let pagination_cost = pagination::score_pages(
        &ends,
        &vertical_items,
        &builder.notes,
        page_height,
        pagination_vertical,
    );

    // Front matter is the leading run of pages that hold only generated items
    // (cover and contents). Printed numbers depend on it, and the folio is
    // built inside the page loop.
    let front_pages = front_page_count(&ends, &vertical_items, builder.front_matter_items);
    let printed = page_labels(ends.len(), front_pages, styled.front_matter());
    let body_pages = ends.len().saturating_sub(front_pages);
    let folio_total = match styled.front_matter() {
        soma_template::FrontMatterNumbering::Continuous => ends.len(),
        _ => body_pages,
    };

    let mut pages = Vec::with_capacity(ends.len().max(1));
    let mut previous = 0usize;
    let mut next_node = builder.text.max_node() + 1;
    for (index, &end) in ends.iter().enumerate() {
        let footnote_height =
            pagination::page_note_height(previous, end, &vertical_items, &builder.notes);
        let (blocks, used) = pagination::assemble_page_blocks(
            &items,
            &vertical_items,
            previous,
            end,
            page_height,
            footnote_height,
            &builder.tables,
        );
        let overflow = vertical_items[previous..end]
            .iter()
            .any(|item| item.height.0 > page_height.0);
        let (footnotes, footnote_notes) =
            pagination::assemble_footnotes(previous, end, &vertical_items, &builder.notes);

        // A page that opens a section may use the section-start master: book
        // design usually drops the running head on a chapter opening.
        let first_item = vertical_items
            .get(previous)
            .map(|item| item.item)
            .unwrap_or(0);
        let section_start = items
            .get(first_item)
            .map(|item| item.section_start)
            .unwrap_or(false);
        let base_master = vertical_items
            .get(previous)
            .map(|item| item.master)
            .unwrap_or(0);
        let master_index = match styled.start_master() {
            Some(index) if section_start => index,
            _ => base_master,
        };
        let master = styled.master(master_index);
        let mut header = Vec::new();
        let mut footer = Vec::new();
        if master.header_height.0 > 0 {
            let source = master.header_text.as_deref().unwrap_or("running-title");
            let text = if source == "running-title" {
                // A running head names the chapter that governs the page. In
                // book mode a chapter opens on its own page, so the page-start
                // and page-end chapters agree. A sample sheet may open a
                // chapter mid-page; the page then carries the new chapter
                // rather than the one it is leaving.
                let last_item = vertical_items
                    .get(previous..end)
                    .and_then(|slice| slice.iter().map(|item| item.item).max())
                    .unwrap_or(0);
                builder
                    .headings
                    .iter()
                    .rev()
                    .find(|heading| heading.level == 1 && heading.item <= last_item)
                    .map(|heading| heading.text.clone())
                    .unwrap_or_default()
            } else {
                String::from(source)
            };
            if let Some(block) = margin_block(
                &mut builder,
                NodeId(next_node),
                &text,
                styled.running_head_style(),
                content_width,
                master.header_height,
                Width(-master.header_height.0),
                false,
            ) {
                header.push(block);
                next_node += 1;
            }
        }
        if master.footer_height.0 > 0 {
            if let Some(format) = master.folio.as_deref() {
                let label = printed.get(index).map(String::as_str).unwrap_or("");
                let text = format
                    .replace("{page}", label)
                    .replace("{pages}", &folio_total.to_string());
                if !label.is_empty() {
                    if let Some(block) = margin_block(
                        &mut builder,
                        NodeId(next_node),
                        &text,
                        styled.folio_style(),
                        content_width,
                        master.footer_height,
                        page_height,
                        true,
                    ) {
                        footer.push(block);
                        next_node += 1;
                    }
                }
            }
        }

        let footnote_numbers = vec![None; footnotes.len()];
        pages.push(Page {
            number: index as u32 + 1,
            printed: printed.get(index).cloned().unwrap_or_default(),
            blocks,
            footnotes,
            footnote_numbers,
            footnote_notes,
            header,
            footer,
            used_height: used,
            footnote_height,
            overflow,
        });
        previous = end;
    }
    if pages.is_empty() {
        pages.push(Page {
            number: 1,
            printed: printed.first().cloned().unwrap_or_default(),
            blocks: Vec::new(),
            footnotes: Vec::new(),
            footnote_numbers: Vec::new(),
            footnote_notes: Vec::new(),
            header: Vec::new(),
            footer: Vec::new(),
            used_height: Width::ZERO,
            footnote_height: Width::ZERO,
            overflow: false,
        });
    }
    builder.text.finish();

    let mut numbering = Numbering::new(styled.footnote_numbering());
    numbering.assign(&mut pages);

    // Resolve the heading page numbers for the next pass.
    let mut page_of_item = vec![0u32; items.len()];
    let mut start = 0usize;
    for (index, &end) in ends.iter().enumerate() {
        for item in &vertical_items[start..end] {
            if let Some(slot) = page_of_item.get_mut(item.item) {
                *slot = index as u32 + 1;
            }
        }
        start = end;
    }
    let label_of = |item: usize| -> String {
        let page = page_of_item.get(item).copied().unwrap_or(1).max(1) as usize;
        printed
            .get(page - 1)
            .cloned()
            .unwrap_or_else(|| page.to_string())
    };
    let toc_found: Vec<TocEntry> = if builder.toc_depth == 0 {
        Vec::new()
    } else {
        builder
            .headings
            .iter()
            .filter(|heading| heading.level <= builder.toc_depth)
            .map(|heading| TocEntry {
                level: heading.level,
                text: heading.text.clone(),
                page: label_of(heading.item),
            })
            .collect()
    };
    let captions = |refs: &[CaptionRef]| -> Vec<TocEntry> {
        refs.iter()
            .map(|reference| TocEntry {
                level: 0,
                text: reference.text.clone(),
                page: label_of(reference.item),
            })
            .collect()
    };
    let found = Generated {
        headings: toc_found,
        figures: captions(&builder.figure_refs),
        tables: captions(&builder.table_refs),
        index: builder.index_entries_found(&label_of),
    };
    let outline: Vec<OutlineEntry> = builder
        .headings
        .iter()
        .map(|heading| {
            let page = page_of_item.get(heading.item).copied().unwrap_or(1).max(1);
            OutlineEntry {
                level: heading.level,
                text: heading.text.clone(),
                page,
                printed: printed.get(page as usize - 1).cloned().unwrap_or_default(),
            }
        })
        .collect();

    Ok((
        PagedLayout {
            page,
            pagination: kind,
            pages,
            text: builder.text,
            engine_hash: engine.param_hash().0,
            rules_version: engine.rules_version(),
            metrics_version: engine.metrics_version(),
            metrics_hash: metrics.hash(),
            vertical_hash: vertical.hash(),
            footnote_passes: passes,
            pagination_cost,
            toc: None,
            index: None,
            outline,
        },
        found,
    ))
}

// ---------------------------------------------------------------------------
// Compilation
// ---------------------------------------------------------------------------

struct OwnedSpan {
    text: String,
    style: StyleId,
    kind: SpanKind,
    decor: Decorations,
    link: Option<String>,
    width: Width,
    penalty: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Glue {
    width: Width,
    plus: Width,
    minus: Width,
}

impl Glue {
    const ZERO: Self = Self {
        width: Width::ZERO,
        plus: Width::ZERO,
        minus: Width::ZERO,
    };

    /// Combine a block margin with the glue already carried by its first or
    /// last child. Margins do not collapse in this model, but a list's margin
    /// and its item's own spacing should not add up either: the larger one
    /// wins, so a list reads as one block.
    fn outer(self, margin: Self) -> Self {
        Self {
            width: Width(self.width.0.max(margin.width.0)),
            plus: Width(self.plus.0.max(margin.plus.0)),
            minus: Width(self.minus.0.max(margin.minus.0)),
        }
    }
}

/// One line of a splittable block, or the single part of an atomic block.
struct Part {
    height: Width,
    /// Footnotes whose marker sits on this line.
    notes: Vec<usize>,
    /// For table bands: the row index this part starts at. Zero elsewhere.
    row: u32,
}

/// Table metadata carried by the flow item of a row group.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct TableContext {
    id: u16,
    /// First row index of this group.
    row: u32,
    /// Number of leading header rows.
    header_rows: u32,
    header_height: Width,
}

/// A repeated header block set, stored once per table.
struct TableHeader {
    blocks: Vec<PlacedBlock>,
    height: Width,
}

/// One laid-out table cell before row heights are resolved.
struct TableCellLayout {
    row: usize,
    column: usize,
    colspan: usize,
    rowspan: usize,
    blocks: Vec<PlacedBlock>,
    /// Content height.
    height: Width,
}

/// Booktabs-style space above and below a horizontal rule. The rule itself
/// keeps its own thickness; this gap is what separates text from a rule
/// without turning every body row into a tall band. Horizontal padding remains
/// a template concern (`indent-left` under `within: cell`, ADR-0019).
pub const TABLE_RULE_SEP: Width = Width::from_em_frac(1, 4);

/// Horizontal padding inside a natural-width table cell. It is the analogue of
/// LaTeX's `\tabcolsep`; a justified table already gets its column gaps from
/// the measure, so this is only added in natural mode.
const TABLE_CELL_PAD_X: i32 = Width::from_em_frac(1, 2).0;

/// Segments of a rule: one full span, or dash/gap runs.
fn rule_segments(total: i32, thickness: i32, dashed: bool) -> Vec<(i32, i32)> {
    let mut out = Vec::new();
    if total <= 0 || thickness <= 0 {
        return out;
    }
    if !dashed {
        out.push((0, total));
        return out;
    }
    let dash = (thickness * 3).max(1);
    let gap = (thickness * 2).max(1);
    let mut offset = 0;
    while offset < total {
        out.push((offset, dash.min(total - offset)));
        offset += dash + gap;
    }
    out
}

/// What every rule segment shares.
#[derive(Clone, Copy)]
struct RulePen {
    node: NodeId,
    style: StyleId,
    dashed: bool,
}

fn push_horizontal_rule(
    out: &mut Vec<PlacedBlock>,
    pen: RulePen,
    x: Width,
    y: Width,
    width: Width,
    thickness: Width,
) {
    for (offset, length) in rule_segments(width.0, thickness.0, pen.dashed) {
        out.push(table_rule(
            pen.node,
            pen.style,
            Width(x.0 + offset),
            y,
            Width(length),
            thickness,
        ));
    }
}

fn push_vertical_rule(
    out: &mut Vec<PlacedBlock>,
    pen: RulePen,
    x: Width,
    y: Width,
    height: Width,
    thickness: Width,
) {
    for (offset, length) in rule_segments(height.0, thickness.0, pen.dashed) {
        out.push(table_rule(
            pen.node,
            pen.style,
            x,
            Width(y.0 + offset),
            thickness,
            Width(length),
        ));
    }
}

/// A horizontal table rule at an exact position.
/// Column widths proportional to what each column holds.
///
/// Every column keeps a floor (`measure / 6n`) and the rest of the measure is
/// shared by natural width, so a table with one long column gets one wide
/// column instead of equal slices. Largest-remainder rounding keeps the widths
/// summing to exactly `measure`, which the rules and the span offsets rely on.
/// Widths for a table that keeps its content width.
///
/// Without shares, the columns take their measured widths and the table is
/// centred in the measure. With shares (Markdown dash counts, `<colgroup>`, a
/// Word grid), the natural total is distributed by those weights; more dashes
/// means a wider column. A table wider than the measure falls back to
/// `column_widths`, which fills and justifies the measure.
fn natural_table_widths(natural: &[i32], shares: &[u16], measure: i32) -> (Vec<i32>, i32, i32) {
    let count = natural.len();
    if count == 0 {
        return (Vec::new(), 0, 0);
    }
    // A measured line can be a unit or two short once the cell is broken; a
    // small bearing keeps a natural table from wrapping a word it just fit.
    // The natural width already includes the two cell pads from the caller.
    let slack = Width::from_em_frac(1, 8).0;
    let mut widths: Vec<i32> = natural.iter().map(|width| *width + slack).collect();
    let total: i64 = widths.iter().map(|width| i64::from(*width)).sum();
    if total >= i64::from(measure) {
        return (column_widths(natural, shares, measure), measure, 0);
    }
    let hint_total: u32 = shares.iter().map(|share| u32::from(*share)).sum();
    if hint_total > 0 {
        // Dashes can widen a column, never narrow it below its measured
        // content: a narrower column would overwrite its padding and produce
        // a vertical one-character strip. The table grows to accommodate the
        // weighted widths and is centred if it still fits.
        let target = total;
        for (index, share) in shares.iter().enumerate() {
            let weighted = target * i64::from(*share) / i64::from(hint_total);
            widths[index] = widths[index].max(weighted as i32);
        }
    }
    let actual: i32 = widths.iter().sum();
    if actual >= measure {
        return (column_widths(natural, shares, measure), measure, 0);
    }
    (widths, actual, (measure - actual).max(0) / 2)
}

fn column_widths(natural: &[i32], shares: &[u16], measure: i32) -> Vec<i32> {
    let count = natural.len();
    if count == 0 {
        return Vec::new();
    }
    let measure = measure.max(count as i32);
    let floor = (measure / (count as i32 * 6)).max(1);
    let hint_total: u32 = shares.iter().map(|share| u32::from(*share)).sum();

    // Without hints the widths are the measured widths, exactly as before.
    if hint_total == 0 {
        return proportional_widths(natural, measure, floor);
    }

    // A hint is a per-mille share of the whole measure, so `25%` is a quarter
    // of the table whatever the other columns hold. The hint is scaled down
    // only when the hinted columns together ask for more than the measure
    // (or more than 90% of it, when unhinted columns still need room).
    let unhinted: Vec<usize> = shares
        .iter()
        .enumerate()
        .filter(|(_, share)| **share == 0)
        .map(|(index, _)| index)
        .collect();
    let limit: u32 = if unhinted.is_empty() { 1000 } else { 900 };
    let hints: Vec<i64> = shares
        .iter()
        .map(|share| {
            let share = u32::from(*share);
            if share == 0 {
                0
            } else if hint_total > limit {
                i64::from((share * limit / hint_total).max(1))
            } else {
                i64::from(share)
            }
        })
        .collect();

    let mut widths = vec![0i32; count];
    let mut used = 0i64;
    for (index, hint) in hints.iter().enumerate() {
        if *hint <= 0 {
            continue;
        }
        let width = (i64::from(measure) * *hint / 1000).max(i64::from(floor));
        widths[index] = width as i32;
        used += width;
    }

    let remaining = (i64::from(measure) - used).max(0);
    if remaining > 0 {
        // Unhinted columns take what is left by natural width; a table whose
        // columns are all hinted shares the rounding leftover by the hints.
        let all_hinted = unhinted.is_empty();
        let flexible: Vec<usize> = if all_hinted {
            (0..count).collect()
        } else {
            unhinted
        };
        let weights: Vec<i64> = if all_hinted {
            flexible.iter().map(|index| hints[*index].max(1)).collect()
        } else {
            flexible
                .iter()
                .map(|index| i64::from(natural[*index].max(1)))
                .collect()
        };
        let total: i64 = weights.iter().sum::<i64>().max(1);
        let mut assigned = 0i64;
        let mut remainders: Vec<(i64, usize)> = Vec::with_capacity(flexible.len());
        for (slot, index) in flexible.iter().enumerate() {
            let exact = remaining * weights[slot];
            let share = exact / total;
            widths[*index] += share as i32;
            assigned += share;
            remainders.push((exact % total, *index));
        }
        let mut left = remaining - assigned;
        remainders.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
        for (_, index) in remainders {
            if left <= 0 {
                break;
            }
            widths[index] += 1;
            left -= 1;
        }
    }
    widths
}

/// Split the measure in proportion to the measured widths, with a floor that
/// keeps every column usable and an exact remainder.
fn proportional_widths(natural: &[i32], measure: i32, floor: i32) -> Vec<i32> {
    let count = natural.len();
    let weights: Vec<i64> = natural
        .iter()
        .map(|width| i64::from((*width).max(1)))
        .collect();
    let total: i64 = weights.iter().sum();
    let rest = (i64::from(measure) - i64::from(floor) * count as i64).max(0);

    let mut widths = vec![floor; count];
    let mut remainders: Vec<(i64, usize)> = Vec::with_capacity(count);
    let mut assigned = 0i64;
    for (index, weight) in weights.iter().enumerate() {
        let exact = rest * weight;
        let share = exact / total;
        widths[index] += share as i32;
        assigned += share;
        remainders.push((exact % total, index));
    }
    let mut left = rest - assigned;
    remainders.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    for (_, index) in remainders {
        if left <= 0 {
            break;
        }
        widths[index] += 1;
        left -= 1;
    }
    widths
}

fn table_rule(
    node: NodeId,
    style: StyleId,
    x: Width,
    y: Width,
    width: Width,
    height: Width,
) -> PlacedBlock {
    PlacedBlock {
        node,
        kind: BoxKind::TableRule,
        style,
        x,
        y,
        width,
        height,
        lines: Vec::new(),
        marks: Vec::new(),
        line_offset: 0,
        lines_total: 0,
        math: None,
        inline_math: Vec::new(),
        ruby: Vec::new(),
        stage: FallbackStage::Normal,
    }
}

struct FlowItem {
    blocks: Vec<PlacedBlock>,
    height: Width,
    before: Glue,
    after: Glue,
    keep_with_next: bool,
    page_break_before: bool,
    forced_break: bool,
    /// First item after a section marker: the page it starts may use the
    /// section-start master.
    section_start: bool,
    footnotes: Vec<usize>,
    /// Empty for atomic blocks; one entry per line for splittable blocks.
    parts: Vec<Part>,
    /// Page master index in effect for this item (`0` is the default).
    master: usize,
    /// Set for table row groups; drives header repetition.
    table: Option<TableContext>,
}

/// One heading recorded during the flow, for running titles and the TOC.
struct Heading {
    /// Index of the heading's flow item.
    item: usize,
    level: u8,
    text: String,
}

/// One vertical item handed to the page breaker.
struct VItem {
    item: usize,
    part: usize,
    height: Width,
    before: Glue,
    after: Glue,
    notes: Vec<usize>,
    /// Charged when the page breaks after this item.
    penalty: i64,
    /// `false` when `keep-with-next` forbids a break here.
    can_break: bool,
    /// Page master index, copied from the flow item.
    master: usize,
    /// Table metadata, copied from the flow item.
    table: Option<TableContext>,
}

struct NoteMap {
    labels: Vec<(String, usize)>,
    /// Compiled footnote blocks per definition.
    blocks: Vec<Vec<PlacedBlock>>,
}

impl NoteMap {
    fn build(doc: &Document) -> Self {
        Self {
            labels: doc
                .footnotes
                .iter()
                .enumerate()
                .map(|(index, definition)| (definition.label.clone(), index))
                .collect(),
            blocks: Vec::new(),
        }
    }

    fn find(&self, label: &str) -> Option<usize> {
        self.labels
            .iter()
            .find(|(candidate, _)| candidate == label)
            .map(|(_, index)| *index)
    }
}

/// Placement of a generated object row: left offset, available width, and the
/// alignment applied after breaking.
#[derive(Clone, Copy)]
struct Slot {
    x: Width,
    width: Width,
    align: Align,
}

struct Builder<'a> {
    styled: &'a StyledDoc,
    metrics: &'a dyn Metrics,
    engine: &'a BreakConfig,
    /// Hyphenation patterns, when the host supplied a set.
    hyphen: Option<&'a soma_hyphen::Patterns>,
    content_width: Width,
    /// Height of the page content box; an unbreakable object taller than this
    /// would overflow every page, so diagrams fall back to their source.
    page_height: Width,
    /// Indent contributed by enclosing quotes and lists: a block inside a
    /// quote is narrowed by the quote's `indent-left`/`indent-right` as well as
    /// by its own.
    extra_left: Width,
    extra_right: Width,
    text: TextIndex,
    notes: NoteMap,
    /// `(flow item index, node, plain text)` for headings, in document order.
    headings: Vec<Heading>,
    /// Repeated header templates, indexed by `TableContext::id`.
    tables: Vec<TableHeader>,
    /// Page master selected by the most recent `Section`.
    current_master: usize,
    /// Next synthetic node id for generated content (TOC rows, running text).
    generated: u32,
    /// A `Section` was seen and its first content item is still unmarked.
    section_start_pending: bool,
    /// Ordinal of the section that owns the paragraphs being compiled. It
    /// changes at every `Block::Section`; `Mark::section` copies it so the
    /// numbering pass can restart without re-walking the DocIR (D56).
    current_section: u32,
    /// Heading depth the TOC includes; `0` disables collection.
    toc_depth: u8,
    /// Entries from the previous TOC pass; empty on the first pass.
    toc_entries: Vec<TocEntry>,
    /// Automatic numbering scheme.
    numbering: soma_template::NumberingSpec,
    /// Committed heading counters for levels 1..=3.
    heading_counts: [u32; 3],
    figure_count: u32,
    equation_count: u32,
    table_count: u32,
    /// Generated items at the head of the flow (cover + contents). Pages that
    /// only hold these are front matter.
    front_matter_items: usize,
    /// Index terms seen while compiling the current block.
    pending_index: Vec<index::IndexTerm>,
    /// Index terms with the flow item they occur in, in document order.
    index_terms: Vec<(index::IndexTerm, usize)>,
    /// Index terms inside each footnote definition, by definition index.
    note_index_terms: Vec<Vec<index::IndexTerm>>,
    /// Ruby annotations seen while compiling the current block, in atom order.
    pending_ruby: Vec<String>,
    /// Set while compiling a description term: no marker, no hanging indent.
    suppress_hang: bool,
    /// While compiling a table cell the flow item is not known yet, so the
    /// markers stay in `pending_index` until the table has been placed.
    defer_index: bool,
    /// Entries from the previous index pass; empty on the first pass.
    index_entries: Vec<TocEntry>,
    /// Figure and table captions found in this pass, with their flow item.
    figure_refs: Vec<CaptionRef>,
    table_refs: Vec<CaptionRef>,
    /// The last paragraph ended well before the measure, so a following
    /// display formula uses TeX's short display skips.
    last_line_short: bool,
}

impl Builder<'_> {
    fn style(&self, node: NodeId, fallback: StyleId) -> StyleId {
        self.styled.style_of(node).unwrap_or(fallback)
    }

    fn computed(&self, style: StyleId) -> ComputedStyle {
        self.styled
            .style(style)
            .cloned()
            .unwrap_or_else(ComputedStyle::base)
    }

    fn before_glue(&self, computed: &ComputedStyle) -> Glue {
        Glue {
            width: computed.space_before,
            plus: computed.space_before_plus,
            minus: computed.space_before_minus,
        }
    }

    fn after_glue(&self, computed: &ComputedStyle) -> Glue {
        Glue {
            width: computed.space_after,
            plus: computed.space_after_plus,
            minus: computed.space_after_minus,
        }
    }

    fn flow_into(
        &mut self,
        blocks: &[Block],
        fallback: StyleId,
        items: &mut Vec<FlowItem>,
        collect_index: bool,
    ) -> Result<(), LayoutError> {
        for block in blocks {
            let before = items.len();
            match block {
                Block::Paragraph(node) => {
                    let style = self.style(node.meta.id, fallback);
                    if let Some(item) =
                        self.paragraph_item(node.meta.id, BoxKind::Paragraph, &node.inlines, style)?
                    {
                        items.push(item);
                    }
                }
                Block::Heading(node) => {
                    let style = self.style(node.meta.id, fallback);
                    let index = items.len();
                    let number = self.heading_number(node.level);
                    let inlines = self.prefixed_inlines(&node.inlines, &number);
                    if let Some(item) =
                        self.paragraph_item(node.meta.id, BoxKind::Heading, &inlines, style)?
                    {
                        self.text.mark_heading(node.meta.id, node.level);
                        items.push(item);
                        self.headings.push(Heading {
                            item: index,
                            level: node.level,
                            text: inline_text(&inlines),
                        });
                        // A level-1 heading opens a chapter: if it opens a
                        // page, that page may use the section-start master.
                        if node.level == 1 {
                            items[index].section_start = true;
                            // A sample sheet reads as one continuous flow even
                            // though its template is a book. The mode is part
                            // of the template, not a build-script edit.
                            if self.styled.layout_mode() == soma_template::LayoutMode::Sample {
                                items[index].page_break_before = false;
                            }
                        }
                    }
                }
                Block::List(node) => {
                    let style = self.style(node.meta.id, fallback);
                    let list_computed = self.computed(style);
                    let list_before = self.before_glue(&list_computed);
                    let list_after = self.after_glue(&list_computed);
                    let list_indent = list_computed.indent_left;
                    let first = items.len();
                    for (index, item) in node.items.iter().enumerate() {
                        let item_style = self.style(item.meta.id, style);
                        // A description term is a bold run-in label: it has no
                        // marker, and it sits at the list indent rather than a
                        // marker column further in.
                        let description = item.meta.attrs.role.as_deref() == Some("description");
                        let item_indent = if description {
                            Width::ZERO
                        } else {
                            self.computed(item_style).indent_left
                        };
                        self.extra_left = self
                            .extra_left
                            .saturating_add(list_indent)
                            .saturating_add(item_indent);
                        // `•` for a bullet, `1.` for a number. The marker goes
                        // into the item's first paragraph so it shares that
                        // paragraph's first line.
                        let marker = if description {
                            String::new()
                        } else if node.ordered {
                            alloc::format!("{}.", node.start + index as u32)
                        } else {
                            String::from("\u{2022}")
                        };
                        let mut marked = false;
                        for block in &item.blocks {
                            if !marked {
                                if let Block::Paragraph(paragraph) = block {
                                    let inlines = if description {
                                        paragraph.inlines.clone()
                                    } else {
                                        self.prefixed_inlines(&paragraph.inlines, &marker)
                                    };
                                    let style = self.style(paragraph.meta.id, item_style);
                                    // The template's negative first-indent says
                                    // "hang"; the actual hang is the measured
                                    // marker prefix, not the template's 1.2em
                                    // guess. Otherwise the first text column and
                                    // the wrapped lines differ by the font's
                                    // bullet/space advance.
                                    let marker_width = if description {
                                        None
                                    } else {
                                        Some(self.metrics.advance(&format!("{marker} "), style))
                                    };
                                    self.suppress_hang = description;
                                    let result = self.paragraph_item_with_hang(
                                        paragraph.meta.id,
                                        BoxKind::Paragraph,
                                        &inlines,
                                        style,
                                        marker_width,
                                    );
                                    self.suppress_hang = false;
                                    if let Some(item) = result? {
                                        items.push(item);
                                        marked = true;
                                        continue;
                                    }
                                }
                            }
                            self.flow_into(
                                core::slice::from_ref(block),
                                item_style,
                                items,
                                collect_index,
                            )?;
                            marked = true;
                        }
                        self.extra_left = Width(self.extra_left.0 - list_indent.0 - item_indent.0);
                    }
                    // A list is one block: its own margins sit outside the
                    // item spacing, so the paragraph after it never touches
                    // the last marker (and the first item does not float up
                    // against the preceding text).
                    if items.len() > first {
                        items[first].before = items[first].before.outer(list_before);
                        let last = items.len() - 1;
                        items[last].after = items[last].after.outer(list_after);
                    }
                }
                Block::BlockQuote(node) => {
                    let style = self.style(node.meta.id, fallback);
                    let computed = self.computed(style);
                    self.extra_left = self.extra_left.saturating_add(computed.indent_left);
                    self.extra_right = self.extra_right.saturating_add(computed.indent_right);
                    let result = self.flow_into(&node.blocks, style, items, collect_index);
                    self.extra_left = Width(self.extra_left.0 - computed.indent_left.0);
                    self.extra_right = Width(self.extra_right.0 - computed.indent_right.0);
                    result?;
                }
                Block::CodeBlock(node) => {
                    let style = self.style(node.meta.id, fallback);
                    if let Some(item) =
                        self.code_item(node.meta.id, node.lang.as_deref(), &node.value, style)?
                    {
                        items.push(item);
                    }
                }
                Block::Diagram(node) => {
                    let style = self.style(node.meta.id, fallback);
                    match self.diagram_item(node.meta.id, node.dialect, &node.source, style)? {
                        Some(item) => items.push(item),
                        None => items.push(self.object_item(
                            node.meta.id,
                            BoxKind::Math,
                            &node.source,
                            style,
                        )?),
                    }
                }
                Block::MathBlock(node) => {
                    let style = self.style(node.meta.id, fallback);
                    match self.math_item(node.meta.id, &node.source, style) {
                        Some(item) => items.push(item),
                        // Malformed formulas keep the text placeholder: a math
                        // error must not fail the page.
                        None => items.push(self.object_item(
                            node.meta.id,
                            BoxKind::Math,
                            &node.source,
                            style,
                        )?),
                    }
                }
                Block::Figure(node) => {
                    let style = self.style(node.meta.id, fallback);
                    // An image needs an accessible name. The author's alt text
                    // wins; otherwise the caption is the honest description.
                    let alt = node
                        .image
                        .source
                        .alt
                        .clone()
                        .filter(|text| !text.is_empty())
                        .or_else(|| {
                            let text = inline_text(&node.caption);
                            (!text.is_empty()).then_some(text)
                        });
                    items.push(self.figure_item(
                        node.meta.id,
                        &node.image.source,
                        alt.as_deref(),
                        style,
                    )?);
                    if !node.caption.is_empty() {
                        // A figure and its caption are one unit: a page never
                        // ends between them (a lone caption is not a caption).
                        if let Some(figure) = items.last_mut() {
                            figure.keep_with_next = true;
                        }
                        let caption_node = node
                            .caption
                            .first()
                            .map(|inline| inline.meta().id)
                            .unwrap_or(node.meta.id);
                        self.figure_count += 1;
                        let pattern = self.numbering.figures.clone();
                        let label = self.caption_label(pattern.as_deref(), self.figure_count);
                        let inlines = self.prefixed_inlines(&node.caption, &label);
                        let caption_style = self.styled.figure_caption_style();
                        let index = items.len();
                        if let Some(item) = self.paragraph_item(
                            caption_node,
                            BoxKind::Paragraph,
                            &inlines,
                            caption_style,
                        )? {
                            items.push(item);
                            self.figure_refs.push(CaptionRef {
                                item: index,
                                text: inline_text(&inlines),
                            });
                        }
                    }
                }
                Block::Table(node) => {
                    let style = self.style(node.meta.id, fallback);
                    let table_computed = self.computed(style);
                    let columns = node.columns.len().max(1);
                    let row_count = node.rows.len();
                    if row_count == 0 {
                        continue;
                    }
                    // Size the columns to what they hold, not to the column
                    // count: a table of "序号 / 说明" must not give both halves
                    // of the page (D90).
                    let mut natural = vec![0i32; columns];
                    for row in &node.rows {
                        let row_style = self.style(row.meta.id, style);
                        let mut column = 0usize;
                        for cell in &row.cells {
                            if column >= columns {
                                break;
                            }
                            let colspan = (cell.colspan as usize)
                                .max(1)
                                .min(columns.saturating_sub(column).max(1));
                            let cell_style = self.style(cell.meta.id, row_style);
                            let widest = self.natural_width(&cell.blocks, cell_style);
                            if colspan == 1 {
                                natural[column] = natural[column].max(widest);
                            } else {
                                let current: i32 = natural[column..column + colspan].iter().sum();
                                if widest > current {
                                    let deficit = widest - current;
                                    let per = deficit / colspan as i32;
                                    for slot in &mut natural[column..column + colspan] {
                                        *slot += per;
                                    }
                                    natural[column + colspan - 1] +=
                                        deficit - per * (colspan as i32 - 1);
                                }
                            }
                            column += colspan;
                        }
                    }
                    let shares: Vec<u16> = node.columns.iter().map(|column| column.share).collect();
                    let natural_mode =
                        self.styled.table_width() == soma_template::TableWidth::Natural;
                    let mut sizing = natural.clone();
                    if natural_mode {
                        for width in &mut sizing {
                            *width += 2 * TABLE_CELL_PAD_X;
                        }
                    }
                    let (widths, table_width, table_left) = if natural_mode {
                        natural_table_widths(&sizing, &shares, self.content_width.0)
                    } else {
                        let widths = column_widths(&natural, &shares, self.content_width.0);
                        (widths, self.content_width.0, 0)
                    };
                    let mut column_x = alloc::vec![0i32; columns + 1];
                    column_x[0] = table_left;
                    for index in 0..columns {
                        column_x[index + 1] = column_x[index] + widths[index];
                    }
                    let mut occupied = vec![vec![false; columns]; row_count];
                    let mut cells: Vec<TableCellLayout> = Vec::new();
                    // Markers inside cells are collected while the cells are
                    // compiled and attached to the table once it is placed.
                    let outer_defer = self.defer_index;
                    if collect_index {
                        self.defer_index = true;
                    }

                    for (row_index, row) in node.rows.iter().enumerate() {
                        let row_style = self.style(row.meta.id, style);
                        let mut column = 0usize;
                        for cell in &row.cells {
                            while column < columns && occupied[row_index][column] {
                                column += 1;
                            }
                            let colspan = (cell.colspan as usize)
                                .max(1)
                                .min(columns.saturating_sub(column).max(1));
                            let rowspan = (cell.rowspan as usize)
                                .max(1)
                                .min(row_count.saturating_sub(row_index).max(1));
                            let cell_pad = if natural_mode { TABLE_CELL_PAD_X } else { 0 };
                            let width = Width(
                                (column_x[column + colspan] - column_x[column] - 2 * cell_pad)
                                    .max(1),
                            );
                            let cell_style = self.style(cell.meta.id, row_style);
                            let mut cell_items = Vec::new();
                            let saved = self.content_width;
                            self.content_width = width;
                            // A cell's blocks are not flow items, so their
                            // markers wait for the table item to exist.
                            self.flow_into(
                                &cell.blocks,
                                cell_style,
                                &mut cell_items,
                                collect_index,
                            )?;
                            self.content_width = saved;

                            let mut blocks = Vec::new();
                            let mut height = Width::ZERO;
                            for item in cell_items {
                                for mut placed in item.blocks {
                                    placed.kind = BoxKind::TableCell;
                                    placed.x = placed.x
                                        + Width(
                                            column_x[column]
                                                + if natural_mode { cell_pad } else { 0 },
                                        );
                                    placed.y = height;
                                    height = height + placed.height;
                                    // The structure tree needs the grid
                                    // position; the box tree only has pixels.
                                    self.text.mark_cell(
                                        placed.node,
                                        TableCell {
                                            row: row_index as u16,
                                            column: column as u16,
                                            row_span: rowspan as u16,
                                            column_span: colspan as u16,
                                        },
                                    );
                                    blocks.push(placed);
                                }
                            }
                            for occupied_row in occupied
                                .iter_mut()
                                .take(row_index + rowspan)
                                .skip(row_index)
                            {
                                for occupied_cell in
                                    occupied_row.iter_mut().take(column + colspan).skip(column)
                                {
                                    *occupied_cell = true;
                                }
                            }
                            cells.push(TableCellLayout {
                                row: row_index,
                                column,
                                colspan,
                                rowspan,
                                blocks,
                                height,
                            });
                            column += colspan;
                        }
                    }

                    let cell_index_terms = core::mem::take(&mut self.pending_index);
                    self.defer_index = outer_defer;

                    // Row gaps are part of the row height (except above row 0).
                    let mut row_heights = vec![Width::ZERO; row_count];
                    for (row_index, row) in node.rows.iter().enumerate() {
                        if row_index == 0 {
                            continue;
                        }
                        let row_style = self.style(row.meta.id, style);
                        let computed = self.computed(row_style);
                        row_heights[row_index] = row_heights[row_index] + computed.space_before;
                    }
                    for cell in cells.iter().filter(|cell| cell.rowspan == 1) {
                        row_heights[cell.row] = row_heights[cell.row].max(cell.height);
                    }
                    for cell in cells.iter().filter(|cell| cell.rowspan > 1) {
                        let span = row_heights[cell.row..cell.row + cell.rowspan]
                            .iter()
                            .fold(Width::ZERO, |sum, height| sum + *height);
                        if cell.height > span {
                            let deficit = Width(cell.height.0 - span.0);
                            let per = Width(deficit.0 / cell.rowspan as i32);
                            for height in row_heights
                                .iter_mut()
                                .take(cell.row + cell.rowspan - 1)
                                .skip(cell.row)
                            {
                                *height = *height + per;
                            }
                            let last = cell.row + cell.rowspan - 1;
                            row_heights[last] = row_heights[last]
                                + Width(deficit.0 - per.0 * (cell.rowspan as i32 - 1));
                        }
                    }

                    // Leading header rows are repeated on continuation pages.
                    let mut header_rows = 0usize;
                    while header_rows < row_count && node.rows[header_rows].header {
                        header_rows += 1;
                    }

                    // Rules occupy their own vertical space, so a continuation
                    // page can shift them like any other block.
                    let rules = self.styled.table_rules();
                    let mut rule_after = vec![Width::ZERO; row_count];
                    for (row, slot) in rule_after.iter_mut().enumerate() {
                        *slot = if row + 1 == row_count {
                            rules.outer_rule
                        } else if header_rows > 0 && row < header_rows {
                            rules.header_rule
                        } else {
                            rules.rule
                        };
                    }
                    // Booktabs puts air on both sides of a horizontal rule.
                    // Only the rule boundaries pay it; body rows keep the
                    // plain line height, so a three-line table is compact.
                    let mut row_top = Vec::with_capacity(row_count + 1);
                    let mut rule_top = vec![Width::ZERO; row_count];
                    let mut cursor = if rules.outer_rule.0 > 0 {
                        rules.outer_rule + TABLE_RULE_SEP
                    } else {
                        Width::ZERO
                    };
                    for (row, height) in row_heights.iter().enumerate() {
                        row_top.push(cursor);
                        cursor = cursor + *height;
                        if rule_after[row].0 > 0 {
                            cursor = cursor + TABLE_RULE_SEP;
                            rule_top[row] = cursor;
                            cursor = cursor + rule_after[row];
                            if row + 1 < row_count {
                                cursor = cursor + TABLE_RULE_SEP;
                            }
                        } else {
                            rule_top[row] = cursor;
                        }
                    }
                    row_top.push(cursor);
                    let table_height = cursor;
                    let table_width = Width(table_width);
                    let header_height = row_top.get(header_rows).copied().unwrap_or(cursor);

                    let mut blocks = Vec::new();
                    let pen = RulePen {
                        node: node.meta.id,
                        style: rules.style_id,
                        dashed: rules.style == soma_template::RuleStyle::Dashed,
                    };
                    if rules.outer_rule.0 > 0 {
                        push_horizontal_rule(
                            &mut blocks,
                            pen,
                            Width(column_x[0]),
                            Width::ZERO,
                            table_width,
                            rules.outer_rule,
                        );
                    }
                    for cell in &cells {
                        let cell_top = row_top[cell.row];
                        for block in &cell.blocks {
                            let mut placed = block.clone();
                            placed.y = placed.y + cell_top;
                            blocks.push(placed);
                        }
                    }
                    for row in 0..row_count {
                        let thickness = rule_after[row];
                        if thickness.0 == 0 {
                            continue;
                        }
                        let y = rule_top[row];
                        // A rule never crosses a cell that spans the boundary,
                        // so the line is emitted column by column.
                        let mut covered = vec![false; columns];
                        for cell in &cells {
                            if cell.rowspan > 1
                                && cell.row <= row
                                && row + 1 < cell.row + cell.rowspan
                            {
                                for slot in covered
                                    .iter_mut()
                                    .take(cell.column + cell.colspan)
                                    .skip(cell.column)
                                {
                                    *slot = true;
                                }
                            }
                        }
                        let mut column = 0usize;
                        while column < columns {
                            if covered[column] {
                                column += 1;
                                continue;
                            }
                            let start = column;
                            while column < columns && !covered[column] {
                                column += 1;
                            }
                            push_horizontal_rule(
                                &mut blocks,
                                pen,
                                Width(column_x[start]),
                                y,
                                Width(column_x[column] - column_x[start]),
                                thickness,
                            );
                        }
                    }

                    // Vertical rules: internal boundaries plus the left/right
                    // frame. A cell that spans the boundary hides the segment
                    // for its own rows, and the header band is split out so the
                    // repeated header can carry its own side lines.
                    for (boundary, edge) in column_x.iter().enumerate() {
                        let thickness = if boundary == 0 || boundary == columns {
                            rules.outer_column_rule
                        } else {
                            rules.column_rule
                        };
                        if thickness.0 == 0 {
                            continue;
                        }
                        let x = if boundary == 0 {
                            Width(column_x[0])
                        } else if boundary == columns {
                            Width(column_x[columns] - thickness.0)
                        } else {
                            Width(*edge - thickness.0 / 2)
                        };
                        let mut covered: Vec<(i32, i32)> = Vec::new();
                        if boundary > 0 && boundary < columns {
                            for cell in &cells {
                                if cell.column < boundary && boundary < cell.column + cell.colspan {
                                    let start = row_top[cell.row].0;
                                    let end_row = cell.row + cell.rowspan - 1;
                                    let end = row_top[end_row + 1].0 - rule_after[end_row].0;
                                    covered.push((start, end.max(start)));
                                }
                            }
                        }
                        covered.sort_unstable();
                        let mut cursor = 0i32;
                        let mut emit = |from: i32, to: i32| {
                            if to <= from {
                                return;
                            }
                            let mut start = from;
                            if header_rows > 0 && start < header_height.0 && to > header_height.0 {
                                push_vertical_rule(
                                    &mut blocks,
                                    pen,
                                    x,
                                    Width(start),
                                    Width(header_height.0 - start),
                                    thickness,
                                );
                                start = header_height.0;
                            }
                            push_vertical_rule(
                                &mut blocks,
                                pen,
                                x,
                                Width(start),
                                Width(to - start),
                                thickness,
                            );
                        };
                        for (start, end) in covered {
                            if start > cursor {
                                emit(cursor, start);
                            }
                            cursor = cursor.max(end);
                        }
                        emit(cursor, table_height.0);
                    }

                    let mut header_blocks = Vec::new();
                    if header_rows > 0 {
                        header_blocks.extend(
                            blocks
                                .iter()
                                .filter(|block| block.y.0 + block.height.0 <= header_height.0)
                                .cloned(),
                        );
                    }
                    let table_id = self.tables.len().min(u16::MAX as usize) as u16;
                    self.tables.push(TableHeader {
                        blocks: header_blocks,
                        height: header_height,
                    });

                    // A page break may fall between rows unless a rowspan
                    // cell crosses that boundary. The repeated header is kept
                    // whole for the same reason.
                    let mut safe = vec![true; row_count];
                    let mut header_split = false;
                    for cell in &cells {
                        if cell.rowspan > 1 {
                            if cell.row < header_rows && cell.row + cell.rowspan > header_rows {
                                // A spanning header cell cannot be repeated
                                // without duplicating part of the body.
                                header_split = true;
                            }
                            for slot in safe
                                .iter_mut()
                                .take(cell.row + cell.rowspan - 1)
                                .skip(cell.row)
                            {
                                *slot = false;
                            }
                        }
                    }
                    for slot in safe.iter_mut().take(header_rows.saturating_sub(1)) {
                        *slot = false;
                    }

                    // Maximal runs of rows that no cell spans across. Each run
                    // becomes one part, so the page breaker can continue the
                    // table without cutting through a cell.
                    let mut units: Vec<(usize, usize)> = Vec::new();
                    if header_split {
                        units.push((0, row_count));
                    } else {
                        let mut start = 0usize;
                        for (row, slot) in safe.iter().enumerate() {
                            if *slot || row + 1 == row_count {
                                units.push((start, row + 1));
                                start = row + 1;
                            }
                        }
                    }

                    let parts: Vec<Part> = if units.len() > 1 {
                        units
                            .iter()
                            .enumerate()
                            .map(|(index, &(start, end))| {
                                // The first band owns the outer top rule, so it
                                // starts at zero rather than at `row_top[0]`.
                                let top = if index == 0 {
                                    Width::ZERO
                                } else {
                                    row_top[start]
                                };
                                Part {
                                    height: Width(row_top[end].0 - top.0),
                                    notes: Vec::new(),
                                    row: start as u32,
                                }
                            })
                            .collect()
                    } else {
                        Vec::new()
                    };
                    let table_index = items.len();
                    items.push(FlowItem {
                        blocks,
                        height: table_height,
                        before: self.before_glue(&table_computed),
                        after: self.after_glue(&table_computed),
                        keep_with_next: false,
                        page_break_before: false,
                        forced_break: false,
                        section_start: false,
                        footnotes: Vec::new(),
                        parts,
                        master: self.current_master,
                        table: Some(TableContext {
                            id: table_id,
                            row: 0,
                            header_rows: header_rows as u32,
                            header_height,
                        }),
                    });

                    if collect_index && !outer_defer {
                        for term in cell_index_terms {
                            self.index_terms.push((term, table_index));
                        }
                    }

                    if !node.caption.is_empty() {
                        let caption_node = node
                            .caption
                            .first()
                            .map(|inline| inline.meta().id)
                            .unwrap_or(node.meta.id);
                        self.table_count += 1;
                        let pattern = self.numbering.tables.clone();
                        let label = self.caption_label(pattern.as_deref(), self.table_count);
                        let inlines = self.prefixed_inlines(&node.caption, &label);
                        let caption_style = self.styled.table_caption_style();
                        if let Some(mut item) = self.paragraph_item(
                            caption_node,
                            BoxKind::Paragraph,
                            &inlines,
                            caption_style,
                        )? {
                            // A table caption sits above its table; keep the
                            // pair together across a page break.
                            item.keep_with_next = true;
                            items.insert(table_index, item);
                            self.table_refs.push(CaptionRef {
                                item: table_index,
                                text: inline_text(&inlines),
                            });
                        }
                    }
                }
                Block::ThematicBreak(node) => {
                    let style = self.style(node.meta.id, fallback);
                    let computed = self.computed(style);
                    let height = computed
                        .line_height
                        .scale(1, 2)
                        .max(Width::from_em_frac(1, 8));
                    items.push(FlowItem {
                        blocks: vec![PlacedBlock {
                            node: node.meta.id,
                            kind: BoxKind::Rule,
                            style,
                            x: Width::ZERO,
                            y: Width::ZERO,
                            width: self.content_width,
                            height,
                            lines: Vec::new(),
                            marks: Vec::new(),
                            line_offset: 0,
                            lines_total: 0,
                            math: None,
                            inline_math: Vec::new(),
                            ruby: Vec::new(),
                            stage: FallbackStage::Normal,
                        }],
                        height,
                        before: self.before_glue(&computed),
                        after: self.after_glue(&computed),
                        keep_with_next: false,
                        page_break_before: false,
                        forced_break: false,
                        section_start: false,
                        footnotes: Vec::new(),
                        parts: Vec::new(),
                        master: self.current_master,
                        table: None,
                    });
                }
                Block::PageBreak(_) => items.push(FlowItem {
                    blocks: Vec::new(),
                    height: Width::ZERO,
                    before: Glue::ZERO,
                    after: Glue::ZERO,
                    keep_with_next: false,
                    page_break_before: false,
                    forced_break: true,
                    section_start: false,
                    footnotes: Vec::new(),
                    parts: Vec::new(),
                    master: self.current_master,
                    table: None,
                }),
                Block::Section(node) => {
                    let style = self.style(node.meta.id, fallback);
                    self.section_start_pending = true;
                    // Footnote `numbering: section` restarts after the marker,
                    // including a section that begins in the middle of a page.
                    self.current_section = self.current_section.saturating_add(1);
                    if let Some(name) = &node.page_template {
                        let index = self.styled.master_index(name).ok_or_else(|| {
                            LayoutError::new(Some(node.meta.id), "unknown page master")
                        })?;
                        if index != self.current_master {
                            self.current_master = index;
                            // A new master starts a new page; otherwise the
                            // first item on the old page would keep the master.
                            if !items.is_empty() {
                                items.push(FlowItem {
                                    blocks: Vec::new(),
                                    height: Width::ZERO,
                                    before: Glue::ZERO,
                                    after: Glue::ZERO,
                                    keep_with_next: false,
                                    page_break_before: false,
                                    forced_break: true,
                                    section_start: false,
                                    footnotes: Vec::new(),
                                    parts: Vec::new(),
                                    master: index,
                                    table: None,
                                });
                            }
                        }
                    }
                    if let Some(title) = &node.title {
                        let index = items.len();
                        if let Some(item) =
                            self.paragraph_item(node.meta.id, BoxKind::Heading, title, style)?
                        {
                            items.push(item);
                            self.headings.push(Heading {
                                item: index,
                                level: node.level,
                                text: inline_text(title),
                            });
                        }
                    }
                }
                Block::Generated(node) => {
                    let style = self.style(node.meta.id, fallback);
                    self.flow_into(&node.blocks, style, items, false)?;
                }
                Block::Raw(node) => {
                    let style = self.style(node.meta.id, fallback);
                    if !node.content.fallback.is_empty() {
                        items.push(self.object_item(
                            node.meta.id,
                            BoxKind::Paragraph,
                            &node.content.fallback,
                            style,
                        )?);
                    }
                }
            }

            // Index markers belong to the block that was just compiled; the
            // last item is the caption when a float added one. Markers inside a
            // table cell are attached by the table, once its item exists.
            if collect_index && !self.defer_index {
                if items.len() > before && !self.pending_index.is_empty() {
                    let item = items.len() - 1;
                    for term in self.pending_index.drain(..) {
                        self.index_terms.push((term, item));
                    }
                } else {
                    self.pending_index.clear();
                }
                // A footnote is printed on the page that refers to it, so its
                // markers belong to the referring item, not to the note area.
                for (item, flow) in items.iter().enumerate().skip(before) {
                    for note in flow.footnotes.clone() {
                        if let Some(terms) = self.note_index_terms.get(note) {
                            for term in terms {
                                self.index_terms.push((term.clone(), item));
                            }
                        }
                    }
                }
            } else if !collect_index {
                self.pending_index.clear();
            }

            // The first content item after a section marker opens a chapter
            // page; empty break markers do not count.
            if self.section_start_pending {
                if let Some(position) =
                    (before..items.len()).find(|index| !items[*index].blocks.is_empty())
                {
                    items[position].section_start = true;
                    self.section_start_pending = false;
                }
            }
        }
        Ok(())
    }

    fn paragraph_item(
        &mut self,
        node: NodeId,
        kind: BoxKind,
        inlines: &[Inline],
        style_id: StyleId,
    ) -> Result<Option<FlowItem>, LayoutError> {
        self.paragraph_item_with_hang(node, kind, inlines, style_id, None)
    }

    fn paragraph_item_with_hang(
        &mut self,
        node: NodeId,
        kind: BoxKind,
        inlines: &[Inline],
        style_id: StyleId,
        hang_override: Option<Width>,
    ) -> Result<Option<FlowItem>, LayoutError> {
        let computed = self.computed(style_id);
        // A quote or a list indents the blocks it holds; the blocks declare
        // their own indent on top of it.
        let left = computed.indent_left.saturating_add(self.extra_left);
        let right = computed.indent_right.saturating_add(self.extra_right);
        let consumed = left.saturating_add(right);
        let inner = Width(self.content_width.0 - consumed.0);
        // A negative `first-indent` is a hanging indent: the template says
        // where the marker goes, the kernel gets `hang` (ADR-0058). A list
        // marker overrides the guess with its measured prefix width so the
        // first text column and every wrapped line start at the same x.
        let hang = if self.suppress_hang {
            Width::ZERO
        } else if computed.first_indent.0 < 0 {
            hang_override.unwrap_or(Width(-computed.first_indent.0))
        } else {
            Width::ZERO
        };
        let first_indent = if self.suppress_hang {
            Width::ZERO
        } else {
            Width(computed.first_indent.0.max(0))
        };
        let paragraph_style = ParagraphStyle {
            width: inner,
            first_indent,
            hang,
            tracking: computed.tracking,
            line_height: computed.line_height,
            // The kernel only justifies or flushes left; centering and
            // right-alignment are applied to the finished lines below.
            align: kernel_align(computed.align),
        };

        let mut spans: Vec<OwnedSpan> = Vec::new();
        let mut footnotes: Vec<usize> = Vec::new();
        let mut marker_notes: Vec<Option<usize>> = Vec::new();
        let mut objects = math::InlineMaths::new();
        self.pending_ruby.clear();
        self.collect(
            inlines,
            style_id,
            Decorations::NONE,
            None,
            &mut spans,
            &mut footnotes,
            &mut marker_notes,
            &mut objects,
        );

        let borrowed: Vec<InlineSpan<'_>> = spans
            .iter()
            .map(|span| InlineSpan {
                text: &span.text,
                style: span.style,
                kind: span.kind,
                decor: span.decor,
                link: span.link.as_deref(),
                width: span.width,
                penalty: span.penalty,
            })
            .collect();

        let paragraph = match paragraph_from_spans(&borrowed, paragraph_style) {
            Ok(paragraph) => paragraph,
            Err(RuleError::EmptyText) => return Ok(None),
        };
        let mut paragraph = paragraph;
        // The marker gap is part of the prefix, not of the justified text
        // column. If it kept its stretch/shrink, justification would move the
        // first text run away from the wrap column by a few units. The gap
        // keeps its natural space width; only its adjustability is removed.
        let mut marker_content = None;
        if hang_override.is_some() {
            if let Some(index) = paragraph
                .boundaries
                .iter()
                .position(|boundary| boundary.kind == BoundaryKind::Space)
            {
                paragraph.boundaries[index].glue.stretch = Width::ZERO;
                paragraph.boundaries[index].glue.shrink = Width::ZERO;
                marker_content = Some(index as u32 + 1);
            }
        }
        scale_trim(&mut paragraph, self.metrics, |style| {
            self.computed(style).font_size.0
        });

        let object_metrics = math::ObjectMetrics::new(self.metrics, &objects);
        let mut result = break_paragraph(&paragraph, &object_metrics, self.engine)
            .map_err(|_| LayoutError::new(Some(node), "line breaking failed"))?;
        align_lines(&mut result.lines, computed.align);
        if let (Some(first), Some(content), Some(marker_width)) =
            (result.lines.first_mut(), marker_content, hang_override)
        {
            pin_marker_gap(first, content, marker_width);
        }

        // Ruby: one base atom per annotation, in document order. The base was
        // pushed as a single atom (`SpanKind::Ruby`), so the zip is exact even
        // when the same text occurs elsewhere in the paragraph.
        let mut ruby: Vec<RubySpan> = Vec::new();
        if !self.pending_ruby.is_empty() {
            let notes = core::mem::take(&mut self.pending_ruby);
            let mut notes = notes.into_iter();
            for (index, atom) in paragraph.atoms.iter().enumerate() {
                if atom.kind != soma_core::AtomKind::Ruby {
                    continue;
                }
                let Some(text) = notes.next() else { break };
                let vertical = self.metrics.line_metrics(atom.style);
                let em = self.computed(atom.style).font_size.0;
                ruby.push(RubySpan {
                    atom: index as u32,
                    text,
                    // The annotation sits above the base's ascent, with a
                    // sixteenth of the base em as breathing room.
                    shift: Width(vertical.ascent.0 + em / 16),
                    size: Width(em / 2),
                });
            }
        }
        // A line carrying ruby has to grow upward or the annotation lands on
        // the previous line's descenders.
        for line in &mut result.lines {
            let mut needed = 0;
            for atom in &line.boxes {
                if let Some(note) = ruby.iter().find(|note| note.atom == atom.atom) {
                    needed = needed.max(note.shift.0 + note.size.0);
                }
            }
            if needed > line.ascent.0 {
                let delta = needed - line.ascent.0;
                line.ascent = Width(needed);
                line.height = line.height + Width(delta);
            }
        }

        let inline_math: Vec<InlineMathBox> = objects
            .resolve(&paragraph.atoms)
            .into_iter()
            .map(|(atom, source, layout)| InlineMathBox {
                atom,
                source,
                math: MathBox {
                    width: layout.width,
                    height: layout.total_height(),
                    baseline: layout.height,
                    items: layout.items,
                    rules: layout.rules,
                    paths: Vec::new(),
                },
            })
            .collect();

        let mut marks = marker_positions(&paragraph.atoms, &result.lines);
        for (mark, note) in marks.iter_mut().zip(marker_notes.iter()) {
            mark.note = *note;
            mark.section = self.current_section;
        }
        self.text.insert(
            node,
            paragraph
                .atoms
                .iter()
                .map(|atom| TextEntry {
                    text: atom.text.clone(),
                    style: atom.style,
                    decorations: atom.decor,
                    link: atom.link.clone(),
                    syntax: None,
                })
                .collect(),
        );
        // Record which atoms a rescue face draws, so the renderer picks the
        // same font the breaker measured (ADR-0038, docs/mixed-font.md).
        for (index, atom) in paragraph.atoms.iter().enumerate() {
            if self.metrics.uses_fallback(&atom.text, atom.style) {
                self.text.mark_fallback(node, index as u32);
            }
        }

        let grid = computed.baseline_grid;
        let mut placed_lines = Vec::with_capacity(result.lines.len());
        let mut parts = Vec::with_capacity(result.lines.len());
        let mut y = Width::ZERO;
        for line in &result.lines {
            let mut line = line.clone();
            let hang = if computed.hanging_punctuation && computed.align == Align::Justify {
                match line.end.checked_sub(1) {
                    // A paragraph's last line is left aligned, so it has no
                    // right edge to hang into.
                    Some(last) if (last as usize) + 1 < paragraph.atoms.len() => {
                        let atom = &paragraph.atoms[last as usize];
                        let cap = Width(scale_units(
                            computed.hanging_cap.0,
                            self.computed(atom.style).font_size.0,
                        ));
                        hanging_amount(atom, cap)
                    }
                    _ => 0,
                }
            } else {
                0
            };
            if hang > 0 {
                // Hanging is pure geometry: the line keeps its `adjustment`
                // and its internal distribution, only the last em box moves.
                if let Some(last) = line.boxes.last_mut() {
                    last.x = Width(last.x.0 + hang);
                }
            }
            let advance = snap_to_grid(line.height, grid);
            placed_lines.push(PlacedLine { y, hang, line });
            parts.push(Part {
                height: advance,
                notes: Vec::new(),
                row: 0,
            });
            y = y + advance;
        }
        for (mark, note) in marks.iter().zip(marker_notes.iter()) {
            if let Some(note) = *note {
                if let Some(part) = parts.get_mut(mark.line as usize) {
                    if !part.notes.contains(&note) {
                        part.notes.push(note);
                    }
                }
            }
        }
        let lines_total = placed_lines.len() as u32;
        let em = self.computed(style_id).font_size;
        self.last_line_short = placed_lines
            .last()
            .is_none_or(|line| line.line.natural_width.0 + 2 * em.0 < inner.0);

        Ok(Some(FlowItem {
            height: y,
            before: self.before_glue(&computed),
            after: self.after_glue(&computed),
            keep_with_next: computed.keep_with_next,
            page_break_before: computed.page_break_before,
            forced_break: false,
            section_start: false,
            footnotes,
            parts,
            master: self.current_master,
            table: None,
            blocks: vec![PlacedBlock {
                node,
                kind,
                style: style_id,
                x: left,
                y: Width::ZERO,
                width: inner,
                height: y,
                lines: placed_lines,
                marks,
                line_offset: 0,
                lines_total,
                stage: result.fallback_stage,
                math: None,
                inline_math,
                ruby,
            }],
        }))
    }

    /// The width a block would take if it never wrapped.
    ///
    /// Table columns are sized from this: a column holding two characters
    /// should not get a fifth of the page just because the table has five
    /// columns (D90). Only paragraphs are measured; a cell holding a nested
    /// list or a table contributes nothing and falls back to the floor width.
    fn natural_width(&mut self, blocks: &[Block], fallback: StyleId) -> i32 {
        let mut widest = 0i32;
        for block in blocks {
            let Block::Paragraph(node) = block else {
                continue;
            };
            let style = self.style(node.meta.id, fallback);
            let computed = self.computed(style);
            let mut spans: Vec<OwnedSpan> = Vec::new();
            let mut footnotes = Vec::new();
            let mut notes = Vec::new();
            let mut objects = math::InlineMaths::new();
            // Measuring a column must not eat the ruby of the block that comes
            // after it.
            self.pending_ruby.clear();
            self.collect(
                &node.inlines,
                style,
                Decorations::NONE,
                None,
                &mut spans,
                &mut footnotes,
                &mut notes,
                &mut objects,
            );
            let borrowed: Vec<InlineSpan<'_>> = spans
                .iter()
                .map(|span| InlineSpan {
                    text: &span.text,
                    style: span.style,
                    kind: span.kind,
                    decor: span.decor,
                    link: span.link.as_deref(),
                    width: span.width,
                    penalty: span.penalty,
                })
                .collect();
            let paragraph_style = ParagraphStyle {
                width: Width(self.content_width.0.max(1)),
                first_indent: Width::ZERO,
                hang: Width::ZERO,
                tracking: Width::ZERO,
                line_height: computed.line_height,
                align: Align::Left,
            };
            let Ok(paragraph) = paragraph_from_spans(&borrowed, paragraph_style) else {
                continue;
            };
            let objects_metrics = math::ObjectMetrics::new(self.metrics, &objects);
            let mut width = 0i32;
            for atom in &paragraph.atoms {
                width += objects_metrics.object_advance(&atom.text, atom.style).0;
            }
            // A space boundary carries a zero `Glue.width`; the kernel asks the
            // metrics for the actual space. Measure it the same way here, or a
            // natural table is short by one space per word and wraps a line it
            // already fits.
            for (index, boundary) in paragraph.boundaries.iter().enumerate() {
                width += match boundary.kind {
                    BoundaryKind::Space => self.metrics.space_width(paragraph.atoms[index].style).0,
                    BoundaryKind::Normal | BoundaryKind::Hyphen => boundary.glue.width.0,
                };
            }
            widest = widest.max(width);
        }
        widest
    }

    #[allow(clippy::too_many_arguments)]
    fn collect(
        &mut self,
        inlines: &[Inline],
        fallback: StyleId,
        decor: Decorations,
        link: Option<&str>,
        out: &mut Vec<OwnedSpan>,
        footnotes: &mut Vec<usize>,
        marker_notes: &mut Vec<Option<usize>>,
        objects: &mut math::InlineMaths,
    ) {
        for inline in inlines {
            let style = self.style(inline.meta().id, fallback);
            match inline {
                Inline::Text(node) => match self.hyphen {
                    Some(patterns) => {
                        push_hyphenated_text(&node.value, style, decor, link, patterns, out)
                    }
                    None => out.push(OwnedSpan {
                        text: node.value.clone(),
                        style,
                        kind: SpanKind::Text,
                        decor,
                        link: link.map(String::from),
                        width: Width::ZERO,
                        penalty: 0,
                    }),
                },
                Inline::Code(node) => out.push(OwnedSpan {
                    text: node.value.clone(),
                    style,
                    kind: SpanKind::Code,
                    decor,
                    link: link.map(String::from),
                    width: Width::ZERO,
                    penalty: 0,
                }),
                Inline::Underline(node) => self.collect(
                    &node.children,
                    style,
                    decor.union(Decorations::UNDERLINE),
                    link,
                    out,
                    footnotes,
                    marker_notes,
                    objects,
                ),
                Inline::Strikethrough(node) => self.collect(
                    &node.children,
                    style,
                    decor.union(Decorations::STRIKETHROUGH),
                    link,
                    out,
                    footnotes,
                    marker_notes,
                    objects,
                ),
                Inline::Strong(node)
                | Inline::Emphasis(node)
                | Inline::SmallCaps(node)
                | Inline::Superscript(node)
                | Inline::Subscript(node) => self.collect(
                    &node.children,
                    style,
                    decor,
                    link,
                    out,
                    footnotes,
                    marker_notes,
                    objects,
                ),
                Inline::Link(node) => self.collect(
                    &node.children,
                    style,
                    decor,
                    Some(node.href.as_str()),
                    out,
                    footnotes,
                    marker_notes,
                    objects,
                ),
                Inline::Image(node) => out.push(OwnedSpan {
                    text: node
                        .source
                        .alt
                        .clone()
                        .unwrap_or_else(|| String::from("\u{fffc}")),
                    decor,
                    link: link.map(String::from),
                    width: Width::ZERO,
                    penalty: 0,
                    style,
                    kind: SpanKind::Object,
                }),
                Inline::Math(node) => {
                    // A parsable formula becomes object atoms measured by their
                    // own boxes; a long one is split at relations and binaries
                    // so the paragraph can break inside it (Math v2). A
                    // malformed formula keeps the source text, as before.
                    let computed = self.computed(style);
                    let measure = Width(
                        self.content_width.0
                            - computed.indent_left.0
                            - computed.indent_right.0
                            - self.extra_left.0
                            - self.extra_right.0,
                    );
                    match math::display_inline_parts(
                        &node.source,
                        &computed,
                        self.metrics,
                        style,
                        measure,
                    ) {
                        Some(parts) => {
                            for (index, part) in parts.into_iter().enumerate() {
                                if index > 0 {
                                    out.push(OwnedSpan {
                                        text: String::new(),
                                        style,
                                        kind: SpanKind::MathBreak,
                                        decor,
                                        link: link.map(String::from),
                                        width: part.gap,
                                        penalty: part.penalty,
                                    });
                                }
                                let marker = objects.push(&node.source, part.layout);
                                out.push(OwnedSpan {
                                    text: marker,
                                    style,
                                    kind: SpanKind::Object,
                                    decor,
                                    link: link.map(String::from),
                                    width: Width::ZERO,
                                    penalty: 0,
                                });
                            }
                        }
                        None => out.push(OwnedSpan {
                            text: node.source.clone(),
                            style,
                            kind: SpanKind::Object,
                            decor,
                            link: link.map(String::from),
                            width: Width::ZERO,
                            penalty: 0,
                        }),
                    }
                }
                Inline::FootnoteRef(node) => {
                    let index = self.notes.find(&node.target);
                    if let Some(index) = index {
                        if !footnotes.contains(&index) {
                            footnotes.push(index);
                        }
                    }
                    marker_notes.push(index);
                    out.push(OwnedSpan {
                        text: String::from(FOOTNOTE_MARKER),
                        style,
                        kind: SpanKind::Footnote,
                        decor,
                        link: link.map(String::from),
                        width: Width::ZERO,
                        penalty: 0,
                    });
                }
                Inline::Ruby(node) => {
                    self.pending_ruby.push(node.annotation.clone());
                    out.push(OwnedSpan {
                        text: node.base.clone(),
                        style,
                        kind: SpanKind::Ruby,
                        decor,
                        link: link.map(String::from),
                        width: Width::ZERO,
                        penalty: 0,
                    });
                }
                Inline::Index(node) => self.pending_index.push(index::parse(&node.term)),
                Inline::LineBreak(_) => out.push(OwnedSpan {
                    text: String::new(),
                    style,
                    kind: SpanKind::HardBreak,
                    decor,
                    link: link.map(String::from),
                    width: Width::ZERO,
                    penalty: 0,
                }),
                Inline::Raw(node) => out.push(OwnedSpan {
                    decor,
                    link: link.map(String::from),
                    width: Width::ZERO,
                    penalty: 0,
                    text: if node.content.fallback.is_empty() {
                        node.content.value.clone()
                    } else {
                        node.content.fallback.clone()
                    },
                    style,
                    kind: SpanKind::Text,
                }),
            }
        }
    }

    fn code_item(
        &mut self,
        node: NodeId,
        lang: Option<&str>,
        value: &str,
        style_id: StyleId,
    ) -> Result<Option<FlowItem>, LayoutError> {
        let computed = self.computed(style_id);
        let pad_top = computed.padding_top;
        let pad_bottom = computed.padding_bottom;
        let pad_left = computed.padding_left;
        let mut lines = Vec::new();
        let mut atoms: Vec<TextEntry> = Vec::new();
        let mut y = pad_top;
        let mut stage = FallbackStage::Normal;
        let highlight = !self.styled.table().syntax().is_empty();
        let mut lexer = soma_highlight::Lexer::new(lang);

        // Code is not wrapped in v1: each source line keeps its own line and a
        // long line may overflow. The panel is full-measure; `pad_left` and
        // `pad_right` only move the text inside it (ADR-0090).
        let wide = ParagraphStyle {
            width: Width::from_em(1000),
            first_indent: Width::ZERO,
            hang: Width::ZERO,
            tracking: Width::ZERO,
            line_height: computed.line_height,
            align: computed.align,
        };

        for source_line in value.lines() {
            // Run the lexer for empty lines too: a block comment or a
            // multi-line string does not end because the author left a blank
            // line inside it.
            let classes = if highlight {
                lexer.line(source_line)
            } else {
                Vec::new()
            };
            let paragraph = match paragraph_from_code_line(source_line, wide, style_id) {
                Ok(paragraph) => paragraph,
                Err(RuleError::EmptyText) => {
                    lines.push(empty_line(computed.line_height, y));
                    y = y + computed.line_height;
                    continue;
                }
            };
            // Each source line is broken on its own, so its atoms start at
            // zero; shift them onto the block's atom table so the renderers
            // can look a box up by `(node, atom)`.
            let base = atoms.len() as u32;
            let mut position = 0usize;
            atoms.extend(paragraph.atoms.iter().map(|atom| {
                let length = atom.text.chars().count();
                let syntax = if highlight {
                    class_at(&classes, position)
                } else {
                    None
                };
                position += length;
                TextEntry {
                    text: atom.text.clone(),
                    style: atom.style,
                    decorations: atom.decor,
                    link: atom.link.clone(),
                    syntax,
                }
            }));
            let result = break_paragraph(&paragraph, self.metrics, self.engine)
                .map_err(|_| LayoutError::new(Some(node), "code breaking failed"))?;
            if result.fallback_stage != FallbackStage::Normal {
                stage = result.fallback_stage;
            }
            for line in &result.lines {
                let mut line = line.clone();
                line.start += base;
                line.end += base;
                for placed in &mut line.boxes {
                    placed.atom += base;
                    placed.x = Width(placed.x.0 + pad_left.0);
                }
                y = y + line.height;
                lines.push(PlacedLine {
                    y: y - line.height,
                    hang: 0,
                    line,
                });
            }
        }

        if lines.is_empty() {
            return Ok(None);
        }
        for (index, atom) in atoms.iter().enumerate() {
            if self.metrics.uses_fallback(&atom.text, atom.style) {
                self.text.mark_fallback(node, index as u32);
            }
        }
        self.text.insert(node, atoms);
        let height = y + pad_bottom;

        Ok(Some(FlowItem {
            height,
            before: self.before_glue(&computed),
            after: self.after_glue(&computed),
            keep_with_next: computed.keep_with_next,
            page_break_before: computed.page_break_before,
            forced_break: false,
            section_start: false,
            footnotes: Vec::new(),
            parts: Vec::new(),
            master: self.current_master,
            table: None,
            blocks: vec![PlacedBlock {
                node,
                kind: BoxKind::Code,
                style: style_id,
                x: Width::ZERO,
                y: Width::ZERO,
                width: self.content_width,
                height,
                lines,
                marks: Vec::new(),
                line_offset: 0,
                lines_total: 0,
                stage,
                math: None,
                inline_math: Vec::new(),
                ruby: Vec::new(),
            }],
        }))
    }

    fn object_item(
        &mut self,
        node: NodeId,
        kind: BoxKind,
        text: &str,
        style_id: StyleId,
    ) -> Result<FlowItem, LayoutError> {
        let align = self.computed(style_id).align;
        self.object_item_at(
            node,
            kind,
            text,
            style_id,
            Slot {
                x: Width::ZERO,
                width: self.content_width,
                align,
            },
        )
    }

    /// An object paragraph at an explicit position and width. Used by the TOC,
    /// where rows carry their own indent and the page number sits in a narrow
    /// right-hand box.
    fn object_item_at(
        &mut self,
        node: NodeId,
        kind: BoxKind,
        text: &str,
        style_id: StyleId,
        slot: Slot,
    ) -> Result<FlowItem, LayoutError> {
        self.placed_item_at(node, kind, text, style_id, slot, true)
    }

    /// Like [`Self::object_item_at`], but the text is segmented and may wrap.
    /// TOC titles use this so a long heading breaks inside its box (ADR-0033).
    fn text_item_at(
        &mut self,
        node: NodeId,
        kind: BoxKind,
        text: &str,
        style_id: StyleId,
        slot: Slot,
    ) -> Result<FlowItem, LayoutError> {
        self.placed_item_at(node, kind, text, style_id, slot, false)
    }

    fn placed_item_at(
        &mut self,
        node: NodeId,
        kind: BoxKind,
        text: &str,
        style_id: StyleId,
        slot: Slot,
        object: bool,
    ) -> Result<FlowItem, LayoutError> {
        let computed = self.computed(style_id);
        let paragraph_style = ParagraphStyle {
            width: slot.width,
            first_indent: Width::ZERO,
            hang: Width::ZERO,
            tracking: Width::ZERO,
            line_height: computed.line_height,
            align: kernel_align(slot.align),
        };
        let spans = [if object {
            InlineSpan::object(text, style_id)
        } else {
            InlineSpan::text(text, style_id)
        }];
        let paragraph = paragraph_from_spans(&spans, paragraph_style)
            .map_err(|_| LayoutError::new(Some(node), "object compilation failed"))?;
        let mut result = break_paragraph(&paragraph, self.metrics, self.engine)
            .map_err(|_| LayoutError::new(Some(node), "object breaking failed"))?;
        align_lines(&mut result.lines, slot.align);
        self.text.insert(
            node,
            paragraph
                .atoms
                .iter()
                .map(|atom| TextEntry {
                    text: atom.text.clone(),
                    style: atom.style,
                    decorations: atom.decor,
                    link: atom.link.clone(),
                    syntax: None,
                })
                .collect(),
        );

        let mut lines = Vec::new();
        let mut y = Width::ZERO;
        for line in &result.lines {
            lines.push(PlacedLine {
                y,
                hang: 0,
                line: line.clone(),
            });
            y = y + line.height;
        }

        Ok(FlowItem {
            height: y,
            before: self.before_glue(&computed),
            after: self.after_glue(&computed),
            keep_with_next: computed.keep_with_next,
            page_break_before: computed.page_break_before,
            forced_break: false,
            section_start: false,
            footnotes: Vec::new(),
            parts: Vec::new(),
            master: self.current_master,
            table: None,
            blocks: vec![PlacedBlock {
                node,
                kind,
                style: style_id,
                x: slot.x,
                y: Width::ZERO,
                width: slot.width,
                height: y,
                lines,
                marks: Vec::new(),
                line_offset: 0,
                lines_total: 0,
                stage: result.fallback_stage,
                math: None,
                inline_math: Vec::new(),
                ruby: Vec::new(),
            }],
        })
    }

    /// Generate the cover page from the document metadata.
    ///
    /// The template decides that a cover exists (`cover:`) and which master
    /// draws it; the *content* is the document's: `meta.cover` when present,
    /// otherwise `meta.title`, `meta.authors` and `meta.date`. Each line is a
    /// normal paragraph in `coverTitle`/`coverSubtitle`/`coverAuthor`/
    /// `coverDate`, so centering and spacing come from the stylesheet.
    fn cover_rows(&mut self, doc: &Document, items: &mut Vec<FlowItem>) -> Result<(), LayoutError> {
        let spec = self
            .styled
            .cover()
            .cloned()
            .unwrap_or(soma_template::CoverSpec { master: None });
        let master = match spec.master.as_deref() {
            None | Some("default") => self.current_master,
            Some(name) => self
                .styled
                .master_index(name)
                .ok_or_else(|| LayoutError::new(None, "unknown cover master"))?,
        };
        let styles = self.styled.cover_styles();
        let CoverText {
            title,
            subtitle,
            authors,
            date,
            colophon,
        } = cover_lines(doc);

        // The cover is one atomic flow item: a cover that does not fit on one
        // page is an overflow, never a spill onto page two. All spacing is
        // internal — the first block's `space-before` positions the title, it is
        // not page glue.
        let mut blocks = Vec::new();
        let mut y = Width::ZERO;
        for (style, text) in [
            (styles.title, title),
            (styles.subtitle, subtitle),
            (styles.author, authors),
            (styles.date, date),
        ] {
            let Some(text) = text else { continue };
            if text.trim().is_empty() {
                continue;
            }
            let node = self.generated_id();
            let inline = Inline::Text(soma_doc::Text::new(soma_doc::NodeMeta::synthesized(), text));
            let Some(item) = self.paragraph_item(node, BoxKind::Paragraph, &[inline], style)?
            else {
                continue;
            };
            y = y + item.before.width;
            for mut block in item.blocks {
                block.y = y;
                blocks.push(block);
            }
            y = y + item.height;
            y = y + item.after.width;
        }

        // The colophon is its own small block: a hairline, then one line per
        // metadata role. Keeping the project/version, the work and the
        // year/licence on separate lines avoids the uneven CJK/Latin spacing
        // of a single `a · 中文 · b · c` run.
        if let Some(colophon) = colophon {
            let rule_style = styles.colophon_rule;
            let rule = self.computed(rule_style);
            let rule_width = Width(self.content_width.0 * 2 / 5);
            let rule_height = Width::from_em_frac(1, 32);
            y = y + rule.space_before;
            let node = self.generated_id();
            blocks.push(PlacedBlock {
                node,
                kind: BoxKind::Rule,
                style: rule_style,
                x: Width((self.content_width.0 - rule_width.0).max(0) / 2),
                y,
                width: rule_width,
                height: rule_height,
                lines: Vec::new(),
                marks: Vec::new(),
                line_offset: 0,
                lines_total: 0,
                stage: FallbackStage::Normal,
                math: None,
                inline_math: Vec::new(),
                ruby: Vec::new(),
            });
            y = y + rule_height + rule.space_after;

            for line in colophon.lines() {
                let line = line.trim();
                if line.is_empty() {
                    continue;
                }
                let node = self.generated_id();
                let inline =
                    Inline::Text(soma_doc::Text::new(soma_doc::NodeMeta::synthesized(), line));
                let Some(item) =
                    self.paragraph_item(node, BoxKind::Paragraph, &[inline], styles.colophon)?
                else {
                    continue;
                };
                // The rule already owns the block's top spacing; every
                // colophon line sits on the same baseline grid.
                for mut block in item.blocks {
                    block.y = y;
                    blocks.push(block);
                }
                y = y + item.height;
            }
        }

        if blocks.is_empty() {
            return Ok(());
        }

        items.push(FlowItem {
            height: y,
            before: Glue::ZERO,
            after: Glue::ZERO,
            keep_with_next: false,
            page_break_before: false,
            forced_break: false,
            section_start: false,
            footnotes: Vec::new(),
            parts: Vec::new(),
            master,
            table: None,
            blocks,
        });
        // The cover is a page of its own.
        items.push(FlowItem {
            blocks: Vec::new(),
            height: Width::ZERO,
            before: Glue::ZERO,
            after: Glue::ZERO,
            keep_with_next: false,
            page_break_before: false,
            forced_break: true,
            section_start: false,
            footnotes: Vec::new(),
            parts: Vec::new(),
            master,
            table: None,
        });
        Ok(())
    }

    /// A fresh synthetic node id for generated content.
    fn generated_id(&mut self) -> NodeId {
        let id = NodeId(self.generated);
        self.generated += 1;
        id
    }

    /// Heading number for `level`, or the empty string when numbering is off
    /// or the level is deeper than `numbering.depth`.
    fn heading_number(&mut self, level: u8) -> String {
        use soma_template::HeadingNumbering;
        if level == 0 || level > self.numbering.depth {
            return String::new();
        }
        let depth = (level - 1) as usize;
        self.heading_counts[depth] += 1;
        for slot in self.heading_counts.iter_mut().skip(depth + 1) {
            *slot = 0;
        }
        match self.numbering.headings {
            HeadingNumbering::None => String::new(),
            HeadingNumbering::ChineseChapter => match level {
                1 => format!("第{}章", chinese_number(self.heading_counts[0])),
                2 => format!("第{}节", chinese_number(self.heading_counts[1])),
                _ => format!("{}、", chinese_number(self.heading_counts[2])),
            },
            HeadingNumbering::ArabicDotted => {
                let mut out = String::new();
                for (index, count) in self.heading_counts[..=depth].iter().enumerate() {
                    if index > 0 {
                        out.push('.');
                    }
                    out.push_str(&count.to_string());
                }
                out
            }
        }
    }

    /// `pattern` with `{n}` replaced by `count`, or the empty string when the
    /// template does not number this kind of caption.
    fn caption_label(&self, pattern: Option<&str>, count: u32) -> String {
        match pattern {
            Some(pattern) => pattern.replace("{n}", &count.to_string()),
            None => String::new(),
        }
    }

    /// `inlines` with `prefix` prepended as a synthetic text run. Used for
    /// generated headings and caption numbers; an empty prefix is a plain copy.
    fn prefixed_inlines(&mut self, inlines: &[Inline], prefix: &str) -> Vec<Inline> {
        if prefix.is_empty() {
            return inlines.to_vec();
        }
        let mut out = Vec::with_capacity(inlines.len() + 1);
        out.push(Inline::Text(soma_doc::Text::new(
            soma_doc::NodeMeta::synthesized(),
            format!("{prefix} "),
        )));
        out.extend_from_slice(inlines);
        out
    }

    /// Emit the table of contents (and the optional figure/table lists) at the
    /// head of the flow.
    ///
    /// Entries come from the previous layout pass (page numbers are only known
    /// after pagination); `layout_with` iterates until they stop moving. Each
    /// row is one flow item holding two or three boxes: the title, optional dot
    /// leaders, and the page number.
    fn toc_rows(
        &mut self,
        spec: &soma_template::TocSpec,
        front: &Generated,
        items: &mut Vec<FlowItem>,
    ) -> Result<(), LayoutError> {
        let title_style = self.styled.toc_title_style();

        // A template can ask for a TOC unconditionally (the default does).
        // A document without headings, figures or tables has nothing to list:
        // emitting a lone "Contents" page would be worse than no contents.
        let has_figures = spec.figures.is_some() && !front.figures.is_empty();
        let has_tables = spec.tables.is_some() && !front.tables.is_empty();
        if self.toc_entries.is_empty() && !has_figures && !has_tables {
            return Ok(());
        }

        if let Some(title) = spec.title.as_deref().filter(|title| !title.is_empty()) {
            let node = self.generated_id();
            let align = self.computed(title_style).align;
            let item = self.object_item_at(
                node,
                BoxKind::Paragraph,
                title,
                title_style,
                Slot {
                    x: Width::ZERO,
                    width: self.content_width,
                    align,
                },
            )?;
            items.push(item);
        }

        let step = Width::from_em_frac(3, 2);
        let entries = self.toc_entries.clone();
        for entry in &entries {
            let is_chapter = entry.level <= 1;
            let style = self.styled.toc_entry_style(entry.level);
            let indent = if is_chapter { Width::ZERO } else { step };
            self.toc_entry_row(style, &entry.text, &entry.page, indent, spec.leaders, items)?;
        }

        for (list_title, entries) in [
            (spec.figures.as_deref(), &front.figures),
            (spec.tables.as_deref(), &front.tables),
        ] {
            let Some(list_title) = list_title else {
                continue;
            };
            if entries.is_empty() {
                continue;
            }
            if !list_title.is_empty() {
                let node = self.generated_id();
                let list_title_style = self.styled.toc_list_title_style();
                let align = self.computed(list_title_style).align;
                let item = self.object_item_at(
                    node,
                    BoxKind::Paragraph,
                    list_title,
                    list_title_style,
                    Slot {
                        x: Width::ZERO,
                        width: self.content_width,
                        align,
                    },
                )?;
                items.push(item);
            }
            let list_style = self.styled.toc_entry_style(2);
            for entry in entries {
                self.toc_entry_row(
                    list_style,
                    &entry.text,
                    &entry.page,
                    step,
                    spec.leaders,
                    items,
                )?;
            }
        }

        if items.is_empty() {
            return Ok(());
        }

        // The contents end the page they start on.
        items.push(FlowItem {
            blocks: Vec::new(),
            height: Width::ZERO,
            before: Glue::ZERO,
            after: Glue::ZERO,
            keep_with_next: false,
            page_break_before: false,
            forced_break: true,
            section_start: false,
            footnotes: Vec::new(),
            parts: Vec::new(),
            master: self.current_master,
            table: None,
        });
        Ok(())
    }

    /// One contents row: title box, optional leaders, page-number box.
    fn toc_entry_row(
        &mut self,
        entry_style: StyleId,
        text: &str,
        page: &str,
        indent: Width,
        leaders: bool,
        items: &mut Vec<FlowItem>,
    ) -> Result<(), LayoutError> {
        // Reserve four digits so a growing page count never reflows the rows.
        let computed = self.computed(entry_style);
        let gap_glue = self.before_glue(&computed);
        let reserve = self.metrics.advance("0000", entry_style).0.max(Width::EM.0);
        let gap = Width::from_em_frac(1, 2);
        let number_width = self.metrics.advance(page, entry_style);
        let number_x = Width(self.content_width.0 - number_width.0);
        // A page list wider than a third of the measure becomes an ordinary
        // paragraph (`term  pages`) instead of a leader row: there is no room
        // left for a title box, and overlapping boxes are worse than no leaders.
        if number_width.0 * 3 > self.content_width.0 {
            let node = self.generated_id();
            let text = alloc::format!("{text}  {page}");
            let mut row = self.text_item_at(
                node,
                BoxKind::Paragraph,
                &text,
                entry_style,
                Slot {
                    x: indent,
                    width: Width(self.content_width.0 - indent.0),
                    align: Align::Left,
                },
            )?;
            row.before = gap_glue;
            items.push(row);
            return Ok(());
        }
        let reserved = self.content_width.0 - indent.0 - reserve - gap.0;
        let actual = number_x.0 - indent.0 - gap.0;
        let title_width = Width(reserved.min(actual));
        if title_width.0 <= Width::EM.0 {
            return Ok(());
        }

        let title_node = self.generated_id();
        let mut row = self.text_item_at(
            title_node,
            BoxKind::Paragraph,
            text,
            entry_style,
            Slot {
                x: indent,
                width: title_width,
                align: Align::Left,
            },
        )?;

        // The page number and the leaders sit on the title's last line, so a
        // wrapped title keeps its number at the end of its final line.
        let last_line = row
            .blocks
            .iter()
            .rev()
            .find_map(|block| block.lines.last().map(|placed| (block, placed)));
        let line_y = last_line.map(|(_, placed)| placed.y).unwrap_or(Width::ZERO);
        let line_used = last_line
            .map(|(_, placed)| {
                let line = &placed.line;
                line.natural_width.0 - line.trim_left.0 - line.trim_right.0
            })
            .unwrap_or(0)
            .max(0)
            .min(title_width.0);

        if leaders {
            let start = indent.0 + line_used + gap.0 / 2;
            let end = number_x.0 - gap.0 / 2;
            if start < end {
                if let Some(mut leader) = self.leader_row(entry_style, start, end) {
                    for block in &mut leader.blocks {
                        block.y = block.y + line_y;
                    }
                    row.blocks.append(&mut leader.blocks);
                }
            }
        }

        let number_node = self.generated_id();
        let mut number_item = self.object_item_at(
            number_node,
            BoxKind::Paragraph,
            page,
            entry_style,
            Slot {
                x: number_x,
                width: number_width,
                align: Align::Left,
            },
        )?;
        for block in &mut number_item.blocks {
            block.y = block.y + line_y;
        }
        row.height = row.height.max(Width(line_y.0 + number_item.height.0));
        row.blocks.append(&mut number_item.blocks);
        row.before = gap_glue;
        items.push(row);
        Ok(())
    }

    /// A run of leader dots filling `[start, end)`.
    fn leader_row(&mut self, style: StyleId, start: i32, end: i32) -> Option<FlowItem> {
        let dot = self.metrics.advance("·", style).0;
        if dot <= 0 {
            return None;
        }
        let room = end - start;
        let mut count = room / dot;
        let mut text = if count > 0 {
            "·".repeat(count as usize)
        } else {
            String::new()
        };
        // Scaled metrics round the whole run, not each dot, so `count * dot`
        // can overshoot the box by a few units. Shrink until the run fits: a
        // leader box that is one unit wider than its slot is an overfull line.
        while count > 0 && self.metrics.advance(&text, style).0 > room {
            count -= 1;
            text.pop();
        }
        if count <= 0 {
            return None;
        }
        let node = self.generated_id();
        let mut item = self
            .object_item_at(
                node,
                BoxKind::Paragraph,
                &text,
                style,
                Slot {
                    x: Width(start),
                    width: Width(room),
                    align: Align::Left,
                },
            )
            .ok()?;
        item.before = Glue::ZERO;
        item.after = Glue::ZERO;
        Some(item)
    }

    /// Sorted, de-duplicated index entries with their page labels.
    fn index_entries_found(&self, label_of: &dyn Fn(usize) -> String) -> Vec<TocEntry> {
        let collation = match self.styled.index().map(|spec| spec.sort) {
            Some(soma_template::IndexSort::Pinyin) => soma_collate::Collation::Pinyin,
            _ => soma_collate::Collation::Codepoint,
        };
        index::rows(&self.index_terms, label_of, collation)
            .into_iter()
            .filter(|row| !row.text.is_empty())
            .map(|row| TocEntry {
                level: row.level,
                text: row.text,
                page: match row.see {
                    Some((kind, target)) => alloc::format!("{} {}", kind.label(), target),
                    None => index::page_list(&row.pages),
                },
            })
            .collect()
    }

    /// Emit the index at the tail of the flow.
    fn index_rows(
        &mut self,
        spec: &soma_template::IndexSpec,
        items: &mut Vec<FlowItem>,
    ) -> Result<(), LayoutError> {
        if self.index_entries.is_empty() {
            return Ok(());
        }
        let entry_style = self.styled.index_style();
        let title_style = self.styled.index_title_style();
        let first = items.len();
        if let Some(title) = spec.title.as_deref().filter(|title| !title.is_empty()) {
            let node = self.generated_id();
            let title_align = self.computed(title_style).align;
            let item = self.text_item_at(
                node,
                BoxKind::Paragraph,
                title,
                title_style,
                Slot {
                    x: Width::ZERO,
                    width: self.content_width,
                    align: title_align,
                },
            )?;
            let index = items.len();
            items.push(item);
            // The index starts its own page, so that page uses the
            // section-start master: no running head above a title that says the
            // same thing.
            items[index].section_start = true;
            // The index is a navigable destination too: it joins the running
            // title, the contents and the bookmark tree as a level-1 heading.
            self.headings.push(Heading {
                item: index,
                level: 1,
                text: String::from(title),
            });
        }
        let entries = self.index_entries.clone();
        // A sub-entry is set in from its parent by one ideograph of its own
        // size: an index is scanned by silhouette, and a depth proportional to
        // the page would drift as the entry style changes.
        let step = Width(
            self.metrics
                .advance("中", entry_style)
                .0
                .max(Width::EM.0 / 2),
        );
        for entry in &entries {
            let indent = Width(step.0 * entry.level as i32);
            if entry.page.is_empty() {
                // A parent line exists only to hold its children: no page
                // number, and no leader dots running to the margin.
                let node = self.generated_id();
                let mut row = self.text_item_at(
                    node,
                    BoxKind::Paragraph,
                    &entry.text,
                    entry_style,
                    Slot {
                        x: indent,
                        width: Width(self.content_width.0 - indent.0),
                        align: Align::Left,
                    },
                )?;
                row.before = Glue::ZERO;
                items.push(row);
                continue;
            }
            self.toc_entry_row(entry_style, &entry.text, &entry.page, indent, true, items)?;
        }
        if items.len() > first {
            // An index starts on its own page unless it is empty.
            items[first].page_break_before = true;
        }
        Ok(())
    }

    /// Place a display formula centred in the content box.
    ///
    /// Returns `None` when the source does not parse, so the caller can fall
    /// back to a text placeholder.
    fn math_item(&mut self, node: NodeId, source: &str, style_id: StyleId) -> Option<FlowItem> {
        if source.trim_start().starts_with("\\begin{tikzpicture}") {
            return self.tikz_item(node, source, style_id);
        }
        let computed = self.computed(style_id);
        let mut layout = math::display(source, &computed, self.metrics, style_id)?;
        let pattern = self
            .numbering
            .equations
            .clone()
            .filter(|_| is_numbered_environment(source));
        // Each row says what it does about its number: take one, take none, or
        // print the tag the author wrote.
        let lines: Vec<(i32, soma_math::RowNumber)> = if layout.rows.is_empty() {
            alloc::vec![(layout.height.0, layout.number.clone())]
        } else {
            layout
                .rows
                .iter()
                .map(|row| (row.baseline.0, row.number.clone()))
                .collect()
        };
        // A tag is text with `$...$` allowed, which is exactly what `\text{}`
        // does, so it is laid out through the same path. An automatic number is
        // a plain run: digits and parentheses in the math face.
        enum Label {
            Text(String),
            Tag(soma_math::MathLayout),
        }
        let mut labels: Vec<Option<Label>> = Vec::with_capacity(lines.len());
        for (_, number) in &lines {
            let label = match number {
                soma_math::RowNumber::Suppressed => None,
                soma_math::RowNumber::Tag { text, bare } => {
                    let tag = if *bare {
                        alloc::format!("\\text{{{text}}}")
                    } else {
                        alloc::format!("\\text{{({text})}}")
                    };
                    Some(
                        match math::display_inline(&tag, &computed, self.metrics, style_id) {
                            Some(tag) => Label::Tag(tag),
                            None => Label::Text(text.clone()),
                        },
                    )
                }
                soma_math::RowNumber::Auto => pattern.as_ref().map(|pattern| {
                    self.equation_count += 1;
                    Label::Text(pattern.replace("{n}", &self.equation_count.to_string()))
                }),
            };
            labels.push(label);
        }
        // A numbered formula spans the measure so its label can sit on the
        // right margin; an unnumbered one shrinks to the formula.
        let numbered = labels.iter().any(|label| label.is_some());
        let box_width = if numbered {
            self.content_width
        } else {
            layout.width
        };
        let height = layout.total_height();
        let x = Width((self.content_width.0 - layout.width.0).max(0) / 2);
        for ((baseline, _), label) in lines.iter().zip(labels) {
            let Some(label) = label else { continue };
            match label {
                Label::Text(text) => {
                    let advance = self.metrics.advance(&text, style_id).0;
                    layout.items.push(soma_math::MathItem::text(
                        text,
                        // Items are relative to the box origin `x`, so undo the
                        // centering offset to land on the right margin.
                        Width(self.content_width.0 - advance - x.0),
                        Width(*baseline),
                        computed.font_size,
                    ));
                }
                Label::Tag(tag) => {
                    let dx = self.content_width.0 - tag.width.0 - x.0;
                    for mut item in tag.items {
                        item.x = Width(item.x.0 + dx);
                        item.y = Width(item.y.0 + *baseline);
                        layout.items.push(item);
                    }
                    for mut rule in tag.rules {
                        rule.x = Width(rule.x.0 + dx);
                        rule.y = Width(rule.y.0 + *baseline);
                        layout.rules.push(rule);
                    }
                }
            }
        }
        // A short line before a display uses TeX's short display skips, but
        // the template's own glue still matters: keep half of it above and
        // 70% below. TeX's zero short skip made a CJK formula touch the line
        // above; an explicit `space-before: 0` still resolves to zero.
        let mut before = self.before_glue(&computed);
        let mut after = self.after_glue(&computed);
        if self.last_line_short {
            before = Glue {
                width: Width(before.width.0 / 2),
                plus: Width(before.plus.0 / 2),
                minus: Width(before.minus.0 / 2),
            };
            after = Glue {
                width: Width(after.width.0 * 7 / 10),
                plus: Width(after.plus.0 * 7 / 10),
                minus: Width(after.minus.0 * 7 / 10),
            };
        }
        self.last_line_short = false;
        Some(FlowItem {
            height,
            before,
            after,
            keep_with_next: computed.keep_with_next,
            page_break_before: computed.page_break_before,
            forced_break: false,
            section_start: false,
            footnotes: Vec::new(),
            parts: Vec::new(),
            master: self.current_master,
            table: None,
            blocks: vec![PlacedBlock {
                node,
                kind: BoxKind::Math,
                style: style_id,
                x,
                y: Width::ZERO,
                width: box_width,
                height,
                lines: Vec::new(),
                marks: Vec::new(),
                line_offset: 0,
                lines_total: 0,
                stage: FallbackStage::Normal,
                inline_math: Vec::new(),
                ruby: Vec::new(),
                math: Some(MathBox {
                    width: box_width,
                    height,
                    baseline: layout.height,
                    items: layout.items,
                    rules: layout.rules,
                    paths: Vec::new(),
                }),
            }],
        })
    }

    /// A TikZ picture is a display object in the math block's frame: the
    /// picture's own baseline is its bounding-box bottom, and every path and
    /// node label is already in that frame (ADR-0092).
    fn tikz_item(&mut self, node: NodeId, source: &str, style_id: StyleId) -> Option<FlowItem> {
        let computed = self.computed(style_id);
        let root = self.computed(ROOT_STYLE);
        let body = self.styled.body_pt();
        if body.num() <= 0 {
            return None;
        }
        let pt = 65_536i64 * body.den() / body.num();
        let metrics = self.metrics;
        let root_for_text = root.clone();
        let mut measure = |text: &str, scale: i64| {
            let size = Width(root_for_text.font_size.0 * scale as i32 / 1000);
            math::text_at_size(text, &root_for_text, metrics, ROOT_STYLE, size)
        };
        let layout = soma_tikz::layout(source, pt, &mut measure).ok()?;
        let height = layout.height;
        let x = Width((self.content_width.0 - layout.width.0).max(0) / 2);
        Some(FlowItem {
            height,
            before: self.before_glue(&computed),
            after: self.after_glue(&computed),
            keep_with_next: computed.keep_with_next,
            page_break_before: computed.page_break_before,
            forced_break: false,
            section_start: false,
            footnotes: Vec::new(),
            parts: Vec::new(),
            master: self.current_master,
            table: None,
            blocks: vec![PlacedBlock {
                node,
                kind: BoxKind::Math,
                style: style_id,
                x,
                y: Width::ZERO,
                width: layout.width,
                height,
                lines: Vec::new(),
                marks: Vec::new(),
                line_offset: 0,
                lines_total: 0,
                stage: FallbackStage::Normal,
                inline_math: Vec::new(),
                ruby: Vec::new(),
                math: Some(MathBox {
                    width: layout.width,
                    height,
                    baseline: layout.baseline,
                    items: layout.items,
                    rules: layout.rules,
                    paths: layout.paths,
                }),
            }],
        })
    }

    fn diagram_item(
        &mut self,
        node: NodeId,
        dialect: soma_doc::DiagramDialect,
        source: &str,
        style_id: StyleId,
    ) -> Result<Option<FlowItem>, LayoutError> {
        let computed = self.computed(style_id);
        let available = self.content_width;
        // A wide flowchart laid out with a 16 em label cap can need a 60%
        // page scale, which leaves 6 pt labels. Wrapping the labels narrower
        // makes the nodes narrower and the whole diagram fit at a larger
        // scale. Try a few caps and keep the one with the largest final scale
        // (the largest label after scaling), subject to the page height. Ties
        // keep the earlier, less-wrapped layout.
        let mut best: Option<(soma_diagram::DiagramLayout, i32)> = None;
        for max_em in [16, 12, 10, 8] {
            let options = soma_diagram::Options {
                label_size: computed.font_size,
                line_gap: computed.line_height,
                line_width: Width::from_em_frac(1, 25),
                max_label_width: computed.font_size.scale(max_em, 1),
            };
            let mut text = DiagramText::new(&computed, self.metrics, style_id);
            let Ok(layout) = soma_diagram::layout(dialect, source, &options, &mut text) else {
                return self.code_item(node, Some(dialect.source_language()), source, style_id);
            };
            if layout.tikz.width.0 <= 0 || layout.tikz.height.0 <= 0 {
                continue;
            }
            let scale = if layout.tikz.width.0 > available.0 {
                (available.0 as i64 * 1000 / layout.tikz.width.0 as i64) as i32
            } else {
                1000
            };
            if scale < 600 {
                continue;
            }
            let height = layout.tikz.height.scale(scale, 1000);
            if height.0 > self.page_height.0 {
                continue;
            }
            if best
                .as_ref()
                .is_none_or(|(_, best_scale)| scale > *best_scale)
            {
                best = Some((layout, scale));
            }
        }
        let Some((layout, scale)) = best else {
            return self.code_item(node, Some(dialect.source_language()), source, style_id);
        };
        let layout = soma_diagram::scale(&layout, scale);
        let height = layout.tikz.height;
        let x = Width((available.0 - layout.tikz.width.0).max(0) / 2);
        Ok(Some(FlowItem {
            height,
            before: self.before_glue(&computed),
            after: self.after_glue(&computed),
            keep_with_next: computed.keep_with_next,
            page_break_before: computed.page_break_before,
            forced_break: false,
            section_start: false,
            footnotes: Vec::new(),
            parts: Vec::new(),
            master: self.current_master,
            table: None,
            blocks: vec![PlacedBlock {
                node,
                kind: BoxKind::Math,
                style: style_id,
                x,
                y: Width::ZERO,
                width: layout.tikz.width,
                height,
                lines: Vec::new(),
                marks: Vec::new(),
                line_offset: 0,
                lines_total: 0,
                stage: FallbackStage::Normal,
                inline_math: Vec::new(),
                ruby: Vec::new(),
                math: Some(MathBox {
                    width: layout.tikz.width,
                    height,
                    baseline: layout.tikz.baseline,
                    items: layout.tikz.items,
                    rules: layout.tikz.rules,
                    paths: layout.tikz.paths,
                }),
            }],
        }))
    }

    fn figure_item(
        &mut self,
        node: NodeId,
        image: &soma_doc::meta::ImageRef,
        alt: Option<&str>,
        style_id: StyleId,
    ) -> Result<FlowItem, LayoutError> {
        let computed = self.computed(style_id);
        self.text.mark_figure(node, image.src.as_str(), alt);
        let policy = self.styled.figure();
        let mut width = match policy.width {
            soma_template::FigureWidth::Full => self.content_width,
            soma_template::FigureWidth::Half => Width(self.content_width.0 / 2),
            soma_template::FigureWidth::Natural => match image.intrinsic {
                Some(intrinsic) if intrinsic.width_px > 0 => {
                    // CSS pixels are 96 dpi; an em is `body_pt` points.
                    let body = self.styled.body_pt();
                    let units =
                        intrinsic.width_px as i64 * 3 * 4096 * body.den() / (4 * body.num().max(1));
                    Width(units.min(self.content_width.0 as i64) as i32)
                }
                _ => self.content_width,
            },
        };
        width = Width(width.0.min(self.content_width.0).max(1));
        let height = match image.intrinsic {
            Some(intrinsic) if intrinsic.width_px > 0 => {
                width.scale(intrinsic.height_px as i32, intrinsic.width_px as i32)
            }
            _ => width.scale(2, 3),
        };
        let free = self.content_width.0 - width.0;
        let x = match policy.align {
            soma_template::FigureAlign::Left => Width::ZERO,
            soma_template::FigureAlign::Center => Width(free / 2),
            soma_template::FigureAlign::Right => Width(free),
        };
        Ok(FlowItem {
            height,
            before: self.before_glue(&computed),
            after: self.after_glue(&computed),
            keep_with_next: computed.keep_with_next,
            page_break_before: computed.page_break_before,
            forced_break: false,
            section_start: false,
            footnotes: Vec::new(),
            parts: Vec::new(),
            master: self.current_master,
            table: None,
            blocks: vec![PlacedBlock {
                node,
                kind: BoxKind::Figure,
                style: style_id,
                x,
                y: Width::ZERO,
                width,
                height,
                lines: Vec::new(),
                marks: Vec::new(),
                line_offset: 0,
                lines_total: 0,
                math: None,
                inline_math: Vec::new(),
                ruby: Vec::new(),
                stage: FallbackStage::Normal,
            }],
        })
    }

    fn compile_notes(&mut self, doc: &Document) -> Result<(), LayoutError> {
        let mut compiled = Vec::with_capacity(doc.footnotes.len());
        for definition in &doc.footnotes {
            let style = self.style(definition.meta.id, ROOT_STYLE);
            // The note number occupies its own left column; the text is laid
            // out to the right of it, so the number never overlaps the first
            // line and multi-digit numbers still fit.
            let column = footnote_number_column(self.computed(style).font_size);
            let saved_width = self.content_width;
            self.content_width = Width(saved_width.0 - column.0);
            let mut items = Vec::new();
            self.flow_into(&definition.blocks, style, &mut items, false)?;
            self.content_width = saved_width;
            let mut blocks = Vec::new();
            let mut y = Width::ZERO;
            for item in items {
                for mut placed in item.blocks {
                    placed.kind = BoxKind::Footnote;
                    placed.x = placed.x + column;
                    placed.y = y;
                    y = y + placed.height;
                    blocks.push(placed);
                }
            }
            compiled.push(blocks);
        }
        self.notes.blocks = compiled;
        Ok(())
    }
}

/// Shortest word worth hyphenating. `left = 2` and `right = 3` already forbid
/// breaks in a five-letter word, and hyphenating `table` would look worse than
/// the loose line it avoids.
const HYPHEN_MIN_WORD: usize = 6;

/// Split a text run at hyphenation points.
///
/// The first piece keeps the ordinary text kind; every later piece starts with
/// `SpanKind::Hyphen`, which the rules layer turns into a breakable boundary
/// carrying a hyphen penalty. Words that look like part of a URL, a path, or
/// an acronym are left alone: a break inside `example.com` or `HTTP` reads as a
/// typo.
fn push_hyphenated_text(
    text: &str,
    style: StyleId,
    decor: Decorations,
    link: Option<&str>,
    patterns: &soma_hyphen::Patterns,
    out: &mut Vec<OwnedSpan>,
) {
    // Latin runs are found by bytes; every slice below starts or ends on an
    // ASCII letter, so a mixed CJK/Latin span stays on character boundaries.
    let bytes = text.as_bytes();
    let mut cuts: Vec<usize> = Vec::new();
    let mut cursor = 0;
    while cursor < bytes.len() {
        if !bytes[cursor].is_ascii_alphabetic() {
            cursor += 1;
            continue;
        }
        let start = cursor;
        while cursor < bytes.len() && bytes[cursor].is_ascii_alphabetic() {
            cursor += 1;
        }
        let word = &text[start..cursor];
        if word.len() < HYPHEN_MIN_WORD || word.bytes().any(|byte| byte.is_ascii_uppercase()) {
            continue;
        }
        let before = start.checked_sub(1).map(|index| bytes[index]);
        let after = bytes.get(cursor).copied();
        let glued = |byte: u8| matches!(byte, b'/' | b':' | b'.' | b'@' | b'_' | b'-');
        if before.is_some_and(glued) || after.is_some_and(glued) {
            continue;
        }
        for offset in patterns.breaks(word) {
            let at = start + offset as usize;
            if at > start && at < cursor {
                cuts.push(at);
            }
        }
    }
    if !cuts.is_empty() {
        let mut cursor = 0;
        let mut kind = SpanKind::Text;
        for cut in cuts {
            out.push(OwnedSpan {
                text: String::from(&text[cursor..cut]),
                style,
                kind,
                decor,
                link: link.map(String::from),
                width: Width::ZERO,
                penalty: 0,
            });
            cursor = cut;
            kind = SpanKind::Hyphen;
        }
        out.push(OwnedSpan {
            text: String::from(&text[cursor..]),
            style,
            kind,
            decor,
            link: link.map(String::from),
            width: Width::ZERO,
            penalty: 0,
        });
        return;
    }
    out.push(OwnedSpan {
        text: String::from(text),
        style,
        kind: SpanKind::Text,
        decor,
        link: link.map(String::from),
        width: Width::ZERO,
        penalty: 0,
    });
}

/// Index markers inside each footnote definition, in definition order.
///
/// A footnote is printed on the page that refers to it, so a marker in a note
/// belongs to the referring item; `flow_into` looks the terms up by note index.
fn note_index_terms(doc: &Document) -> Vec<Vec<index::IndexTerm>> {
    doc.footnotes
        .iter()
        .map(|definition| {
            let mut terms = Vec::new();
            for block in &definition.blocks {
                block_index_terms(block, &mut terms);
            }
            terms
        })
        .collect()
}

fn block_index_terms(block: &Block, terms: &mut Vec<index::IndexTerm>) {
    match block {
        Block::Paragraph(node) => inline_index_terms(&node.inlines, terms),
        Block::Heading(node) => inline_index_terms(&node.inlines, terms),
        Block::List(node) => {
            for item in &node.items {
                for block in &item.blocks {
                    block_index_terms(block, terms);
                }
            }
        }
        Block::BlockQuote(node) => {
            for block in &node.blocks {
                block_index_terms(block, terms);
            }
        }
        Block::Figure(node) => inline_index_terms(&node.caption, terms),
        Block::Table(node) => {
            inline_index_terms(&node.caption, terms);
            for row in &node.rows {
                for cell in &row.cells {
                    for block in &cell.blocks {
                        block_index_terms(block, terms);
                    }
                }
            }
        }
        Block::Generated(node) => {
            for block in &node.blocks {
                block_index_terms(block, terms);
            }
        }
        Block::CodeBlock(_)
        | Block::MathBlock(_)
        | Block::Diagram(_)
        | Block::ThematicBreak(_)
        | Block::PageBreak(_)
        | Block::Section(_)
        | Block::Raw(_) => {}
    }
}

fn inline_index_terms(inlines: &[Inline], terms: &mut Vec<index::IndexTerm>) {
    for inline in inlines {
        match inline {
            Inline::Index(node) => terms.push(index::parse(&node.term)),
            Inline::Strong(node)
            | Inline::Emphasis(node)
            | Inline::Underline(node)
            | Inline::Strikethrough(node)
            | Inline::SmallCaps(node)
            | Inline::Superscript(node)
            | Inline::Subscript(node) => inline_index_terms(&node.children, terms),
            Inline::Link(node) => inline_index_terms(&node.children, terms),
            _ => {}
        }
    }
}

/// Atoms that render as a footnote number rather than their text.
fn marker_positions(atoms: &[soma_core::Atom], lines: &[Line]) -> Vec<Mark> {
    let mut marks = Vec::new();
    let mut marker = [0u8; 4];
    let text = FOOTNOTE_MARKER.encode_utf8(&mut marker);
    for (index, atom) in atoms.iter().enumerate() {
        if atom.text.as_str() == &*text {
            let line = lines
                .iter()
                .position(|line| index >= line.start as usize && index < line.end as usize)
                .unwrap_or(0);
            marks.push(Mark {
                atom: index as u32,
                number: 0,
                line: line as u32,
                section: 0,
                note: None,
            });
        }
    }
    marks
}

/// Plain text of an inline sequence, used for running headers.
fn inline_text(inlines: &[Inline]) -> String {
    let mut out = String::new();
    for inline in inlines {
        match inline {
            Inline::Text(node) => out.push_str(&node.value),
            Inline::Code(node) => out.push_str(&node.value),
            Inline::Math(node) => out.push_str(&node.source),
            Inline::Ruby(node) => out.push_str(&node.base),
            Inline::Image(node) => {
                if let Some(alt) = &node.source.alt {
                    out.push_str(alt);
                }
            }
            Inline::Raw(node) => out.push_str(&node.content.fallback),
            Inline::LineBreak(_) | Inline::FootnoteRef(_) | Inline::Index(_) => {}
            other => out.push_str(&inline_text(other.children())),
        }
    }
    out
}

/// The only glyph a truncated header/footer appends (D64).
const MARGIN_ELLIPSIS: &str = "…";

/// Paragraph shape for a margin-band line. The width is deliberately far
/// wider than any page: measuring a candidate this way keeps the whole string
/// on the first line, so its natural width is the string's width.
fn margin_paragraph_style(line_height: Width) -> ParagraphStyle {
    ParagraphStyle {
        width: Width::from_em(1000),
        first_indent: Width::ZERO,
        hang: Width::ZERO,
        tracking: Width::ZERO,
        line_height,
        align: soma_core::Align::Justify,
    }
}

/// Natural width of `text` as one margin-band line.
///
/// Headers and footers do not wrap, so the first line is the whole candidate.
/// Using the same breaker as the placed block means the truncation decision
/// and the final line agree on what "measured width" is, including boundary
/// glue and trim rules.
fn margin_text_width(
    metrics: &dyn Metrics,
    engine: &BreakConfig,
    text: &str,
    style: StyleId,
    line_height: Width,
) -> Option<Width> {
    let paragraph = paragraph_from_text(text, margin_paragraph_style(line_height), style).ok()?;
    let result = break_paragraph(&paragraph, metrics, engine).ok()?;
    result.lines.first().map(|line| line.natural_width)
}

/// Fit one margin-band string into the band's measure (D64).
///
/// Strategy: truncation, not wrapping. Keep the longest character prefix whose
/// measured width plus `…` still fits, trim trailing spaces before the
/// ellipsis, and return `None` when even the ellipsis is wider than the
/// measure. `None` makes the caller omit the band text: an empty band is
/// better than text that enters the content column.
fn fit_margin_text(
    metrics: &dyn Metrics,
    engine: &BreakConfig,
    text: &str,
    style: StyleId,
    line_height: Width,
    measure: Width,
) -> Option<String> {
    let width = margin_text_width(metrics, engine, text, style, line_height)?;
    if width <= measure {
        return Some(String::from(text));
    }
    if margin_text_width(metrics, engine, MARGIN_ELLIPSIS, style, line_height)? > measure {
        return None;
    }

    // Character boundaries, plus the end of the string. Binary search for the
    // largest prefix that fits; trimming trailing whitespace keeps the width
    // monotone (a trimmed prefix can only stay equal or grow).
    let mut ends: Vec<usize> = text.char_indices().map(|(index, _)| index).collect();
    ends.push(text.len());
    let mut low = 0usize;
    let mut high = ends.len() - 1;
    while low < high {
        let middle = (low + high).div_ceil(2);
        let mut candidate = String::from(text[..ends[middle]].trim_end());
        candidate.push_str(MARGIN_ELLIPSIS);
        let fits = margin_text_width(metrics, engine, &candidate, style, line_height)
            .is_some_and(|width| width <= measure);
        if fits {
            low = middle;
        } else {
            high = middle - 1;
        }
    }

    let mut out = String::from(text[..ends[low]].trim_end());
    out.push_str(MARGIN_ELLIPSIS);
    Some(out)
}

/// Build a header/footer line: one centered line, outside the content box.
///
/// The band lives in the page margin, so it never competes with the content
/// height and cannot change pagination. A band cannot wrap either: wrapping
/// would push a second line into the first row of the content box. Long text
/// is therefore truncated at a character boundary with an explicit `…`
/// (D64), and the kept prefix is measured with the same breaker that places
/// the final line.
#[allow(clippy::too_many_arguments)]
fn margin_block(
    builder: &mut Builder,
    node: NodeId,
    text: &str,
    style: StyleId,
    content_width: Width,
    band_height: Width,
    y: Width,
    bottom_aligned: bool,
) -> Option<PlacedBlock> {
    if text.is_empty() {
        return None;
    }
    // The band's height is carried by the block, not by the line: a footer
    // line is placed at the bottom of its band and a header at the top, both
    // with their natural line box. `line_height` still has to be positive —
    // DocIR rejects zero — and the natural line box is the honest value: it
    // also becomes the body height of the K3 height cost, which a folio should
    // not perturb.
    let line_height = builder.metrics.line_metrics(style).height();
    let paragraph_style = margin_paragraph_style(line_height);
    let mut text = String::from(text);
    let mut paragraph = paragraph_from_text(&text, paragraph_style, style).ok()?;
    let mut result = break_paragraph(&paragraph, builder.metrics, builder.engine).ok()?;
    if result
        .lines
        .first()
        .is_none_or(|line| line.natural_width > content_width)
    {
        text = fit_margin_text(
            builder.metrics,
            builder.engine,
            &text,
            style,
            line_height,
            content_width,
        )?;
        paragraph = paragraph_from_text(&text, paragraph_style, style).ok()?;
        result = break_paragraph(&paragraph, builder.metrics, builder.engine).ok()?;
    }
    let stage = result.fallback_stage;
    let line = result.lines.into_iter().next()?;
    let natural = line.natural_width;
    if natural > content_width {
        // `fit_margin_text` measured this exact string. Keep the guard anyway:
        // a future measurement change must not let the band enter the content
        // column. Printing nothing is safer than overlapping the first line.
        return None;
    }
    let computed = builder.computed(style);
    let x = match computed.align {
        Align::Left => Width::ZERO,
        // A single margin line has nothing to justify. Templates that predate
        // the running-head style inherit `justify`; keep their centred look.
        Align::Center | Align::Justify => Width((content_width.0 - natural.0).max(0) / 2),
        Align::Right => Width((content_width.0 - natural.0).max(0)),
    };

    builder.text.insert(
        node,
        paragraph
            .atoms
            .iter()
            .map(|atom| TextEntry {
                text: atom.text.clone(),
                style: atom.style,
                decorations: atom.decor,
                link: atom.link.clone(),
                syntax: None,
            })
            .collect(),
    );

    Some(PlacedBlock {
        node,
        kind: BoxKind::Paragraph,
        style,
        x,
        y,
        width: natural,
        height: band_height,
        lines: vec![PlacedLine {
            // A footer's band is the distance from the text block to the
            // folio, so its line sits at the bottom of the band; a header
            // hangs from the top of its band, in the top margin.
            y: if bottom_aligned {
                Width((band_height.0 - line.height.0).max(0))
            } else {
                Width::ZERO
            },
            hang: 0,
            line,
        }],
        marks: Vec::new(),
        line_offset: 0,
        lines_total: 1,
        stage,
        math: None,
        inline_math: Vec::new(),
        ruby: Vec::new(),
    })
}

/// Round a line advance up to a whole number of baseline-grid steps.
fn snap_to_grid(height: Width, grid: Width) -> Width {
    if grid.0 <= 0 || height.0 <= 0 {
        return height;
    }
    let steps = (height.0 + grid.0 - 1) / grid.0;
    Width(steps.max(1) * grid.0)
}

/// Count the leading pages that hold only generated items (cover + contents).
fn front_page_count(ends: &[usize], vertical: &[VItem], front_items: usize) -> usize {
    if front_items == 0 {
        return 0;
    }
    let mut start = 0usize;
    let mut pages = 0usize;
    for &end in ends {
        let only_front = vertical[start..end]
            .iter()
            .all(|item| item.item < front_items);
        if !only_front {
            break;
        }
        pages += 1;
        start = end;
    }
    pages
}

/// Printed label per page. Front matter may be unnumbered or use lowercase
/// roman numerals; the body always restarts at 1 unless numbering is
/// continuous.
fn page_labels(
    total: usize,
    front: usize,
    mode: soma_template::FrontMatterNumbering,
) -> Vec<String> {
    use soma_template::FrontMatterNumbering;
    let front = front.min(total);
    let body = total.saturating_sub(front);
    let mut labels = Vec::with_capacity(total);
    match mode {
        FrontMatterNumbering::Continuous => {
            for page in 1..=total {
                labels.push(page.to_string());
            }
        }
        FrontMatterNumbering::Roman => {
            for page in 1..=front {
                labels.push(roman(page));
            }
            for page in 1..=body {
                labels.push(page.to_string());
            }
        }
        FrontMatterNumbering::Hidden => {
            for _ in 0..front {
                labels.push(String::new());
            }
            for page in 1..=body {
                labels.push(page.to_string());
            }
        }
    }
    labels
}

/// Lowercase Roman numeral. Values outside `1..=3999` fall back to digits.
fn roman(value: usize) -> String {
    if value == 0 || value > 3999 {
        return value.to_string();
    }
    const TABLE: [(usize, &str); 13] = [
        (1000, "m"),
        (900, "cm"),
        (500, "d"),
        (400, "cd"),
        (100, "c"),
        (90, "xc"),
        (50, "l"),
        (40, "xl"),
        (10, "x"),
        (9, "ix"),
        (5, "v"),
        (4, "iv"),
        (1, "i"),
    ];
    let mut remaining = value;
    let mut out = String::new();
    for (amount, sign) in TABLE {
        while remaining >= amount {
            out.push_str(sign);
            remaining -= amount;
        }
    }
    out
}

/// Chinese numeral for `1..=99`, the range a chapter or section count needs.
/// Larger values fall back to Western digits.
fn chinese_number(value: u32) -> String {
    const DIGITS: [&str; 10] = ["零", "一", "二", "三", "四", "五", "六", "七", "八", "九"];
    if value == 0 || value > 99 {
        return value.to_string();
    }
    let tens = value / 10;
    let ones = value % 10;
    let mut out = String::new();
    if tens > 0 {
        if tens > 1 {
            out.push_str(DIGITS[tens as usize]);
        }
        out.push('十');
    }
    if ones > 0 {
        out.push_str(DIGITS[ones as usize]);
    }
    out
}

/// Scale the rules layer's base-em trim caps to each atom's own font size.
/// The kernel only knows Justify and Left. Centering and right-alignment are
/// applied to the finished lines, so the kernel sees a left-aligned paragraph
/// (no stretch, no trim redistribution).
fn kernel_align(align: Align) -> Align {
    match align {
        Align::Center | Align::Right => Align::Left,
        other => other,
    }
}

/// Shift every line of a centered or right-aligned paragraph.
///
/// `Line.target_width` excludes the first-line indent and `Line.indent` is the
/// line's own left offset, so the full line box is `target + indent`; a centered
/// line starts at `(full - natural) / 2`, a right-aligned one at
/// `full - natural`. Overfull lines stay where they are and overflow instead.
fn align_lines(lines: &mut [Line], align: Align) {
    if matches!(align, Align::Justify | Align::Left) {
        return;
    }
    for line in lines {
        // The kernel supports Justify/Left only, so centered and right-aligned
        // paragraphs are laid out through the Left path. A leading opening
        // mark is optically trimmed there; a centered line has no common left
        // text edge to align to, so undo that shift before centering. A line
        // that was genuinely overfull keeps its trim.
        if line.natural_width <= line.target_width && line.trim_left.0 > 0 {
            let trim = line.trim_left.0;
            for atom in &mut line.boxes {
                atom.x = Width(atom.x.0 + trim);
            }
            line.trim_left = Width::ZERO;
        }
        let full = line.target_width.0 + line.indent.0;
        let slack = full - line.natural_width.0;
        if slack <= 0 {
            continue;
        }
        let start = match align {
            Align::Center => slack / 2,
            Align::Right => slack,
            _ => 0,
        };
        let shift = start - line.indent.0;
        if shift == 0 {
            continue;
        }
        for atom in &mut line.boxes {
            atom.x = Width(atom.x.0 + shift);
        }
    }
}

fn scale_trim(paragraph: &mut Paragraph, metrics: &dyn Metrics, size_of: impl Fn(StyleId) -> i32) {
    for atom in &mut paragraph.atoms {
        let size = size_of(atom.style);
        if size > 0 && size != 4096 {
            atom.compress_left = Width(scale_units(atom.compress_left.0, size));
            atom.compress_right = Width(scale_units(atom.compress_right.0, size));
        }
        // A real outline knows where the ink ends, for every glyph: a line can
        // end on a Han character or on a fallback face's glyph just as easily
        // as on a mark (ADR-0038). A table without outline data keeps the rule
        // table's constants.
        // A gap can be negative: an italic or a full-width mark may hang its
        // ink past the advance box. A trim budget cannot be (the IR validates
        // it), and trimming into the glyph is never wanted, so clamp at zero.
        if let Some(ink) = metrics.ink_left(&atom.text, atom.style) {
            atom.compress_left = Width(ink.0.max(0));
        }
        if let Some(ink) = metrics.ink_right(&atom.text, atom.style) {
            atom.compress_right = Width(ink.0.max(0));
        }
    }
}

fn scale_units(value: i32, size: i32) -> i32 {
    (value as i64 * size as i64 / 4096) as i32
}

/// Put the text after a list marker on the measured marker column.
///
/// Justification spreads `adjustment` over the line's boundaries. The marker
/// gap is a prefix, not part of the justified text column: even with its
/// stretch and shrink zeroed, `distribute` still shares the remainder, so the
/// first text run can drift a few units away from the wrap column. Shift the
/// content box and everything after it by the same delta, so the first text
/// glyph and every wrapped line start at the same absolute x.
fn pin_marker_gap(line: &mut Line, content: u32, marker_width: Width) {
    let Some(content_index) = line.boxes.iter().position(|box_| box_.atom == content) else {
        return;
    };
    // The marker is a column, not text. A justified first line trims the
    // leading bearing (ADR-0082/0105), which moves the marker but not the
    // ragged items in the same list. Undo that trim on the prefix so every
    // item puts its marker at the same x and the gap keeps its natural width.
    let trim = line.trim_left.0;
    if trim != 0 {
        for box_ in line.boxes.iter_mut().take(content_index) {
            box_.x = Width(box_.x.0 + trim);
        }
    }
    let target = line.indent.0 + marker_width.0;
    let delta = target - line.boxes[content_index].x.0;
    if delta == 0 {
        return;
    }
    for box_ in line.boxes.iter_mut().skip(content_index) {
        box_.x = Width(box_.x.0 + delta);
    }
}

/// How far the final punctuation hangs into the margin. Chinese closing
/// punctuation carries half an em of side bearing on its right; moving the
/// glyph — not the em box — to the text edge is what makes a justified line
/// look flush (ADR-0036).
fn hanging_amount(atom: &soma_core::Atom, cap: Width) -> i32 {
    // Hanging pushes the whole em box past the text edge. Trimming already
    // spends the side bearing, so the two are independent styles; a template
    // picks one, not both.
    let limit = cap.0.min(atom.compress_right.0).max(0);
    match atom.class {
        PunctClass::Close | PunctClass::Middle => limit,
        PunctClass::Dash | PunctClass::Open | PunctClass::None => 0,
    }
}

fn class_at(spans: &[soma_highlight::ClassSpan], at: usize) -> Option<SyntaxClass> {
    spans
        .iter()
        .find(|span| span.start <= at && at < span.end)
        .map(|span| span.class)
}

fn empty_line(line_height: Width, y: Width) -> PlacedLine {
    PlacedLine {
        y,
        hang: 0,
        line: Line {
            start: 0,
            end: 0,
            indent: Width::ZERO,
            target_width: Width::ZERO,
            natural_width: Width::ZERO,
            trim_left: Width::ZERO,
            trim_right: Width::ZERO,
            ratio_milli: 0,
            ascent: line_height,
            descent: Width::ZERO,
            leading: Width::ZERO,
            height: line_height,
            adjustment: 0,
            boundary_deviation: 0,
            cjk_latin_adjustments: Vec::new(),
            cost: Cost(0),
            overfull: false,
            emergency: false,
            forced_end: false,
            hyphenated: false,
            boxes: Vec::new(),
        },
    }
}

// ---------------------------------------------------------------------------
// Pagination
// ---------------------------------------------------------------------------

/// Footnote numbering policy from the template.
struct Numbering {
    policy: soma_template::FootnoteNumbering,
    next: u32,
    /// Section ordinal currently owning the counter. Only `Section` policy
    /// reads it; the other policies keep one counter as before.
    section: u32,
}

impl Numbering {
    fn new(policy: soma_template::FootnoteNumbering) -> Self {
        Self {
            policy,
            next: 1,
            section: 0,
        }
    }

    fn assign(&mut self, pages: &mut [Page]) {
        for page in pages.iter_mut() {
            if matches!(self.policy, soma_template::FootnoteNumbering::Page) {
                self.next = 1;
            }
            // A mark carries the section it was compiled in. Restarting on a
            // section change makes the counter follow `Block::Section` even
            // when two sections share one page (D56).
            let mut mark_numbers: Vec<(Option<usize>, u32)> = Vec::new();
            for block in page.blocks.iter_mut() {
                for mark in block.marks.iter_mut() {
                    if matches!(self.policy, soma_template::FootnoteNumbering::Section)
                        && mark.section != self.section
                    {
                        self.section = mark.section;
                        self.next = 1;
                    }
                    mark.number = self.next;
                    mark_numbers.push((mark.note, self.next));
                    self.next += 1;
                }
            }

            // The note area is assembled in first-reference order. A note's
            // number comes from the mark that points at it, not from its
            // position in that list: repeated references or a section restart
            // must not shift the printed note number away from its label.
            // A note with no visible marker gets the next free number.
            let mut notes: Vec<usize> = Vec::new();
            for note in &page.footnote_notes {
                if !notes.contains(note) {
                    notes.push(*note);
                }
            }
            let mut note_numbers: Vec<(usize, u32)> = Vec::with_capacity(notes.len());
            for note in notes {
                let number = match mark_numbers
                    .iter()
                    .find(|(mark_note, _)| *mark_note == Some(note))
                {
                    Some((_, number)) => *number,
                    None => {
                        let number = self.next;
                        self.next += 1;
                        number
                    }
                };
                note_numbers.push((note, number));
            }
            let mut printed: Vec<usize> = Vec::new();
            page.footnote_numbers = page
                .footnote_notes
                .iter()
                .map(|note| {
                    let number = note_numbers
                        .iter()
                        .find(|(candidate, _)| candidate == note)
                        .map(|(_, number)| *number);
                    if printed.contains(note) {
                        None
                    } else {
                        printed.push(*note);
                        number
                    }
                })
                .collect();
        }
    }
}

/// The four cover lines, taken from the document metadata.
struct CoverText {
    title: Option<String>,
    subtitle: Option<String>,
    authors: Option<String>,
    date: Option<String>,
    colophon: Option<String>,
}

fn cover_lines(doc: &Document) -> CoverText {
    let meta = &doc.meta;
    let cover = meta.cover.as_ref();
    let zh = meta
        .language
        .as_deref()
        .map(|tag| tag.starts_with("zh"))
        .unwrap_or(false);

    let title = cover
        .and_then(|cover| cover.title.clone())
        .or_else(|| meta.title.as_deref().map(inline_text))
        .filter(|text| !text.is_empty());
    let subtitle = cover
        .and_then(|cover| cover.subtitle.clone())
        .filter(|text| !text.is_empty());

    let mut names: Vec<String> = match cover {
        Some(cover) if !cover.authors.is_empty() => cover.authors.clone(),
        _ => meta
            .authors
            .iter()
            .map(|author| author.name.clone())
            .collect(),
    };
    names.retain(|name| !name.is_empty());
    let authors = if names.is_empty() {
        None
    } else {
        let separator = if zh { "、" } else { ", " };
        Some(names.join(separator))
    };

    let date = match cover.and_then(|cover| cover.date.clone()) {
        Some(date) => Some(date),
        None => meta
            .date
            .map(|date| format!("{}-{:02}-{:02}", date.year, date.month, date.day)),
    };
    // A colophon is a cover field, not a document property: the cover
    // template decides whether to show it.
    let colophon = cover
        .and_then(|cover| {
            cover
                .fields
                .iter()
                .find(|field| field.key == "colophon")
                .map(|field| field.value.clone())
        })
        .filter(|text| !text.is_empty());
    CoverText {
        title,
        subtitle,
        authors,
        date,
        colophon,
    }
}

/// Environments that take an equation number. `aligned`/`split` are inner
/// blocks and never numbered; the `*` variants opt out at the source.
fn is_numbered_environment(source: &str) -> bool {
    ["align", "equation", "gather", "multline"]
        .iter()
        .any(|name| source.contains(&format!("\\begin{{{name}}}")))
}

#[cfg(test)]
mod unit {
    use super::*;
    use soma_core::SyntheticMetrics;
    use soma_rules::justified_style;

    #[test]
    fn negative_ink_never_becomes_a_negative_trim() {
        // A mark can hang its ink past the advance box (`advance - x_max` is
        // then negative). `compress_*` is validated as non-negative, so a table
        // with such a mark must not make the paragraph unbreakable.
        struct Overhang;
        impl soma_core::Metrics for Overhang {
            fn advance(&self, _text: &str, _style: soma_core::StyleId) -> Width {
                Width::EM
            }
            fn line_metrics(&self, _style: soma_core::StyleId) -> soma_core::LineMetrics {
                soma_core::LineMetrics {
                    ascent: Width::EM,
                    descent: Width::ZERO,
                    leading: Width::ZERO,
                }
            }
            fn ink_left(&self, _text: &str, _style: soma_core::StyleId) -> Option<Width> {
                Some(Width(-120))
            }
            fn ink_right(&self, _text: &str, _style: soma_core::StyleId) -> Option<Width> {
                Some(Width(-80))
            }
        }

        let mut paragraph = paragraph_from_text(
            "中文，排版。",
            justified_style(Width::from_em(4)),
            StyleId(0),
        )
        .expect("paragraph");
        let punctuation = paragraph
            .atoms
            .iter()
            .position(|atom| atom.text == "，")
            .expect("the comma is an atom");
        assert!(paragraph.atoms[punctuation].compress_right.0 > 0);

        scale_trim(&mut paragraph, &Overhang, |_| 4096);

        for atom in &paragraph.atoms {
            assert!(atom.compress_left.0 >= 0, "{:?}", atom.text);
            assert!(atom.compress_right.0 >= 0, "{:?}", atom.text);
        }
        assert_eq!(paragraph.atoms[punctuation].compress_right, Width::ZERO);
        assert_eq!(paragraph.validate(), Ok(()));
    }

    #[test]
    fn columns_share_the_measure_by_what_they_hold() {
        // Equal columns stay equal, and the widths still sum to the measure.
        let equal = column_widths(&[100, 100, 100], &[], 9000);
        assert_eq!(equal, [3000, 3000, 3000]);

        // One long column takes the room it needs.
        let uneven = column_widths(&[100, 900], &[], 10000);
        assert_eq!(uneven.iter().sum::<i32>(), 10000);
        assert!(uneven[1] > uneven[0] * 4, "the long column gets the room");

        // Rounding never loses or invents a unit.
        for measure in [9999, 10000, 10001] {
            let widths = column_widths(&[37, 41, 43, 47], &[], measure);
            assert_eq!(widths.iter().sum::<i32>(), measure, "measure {measure}");
        }

        // A column with nothing in it still gets a floor.
        let empty = column_widths(&[0, 5000], &[], 9000);
        assert!(empty[0] > 0, "{empty:?}");
    }

    #[test]
    fn a_hinted_column_gets_its_share_and_the_rest_is_measured() {
        // 700‰ + auto: the hint is absolute, the auto column takes the rest.
        let widths = column_widths(&[100, 100], &[700, 0], 10000);
        assert_eq!(widths.iter().sum::<i32>(), 10000);
        assert!(widths[0] > 6500 && widths[0] < 7500, "{widths:?}");

        // Every column hinted: the shares are the split.
        let split = column_widths(&[100, 100], &[600, 400], 10000);
        assert_eq!(split.iter().sum::<i32>(), 10000);
        assert!(split[0] >= split[1] * 3 / 2, "{split:?}");

        // Hints over 1000‰ are scaled down, not allowed to overflow.
        let clamped = column_widths(&[100, 100], &[900, 900], 10000);
        assert_eq!(clamped.iter().sum::<i32>(), 10000);
    }

    #[test]
    fn trim_caps_scale_with_the_atom_font_size() {
        let style = justified_style(Width::from_em(20));
        let mut paragraph = paragraph_from_text("（字）。", style, StyleId(1)).expect("paragraph");
        let open = paragraph.atoms[0].compress_left.0;
        let close = paragraph.atoms[2].compress_right.0;
        assert!(open > 0 && close > 0);

        scale_trim(&mut paragraph, &SyntheticMetrics::default(), |_| 8192);
        assert_eq!(paragraph.atoms[0].compress_left.0, open * 2);
        assert_eq!(paragraph.atoms[2].compress_right.0, close * 2);
    }

    #[test]
    fn trim_is_unchanged_at_the_body_size() {
        let style = justified_style(Width::from_em(20));
        let mut paragraph = paragraph_from_text("（字）。", style, StyleId(1)).expect("paragraph");
        let before = paragraph.atoms.clone();
        scale_trim(&mut paragraph, &SyntheticMetrics::default(), |_| 4096);
        for (atom, old) in paragraph.atoms.iter().zip(before.iter()) {
            assert_eq!(atom.compress_left, old.compress_left);
            assert_eq!(atom.compress_right, old.compress_right);
        }
    }
}
