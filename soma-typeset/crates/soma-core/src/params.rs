//! Parameter model.
//!
//! The public shapes follow `docs/params.md`. K1 implements the frozen
//! `default-v1` values, validation, and the deterministic parameter hash.

use crate::units::Width;

/// Rule-layer penalty normalization upper bound.
pub const PENALTY_REF: u32 = 100;

/// Sentinel used when a line has no finite stretch/shrink ratio.
pub const BADNESS_RATIO_CLAMP: u32 = 100_000;

/// Non-negative cost weight.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Weight(pub u32);

/// Per-mille value: `1000` means `1.0`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Permille(pub u16);

/// Stable preset identifier.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PresetId(pub &'static str);

/// Deterministic hash of `BreakParams` + feature flags.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ParamHash(pub u64);

/// Tunable cost weights. All quantities are normalized by `EM`, `ideal_gap`,
/// `PENALTY_REF`, or `body_height` before the weight is applied.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CostParams {
    pub line: Weight,
    pub badness_scale: Weight,
    pub badness_exponent: u8,
    pub break_penalty: Weight,
    pub compress_amount: Weight,
    pub compress_count: Weight,
    pub boundary_deviation: Weight,
    pub short_last_scale: Weight,
    pub short_last_threshold: Width,
    pub height_scale: Weight,
    pub height_threshold: Option<Width>,
    pub emergency: Weight,
    pub forbidden: Weight,
    pub fitness: Weight,
}

/// Fallback ladder switches.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FallbackParams {
    pub allow_emergency: bool,
    pub allow_forbidden: bool,
    pub allow_overfull: bool,
    pub emergency_stretch: Width,
}

/// Feature switches that are part of the parameter hash.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FeatureFlags {
    pub edge_trim: bool,
    pub boundary_deviation: bool,
    pub height_cost: bool,
    pub fitness: bool,
    pub forced_breaks: bool,
}

/// Trim order at line edges.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrimPriority {
    EndFirst,
    StartFirst,
    Balanced,
}

/// Deterministic tie-break policy.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TieBreak {
    CostLineStart,
}

/// Tier A parameters read by the breaker.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BreakParams {
    pub tolerance: Permille,
    pub emergency_tolerance: Permille,
    pub max_lines: Option<u32>,
    pub max_kp_atoms: Option<u32>,
    pub trim_priority: TrimPriority,
    pub tie_break: TieBreak,
    pub costs: CostParams,
    pub fallback: FallbackParams,
}

/// Parameter validation error with a stable path.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ParamError {
    pub path: &'static str,
    pub reason: &'static str,
}

impl core::fmt::Display for ParamError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}: {}", self.path, self.reason)
    }
}

/// One validated parameter set.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ParamSet {
    pub preset: PresetId,
    pub model_version: u16,
    pub rules_version: u16,
    pub metrics_version: u16,
    pub break_params: BreakParams,
    pub features: FeatureFlags,
}

impl ParamSet {
    /// The frozen `default-v1` preset.
    pub fn default_v1() -> Self {
        Self {
            preset: PresetId("default-v1"),
            // 4: the short-line term also charges a short non-final line
            // (D115). 2: ADR-0111/0112 changed code segmentation and the
            // separator penalties.
            model_version: 4,
            rules_version: 2,
            metrics_version: 0,
            break_params: BreakParams {
                tolerance: Permille(3000),
                emergency_tolerance: Permille(10_000),
                max_lines: None,
                max_kp_atoms: None,
                trim_priority: TrimPriority::EndFirst,
                tie_break: TieBreak::CostLineStart,
                costs: CostParams {
                    line: Weight(10),
                    badness_scale: Weight(100),
                    badness_exponent: 3,
                    break_penalty: Weight(100),
                    compress_amount: Weight(20),
                    compress_count: Weight(5),
                    boundary_deviation: Weight(50),
                    short_last_scale: Weight(1_000_000),
                    short_last_threshold: Width::from_em(2),
                    height_scale: Weight(0),
                    height_threshold: None,
                    emergency: Weight(10_000_000),
                    forbidden: Weight(100_000_000),
                    fitness: Weight(0),
                },
                fallback: FallbackParams {
                    allow_emergency: true,
                    allow_forbidden: true,
                    allow_overfull: true,
                    emergency_stretch: Width::from_em(3),
                },
            },
            features: FeatureFlags {
                edge_trim: true,
                boundary_deviation: true,
                height_cost: false,
                fitness: false,
                forced_breaks: true,
            },
        }
    }

    /// Chinese publishing defaults: the same cost model as `default-v1` with
    /// a tighter normal-path tolerance. The engine ships this preset; the
    /// blind test and the interviews decide whether it stays.
    pub fn publish_v1() -> Self {
        let mut params = Self::default_v1();
        params.preset = PresetId("publish-v1");
        params.break_params.tolerance = Permille(1_500);
        params
    }

    /// Chinese book defaults: `publish-v1` plus a larger short-last-line
    /// threshold, so a two-character tail is charged like the widow it is.
    pub fn book_v1() -> Self {
        let mut params = Self::publish_v1();
        params.preset = PresetId("book-v1");
        params.break_params.costs.short_last_threshold = Width::from_em(3);
        params
    }

    /// The proposal preset: `book-v1` with a five-character short-tail
    /// threshold and a doubled charge. A proposal's last line should reach
    /// into the measure; a 4–5 character tail is a visible orphan even though
    /// a book would tolerate it. The Studio uses this preset; the command line
    /// keeps `latex-v1` (ADR-0115).
    pub fn proposal_v1() -> Self {
        let mut params = Self::book_v1();
        params.preset = PresetId("proposal-v1");
        params.break_params.costs.short_last_threshold = Width::from_em(8);
        params.break_params.costs.short_last_scale = Weight(2_000_000);
        params
    }

    /// LaTeX article defaults: `\tolerance=200` (badness, which is
    /// `100 * |r|^3` here, so `|r| <= 2^(1/3) ~= 1.26`), and no
    /// short-last-line penalty (TeX leaves the last line ragged).
    ///
    /// TeX's own `\emergencystretch` is `0pt`; the rescue pass still uses the
    /// engine's 3em, because TeX would emit an overfull box there while we
    /// prefer one very loose line. The rescue is not part of the normal path.
    pub fn latex_v1() -> Self {
        let mut params = Self::default_v1();
        params.preset = PresetId("latex-v1");
        params.break_params.tolerance = Permille(1_260);
        params.break_params.costs.short_last_scale = Weight(0);
        params
    }

    /// `publish-v1` plus K7: adjacent justified lines may not jump more than
    /// one looseness class for free. One class costs one line's worth.
    pub fn rhythm_v1() -> Self {
        let mut params = Self::publish_v1();
        params.preset = PresetId("rhythm-v1");
        params.break_params.costs.fitness = Weight(10);
        params.features.fitness = true;
        params
    }

    /// Validate ranges and cross-field constraints.
    pub fn validate(&self) -> Result<(), ParamError> {
        let break_params = self.break_params;
        let costs = break_params.costs;

        if break_params.tolerance.0 > 10_000 {
            return Err(ParamError {
                path: "break_params.tolerance",
                reason: "must be <= 10000 permille",
            });
        }
        if break_params.emergency_tolerance.0 > 10_000 {
            return Err(ParamError {
                path: "break_params.emergency_tolerance",
                reason: "must be <= 10000 permille",
            });
        }
        if break_params.emergency_tolerance.0 < break_params.tolerance.0 {
            return Err(ParamError {
                path: "break_params.emergency_tolerance",
                reason: "must be >= tolerance",
            });
        }
        if let Some(limit) = break_params.max_lines {
            if limit == 0 || limit > 10_000 {
                return Err(ParamError {
                    path: "break_params.max_lines",
                    reason: "must be in 1..=10000",
                });
            }
        }
        if let Some(limit) = break_params.max_kp_atoms {
            if limit == 0 || limit > 10_000 {
                return Err(ParamError {
                    path: "break_params.max_kp_atoms",
                    reason: "must be in 1..=10000",
                });
            }
        }
        if self.features.edge_trim && break_params.trim_priority != TrimPriority::EndFirst {
            return Err(ParamError {
                path: "break_params.trim_priority",
                reason: "only EndFirst is implemented in model v1",
            });
        }
        if !(1..=5).contains(&costs.badness_exponent) {
            return Err(ParamError {
                path: "break_params.costs.badness_exponent",
                reason: "must be in 1..=5",
            });
        }
        if costs.badness_scale.0 > 1_000_000 {
            return Err(ParamError {
                path: "break_params.costs.badness_scale",
                reason: "must be <= 1000000",
            });
        }
        for (path, weight) in [
            ("break_params.costs.line", costs.line),
            ("break_params.costs.break_penalty", costs.break_penalty),
            ("break_params.costs.compress_amount", costs.compress_amount),
            ("break_params.costs.compress_count", costs.compress_count),
            (
                "break_params.costs.boundary_deviation",
                costs.boundary_deviation,
            ),
            (
                "break_params.costs.short_last_scale",
                costs.short_last_scale,
            ),
            ("break_params.costs.height_scale", costs.height_scale),
            ("break_params.costs.emergency", costs.emergency),
            ("break_params.costs.forbidden", costs.forbidden),
            ("break_params.costs.fitness", costs.fitness),
        ] {
            if weight.0 > 1_000_000_000 {
                return Err(ParamError {
                    path,
                    reason: "must be <= 1000000000",
                });
            }
        }
        if costs.short_last_threshold < Width::ZERO
            || costs.short_last_threshold > Width::from_em(8)
        {
            return Err(ParamError {
                path: "break_params.costs.short_last_threshold",
                reason: "must be in 0..=8em",
            });
        }
        if let Some(threshold) = costs.height_threshold {
            if threshold < Width::ZERO || threshold > Width::from_em(5) {
                return Err(ParamError {
                    path: "break_params.costs.height_threshold",
                    reason: "must be in 0..=5em",
                });
            }
            if costs.height_scale.0 > 0 && threshold == Width::ZERO {
                return Err(ParamError {
                    path: "break_params.costs.height_threshold",
                    reason: "must be > 0 when height_scale > 0",
                });
            }
        }
        if break_params.fallback.allow_forbidden && !break_params.fallback.allow_emergency {
            return Err(ParamError {
                path: "break_params.fallback.allow_forbidden",
                reason: "requires allow_emergency",
            });
        }
        if break_params.fallback.emergency_stretch < Width::ZERO
            || break_params.fallback.emergency_stretch > Width::from_em(10)
        {
            return Err(ParamError {
                path: "break_params.fallback.emergency_stretch",
                reason: "must be in 0..=10em",
            });
        }

        Ok(())
    }

    /// Hash `BreakParams` + feature flags; versions and preset id are separate.
    pub fn param_hash(&self) -> ParamHash {
        let mut hasher = ParamHasher::new();
        let params = self.break_params;
        let costs = params.costs;

        hasher.write_u64(params.tolerance.0 as u64);
        hasher.write_u64(params.emergency_tolerance.0 as u64);
        hasher.write_option_u32(params.max_lines);
        hasher.write_option_u32(params.max_kp_atoms);
        hasher.write_u8(match params.trim_priority {
            TrimPriority::EndFirst => 0,
            TrimPriority::StartFirst => 1,
            TrimPriority::Balanced => 2,
        });
        hasher.write_u8(match params.tie_break {
            TieBreak::CostLineStart => 0,
        });

        hasher.write_u64(costs.line.0 as u64);
        hasher.write_u64(costs.badness_scale.0 as u64);
        hasher.write_u8(costs.badness_exponent);
        hasher.write_u64(costs.break_penalty.0 as u64);
        hasher.write_u64(costs.compress_amount.0 as u64);
        hasher.write_u64(costs.compress_count.0 as u64);
        hasher.write_u64(costs.boundary_deviation.0 as u64);
        hasher.write_u64(costs.short_last_scale.0 as u64);
        hasher.write_i64(costs.short_last_threshold.0 as i64);
        hasher.write_u64(costs.height_scale.0 as u64);
        hasher.write_option_i32(costs.height_threshold.map(|width| width.0));
        hasher.write_u64(costs.emergency.0 as u64);
        hasher.write_u64(costs.forbidden.0 as u64);
        hasher.write_u64(costs.fitness.0 as u64);

        hasher.write_bool(params.fallback.allow_emergency);
        hasher.write_bool(params.fallback.allow_forbidden);
        hasher.write_bool(params.fallback.allow_overfull);
        hasher.write_i64(params.fallback.emergency_stretch.0 as i64);

        hasher.write_bool(self.features.edge_trim);
        hasher.write_bool(self.features.boundary_deviation);
        hasher.write_bool(self.features.height_cost);
        hasher.write_bool(self.features.fitness);
        hasher.write_bool(self.features.forced_breaks);

        hasher.finish()
    }
}

/// FNV-1a over logical fields in a fixed order. This is intentionally small
/// and dependency-free so native and wasm produce the same hash.
struct ParamHasher {
    state: u64,
}

impl ParamHasher {
    const fn new() -> Self {
        Self {
            state: 0xcbf2_9ce4_8422_2325,
        }
    }

    fn write_u8(&mut self, value: u8) {
        self.write_u64(value as u64);
    }

    fn write_u64(&mut self, value: u64) {
        for byte in value.to_le_bytes() {
            self.state ^= byte as u64;
            self.state = self.state.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }

    fn write_i64(&mut self, value: i64) {
        self.write_u64(value as u64);
    }

    fn write_bool(&mut self, value: bool) {
        self.write_u8(u8::from(value));
    }

    fn write_option_u32(&mut self, value: Option<u32>) {
        match value {
            Some(value) => {
                self.write_u8(1);
                self.write_u64(value as u64);
            }
            None => self.write_u8(0),
        }
    }

    fn write_option_i32(&mut self, value: Option<i32>) {
        match value {
            Some(value) => {
                self.write_u8(1);
                self.write_i64(value as i64);
            }
            None => self.write_u8(0),
        }
    }

    fn finish(self) -> ParamHash {
        ParamHash(self.state)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn publish_v1_only_tightens_the_tolerance() {
        let base = ParamSet::default_v1();
        let publish = ParamSet::publish_v1();
        assert_eq!(publish.preset.0, "publish-v1");
        assert_eq!(publish.break_params.tolerance, Permille(1_500));
        assert_eq!(
            publish.break_params.costs.line,
            base.break_params.costs.line
        );
        assert_eq!(
            publish.break_params.costs.badness_scale,
            base.break_params.costs.badness_scale
        );
        assert_eq!(publish.break_params.fallback, base.break_params.fallback);
        assert_eq!(publish.rules_version, 2, "ADR-0111/0112 changed the rules");
        publish.validate().expect("publish-v1 must be valid");
        assert_ne!(publish.param_hash(), base.param_hash());
    }

    #[test]
    fn proposal_v1_charges_a_longer_tail_harder() {
        let book = ParamSet::book_v1();
        let proposal = ParamSet::proposal_v1();
        assert_eq!(proposal.preset.0, "proposal-v1");
        assert_eq!(
            proposal.break_params.costs.short_last_threshold,
            Width::from_em(8)
        );
        assert_eq!(
            proposal.break_params.costs.short_last_scale,
            Weight(2_000_000)
        );
        // Only the short-tail knobs move; the rest is `book-v1`.
        assert_eq!(
            proposal.break_params.costs.line,
            book.break_params.costs.line
        );
        assert_eq!(proposal.break_params.tolerance, book.break_params.tolerance);
        proposal.validate().expect("proposal-v1 must be valid");
        assert_ne!(proposal.param_hash(), book.param_hash());
    }

    #[test]
    fn latex_v1_matches_tex_tolerance_and_leaves_the_last_line_ragged() {
        let base = ParamSet::default_v1();
        let latex = ParamSet::latex_v1();
        assert_eq!(latex.preset.0, "latex-v1");
        // TeX's \tolerance=200 is a badness limit; badness = 100 * |r|^3, so
        // |r| <= 2^(1/3), which is 1.26 in per-mille.
        assert_eq!(latex.break_params.tolerance, Permille(1_260));
        assert_eq!(latex.break_params.costs.short_last_scale, Weight(0));
        // Only the two TeX-specific knobs move: the cost model is unchanged.
        assert_eq!(latex.model_version, base.model_version);
        assert_eq!(latex.break_params.costs.line, base.break_params.costs.line);
        assert_eq!(
            latex.break_params.costs.badness_scale,
            base.break_params.costs.badness_scale
        );
        assert_eq!(latex.break_params.fallback, base.break_params.fallback);
        latex.validate().expect("latex-v1 must be valid");
        assert_ne!(latex.param_hash(), base.param_hash());
        assert_ne!(
            latex.param_hash(),
            ParamSet::publish_v1().param_hash(),
            "a distinct preset must not collide with publish-v1"
        );
    }
}
