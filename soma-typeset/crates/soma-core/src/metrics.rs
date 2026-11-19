//! Host-supplied font metrics.
//!
//! The core never opens a font, scans a font directory, or calls a platform
//! text API. Callers provide advances and line metrics through this small
//! interface. A table-backed implementation and a synthetic implementation are
//! both possible adapters at this seam.

use crate::units::Width;

/// Index into the caller's style table.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct StyleId(pub u16);

/// Vertical metrics for one line of a style.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LineMetrics {
    pub ascent: Width,
    pub descent: Width,
    pub leading: Width,
}

/// Per-glyph metrics. `overhang` is visual only; it never enters advance or
/// line-breaking width.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GlyphMetrics {
    pub advance: Width,
    pub ascent: Width,
    pub descent: Width,
    pub leading: Width,
    /// Positive shifts the baseline up (superscript), negative shifts it down.
    pub baseline_shift: Width,
    pub overhang_left: Width,
    pub overhang_right: Width,
}

impl LineMetrics {
    pub const fn height(self) -> Width {
        self.ascent
            .saturating_add(self.descent)
            .saturating_add(self.leading)
    }
}

/// The geometry seam between the typesetting core and the host.
/// Metrics seam. Implementations are pure functions of `(StyleId, text)` and
/// must be `Sync` so `&dyn Metrics` can be shared across threads
/// (ADR-0010 §6). Caches may memoize, never change output.
pub trait Metrics: Sync {
    /// Width of one break atom: a CJK character, a Latin run, or an inline
    /// object. The implementation may sum per-character advances internally.
    fn advance(&self, text: &str, style: StyleId) -> Width;

    fn line_metrics(&self, style: StyleId) -> LineMetrics;

    /// Per-glyph metrics used by the line box. The default derives them from
    /// `advance` and `line_metrics`; hosts with richer font data override it.
    fn glyph_metrics(&self, text: &str, style: StyleId) -> GlyphMetrics {
        let line = self.line_metrics(style);
        GlyphMetrics {
            advance: self.advance(text, style),
            ascent: line.ascent,
            descent: line.descent,
            leading: line.leading,
            baseline_shift: Width::ZERO,
            overhang_left: Width::ZERO,
            overhang_right: Width::ZERO,
        }
    }

    /// Optional kerning hook. MVP implementations return zero.
    fn kerning(&self, _left: &str, _right: &str, _style: StyleId) -> Width {
        Width::ZERO
    }

    /// Width of a space boundary. Defaults to the advance of `" "`.
    fn space_width(&self, style: StyleId) -> Width {
        self.advance(" ", style)
    }
}

/// A deterministic synthetic metrics implementation for tests and fixtures.
///
/// This is not a font table. It treats whitespace, ASCII letters/digits,
/// ASCII punctuation, and non-ASCII characters as four width classes so the
/// breaker can be exercised without a font dependency.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SyntheticMetrics {
    pub cjk: Width,
    pub latin: Width,
    pub space: Width,
    pub punctuation: Width,
}

impl Default for SyntheticMetrics {
    fn default() -> Self {
        Self {
            cjk: Width::EM,
            latin: Width::from_em_frac(1, 2),
            space: Width::from_em_frac(1, 4),
            punctuation: Width::EM,
        }
    }
}

impl Metrics for SyntheticMetrics {
    fn advance(&self, text: &str, _style: StyleId) -> Width {
        let mut total: i64 = 0;
        for c in text.chars() {
            let width = if c == ' ' {
                self.space
            } else if c.is_ascii_alphanumeric() {
                self.latin
            } else if c.is_ascii_punctuation() {
                self.punctuation
            } else {
                self.cjk
            };
            total += width.units() as i64;
        }
        Width(total as i32)
    }

    fn line_metrics(&self, _style: StyleId) -> LineMetrics {
        LineMetrics {
            ascent: Width::from_em_frac(22, 25),
            descent: Width::from_em_frac(3, 25),
            leading: Width::from_em_frac(1, 5),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn synthetic_advance_is_deterministic() {
        let metrics = SyntheticMetrics::default();
        assert_eq!(metrics.advance("中", StyleId(0)), Width::EM);
        assert_eq!(
            metrics.advance("English", StyleId(0)),
            Width::from_em_frac(7, 2)
        );
        assert_eq!(metrics.space_width(StyleId(0)), Width::from_em_frac(1, 4));
    }

    #[test]
    fn line_height_is_sum_of_parts() {
        let metrics = SyntheticMetrics::default();
        let line = metrics.line_metrics(StyleId(0));
        assert_eq!(line.height(), line.ascent + line.descent + line.leading);
    }
}
