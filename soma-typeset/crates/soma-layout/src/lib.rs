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

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use soma_core::{
    break_paragraph, BreakConfig, Cost, FallbackStage, Line, Metrics, ParagraphStyle, StyleId,
    Width,
};
use soma_doc::{Block, Document, Inline, NodeId};
use soma_rules::{paragraph_from_spans, paragraph_from_text, InlineSpan, RuleError, SpanKind};
use soma_template::{ComputedStyle, ResolvedPage, StyledDoc};

pub const SCHEMA: &str = "soma.pages/1";

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

/// Vertical cost model. These are pagination-model constants, not engine knobs:
/// they do not enter `paramHash`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VerticalParams {
    /// Weight applied for a page whose fill leaves no freedom. `0` keeps the
    /// page count neutral; the DP then optimises badness only.
    pub page_weight: i64,
    /// Scale for `badness = scale * leftover_permille^3 / 1e6`.
    pub badness_scale: i64,
    /// Cost of a page that must hold one block taller than the content area.
    pub overflow_penalty: i64,
}

impl VerticalParams {
    pub const fn default_v1() -> Self {
        Self {
            page_weight: 0,
            badness_scale: 100,
            overflow_penalty: 1_000_000,
        }
    }
}

impl Default for VerticalParams {
    fn default() -> Self {
        Self::default_v1()
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
}

/// One line inside a placed block. `y` is relative to the block top.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlacedLine {
    pub y: Width,
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
    pub stage: FallbackStage,
}

/// One page.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Page {
    pub number: u32,
    pub blocks: Vec<PlacedBlock>,
    pub footnotes: Vec<PlacedBlock>,
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
    };
    let items = builder.flow(&doc.blocks, ROOT_STYLE)?;
    builder.compile_notes(doc)?;
    builder.text.finish();

    let page_height = page.content_height();
    let mut passes = 1u32;

    let (mut page_items, mut kind) = match strategy {
        PaginationStrategy::VerticalKp => {
            match vertical_place(&items, page_height, &builder.notes, vertical) {
                Some(pages) => (pages, PaginationKind::VerticalKp),
                None => {
                    // No feasible break sequence: fall back to the deterministic
                    // greedy pass and record it.
                    let mut reserved: Vec<Width> = Vec::new();
                    let mut placed = greedy_place(&items, page_height, &reserved);
                    for _ in 0..8 {
                        let heights = footnote_heights(&placed, &items, &builder.notes);
                        if heights == reserved {
                            break;
                        }
                        reserved = heights;
                        placed = greedy_place(&items, page_height, &reserved);
                        passes += 1;
                    }
                    (placed, PaginationKind::Greedy)
                }
            }
        }
        PaginationStrategy::Greedy => {
            let mut reserved: Vec<Width> = Vec::new();
            let mut placed = greedy_place(&items, page_height, &reserved);
            for _ in 0..8 {
                let heights = footnote_heights(&placed, &items, &builder.notes);
                if heights == reserved {
                    break;
                }
                reserved = heights;
                placed = greedy_place(&items, page_height, &reserved);
                passes += 1;
            }
            (placed, PaginationKind::Greedy)
        }
    };

    // Drop page groups that contain nothing (a leading forced break).
    page_items.retain(|group| !group.is_empty());
    if page_items.is_empty() {
        page_items.push(Vec::new());
    }
    if page_items.len() == 1
        && kind == PaginationKind::Greedy
        && strategy == PaginationStrategy::VerticalKp
    {
        // Fallback already recorded; nothing more to do.
        kind = PaginationKind::Greedy;
    }

    let pagination_cost = score_pages(&page_items, &items, &builder.notes, page_height, vertical);

    let mut pages = Vec::with_capacity(page_items.len());
    for (index, item_indices) in page_items.iter().enumerate() {
        let mut blocks: Vec<PlacedBlock> = Vec::new();
        let mut used = Width::ZERO;
        let mut overflow = false;

        for &item_index in item_indices {
            let item = &items[item_index];
            if !blocks.is_empty() {
                used = used + item.before;
            }
            for block in &item.blocks {
                let mut placed = block.clone();
                placed.y = placed.y + used;
                blocks.push(placed);
            }
            used = used + item.height + item.after;
            if item.height > page_height {
                overflow = true;
            }
        }

        let footnotes = assemble_footnotes(&page_items[index], &items, &builder.notes);
        let footnote_height = page_note_height(&page_items[index], &items, &builder.notes);

        pages.push(Page {
            number: index as u32 + 1,
            blocks,
            footnotes,
            used_height: used,
            footnote_height,
            overflow,
        });
    }

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

struct FlowItem {
    blocks: Vec<PlacedBlock>,
    height: Width,
    before: Width,
    after: Width,
    keep_with_next: bool,
    page_break_before: bool,
    forced_break: bool,
    footnotes: Vec<usize>,
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
                    if let Some(item) =
                        self.paragraph_item(node.meta.id, BoxKind::Heading, &node.inlines, style)?
                    {
                        items.push(item);
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
                        let row_space = self.computed(row_style).space_before;
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
                            before: row_space,
                            after: Width::ZERO,
                            keep_with_next: false,
                            page_break_before: false,
                            forced_break: false,
                            footnotes: Vec::new(),
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
                            stage: FallbackStage::Normal,
                        }],
                        height,
                        before: computed.space_before,
                        after: computed.space_after,
                        keep_with_next: false,
                        page_break_before: false,
                        forced_break: false,
                        footnotes: Vec::new(),
                    });
                }
                Block::PageBreak(_) => items.push(FlowItem {
                    blocks: Vec::new(),
                    height: Width::ZERO,
                    before: Width::ZERO,
                    after: Width::ZERO,
                    keep_with_next: false,
                    page_break_before: false,
                    forced_break: true,
                    footnotes: Vec::new(),
                }),
                Block::Section(node) => {
                    let style = self.style(node.meta.id, fallback);
                    if let Some(title) = &node.title {
                        if let Some(item) =
                            self.paragraph_item(node.meta.id, BoxKind::Heading, title, style)?
                        {
                            items.push(item);
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
        self.collect(inlines, style_id, &mut spans, &mut footnotes);

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

        let result = break_paragraph(&paragraph, self.metrics, self.engine)
            .map_err(|_| LayoutError::new(Some(node), "line breaking failed"))?;

        let marks = marker_positions(&paragraph.atoms);
        self.text.insert(
            node,
            paragraph
                .atoms
                .iter()
                .map(|atom| (atom.text.clone(), atom.style))
                .collect(),
        );

        let mut placed_lines = Vec::with_capacity(result.lines.len());
        let mut y = Width::ZERO;
        for line in &result.lines {
            placed_lines.push(PlacedLine {
                y,
                line: line.clone(),
            });
            y = y + line.height;
        }

        Ok(Some(FlowItem {
            height: y,
            before: computed.space_before,
            after: computed.space_after,
            keep_with_next: computed.keep_with_next,
            page_break_before: computed.page_break_before,
            forced_break: false,
            footnotes,
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
                | Inline::Subscript(node) => self.collect(&node.children, style, out, footnotes),
                Inline::Link(node) => self.collect(&node.children, style, out, footnotes),
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
                    if let Some(index) = self.notes.find(&node.target) {
                        if !footnotes.contains(&index) {
                            footnotes.push(index);
                        }
                    }
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
            before: computed.space_before,
            after: computed.space_after,
            keep_with_next: computed.keep_with_next,
            page_break_before: computed.page_break_before,
            forced_break: false,
            footnotes: Vec::new(),
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
                line: line.clone(),
            });
            y = y + line.height;
        }

        Ok(FlowItem {
            height: y,
            before: computed.space_before,
            after: computed.space_after,
            keep_with_next: computed.keep_with_next,
            page_break_before: computed.page_break_before,
            forced_break: false,
            footnotes: Vec::new(),
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
            before: computed.space_before,
            after: computed.space_after,
            keep_with_next: computed.keep_with_next,
            page_break_before: computed.page_break_before,
            forced_break: false,
            footnotes: Vec::new(),
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
fn marker_positions(atoms: &[soma_core::Atom]) -> Vec<Mark> {
    let mut marks = Vec::new();
    let mut marker = [0u8; 4];
    let text = FOOTNOTE_MARKER.encode_utf8(&mut marker);
    for (index, atom) in atoms.iter().enumerate() {
        if atom.text.as_str() == &*text {
            marks.push(Mark {
                atom: index as u32,
                number: 0,
            });
        }
    }
    marks
}

fn empty_line(line_height: Width, y: Width) -> PlacedLine {
    PlacedLine {
        y,
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

fn greedy_place(items: &[FlowItem], page_height: Width, reserved: &[Width]) -> Vec<Vec<usize>> {
    let mut pages: Vec<Vec<usize>> = Vec::new();
    let mut current: Vec<usize> = Vec::new();
    let mut used = Width::ZERO;

    for (index, item) in items.iter().enumerate() {
        if item.forced_break {
            if !current.is_empty() {
                pages.push(core::mem::take(&mut current));
            }
            used = Width::ZERO;
            continue;
        }

        let page_index = pages.len();
        let limit =
            page_height.saturating_sub(reserved.get(page_index).copied().unwrap_or(Width::ZERO));

        let spacing = if current.is_empty() {
            Width::ZERO
        } else {
            item.before
        };
        let needed = spacing + item.height + item.after;

        if (item.page_break_before || (!current.is_empty() && used + needed > limit))
            && !current.is_empty()
        {
            pages.push(core::mem::take(&mut current));
            used = Width::ZERO;
        }

        let spacing = if current.is_empty() {
            Width::ZERO
        } else {
            item.before
        };
        used = used + spacing + item.height + item.after;
        current.push(index);
    }

    if !current.is_empty() {
        pages.push(current);
    }

    fix_keep_with_next(items, &mut pages);
    if pages.is_empty() {
        pages.push(Vec::new());
    }
    pages
}

/// Footnote area height for one page group.
fn page_note_height(page_items: &[usize], items: &[FlowItem], notes: &NoteMap) -> Width {
    let mut seen: Vec<usize> = Vec::new();
    for &item_index in page_items {
        for note in &items[item_index].footnotes {
            if !seen.contains(note) {
                seen.push(*note);
            }
        }
    }
    let mut height = Width::ZERO;
    for note in seen {
        if let Some(blocks) = notes.blocks.get(note) {
            for block in blocks {
                height = height + block.height;
            }
        }
    }
    height
}

/// TeX-style page badness: cubic in the leftover fraction, in permille.
fn page_badness(used: Width, page_height: Width, overflow: bool, params: VerticalParams) -> i64 {
    if overflow {
        return params.overflow_penalty;
    }
    let free = page_height.0 - used.0;
    if free <= 0 || page_height.0 <= 0 {
        return 0;
    }
    let milli = free as i64 * 1000 / page_height.0 as i64;
    params.badness_scale * milli * milli * milli / 1_000_000
}

fn relax(best: &mut [(i64, u32, usize)], from: usize, to: usize, page_cost: i64) {
    let base = best[from];
    let candidate = (base.0 + page_cost, base.1 + 1, from);
    if candidate < best[to] {
        best[to] = candidate;
    }
}

/// Vertical Knuth-Plass over flow items: choose page breaks minimizing summed
/// page badness. The last page is free (like a paragraph's last line). Hard
/// constraints: forced breaks, `page_break_before`, `keep-with-next`, and a
/// single overfull block must own its page.
fn vertical_place(
    items: &[FlowItem],
    page_height: Width,
    notes: &NoteMap,
    params: VerticalParams,
) -> Option<Vec<Vec<usize>>> {
    let n = items.len();
    if n == 0 {
        return Some(Vec::new());
    }
    const INF: i64 = i64::MAX / 4;
    let mut best: Vec<(i64, u32, usize)> = vec![(INF, u32::MAX, 0); n + 1];
    best[0] = (0, 0, 0);

    for i in 0..n {
        if best[i].0 == INF {
            continue;
        }
        if items[i].forced_break {
            relax(&mut best, i, i + 1, 0);
            continue;
        }

        let mut used = Width::ZERO;
        let mut notes_seen: Vec<usize> = Vec::new();
        let mut j = i;
        while j < n {
            let item = &items[j];
            if item.forced_break {
                break;
            }
            if j > i && item.page_break_before {
                break;
            }

            let gap = if j == i { Width::ZERO } else { item.before };
            used = used + gap + item.height + item.after;
            for note in &item.footnotes {
                if !notes_seen.contains(note) {
                    notes_seen.push(*note);
                }
            }
            let total = used + note_height_of(&notes_seen, notes);
            let overflow = total.0 > page_height.0;
            let single = j == i;

            if overflow && !single {
                break;
            }
            if overflow && single {
                let end = j + 1;
                let last = end == n;
                let mut cost = params.page_weight;
                if !last {
                    cost += page_badness(total, page_height, true, params);
                }
                relax(&mut best, i, end, cost);
                break;
            }

            let end = j + 1;
            let last = end == n;
            let can_break = last
                || items[end].forced_break
                || (!items[j].keep_with_next && !items[end].page_break_before);
            if can_break {
                let mut cost = params.page_weight;
                if !last {
                    cost += page_badness(total, page_height, false, params);
                }
                relax(&mut best, i, end, cost);
            }
            j += 1;
        }
    }

    if best[n].0 == INF {
        return None;
    }

    let mut groups: Vec<Vec<usize>> = Vec::new();
    let mut at = n;
    while at > 0 {
        let prev = best[at].2;
        let group: Vec<usize> = (prev..at)
            .filter(|index| !items[*index].forced_break)
            .collect();
        if !group.is_empty() {
            groups.push(group);
        }
        at = prev;
    }
    groups.reverse();
    Some(groups)
}

fn note_height_of(notes_seen: &[usize], notes: &NoteMap) -> Width {
    let mut height = Width::ZERO;
    for note in notes_seen {
        if let Some(blocks) = notes.blocks.get(*note) {
            for block in blocks {
                height = height + block.height;
            }
        }
    }
    height
}

/// Cost of a finished pagination under the vertical model. Used for reporting
/// and for the optimality test against the greedy path.
fn score_pages(
    groups: &[Vec<usize>],
    items: &[FlowItem],
    notes: &NoteMap,
    page_height: Width,
    params: VerticalParams,
) -> i64 {
    let last = groups.len().saturating_sub(1);
    let mut total = 0i64;
    for (index, group) in groups.iter().enumerate() {
        let mut used = Width::ZERO;
        for (offset, &item_index) in group.iter().enumerate() {
            let item = &items[item_index];
            if offset > 0 {
                used = used + item.before;
            }
            used = used + item.height + item.after;
        }
        let height = used + page_note_height(group, items, notes);
        let overflow = group
            .iter()
            .any(|&item_index| items[item_index].height.0 > page_height.0);
        let mut cost = params.page_weight;
        if index != last {
            cost += page_badness(height, page_height, overflow, params);
        }
        total += cost;
    }
    total
}

/// A `keep-with-next` block must not be the last block on a page.
fn fix_keep_with_next(items: &[FlowItem], pages: &mut [Vec<usize>]) {
    for index in 0..pages.len().saturating_sub(1) {
        while let Some(&last) = pages[index].last() {
            if !items[last].keep_with_next || pages[index].len() < 2 {
                break;
            }
            pages[index].pop();
            pages[index + 1].insert(0, last);
        }
    }
}

fn page_note_indices(page_items: &[usize], items: &[FlowItem]) -> Vec<usize> {
    let mut seen: Vec<usize> = Vec::new();
    for &item_index in page_items {
        for note in &items[item_index].footnotes {
            if !seen.contains(note) {
                seen.push(*note);
            }
        }
    }
    seen
}

fn footnote_heights(page_items: &[Vec<usize>], items: &[FlowItem], notes: &NoteMap) -> Vec<Width> {
    page_items
        .iter()
        .map(|page| {
            let mut height = Width::ZERO;
            for note in page_note_indices(page, items) {
                if let Some(blocks) = notes.blocks.get(note) {
                    for block in blocks {
                        height = height + block.height;
                    }
                }
            }
            height
        })
        .collect()
}

fn assemble_footnotes(
    page_items: &[usize],
    items: &[FlowItem],
    notes: &NoteMap,
) -> Vec<PlacedBlock> {
    let mut out = Vec::new();
    let mut y = Width::ZERO;
    for note in page_note_indices(page_items, items) {
        if let Some(blocks) = notes.blocks.get(note) {
            for block in blocks {
                let mut placed = block.clone();
                placed.y = y;
                y = y + placed.height;
                out.push(placed);
            }
        }
    }
    out
}

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
