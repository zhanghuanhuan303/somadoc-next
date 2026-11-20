//! Metrics adapters that turn a real font table into the kernel seam.
//!
//! Two adapters live here:
//!
//! - [`TableMetrics`] measures everything at one face and one size. It is the
//!   minimal adapter for demos and kernel-level experiments.
//! - [`StyledMetrics`] maps each `StyleId` to a face and a font size through the
//!   resolved style table, so headings and inline emphasis measure at their own
//!   size. This is the adapter the document pipeline uses.
//!
//! Both are pure functions of `(table, style)`; no state, no cache, no fonts.

use soma_core::{LineMetrics, Metrics, StyleId, Width};
use soma_metrics::{Face, MetricsTable};
use soma_template::{ComputedStyle, FontWeight, StyledDoc};

/// Em size used by the kernel: one em is 4096 units.
const EM: i64 = 4096;

/// One face at one font size.
pub struct TableMetrics<'a> {
    table: &'a MetricsTable,
    face: usize,
    size: Width,
}

impl<'a> TableMetrics<'a> {
    pub fn new(table: &'a MetricsTable, face: usize, size: Width) -> Self {
        Self { table, face, size }
    }

    fn face(&self) -> Option<&soma_metrics::Face> {
        self.table.face(self.face)
    }
}

impl Metrics for TableMetrics<'_> {
    fn advance(&self, text: &str, _style: StyleId) -> Width {
        match self.face() {
            Some(face) => Width(scale(face.advance(text), self.size.0)),
            None => Width::ZERO,
        }
    }

    fn line_metrics(&self, _style: StyleId) -> LineMetrics {
        match self.face() {
            Some(face) => LineMetrics {
                ascent: Width(scale(face.ascent, self.size.0)),
                descent: Width(scale(face.descent, self.size.0)),
                leading: Width(scale(face.leading, self.size.0)),
            },
            None => LineMetrics::default(),
        }
    }

    fn space_width(&self, style: StyleId) -> Width {
        match self.face() {
            Some(face) => Width(scale(face.space, self.size.0)),
            None => self.advance(" ", style),
        }
    }

    fn math_advance(&self, text: &str, style: StyleId) -> Width {
        match self.table.math_face() {
            Some(face) => Width(scale(face.advance(text), self.size.0)),
            None => self.advance(text, style),
        }
    }

    fn math_metrics(&self, style: StyleId) -> LineMetrics {
        match self.table.math_face() {
            Some(face) => LineMetrics {
                ascent: Width(scale(face.ascent, self.size.0)),
                descent: Width(scale(face.descent, self.size.0)),
                leading: Width::ZERO,
            },
            None => self.line_metrics(style),
        }
    }

    fn ink_right(&self, text: &str, _style: StyleId) -> Option<Width> {
        let face = self.face()?;
        let codepoint = last_codepoint(text)?;
        face.ink(codepoint)
            .map(|(_, right)| Width(scale(right, self.size.0)))
    }

    fn ink_left(&self, text: &str, _style: StyleId) -> Option<Width> {
        let face = self.face()?;
        let codepoint = first_codepoint(text)?;
        face.ink(codepoint)
            .map(|(left, _)| Width(scale(left, self.size.0)))
    }

    fn version(&self) -> u16 {
        soma_metrics::VERSION
    }

    fn hash(&self) -> u64 {
        self.table.hash()
    }
}

/// Per-style metrics from the resolved style table.
pub struct StyledMetrics<'a> {
    table: &'a MetricsTable,
    styled: &'a StyledDoc,
}

impl<'a> StyledMetrics<'a> {
    pub fn new(table: &'a MetricsTable, styled: &'a StyledDoc) -> Self {
        Self { table, styled }
    }

    pub fn table(&self) -> &MetricsTable {
        self.table
    }

    fn computed(&self, style: StyleId) -> ComputedStyle {
        self.styled
            .style(style)
            .cloned()
            .unwrap_or_else(ComputedStyle::base)
    }

    /// The face that draws `text`: the style's own face when it has every
    /// glyph, otherwise the first rescue face that does. `None` when even the
    /// rescue faces cannot draw it, and the caller falls back to the style face
    /// (which will render `.notdef`, but the widths stay sane).
    fn face_for(&self, text: &str, computed: &ComputedStyle) -> Option<&soma_metrics::Face> {
        let primary = self.face_of(computed)?;
        if primary.covers(text) {
            return Some(primary);
        }
        self.table
            .faces()
            .iter()
            .filter(|face| face.family == soma_metrics::FAMILY_FALLBACK)
            .find(|face| face.covers(text))
            // No rescue face either: keep the style's face so widths stay
            // sane. The glyph will render as .notdef, which is visible.
            .or(Some(primary))
    }

    fn face_of(&self, computed: &ComputedStyle) -> Option<&Face> {
        // Search by family and weight rather than by slot: a table written in
        // any order then still answers with the face the style asked for.
        let family = if soma_template::family_is_sans(computed.family.as_deref()) {
            soma_metrics::FAMILY_SANS
        } else {
            soma_metrics::FAMILY_SERIF
        };
        let bold = computed.weight == FontWeight::Bold;
        let faces = self.table.faces();
        faces
            .iter()
            .find(|face| face.family == family && (face.weight != 0) == bold)
            .or_else(|| faces.iter().find(|face| face.family == family))
            .or_else(|| self.table.face(0))
    }
}

impl Metrics for StyledMetrics<'_> {
    fn advance(&self, text: &str, style: StyleId) -> Width {
        let computed = self.computed(style);
        match self.face_for(text, &computed) {
            Some(face) => Width(scale(face.advance(text), computed.font_size.0)),
            None => Width::ZERO,
        }
    }

    fn line_metrics(&self, style: StyleId) -> LineMetrics {
        let computed = self.computed(style);
        match self.face_of(&computed) {
            Some(face) => LineMetrics {
                ascent: Width(scale(face.ascent, computed.font_size.0)),
                descent: Width(scale(face.descent, computed.font_size.0)),
                leading: Width(scale(face.leading, computed.font_size.0)),
            },
            None => LineMetrics::default(),
        }
    }

    fn glyph_metrics(&self, text: &str, style: StyleId) -> soma_core::GlyphMetrics {
        let computed = self.computed(style);
        match self.face_for(text, &computed) {
            Some(face) => soma_core::GlyphMetrics {
                advance: Width(scale(face.advance(text), computed.font_size.0)),
                ascent: Width(scale(face.ascent, computed.font_size.0)),
                descent: Width(scale(face.descent, computed.font_size.0)),
                leading: Width(scale(face.leading, computed.font_size.0)),
                ..soma_core::GlyphMetrics::default()
            },
            None => soma_core::GlyphMetrics::default(),
        }
    }

    fn space_width(&self, style: StyleId) -> Width {
        let computed = self.computed(style);
        match self.face_of(&computed) {
            Some(face) => Width(scale(face.space, computed.font_size.0)),
            None => self.advance(" ", style),
        }
    }

    fn math_advance(&self, text: &str, style: StyleId) -> Width {
        let computed = self.computed(style);
        match self.table.math_face() {
            Some(face) => Width(scale(face.advance(text), computed.font_size.0)),
            None => self.advance(text, style),
        }
    }

    fn math_metrics(&self, style: StyleId) -> LineMetrics {
        let computed = self.computed(style);
        match self.table.math_face() {
            Some(face) => LineMetrics {
                ascent: Width(scale(face.ascent, computed.font_size.0)),
                descent: Width(scale(face.descent, computed.font_size.0)),
                leading: Width::ZERO,
            },
            None => self.line_metrics(style),
        }
    }

    fn uses_fallback(&self, text: &str, style: StyleId) -> bool {
        let computed = self.computed(style);
        match (self.face_of(&computed), self.face_for(text, &computed)) {
            (Some(primary), Some(drawn)) => !core::ptr::eq(primary, drawn),
            _ => false,
        }
    }

    fn ink_right(&self, text: &str, style: StyleId) -> Option<Width> {
        let computed = self.computed(style);
        let face = self.face_for(text, &computed)?;
        let codepoint = last_codepoint(text)?;
        face.ink(codepoint)
            .map(|(_, right)| Width(scale(right, computed.font_size.0)))
    }

    fn ink_left(&self, text: &str, style: StyleId) -> Option<Width> {
        let computed = self.computed(style);
        let face = self.face_for(text, &computed)?;
        let codepoint = first_codepoint(text)?;
        face.ink(codepoint)
            .map(|(left, _)| Width(scale(left, computed.font_size.0)))
    }

    fn version(&self) -> u16 {
        soma_metrics::VERSION
    }

    fn hash(&self) -> u64 {
        self.table.hash()
    }
}

/// The first glyph of an atom. A Latin atom is a whole run; its leading edge is
/// the first character's.
fn first_codepoint(text: &str) -> Option<u32> {
    text.chars().next().map(|character| character as u32)
}

/// The last glyph of an atom; its trailing edge is what a line end trims.
fn last_codepoint(text: &str) -> Option<u32> {
    text.chars().last().map(|character| character as u32)
}

/// Scale a table value (1/4096 em) by a font size (also 1/4096 em).
fn scale(value: i32, size: i32) -> i32 {
    let scaled = value as i64 * size as i64 / EM;
    if scaled > i32::MAX as i64 {
        i32::MAX
    } else if scaled < i32::MIN as i64 {
        i32::MIN
    } else {
        scaled as i32
    }
}
