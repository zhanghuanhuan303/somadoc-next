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

    /// Version of the metrics source. `0` means "not a versioned table"
    /// (the synthetic test adapter). Table-backed implementations return the
    /// format version they were built for.
    fn version(&self) -> u16 {
        0
    }

    /// Content hash of the metrics source. `0` for adapters without one. The
    /// page result carries this value so a table swap is visible in canonical
    /// bytes even when the version does not change.
    fn hash(&self) -> u64 {
        0
    }

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

    /// Whether the face that draws `style` has a glyph for `character`.
    ///
    /// The math layout asks before setting a variable in the Mathematical
    /// Italic block: a face without those glyphs would draw `.notdef`. A host
    /// that cannot answer says no, which keeps the letter upright — a formula
    /// set in roman is a smaller disappointment than a formula full of boxes.
    fn has_glyph(&self, _character: char, _style: StyleId) -> bool {
        false
    }

    /// Optional kerning hook. MVP implementations return zero.
    fn kerning(&self, _left: &str, _right: &str, _style: StyleId) -> Width {
        Width::ZERO
    }

    /// Gap between the glyph outline and the right edge of its advance, in
    /// 1/4096 em. For a Latin run this is the last character's bearing. `None`
    /// when the source has no outline data; the layout then keeps the rule
    /// table's constant (`compress_right`).
    ///
    /// Every atom can be the last one on a line (ADR-0038), so every atom is
    /// asked, not only marks.
    fn ink_right(&self, _text: &str, _style: StyleId) -> Option<Width> {
        None
    }

    /// Gap between the left edge of a glyph's advance and its outline. For a
    /// Latin run this is the first character's bearing.
    fn ink_left(&self, _text: &str, _style: StyleId) -> Option<Width> {
        None
    }

    /// True when a rescue face draws this text rather than the style's own.
    /// The renderer asks the same question the breaker asked, so the two agree
    /// on which face produced the widths (docs/mixed-font.md).
    fn uses_fallback(&self, _text: &str, _style: StyleId) -> bool {
        false
    }

    /// Advance of one math symbol at the style's size.
    ///
    /// Hosts with a dedicated math face override this so formula symbols come
    /// from that face; the default uses the text face.
    fn math_advance(&self, text: &str, style: StyleId) -> Width {
        self.advance(text, style)
    }

    /// Line metrics of the math face. The default reuses the text metrics.
    fn math_metrics(&self, style: StyleId) -> LineMetrics {
        self.line_metrics(style)
    }

    /// One constant from the math face's OpenType `MATH` table, in per-mille of
    /// one em. `None` means the host has no table; `soma-math` then uses its
    /// TeX fallback values.
    ///
    /// The variant order is the storage order of `soma.metrics`; changing it
    /// needs a new table version.
    fn math_param(&self, _param: MathParam, _style: StyleId) -> Option<i32> {
        None
    }

    /// Ink extent of a math glyph run at the style's size, in 1/4096 em:
    /// `(above baseline, below baseline)`. `None` when the host has no outline
    /// data for every character in `text`.
    ///
    /// The math box needs ink, not the font's em box: a radical bar sits on the
    /// radicand's ink top, a fence is sized by the body's ink, and a fraction
    /// clears the bar by the numerator's ink depth (Math v2).
    fn math_ink(&self, _text: &str, _style: StyleId) -> Option<(Width, Width)> {
        None
    }

    /// Italic correction of the last glyph in `text`, in 1/4096 em. A
    /// superscript after a slanted base starts this much further right.
    fn math_italic(&self, _text: &str, _style: StyleId) -> Width {
        Width::ZERO
    }

    /// Advance of an unbreakable object atom (`AtomKind::InlineObject`).
    ///
    /// Hosts that typeset objects themselves (inline math, inline images with
    /// known geometry) override this so the object's box — not the placeholder
    /// text — enters line breaking. The default measures the text, which keeps
    /// adapters without object support byte-identical.
    fn object_advance(&self, text: &str, style: StyleId) -> Width {
        self.advance(text, style)
    }

    /// Vertical metrics of an unbreakable object atom. The default mirrors
    /// [`Self::glyph_metrics`], so the line box only changes when a host opts
    /// in.
    fn object_metrics(&self, text: &str, style: StyleId) -> GlyphMetrics {
        self.glyph_metrics(text, style)
    }

    /// Width of a space boundary. Defaults to the advance of `" "`.
    fn space_width(&self, style: StyleId) -> Width {
        self.advance(" ", style)
    }
}

/// A constant from the math face's OpenType `MATH` table.
///
/// Values are stored in per-mille of one em (a length of 0.677 em is 677; a
/// percentage of 70% is also 700). The discriminants are the table's storage
/// order: `soma-metrics` writes them in this order and `tools/metrics-gen`
/// reads the same names. Adding a variant is a new metrics-table version.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u16)]
pub enum MathParam {
    /// `MathConstants.axisHeight`.
    AxisHeight = 0,
    /// `MathConstants.fractionRuleThickness`.
    RuleThickness = 1,
    /// `MathConstants.scriptPercentScaleDown` (percent -> per-mille).
    ScriptPercent = 2,
    /// `MathConstants.scriptScriptPercentScaleDown`.
    ScriptScriptPercent = 3,
    /// `MathConstants.superscriptShiftUp`.
    SupShift = 4,
    /// `MathConstants.superscriptShiftUpCramped`.
    SupShiftCramped = 5,
    /// `MathConstants.subscriptShiftDown`.
    SubShift = 6,
    /// `MathConstants.superscriptBaselineDropMax`.
    SupDrop = 7,
    /// `MathConstants.subscriptBaselineDropMin`.
    SubDrop = 8,
    /// `MathConstants.subSuperscriptGapMin`.
    SubSupGap = 9,
    /// `MathConstants.superscriptBottomMin`.
    SupBottomMin = 10,
    /// `MathConstants.superscriptBottomMaxWithSubscript`.
    SupBottomMaxWithSub = 11,
    /// `MathConstants.fractionNumeratorShiftUp`.
    NumShift = 12,
    /// `MathConstants.fractionNumeratorDisplayStyleShiftUp`.
    NumShiftDisplay = 13,
    /// `MathConstants.fractionDenominatorShiftDown`.
    DenomShift = 14,
    /// `MathConstants.fractionDenominatorDisplayStyleShiftDown`.
    DenomShiftDisplay = 15,
    /// `MathConstants.fractionNumeratorGapMin`.
    NumGapMin = 16,
    /// `MathConstants.fractionDenominatorGapMin`.
    DenomGapMin = 17,
    /// `MathConstants.upperLimitGapMin`.
    LimitAboveGap = 18,
    /// `MathConstants.upperLimitBaselineRiseMin`.
    LimitAboveRise = 19,
    /// `MathConstants.lowerLimitGapMin`.
    LimitBelowGap = 20,
    /// `MathConstants.lowerLimitBaselineDropMin`.
    LimitBelowDrop = 21,
    /// `MathConstants.radicalRuleThickness`.
    RadicalRule = 22,
    /// `MathConstants.radicalExtraAscender`.
    RadicalExtra = 23,
    /// `MathConstants.radicalKernBeforeDegree`.
    RadicalKernBefore = 24,
    /// `MathConstants.radicalKernAfterDegree`.
    RadicalKernAfter = 25,
    /// `MathConstants.radicalDegreeBottomRaisePercent` (percent -> per-mille).
    RadicalDegreeRaise = 26,
    /// `MathConstants.displayOperatorMinHeight`.
    DisplayOperatorMin = 27,
    /// `MathConstants.overbarVerticalGap`.
    OverbarGap = 28,
    /// `MathConstants.overbarRuleThickness`.
    OverbarRule = 29,
    /// `MathConstants.overbarExtraAscender`.
    OverbarExtra = 30,
    /// `MathConstants.underbarVerticalGap`.
    UnderbarGap = 31,
    /// `MathConstants.underbarRuleThickness`.
    UnderbarRule = 32,
    /// `MathConstants.underbarExtraDescender`.
    UnderbarExtra = 33,
    /// `MathConstants.accentBaseHeight`.
    AccentBaseHeight = 34,
}

impl MathParam {
    pub const COUNT: usize = 35;

    pub const ALL: [Self; Self::COUNT] = [
        Self::AxisHeight,
        Self::RuleThickness,
        Self::ScriptPercent,
        Self::ScriptScriptPercent,
        Self::SupShift,
        Self::SupShiftCramped,
        Self::SubShift,
        Self::SupDrop,
        Self::SubDrop,
        Self::SubSupGap,
        Self::SupBottomMin,
        Self::SupBottomMaxWithSub,
        Self::NumShift,
        Self::NumShiftDisplay,
        Self::DenomShift,
        Self::DenomShiftDisplay,
        Self::NumGapMin,
        Self::DenomGapMin,
        Self::LimitAboveGap,
        Self::LimitAboveRise,
        Self::LimitBelowGap,
        Self::LimitBelowDrop,
        Self::RadicalRule,
        Self::RadicalExtra,
        Self::RadicalKernBefore,
        Self::RadicalKernAfter,
        Self::RadicalDegreeRaise,
        Self::DisplayOperatorMin,
        Self::OverbarGap,
        Self::OverbarRule,
        Self::OverbarExtra,
        Self::UnderbarGap,
        Self::UnderbarRule,
        Self::UnderbarExtra,
        Self::AccentBaseHeight,
    ];

    pub const fn index(self) -> usize {
        self as usize
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
