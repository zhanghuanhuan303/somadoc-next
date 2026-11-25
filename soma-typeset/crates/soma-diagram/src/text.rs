//! The text seam: the caller measures and wraps node/edge labels.
//!
//! `soma-diagram` never opens a font. The layout supplies an engine backed by
//! the document metrics and CJK rules, so a label is wrapped with the same
//! quality as body text.

use alloc::string::String;
use alloc::vec::Vec;

use soma_core::Width;
use soma_math::{MathItem, MathRule};

/// One laid-out label line. Items and rules are baseline-relative; `x = 0` is
/// the left edge of the line.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LabelLine {
    pub text: String,
    pub width: Width,
    pub ascent: Width,
    pub descent: Width,
    pub items: Vec<MathItem>,
    pub rules: Vec<MathRule>,
}

/// The caller's label layout.
pub trait TextEngine {
    /// Lay out one line without wrapping.
    fn line(&mut self, text: &str) -> LabelLine;

    /// Break `text` into lines no wider than `max_width`, reusing the caller's
    /// CJK/Latin rules. A single unbreakable word may overflow.
    fn wrap(&mut self, text: &str, max_width: Width) -> Vec<LabelLine>;
}
