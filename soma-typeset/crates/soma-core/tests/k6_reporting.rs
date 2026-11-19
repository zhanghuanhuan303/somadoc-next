//! K6 reporting tests: deterministic algorithm selection and result flags.
//!
//! K6's calibration itself needs a tuning corpus; these tests lock the
//! measurement plumbing that calibration consumes.

use soma_core::{
    break_paragraph, break_paragraph_greedy, break_paragraph_traced, Align, Atom, AtomKind,
    Boundary, BoundaryKind, BoundaryRole, BreakConfig, Breakability, BreakerKind, FallbackStage,
    Glue, GreedyMode, Paragraph, ParagraphStyle, ParamSet, PunctClass, StyleId, SyntheticMetrics,
    Width,
};

fn atom(text: &str) -> Atom {
    Atom {
        text: text.into(),
        kind: AtomKind::Cjk,
        class: PunctClass::None,
        style: StyleId(0),
        compress_left: Width::ZERO,
        compress_right: Width::ZERO,
    }
}

fn boundary() -> Boundary {
    Boundary {
        breakable: Breakability::Allowed,
        kind: BoundaryKind::Normal,
        role: BoundaryRole::Plain,
        glue: Glue::default(),
        penalty: 0,
    }
}

fn style(width: Width) -> ParagraphStyle {
    ParagraphStyle {
        width,
        first_indent: Width::ZERO,
        line_height: Width::from_em_frac(3, 2),
        align: Align::Justify,
    }
}

fn four_atoms() -> Paragraph {
    Paragraph {
        style: style(Width::from_em(2)),
        atoms: vec![atom("甲"), atom("乙"), atom("丙"), atom("丁")],
        boundaries: vec![boundary(), boundary(), boundary()],
    }
}

#[test]
fn optimal_reports_breaker_and_normal_stage() {
    let result = break_paragraph(
        &four_atoms(),
        &SyntheticMetrics::default(),
        &BreakConfig::default(),
    )
    .unwrap();

    assert_eq!(result.breaker_used, BreakerKind::Optimal);
    assert_eq!(result.fallback_stage, FallbackStage::Normal);
    assert!(!result.length_limited);
    assert_eq!(result.emergency_lines, 0);
    assert_eq!(result.forbidden_violations, 0);
}

#[test]
fn greedy_reports_its_mode() {
    let metrics = SyntheticMetrics::default();
    let config = BreakConfig::default();

    let furthest =
        break_paragraph_greedy(&four_atoms(), &metrics, &config, GreedyMode::Furthest).unwrap();
    assert_eq!(furthest.breaker_used, BreakerKind::GreedyF);
    assert!(!furthest.length_limited);

    let min_cost =
        break_paragraph_greedy(&four_atoms(), &metrics, &config, GreedyMode::MinCost).unwrap();
    assert_eq!(min_cost.breaker_used, BreakerKind::GreedyC);
    assert!(!min_cost.length_limited);
}

#[test]
fn max_kp_atoms_uses_greedy_c() {
    let metrics = SyntheticMetrics::default();
    let mut params = ParamSet::default_v1();
    params.break_params.max_kp_atoms = Some(2);
    let config = BreakConfig::from_params(params);

    let limited = break_paragraph(&four_atoms(), &metrics, &config).unwrap();
    let explicit =
        break_paragraph_greedy(&four_atoms(), &metrics, &config, GreedyMode::MinCost).unwrap();

    assert_eq!(limited.breaker_used, BreakerKind::GreedyC);
    assert!(limited.length_limited);
    assert!(!explicit.length_limited);
    assert_eq!(limited.lines, explicit.lines);
    assert_eq!(limited.total_cost, explicit.total_cost);
}

#[test]
fn length_limited_is_false_within_the_limit() {
    let mut params = ParamSet::default_v1();
    params.break_params.max_kp_atoms = Some(10);

    let result = break_paragraph(
        &four_atoms(),
        &SyntheticMetrics::default(),
        &BreakConfig::from_params(params),
    )
    .unwrap();

    assert_eq!(result.breaker_used, BreakerKind::Optimal);
    assert!(!result.length_limited);
}

#[test]
fn fallback_reports_emergency_stage() {
    let paragraph = Paragraph {
        style: style(Width::from_em(2)),
        atoms: vec![Atom {
            text: "abcdefghij".into(),
            kind: AtomKind::LatinRun,
            class: PunctClass::None,
            style: StyleId(0),
            compress_left: Width::ZERO,
            compress_right: Width::ZERO,
        }],
        boundaries: vec![],
    };

    let result = break_paragraph(
        &paragraph,
        &SyntheticMetrics::default(),
        &BreakConfig::default(),
    )
    .unwrap();

    assert_eq!(result.fallback_stage, FallbackStage::Emergency);
    assert_eq!(result.breaker_used, BreakerKind::Optimal);
    assert_eq!(result.emergency_lines, 1);
    assert_eq!(result.forbidden_violations, 0);
    assert!(result.lines[0].emergency);
}

#[test]
fn trace_reports_the_same_flags() {
    let (result, trace) = break_paragraph_traced(
        &four_atoms(),
        &SyntheticMetrics::default(),
        &BreakConfig::default(),
    )
    .unwrap();

    assert_eq!(trace.breaker_used, result.breaker_used);
    assert_eq!(trace.fallback_stage, result.fallback_stage);
    assert_eq!(
        trace.fallback_used,
        result.fallback_stage != FallbackStage::Normal
    );
}

#[test]
fn canonical_result_contains_reporting_fields() {
    let result = break_paragraph(
        &four_atoms(),
        &SyntheticMetrics::default(),
        &BreakConfig::default(),
    )
    .unwrap();
    let json = result.canonical_json();

    assert!(json.contains("\"schema\":\"soma.layout/6\""));
    assert!(json.contains("\"breakerUsed\":\"optimal\""));
    assert!(json.contains("\"fallbackStage\":\"normal\""));
    assert!(json.contains("\"lengthLimited\":false"));
    assert!(json.contains("\"emergencyLines\":0"));
    assert!(json.contains("\"forbiddenViolations\":0"));
}
