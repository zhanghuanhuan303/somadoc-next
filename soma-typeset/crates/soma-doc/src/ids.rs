//! Identifiers and source mapping.
//!
//! Every DocIR node carries a [`NodeId`] and a [`SourceSpan`]. Ids are assigned
//! by [`crate::Document::renumber`] or by a front end; they must be unique and
//! non-zero. Spans are byte offsets into a named source entry, which keeps
//! diagnostics and incremental layout addressable without carrying source text
//! inside the IR.

use alloc::string::String;
use alloc::vec::Vec;

/// Stable, document-local node identifier.
///
/// `NodeId(0)` means "not assigned". Construction is cheap and explicit; the
/// alternative (auto-assignment inside constructors) hides ordering bugs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NodeId(pub u32);

impl NodeId {
    /// The placeholder used before [`crate::Document::renumber`] runs.
    pub const UNASSIGNED: Self = Self(0);

    pub const fn new(raw: u32) -> Self {
        Self(raw)
    }

    pub const fn is_assigned(self) -> bool {
        self.0 != 0
    }
}

/// Identifies one input source (file, fragment, or synthesized origin).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SourceId(pub u16);

impl SourceId {
    /// Used for content that has no source text (generated blocks, synthesized
    /// fallbacks, tests).
    pub const UNKNOWN: Self = Self(0);

    pub const fn new(raw: u16) -> Self {
        Self(raw)
    }

    pub const fn is_known(self) -> bool {
        self.0 != 0
    }
}

/// A byte range inside one source.
///
/// `start == end` is a valid zero-width position (for example the position of a
/// generated marker). `start > end` is rejected by validation.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct SourceSpan {
    pub source: SourceId,
    pub start: u32,
    pub end: u32,
}

impl SourceSpan {
    pub const UNKNOWN: Self = Self {
        source: SourceId::UNKNOWN,
        start: 0,
        end: 0,
    };

    pub const fn at(source: SourceId, start: u32, end: u32) -> Self {
        Self { source, start, end }
    }

    /// A zero-width span inside a known source.
    pub const fn point(source: SourceId, offset: u32) -> Self {
        Self {
            source,
            start: offset,
            end: offset,
        }
    }

    pub const fn is_known(self) -> bool {
        self.source.is_known()
    }

    pub const fn is_empty(self) -> bool {
        self.start == self.end
    }

    pub const fn len(self) -> u32 {
        self.end.saturating_sub(self.start)
    }
}

/// How a source was produced. Kept as data so diagnostics can name the front end
/// without the IR depending on any front-end crate.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SourceKind {
    #[default]
    Unknown,
    Markdown,
    Html,
    Latex,
    Word,
    /// Built programmatically (template cover, generated TOC, tests).
    Synthetic,
}

impl SourceKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Unknown => "unknown",
            Self::Markdown => "markdown",
            Self::Html => "html",
            Self::Latex => "latex",
            Self::Word => "word",
            Self::Synthetic => "synthetic",
        }
    }
}

/// One named input.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceInfo {
    pub id: SourceId,
    pub name: String,
    pub kind: SourceKind,
}

impl SourceInfo {
    pub fn new(id: SourceId, name: impl Into<String>, kind: SourceKind) -> Self {
        Self {
            id,
            name: name.into(),
            kind,
        }
    }
}

/// Deterministic, insertion-ordered list of sources.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SourceMap {
    pub entries: Vec<SourceInfo>,
}

impl SourceMap {
    pub const fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    pub fn push(&mut self, info: SourceInfo) {
        self.entries.push(info);
    }

    pub fn get(&self, id: SourceId) -> Option<&SourceInfo> {
        self.entries.iter().find(|entry| entry.id == id)
    }

    pub fn contains(&self, id: SourceId) -> bool {
        self.get(id).is_some()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }
}
