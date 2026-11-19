//! Display math: bridge `soma_math` to the layout metrics seam.
//!
//! `soma_math` lays out a formula at an arbitrary size and asks for advances and
//! font extents at that size. [`Metrics`] only measures at the style's own font
//! size, so [`MathAdapter`] rescales by `size / font_size`. The rescale is the
//! same integer truncation used everywhere else, which keeps the box tree
//! reproducible across targets.

use soma_core::{Metrics, StyleId, Width};
use soma_math::{MathLayout, MathMetrics, MathStyle};
use soma_template::ComputedStyle;

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
        rescale(self.metrics.advance(text, self.style), size, self.base)
    }

    fn ascent(&self, size: Width) -> Width {
        let line = self.metrics.line_metrics(self.style);
        rescale(line.ascent, size, self.base)
    }

    fn descent(&self, size: Width) -> Width {
        let line = self.metrics.line_metrics(self.style);
        rescale(line.descent, size, self.base)
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
