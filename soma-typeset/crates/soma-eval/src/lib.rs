//! Evaluation harness for `soma-typeset`.
//!
//! This crate is deliberately separate from the `no_std` core. It provides:
//!
//! - deterministic synthetic corpora for benchmarks and sensitivity sweeps,
//! - per-run metrics and P50/P95 aggregation,
//! - Tier A sensitivity tables,
//! - blind-test statistics (Wilson interval + Fleiss' kappa).
//!
//! These tools report diagnostics. They do not turn a synthetic corpus or a
//! small blind test into a quality claim; that remains Gate Q.

use std::time::Instant;

use soma_core::{
    break_paragraph, break_paragraph_traced, BreakConfig, BreakerKind, FallbackStage, Metrics,
    Paragraph, ParamSet, Permille, StyleId, SyntheticMetrics, Weight, Width,
};
use soma_rules::{justified_style, paragraph_from_text};

/// The three corpus shapes used by the K6 benchmark and sweeps.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CorpusKind {
    PureCjk,
    Mixed,
    PunctuationDense,
}

impl CorpusKind {
    pub const ALL: [Self; 3] = [Self::PureCjk, Self::Mixed, Self::PunctuationDense];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::PureCjk => "pure-cjk",
            Self::Mixed => "mixed",
            Self::PunctuationDense => "punct-dense",
        }
    }

    fn base_text(self) -> &'static str {
        match self {
            Self::PureCjk => "中文排版的核心是把规则变成可调的代价模型而不是在西文引擎上打补丁",
            Self::Mixed => "中文English混排，数字 123，排版Engine v2。",
            Self::PunctuationDense => "「引号」，（括号）；冒号：逗号，句号。问号？感叹号！……",
        }
    }
}

/// Build a deterministic paragraph with roughly `target_atoms` atoms.
///
/// The text is repeated until it has at least `target_atoms` atoms, then
/// truncated at atom boundaries. Truncation never splits an atom, so it cannot
/// create a Latin or digit run.
pub fn build_paragraph(kind: CorpusKind, target_atoms: usize, width: Width) -> Paragraph {
    let target = target_atoms.max(1);
    let base = kind.base_text();
    let mut text = String::from(base);
    let mut paragraph =
        paragraph_from_text(&text, justified_style(width), StyleId(0)).expect("base corpus");

    while paragraph.atoms.len() < target {
        text.push_str(base);
        paragraph =
            paragraph_from_text(&text, justified_style(width), StyleId(0)).expect("built corpus");
    }

    let keep = target.min(paragraph.atoms.len());
    paragraph.atoms.truncate(keep);
    paragraph.boundaries.truncate(keep.saturating_sub(1));
    paragraph
}

/// Metrics for one paragraph run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RunMetrics {
    pub atoms: u32,
    pub lines: u32,
    pub evaluated: u64,
    pub cost: i64,
    pub breaker: BreakerKind,
    pub stage: FallbackStage,
    pub length_limited: bool,
    pub emergency_lines: u32,
    pub overfull_lines: u32,
    pub forbidden_violations: u32,
    pub trim_total: i64,
    pub boundary_deviation_total: i64,
}

/// Run one paragraph and collect reporting fields. The layout is the same as
/// `break_paragraph`; the trace only supplies `evaluated_candidates`.
pub fn measure(paragraph: &Paragraph, metrics: &dyn Metrics, config: &BreakConfig) -> RunMetrics {
    let (result, trace) = break_paragraph_traced(paragraph, metrics, config)
        .expect("evaluation corpus must be breakable");

    RunMetrics {
        atoms: paragraph.atoms.len() as u32,
        lines: result.lines.len() as u32,
        evaluated: trace.evaluated_candidates,
        cost: result.total_cost.0,
        breaker: result.breaker_used,
        stage: result.fallback_stage,
        length_limited: result.length_limited,
        emergency_lines: result.emergency_lines,
        overfull_lines: result.lines.iter().filter(|line| line.overfull).count() as u32,
        forbidden_violations: result.forbidden_violations,
        trim_total: result
            .lines
            .iter()
            .map(|line| line.trim_left.0 as i64 + line.trim_right.0 as i64)
            .sum(),
        boundary_deviation_total: result
            .lines
            .iter()
            .map(|line| line.boundary_deviation)
            .sum(),
    }
}

/// Aggregate metrics over a corpus.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AggregateMetrics {
    pub paragraphs: u32,
    pub lines: u32,
    pub evaluated: u64,
    pub cost: i64,
    pub emergency_lines: u32,
    pub overfull_lines: u32,
    pub forbidden_violations: u32,
    pub trim_total: i64,
    pub boundary_deviation_total: i64,
    pub normal: u32,
    pub emergency: u32,
    pub forbidden_rescue: u32,
    pub length_limited: u32,
}

impl AggregateMetrics {
    pub fn add(&mut self, metrics: &RunMetrics) {
        self.paragraphs += 1;
        self.lines += metrics.lines;
        self.evaluated += metrics.evaluated;
        self.cost += metrics.cost;
        self.emergency_lines += metrics.emergency_lines;
        self.overfull_lines += metrics.overfull_lines;
        self.forbidden_violations += metrics.forbidden_violations;
        self.trim_total += metrics.trim_total;
        self.boundary_deviation_total += metrics.boundary_deviation_total;
        match metrics.stage {
            FallbackStage::Normal => self.normal += 1,
            FallbackStage::Emergency => self.emergency += 1,
            FallbackStage::ForbiddenRescue => self.forbidden_rescue += 1,
            FallbackStage::Overfull => self.emergency += 1,
        }
        if metrics.length_limited {
            self.length_limited += 1;
        }
    }

    pub fn add_run(&mut self, other: &Self) {
        self.paragraphs += other.paragraphs;
        self.lines += other.lines;
        self.evaluated += other.evaluated;
        self.cost += other.cost;
        self.emergency_lines += other.emergency_lines;
        self.overfull_lines += other.overfull_lines;
        self.forbidden_violations += other.forbidden_violations;
        self.trim_total += other.trim_total;
        self.boundary_deviation_total += other.boundary_deviation_total;
        self.normal += other.normal;
        self.emergency += other.emergency;
        self.forbidden_rescue += other.forbidden_rescue;
        self.length_limited += other.length_limited;
    }

    pub fn length_limited_share(&self) -> f64 {
        if self.paragraphs == 0 {
            0.0
        } else {
            self.length_limited as f64 / self.paragraphs as f64
        }
    }
}

/// One benchmark row for one corpus kind and atom count.
#[derive(Clone, Debug, PartialEq)]
pub struct BenchRow {
    pub kind: CorpusKind,
    pub target_atoms: usize,
    pub atoms: u32,
    pub p50_ms: f64,
    pub p95_ms: f64,
    pub metrics: RunMetrics,
}

/// Benchmark `sizes` for one corpus kind. Timings measure `break_paragraph`
/// only; metrics are collected afterwards with the traced API.
pub fn bench(
    kind: CorpusKind,
    sizes: &[usize],
    repeats: usize,
    config: &BreakConfig,
) -> Vec<BenchRow> {
    let metrics = SyntheticMetrics::default();
    let repeats = repeats.max(1);
    let mut rows = Vec::with_capacity(sizes.len());

    for &target_atoms in sizes {
        let paragraph = build_paragraph(kind, target_atoms, Width::from_em(20));
        let _ = break_paragraph(&paragraph, &metrics, config).expect("benchmark corpus");

        let mut samples = Vec::with_capacity(repeats);
        for _ in 0..repeats {
            let start = Instant::now();
            let _ = break_paragraph(&paragraph, &metrics, config).expect("benchmark corpus");
            samples.push(start.elapsed().as_secs_f64() * 1_000.0);
        }
        samples.sort_by(|a, b| a.partial_cmp(b).expect("timings are finite"));

        let run = measure(&paragraph, &metrics, config);
        rows.push(BenchRow {
            kind,
            target_atoms,
            atoms: run.atoms,
            p50_ms: percentile(&samples, 0.50),
            p95_ms: percentile(&samples, 0.95),
            metrics: run,
        });
    }

    rows
}

fn percentile(sorted: &[f64], quantile: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let index = ((sorted.len() as f64 - 1.0) * quantile).round() as usize;
    sorted[index]
}

/// Host peak RSS in KiB, read from `/proc/self/status` on Linux.
pub fn peak_rss_kb() -> Option<u64> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    let line = status.lines().find(|line| line.starts_with("VmHWM:"))?;
    line.split_whitespace().nth(1)?.parse().ok()
}

/// A Tier A weight that the sensitivity tool can sweep one at a time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WeightParam {
    Line,
    BreakPenalty,
    CompressAmount,
    CompressCount,
    BoundaryDeviation,
    ShortLastScale,
}

impl WeightParam {
    pub const ALL: [Self; 6] = [
        Self::Line,
        Self::BreakPenalty,
        Self::CompressAmount,
        Self::CompressCount,
        Self::BoundaryDeviation,
        Self::ShortLastScale,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Line => "line",
            Self::BreakPenalty => "break_penalty",
            Self::CompressAmount => "compress_amount",
            Self::CompressCount => "compress_count",
            Self::BoundaryDeviation => "boundary_deviation",
            Self::ShortLastScale => "short_last_scale",
        }
    }

    fn apply(self, params: &mut ParamSet, value: u32) {
        match self {
            Self::Line => params.break_params.costs.line = Weight(value),
            Self::BreakPenalty => params.break_params.costs.break_penalty = Weight(value),
            Self::CompressAmount => params.break_params.costs.compress_amount = Weight(value),
            Self::CompressCount => params.break_params.costs.compress_count = Weight(value),
            Self::BoundaryDeviation => params.break_params.costs.boundary_deviation = Weight(value),
            Self::ShortLastScale => params.break_params.costs.short_last_scale = Weight(value),
        }
    }
}

/// One row of the `(tolerance, badness_scale)` sweep.
#[derive(Clone, Debug, PartialEq)]
pub struct ToleranceSweepRow {
    pub tolerance: u16,
    pub badness_scale: u32,
    pub aggregate: AggregateMetrics,
}

/// Sweep `tolerance x badness_scale` over the given corpus kinds.
pub fn sweep_tolerance(
    kinds: &[CorpusKind],
    target_atoms: usize,
    tolerances: &[u16],
    scales: &[u32],
    base: &ParamSet,
) -> Vec<ToleranceSweepRow> {
    let metrics = SyntheticMetrics::default();
    let corpora = build_corpora(kinds, target_atoms);
    let mut rows = Vec::with_capacity(tolerances.len() * scales.len());

    for &tolerance in tolerances {
        for &badness_scale in scales {
            let mut params = *base;
            params.break_params.tolerance = Permille(tolerance);
            params.break_params.costs.badness_scale = Weight(badness_scale);
            let config = BreakConfig::from_params(params);
            let mut aggregate = AggregateMetrics::default();
            for paragraph in &corpora {
                aggregate.add(&measure(paragraph, &metrics, &config));
            }
            rows.push(ToleranceSweepRow {
                tolerance,
                badness_scale,
                aggregate,
            });
        }
    }

    rows
}

/// One row of a one-dimensional Tier A weight sweep.
#[derive(Clone, Debug, PartialEq)]
pub struct WeightSweepRow {
    pub param: WeightParam,
    pub value: u32,
    pub aggregate: AggregateMetrics,
}

/// Sweep one Tier A weight at a time.
pub fn sweep_weight(
    kinds: &[CorpusKind],
    target_atoms: usize,
    param: WeightParam,
    values: &[u32],
    base: &ParamSet,
) -> Vec<WeightSweepRow> {
    let metrics = SyntheticMetrics::default();
    let corpora = build_corpora(kinds, target_atoms);
    let mut rows = Vec::with_capacity(values.len());

    for &value in values {
        let mut params = *base;
        param.apply(&mut params, value);
        let config = BreakConfig::from_params(params);
        let mut aggregate = AggregateMetrics::default();
        for paragraph in &corpora {
            aggregate.add(&measure(paragraph, &metrics, &config));
        }
        rows.push(WeightSweepRow {
            param,
            value,
            aggregate,
        });
    }

    rows
}

fn build_corpora(kinds: &[CorpusKind], target_atoms: usize) -> Vec<Paragraph> {
    kinds
        .iter()
        .map(|kind| build_paragraph(*kind, target_atoms, Width::from_em(20)))
        .collect()
}

/// One pairwise blind-test observation. `A` is the candidate (K-P); `B` is the
/// baseline (normally Greedy-F).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Observation {
    pub paragraph: u32,
    pub evaluator: u32,
    pub choice: Choice,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Choice {
    A,
    B,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlindDecision {
    Pass,
    Inconclusive,
    Fail,
}

impl BlindDecision {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pass => "pass",
            Self::Inconclusive => "inconclusive",
            Self::Fail => "fail",
        }
    }
}

/// Blind-test report. `point` is `a_wins / paragraphs`; ties count against A.
#[derive(Clone, Debug, PartialEq)]
pub struct BlindReport {
    pub paragraphs: u32,
    pub evaluators: u32,
    pub a_wins: u32,
    pub b_wins: u32,
    pub ties: u32,
    pub unanimous_b: u32,
    pub point: f64,
    pub wilson_low: f64,
    pub wilson_high: f64,
    pub kappa: Option<f64>,
    pub sample_ok: bool,
    pub decision: BlindDecision,
}

/// Parse `paragraph,evaluator,choice` rows. A header row is ignored; `choice`
/// must be `A` or `B` (case-insensitive).
pub fn parse_observations(input: &str) -> Result<Vec<Observation>, String> {
    let mut observations = Vec::new();

    for (line_index, raw) in input.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let fields: Vec<&str> = line.split(',').map(str::trim).collect();
        if fields.len() != 3 {
            return Err(format!("line {}: expected 3 fields", line_index + 1));
        }
        if fields[0].eq_ignore_ascii_case("paragraph") {
            continue;
        }
        let paragraph = fields[0]
            .parse::<u32>()
            .map_err(|_| format!("line {}: invalid paragraph", line_index + 1))?;
        let evaluator = fields[1]
            .parse::<u32>()
            .map_err(|_| format!("line {}: invalid evaluator", line_index + 1))?;
        let choice = match fields[2].to_ascii_uppercase().as_str() {
            "A" => Choice::A,
            "B" => Choice::B,
            _ => return Err(format!("line {}: choice must be A or B", line_index + 1)),
        };
        observations.push(Observation {
            paragraph,
            evaluator,
            choice,
        });
    }

    if observations.is_empty() {
        return Err("no observations".into());
    }
    Ok(observations)
}

/// Wilson score interval for a binomial proportion.
pub fn wilson_interval(wins: u32, n: u32, z: f64) -> (f64, f64) {
    if n == 0 {
        return (0.0, 1.0);
    }
    let n = n as f64;
    let p = wins as f64 / n;
    let z2 = z * z;
    let denominator = 1.0 + z2 / n;
    let center = (p + z2 / (2.0 * n)) / denominator;
    let margin = z * ((p * (1.0 - p) / n) + (z2 / (4.0 * n * n))).sqrt() / denominator;
    ((center - margin).max(0.0), (center + margin).min(1.0))
}

/// Fleiss' kappa for a binary, complete ratings matrix.
pub fn fleiss_kappa(observations: &[Observation]) -> Option<f64> {
    if observations.is_empty() {
        return None;
    }
    let mut paragraphs: Vec<u32> = observations.iter().map(|obs| obs.paragraph).collect();
    paragraphs.sort_unstable();
    paragraphs.dedup();

    let n = paragraphs.len() as u32;
    if n == 0 || observations.len() as u32 % n != 0 {
        return None;
    }
    let k = observations.len() as u32 / n;
    if k < 2 {
        return None;
    }
    for paragraph in &paragraphs {
        if observations
            .iter()
            .filter(|obs| obs.paragraph == *paragraph)
            .count() as u32
            != k
        {
            return None;
        }
    }

    let mut p_sum = 0.0;
    let mut total_a = 0u32;
    for paragraph in &paragraphs {
        let a = observations
            .iter()
            .filter(|obs| obs.paragraph == *paragraph && obs.choice == Choice::A)
            .count() as u32;
        let b = k - a;
        total_a += a;
        let p_i = ((a * a + b * b) as f64 - k as f64) / (k as f64 * (k as f64 - 1.0));
        p_sum += p_i;
    }

    let p_bar = p_sum / n as f64;
    let p_a = total_a as f64 / (n * k) as f64;
    let p_e = p_a * p_a + (1.0 - p_a) * (1.0 - p_a);
    if (1.0 - p_e).abs() < f64::EPSILON {
        return Some(1.0);
    }
    Some((p_bar - p_e) / (1.0 - p_e))
}

/// Build a blind-test report. Gate Q needs at least 30 paragraphs, point
/// estimate >= 60%, Wilson lower bound > 50%, Fleiss kappa >= 0.2, and no
/// unanimous baseline win. Everything else is `Inconclusive`, not a pass.
pub fn blind_report(observations: &[Observation]) -> Result<BlindReport, String> {
    let mut paragraphs: Vec<u32> = observations.iter().map(|obs| obs.paragraph).collect();
    paragraphs.sort_unstable();
    paragraphs.dedup();

    let n = paragraphs.len() as u32;
    if n == 0 || observations.len() as u32 % n != 0 {
        return Err("observations do not form a complete matrix".into());
    }
    let k = observations.len() as u32 / n;

    let mut a_wins = 0u32;
    let mut b_wins = 0u32;
    let mut ties = 0u32;
    let mut unanimous_b = 0u32;

    for paragraph in &paragraphs {
        let ratings: Vec<Choice> = observations
            .iter()
            .filter(|obs| obs.paragraph == *paragraph)
            .map(|obs| obs.choice)
            .collect();
        if ratings.len() as u32 != k {
            return Err("each paragraph must have the same number of ratings".into());
        }
        let a = ratings
            .iter()
            .filter(|choice| **choice == Choice::A)
            .count() as u32;
        let b = k - a;
        if a > b {
            a_wins += 1;
        } else if b > a {
            b_wins += 1;
        } else {
            ties += 1;
        }
        if b == k {
            unanimous_b += 1;
        }
    }

    let point = a_wins as f64 / n as f64;
    let (wilson_low, wilson_high) = wilson_interval(a_wins, n, 1.96);
    let kappa = fleiss_kappa(observations);
    let sample_ok = n >= 30;
    let kappa_ok = kappa.is_some_and(|value| value >= 0.2);

    let decision = if unanimous_b > 0 || wilson_high < 0.5 {
        BlindDecision::Fail
    } else if !sample_ok || !kappa_ok || point < 0.6 || wilson_low <= 0.5 {
        BlindDecision::Inconclusive
    } else {
        BlindDecision::Pass
    };

    Ok(BlindReport {
        paragraphs: n,
        evaluators: k,
        a_wins,
        b_wins,
        ties,
        unanimous_b,
        point,
        wilson_low,
        wilson_high,
        kappa,
        sample_ok,
        decision,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn observation(paragraph: u32, evaluator: u32, choice: Choice) -> Observation {
        Observation {
            paragraph,
            evaluator,
            choice,
        }
    }

    #[test]
    fn corpus_is_deterministic_and_truncates_at_atoms() {
        for kind in CorpusKind::ALL {
            let a = build_paragraph(kind, 120, Width::from_em(20));
            let b = build_paragraph(kind, 120, Width::from_em(20));
            assert_eq!(a, b);
            assert_eq!(a.atoms.len(), 120);
            assert_eq!(a.boundaries.len(), 119);
            assert!(a.atoms.iter().all(|atom| !atom.text.is_empty()));
        }
    }

    #[test]
    fn bench_produces_one_row_per_size() {
        let config = BreakConfig::default();
        let rows = bench(CorpusKind::PureCjk, &[80, 160], 1, &config);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].atoms, 80);
        assert_eq!(rows[1].atoms, 160);
        assert!(rows.iter().all(|row| row.p50_ms >= 0.0));
    }

    #[test]
    fn tolerance_sweep_is_deterministic() {
        let base = ParamSet::default_v1();
        let first = sweep_tolerance(&[CorpusKind::PureCjk], 80, &[500, 3000], &[50, 100], &base);
        let second = sweep_tolerance(&[CorpusKind::PureCjk], 80, &[500, 3000], &[50, 100], &base);
        assert_eq!(first, second);
        assert_eq!(first.len(), 4);
    }

    #[test]
    fn wilson_interval_contains_the_point_estimate() {
        let (low, high) = wilson_interval(18, 30, 1.96);
        assert!(low < 0.6 && high > 0.6);
        assert!(low > 0.0 && high < 1.0);
    }

    #[test]
    fn perfect_agreement_has_kappa_one() {
        let observations: Vec<_> = (0..4)
            .flat_map(|paragraph| {
                (0..3).map(move |evaluator| observation(paragraph, evaluator, Choice::A))
            })
            .collect();
        assert_eq!(fleiss_kappa(&observations), Some(1.0));
    }

    #[test]
    fn parse_observations_rejects_bad_choice() {
        assert!(parse_observations("0,0,A\n0,1,C\n").is_err());
    }

    #[test]
    fn blind_report_requires_thirty_paragraphs() {
        let mut observations = Vec::new();
        for paragraph in 0..10 {
            for evaluator in 0..5 {
                let choice = if evaluator == 0 && paragraph % 5 == 0 {
                    Choice::B
                } else {
                    Choice::A
                };
                observations.push(observation(paragraph, evaluator, choice));
            }
        }
        let report = blind_report(&observations).unwrap();
        assert!(!report.sample_ok);
        assert_eq!(report.decision, BlindDecision::Inconclusive);
    }

    #[test]
    fn blind_report_passes_a_clear_large_win() {
        let mut observations = Vec::new();
        for paragraph in 0..30 {
            for evaluator in 0..5 {
                let choice = if paragraph < 24 || evaluator == 0 {
                    Choice::A
                } else {
                    Choice::B
                };
                observations.push(observation(paragraph, evaluator, choice));
            }
        }
        let report = blind_report(&observations).unwrap();
        assert_eq!(report.paragraphs, 30);
        assert_eq!(report.a_wins, 24);
        assert_eq!(report.b_wins, 6);
        assert_eq!(report.unanimous_b, 0);
        assert!(report.kappa.unwrap() > 0.2);
        assert_eq!(report.decision, BlindDecision::Pass);
    }

    #[test]
    fn blind_report_fails_on_a_unanimous_baseline_win() {
        let mut observations = Vec::new();
        for paragraph in 0..30 {
            for evaluator in 0..5 {
                let choice = if paragraph == 0 { Choice::B } else { Choice::A };
                observations.push(observation(paragraph, evaluator, choice));
            }
        }
        let report = blind_report(&observations).unwrap();
        assert_eq!(report.unanimous_b, 1);
        assert_eq!(report.decision, BlindDecision::Fail);
    }

    #[test]
    fn length_limited_share_is_reported_per_corpus() {
        let kind = CorpusKind::PureCjk;
        let paragraph = build_paragraph(kind, 60, Width::from_em(20));
        let metrics = SyntheticMetrics::default();

        let unlimited = BreakConfig::default();
        let mut params = ParamSet::default_v1();
        params.break_params.max_kp_atoms = Some(20);
        let limited = BreakConfig::from_params(params);

        let mut aggregate = AggregateMetrics::default();
        aggregate.add(&measure(&paragraph, &metrics, &unlimited));
        aggregate.add(&measure(&paragraph, &metrics, &limited));

        assert_eq!(aggregate.paragraphs, 2);
        assert_eq!(aggregate.length_limited, 1);
        assert!((aggregate.length_limited_share() - 0.5).abs() < 1e-12);
        assert_eq!(AggregateMetrics::default().length_limited_share(), 0.0);
    }
}
