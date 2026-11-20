//! Paragraph line breaking.
//!
//! The implementation is intentionally a small reference engine: a global
//! dynamic program over breakpoints plus two greedy baselines. It is fast
//! enough for paragraph-level MVP workloads because every line cost is O(1)
//! with prefix sums. Optimizations (active-node pruning, page/chapter
//! windows) come later; the interface and units stay the same.

use alloc::vec::Vec;

use crate::ir::{Align, AtomKind, BoundaryKind, BoundaryRole, Breakability, IrError, Paragraph};
use crate::metrics::Metrics;
use crate::params::{ParamError, ParamHash, ParamSet, BADNESS_RATIO_CLAMP};
use crate::result::{AtomBox, BreakerKind, CjkLatinAdjust, FallbackStage, LayoutResult, Line};
use crate::units::{Cost, Width};

/// Optimization window. MVP implements only `Paragraph`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Window {
    #[default]
    Paragraph,
    Page,
    Chapter,
    Book,
}

/// Greedy baseline flavor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GreedyMode {
    /// Take the furthest break that fits. This is the industrial baseline.
    Furthest,
    /// Take the break with the lowest current-line cost. This shares the cost
    /// model with Knuth-Plass but has no lookahead.
    MinCost,
}

/// Configuration for one break request.
///
/// The parameter set is validated before layout. `param_hash` is captured at
/// construction so a result can be audited even if the caller keeps a mutable
/// copy of the parameters elsewhere.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BreakConfig {
    params: ParamSet,
    window: Window,
    param_hash: ParamHash,
}

impl BreakConfig {
    pub fn new(params: ParamSet, window: Window) -> Self {
        Self {
            params,
            window,
            param_hash: params.param_hash(),
        }
    }

    pub fn from_params(params: ParamSet) -> Self {
        Self::new(params, Window::Paragraph)
    }

    pub fn params(&self) -> &ParamSet {
        &self.params
    }

    pub fn window(&self) -> Window {
        self.window
    }

    pub fn param_hash(&self) -> ParamHash {
        self.param_hash
    }

    pub fn rules_version(&self) -> u16 {
        self.params.rules_version
    }

    pub fn metrics_version(&self) -> u16 {
        self.params.metrics_version
    }
}

impl Default for BreakConfig {
    fn default() -> Self {
        Self::from_params(ParamSet::default_v1())
    }
}

/// Errors returned before a result can be produced.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BreakError {
    UnsupportedWindow(Window),
    UnsupportedAlign(Align),
    Params(ParamError),
    Ir(IrError),
    NoFeasibleBreak,
    TooManyLines { limit: u32 },
    ForcedBreaksDisabled,
}

impl From<IrError> for BreakError {
    fn from(value: IrError) -> Self {
        Self::Ir(value)
    }
}

impl core::fmt::Display for BreakError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::UnsupportedWindow(window) => write!(
                f,
                "optimization window {window:?} is not implemented; MVP supports Paragraph"
            ),
            Self::UnsupportedAlign(align) => write!(
                f,
                "paragraph alignment {align:?} is not implemented; MVP supports Justify"
            ),
            Self::Params(error) => write!(f, "invalid params: {error}"),
            Self::Ir(error) => write!(f, "invalid paragraph IR: {error}"),
            Self::NoFeasibleBreak => f.write_str("no feasible break sequence"),
            Self::TooManyLines { limit } => write!(f, "line count exceeds limit {limit}"),
            Self::ForcedBreaksDisabled => {
                f.write_str("paragraph contains a forced break but features.forced_breaks is off")
            }
        }
    }
}

impl core::error::Error for BreakError {}

/// One line's cost breakdown. Trace output is intentionally outside the
/// canonical result; it is for debugging and calibration.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LineTrace {
    pub start: u32,
    pub end: u32,
    pub target: i64,
    pub natural: i64,
    pub effective: i64,
    pub stretch: i64,
    pub shrink: i64,
    pub ratio_milli: i32,
    pub badness: i64,
    pub ascent: i64,
    pub descent: i64,
    pub leading: i64,
    pub height: i64,
    pub trim_left: i32,
    pub trim_right: i32,
    pub compression_cost: i64,
    pub boundary_deviation: i64,
    /// Exact mean absolute deviation recomputed from the emitted boundary
    /// adjustments; trace-only, O(number of CjkLatin boundaries).
    pub boundary_deviation_exact: i64,
    pub break_penalty: i64,
    pub line_penalty: i64,
    pub short_score: i64,
    pub emergency: bool,
    pub overfull: bool,
    pub cost: i64,
}

/// Trace for one optimal break request.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KpTrace {
    pub lines: Vec<LineTrace>,
    pub evaluated_candidates: u64,
    pub fallback_used: bool,
    pub fallback_stage: FallbackStage,
    pub breaker_used: BreakerKind,
    pub tolerance: i32,
}

/// Break a paragraph and return a per-line breakdown. The trace is not part of
/// the canonical result and does not affect layout.
pub fn break_paragraph_traced(
    paragraph: &Paragraph,
    metrics: &dyn Metrics,
    config: &BreakConfig,
) -> Result<(LayoutResult, KpTrace), BreakError> {
    let (result, evaluated_candidates, fallback_used) =
        break_with_stats(paragraph, metrics, config, Algorithm::Optimal)?;
    let shaped = Shaped::new(paragraph, metrics);
    let mut lines = Vec::with_capacity(result.lines.len());

    for line in &result.lines {
        let start = line.start as usize;
        let end = line.end as usize;
        let kind = end_kind(&shaped, end);
        let eval = match shaped.eval_line(start, end, kind, config, false, false) {
            Some(eval) => eval,
            None => {
                let mut params = *config.params();
                params.break_params.tolerance = params.break_params.emergency_tolerance;
                params.break_params.emergency_stretch =
                    params.break_params.fallback.emergency_stretch;
                let relaxed = BreakConfig::from_params(params);
                shaped
                    .eval_line(start, end, kind, &relaxed, true, true)
                    .ok_or(BreakError::NoFeasibleBreak)?
            }
        };

        let vertical = shaped.line_vertical(start, end);
        let cjk_count = line.cjk_latin_adjustments.len() as i64;
        let boundary_deviation_exact = if cjk_count > 0 {
            line.cjk_latin_adjustments
                .iter()
                .map(|adjust| adjust.adjustment.unsigned_abs() as i64)
                .sum::<i64>()
                / cjk_count
        } else {
            0
        };

        lines.push(LineTrace {
            start: line.start,
            end: line.end,
            target: eval.target,
            natural: eval.natural,
            effective: eval.effective,
            stretch: shaped.total_stretch(start, end),
            shrink: shaped.total_shrink(start, end),
            ratio_milli: saturating_i32(eval.ratio_milli),
            badness: eval.badness,
            ascent: vertical.ascent,
            descent: vertical.descent,
            leading: vertical.leading,
            height: vertical.height,
            trim_left: eval.trim_left,
            trim_right: eval.trim_right,
            compression_cost: eval.compression_cost,
            boundary_deviation: line.boundary_deviation,
            boundary_deviation_exact,
            break_penalty: eval.break_penalty,
            line_penalty: eval.line_penalty,
            short_score: eval.short_score,
            emergency: line.emergency,
            overfull: line.overfull,
            cost: line.cost.0,
        });
    }

    let fallback_stage = result.fallback_stage;
    let breaker_used = result.breaker_used;

    Ok((
        result,
        KpTrace {
            lines,
            evaluated_candidates,
            fallback_used,
            fallback_stage,
            breaker_used,
            tolerance: config.params().break_params.tolerance.0 as i32,
        },
    ))
}

/// Break a paragraph with the global dynamic program.
pub fn break_paragraph(
    paragraph: &Paragraph,
    metrics: &dyn Metrics,
    config: &BreakConfig,
) -> Result<LayoutResult, BreakError> {
    break_with(paragraph, metrics, config, Algorithm::Optimal)
}

/// Break a paragraph with a greedy baseline.
pub fn break_paragraph_greedy(
    paragraph: &Paragraph,
    metrics: &dyn Metrics,
    config: &BreakConfig,
    mode: GreedyMode,
) -> Result<LayoutResult, BreakError> {
    break_with(paragraph, metrics, config, Algorithm::Greedy(mode))
}

#[derive(Clone, Copy, Debug)]
enum Algorithm {
    Optimal,
    Greedy(GreedyMode),
}

fn break_with(
    paragraph: &Paragraph,
    metrics: &dyn Metrics,
    config: &BreakConfig,
    algorithm: Algorithm,
) -> Result<LayoutResult, BreakError> {
    break_with_stats(paragraph, metrics, config, algorithm).map(|(result, _, _)| result)
}

fn break_with_stats(
    paragraph: &Paragraph,
    metrics: &dyn Metrics,
    config: &BreakConfig,
    algorithm: Algorithm,
) -> Result<(LayoutResult, u64, bool), BreakError> {
    if config.window() != Window::Paragraph {
        return Err(BreakError::UnsupportedWindow(config.window()));
    }
    config.params().validate().map_err(BreakError::Params)?;
    // Left is accepted: the boxes are placed without justification, while the
    // breakpoints still come from the same cost model. Centering and
    // right-alignment are applied to the finished lines by `soma-layout`.
    if matches!(paragraph.style.align, Align::Center | Align::Right) {
        return Err(BreakError::UnsupportedAlign(paragraph.style.align));
    }
    paragraph.validate()?;

    let shaped = Shaped::new(paragraph, metrics);
    let mut algorithm = algorithm;
    let mut length_limited = false;
    if let Some(limit) = config.params().break_params.max_kp_atoms {
        if shaped.len() > limit as usize && matches!(algorithm, Algorithm::Optimal) {
            // Deterministic length policy: never use wall-clock time to pick
            // an algorithm. The result is marked so quality reports can keep
            // Greedy-C layouts out of the K-P subset.
            algorithm = Algorithm::Greedy(GreedyMode::MinCost);
            length_limited = true;
        }
    }

    let (mut result, evaluated, fallback_stage) =
        match run(&shaped, config, algorithm, false, false) {
            Ok((result, evaluated)) => (result, evaluated, FallbackStage::Normal),
            Err(BreakError::NoFeasibleBreak) => {
                let break_params = config.params().break_params;
                if !break_params.fallback.allow_emergency {
                    return Err(BreakError::NoFeasibleBreak);
                }

                // Emergency stage: use the declared emergency tolerance and
                // stretch, not hard-coded values. Overfull lines are allowed only
                // when the fallback policy permits them.
                let mut params = *config.params();
                params.break_params.tolerance = break_params.emergency_tolerance;
                params.break_params.emergency_stretch = break_params.fallback.emergency_stretch;
                let relaxed = BreakConfig::from_params(params);

                match run(&shaped, &relaxed, algorithm, false, false) {
                    Ok((result, evaluated)) => (result, evaluated, FallbackStage::Emergency),
                    Err(BreakError::NoFeasibleBreak) => {
                        // Emergency flags are the last resort inside the relaxed
                        // stage: first try the relaxed tolerance/stretch without
                        // flagging lines, then allow explicit emergency lines.
                        match run(&shaped, &relaxed, algorithm, true, false) {
                            Ok((result, evaluated)) => {
                                (result, evaluated, FallbackStage::Emergency)
                            }
                            Err(BreakError::NoFeasibleBreak)
                                if break_params.fallback.allow_forbidden =>
                            {
                                let (result, evaluated) =
                                    run(&shaped, &relaxed, algorithm, true, true)?;
                                (result, evaluated, FallbackStage::ForbiddenRescue)
                            }
                            Err(error) => return Err(error),
                        }
                    }
                    Err(error) => return Err(error),
                }
            }
            Err(error) => return Err(error),
        };

    result.fallback_stage = fallback_stage;
    result.length_limited = length_limited;
    Ok((result, evaluated, fallback_stage != FallbackStage::Normal))
}

fn run(
    shaped: &Shaped<'_>,
    config: &BreakConfig,
    algorithm: Algorithm,
    allow_emergency: bool,
    allow_forbidden: bool,
) -> Result<(LayoutResult, u64), BreakError> {
    match algorithm {
        Algorithm::Optimal => optimal(shaped, config, allow_emergency, allow_forbidden),
        Algorithm::Greedy(mode) => greedy(shaped, config, mode, allow_emergency, allow_forbidden),
    }
}

/// A shaped paragraph: atom widths plus prefix sums for O(1) line queries.
struct Shaped<'a> {
    paragraph: &'a Paragraph,
    metrics: &'a dyn Metrics,
    atom_width: Vec<i64>,
    glue: Vec<i64>,
    stretch: Vec<i64>,
    shrink: Vec<i64>,
    atom_prefix: Vec<i64>,
    glue_prefix: Vec<i64>,
    /// Hyphen glyph width per boundary, else zero. Only the boundary a line
    /// actually breaks at contributes its width.
    hyphen: Vec<i64>,
    stretch_prefix: Vec<i64>,
    shrink_prefix: Vec<i64>,
    cjk_latin_stretch_prefix: Vec<i64>,
    cjk_latin_shrink_prefix: Vec<i64>,
    cjk_latin_glue_prefix: Vec<i64>,
    cjk_latin_count_prefix: Vec<i64>,
    min_atom: i64,
    max_atom: i64,
    max_glue: i64,
    max_stretch: i64,
    max_shrink: i64,
    max_trim_left: i64,
    max_trim_right: i64,
}

impl<'a> Shaped<'a> {
    fn new(paragraph: &'a Paragraph, metrics: &'a dyn Metrics) -> Self {
        let atom_count = paragraph.atoms.len();
        let boundary_count = paragraph.boundaries.len();

        let mut atom_width = Vec::with_capacity(atom_count);
        let tracking = paragraph.style.tracking.0.max(0) as i64;
        for (index, atom) in paragraph.atoms.iter().enumerate() {
            // Object atoms have host-supplied geometry (inline math); text atoms
            // go through the font path.
            let mut width = match atom.kind {
                AtomKind::InlineObject => metrics.object_advance(&atom.text, atom.style).0 as i64,
                _ => metrics.advance(&atom.text, atom.style).0 as i64,
            };
            // Tracking opens the space *between* atoms; the last one would only
            // pad the line end.
            if tracking > 0 && index + 1 < atom_count && atom.kind != AtomKind::InlineObject {
                width += tracking;
            }
            atom_width.push(width);
        }

        let mut glue = Vec::with_capacity(boundary_count);
        let mut stretch = Vec::with_capacity(boundary_count);
        let mut shrink = Vec::with_capacity(boundary_count);
        let mut cjk_latin_stretch = Vec::with_capacity(boundary_count);
        let mut cjk_latin_shrink = Vec::with_capacity(boundary_count);
        let mut cjk_latin_glue = Vec::with_capacity(boundary_count);
        let mut cjk_latin_count = Vec::with_capacity(boundary_count);
        for (index, boundary) in paragraph.boundaries.iter().enumerate() {
            let width = match boundary.kind {
                BoundaryKind::Space => metrics.space_width(paragraph.atoms[index].style).0 as i64,
                BoundaryKind::Normal | BoundaryKind::Hyphen => boundary.glue.width.0 as i64,
            };
            let boundary_stretch = boundary.glue.stretch.0.max(0) as i64;
            let boundary_shrink = boundary.glue.shrink.0.max(0) as i64;
            let is_cjk_latin = boundary.role == BoundaryRole::CjkLatin;
            glue.push(width);
            stretch.push(boundary_stretch);
            shrink.push(boundary_shrink);
            cjk_latin_stretch.push(if is_cjk_latin { boundary_stretch } else { 0 });
            cjk_latin_shrink.push(if is_cjk_latin { boundary_shrink } else { 0 });
            cjk_latin_glue.push(if is_cjk_latin {
                boundary.glue.width.0 as i64
            } else {
                0
            });
            cjk_latin_count.push(i64::from(is_cjk_latin));
        }

        let atom_prefix = prefix_sums(&atom_width);
        let glue_prefix = prefix_sums(&glue);
        // A broken hyphen is drawn in the style of the atom before the break.
        let hyphen: Vec<i64> = paragraph
            .boundaries
            .iter()
            .enumerate()
            .map(|(index, boundary)| {
                if boundary.kind == BoundaryKind::Hyphen {
                    metrics.advance("-", paragraph.atoms[index].style).0.max(0) as i64
                } else {
                    0
                }
            })
            .collect();
        let stretch_prefix = prefix_sums(&stretch);
        let shrink_prefix = prefix_sums(&shrink);
        let cjk_latin_stretch_prefix = prefix_sums(&cjk_latin_stretch);
        let cjk_latin_shrink_prefix = prefix_sums(&cjk_latin_shrink);
        let cjk_latin_glue_prefix = prefix_sums(&cjk_latin_glue);
        let cjk_latin_count_prefix = prefix_sums(&cjk_latin_count);

        let min_atom = atom_width.iter().copied().min().unwrap_or(0).max(0);
        let max_atom = atom_width.iter().copied().max().unwrap_or(0).max(0);
        let max_glue = glue.iter().copied().max().unwrap_or(0).max(0);
        let max_stretch = stretch.iter().copied().max().unwrap_or(0).max(0);
        let max_shrink = shrink.iter().copied().max().unwrap_or(0).max(0);
        let max_trim_left = paragraph
            .atoms
            .iter()
            .map(|atom| atom.compress_left.0 as i64)
            .max()
            .unwrap_or(0)
            .max(0);
        let max_trim_right = paragraph
            .atoms
            .iter()
            .map(|atom| atom.compress_right.0 as i64)
            .max()
            .unwrap_or(0)
            .max(0);

        Self {
            paragraph,
            metrics,
            atom_width,
            glue,
            stretch,
            shrink,
            atom_prefix,
            glue_prefix,
            hyphen,
            stretch_prefix,
            shrink_prefix,
            cjk_latin_stretch_prefix,
            cjk_latin_shrink_prefix,
            cjk_latin_glue_prefix,
            cjk_latin_count_prefix,
            min_atom,
            max_atom,
            max_glue,
            max_stretch,
            max_shrink,
            max_trim_left,
            max_trim_right,
        }
    }

    fn len(&self) -> usize {
        self.paragraph.atoms.len()
    }

    fn target(&self, start: usize) -> i64 {
        let width = self.paragraph.style.width.0 as i64;
        if start == 0 {
            // The first line is indented inward by `first_indent` and outward
            // by `hang`: a list marker hangs in the margin (ADR-0058).
            width + self.paragraph.style.hang.0 as i64 - self.paragraph.style.first_indent.0 as i64
        } else {
            width
        }
    }

    /// Natural width of line `[start, end)`. Glue on the boundary immediately
    /// before `start` and after `end - 1` is discarded at the break.
    fn natural(&self, start: usize, end: usize) -> i64 {
        let atoms = self.atom_prefix[end] - self.atom_prefix[start];
        let boundaries = self.glue_prefix[end - 1] - self.glue_prefix[start];
        // A hyphen exists only where the line breaks; internal hyphenation
        // points contribute nothing (they are not a rendered glyph).
        let hyphen = if end < self.len() {
            self.hyphen[end - 1]
        } else {
            0
        };
        atoms + boundaries + hyphen
    }

    fn total_stretch(&self, start: usize, end: usize) -> i64 {
        self.stretch_prefix[end - 1] - self.stretch_prefix[start]
    }

    fn total_shrink(&self, start: usize, end: usize) -> i64 {
        self.shrink_prefix[end - 1] - self.shrink_prefix[start]
    }

    fn line_vertical(&self, start: usize, end: usize) -> LineVertical {
        let mut ascent = 0i64;
        let mut descent = 0i64;
        let mut leading = 0i64;

        for index in start..end {
            let atom = &self.paragraph.atoms[index];
            let glyph = match atom.kind {
                AtomKind::InlineObject => self.metrics.object_metrics(&atom.text, atom.style),
                _ => self.metrics.glyph_metrics(&atom.text, atom.style),
            };
            ascent = ascent.max(glyph.ascent.0 as i64 + glyph.baseline_shift.0 as i64);
            descent = descent.max(glyph.descent.0 as i64 - glyph.baseline_shift.0 as i64);
            leading = leading.max(glyph.leading.0 as i64);
        }

        let descent = descent.max(0);
        let content_height = ascent.saturating_add(descent).saturating_add(leading);
        let height = content_height.max(self.paragraph.style.line_height.0 as i64);
        LineVertical {
            ascent,
            descent,
            leading,
            height,
        }
    }

    /// Mean absolute CjkLatin deviation for a line whose adjustment is
    /// `adjustment_abs` width units. Returns `(D, ideal_gap)`, both in width
    /// units. `ideal_gap` is the mean CjkLatin boundary glue, falling back to
    /// 1em when the rule layer supplied zero.
    fn cjk_latin_deviation(&self, start: usize, end: usize, adjustment: i64) -> (i64, i64) {
        if adjustment == 0 || end <= start + 1 {
            return (0, Width::EM.0 as i64);
        }

        let use_stretch = adjustment >= 0;
        let adjustment_abs = adjustment.unsigned_abs();
        let count = self.cjk_latin_count_prefix[end - 1] - self.cjk_latin_count_prefix[start];
        if count <= 0 {
            return (0, Width::EM.0 as i64);
        }

        let total_weight = if use_stretch {
            self.total_stretch(start, end)
        } else {
            self.total_shrink(start, end)
        };
        if total_weight <= 0 {
            return (0, Width::EM.0 as i64);
        }

        let sum_weight = if use_stretch {
            self.cjk_latin_stretch_prefix[end - 1] - self.cjk_latin_stretch_prefix[start]
        } else {
            self.cjk_latin_shrink_prefix[end - 1] - self.cjk_latin_shrink_prefix[start]
        };
        let sum_glue = self.cjk_latin_glue_prefix[end - 1] - self.cjk_latin_glue_prefix[start];

        let numerator = adjustment_abs * sum_weight as u64;
        let denominator = total_weight as u64 * count as u64;
        let deviation = numerator.checked_div(denominator).unwrap_or(0) as i64;
        let ideal_gap = (sum_glue / count).max(1);
        (deviation, ideal_gap)
    }

    fn is_break_allowed(&self, end: usize, allow_forbidden: bool) -> bool {
        if end == self.len() {
            return true;
        }
        match self.paragraph.boundaries[end - 1].breakable {
            Breakability::Allowed => true,
            Breakability::Forbidden => allow_forbidden,
            Breakability::Forced => false,
        }
    }

    fn break_penalty(&self, end: usize) -> i64 {
        if end == self.len() {
            0
        } else {
            self.paragraph.boundaries[end - 1].penalty as i64
        }
    }

    fn atom_width(&self, index: usize) -> i64 {
        self.atom_width[index]
    }

    fn glue_width(&self, index: usize) -> i64 {
        self.glue[index]
    }

    fn boundary_weight(&self, index: usize, stretch: bool) -> i64 {
        if stretch {
            self.stretch[index]
        } else {
            self.shrink[index]
        }
    }

    /// Conservative atom-count window for justified lines.
    ///
    /// A justified line with `k` atoms cannot be feasible when its maximum
    /// possible natural width is below the underfull threshold, or when its
    /// minimum possible width after trim is above the overfull threshold. The
    /// window is derived from global maxima/minima, so it may be looser than
    /// the exact line, but it never discards a feasible line. Ragged lines
    /// (paragraph end / forced segment end) are not pruned.
    fn line_length_bounds(&self, config: &BreakConfig, allow_emergency: bool) -> (usize, usize) {
        let n = self.len();
        if n == 0 {
            return (1, 0);
        }
        if allow_emergency {
            // Emergency lines may be arbitrarily short or overfull (subject
            // to `allow_overfull`), so the normal-stage width window does not
            // apply. Keep the exhaustive scan for the fallback path.
            return (1, n);
        }

        let params = config.params().break_params;
        let tolerance = params.tolerance.0 as i128;
        let emergency = params.emergency_stretch.0.max(0) as i128;
        let width = self.paragraph.style.width.0 as i128;
        let first_indent = self.paragraph.style.first_indent.0.max(0) as i128;
        let hang = self.paragraph.style.hang.0.max(0) as i128;
        let target_min = width - first_indent;
        let target_max = width + hang;
        let max_atom = self.max_atom as i128;
        let max_glue = self.max_glue as i128;
        let min_atom = self.min_atom as i128;
        let max_stretch = self.max_stretch as i128;
        let max_shrink = self.max_shrink as i128;

        let mut k_min = n + 1;
        for k in 1..=n {
            let k = k as i128;
            let max_natural = k * max_atom + (k - 1) * max_glue;
            let max_stretch = (k - 1) * max_stretch + emergency;
            // deficit * 1000 <= tolerance * stretch is the feasibility test.
            if (max_natural - target_min) * 1000 + tolerance * max_stretch >= 0 {
                k_min = k as usize;
                break;
            }
        }

        let max_trim = (self.max_trim_left + self.max_trim_right) as i128;
        // H(k) * 1000: overfull infeasibility margin for k atoms.
        let coefficient = min_atom * 1000 - tolerance * max_shrink;
        let mut k_max = n;
        if coefficient > 0 {
            for k in 1..=n {
                let k = k as i128;
                let margin = k * min_atom * 1000
                    - max_trim * 1000
                    - target_max * 1000
                    - tolerance * (k - 1) * max_shrink;
                if margin > 0 {
                    k_max = (k as usize) - 1;
                    break;
                }
            }
        }

        (k_min, k_max)
    }

    fn eval_line(
        &self,
        start: usize,
        end: usize,
        end_kind: LineEnd,
        config: &BreakConfig,
        allow_emergency: bool,
        allow_forbidden: bool,
    ) -> Option<LineEval> {
        debug_assert!(start < end && end <= self.len());

        let target = self.target(start);
        if target <= 0 {
            return None;
        }

        let natural = self.natural(start, end);
        let shrink_total = self.total_shrink(start, end);
        let break_params = config.params().break_params;
        let costs = break_params.costs;
        let break_penalty = self.break_penalty(end);
        let line_penalty = costs.line.0 as i64;
        let mut cost = line_penalty + break_penalty_cost(break_penalty, costs.break_penalty.0);
        let forbidden_break = end < self.len()
            && self.paragraph.boundaries[end - 1].breakable == Breakability::Forbidden;
        if forbidden_break {
            if !allow_forbidden {
                return None;
            }
            cost += costs.forbidden.0 as i64;
        }
        if config.params().features.height_cost && costs.height_scale.0 > 0 {
            let vertical = self.line_vertical(start, end);
            let threshold = costs
                .height_threshold
                .unwrap_or(self.paragraph.style.line_height)
                .0 as i64;
            let excess = (vertical.height - threshold).max(0);
            let body_height = (self.paragraph.style.line_height.0 as i64).max(1);
            let height_cost = (costs.height_scale.0 as i128 * excess as i128 / body_height as i128)
                .min(i64::MAX as i128) as i64;
            cost += height_cost;
        }
        let mut adjustment = 0i32;
        let mut emergency = false;
        let mut overfull = false;
        let mut ratio_value = 0i64;
        let mut badness_value = 0i64;
        let mut short_score = 0i64;
        let mut trim_left = 0i32;
        let mut trim_right = 0i32;
        let mut compress_cost = 0i64;
        let mut effective = natural;

        // A justified line always drops the trailing bearing so its ink lands
        // on the text edge (ADR-0036). A ragged line (paragraph end, forced
        // break) is left aligned: it only trims when it would otherwise
        // overrun, which keeps its punctuation inside the block.
        let justified = end_kind == LineEnd::Justified;
        if config.params().features.edge_trim && (justified || natural > target) {
            // Never trim more than the mark's own advance, even if a rule table
            // and a metrics table disagree about its side bearing. A real
            // outline can give up more than half an em (a comma's ink sits in
            // the left third), and that is exactly what lands it on the edge.
            let mark_width = self.atom_width(end - 1);
            let right_cap =
                (self.paragraph.atoms[end - 1].compress_right.0.max(0) as i64).min(mark_width);
            let start_width = self.atom_width(start);
            let left_cap =
                (self.paragraph.atoms[start].compress_left.0.max(0) as i64).min(start_width);

            // The em box, not the ink, is what native Chinese punctuation draws
            // with. Trimming here (rather than at the renderer) keeps badness,
            // adjustment and the box coordinates in agreement; a justified line
            // then grows the inter-character glue by the same amount, so the
            // ink lands on the text edge.
            let c_end = right_cap;
            trim_right = c_end as i32;
            effective = natural - c_end;

            // Only an overfull line needs to give up the leading mark's side
            // bearing as well, and only an originally overfull line pays for
            // the trim: the trailing bearing of a short line is optical
            // alignment, not compensation.
            let mut c = c_end;
            if effective > target {
                let excess = effective - target;
                let c_start = excess.min(left_cap);
                trim_left = c_start as i32;
                effective -= c_start;
                c += c_start;
            }
            if natural > target {
                compress_cost =
                    compression_cost(c, costs.compress_amount.0, costs.compress_count.0);
                cost += compress_cost;
            }
        }

        if end_kind != LineEnd::Justified {
            overfull = effective > target;
            if overfull {
                if !allow_emergency || !break_params.fallback.allow_overfull {
                    return None;
                }
                emergency = true;
                cost += costs.emergency.0 as i64;
            } else if end_kind == LineEnd::ParagraphEnd {
                short_score = short_last_cost(
                    effective,
                    costs.short_last_threshold.0 as i64,
                    costs.short_last_scale.0,
                );
                cost += short_score;
            }

            return Some(LineEval {
                cost: Cost(cost),
                target,
                natural,
                effective,
                adjustment,
                ratio_milli: 0,
                boundary_deviation: 0,
                break_penalty,
                line_penalty,
                badness: 0,
                short_score,
                trim_left,
                trim_right,
                compression_cost: compress_cost,
                overfull,
                emergency,
                forced_end: end_kind == LineEnd::ForcedEnd,
            });
        }

        if effective < target {
            let deficit = target - effective;
            let stretch =
                self.total_stretch(start, end) + break_params.emergency_stretch.0.max(0) as i64;
            if stretch <= 0 {
                if !allow_emergency {
                    return None;
                }
                emergency = true;
                cost += costs.emergency.0 as i64;
                adjustment = saturating_i32(deficit);
            } else {
                let ratio = ratio_milli(deficit, stretch);
                ratio_value = ratio;
                if ratio > break_params.tolerance.0 as i64 {
                    if !allow_emergency {
                        return None;
                    }
                    emergency = true;
                    cost += costs.emergency.0 as i64;
                }
                badness_value = badness(ratio, costs.badness_scale.0, costs.badness_exponent);
                cost += badness_value;
                adjustment = saturating_i32(deficit);
            }
        } else if effective > target {
            let excess = effective - target;
            let shrink = shrink_total;
            // TeX calls this an overfull box: the glue cannot absorb the
            // excess, so its badness is infinite. A justified line that
            // crushed its glue past its own shrink would look broken — a
            // Latin line's spaces collapse and the words touch (D86) — so the
            // break is only reachable through the fallback ladder.
            overfull = shrink <= 0 || excess > shrink;
            if overfull {
                if !allow_emergency || !break_params.fallback.allow_overfull {
                    return None;
                }
                emergency = true;
                cost += costs.emergency.0 as i64;
                ratio_value = ratio_milli(excess, shrink.max(1));
                badness_value = badness(ratio_value, costs.badness_scale.0, costs.badness_exponent);
                cost += badness_value;
                adjustment = -saturating_i32(excess);
            } else {
                let ratio = ratio_milli(excess, shrink);
                ratio_value = ratio;
                if ratio > break_params.tolerance.0 as i64 {
                    if !allow_emergency {
                        return None;
                    }
                    emergency = true;
                    cost += costs.emergency.0 as i64;
                }
                badness_value = badness(ratio, costs.badness_scale.0, costs.badness_exponent);
                cost += badness_value;
                adjustment = -saturating_i32(excess);
            }
        }

        let (boundary_deviation, ideal_gap) =
            self.cjk_latin_deviation(start, end, adjustment as i64);
        if config.params().features.boundary_deviation && boundary_deviation > 0 {
            let boundary_cost = (costs.boundary_deviation.0 as i128 * boundary_deviation as i128
                / ideal_gap as i128)
                .min(i64::MAX as i128) as i64;
            cost += boundary_cost;
        }

        Some(LineEval {
            cost: Cost(cost),
            target,
            natural,
            effective,
            adjustment,
            ratio_milli: ratio_value,
            boundary_deviation,
            break_penalty,
            line_penalty,
            badness: badness_value,
            short_score,
            trim_left,
            trim_right,
            compression_cost: compress_cost,
            overfull,
            emergency,
            forced_end: false,
        })
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct LineVertical {
    ascent: i64,
    descent: i64,
    leading: i64,
    height: i64,
}

#[derive(Clone, Copy, Debug)]
struct LineEval {
    cost: Cost,
    target: i64,
    natural: i64,
    effective: i64,
    adjustment: i32,
    ratio_milli: i64,
    boundary_deviation: i64,
    break_penalty: i64,
    line_penalty: i64,
    badness: i64,
    short_score: i64,
    trim_left: i32,
    trim_right: i32,
    compression_cost: i64,
    overfull: bool,
    emergency: bool,
    forced_end: bool,
}

/// How a line relates to its segment and paragraph end.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LineEnd {
    /// A non-final line inside a forced segment; it is justified normally.
    Justified,
    /// The last line of the paragraph; it is ragged and gets `short_score`.
    ParagraphEnd,
    /// The last line of a forced segment that is not the paragraph end. It is
    /// not stretched and does not get `short_score`; `Line.forced_end` is set.
    ForcedEnd,
}

/// One independently broken run between forced boundaries.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Segment {
    /// Global atom index, inclusive.
    start: usize,
    /// Global atom index, exclusive.
    end: usize,
    is_paragraph_end: bool,
}

/// Split the paragraph at `Forced` boundaries. Segments are non-empty because
/// a forced boundary always separates two atoms. The first line of the
/// document keeps `first_indent` because its global start is 0; a segment after
/// a forced break starts at a global index greater than 0.
fn split_segments(shaped: &Shaped<'_>, config: &BreakConfig) -> Result<Vec<Segment>, BreakError> {
    let n = shaped.len();
    let mut segments = Vec::new();
    let mut start = 0;

    for (index, boundary) in shaped.paragraph.boundaries.iter().enumerate() {
        if boundary.breakable != Breakability::Forced {
            continue;
        }
        if !config.params().features.forced_breaks {
            return Err(BreakError::ForcedBreaksDisabled);
        }
        segments.push(Segment {
            start,
            end: index + 1,
            is_paragraph_end: false,
        });
        start = index + 1;
    }

    segments.push(Segment {
        start,
        end: n,
        is_paragraph_end: true,
    });
    Ok(segments)
}

fn end_kind(shaped: &Shaped<'_>, end: usize) -> LineEnd {
    if end == shaped.len() {
        LineEnd::ParagraphEnd
    } else if shaped.paragraph.boundaries[end - 1].breakable == Breakability::Forced {
        LineEnd::ForcedEnd
    } else {
        LineEnd::Justified
    }
}

/// Four looseness classes, the TeX fitness idea: a line is tight, normal,
/// loose, or very loose. K7 only charges a *change* of class between adjacent
/// justified lines; the absolute class is already priced by badness.
const FITNESS_CLASSES: usize = 4;
const FITNESS_NORMAL: usize = 1;

/// One rhythm-DP cell: `(cost, lines, previous start, previous class)`.
type FitnessCell = (Cost, u32, usize, u8);

fn fitness_class(ratio_milli: i64) -> usize {
    if ratio_milli < -500 {
        0
    } else if ratio_milli <= 500 {
        1
    } else if ratio_milli <= 1500 {
        2
    } else {
        3
    }
}

fn optimal(
    shaped: &Shaped<'_>,
    config: &BreakConfig,
    allow_emergency: bool,
    allow_forbidden: bool,
) -> Result<(LayoutResult, u64), BreakError> {
    let params = config.params();
    if params.features.fitness && params.break_params.costs.fitness.0 > 0 {
        return optimal_with_fitness(shaped, config, allow_emergency, allow_forbidden);
    }
    let segments = split_segments(shaped, config)?;
    let (k_min, k_max) = shaped.line_length_bounds(config, allow_emergency);
    let mut evaluated = 0u64;
    let mut chosen: Vec<(usize, usize, LineEval)> = Vec::new();
    let mut total = 0i64;
    let mut line_count = 0u32;

    for segment in &segments {
        let len = segment.end - segment.start;
        let mut best: Vec<Option<Cost>> = Vec::new();
        best.resize(len + 1, None);
        let mut best_lines: Vec<u32> = Vec::new();
        best_lines.resize(len + 1, u32::MAX);
        let mut previous: Vec<Option<usize>> = Vec::new();
        previous.resize(len + 1, None);

        best[0] = Some(Cost::ZERO);
        best_lines[0] = 0;

        for local_end in 1..=len {
            let end = segment.start + local_end;
            if local_end < len && !shaped.is_break_allowed(end, allow_forbidden) {
                continue;
            }
            let kind = if local_end == len {
                if segment.is_paragraph_end {
                    LineEnd::ParagraphEnd
                } else {
                    LineEnd::ForcedEnd
                }
            } else {
                LineEnd::Justified
            };

            let (start_begin, start_end) = if kind == LineEnd::Justified {
                if k_min > k_max || local_end < k_min {
                    (0, 0)
                } else {
                    let max_start = local_end - k_min;
                    let min_start = local_end.saturating_sub(k_max);
                    (min_start, max_start + 1)
                }
            } else {
                (0, local_end)
            };

            let mut candidate: Option<(Cost, u32, usize, LineEval)> = None;
            for local_start in start_begin..start_end {
                let Some(base) = best[local_start] else {
                    continue;
                };
                let start = segment.start + local_start;
                evaluated = evaluated.saturating_add(1);
                let Some(eval) =
                    shaped.eval_line(start, end, kind, config, allow_emergency, allow_forbidden)
                else {
                    continue;
                };

                let candidate_cost = Cost(base.0.saturating_add(eval.cost.0));
                let candidate_lines = best_lines[local_start].saturating_add(1);
                let better = match candidate {
                    None => true,
                    Some((cost, lines, _, _)) => {
                        (candidate_cost.0, candidate_lines) < (cost.0, lines)
                    }
                };
                if better {
                    candidate = Some((candidate_cost, candidate_lines, local_start, eval));
                }
            }

            if let Some((cost, lines, local_start, _)) = candidate {
                best[local_end] = Some(cost);
                best_lines[local_end] = lines;
                previous[local_end] = Some(local_start);
            }
        }

        let segment_total = best[len].ok_or(BreakError::NoFeasibleBreak)?;
        total = total.saturating_add(segment_total.0);
        line_count = line_count.saturating_add(best_lines[len]);

        let mut local_end = len;
        let mut segment_lines: Vec<(usize, usize, LineEval)> = Vec::new();
        while local_end > 0 {
            let local_start = previous[local_end].ok_or(BreakError::NoFeasibleBreak)?;
            let start = segment.start + local_start;
            let end = segment.start + local_end;
            let kind = if local_end == len {
                if segment.is_paragraph_end {
                    LineEnd::ParagraphEnd
                } else {
                    LineEnd::ForcedEnd
                }
            } else {
                LineEnd::Justified
            };
            let eval = shaped
                .eval_line(start, end, kind, config, allow_emergency, allow_forbidden)
                .ok_or(BreakError::NoFeasibleBreak)?;
            segment_lines.push((start, end, eval));
            local_end = local_start;
        }
        segment_lines.reverse();
        chosen.extend(segment_lines);
    }

    if let Some(limit) = config.params().break_params.max_lines {
        if line_count > limit {
            return Err(BreakError::TooManyLines { limit });
        }
    }

    Ok((
        build_result(shaped, &chosen, Cost(total), config, BreakerKind::Optimal),
        evaluated,
    ))
}

/// K7: the same DP as [`optimal`], with the looseness class of the previous
/// line as part of the state. Four extra states per break point; a paragraph
/// without the feature enabled never enters this function.
fn optimal_with_fitness(
    shaped: &Shaped<'_>,
    config: &BreakConfig,
    allow_emergency: bool,
    allow_forbidden: bool,
) -> Result<(LayoutResult, u64), BreakError> {
    let segments = split_segments(shaped, config)?;
    let (k_min, k_max) = shaped.line_length_bounds(config, allow_emergency);
    let weight = config.params().break_params.costs.fitness.0 as i64;
    let mut evaluated = 0u64;
    let mut chosen: Vec<(usize, usize, LineEval)> = Vec::new();
    let mut total = 0i64;
    let mut line_count = 0u32;

    for segment in &segments {
        let len = segment.end - segment.start;
        // Per local break point and per class: (cost, lines, previous start,
        // previous class).
        let mut best: Vec<[Option<FitnessCell>; FITNESS_CLASSES]> =
            alloc::vec![[None; FITNESS_CLASSES]; len + 1];
        best[0][FITNESS_NORMAL] = Some((Cost::ZERO, 0, 0, FITNESS_NORMAL as u8));

        for local_end in 1..=len {
            let end = segment.start + local_end;
            if local_end < len && !shaped.is_break_allowed(end, allow_forbidden) {
                continue;
            }
            let kind = if local_end == len {
                if segment.is_paragraph_end {
                    LineEnd::ParagraphEnd
                } else {
                    LineEnd::ForcedEnd
                }
            } else {
                LineEnd::Justified
            };

            let (start_begin, start_end) = if kind == LineEnd::Justified {
                if k_min > k_max || local_end < k_min {
                    (0, 0)
                } else {
                    let max_start = local_end - k_min;
                    let min_start = local_end.saturating_sub(k_max);
                    (min_start, max_start + 1)
                }
            } else {
                (0, local_end)
            };

            for local_start in start_begin..start_end {
                let start = segment.start + local_start;
                let Some(eval) =
                    shaped.eval_line(start, end, kind, config, allow_emergency, allow_forbidden)
                else {
                    continue;
                };
                evaluated = evaluated.saturating_add(1);
                let class = fitness_class(eval.ratio_milli);
                for previous_class in 0..FITNESS_CLASSES {
                    let Some((base, base_lines, _, _)) = best[local_start][previous_class] else {
                        continue;
                    };
                    let jump = (class as i64 - previous_class as i64).unsigned_abs() as i64;
                    let penalty = weight.saturating_mul(jump);
                    let cost = Cost(base.0.saturating_add(eval.cost.0).saturating_add(penalty));
                    let lines = base_lines.saturating_add(1);
                    let better = match best[local_end][class] {
                        None => true,
                        Some((current, current_lines, current_start, _)) => {
                            (cost.0, lines, local_start) < (current.0, current_lines, current_start)
                        }
                    };
                    if better {
                        best[local_end][class] =
                            Some((cost, lines, local_start, previous_class as u8));
                    }
                }
            }
        }

        let mut final_class = 0usize;
        let mut final_cell: Option<FitnessCell> = None;
        for (class, cell) in best[len].iter().enumerate() {
            let Some(cell) = cell else { continue };
            let better = match final_cell {
                None => true,
                Some((cost, lines, start, previous)) => {
                    (cell.0 .0, cell.1, cell.2, class) < (cost.0, lines, start, previous as usize)
                }
            };
            if better {
                final_cell = Some(*cell);
                final_class = class;
            }
        }
        let Some((segment_total, segment_lines, _, _)) = final_cell else {
            return Err(BreakError::NoFeasibleBreak);
        };
        total = total.saturating_add(segment_total.0);
        line_count = line_count.saturating_add(segment_lines);

        let mut local_end = len;
        let mut class = final_class;
        let mut reversed: Vec<(usize, usize, LineEval)> = Vec::new();
        while local_end > 0 {
            let Some((_, _, local_start, previous)) = best[local_end][class] else {
                return Err(BreakError::NoFeasibleBreak);
            };
            let start = segment.start + local_start;
            let end = segment.start + local_end;
            let kind = if local_end == len {
                if segment.is_paragraph_end {
                    LineEnd::ParagraphEnd
                } else {
                    LineEnd::ForcedEnd
                }
            } else {
                LineEnd::Justified
            };
            let eval = shaped
                .eval_line(start, end, kind, config, allow_emergency, allow_forbidden)
                .ok_or(BreakError::NoFeasibleBreak)?;
            reversed.push((start, end, eval));
            local_end = local_start;
            class = previous as usize;
        }
        reversed.reverse();
        chosen.extend(reversed);
    }

    if let Some(limit) = config.params().break_params.max_lines {
        if line_count > limit {
            return Err(BreakError::TooManyLines { limit });
        }
    }

    Ok((
        build_result(shaped, &chosen, Cost(total), config, BreakerKind::Optimal),
        evaluated,
    ))
}

fn greedy(
    shaped: &Shaped<'_>,
    config: &BreakConfig,
    mode: GreedyMode,
    allow_emergency: bool,
    allow_forbidden: bool,
) -> Result<(LayoutResult, u64), BreakError> {
    let segments = split_segments(shaped, config)?;
    let (k_min, k_max) = shaped.line_length_bounds(config, allow_emergency);
    let mut chosen: Vec<(usize, usize, LineEval)> = Vec::new();
    let mut total = 0i64;
    let mut evaluated = 0u64;

    for segment in &segments {
        let len = segment.end - segment.start;
        let mut local_start = 0;

        while local_start < len {
            let mut best_choice: Option<(usize, LineEval)> = None;

            for local_end in (local_start + 1)..=len {
                let end = segment.start + local_end;
                if local_end < len && !shaped.is_break_allowed(end, allow_forbidden) {
                    continue;
                }
                let kind = if local_end == len {
                    if segment.is_paragraph_end {
                        LineEnd::ParagraphEnd
                    } else {
                        LineEnd::ForcedEnd
                    }
                } else {
                    LineEnd::Justified
                };
                if kind == LineEnd::Justified {
                    let k = local_end - local_start;
                    if k_min > k_max || k < k_min || k > k_max {
                        continue;
                    }
                }
                evaluated = evaluated.saturating_add(1);
                let start = segment.start + local_start;
                let Some(eval) =
                    shaped.eval_line(start, end, kind, config, allow_emergency, allow_forbidden)
                else {
                    continue;
                };

                match mode {
                    GreedyMode::Furthest => {
                        best_choice = Some((local_end, eval));
                    }
                    GreedyMode::MinCost => {
                        let better = match best_choice {
                            None => true,
                            Some((current_end, current_eval)) => {
                                (eval.cost.0, local_end) < (current_eval.cost.0, current_end)
                            }
                        };
                        if better {
                            best_choice = Some((local_end, eval));
                        }
                    }
                }
            }

            let (local_end, eval) = best_choice.ok_or(BreakError::NoFeasibleBreak)?;
            total = total.saturating_add(eval.cost.0);
            chosen.push((segment.start + local_start, segment.start + local_end, eval));
            local_start = local_end;

            if let Some(limit) = config.params().break_params.max_lines {
                if chosen.len() as u32 > limit {
                    return Err(BreakError::TooManyLines { limit });
                }
            }
        }
    }

    Ok((
        build_result(
            shaped,
            &chosen,
            Cost(total),
            config,
            match mode {
                GreedyMode::Furthest => BreakerKind::GreedyF,
                GreedyMode::MinCost => BreakerKind::GreedyC,
            },
        ),
        evaluated,
    ))
}

fn build_result(
    shaped: &Shaped<'_>,
    chosen: &[(usize, usize, LineEval)],
    total: Cost,
    config: &BreakConfig,
    breaker_used: BreakerKind,
) -> LayoutResult {
    let mut lines = Vec::with_capacity(chosen.len());

    for (start, end, eval) in chosen {
        let indent = if *start == 0 {
            shaped.paragraph.style.first_indent - shaped.paragraph.style.hang
        } else {
            Width::ZERO
        };
        // A single atom has no internal boundary to stretch or shrink.
        // Keep the signed adjustment only when it can actually be distributed.
        // Left-aligned text is never adjusted: the breakpoints still come from
        // the justified cost model, but the boxes keep their natural widths.
        let adjustment =
            if end.saturating_sub(*start) <= 1 || shaped.paragraph.style.align == Align::Left {
                0
            } else {
                eval.adjustment
            };
        let trim_left = Width(eval.trim_left);
        let trim_right = Width(eval.trim_right);
        let boxes = build_boxes(
            shaped, *start, *end, indent, adjustment, trim_left, trim_right,
        );
        let cjk_latin_adjustments = cjk_latin_adjustments(shaped, *start, *end, adjustment);
        let vertical = shaped.line_vertical(*start, *end);

        lines.push(Line {
            start: *start as u32,
            end: *end as u32,
            indent,
            target_width: Width(saturating_i32(eval.target)),
            natural_width: Width(saturating_i32(eval.natural)),
            trim_left,
            trim_right,
            ratio_milli: saturating_i32(eval.ratio_milli),
            ascent: Width(saturating_i32(vertical.ascent)),
            descent: Width(saturating_i32(vertical.descent)),
            leading: Width(saturating_i32(vertical.leading)),
            height: Width(saturating_i32(vertical.height)),
            adjustment,
            boundary_deviation: eval.boundary_deviation,
            cjk_latin_adjustments,
            cost: eval.cost,
            overfull: eval.overfull,
            emergency: eval.emergency,
            forced_end: eval.forced_end,
            hyphenated: *end < shaped.len()
                && shaped.paragraph.boundaries[*end - 1].kind == BoundaryKind::Hyphen,
            boxes,
        });
    }

    let emergency_lines = lines.iter().filter(|line| line.emergency).count() as u32;
    let forbidden_violations = chosen
        .iter()
        .filter(|(_, end, _)| {
            *end < shaped.len()
                && shaped.paragraph.boundaries[*end - 1].breakable == Breakability::Forbidden
        })
        .count() as u32;

    LayoutResult {
        lines,
        total_cost: total,
        breaker_used,
        // Overwritten by `break_with_stats` when the relaxed fallback ran.
        fallback_stage: FallbackStage::Normal,
        length_limited: false,
        emergency_lines,
        forbidden_violations,
        rules_version: config.rules_version(),
        metrics_version: config.metrics_version(),
    }
}

fn build_boxes(
    shaped: &Shaped<'_>,
    start: usize,
    end: usize,
    indent: Width,
    adjustment: i32,
    trim_left: Width,
    trim_right: Width,
) -> Vec<AtomBox> {
    let mut boxes = Vec::with_capacity(end - start);
    let weights = boundary_weights(shaped, start, end, adjustment);
    let adjustments = spread(adjustment, &weights);

    let mut x = indent.0 as i64 - trim_left.0 as i64;
    for atom_index in start..end {
        let mut advance = shaped.atom_width(atom_index);
        if atom_index + 1 == end {
            advance -= trim_right.0 as i64;
        }

        boxes.push(AtomBox {
            atom: atom_index as u32,
            x: Width(saturating_i32(x)),
            advance: Width(saturating_i32(advance)),
        });

        x += advance;

        if atom_index + 1 < end {
            let boundary_index = atom_index - start;
            x += shaped.glue_width(atom_index);
            x += adjustments.get(boundary_index).copied().unwrap_or(0) as i64;
        }
    }

    boxes
}

fn cjk_latin_adjustments(
    shaped: &Shaped<'_>,
    start: usize,
    end: usize,
    adjustment: i32,
) -> Vec<CjkLatinAdjust> {
    if adjustment == 0 || end <= start + 1 {
        return Vec::new();
    }

    let weights = boundary_weights(shaped, start, end, adjustment);
    let adjustments = spread(adjustment, &weights);
    let mut out = Vec::new();

    for (offset, adjustment) in adjustments.iter().enumerate() {
        let boundary_index = start + offset;
        if shaped.paragraph.boundaries[boundary_index].role == BoundaryRole::CjkLatin {
            out.push(CjkLatinAdjust {
                boundary: boundary_index as u32,
                adjustment: *adjustment,
            });
        }
    }

    out
}

/// Distribute an adjustment proportionally to the stretch (positive total) or
/// shrink (negative total) of each internal boundary. Remainder is assigned to
/// the first boundaries, making the result deterministic.
fn boundary_weights(shaped: &Shaped<'_>, start: usize, end: usize, adjustment: i32) -> Vec<i64> {
    let use_stretch = adjustment >= 0;
    let mut weights = Vec::with_capacity(end.saturating_sub(start).saturating_sub(1));

    for boundary_index in start..end.saturating_sub(1) {
        let weight = shaped.boundary_weight(boundary_index, use_stretch);
        weights.push(weight);
    }

    weights
}

/// Spread a justification adjustment over the boundaries that can actually
/// take it. A boundary with no stretch (a hyphenation point, two objects set
/// against each other) never opens up; a line with nothing to stretch is left
/// short, which is what a compositor sees as an underfull line. Smearing the
/// adjustment evenly instead would put space between the letters of a word.
fn spread(adjustment: i32, weights: &[i64]) -> Vec<i32> {
    let can_grow = weights.iter().any(|weight| *weight > 0);
    let can_shrink = adjustment < 0 && weights.iter().any(|weight| *weight < 0);
    if can_grow || can_shrink {
        distribute(adjustment as i64, weights)
    } else {
        alloc::vec![0; weights.len()]
    }
}

fn distribute(total: i64, weights: &[i64]) -> Vec<i32> {
    if weights.is_empty() {
        return Vec::new();
    }

    let count = weights.len();
    let sum: i64 = weights.iter().sum();
    let mut out = Vec::with_capacity(count);

    if sum <= 0 {
        let base = total / count as i64;
        let remainder = total % count as i64;
        let step = remainder.signum();
        for index in 0..count {
            let share = base
                + if (index as i64) < remainder.abs() {
                    step
                } else {
                    0
                };
            out.push(saturating_i32(share));
        }
        return out;
    }

    let mut allocated = 0i64;
    for weight in weights {
        let share = total.saturating_mul(*weight) / sum;
        allocated += share;
        out.push(saturating_i32(share));
    }

    let remainder = total - allocated;
    if remainder != 0 {
        let step = remainder.signum();
        for entry in out.iter_mut().take(remainder.unsigned_abs() as usize) {
            *entry = saturating_i32(*entry as i64 + step);
        }
    }

    out
}

fn ratio_milli(value: i64, capacity: i64) -> i64 {
    if capacity <= 0 {
        0
    } else {
        value.saturating_mul(1000) / capacity
    }
}

fn break_penalty_cost(penalty: i64, weight: u32) -> i64 {
    if penalty <= 0 || weight == 0 {
        return 0;
    }
    let value = (penalty as i128) * (weight as i128) / 100;
    value.min(i64::MAX as i128) as i64
}

fn compression_cost(trim: i64, amount_weight: u32, count_weight: u32) -> i64 {
    if trim <= 0 {
        return 0;
    }
    let amount =
        (amount_weight as i128 * trim as i128 / Width::EM.0 as i128).min(i64::MAX as i128) as i64;
    amount.saturating_add(count_weight as i64)
}

fn badness(ratio_milli: i64, scale: u32, exponent: u8) -> i64 {
    let ratio = ratio_milli.unsigned_abs().min(BADNESS_RATIO_CLAMP as u64);

    // `default-v1` uses exponent 3 and a small scale. Keep that path in u64
    // (cheaper division); fall back to u128 when the power or product would
    // overflow. The formula and rounding are unchanged.
    if let Some(denominator) = 1000u64.checked_pow(exponent as u32) {
        if let Some(power) = ratio.checked_pow(exponent as u32) {
            if let Some(product) = (scale as u64).checked_mul(power) {
                return (product / denominator) as i64;
            }
        }
    }

    let mut power = 1u128;
    let mut denominator = 1u128;
    for _ in 0..exponent {
        power = power.saturating_mul(ratio as u128);
        denominator = denominator.saturating_mul(1000);
    }
    let value = (scale as u128).saturating_mul(power) / denominator;
    value.min(i64::MAX as u128) as i64
}

fn short_last_cost(natural: i64, threshold: i64, scale: u32) -> i64 {
    if threshold <= 0 || scale == 0 {
        return 0;
    }
    let short = (threshold - natural).max(0);
    let value = (short as i128) * (scale as i128) / (threshold as i128);
    value.min(i64::MAX as i128) as i64
}

fn saturating_i32(value: i64) -> i32 {
    value.clamp(i32::MIN as i64, i32::MAX as i64) as i32
}

fn prefix_sums(values: &[i64]) -> Vec<i64> {
    let mut out = Vec::with_capacity(values.len() + 1);
    out.push(0);
    let mut sum = 0i64;
    for value in values {
        sum += *value;
        out.push(sum);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{Atom, AtomKind, Boundary, Decorations, Glue, ParagraphStyle, PunctClass};
    use crate::metrics::{StyleId, SyntheticMetrics};
    use alloc::string::ToString;
    use alloc::vec;

    fn atom(text: &str, kind: crate::ir::AtomKind, class: PunctClass) -> Atom {
        Atom {
            text: text.to_string(),
            kind,
            class,
            style: StyleId(0),
            decor: Decorations::NONE,
            link: None,
            compress_left: Width::ZERO,
            compress_right: Width::ZERO,
        }
    }

    fn boundary(breakable: Breakability, glue: Glue) -> Boundary {
        Boundary {
            breakable,
            kind: BoundaryKind::Normal,
            role: crate::ir::BoundaryRole::Plain,
            glue,
            penalty: 0,
        }
    }

    fn punctuation_paragraph(width_em: i32) -> Paragraph {
        let glue = Glue {
            width: Width::ZERO,
            stretch: Width::from_em_frac(1, 2),
            shrink: Width::from_em_frac(1, 2),
        };

        Paragraph {
            style: ParagraphStyle {
                width: Width::from_em(width_em),
                first_indent: Width::ZERO,
                hang: Width::ZERO,
                tracking: Width::ZERO,
                line_height: Width::from_em_frac(3, 2),
                align: Align::Justify,
            },
            atoms: vec![
                atom("中", AtomKind::Cjk, PunctClass::None),
                atom("文", AtomKind::Cjk, PunctClass::None),
                atom("，", AtomKind::Cjk, PunctClass::Middle),
                atom("混", AtomKind::Cjk, PunctClass::None),
                atom("排", AtomKind::Cjk, PunctClass::None),
            ],
            boundaries: vec![
                boundary(Breakability::Allowed, glue),
                boundary(Breakability::Forbidden, Glue::default()),
                boundary(Breakability::Allowed, glue),
                boundary(Breakability::Allowed, glue),
            ],
        }
    }

    fn two_atom_paragraph(width_em: i32) -> Paragraph {
        Paragraph {
            style: ParagraphStyle {
                width: Width::from_em(width_em),
                first_indent: Width::ZERO,
                hang: Width::ZERO,
                tracking: Width::ZERO,
                line_height: Width::from_em_frac(3, 2),
                align: Align::Justify,
            },
            atoms: vec![
                atom("中", AtomKind::Cjk, PunctClass::None),
                atom("文", AtomKind::Cjk, PunctClass::None),
            ],
            boundaries: vec![boundary(Breakability::Allowed, Glue::default())],
        }
    }

    fn tie_paragraph() -> Paragraph {
        Paragraph {
            style: ParagraphStyle {
                width: Width::from_em(2),
                first_indent: Width::ZERO,
                hang: Width::ZERO,
                tracking: Width::ZERO,
                line_height: Width::from_em_frac(3, 2),
                align: Align::Justify,
            },
            atoms: vec![
                atom("甲甲", AtomKind::Cjk, PunctClass::None),
                atom("乙", AtomKind::Cjk, PunctClass::None),
                atom("丙", AtomKind::Cjk, PunctClass::None),
            ],
            boundaries: vec![
                boundary(
                    Breakability::Allowed,
                    Glue {
                        width: Width::ZERO,
                        stretch: Width::ZERO,
                        shrink: Width::from_em(1),
                    },
                ),
                boundary(Breakability::Allowed, Glue::default()),
            ],
        }
    }

    fn uniform_paragraph(atom_count: usize, width_em: i32) -> Paragraph {
        Paragraph {
            style: ParagraphStyle {
                width: Width::from_em(width_em),
                first_indent: Width::ZERO,
                hang: Width::ZERO,
                tracking: Width::ZERO,
                line_height: Width::from_em_frac(3, 2),
                align: Align::Justify,
            },
            atoms: (0..atom_count)
                .map(|_| atom("中", AtomKind::Cjk, PunctClass::None))
                .collect(),
            boundaries: (0..atom_count.saturating_sub(1))
                .map(|_| boundary(Breakability::Allowed, Glue::default()))
                .collect(),
        }
    }

    #[test]
    fn optimal_respects_forbidden_break() {
        let paragraph = punctuation_paragraph(3);
        let metrics = SyntheticMetrics::default();
        let config = BreakConfig::default();
        let result = break_paragraph(&paragraph, &metrics, &config).unwrap();

        // Ending a line at atom 2 would break on the forbidden boundary before
        // the Chinese comma.
        assert!(result.lines.iter().all(|line| line.end != 2));
        assert_eq!(result.lines.last().unwrap().end, 5);
    }

    #[test]
    fn optimal_is_not_worse_than_min_cost_greedy() {
        let paragraph = punctuation_paragraph(3);
        let metrics = SyntheticMetrics::default();
        let config = BreakConfig::default();

        let optimal = break_paragraph(&paragraph, &metrics, &config).unwrap();
        let greedy =
            break_paragraph_greedy(&paragraph, &metrics, &config, GreedyMode::MinCost).unwrap();

        assert!(optimal.total_cost <= greedy.total_cost);
    }

    #[test]
    fn canonical_result_is_deterministic() {
        let paragraph = punctuation_paragraph(3);
        let metrics = SyntheticMetrics::default();
        let config = BreakConfig::default();

        let first = break_paragraph(&paragraph, &metrics, &config).unwrap();
        let second = break_paragraph(&paragraph, &metrics, &config).unwrap();

        assert_eq!(first, second);
        assert_eq!(first.canonical_json(), second.canonical_json());
    }

    #[test]
    fn unsupported_window_is_reported() {
        let paragraph = punctuation_paragraph(3);
        let metrics = SyntheticMetrics::default();
        let config = BreakConfig::new(crate::params::ParamSet::default_v1(), Window::Page);

        assert_eq!(
            break_paragraph(&paragraph, &metrics, &config),
            Err(BreakError::UnsupportedWindow(Window::Page))
        );
    }

    #[test]
    fn distribute_sums_to_total() {
        assert_eq!(distribute(10, &[1, 1, 1]), vec![4, 3, 3]);
        assert_eq!(distribute(-10, &[1, 1, 1]), vec![-4, -3, -3]);
        assert_eq!(distribute(0, &[1, 1, 1]), vec![0, 0, 0]);
        assert_eq!(distribute(5, &[0, 0, 0]), vec![2, 2, 1]);
    }

    #[test]
    fn short_last_cost_only_applies_below_two_ems() {
        assert_eq!(
            short_last_cost(Width::EM.0 as i64, Width::from_em(2).0 as i64, 1_000_000),
            500_000
        );
        assert_eq!(
            short_last_cost(
                (Width::EM.0 as i64) * 2,
                Width::from_em(2).0 as i64,
                1_000_000
            ),
            0
        );
        assert_eq!(
            short_last_cost(
                (Width::EM.0 as i64) * 3,
                Width::from_em(2).0 as i64,
                1_000_000
            ),
            0
        );
    }

    #[test]
    fn last_line_counts_line_cost() {
        let paragraph = two_atom_paragraph(2);
        let metrics = SyntheticMetrics::default();
        let result = break_paragraph(&paragraph, &metrics, &BreakConfig::default()).unwrap();

        assert_eq!(result.lines.len(), 1);
        assert_eq!(result.total_cost, Cost(10));
    }

    #[test]
    fn tie_break_prefers_the_longer_last_line() {
        use crate::params::{ParamSet, Weight};

        let paragraph = tie_paragraph();
        let metrics = SyntheticMetrics::default();
        let mut params = ParamSet::default_v1();
        params.break_params.costs.badness_scale = Weight(0);
        params.break_params.costs.short_last_scale = Weight(0);
        let config = BreakConfig::from_params(params);

        let result = break_paragraph(&paragraph, &metrics, &config).unwrap();

        assert_eq!(result.lines.len(), 2);
        assert_eq!(result.lines[1].start, 1);
        assert_eq!(result.total_cost, Cost(20));
    }

    #[test]
    fn brute_force_matches_dp() {
        let paragraph = punctuation_paragraph(3);
        let metrics = SyntheticMetrics::default();
        let config = BreakConfig::default();
        let shaped = Shaped::new(&paragraph, &metrics);

        let brute = brute_force_cost(&shaped, &config).expect("fixture must be feasible");
        let (optimal, _) =
            optimal(&shaped, &config, false, false).expect("fixture must be feasible");

        assert_eq!(optimal.total_cost.0, brute);
    }

    #[test]
    fn brute_force_matches_dp_across_lengths() {
        let metrics = SyntheticMetrics::default();
        let config = BreakConfig::default();

        for atom_count in 3..=10 {
            for width_em in 2..=3 {
                let paragraph = uniform_paragraph(atom_count, width_em);
                let shaped = Shaped::new(&paragraph, &metrics);
                let brute = brute_force_cost(&shaped, &config).expect("fixture must be feasible");
                let (optimal, _) =
                    optimal(&shaped, &config, false, false).expect("fixture must be feasible");
                assert_eq!(
                    optimal.total_cost.0, brute,
                    "n={atom_count}, width={width_em}"
                );
            }
        }
    }

    #[test]
    fn brute_force_matches_dp_with_forced_breaks() {
        let mut paragraph = punctuation_paragraph(3);
        paragraph.boundaries[1] = boundary(Breakability::Forced, Glue::default());

        let metrics = SyntheticMetrics::default();
        let config = BreakConfig::default();
        let shaped = Shaped::new(&paragraph, &metrics);

        let brute = brute_force_cost(&shaped, &config).expect("fixture must be feasible");
        let (optimal, _) =
            optimal(&shaped, &config, false, false).expect("fixture must be feasible");

        assert_eq!(optimal.total_cost.0, brute);
        assert!(optimal.lines[0].forced_end);
        assert_eq!(optimal.lines[0].end, 2);
    }

    #[test]
    fn pruning_matches_brute_force_on_varied_widths() {
        let mut paragraph = punctuation_paragraph(5);
        // Make the latin run longer and give every internal boundary room to
        // stretch/shrink, so the atom-count window is neither trivial nor
        // equivalent to a fixed line length.
        paragraph.atoms[1] = atom("latin", AtomKind::LatinRun, PunctClass::None);
        let glue = Glue {
            width: Width::ZERO,
            stretch: Width::from_em_frac(1, 10),
            shrink: Width::from_em_frac(1, 10),
        };
        for boundary in &mut paragraph.boundaries {
            boundary.glue = glue;
        }

        let metrics = SyntheticMetrics::default();
        let config = BreakConfig::default();
        let shaped = Shaped::new(&paragraph, &metrics);

        let brute = brute_force_cost(&shaped, &config).expect("fixture must be feasible");
        let (optimal, _) =
            optimal(&shaped, &config, false, false).expect("fixture must be feasible");

        assert_eq!(optimal.total_cost.0, brute);
    }

    #[test]
    fn pruning_reduces_the_candidate_count() {
        let paragraph = uniform_paragraph(100, 20);
        let metrics = SyntheticMetrics::default();
        let (result, trace) =
            break_paragraph_traced(&paragraph, &metrics, &BreakConfig::default()).unwrap();

        assert_eq!(result.lines.len(), 5);
        // Exhaustive would be 100*99/2 = 4,950 evaluations.
        assert!(trace.evaluated_candidates < 2_000);
    }

    fn brute_force_cost(shaped: &Shaped<'_>, config: &BreakConfig) -> Option<i64> {
        let segments = split_segments(shaped, config).ok()?;
        let mut grand = 0i64;

        for segment in &segments {
            let len = segment.end - segment.start;
            let mut best: Option<i64> = None;

            for mask in 0u32..(1u32 << len.saturating_sub(1)) {
                let mut local_ends = Vec::new();
                for local_end in 1..len {
                    if mask & (1u32 << (local_end - 1)) != 0 {
                        local_ends.push(local_end);
                    }
                }
                local_ends.push(len);

                let mut local_start = 0;
                let mut total = 0i64;
                let mut feasible = true;
                for local_end in local_ends {
                    let end = segment.start + local_end;
                    if local_end < len && !shaped.is_break_allowed(end, false) {
                        feasible = false;
                        break;
                    }
                    let kind = if local_end == len {
                        if segment.is_paragraph_end {
                            LineEnd::ParagraphEnd
                        } else {
                            LineEnd::ForcedEnd
                        }
                    } else {
                        LineEnd::Justified
                    };
                    let Some(eval) = shaped.eval_line(
                        segment.start + local_start,
                        end,
                        kind,
                        config,
                        false,
                        false,
                    ) else {
                        feasible = false;
                        break;
                    };
                    total = total.saturating_add(eval.cost.0);
                    local_start = local_end;
                }

                if feasible {
                    best = Some(best.map_or(total, |current| current.min(total)));
                }
            }

            grand = grand.checked_add(best?)?;
        }

        Some(grand)
    }
}
