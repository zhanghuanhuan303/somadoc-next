//! Deterministic vector diagrams (ADR-0102).
//!
//! The compiler turns a Mermaid flowchart or a Graphviz DOT source into the
//! exact same integer vector box TikZ produces: [`soma_tikz::TikzLayout`]
//! (`paths` + text items + rules). It never opens a font; the caller supplies a
//! [`TextEngine`] so labels use the document's own metrics and CJK rules.
//!
//! The parser is strict: an unsupported construct is an error and the layout
//! falls back to the original code, never to a wrong picture.

#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]

extern crate alloc;

mod dot;
mod ir;
mod layout;
mod mermaid;
mod shapes;
mod text;
mod tikz;

pub use layout::{layout, scale, DiagramLayout, Options};
pub use text::{LabelLine, TextEngine};
pub use tikz::to_tikz;

/// A short user-facing error. `line` is 1-based when the source has one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiagramError {
    pub line: u32,
    pub message: &'static str,
}

impl DiagramError {
    pub(crate) fn at(line: u32, message: &'static str) -> Self {
        Self { line, message }
    }
}

impl core::fmt::Display for DiagramError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        if self.line > 0 {
            write!(f, "line {}: {}", self.line, self.message)
        } else {
            f.write_str(self.message)
        }
    }
}
