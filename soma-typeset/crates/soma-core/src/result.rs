//! Deterministic layout result.
//!
//! The result is the cross-runtime contract. It contains only integers and
//! indices, never floats or atom text, so native and wasm builds can compare
//! canonical bytes. Rendering adapters map atom indices back to text and paint
//! at the supplied positions.

use alloc::string::String;
use alloc::vec::Vec;

use crate::units::{Cost, Width};

/// One painted atom inside a line. `x` is relative to the paragraph content
/// origin, including the first-line indent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AtomBox {
    pub atom: u32,
    pub x: Width,
    pub advance: Width,
}

/// One CjkLatin boundary's actual justification adjustment.
///
/// Boundary indices are paragraph-global. Only CjkLatin boundaries are emitted;
/// the list is O(number of CjkLatin boundaries) and does not enter the DP.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CjkLatinAdjust {
    pub boundary: u32,
    pub adjustment: i32,
}

/// One laid-out line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Line {
    /// First atom index, inclusive.
    pub start: u32,
    /// Last atom index, exclusive.
    pub end: u32,
    pub indent: Width,
    pub target_width: Width,
    pub natural_width: Width,
    pub trim_left: Width,
    pub trim_right: Width,
    pub ratio_milli: i32,
    pub ascent: Width,
    pub descent: Width,
    pub leading: Width,
    pub height: Width,
    /// Signed justification adjustment. Zero for a paragraph last line, a
    /// forced segment end, and left-aligned paragraphs.
    pub adjustment: i32,
    /// O(1) approximate mean absolute CjkLatin deviation, in width units.
    pub boundary_deviation: i64,
    /// Actual adjustments at CjkLatin boundaries; eval/renderer can verify the
    /// proportional-distribution assumption.
    pub cjk_latin_adjustments: Vec<CjkLatinAdjust>,
    pub cost: Cost,
    /// True when the line exceeds its target even after allowed shrink. A line
    /// with `adjustment < 0` but `overfull == false` was compressed to fit.
    pub overfull: bool,
    pub emergency: bool,
    /// True when this line ends at a `Forced` boundary that is not the
    /// paragraph end. Such a line is not stretched and has no `short_score`.
    pub forced_end: bool,
    /// True when the line ends at a hyphenation point: the renderer draws a
    /// hyphen after the last box and the width already includes it (ADR-0059).
    pub hyphenated: bool,
    pub boxes: Vec<AtomBox>,
}

/// Which algorithm actually produced the result. `max_kp_atoms` can downgrade
/// a requested K-P run to Greedy-C deterministically.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BreakerKind {
    Optimal,
    /// Same cost model as K-P, no lookahead (`GreedyMode::MinCost`).
    GreedyC,
    /// Furthest fit (`GreedyMode::Furthest`).
    GreedyF,
}

impl BreakerKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Optimal => "optimal",
            Self::GreedyC => "greedyC",
            Self::GreedyF => "greedyF",
        }
    }
}

/// Which fallback stage produced the result. `total_cost` is only comparable
/// within the same stage. `ForbiddenRescue` and `Overfull` are reserved for the
/// full ladder; the current engine emits `Normal` or `Emergency`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FallbackStage {
    Normal,
    Emergency,
    ForbiddenRescue,
    Overfull,
}

impl FallbackStage {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::Emergency => "emergency",
            Self::ForbiddenRescue => "forbiddenRescue",
            Self::Overfull => "overfull",
        }
    }
}

impl Line {
    /// Write the canonical JSON form of one line. `LayoutResult` and page
    /// results share this so a line has exactly one serialization.
    pub fn write_canonical(&self, out: &mut impl core::fmt::Write) -> core::fmt::Result {
        write!(
            out,
            "{{\"start\":{},\"end\":{},\"indent\":{},\"targetWidth\":{},\"naturalWidth\":{},\"trimLeft\":{},\"trimRight\":{},\"ratioMilli\":{},\"ascent\":{},\"descent\":{},\"leading\":{},\"height\":{},\"adjustment\":{},\"boundaryDeviation\":{},\"cjkLatinAdjustments\":[",
            self.start,
            self.end,
            self.indent.0,
            self.target_width.0,
            self.natural_width.0,
            self.trim_left.0,
            self.trim_right.0,
            self.ratio_milli,
            self.ascent.0,
            self.descent.0,
            self.leading.0,
            self.height.0,
            self.adjustment,
            self.boundary_deviation
        )?;

        for (adjust_index, adjust) in self.cjk_latin_adjustments.iter().enumerate() {
            if adjust_index > 0 {
                out.write_char(',')?;
            }
            write!(
                out,
                "{{\"boundary\":{},\"adjustment\":{}}}",
                adjust.boundary, adjust.adjustment
            )?;
        }

        write!(
            out,
            "],\"cost\":{},\"overfull\":{},\"emergency\":{},\"forcedEnd\":{},",
            self.cost.0, self.overfull, self.emergency, self.forced_end
        )?;
        // Only a hyphenated line writes the key, so every other line keeps its
        // bytes from before hyphenation existed.
        if self.hyphenated {
            out.write_str("\"hyphenated\":true,")?;
        }
        out.write_str("\"boxes\":[")?;

        for (box_index, atom_box) in self.boxes.iter().enumerate() {
            if box_index > 0 {
                out.write_char(',')?;
            }
            write!(
                out,
                "{{\"atom\":{},\"x\":{},\"advance\":{}}}",
                atom_box.atom, atom_box.x.0, atom_box.advance.0
            )?;
        }

        out.write_str("]}")
    }
}

/// The complete, serializable result of breaking one paragraph.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LayoutResult {
    pub lines: Vec<Line>,
    /// Only comparable within the same `(modelVersion, preset, rulesVersion,
    /// metricsVersion, fallback_stage)`.
    pub total_cost: Cost,
    pub breaker_used: BreakerKind,
    pub fallback_stage: FallbackStage,
    pub length_limited: bool,
    pub emergency_lines: u32,
    pub forbidden_violations: u32,
    pub rules_version: u16,
    pub metrics_version: u16,
}

impl LayoutResult {
    pub const SCHEMA: &'static str = "soma.layout/6";
    pub const UNITS: &'static str = "1/4096em";

    pub fn line_count(&self) -> u32 {
        self.lines.len() as u32
    }

    /// Write the canonical JSON form. Field order, integer units, and line
    /// order are part of the format; do not use a map-backed serializer here.
    pub fn write_canonical(&self, out: &mut impl core::fmt::Write) -> core::fmt::Result {
        write!(
            out,
            "{{\"schema\":\"{}\",\"units\":\"{}\",\"rulesVersion\":{},\"metricsVersion\":{},\"breakerUsed\":\"{}\",\"fallbackStage\":\"{}\",\"lengthLimited\":{},\"emergencyLines\":{},\"forbiddenViolations\":{},\"lineCount\":{},\"totalCost\":{},\"lines\":[",
            Self::SCHEMA,
            Self::UNITS,
            self.rules_version,
            self.metrics_version,
            self.breaker_used.as_str(),
            self.fallback_stage.as_str(),
            self.length_limited,
            self.emergency_lines,
            self.forbidden_violations,
            self.line_count(),
            self.total_cost.0
        )?;

        for (line_index, line) in self.lines.iter().enumerate() {
            if line_index > 0 {
                out.write_char(',')?;
            }
            line.write_canonical(out)?;
        }

        out.write_str("]}")
    }

    /// Canonical JSON as a `String`. Writing into a `String` cannot fail.
    pub fn canonical_json(&self) -> String {
        let mut out = String::new();
        self.write_canonical(&mut out)
            .expect("writing canonical JSON into a String cannot fail");
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    #[test]
    fn canonical_json_is_stable_and_integer_only() {
        let result = LayoutResult {
            lines: vec![Line {
                start: 0,
                end: 1,
                indent: Width::ZERO,
                target_width: Width::from_em(10),
                natural_width: Width::from_em(1),
                trim_left: Width::ZERO,
                trim_right: Width::ZERO,
                ratio_milli: 0,
                ascent: Width::from_em(1),
                descent: Width::from_em_frac(1, 4),
                leading: Width::from_em_frac(1, 4),
                height: Width::from_em_frac(3, 2),
                adjustment: 0,
                boundary_deviation: 0,
                cjk_latin_adjustments: vec![],
                cost: Cost(0),
                overfull: false,
                emergency: false,
                forced_end: false,
                hyphenated: false,
                boxes: vec![AtomBox {
                    atom: 0,
                    x: Width::ZERO,
                    advance: Width::EM,
                }],
            }],
            total_cost: Cost(0),
            breaker_used: BreakerKind::Optimal,
            fallback_stage: FallbackStage::Normal,
            length_limited: false,
            emergency_lines: 0,
            forbidden_violations: 0,
            rules_version: 1,
            metrics_version: 2,
        };

        let first = result.canonical_json();
        let second = result.canonical_json();
        assert_eq!(first, second);
        assert_eq!(
            first,
            "{\"schema\":\"soma.layout/6\",\"units\":\"1/4096em\",\"rulesVersion\":1,\"metricsVersion\":2,\"breakerUsed\":\"optimal\",\"fallbackStage\":\"normal\",\"lengthLimited\":false,\"emergencyLines\":0,\"forbiddenViolations\":0,\"lineCount\":1,\"totalCost\":0,\"lines\":[{\"start\":0,\"end\":1,\"indent\":0,\"targetWidth\":40960,\"naturalWidth\":4096,\"trimLeft\":0,\"trimRight\":0,\"ratioMilli\":0,\"ascent\":4096,\"descent\":1024,\"leading\":1024,\"height\":6144,\"adjustment\":0,\"boundaryDeviation\":0,\"cjkLatinAdjustments\":[],\"cost\":0,\"overfull\":false,\"emergency\":false,\"forcedEnd\":false,\"boxes\":[{\"atom\":0,\"x\":0,\"advance\":4096}]}]}"
        );
    }
}
