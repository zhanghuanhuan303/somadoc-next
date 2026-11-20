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
pub mod math;
pub mod metrics;
mod pagination;

pub use metrics::{StyledMetrics, TableMetrics};

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

use soma_core::{
    break_paragraph, Align, BreakConfig, Cost, FallbackStage, Line, Metrics, Paragraph,
    ParagraphStyle, PunctClass, StyleId, Width,
};
use soma_doc::{Block, Document, Inline, NodeId};
use soma_rules::{paragraph_from_spans, paragraph_from_text, InlineSpan, RuleError, SpanKind};
use soma_template::{ComputedStyle, ResolvedPage, StyledDoc};

pub const SCHEMA: &str = "soma.pages/9";

/// Placeholder atom for a footnote marker. Renderers replace the marked atom
/// with [`Mark::number`].
pub const FOOTNOTE_MARKER: char = '\u{e000}';
/// The marker as a string, for metrics adapters that must recognise it.
pub const FOOTNOTE_MARKER_TEXT: &str = "\u{e000}";

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

pub use soma_core::VerticalParams;

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
    /// Index of the first line in this page slice. Non-zero means the block is
    /// continued from the previous page.
    pub line_offset: u32,
    /// Total number of lines in the unsplit block.
    pub lines_total: u32,
    pub stage: FallbackStage,
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

    /// True when the atom is drawn by a rescue face rather than its style's
    /// own. Layout measured it with the same rule, so the renderer and the
    /// breaker agree (docs/mixed-font.md).
    fn uses_fallback(&self, _block: NodeId, _atom: u32) -> bool {
        false
    }
}

/// Owned atom text per DocIR block. Built while laying out, so the renderer
/// does not recompile anything.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TextIndex {
    entries: Vec<(NodeId, Vec<(String, StyleId)>)>,
    /// Atoms drawn by a rescue face, `(node, atom)`, sorted at `finish`.
    fallbacks: Vec<(u32, u32)>,
    /// Highest node id inserted so far. Tracked explicitly: `entries` is only
    /// sorted at `finish`, so the last element is not the maximum while blocks
    /// are still being added (the TOC adds ids above the document's).
    max: u32,
}

impl TextIndex {
    pub fn blocks(&self) -> &[(NodeId, Vec<(String, StyleId)>)] {
        &self.entries
    }

    fn insert(&mut self, node: NodeId, atoms: Vec<(String, StyleId)>) {
        self.max = self.max.max(node.0);
        self.entries.push((node, atoms));
    }

    fn mark_fallback(&mut self, node: NodeId, atom: u32) {
        self.fallbacks.push((node.0, atom));
    }

    fn finish(&mut self) {
        self.entries.sort_by_key(|(node, _)| *node);
        self.fallbacks.sort_unstable();
        self.fallbacks.dedup();
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
                .map(|(text, _)| text.as_str())
                .unwrap_or(""),
            Err(_) => "",
        }
    }

    fn atom_style(&self, block: NodeId, atom: u32) -> Option<StyleId> {
        match self.entries.binary_search_by_key(&block, |(node, _)| *node) {
            Ok(index) => self.entries[index]
                .1
                .get(atom as usize)
                .map(|(_, style)| *style),
            Err(_) => None,
        }
    }

    fn uses_fallback(&self, block: NodeId, atom: u32) -> bool {
        self.fallbacks.binary_search(&(block.0, atom)).is_ok()
    }
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
}

/// Lay out with an explicit strategy and vertical cost model.
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
    vertical: VerticalParams,
) -> Result<PagedLayout, LayoutError> {
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
        content_width,
        text: TextIndex::default(),
        notes: NoteMap::build(doc),
        headings: Vec::new(),
        tables: Vec::new(),
        current_master: 0,
        generated: generated_base,
        section_start_pending: false,
        toc_depth: toc.map(|spec| spec.depth).unwrap_or(0),
        toc_entries: front.headings.clone(),
        numbering: styled.numbering().clone(),
        heading_counts: [0; 3],
        figure_count: 0,
        table_count: 0,
        front_matter_items: 0,
        pending_index: Vec::new(),
        index_terms: Vec::new(),
        index_entries: index.to_vec(),
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
    let (vertical_items, segments) = pagination::build_stream(&items, vertical);
    let mut passes = 1u32;

    let (ends, kind) = match strategy {
        PaginationStrategy::VerticalKp => match pagination::place_kp(
            &vertical_items,
            &segments,
            page_height,
            &builder.notes,
            vertical,
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
        vertical,
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
        let footnotes =
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
                let item_index = vertical_items
                    .get(previous)
                    .map(|item| item.item)
                    .unwrap_or(0);
                builder
                    .headings
                    .iter()
                    .rev()
                    .find(|heading| heading.item <= item_index)
                    .map(|heading| heading.text.clone())
                    .unwrap_or_default()
            } else {
                String::from(source)
            };
            if let Some(block) = margin_block(
                &mut builder,
                NodeId(next_node),
                &text,
                ROOT_STYLE,
                content_width,
                master.header_height,
                Width(-master.header_height.0),
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
                        ROOT_STYLE,
                        content_width,
                        master.footer_height,
                        page_height,
                    ) {
                        footer.push(block);
                        next_node += 1;
                    }
                }
            }
        }

        pages.push(Page {
            number: index as u32 + 1,
            printed: printed.get(index).cloned().unwrap_or_default(),
            blocks,
            footnotes,
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
            printed: String::from("1"),
            blocks: Vec::new(),
            footnotes: Vec::new(),
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
    height: Width,
}

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
    content_width: Width,
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
    /// Heading depth the TOC includes; `0` disables collection.
    toc_depth: u8,
    /// Entries from the previous TOC pass; empty on the first pass.
    toc_entries: Vec<TocEntry>,
    /// Automatic numbering scheme.
    numbering: soma_template::NumberingSpec,
    /// Committed heading counters for levels 1..=3.
    heading_counts: [u32; 3],
    figure_count: u32,
    table_count: u32,
    /// Generated items at the head of the flow (cover + contents). Pages that
    /// only hold these are front matter.
    front_matter_items: usize,
    /// Index terms seen while compiling the current block.
    pending_index: Vec<String>,
    /// Index terms with the flow item they occur in, in document order.
    index_terms: Vec<(String, usize)>,
    /// Entries from the previous index pass; empty on the first pass.
    index_entries: Vec<TocEntry>,
    /// Figure and table captions found in this pass, with their flow item.
    figure_refs: Vec<CaptionRef>,
    table_refs: Vec<CaptionRef>,
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
                        }
                    }
                }
                Block::List(node) => {
                    let style = self.style(node.meta.id, fallback);
                    for item in &node.items {
                        let item_style = self.style(item.meta.id, style);
                        self.flow_into(&item.blocks, item_style, items, collect_index)?;
                    }
                }
                Block::BlockQuote(node) => {
                    let style = self.style(node.meta.id, fallback);
                    self.flow_into(&node.blocks, style, items, collect_index)?;
                }
                Block::CodeBlock(node) => {
                    let style = self.style(node.meta.id, fallback);
                    if let Some(item) = self.code_item(node.meta.id, &node.value, style)? {
                        items.push(item);
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
                    items.push(self.figure_item(node.meta.id, &node.image.source, style)?);
                    if !node.caption.is_empty() {
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
                    let columns = node.columns.len().max(1);
                    let column_width = Width(self.content_width.0 / columns as i32);
                    let row_count = node.rows.len();
                    if row_count == 0 {
                        continue;
                    }
                    let mut occupied = vec![vec![false; columns]; row_count];
                    let mut cells: Vec<TableCellLayout> = Vec::new();

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
                            let width = Width(column_width.0 * colspan as i32);
                            let cell_style = self.style(cell.meta.id, row_style);
                            let mut cell_items = Vec::new();
                            let saved = self.content_width;
                            self.content_width = width;
                            self.flow_into(&cell.blocks, cell_style, &mut cell_items, false)?;
                            self.content_width = saved;

                            let mut blocks = Vec::new();
                            let mut height = Width::ZERO;
                            for item in cell_items {
                                for mut placed in item.blocks {
                                    placed.kind = BoxKind::TableCell;
                                    placed.x = placed.x + Width(column_width.0 * column as i32);
                                    placed.y = height;
                                    height = height + placed.height;
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

                    // Row gaps are part of the row height (except above row 0).
                    let mut row_heights = vec![Width::ZERO; row_count];
                    let mut leading_gap = Width::ZERO;
                    for (row_index, row) in node.rows.iter().enumerate() {
                        let row_style = self.style(row.meta.id, style);
                        let computed = self.computed(row_style);
                        if row_index == 0 {
                            leading_gap = computed.space_before;
                        } else {
                            row_heights[row_index] = row_heights[row_index] + computed.space_before;
                        }
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
                    let mut row_top = Vec::with_capacity(row_count + 1);
                    let mut cursor = rules.outer_rule;
                    for (row, height) in row_heights.iter().enumerate() {
                        row_top.push(cursor);
                        cursor = cursor + *height + rule_after[row];
                    }
                    row_top.push(cursor);
                    let table_height = cursor;
                    let table_width = Width(column_width.0 * columns as i32);
                    let header_height = row_top[header_rows];

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
                            Width::ZERO,
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
                        let y = row_top[row] + row_heights[row];
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
                                Width(column_width.0 * start as i32),
                                y,
                                Width(column_width.0 * (column - start) as i32),
                                thickness,
                            );
                        }
                    }

                    // Vertical rules: internal boundaries plus the left/right
                    // frame. A cell that spans the boundary hides the segment
                    // for its own rows, and the header band is split out so the
                    // repeated header can carry its own side lines.
                    for boundary in 0..=columns {
                        let thickness = if boundary == 0 || boundary == columns {
                            rules.outer_column_rule
                        } else {
                            rules.column_rule
                        };
                        if thickness.0 == 0 {
                            continue;
                        }
                        let x = if boundary == 0 {
                            Width::ZERO
                        } else if boundary == columns {
                            Width(table_width.0 - thickness.0)
                        } else {
                            Width(column_width.0 * boundary as i32 - thickness.0 / 2)
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
                        before: Glue {
                            width: leading_gap,
                            plus: Width::ZERO,
                            minus: Width::ZERO,
                        },
                        after: Glue::ZERO,
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
            // last item is the caption when a float added one.
            if collect_index {
                if items.len() > before && !self.pending_index.is_empty() {
                    let item = items.len() - 1;
                    for term in self.pending_index.drain(..) {
                        self.index_terms.push((term, item));
                    }
                } else {
                    self.pending_index.clear();
                }
            } else {
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
        let computed = self.computed(style_id);
        let consumed = computed.indent_left.saturating_add(computed.indent_right);
        let inner = Width(self.content_width.0 - consumed.0);
        let paragraph_style = ParagraphStyle {
            width: inner,
            first_indent: computed.first_indent,
            line_height: computed.line_height,
            // The kernel only justifies or flushes left; centering and
            // right-alignment are applied to the finished lines below.
            align: kernel_align(computed.align),
        };

        let mut spans: Vec<OwnedSpan> = Vec::new();
        let mut footnotes: Vec<usize> = Vec::new();
        let mut marker_notes: Vec<Option<usize>> = Vec::new();
        let mut objects = math::InlineMaths::new();
        self.collect(
            inlines,
            style_id,
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
            })
            .collect();

        let paragraph = match paragraph_from_spans(&borrowed, paragraph_style) {
            Ok(paragraph) => paragraph,
            Err(RuleError::EmptyText) => return Ok(None),
        };
        let mut paragraph = paragraph;
        scale_trim(&mut paragraph, self.metrics, |style| {
            self.computed(style).font_size.0
        });

        let object_metrics = math::ObjectMetrics::new(self.metrics, &objects);
        let mut result = break_paragraph(&paragraph, &object_metrics, self.engine)
            .map_err(|_| LayoutError::new(Some(node), "line breaking failed"))?;
        align_lines(&mut result.lines, computed.align);

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
                },
            })
            .collect();

        let marks = marker_positions(&paragraph.atoms, &result.lines);
        self.text.insert(
            node,
            paragraph
                .atoms
                .iter()
                .map(|atom| (atom.text.clone(), atom.style))
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
                x: computed.indent_left,
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
            }],
        }))
    }

    fn collect(
        &mut self,
        inlines: &[Inline],
        fallback: StyleId,
        out: &mut Vec<OwnedSpan>,
        footnotes: &mut Vec<usize>,
        marker_notes: &mut Vec<Option<usize>>,
        objects: &mut math::InlineMaths,
    ) {
        for inline in inlines {
            let style = self.style(inline.meta().id, fallback);
            match inline {
                Inline::Text(node) => out.push(OwnedSpan {
                    text: node.value.clone(),
                    style,
                    kind: SpanKind::Text,
                }),
                Inline::Code(node) => out.push(OwnedSpan {
                    text: node.value.clone(),
                    style,
                    kind: SpanKind::Code,
                }),
                Inline::Strong(node)
                | Inline::Emphasis(node)
                | Inline::Underline(node)
                | Inline::Strikethrough(node)
                | Inline::SmallCaps(node)
                | Inline::Superscript(node)
                | Inline::Subscript(node) => {
                    self.collect(&node.children, style, out, footnotes, marker_notes, objects)
                }
                Inline::Link(node) => {
                    self.collect(&node.children, style, out, footnotes, marker_notes, objects)
                }
                Inline::Image(node) => out.push(OwnedSpan {
                    text: node
                        .source
                        .alt
                        .clone()
                        .unwrap_or_else(|| String::from("\u{fffc}")),
                    style,
                    kind: SpanKind::Object,
                }),
                Inline::Math(node) => {
                    // A parsable formula becomes an object atom measured by its
                    // own box; a malformed one keeps the source text, exactly
                    // as before ADR-0027.
                    let computed = self.computed(style);
                    match math::display_inline(&node.source, &computed, self.metrics, style) {
                        Some(layout) => {
                            let marker = objects.push(&node.source, layout);
                            out.push(OwnedSpan {
                                text: marker,
                                style,
                                kind: SpanKind::Object,
                            });
                        }
                        None => out.push(OwnedSpan {
                            text: node.source.clone(),
                            style,
                            kind: SpanKind::Object,
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
                        kind: SpanKind::Object,
                    });
                }
                Inline::Ruby(node) => out.push(OwnedSpan {
                    text: node.base.clone(),
                    style,
                    kind: SpanKind::Text,
                }),
                Inline::Index(node) => self.pending_index.push(node.term.clone()),
                Inline::LineBreak(_) => out.push(OwnedSpan {
                    text: String::new(),
                    style,
                    kind: SpanKind::HardBreak,
                }),
                Inline::Raw(node) => out.push(OwnedSpan {
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
        value: &str,
        style_id: StyleId,
    ) -> Result<Option<FlowItem>, LayoutError> {
        let computed = self.computed(style_id);
        let mut lines = Vec::new();
        let mut atoms: Vec<(String, StyleId)> = Vec::new();
        let mut y = Width::ZERO;
        let mut stage = FallbackStage::Normal;

        // Code is not wrapped in v1: each source line keeps its own line and a
        // long line may overflow.
        let wide = ParagraphStyle {
            width: Width::from_em(1000),
            first_indent: Width::ZERO,
            line_height: computed.line_height,
            align: computed.align,
        };

        for source_line in value.lines() {
            let paragraph = match paragraph_from_text(source_line, wide, style_id) {
                Ok(paragraph) => paragraph,
                Err(RuleError::EmptyText) => {
                    lines.push(empty_line(computed.line_height, y));
                    y = y + computed.line_height;
                    continue;
                }
            };
            atoms.extend(
                paragraph
                    .atoms
                    .iter()
                    .map(|atom| (atom.text.clone(), atom.style)),
            );
            let result = break_paragraph(&paragraph, self.metrics, self.engine)
                .map_err(|_| LayoutError::new(Some(node), "code breaking failed"))?;
            if result.fallback_stage != FallbackStage::Normal {
                stage = result.fallback_stage;
            }
            for line in &result.lines {
                lines.push(PlacedLine {
                    y,
                    hang: 0,
                    line: line.clone(),
                });
                y = y + line.height;
            }
        }

        if lines.is_empty() {
            return Ok(None);
        }
        self.text.insert(node, atoms);

        Ok(Some(FlowItem {
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
                kind: BoxKind::Code,
                style: style_id,
                x: Width::ZERO,
                y: Width::ZERO,
                width: self.content_width,
                height: y,
                lines,
                marks: Vec::new(),
                line_offset: 0,
                lines_total: 0,
                stage,
                math: None,
                inline_math: Vec::new(),
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
                .map(|atom| (atom.text.clone(), atom.style))
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
        let (title, subtitle, authors, date) = cover_lines(doc);

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
        let entry_style = self.styled.toc_style();
        let title_style = self.styled.toc_title_style();

        if let Some(title) = spec.title.as_deref().filter(|title| !title.is_empty()) {
            let node = self.generated_id();
            let item = self.object_item_at(
                node,
                BoxKind::Paragraph,
                title,
                title_style,
                Slot {
                    x: Width::ZERO,
                    width: self.content_width,
                    align: Align::Left,
                },
            )?;
            items.push(item);
        }

        let step = Width::from_em_frac(3, 2);
        let entries = self.toc_entries.clone();
        for entry in &entries {
            let indent = Width(step.0 * (entry.level.max(1) as i32 - 1));
            self.toc_entry_row(
                entry_style,
                &entry.text,
                &entry.page,
                indent,
                spec.leaders,
                items,
            )?;
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
                let item = self.object_item_at(
                    node,
                    BoxKind::Paragraph,
                    list_title,
                    title_style,
                    Slot {
                        x: Width::ZERO,
                        width: self.content_width,
                        align: Align::Left,
                    },
                )?;
                items.push(item);
            }
            for entry in entries {
                self.toc_entry_row(
                    entry_style,
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
            row.before = Glue::ZERO;
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
        row.before = Glue::ZERO;
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
        let mut terms: Vec<&String> = self.index_terms.iter().map(|(term, _)| term).collect();
        terms.sort();
        terms.dedup();
        let mut found = Vec::new();
        for term in terms {
            let mut pages: Vec<String> = Vec::new();
            for (candidate, item) in &self.index_terms {
                if candidate != term {
                    continue;
                }
                let label = label_of(*item);
                if !label.is_empty() && !pages.contains(&label) {
                    pages.push(label);
                }
            }
            if !pages.is_empty() {
                found.push(TocEntry {
                    level: 0,
                    text: term.clone(),
                    page: pages.join(", "),
                });
            }
        }
        found
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
            let item = self.text_item_at(
                node,
                BoxKind::Paragraph,
                title,
                title_style,
                Slot {
                    x: Width::ZERO,
                    width: self.content_width,
                    align: Align::Left,
                },
            )?;
            let index = items.len();
            items.push(item);
            // The index is a navigable destination too: it joins the running
            // title, the contents and the bookmark tree as a level-1 heading.
            self.headings.push(Heading {
                item: index,
                level: 1,
                text: String::from(title),
            });
        }
        let entries = self.index_entries.clone();
        for entry in &entries {
            self.toc_entry_row(
                entry_style,
                &entry.text,
                &entry.page,
                Width::ZERO,
                true,
                items,
            )?;
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
        let computed = self.computed(style_id);
        let layout = math::display(source, &computed, self.metrics, style_id)?;
        let box_width = layout.width;
        let height = layout.total_height();
        let x = Width((self.content_width.0 - box_width.0).max(0) / 2);
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
                width: box_width,
                height,
                lines: Vec::new(),
                marks: Vec::new(),
                line_offset: 0,
                lines_total: 0,
                stage: FallbackStage::Normal,
                inline_math: Vec::new(),
                math: Some(MathBox {
                    width: box_width,
                    height,
                    baseline: layout.height,
                    items: layout.items,
                    rules: layout.rules,
                }),
            }],
        })
    }

    fn figure_item(
        &mut self,
        node: NodeId,
        image: &soma_doc::meta::ImageRef,
        style_id: StyleId,
    ) -> Result<FlowItem, LayoutError> {
        let computed = self.computed(style_id);
        let width = self.content_width;
        let height = match image.intrinsic {
            Some(intrinsic) if intrinsic.width_px > 0 => {
                width.scale(intrinsic.height_px as i32, intrinsic.width_px as i32)
            }
            _ => width.scale(2, 3),
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
                x: Width::ZERO,
                y: Width::ZERO,
                width,
                height,
                lines: Vec::new(),
                marks: Vec::new(),
                line_offset: 0,
                lines_total: 0,
                math: None,
                inline_math: Vec::new(),
                stage: FallbackStage::Normal,
            }],
        })
    }

    fn compile_notes(&mut self, doc: &Document) -> Result<(), LayoutError> {
        let mut compiled = Vec::with_capacity(doc.footnotes.len());
        for definition in &doc.footnotes {
            let style = self.style(definition.meta.id, ROOT_STYLE);
            let mut items = Vec::new();
            self.flow_into(&definition.blocks, style, &mut items, false)?;
            let mut blocks = Vec::new();
            let mut y = Width::ZERO;
            for item in items {
                for mut placed in item.blocks {
                    placed.kind = BoxKind::Footnote;
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

/// Build a header/footer line: one centered line, outside the content box.
///
/// The band lives in the page margin, so it never competes with the content
/// height and cannot change pagination.
fn margin_block(
    builder: &mut Builder,
    node: NodeId,
    text: &str,
    style: StyleId,
    content_width: Width,
    band_height: Width,
    y: Width,
) -> Option<PlacedBlock> {
    if text.is_empty() {
        return None;
    }
    let paragraph_style = ParagraphStyle {
        width: Width::from_em(1000),
        first_indent: Width::ZERO,
        line_height: band_height,
        align: soma_core::Align::Justify,
    };
    let paragraph = paragraph_from_text(text, paragraph_style, style).ok()?;
    builder.text.insert(
        node,
        paragraph
            .atoms
            .iter()
            .map(|atom| (atom.text.clone(), atom.style))
            .collect(),
    );
    let result = break_paragraph(&paragraph, builder.metrics, builder.engine).ok()?;
    let stage = result.fallback_stage;
    let line = result.lines.into_iter().next()?;
    let natural = line.natural_width;
    let x = Width((content_width.0 - natural.0).max(0) / 2);

    Some(PlacedBlock {
        node,
        kind: BoxKind::Paragraph,
        style,
        x,
        y,
        width: natural,
        height: band_height,
        lines: vec![PlacedLine {
            y: Width::ZERO,
            hang: 0,
            line,
        }],
        marks: Vec::new(),
        line_offset: 0,
        lines_total: 1,
        stage,
        math: None,
        inline_math: Vec::new(),
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
        if let Some(ink) = metrics.ink_left(&atom.text, atom.style) {
            atom.compress_left = ink;
        }
        if let Some(ink) = metrics.ink_right(&atom.text, atom.style) {
            atom.compress_right = ink;
        }
    }
}

fn scale_units(value: i32, size: i32) -> i32 {
    (value as i64 * size as i64 / 4096) as i32
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
        PunctClass::Open | PunctClass::None => 0,
    }
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
}

impl Numbering {
    fn new(policy: soma_template::FootnoteNumbering) -> Self {
        Self { policy, next: 1 }
    }

    fn assign(&mut self, pages: &mut [Page]) {
        for page in pages.iter_mut() {
            if matches!(self.policy, soma_template::FootnoteNumbering::Page) {
                self.next = 1;
            }
            for block in page.blocks.iter_mut() {
                for mark in block.marks.iter_mut() {
                    mark.number = self.next;
                    self.next += 1;
                }
            }
        }
    }
}

/// The four cover lines, taken from the document metadata.
fn cover_lines(
    doc: &Document,
) -> (
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
) {
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
    (title, subtitle, authors, date)
}

#[cfg(test)]
mod unit {
    use super::*;
    use soma_core::SyntheticMetrics;
    use soma_rules::justified_style;

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
