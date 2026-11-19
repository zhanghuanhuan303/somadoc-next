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
pub mod metrics;
mod pagination;

pub use metrics::{StyledMetrics, TableMetrics};

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

use soma_core::{
    break_paragraph, BreakConfig, Cost, FallbackStage, Line, Metrics, Paragraph, ParagraphStyle,
    PunctClass, StyleId, Width,
};
use soma_doc::{Block, Document, Inline, NodeId};
use soma_rules::{paragraph_from_spans, paragraph_from_text, InlineSpan, RuleError, SpanKind};
use soma_template::{ComputedStyle, ResolvedPage, StyledDoc};

pub const SCHEMA: &str = "soma.pages/3";

/// Placeholder atom for a footnote marker. Renderers replace the marked atom
/// with [`Mark::number`].
pub const FOOTNOTE_MARKER: char = '\u{e000}';

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
    /// Index of the first line in this page slice. Non-zero means the block is
    /// continued from the previous page.
    pub line_offset: u32,
    /// Total number of lines in the unsplit block.
    pub lines_total: u32,
    pub stage: FallbackStage,
}

/// One page.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Page {
    pub number: u32,
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
}

/// Owned atom text per DocIR block. Built while laying out, so the renderer
/// does not recompile anything.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TextIndex {
    entries: Vec<(NodeId, Vec<(String, StyleId)>)>,
}

impl TextIndex {
    pub fn blocks(&self) -> &[(NodeId, Vec<(String, StyleId)>)] {
        &self.entries
    }

    fn insert(&mut self, node: NodeId, atoms: Vec<(String, StyleId)>) {
        self.entries.push((node, atoms));
    }

    fn finish(&mut self) {
        self.entries.sort_by_key(|(node, _)| *node);
    }

    fn max_node(&self) -> u32 {
        self.entries.last().map(|(node, _)| node.0).unwrap_or(0)
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

/// Lay out with an explicit strategy and vertical cost model.
pub fn layout_with(
    doc: &Document,
    styled: &StyledDoc,
    metrics: &dyn Metrics,
    engine: &BreakConfig,
    strategy: PaginationStrategy,
    vertical: VerticalParams,
) -> Result<PagedLayout, LayoutError> {
    let page = styled.page();
    let content_width = page.content_width();
    if content_width.0 <= 0 || page.content_height().0 <= 0 {
        return Err(LayoutError::new(None, "page content box must be positive"));
    }

    let mut builder = Builder {
        styled,
        metrics,
        engine,
        content_width,
        text: TextIndex::default(),
        notes: NoteMap::build(doc),
        headings: Vec::new(),
    };
    let items = builder.flow(&doc.blocks, ROOT_STYLE)?;
    builder.compile_notes(doc)?;

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

    let mut pages = Vec::with_capacity(ends.len().max(1));
    let mut previous = 0usize;
    let total_pages = ends.len().max(1) as u32;
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
        );
        let overflow = vertical_items[previous..end]
            .iter()
            .any(|item| item.height.0 > page_height.0);
        let footnotes =
            pagination::assemble_footnotes(previous, end, &vertical_items, &builder.notes);

        let mut header = Vec::new();
        let mut footer = Vec::new();
        if page.header_height.0 > 0 {
            let source = styled.header_text().unwrap_or("running-title");
            let text = if source == "running-title" {
                let item_index = vertical_items
                    .get(previous)
                    .map(|item| item.item)
                    .unwrap_or(0);
                builder
                    .headings
                    .iter()
                    .rev()
                    .find(|(position, _, _)| *position <= item_index)
                    .map(|(_, _, title)| title.clone())
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
                page.header_height,
                Width(-page.header_height.0),
            ) {
                header.push(block);
                next_node += 1;
            }
        }
        if page.footer_height.0 > 0 {
            if let Some(format) = styled.folio() {
                let text = format
                    .replace("{page}", &(index as u32 + 1).to_string())
                    .replace("{pages}", &total_pages.to_string());
                if let Some(block) = margin_block(
                    &mut builder,
                    NodeId(next_node),
                    &text,
                    ROOT_STYLE,
                    content_width,
                    page.footer_height,
                    page_height,
                ) {
                    footer.push(block);
                    next_node += 1;
                }
            }
        }

        pages.push(Page {
            number: index as u32 + 1,
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

    Ok(PagedLayout {
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
    })
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
}

struct FlowItem {
    blocks: Vec<PlacedBlock>,
    height: Width,
    before: Glue,
    after: Glue,
    keep_with_next: bool,
    page_break_before: bool,
    forced_break: bool,
    footnotes: Vec<usize>,
    /// Empty for atomic blocks; one entry per line for splittable blocks.
    parts: Vec<Part>,
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

struct Builder<'a> {
    styled: &'a StyledDoc,
    metrics: &'a dyn Metrics,
    engine: &'a BreakConfig,
    content_width: Width,
    text: TextIndex,
    notes: NoteMap,
    /// `(flow item index, node, plain text)` for headings, in document order.
    headings: Vec<(usize, NodeId, String)>,
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

    fn flow(&mut self, blocks: &[Block], fallback: StyleId) -> Result<Vec<FlowItem>, LayoutError> {
        let mut items = Vec::new();
        self.flow_into(blocks, fallback, &mut items)?;
        Ok(items)
    }

    fn flow_into(
        &mut self,
        blocks: &[Block],
        fallback: StyleId,
        items: &mut Vec<FlowItem>,
    ) -> Result<(), LayoutError> {
        for block in blocks {
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
                    if let Some(item) =
                        self.paragraph_item(node.meta.id, BoxKind::Heading, &node.inlines, style)?
                    {
                        items.push(item);
                        self.headings
                            .push((index, node.meta.id, inline_text(&node.inlines)));
                    }
                }
                Block::List(node) => {
                    let style = self.style(node.meta.id, fallback);
                    for item in &node.items {
                        let item_style = self.style(item.meta.id, style);
                        self.flow_into(&item.blocks, item_style, items)?;
                    }
                }
                Block::BlockQuote(node) => {
                    let style = self.style(node.meta.id, fallback);
                    self.flow_into(&node.blocks, style, items)?;
                }
                Block::CodeBlock(node) => {
                    let style = self.style(node.meta.id, fallback);
                    if let Some(item) = self.code_item(node.meta.id, &node.value, style)? {
                        items.push(item);
                    }
                }
                Block::MathBlock(node) => {
                    let style = self.style(node.meta.id, fallback);
                    items.push(self.object_item(
                        node.meta.id,
                        BoxKind::Math,
                        &node.source,
                        style,
                    )?);
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
                        if let Some(item) = self.paragraph_item(
                            caption_node,
                            BoxKind::Paragraph,
                            &node.caption,
                            style,
                        )? {
                            items.push(item);
                        }
                    }
                }
                Block::Table(node) => {
                    let style = self.style(node.meta.id, fallback);
                    let columns = node.columns.len().max(1);
                    let column_width = Width(self.content_width.0 / columns as i32);
                    for row in &node.rows {
                        let row_style = self.style(row.meta.id, style);
                        let row_computed = self.computed(row_style);
                        let mut row_blocks = Vec::new();
                        let mut row_height = Width::ZERO;
                        for (cell_index, cell) in row.cells.iter().enumerate() {
                            let cell_style = self.style(cell.meta.id, row_style);
                            let x = Width(column_width.0 * cell_index as i32);
                            let mut cell_items = Vec::new();
                            self.flow_into(&cell.blocks, cell_style, &mut cell_items)?;
                            let mut y = Width::ZERO;
                            for item in cell_items {
                                for mut placed in item.blocks {
                                    placed.kind = BoxKind::TableCell;
                                    placed.x = x;
                                    placed.y = y;
                                    y = y + placed.height;
                                    row_blocks.push(placed);
                                }
                            }
                            row_height = row_height.max(y);
                        }
                        items.push(FlowItem {
                            blocks: row_blocks,
                            height: row_height,
                            before: self.before_glue(&row_computed),
                            after: Glue::ZERO,
                            keep_with_next: false,
                            page_break_before: false,
                            forced_break: false,
                            footnotes: Vec::new(),
                            parts: Vec::new(),
                        });
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
                            stage: FallbackStage::Normal,
                        }],
                        height,
                        before: self.before_glue(&computed),
                        after: self.after_glue(&computed),
                        keep_with_next: false,
                        page_break_before: false,
                        forced_break: false,
                        footnotes: Vec::new(),
                        parts: Vec::new(),
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
                    footnotes: Vec::new(),
                    parts: Vec::new(),
                }),
                Block::Section(node) => {
                    let style = self.style(node.meta.id, fallback);
                    if let Some(title) = &node.title {
                        let index = items.len();
                        if let Some(item) =
                            self.paragraph_item(node.meta.id, BoxKind::Heading, title, style)?
                        {
                            items.push(item);
                            self.headings
                                .push((index, node.meta.id, inline_text(title)));
                        }
                    }
                }
                Block::Generated(node) => {
                    let style = self.style(node.meta.id, fallback);
                    self.flow_into(&node.blocks, style, items)?;
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
            align: computed.align,
        };

        let mut spans: Vec<OwnedSpan> = Vec::new();
        let mut footnotes: Vec<usize> = Vec::new();
        let mut marker_notes: Vec<Option<usize>> = Vec::new();
        self.collect(
            inlines,
            style_id,
            &mut spans,
            &mut footnotes,
            &mut marker_notes,
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
        scale_trim(&mut paragraph, |style| self.computed(style).font_size.0);

        let result = break_paragraph(&paragraph, self.metrics, self.engine)
            .map_err(|_| LayoutError::new(Some(node), "line breaking failed"))?;

        let marks = marker_positions(&paragraph.atoms, &result.lines);
        self.text.insert(
            node,
            paragraph
                .atoms
                .iter()
                .map(|atom| (atom.text.clone(), atom.style))
                .collect(),
        );

        let grid = computed.baseline_grid;
        let mut placed_lines = Vec::with_capacity(result.lines.len());
        let mut parts = Vec::with_capacity(result.lines.len());
        let mut y = Width::ZERO;
        for line in &result.lines {
            let mut line = line.clone();
            let hang = if computed.hanging_punctuation {
                hanging_amount(&paragraph, &line, computed.hanging_cap)
            } else {
                0
            };
            if hang > 0 {
                let sign = if line.adjustment < 0 { -1 } else { 1 };
                line.adjustment -= sign * hang;
                if let Some(last) = line.boxes.last_mut() {
                    last.x = Width(last.x.0 + hang);
                }
            }
            let advance = snap_to_grid(line.height, grid);
            placed_lines.push(PlacedLine { y, hang, line });
            parts.push(Part {
                height: advance,
                notes: Vec::new(),
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
            footnotes,
            parts,
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
                    self.collect(&node.children, style, out, footnotes, marker_notes)
                }
                Inline::Link(node) => {
                    self.collect(&node.children, style, out, footnotes, marker_notes)
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
                Inline::Math(node) => out.push(OwnedSpan {
                    text: node.source.clone(),
                    style,
                    kind: SpanKind::Object,
                }),
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
            footnotes: Vec::new(),
            parts: Vec::new(),
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
        let computed = self.computed(style_id);
        let paragraph_style = ParagraphStyle {
            width: self.content_width,
            first_indent: Width::ZERO,
            line_height: computed.line_height,
            align: computed.align,
        };
        let spans = [InlineSpan::object(text, style_id)];
        let paragraph = paragraph_from_spans(&spans, paragraph_style)
            .map_err(|_| LayoutError::new(Some(node), "object compilation failed"))?;
        let result = break_paragraph(&paragraph, self.metrics, self.engine)
            .map_err(|_| LayoutError::new(Some(node), "object breaking failed"))?;
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
            footnotes: Vec::new(),
            parts: Vec::new(),
            blocks: vec![PlacedBlock {
                node,
                kind,
                style: style_id,
                x: Width::ZERO,
                y: Width::ZERO,
                width: self.content_width,
                height: y,
                lines,
                marks: Vec::new(),
                line_offset: 0,
                lines_total: 0,
                stage: result.fallback_stage,
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
            footnotes: Vec::new(),
            parts: Vec::new(),
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
                stage: FallbackStage::Normal,
            }],
        })
    }

    fn compile_notes(&mut self, doc: &Document) -> Result<(), LayoutError> {
        let mut compiled = Vec::with_capacity(doc.footnotes.len());
        for definition in &doc.footnotes {
            let style = self.style(definition.meta.id, ROOT_STYLE);
            let mut items = Vec::new();
            self.flow_into(&definition.blocks, style, &mut items)?;
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
            Inline::LineBreak(_) | Inline::FootnoteRef(_) => {}
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

/// Scale the rules layer's base-em trim caps to each atom's own font size.
fn scale_trim(paragraph: &mut Paragraph, size_of: impl Fn(StyleId) -> i32) {
    for atom in &mut paragraph.atoms {
        let size = size_of(atom.style);
        if size <= 0 || size == 4096 {
            continue;
        }
        atom.compress_left = Width(scale_units(atom.compress_left.0, size));
        atom.compress_right = Width(scale_units(atom.compress_right.0, size));
    }
}

fn scale_units(value: i32, size: i32) -> i32 {
    (value as i64 * size as i64 / 4096) as i32
}

/// How far the final punctuation may hang into the margin. Hanging relaxes
/// justification in either direction: it reduces compression on a tight line
/// and stretch on a loose one.
fn hanging_amount(paragraph: &Paragraph, line: &Line, cap: Width) -> i32 {
    if line.adjustment == 0 || cap.0 <= 0 {
        return 0;
    }
    let Some(last) = line.end.checked_sub(1) else {
        return 0;
    };
    let Some(atom) = paragraph.atoms.get(last as usize) else {
        return 0;
    };
    match atom.class {
        PunctClass::Close | PunctClass::Middle => line.adjustment.abs().min(cap.0),
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

#[cfg(test)]
mod unit {
    use super::*;
    use soma_rules::justified_style;

    #[test]
    fn trim_caps_scale_with_the_atom_font_size() {
        let style = justified_style(Width::from_em(20));
        let mut paragraph = paragraph_from_text("（字）。", style, StyleId(1)).expect("paragraph");
        let open = paragraph.atoms[0].compress_left.0;
        let close = paragraph.atoms[2].compress_right.0;
        assert!(open > 0 && close > 0);

        scale_trim(&mut paragraph, |_| 8192);
        assert_eq!(paragraph.atoms[0].compress_left.0, open * 2);
        assert_eq!(paragraph.atoms[2].compress_right.0, close * 2);
    }

    #[test]
    fn trim_is_unchanged_at_the_body_size() {
        let style = justified_style(Width::from_em(20));
        let mut paragraph = paragraph_from_text("（字）。", style, StyleId(1)).expect("paragraph");
        let before = paragraph.atoms.clone();
        scale_trim(&mut paragraph, |_| 4096);
        for (atom, old) in paragraph.atoms.iter().zip(before.iter()) {
            assert_eq!(atom.compress_left, old.compress_left);
            assert_eq!(atom.compress_right, old.compress_right);
        }
    }
}
