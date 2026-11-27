//! K6 fallback ladder: emergency and forbidden rescue.
//!
//! The fixture is deliberately impossible without a forbidden break when
//! `allow_overfull = false`: the only allowed break leaves a line that would
//! have to be overfull, while breaking on the forbidden boundary gives two
//! exact lines.

use soma_core::{
    break_paragraph, Align, Atom, AtomKind, Boundary, BoundaryKind, BoundaryRole, BreakConfig,
    BreakError, Breakability, Decorations, FallbackStage, Glue, Paragraph, ParagraphStyle,
    ParamSet, PunctClass, StyleId, SyntheticMetrics, Width,
};

fn atom(text: &str) -> Atom {
    Atom {
        text: text.into(),
        kind: AtomKind::Cjk,
        class: PunctClass::None,
        style: StyleId(0),
        decor: Decorations::NONE,
        link: None,
        compress_left: Width::ZERO,
        compress_right: Width::ZERO,
    }
}

fn boundary(breakable: Breakability) -> Boundary {
    Boundary {
        breakable,
        kind: BoundaryKind::Normal,
        role: BoundaryRole::Plain,
        glue: Glue::default(),
        penalty: 0,
        pair_squeeze: Width::ZERO,
    }
}

fn rescue_paragraph() -> Paragraph {
    Paragraph {
        style: ParagraphStyle {
            width: Width::from_em(2),
            first_indent: Width::ZERO,
            hang: Width::ZERO,
            tracking: Width::ZERO,
            line_height: Width::from_em_frac(3, 2),
            align: Align::Justify,
        },
        atoms: vec![atom("甲"), atom("乙"), atom("丙"), atom("丁")],
        boundaries: vec![
            boundary(Breakability::Allowed),
            boundary(Breakability::Forbidden),
            boundary(Breakability::Forbidden),
        ],
    }
}

fn no_overfull_params() -> ParamSet {
    let mut params = ParamSet::default_v1();
    params.break_params.fallback.allow_overfull = false;
    params
}

#[test]
fn forbidden_rescue_is_the_last_resort() {
    let result = break_paragraph(
        &rescue_paragraph(),
        &SyntheticMetrics::default(),
        &BreakConfig::from_params(no_overfull_params()),
    )
    .unwrap();

    assert_eq!(result.fallback_stage, FallbackStage::ForbiddenRescue);
    assert_eq!(result.forbidden_violations, 1);
    assert_eq!(result.emergency_lines, 0);

    assert_eq!(result.lines.len(), 2);
    assert_eq!(result.lines[0].end, 2);
    assert_eq!(result.lines[1].end, 4);
    assert_eq!(result.lines[0].adjustment, 0);
    assert_eq!(result.lines[1].adjustment, 0);
}

#[test]
fn forbidden_rescue_is_skipped_when_disallowed() {
    let mut params = no_overfull_params();
    params.break_params.fallback.allow_forbidden = false;

    let error = break_paragraph(
        &rescue_paragraph(),
        &SyntheticMetrics::default(),
        &BreakConfig::from_params(params),
    )
    .unwrap_err();

    assert_eq!(error, BreakError::NoFeasibleBreak);
}

#[test]
fn normal_stage_never_violates_forbidden_boundaries() {
    let paragraph = Paragraph {
        style: ParagraphStyle {
            width: Width::from_em(4),
            first_indent: Width::ZERO,
            hang: Width::ZERO,
            tracking: Width::ZERO,
            line_height: Width::from_em_frac(3, 2),
            align: Align::Justify,
        },
        atoms: vec![atom("甲"), atom("乙"), atom("丙"), atom("丁")],
        boundaries: vec![
            boundary(Breakability::Allowed),
            boundary(Breakability::Forbidden),
            boundary(Breakability::Allowed),
        ],
    };

    let result = break_paragraph(
        &paragraph,
        &SyntheticMetrics::default(),
        &BreakConfig::default(),
    )
    .unwrap();

    // A single 4em line is feasible without crossing the forbidden boundary.
    assert_eq!(result.fallback_stage, FallbackStage::Normal);
    assert_eq!(result.forbidden_violations, 0);
    assert_eq!(result.lines.len(), 1);
}

#[test]
fn emergency_stage_reports_its_lines() {
    let paragraph = Paragraph {
        style: ParagraphStyle {
            width: Width::from_em(2),
            first_indent: Width::ZERO,
            hang: Width::ZERO,
            tracking: Width::ZERO,
            line_height: Width::from_em_frac(3, 2),
            align: Align::Justify,
        },
        atoms: vec![Atom {
            text: "abcdefghij".into(),
            kind: AtomKind::LatinRun,
            class: PunctClass::None,
            style: StyleId(0),
            decor: Decorations::NONE,
            link: None,
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
    assert_eq!(result.emergency_lines, 1);
    assert_eq!(result.forbidden_violations, 0);
    assert!(result.lines[0].overfull);
}
