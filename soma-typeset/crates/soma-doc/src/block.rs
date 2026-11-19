//! Block-level DocIR.
//!
//! Blocks describe structure and semantics. They never carry widths, fonts,
//! margins, or colours; those are computed by `soma-template` and materialised
//! by `soma-layout`.

use alloc::string::String;
use alloc::vec::Vec;

use crate::inline::{Image, Inline};
use crate::node::NodeMeta;
use crate::raw::RawContent;

/// One block node.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Block {
    Paragraph(Paragraph),
    Heading(Heading),
    List(List),
    BlockQuote(BlockQuote),
    CodeBlock(CodeBlock),
    MathBlock(MathBlock),
    Figure(Figure),
    Table(Table),
    ThematicBreak(ThematicBreak),
    PageBreak(PageBreak),
    /// Opens a section. Sections are markers in document order rather than a
    /// nested tree, so flattening for pagination and running headers is exact.
    Section(Section),
    Generated(GeneratedBlock),
    Raw(RawBlock),
}

impl Block {
    pub fn meta(&self) -> &NodeMeta {
        match self {
            Self::Paragraph(node) => &node.meta,
            Self::Heading(node) => &node.meta,
            Self::List(node) => &node.meta,
            Self::BlockQuote(node) => &node.meta,
            Self::CodeBlock(node) => &node.meta,
            Self::MathBlock(node) => &node.meta,
            Self::Figure(node) => &node.meta,
            Self::Table(node) => &node.meta,
            Self::ThematicBreak(node) => &node.meta,
            Self::PageBreak(node) => &node.meta,
            Self::Section(node) => &node.meta,
            Self::Generated(node) => &node.meta,
            Self::Raw(node) => &node.meta,
        }
    }

    pub fn meta_mut(&mut self) -> &mut NodeMeta {
        match self {
            Self::Paragraph(node) => &mut node.meta,
            Self::Heading(node) => &mut node.meta,
            Self::List(node) => &mut node.meta,
            Self::BlockQuote(node) => &mut node.meta,
            Self::CodeBlock(node) => &mut node.meta,
            Self::MathBlock(node) => &mut node.meta,
            Self::Figure(node) => &mut node.meta,
            Self::Table(node) => &mut node.meta,
            Self::ThematicBreak(node) => &mut node.meta,
            Self::PageBreak(node) => &mut node.meta,
            Self::Section(node) => &mut node.meta,
            Self::Generated(node) => &mut node.meta,
            Self::Raw(node) => &mut node.meta,
        }
    }

    /// Canonical JSON tag for this variant.
    pub const fn kind_str(&self) -> &'static str {
        match self {
            Self::Paragraph(_) => "paragraph",
            Self::Heading(_) => "heading",
            Self::List(_) => "list",
            Self::BlockQuote(_) => "blockQuote",
            Self::CodeBlock(_) => "codeBlock",
            Self::MathBlock(_) => "mathBlock",
            Self::Figure(_) => "figure",
            Self::Table(_) => "table",
            Self::ThematicBreak(_) => "thematicBreak",
            Self::PageBreak(_) => "pageBreak",
            Self::Section(_) => "section",
            Self::Generated(_) => "generated",
            Self::Raw(_) => "raw",
        }
    }
}

/// A run of inline content.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Paragraph {
    pub meta: NodeMeta,
    pub inlines: Vec<Inline>,
}

impl Paragraph {
    pub fn new(meta: NodeMeta, inlines: Vec<Inline>) -> Self {
        Self { meta, inlines }
    }
}

/// Section heading. `level` is the semantic outline level, 1..=6.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Heading {
    pub meta: NodeMeta,
    pub level: u8,
    pub inlines: Vec<Inline>,
}

impl Heading {
    pub fn new(meta: NodeMeta, level: u8, inlines: Vec<Inline>) -> Self {
        Self {
            meta,
            level,
            inlines,
        }
    }
}

/// Ordered or unordered list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct List {
    pub meta: NodeMeta,
    pub ordered: bool,
    /// First number for ordered lists. Must be 1 for unordered lists.
    pub start: u32,
    /// Markdown tight/loose distinction; templates use it for item spacing.
    pub tight: bool,
    pub items: Vec<ListItem>,
}

impl List {
    pub fn new(meta: NodeMeta, ordered: bool, start: u32, items: Vec<ListItem>) -> Self {
        Self {
            meta,
            ordered,
            start,
            tight: true,
            items,
        }
    }

    pub fn unordered(meta: NodeMeta, items: Vec<ListItem>) -> Self {
        Self::new(meta, false, 1, items)
    }
}

/// One list item. Item content is blocks, so nested lists and paragraphs work.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ListItem {
    pub meta: NodeMeta,
    pub blocks: Vec<Block>,
}

impl ListItem {
    pub fn new(meta: NodeMeta, blocks: Vec<Block>) -> Self {
        Self { meta, blocks }
    }
}

/// Block quotation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BlockQuote {
    pub meta: NodeMeta,
    pub blocks: Vec<Block>,
}

impl BlockQuote {
    pub fn new(meta: NodeMeta, blocks: Vec<Block>) -> Self {
        Self { meta, blocks }
    }
}

/// Verbatim code block.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CodeBlock {
    pub meta: NodeMeta,
    /// Info-string language tag, lowercase (`rust`, `c`, `text`).
    pub lang: Option<String>,
    pub value: String,
}

impl CodeBlock {
    pub fn new(meta: NodeMeta, lang: Option<String>, value: impl Into<String>) -> Self {
        Self {
            meta,
            lang,
            value: value.into(),
        }
    }
}

/// Display formula. `source` is passed to the math adapter unchanged.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MathBlock {
    pub meta: NodeMeta,
    pub source: String,
}

impl MathBlock {
    pub fn new(meta: NodeMeta, source: impl Into<String>) -> Self {
        Self {
            meta,
            source: source.into(),
        }
    }
}

/// Figure with an image and a caption.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Figure {
    pub meta: NodeMeta,
    pub image: Image,
    pub caption: Vec<Inline>,
    /// Cross-reference target (`fig:arch`). The visible number is generated by
    /// the template, never stored here.
    pub label: Option<String>,
}

impl Figure {
    pub fn new(meta: NodeMeta, image: Image) -> Self {
        Self {
            meta,
            image,
            caption: Vec::new(),
            label: None,
        }
    }
}

/// Table. Column *widths* are a layout decision and therefore absent; only
/// alignment hints and cell content live in the IR.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Table {
    pub meta: NodeMeta,
    pub columns: Vec<Column>,
    pub rows: Vec<Row>,
    pub caption: Vec<Inline>,
    pub label: Option<String>,
}

impl Table {
    pub fn new(meta: NodeMeta, columns: Vec<Column>, rows: Vec<Row>) -> Self {
        Self {
            meta,
            columns,
            rows,
            caption: Vec::new(),
            label: None,
        }
    }
}

/// Horizontal cell alignment hint.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CellAlign {
    /// Template default.
    #[default]
    Default,
    Left,
    Center,
    Right,
}

impl CellAlign {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::Left => "left",
            Self::Center => "center",
            Self::Right => "right",
        }
    }
}

/// Column description.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Column {
    pub meta: NodeMeta,
    pub align: CellAlign,
}

impl Column {
    pub fn new(meta: NodeMeta, align: CellAlign) -> Self {
        Self { meta, align }
    }
}

/// Table row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub meta: NodeMeta,
    /// Header row (rendered with the header style, repeated across pages).
    pub header: bool,
    pub cells: Vec<Cell>,
}

impl Row {
    pub fn new(meta: NodeMeta, header: bool, cells: Vec<Cell>) -> Self {
        Self {
            meta,
            header,
            cells,
        }
    }
}

/// Table cell. Cell content is blocks, so a cell can hold paragraphs, lists, or
/// nested tables.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cell {
    pub meta: NodeMeta,
    pub colspan: u16,
    pub rowspan: u16,
    pub blocks: Vec<Block>,
}

impl Cell {
    pub fn new(meta: NodeMeta, blocks: Vec<Block>) -> Self {
        Self {
            meta,
            colspan: 1,
            rowspan: 1,
            blocks,
        }
    }

    pub fn with_spans(mut self, colspan: u16, rowspan: u16) -> Self {
        self.colspan = colspan;
        self.rowspan = rowspan;
        self
    }
}

/// Horizontal rule.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ThematicBreak {
    pub meta: NodeMeta,
}

impl ThematicBreak {
    pub fn new(meta: NodeMeta) -> Self {
        Self { meta }
    }
}

/// Author-requested page break.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PageBreak {
    pub meta: NodeMeta,
}

impl PageBreak {
    pub fn new(meta: NodeMeta) -> Self {
        Self { meta }
    }
}

/// Section marker.
///
/// Numbers, running headers, and page templates are resolved by the template and
/// pagination layer; the IR only states that a section starts here, at which
/// level, whether it is numbered, and which page master it asks for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Section {
    pub meta: NodeMeta,
    pub level: u8,
    pub numbered: bool,
    /// Page master name from the template, if the section switches one.
    pub page_template: Option<String>,
    /// Section title when it is not carried by a following heading.
    pub title: Option<Vec<Inline>>,
}

impl Section {
    pub fn new(meta: NodeMeta, level: u8) -> Self {
        Self {
            meta,
            level,
            numbered: true,
            page_template: None,
            title: None,
        }
    }
}

/// Content produced by the document/template stage rather than the source:
/// table of contents, index, bibliography, cover.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GeneratedKind {
    Toc,
    ListOfFigures,
    ListOfTables,
    Index,
    Bibliography,
    Cover,
    Custom,
}

impl GeneratedKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Toc => "toc",
            Self::ListOfFigures => "listOfFigures",
            Self::ListOfTables => "listOfTables",
            Self::Index => "index",
            Self::Bibliography => "bibliography",
            Self::Cover => "cover",
            Self::Custom => "custom",
        }
    }
}

/// Placeholder that a later stage fills, optionally with pre-filled blocks.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GeneratedBlock {
    pub meta: NodeMeta,
    pub kind: GeneratedKind,
    /// Slot name; required for [`GeneratedKind::Custom`].
    pub slot: Option<String>,
    pub blocks: Vec<Block>,
}

impl GeneratedBlock {
    pub fn new(meta: NodeMeta, kind: GeneratedKind) -> Self {
        Self {
            meta,
            kind,
            slot: None,
            blocks: Vec::new(),
        }
    }

    pub fn with_slot(mut self, slot: impl Into<String>) -> Self {
        self.slot = Some(slot.into());
        self
    }
}

/// Block payload DocIR cannot model.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RawBlock {
    pub meta: NodeMeta,
    pub content: RawContent,
}

impl RawBlock {
    pub fn new(meta: NodeMeta, content: RawContent) -> Self {
        Self { meta, content }
    }
}

impl From<Paragraph> for Block {
    fn from(node: Paragraph) -> Self {
        Self::Paragraph(node)
    }
}

impl From<Heading> for Block {
    fn from(node: Heading) -> Self {
        Self::Heading(node)
    }
}

impl From<List> for Block {
    fn from(node: List) -> Self {
        Self::List(node)
    }
}

impl From<BlockQuote> for Block {
    fn from(node: BlockQuote) -> Self {
        Self::BlockQuote(node)
    }
}

impl From<CodeBlock> for Block {
    fn from(node: CodeBlock) -> Self {
        Self::CodeBlock(node)
    }
}

impl From<MathBlock> for Block {
    fn from(node: MathBlock) -> Self {
        Self::MathBlock(node)
    }
}

impl From<Figure> for Block {
    fn from(node: Figure) -> Self {
        Self::Figure(node)
    }
}

impl From<Table> for Block {
    fn from(node: Table) -> Self {
        Self::Table(node)
    }
}

impl From<ThematicBreak> for Block {
    fn from(node: ThematicBreak) -> Self {
        Self::ThematicBreak(node)
    }
}

impl From<PageBreak> for Block {
    fn from(node: PageBreak) -> Self {
        Self::PageBreak(node)
    }
}

impl From<Section> for Block {
    fn from(node: Section) -> Self {
        Self::Section(node)
    }
}

impl From<GeneratedBlock> for Block {
    fn from(node: GeneratedBlock) -> Self {
        Self::Generated(node)
    }
}

impl From<RawBlock> for Block {
    fn from(node: RawBlock) -> Self {
        Self::Raw(node)
    }
}
