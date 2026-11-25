//! `soma-doc`: clean-room semantic document IR (DocIR).
//!
//! This crate owns "what the document is": document structure, inline semantics,
//! metadata, footnotes, figures, tables, and math source. It owns no physical
//! property: no width, font, margin, colour, or page geometry. Those belong to
//! `soma-template` (presentation) and `soma-layout` (boxes and pages).
//!
//! Design rules (see `docs/architecture-v2.md` §5 and ADR-0001/0008/0009):
//!
//! - every node carries a [`NodeId`], a [`SourceSpan`], and [`Attrs`];
//! - unknown source constructs survive as [`RawContent`] with a fallback;
//! - canonical JSON is written by one hand-written writer, never a map-backed
//!   serializer, so byte output is stable across native and wasm;
//! - no floats, no filesystem, no font, no serde.
//!
//! ```no_run
//! use soma_doc::{Block, Document, DocumentMeta, Paragraph, Text};
//! use soma_doc::node::NodeMeta;
//!
//! let mut document = Document::new(DocumentMeta::new());
//! document.push_block(Block::Paragraph(Paragraph::new(
//!     NodeMeta::synthesized(),
//!     vec![Text::new(NodeMeta::synthesized(), "正文").into()],
//! )));
//! document.renumber();
//! document.validate().expect("valid document");
//! ```

#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]

extern crate alloc;

pub mod attrs;
pub mod block;
pub mod canonical;
pub mod doc;
pub mod hash;
pub mod ids;
pub mod inline;
pub mod meta;
pub mod node;
pub mod raw;
pub mod text;
pub mod validate;

pub use attrs::{Attrs, MAX_ATTR_LEN, MAX_CLASSES};
pub use block::{
    Block, BlockQuote, Cell, CellAlign, CodeBlock, Column, Figure, GeneratedBlock, GeneratedKind,
    Heading, List, ListItem, MathBlock, PageBreak, Paragraph, RawBlock, Row, Section, Table,
    ThematicBreak,
};
pub use canonical::CanonicalMode;
pub use doc::{Document, FootnoteDef, DOC_IR_VERSION, SCHEMA};
pub use hash::{content_hash, document_hash, fnv1a_64, DocHash};
pub use ids::{NodeId, SourceId, SourceInfo, SourceKind, SourceMap, SourceSpan};
pub use inline::{
    Code, Container, FootnoteRef, Image, IndexRef, Inline, LineBreak, LineBreakKind, Link,
    MathInline, RawInline, Ruby, Text,
};
pub use meta::{
    Author, CoverData, Date, DocumentMeta, ImageIntrinsic, ImageRef, MetaField, TemplateRef,
};
pub use node::NodeMeta;
pub use raw::{RawCapability, RawContent};
pub use validate::{DocError, MAX_DEPTH};
