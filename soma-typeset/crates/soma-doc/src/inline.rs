//! Inline-level DocIR.
//!
//! Inlines are semantic only: emphasis, links, code, math source, ruby. Font
//! size, colour, underline position, and superscript shift belong to the
//! template and the layout box tree, not here.

use alloc::string::String;
use alloc::vec::Vec;

use crate::meta::ImageRef;
use crate::node::NodeMeta;
use crate::raw::RawContent;

/// One inline node.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Inline {
    Text(Text),
    Code(Code),
    Strong(Container),
    Emphasis(Container),
    Underline(Container),
    Strikethrough(Container),
    SmallCaps(Container),
    Superscript(Container),
    Subscript(Container),
    Link(Link),
    Image(Image),
    Math(MathInline),
    FootnoteRef(FootnoteRef),
    Index(IndexRef),
    Ruby(Ruby),
    LineBreak(LineBreak),
    Raw(RawInline),
}

impl Inline {
    pub fn meta(&self) -> &NodeMeta {
        match self {
            Self::Text(node) => &node.meta,
            Self::Code(node) => &node.meta,
            Self::Strong(node)
            | Self::Emphasis(node)
            | Self::Underline(node)
            | Self::Strikethrough(node)
            | Self::SmallCaps(node)
            | Self::Superscript(node)
            | Self::Subscript(node) => &node.meta,
            Self::Link(node) => &node.meta,
            Self::Image(node) => &node.meta,
            Self::Math(node) => &node.meta,
            Self::FootnoteRef(node) => &node.meta,
            Self::Index(node) => &node.meta,
            Self::Ruby(node) => &node.meta,
            Self::LineBreak(node) => &node.meta,
            Self::Raw(node) => &node.meta,
        }
    }

    pub fn meta_mut(&mut self) -> &mut NodeMeta {
        match self {
            Self::Text(node) => &mut node.meta,
            Self::Code(node) => &mut node.meta,
            Self::Strong(node)
            | Self::Emphasis(node)
            | Self::Underline(node)
            | Self::Strikethrough(node)
            | Self::SmallCaps(node)
            | Self::Superscript(node)
            | Self::Subscript(node) => &mut node.meta,
            Self::Link(node) => &mut node.meta,
            Self::Image(node) => &mut node.meta,
            Self::Math(node) => &mut node.meta,
            Self::FootnoteRef(node) => &mut node.meta,
            Self::Index(node) => &mut node.meta,
            Self::Ruby(node) => &mut node.meta,
            Self::LineBreak(node) => &mut node.meta,
            Self::Raw(node) => &mut node.meta,
        }
    }

    /// Canonical JSON tag for this variant.
    pub const fn kind_str(&self) -> &'static str {
        match self {
            Self::Text(_) => "text",
            Self::Code(_) => "code",
            Self::Strong(_) => "strong",
            Self::Emphasis(_) => "emphasis",
            Self::Underline(_) => "underline",
            Self::Strikethrough(_) => "strikethrough",
            Self::SmallCaps(_) => "smallCaps",
            Self::Superscript(_) => "superscript",
            Self::Subscript(_) => "subscript",
            Self::Link(_) => "link",
            Self::Image(_) => "image",
            Self::Math(_) => "math",
            Self::FootnoteRef(_) => "footnoteRef",
            Self::Index(_) => "index",
            Self::Ruby(_) => "ruby",
            Self::LineBreak(_) => "lineBreak",
            Self::Raw(_) => "raw",
        }
    }

    /// Direct children, if this variant has any.
    pub fn children(&self) -> &[Inline] {
        match self {
            Self::Strong(node)
            | Self::Emphasis(node)
            | Self::Underline(node)
            | Self::Strikethrough(node)
            | Self::SmallCaps(node)
            | Self::Superscript(node)
            | Self::Subscript(node) => &node.children,
            Self::Link(node) => &node.children,
            _ => &[],
        }
    }

    pub fn children_mut(&mut self) -> Option<&mut Vec<Inline>> {
        match self {
            Self::Strong(node)
            | Self::Emphasis(node)
            | Self::Underline(node)
            | Self::Strikethrough(node)
            | Self::SmallCaps(node)
            | Self::Superscript(node)
            | Self::Subscript(node) => Some(&mut node.children),
            Self::Link(node) => Some(&mut node.children),
            _ => None,
        }
    }
}

/// Literal text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Text {
    pub meta: NodeMeta,
    pub value: String,
}

impl Text {
    pub fn new(meta: NodeMeta, value: impl Into<String>) -> Self {
        Self {
            meta,
            value: value.into(),
        }
    }
}

/// Verbatim inline code. Its value is not segmented or hyphenated by the rules
/// layer; the box tree keeps it as one unbreakable unit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Code {
    pub meta: NodeMeta,
    pub value: String,
}

impl Code {
    pub fn new(meta: NodeMeta, value: impl Into<String>) -> Self {
        Self {
            meta,
            value: value.into(),
        }
    }
}

/// A node that only carries children (`Strong`, `Emphasis`, ...).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Container {
    pub meta: NodeMeta,
    pub children: Vec<Inline>,
}

impl Container {
    pub fn new(meta: NodeMeta, children: Vec<Inline>) -> Self {
        Self { meta, children }
    }
}

/// Hyperlink.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Link {
    pub meta: NodeMeta,
    pub href: String,
    pub title: Option<String>,
    pub children: Vec<Inline>,
}

impl Link {
    pub fn new(meta: NodeMeta, href: impl Into<String>, children: Vec<Inline>) -> Self {
        Self {
            meta,
            href: href.into(),
            title: None,
            children,
        }
    }
}

/// Inline image.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Image {
    pub meta: NodeMeta,
    pub source: ImageRef,
}

impl Image {
    pub fn new(meta: NodeMeta, source: ImageRef) -> Self {
        Self { meta, source }
    }
}

/// Inline formula. `source` is whatever the math adapter understands; the core
/// never parses it (ADR: math is an adapter).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MathInline {
    pub meta: NodeMeta,
    pub source: String,
}

impl MathInline {
    pub fn new(meta: NodeMeta, source: impl Into<String>) -> Self {
        Self {
            meta,
            source: source.into(),
        }
    }
}

/// Reference to a footnote definition.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FootnoteRef {
    pub meta: NodeMeta,
    /// Matches [`crate::FootnoteDef::label`].
    pub target: String,
    /// Marker as written in the source (`1`, `*`, `注`). Numbering is generated
    /// later; this is only what the author typed.
    pub label: Option<String>,
}

/// An index marker: it occupies no space, it records that `term` occurs at this
/// position. Layout collects the positions and the template decides whether an
/// index is printed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IndexRef {
    pub meta: NodeMeta,
    /// Display term. Sorting and cross-references are not part of DocIR.
    pub term: String,
}

impl IndexRef {
    pub fn new(meta: NodeMeta, term: impl Into<String>) -> Self {
        Self {
            meta,
            term: term.into(),
        }
    }
}

impl FootnoteRef {
    pub fn new(meta: NodeMeta, target: impl Into<String>) -> Self {
        Self {
            meta,
            target: target.into(),
            label: None,
        }
    }

    pub fn with_label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }
}

/// Ruby annotation (furigana / 注音). `base` and `annotation` are plain strings:
/// ruby is a pronunciation annotation, not a rich-text container.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ruby {
    pub meta: NodeMeta,
    pub base: String,
    pub annotation: String,
}

impl Ruby {
    pub fn new(meta: NodeMeta, base: impl Into<String>, annotation: impl Into<String>) -> Self {
        Self {
            meta,
            base: base.into(),
            annotation: annotation.into(),
        }
    }
}

/// Soft or hard line break inside a paragraph.
///
/// A hard break becomes a `Forced` boundary in the paragraph IR (K5).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineBreakKind {
    Soft,
    Hard,
}

impl LineBreakKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Soft => "soft",
            Self::Hard => "hard",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LineBreak {
    pub meta: NodeMeta,
    pub kind: LineBreakKind,
}

impl LineBreak {
    pub fn new(meta: NodeMeta, kind: LineBreakKind) -> Self {
        Self { meta, kind }
    }
}

/// Inline payload DocIR cannot model.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RawInline {
    pub meta: NodeMeta,
    pub content: RawContent,
}

impl RawInline {
    pub fn new(meta: NodeMeta, content: RawContent) -> Self {
        Self { meta, content }
    }
}

// Unambiguous conversions. Containers (`Strong`, `Emphasis`, ...) deliberately
// have no `From` impl: the same `Container` cannot infer which emphasis it is.
impl From<Text> for Inline {
    fn from(node: Text) -> Self {
        Self::Text(node)
    }
}

impl From<Code> for Inline {
    fn from(node: Code) -> Self {
        Self::Code(node)
    }
}

impl From<Link> for Inline {
    fn from(node: Link) -> Self {
        Self::Link(node)
    }
}

impl From<Image> for Inline {
    fn from(node: Image) -> Self {
        Self::Image(node)
    }
}

impl From<MathInline> for Inline {
    fn from(node: MathInline) -> Self {
        Self::Math(node)
    }
}

impl From<IndexRef> for Inline {
    fn from(value: IndexRef) -> Self {
        Self::Index(value)
    }
}

impl From<FootnoteRef> for Inline {
    fn from(node: FootnoteRef) -> Self {
        Self::FootnoteRef(node)
    }
}

impl From<Ruby> for Inline {
    fn from(node: Ruby) -> Self {
        Self::Ruby(node)
    }
}

impl From<LineBreak> for Inline {
    fn from(node: LineBreak) -> Self {
        Self::LineBreak(node)
    }
}

impl From<RawInline> for Inline {
    fn from(node: RawInline) -> Self {
        Self::Raw(node)
    }
}
