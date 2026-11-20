//! Display math: bridge `soma_math` to the layout metrics seam.
//!
//! `soma_math` lays out a formula at an arbitrary size and asks for advances and
//! font extents at that size. [`Metrics`] only measures at the style's own font
//! size, so [`MathAdapter`] rescales by `size / font_size`. The rescale is the
//! same integer truncation used everywhere else, which keeps the box tree
//! reproducible across targets.
//!
//! The module also owns inline math: [`InlineMaths`] registers a laid-out formula
//! under a private-use marker, and [`ObjectMetrics`] answers the kernel's object
//! hooks for that marker so line breaking sees the formula box, not its source.

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use soma_core::{GlyphMetrics, LineMetrics, Metrics, StyleId, Width};
use soma_math::{MathLayout, MathMetrics, MathStyle};
use soma_template::ComputedStyle;

/// First character of an inline-math atom marker. Distinct from the footnote
/// marker (`U+E000`), and never drawn: renderers look the atom up in
/// [`crate::InlineMathBox`] instead.
pub const MARKER: char = '\u{e001}';

/// `soma_math::MathMetrics` backed by one `StyleId` of a [`Metrics`] source.
pub struct MathAdapter<'a> {
    metrics: &'a dyn Metrics,
    style: StyleId,
    /// Font size the wrapped metrics measure at.
    base: Width,
}

impl<'a> MathAdapter<'a> {
    pub fn new(metrics: &'a dyn Metrics, style: StyleId, computed: &ComputedStyle) -> Self {
        Self {
            metrics,
            style,
            base: computed.font_size,
        }
    }
}

impl MathMetrics for MathAdapter<'_> {
    fn advance(&self, text: &str, size: Width) -> Width {
        rescale(self.metrics.math_advance(text, self.style), size, self.base)
    }

    fn ascent(&self, size: Width) -> Width {
        let line = self.metrics.math_metrics(self.style);
        rescale(line.ascent, size, self.base)
    }

    fn descent(&self, size: Width) -> Width {
        let line = self.metrics.math_metrics(self.style);
        rescale(line.descent, size, self.base)
    }

    fn has(&self, character: char) -> bool {
        self.metrics.has_glyph(character, self.style)
    }
}

/// Layout constants for a display formula at the style's font size.
///
/// The kernel defaults are tuned for `1em` body text; an inline style with a
/// different `font_size` only needs `size` replaced, and every em-relative
/// constant stays the same. A template override can change the constants later
/// without touching this adapter.
pub fn style_for(computed: &ComputedStyle) -> MathStyle {
    MathStyle {
        size: computed.font_size,
        ..MathStyle::default()
    }
}

/// Layout constants for an inline formula at the style's font size.
///
/// Inline math is set in text style: big operators keep their limits to the
/// right instead of stacking them.
pub fn inline_style_for(computed: &ComputedStyle) -> MathStyle {
    MathStyle {
        size: computed.font_size,
        display: false,
        ..MathStyle::default()
    }
}

/// Lay out a display formula, or `None` when the source is not valid TeX math.
///
/// Callers fall back to a text placeholder on `None`; a malformed formula must
/// never fail the whole layout.
pub fn display(
    source: &str,
    computed: &ComputedStyle,
    metrics: &dyn Metrics,
    style: StyleId,
) -> Option<MathLayout> {
    let math_style = style_for(computed);
    soma_math::layout(
        source,
        &math_style,
        &MathAdapter::new(metrics, style, computed),
    )
    .ok()
}

/// Lay out an inline formula in text style. See [`display`] for the error path.
pub fn display_inline(
    source: &str,
    computed: &ComputedStyle,
    metrics: &dyn Metrics,
    style: StyleId,
) -> Option<MathLayout> {
    let math_style = inline_style_for(computed);
    soma_math::layout(
        source,
        &math_style,
        &MathAdapter::new(metrics, style, computed),
    )
    .ok()
}

/// One inline formula, registered under a marker until the break produces the
/// atom index.
pub struct InlineMath {
    marker: String,
    source: String,
    layout: MathLayout,
}

/// Registry of inline formulas for one paragraph.
///
/// Markers are `U+E001` plus a decimal index. They replace the object text in
/// the atom stream, so the kernel never confuses a formula with body text that
/// happens to look like TeX.
#[derive(Default)]
pub struct InlineMaths {
    items: Vec<InlineMath>,
}

impl InlineMaths {
    pub fn new() -> Self {
        Self { items: Vec::new() }
    }

    /// Register a formula and return the marker to use as the atom text.
    pub fn push(&mut self, source: &str, layout: MathLayout) -> String {
        let marker = format!("{}{}", MARKER, self.items.len());
        self.items.push(InlineMath {
            marker: marker.clone(),
            source: source.to_string(),
            layout,
        });
        marker
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    fn find(&self, marker: &str) -> Option<&InlineMath> {
        self.items.iter().find(|item| item.marker == marker)
    }

    /// `(source, math)` pairs in atom order, ready for the placed block.
    pub fn resolve(&self, atoms: &[soma_core::Atom]) -> Vec<(u32, String, MathLayout)> {
        if self.items.is_empty() {
            return Vec::new();
        }
        let mut out = Vec::new();
        for (index, atom) in atoms.iter().enumerate() {
            if let Some(item) = self.find(&atom.text) {
                out.push((index as u32, item.source.clone(), item.layout.clone()));
            }
        }
        out
    }
}

/// Metrics proxy that answers the object hooks for registered inline formulas
/// and forwards everything else to the wrapped source.
pub struct ObjectMetrics<'a> {
    base: &'a dyn Metrics,
    objects: &'a InlineMaths,
}

impl<'a> ObjectMetrics<'a> {
    pub fn new(base: &'a dyn Metrics, objects: &'a InlineMaths) -> Self {
        Self { base, objects }
    }
}

impl Metrics for ObjectMetrics<'_> {
    fn advance(&self, text: &str, style: StyleId) -> Width {
        self.base.advance(text, style)
    }

    fn line_metrics(&self, style: StyleId) -> LineMetrics {
        self.base.line_metrics(style)
    }

    fn space_width(&self, style: StyleId) -> Width {
        self.base.space_width(style)
    }

    fn glyph_metrics(&self, text: &str, style: StyleId) -> GlyphMetrics {
        self.base.glyph_metrics(text, style)
    }

    fn object_advance(&self, text: &str, style: StyleId) -> Width {
        // A footnote reference is a small superscript number, not a full glyph
        // in the text run: an em of width pushes the following mark onto its
        // own line and opens a hole before it. Three fifths of a digit is the
        // room a superscript actually needs.
        if text == crate::FOOTNOTE_MARKER_TEXT {
            return Width(self.base.advance("0", style).0 * 3 / 5);
        }
        match self.objects.find(text) {
            Some(object) => object.layout.width,
            None => self.base.object_advance(text, style),
        }
    }

    fn ink_right(&self, text: &str, style: StyleId) -> Option<Width> {
        self.base.ink_right(text, style)
    }

    fn object_metrics(&self, text: &str, style: StyleId) -> GlyphMetrics {
        if text == crate::FOOTNOTE_MARKER_TEXT {
            let line = self.base.line_metrics(style);
            return GlyphMetrics {
                advance: Width(self.base.advance("0", style).0 * 3 / 5),
                ascent: line.ascent,
                descent: line.descent,
                leading: line.leading,
                baseline_shift: Width::ZERO,
                overhang_left: Width::ZERO,
                overhang_right: Width::ZERO,
            };
        }
        match self.objects.find(text) {
            Some(object) => GlyphMetrics {
                advance: object.layout.width,
                ascent: object.layout.height,
                descent: object.layout.depth,
                leading: Width::ZERO,
                baseline_shift: Width::ZERO,
                overhang_left: Width::ZERO,
                overhang_right: Width::ZERO,
            },
            None => self.base.object_metrics(text, style),
        }
    }

    fn version(&self) -> u16 {
        self.base.version()
    }

    fn hash(&self) -> u64 {
        self.base.hash()
    }
}

/// Scale a value measured at `base` to `size`. Zero or negative `base` means the
/// source has no font-size information; keep the value unchanged.
fn rescale(value: Width, size: Width, base: Width) -> Width {
    if base.0 <= 0 || size.0 == base.0 {
        return value;
    }
    let scaled = value.0 as i64 * size.0 as i64 / base.0 as i64;
    if scaled > i32::MAX as i64 {
        Width(i32::MAX)
    } else if scaled < i32::MIN as i64 {
        Width(i32::MIN)
    } else {
        Width(scaled as i32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use soma_core::LineMetrics;

    struct Fixed;

    impl Metrics for Fixed {
        fn advance(&self, text: &str, _style: StyleId) -> Width {
            // Every symbol is 1em wide at the style size.
            if text.is_empty() {
                Width::ZERO
            } else {
                Width::EM
            }
        }

        fn line_metrics(&self, _style: StyleId) -> LineMetrics {
            LineMetrics {
                ascent: Width::EM,
                descent: Width(1024),
                leading: Width::ZERO,
            }
        }
    }

    #[test]
    fn adapter_rescales_advances_to_the_requested_size() {
        let computed = ComputedStyle::base();
        let adapter = MathAdapter::new(&Fixed, StyleId(0), &computed);
        assert_eq!(adapter.advance("x", Width::EM), Width::EM);
        assert_eq!(adapter.advance("x", Width::EM / 2), Width::EM / 2);
        assert_eq!(adapter.ascent(Width::EM / 2), Width::EM / 2);
        assert_eq!(adapter.descent(Width::EM / 2), Width(512));
    }

    #[test]
    fn display_rejects_malformed_source() {
        let computed = ComputedStyle::base();
        assert!(display("\\frac{1}", &computed, &Fixed, StyleId(0)).is_none());
        assert!(display("", &computed, &Fixed, StyleId(0)).is_none());
    }

    #[test]
    fn display_keeps_the_box_inside_one_em_line() {
        let computed = ComputedStyle::base();
        let layout = display("x+1", &computed, &Fixed, StyleId(0)).unwrap();
        assert_eq!(layout.total_height(), Width::EM + Width(1024));
        // x, +, 1 with one relation gap.
        assert_eq!(layout.items.len(), 3);
        assert!(layout.width.0 > Width::EM.0 * 2);
    }
}
