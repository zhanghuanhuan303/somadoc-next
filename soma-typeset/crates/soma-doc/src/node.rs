//! The per-node header shared by blocks, inlines, table parts, and footnotes.

use crate::attrs::Attrs;
use crate::ids::{NodeId, SourceSpan};

/// Identity, provenance, and semantic attributes of one node.
///
/// Using one header everywhere keeps renumbering, span mapping, and validation
/// uniform; it also means adding a node kind never adds bookkeeping fields.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NodeMeta {
    pub id: NodeId,
    pub span: SourceSpan,
    pub attrs: Attrs,
}

impl NodeMeta {
    /// A node with no source text: generated content, synthesized fallback, or
    /// test fixtures. Ids stay unassigned until [`crate::Document::renumber`].
    pub fn synthesized() -> Self {
        Self::default()
    }

    /// A node that came from source bytes.
    pub fn at(span: SourceSpan) -> Self {
        Self {
            id: NodeId::UNASSIGNED,
            span,
            attrs: Attrs::new(),
        }
    }

    pub fn with_id(mut self, id: NodeId) -> Self {
        self.id = id;
        self
    }

    pub fn with_attrs(mut self, attrs: Attrs) -> Self {
        self.attrs = attrs;
        self
    }

    pub fn with_role(mut self, role: impl Into<alloc::string::String>) -> Self {
        self.attrs.role = Some(role.into());
        self
    }

    pub fn with_anchor(mut self, anchor: impl Into<alloc::string::String>) -> Self {
        self.attrs.anchor = Some(anchor.into());
        self
    }
}
