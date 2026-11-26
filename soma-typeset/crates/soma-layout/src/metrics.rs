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

use soma_core::{LineMetrics, MathParam, MathVariant, Metrics, StyleId, Width};
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

    fn math_param(&self, param: MathParam, _style: StyleId) -> Option<i32> {
        self.table.math_face()?.math().constant(param.index())
    }

    fn math_ink(&self, text: &str, _style: StyleId) -> Option<(Width, Width)> {
        math_ink(self.table.math_face()?, text, self.size)
    }

    fn math_italic(&self, text: &str, _style: StyleId) -> Width {
        match self.table.math_face() {
            Some(face) => math_italic(face, text, self.size),
            None => Width::ZERO,
        }
    }

    fn math_variant_count(&self, text: &str, _style: StyleId) -> u16 {
        math_variant_count(self.table.math_face(), text)
    }

    fn math_variant(&self, text: &str, index: u16, _style: StyleId) -> Option<MathVariant> {
        math_variant_of(self.table.math_face()?, text, index, self.size)
    }

    fn math_top_accent(&self, text: &str, _style: StyleId) -> Option<Width> {
        math_top_accent(self.table.math_face()?, text, self.size)
    }

    fn math_has_glyph(&self, character: char, _style: StyleId) -> bool {
        self.table
            .math_face()
            .is_some_and(|face| face.has(character as u32))
    }

    fn math_glyph_bottom(&self, text: &str, _style: StyleId) -> Option<Width> {
        math_glyph_bottom(self.table.math_face()?, text, self.size)
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
    /// glyph, otherwise the first rescue face that does, then the math face
    /// (which carries most symbols a text face omits, such as the micro sign).
    /// `None` when nothing can draw it, and the caller falls back to the style
    /// face (which will render `.notdef`, but the widths stay sane).
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
            .or_else(|| self.table.math_face().filter(|face| face.covers(text)))
            // No rescue face either: keep the style's face so widths stay
            // sane. The glyph will render as .notdef, which is visible.
            .or(Some(primary))
    }

    fn face_of(&self, computed: &ComputedStyle) -> Option<&Face> {
        // Search by family and weight rather than by slot: a table written in
        // any order then still answers with the face the style asked for.
        let role = soma_template::family_role(computed.family.as_deref());
        let family = match role {
            soma_template::FamilyRole::Serif => soma_metrics::FAMILY_SERIF,
            soma_template::FamilyRole::Sans => soma_metrics::FAMILY_SANS,
            soma_template::FamilyRole::Mono => soma_metrics::FAMILY_MONO,
        };
        let bold = computed.weight == FontWeight::Bold;
        let faces = self.table.faces();
        let by_family = |family: u8| {
            faces
                .iter()
                .find(|face| face.family == family && (face.weight != 0) == bold)
                .or_else(|| faces.iter().find(|face| face.family == family))
        };
        // A table measured without a mono face still has to lay out code: a
        // mono template falls back to the sans face, then to the body face.
        by_family(family)
            .or_else(|| {
                (role == soma_template::FamilyRole::Mono)
                    .then(|| by_family(soma_metrics::FAMILY_SANS))
                    .flatten()
            })
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

    fn has_glyph(&self, character: char, style: StyleId) -> bool {
        // A formula draws from the math face, whose coverage differs from the
        // body face's; "can this document draw the character" is the union.
        let computed = self.computed(style);
        let math = self
            .table
            .math_face()
            .is_some_and(|face| face.has(character as u32));
        math || self
            .face_of(&computed)
            .is_some_and(|face| face.has(character as u32))
    }

    fn math_advance(&self, text: &str, style: StyleId) -> Width {
        let computed = self.computed(style);
        let Some(math) = self.table.math_face() else {
            return self.advance(text, style);
        };
        // A `\text{...}` run inside a formula is drawn from the *text* face:
        // the math face carries no Han glyphs, and the PDF writer picks the
        // face per character. Measure the same way or the formula's width and
        // the drawn positions disagree.
        let body = self.face_of(&computed);
        let rescue = self
            .table
            .faces()
            .iter()
            .find(|face| face.family == soma_metrics::FAMILY_FALLBACK);
        let mut total: i64 = 0;
        for character in text.chars() {
            let codepoint = character as u32;
            let face = if math.has(codepoint) {
                math
            } else if let Some(face) = body.filter(|face| face.has(codepoint)) {
                face
            } else if let Some(face) = rescue.filter(|face| face.has(codepoint)) {
                face
            } else {
                math
            };
            total += i64::from(face.advance_at(codepoint));
        }
        let total = total.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32;
        Width(scale(total, computed.font_size.0))
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

    fn math_param(&self, param: MathParam, _style: StyleId) -> Option<i32> {
        self.table.math_face()?.math().constant(param.index())
    }

    fn math_ink(&self, text: &str, style: StyleId) -> Option<(Width, Width)> {
        let computed = self.computed(style);
        math_ink(self.table.math_face()?, text, computed.font_size)
    }

    fn math_italic(&self, text: &str, style: StyleId) -> Width {
        let computed = self.computed(style);
        match self.table.math_face() {
            Some(face) => math_italic(face, text, computed.font_size),
            None => Width::ZERO,
        }
    }

    fn math_variant_count(&self, text: &str, _style: StyleId) -> u16 {
        math_variant_count(self.table.math_face(), text)
    }

    fn math_variant(&self, text: &str, index: u16, style: StyleId) -> Option<MathVariant> {
        let computed = self.computed(style);
        math_variant_of(self.table.math_face()?, text, index, computed.font_size)
    }

    fn math_top_accent(&self, text: &str, style: StyleId) -> Option<Width> {
        let computed = self.computed(style);
        math_top_accent(self.table.math_face()?, text, computed.font_size)
    }

    fn math_has_glyph(&self, character: char, _style: StyleId) -> bool {
        self.table
            .math_face()
            .is_some_and(|face| face.has(character as u32))
    }

    fn math_glyph_bottom(&self, text: &str, style: StyleId) -> Option<Width> {
        let computed = self.computed(style);
        math_glyph_bottom(self.table.math_face()?, text, computed.font_size)
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

/// Vertical ink of a math run from the table's per-glyph records, scaled to
/// `size`. `None` when any character has no record (a Han character inside
/// `\text`, for example), so the caller falls back to the em box.
fn math_ink(face: &soma_metrics::Face, text: &str, size: Width) -> Option<(Width, Width)> {
    let mut above = 0i32;
    let mut below = 0i32;
    let mut any = false;
    for character in text.chars() {
        let (top, bottom, _) = face.math().glyph(character as u32)?;
        above = above.max(top);
        below = below.max(bottom);
        any = true;
    }
    if !any {
        return None;
    }
    Some((
        Width(scale(above.max(0), size.0)),
        Width(scale(below.max(0), size.0)),
    ))
}

/// Italic correction of a math run's last glyph, scaled to `size`.
fn math_italic(face: &soma_metrics::Face, text: &str, size: Width) -> Width {
    let Some(codepoint) = last_codepoint(text) else {
        return Width::ZERO;
    };
    face.math()
        .glyph(codepoint)
        .map(|(_, _, italic)| Width(scale(italic, size.0)))
        .unwrap_or(Width::ZERO)
}

/// Number of vertical variants of the first glyph in `text`.
fn math_variant_count(face: Option<&soma_metrics::Face>, text: &str) -> u16 {
    let Some(face) = face else {
        return 0;
    };
    let Some(codepoint) = first_codepoint(text) else {
        return 0;
    };
    face.math().variants(codepoint).len().min(u16::MAX as usize) as u16
}

/// Signed ink bottom of the first glyph in `text`, scaled to `size`.
fn math_glyph_bottom(face: &soma_metrics::Face, text: &str, size: Width) -> Option<Width> {
    let codepoint = first_codepoint(text)?;
    face.math()
        .glyph(codepoint)
        .map(|(_, bottom, _)| Width(scale(bottom, size.0)))
}

/// Top accent attachment of the first glyph in `text`, scaled to `size`.
fn math_top_accent(face: &soma_metrics::Face, text: &str, size: Width) -> Option<Width> {
    let codepoint = first_codepoint(text)?;
    face.math()
        .top_accent(codepoint)
        .map(|value| Width(scale(value, size.0)))
}

/// One vertical variant of the first glyph in `text`, scaled to `size`.
fn math_variant_of(
    face: &soma_metrics::Face,
    text: &str,
    index: u16,
    size: Width,
) -> Option<MathVariant> {
    let codepoint = first_codepoint(text)?;
    let variant = face.math().variants(codepoint).get(index as usize)?;
    Some(MathVariant {
        advance: Width(scale(variant.advance, size.0)),
        above: Width(scale(variant.above, size.0)),
        below: Width(scale(variant.below, size.0)),
        right_gap: Width(scale(variant.right_gap, size.0)),
    })
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
