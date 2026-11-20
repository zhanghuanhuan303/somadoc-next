//! K4 public tests: edge trimming and the rendering coordinate convention.
//!
//! All widths are integer units (`1em = 4096`). The fixtures are chosen so a
//! line is overfull regardless of where the DP breaks, which makes the trim
//! amounts exact.

use soma_core::{
    break_paragraph, break_paragraph_traced, Align, Atom, AtomKind, Boundary, BoundaryKind,
    BoundaryRole, BreakConfig, BreakError, Breakability, Decorations, Glue, Paragraph,
    ParagraphStyle, ParamSet, PunctClass, StyleId, SyntheticMetrics, TrimPriority, Width,
};

fn atom(text: &str, kind: AtomKind, class: PunctClass, left: Width, right: Width) -> Atom {
    Atom {
        text: text.into(),
        kind,
        class,
        style: StyleId(0),
        decor: Decorations::NONE,
        link: None,
        compress_left: left,
        compress_right: right,
    }
}

fn allowed_boundary() -> Boundary {
    Boundary {
        breakable: Breakability::Allowed,
        kind: BoundaryKind::Normal,
        role: BoundaryRole::Plain,
        glue: Glue::default(),
        penalty: 0,
    }
}

fn forbidden_boundary() -> Boundary {
    Boundary {
        breakable: Breakability::Forbidden,
        ..allowed_boundary()
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

/// `中 中 ，` with 0.5em of removable right side bearing on the comma.
/// Natural 3em, target 2.5em: exactly 0.5em must be trimmed at the end.
fn trailing_comma_paragraph() -> Paragraph {
    Paragraph {
        style: style(Width::from_em_frac(5, 2)),
        atoms: vec![
            atom(
                "中",
                AtomKind::Cjk,
                PunctClass::None,
                Width::ZERO,
                Width::ZERO,
            ),
            atom(
                "中",
                AtomKind::Cjk,
                PunctClass::None,
                Width::ZERO,
                Width::ZERO,
            ),
            atom(
                "，",
                AtomKind::Other,
                PunctClass::Middle,
                Width::ZERO,
                Width::from_em_frac(1, 2),
            ),
        ],
        boundaries: vec![allowed_boundary(), forbidden_boundary()],
    }
}

/// `（ 中 ）` with 0.25em on each outer side. Natural 3em, target 2.5em:
/// 0.5em must be removed, 0.25em at the end and 0.25em at the start.
fn double_trim_paragraph() -> Paragraph {
    Paragraph {
        style: style(Width::from_em_frac(5, 2)),
        atoms: vec![
            atom(
                "（",
                AtomKind::Other,
                PunctClass::Open,
                Width::from_em_frac(1, 4),
                Width::ZERO,
            ),
            atom(
                "中",
                AtomKind::Cjk,
                PunctClass::None,
                Width::ZERO,
                Width::ZERO,
            ),
            atom(
                "）",
                AtomKind::Other,
                PunctClass::Close,
                Width::ZERO,
                Width::from_em_frac(1, 4),
            ),
        ],
        boundaries: vec![forbidden_boundary(), forbidden_boundary()],
    }
}

#[test]
fn edge_trim_matches_target() {
    let result = break_paragraph(
        &trailing_comma_paragraph(),
        &SyntheticMetrics::default(),
        &BreakConfig::default(),
    )
    .unwrap();

    assert_eq!(result.lines.len(), 1);
    let line = &result.lines[0];
    assert_eq!(line.natural_width, Width::from_em(3));
    assert_eq!(line.target_width, Width::from_em_frac(5, 2));
    assert_eq!(line.trim_left, Width::ZERO);
    assert_eq!(line.trim_right, Width::from_em_frac(1, 2));
    assert_eq!(line.adjustment, 0);
    assert_eq!(line.ratio_milli, 0);
    assert!(!line.overfull);
    assert!(!line.emergency);

    let last = line.boxes.last().unwrap();
    assert_eq!(last.advance, Width::EM - Width::from_em_frac(1, 2));
    assert_eq!(
        last.x.0 + last.advance.0,
        line.indent.0 + line.target_width.0
    );
}

#[test]
fn trim_prefers_end_before_start() {
    let result = break_paragraph(
        &double_trim_paragraph(),
        &SyntheticMetrics::default(),
        &BreakConfig::default(),
    )
    .unwrap();

    let line = &result.lines[0];
    assert_eq!(line.trim_right, Width::from_em_frac(1, 4));
    assert_eq!(line.trim_left, Width::from_em_frac(1, 4));

    let first = line.boxes.first().unwrap();
    let last = line.boxes.last().unwrap();
    assert_eq!(first.x, Width::ZERO - Width::from_em_frac(1, 4));
    assert_eq!(last.advance, Width::EM - Width::from_em_frac(1, 4));
    assert_eq!(
        last.x.0 + last.advance.0,
        line.indent.0 + line.target_width.0
    );
}

#[test]
fn single_atom_double_trim() {
    let paragraph = Paragraph {
        style: style(Width::from_em_frac(1, 4)),
        atoms: vec![atom(
            "中",
            AtomKind::Cjk,
            PunctClass::None,
            Width::from_em_frac(1, 4),
            Width::from_em_frac(1, 2),
        )],
        boundaries: vec![],
    };

    let result = break_paragraph(
        &paragraph,
        &SyntheticMetrics::default(),
        &BreakConfig::default(),
    )
    .unwrap();

    let line = &result.lines[0];
    assert_eq!(line.start, 0);
    assert_eq!(line.end, 1);
    assert_eq!(line.trim_left, Width::from_em_frac(1, 4));
    assert_eq!(line.trim_right, Width::from_em_frac(1, 2));
    assert_eq!(line.natural_width, Width::EM);

    // A single atom has no internal boundary, so the signed adjustment is 0;
    // trim still changes the emitted box.
    assert_eq!(line.adjustment, 0);
    assert_eq!(line.boxes.len(), 1);
    let only = &line.boxes[0];
    assert_eq!(only.x, Width::ZERO - Width::from_em_frac(1, 4));
    assert_eq!(only.advance, Width::EM - Width::from_em_frac(1, 2));
    assert_eq!(
        only.x.0 + only.advance.0,
        line.indent.0 + line.target_width.0
    );
}

#[test]
fn double_trim_still_overfull_fallback() {
    let paragraph = Paragraph {
        style: style(Width::from_em_frac(1, 4)),
        atoms: vec![atom(
            "中",
            AtomKind::Cjk,
            PunctClass::None,
            Width::from_em_frac(1, 4),
            Width::from_em_frac(1, 4),
        )],
        boundaries: vec![],
    };

    let (result, trace) = break_paragraph_traced(
        &paragraph,
        &SyntheticMetrics::default(),
        &BreakConfig::default(),
    )
    .unwrap();

    assert!(trace.fallback_used);
    let line = &result.lines[0];
    assert_eq!(line.trim_left, Width::from_em_frac(1, 4));
    assert_eq!(line.trim_right, Width::from_em_frac(1, 4));
    assert_eq!(line.natural_width, Width::EM);
    assert!(line.overfull);
    assert!(line.emergency);
}

#[test]
fn an_underfull_line_drops_the_bearing_without_paying_for_it() {
    let stretchy = || Boundary {
        glue: Glue {
            stretch: Width::from_em(2),
            ..Glue::default()
        },
        ..allowed_boundary()
    };
    let paragraph = Paragraph {
        style: style(Width::from_em_frac(7, 2)),
        atoms: vec![
            atom(
                "中",
                AtomKind::Cjk,
                PunctClass::None,
                Width::ZERO,
                Width::ZERO,
            ),
            atom(
                "中",
                AtomKind::Cjk,
                PunctClass::None,
                Width::ZERO,
                Width::ZERO,
            ),
            atom(
                "，",
                AtomKind::Other,
                PunctClass::Middle,
                Width::ZERO,
                Width::from_em_frac(1, 2),
            ),
            atom(
                "中",
                AtomKind::Cjk,
                PunctClass::None,
                Width::ZERO,
                Width::ZERO,
            ),
        ],
        boundaries: vec![stretchy(), forbidden_boundary(), stretchy()],
    };

    let (result, trace) = break_paragraph_traced(
        &paragraph,
        &SyntheticMetrics::default(),
        &BreakConfig::default(),
    )
    .unwrap();

    assert_eq!(result.lines.len(), 2, "the period must end the first line");
    let line = &result.lines[0];
    assert!(line.natural_width < line.target_width);
    assert!(!line.emergency, "the stretch covers the deficit");
    // The breaker reads `compress_right`, not the punctuation class: a short
    // line still gives up the trailing bearing so the ink sits on the edge.
    assert_eq!(line.trim_right, Width::from_em_frac(1, 2));
    assert_eq!(line.trim_left, Width::ZERO);
    assert_eq!(line.boxes[2].advance, Width::from_em_frac(1, 2));
    // Optical alignment is not compensation, so a short line is not charged.
    assert_eq!(trace.lines[0].compression_cost, 0);

    // The ragged last line is left aligned and does not overrun, so it keeps
    // its advance untouched.
    assert_eq!(result.lines[1].trim_right, Width::ZERO);
}

#[test]
fn trim_cost_enters_line_cost() {
    let (result, trace) = break_paragraph_traced(
        &trailing_comma_paragraph(),
        &SyntheticMetrics::default(),
        &BreakConfig::default(),
    )
    .unwrap();

    let line = &result.lines[0];
    // line 10 + compress_amount(20) * 0.5em + compress_count(5) = 25.
    assert_eq!(line.cost.0, 25);

    let trace_line = &trace.lines[0];
    assert_eq!(trace_line.compression_cost, 15);
    assert_eq!(trace_line.trim_left, 0);
    assert_eq!(trace_line.trim_right, 2048);
}

#[test]
fn breaker_ignores_punctuation_class() {
    let with_class = Paragraph {
        style: style(Width::from_em_frac(3, 2)),
        atoms: vec![
            atom(
                "中",
                AtomKind::Cjk,
                PunctClass::None,
                Width::ZERO,
                Width::ZERO,
            ),
            atom(
                "，",
                AtomKind::Other,
                PunctClass::Middle,
                Width::ZERO,
                Width::from_em_frac(1, 2),
            ),
        ],
        boundaries: vec![allowed_boundary()],
    };
    let without_class = Paragraph {
        style: style(Width::from_em_frac(3, 2)),
        atoms: vec![
            atom(
                "中",
                AtomKind::Cjk,
                PunctClass::None,
                Width::ZERO,
                Width::ZERO,
            ),
            atom(
                "，",
                AtomKind::Other,
                PunctClass::None,
                Width::ZERO,
                Width::from_em_frac(1, 2),
            ),
        ],
        boundaries: vec![allowed_boundary()],
    };

    let metrics = SyntheticMetrics::default();
    let config = BreakConfig::default();
    let a = break_paragraph(&with_class, &metrics, &config).unwrap();
    let b = break_paragraph(&without_class, &metrics, &config).unwrap();

    assert_eq!(a.canonical_json(), b.canonical_json());
    assert_eq!(a.lines[0].trim_right, Width::from_em_frac(1, 2));
}

#[test]
fn edge_trim_flag_disables_trim() {
    let mut params = ParamSet::default_v1();
    params.features.edge_trim = false;
    // Ignored while edge_trim is off, so an unimplemented order must not
    // turn into a validation error here.
    params.break_params.trim_priority = TrimPriority::StartFirst;
    let config = BreakConfig::from_params(params);

    let (result, trace) = break_paragraph_traced(
        &trailing_comma_paragraph(),
        &SyntheticMetrics::default(),
        &config,
    )
    .unwrap();

    assert!(trace.fallback_used);
    let line = &result.lines[0];
    assert_eq!(line.trim_left, Width::ZERO);
    assert_eq!(line.trim_right, Width::ZERO);
    assert!(result
        .lines
        .iter()
        .all(|line| line.trim_left == Width::ZERO && line.trim_right == Width::ZERO));
}

#[test]
fn canonical_result_contains_trim_fields() {
    let result = break_paragraph(
        &trailing_comma_paragraph(),
        &SyntheticMetrics::default(),
        &BreakConfig::default(),
    )
    .unwrap();
    let json = result.canonical_json();

    assert!(
        json.contains("\"naturalWidth\":12288,\"trimLeft\":0,\"trimRight\":2048,\"ratioMilli\":0")
    );
}

#[test]
fn unsupported_trim_priority_is_rejected() {
    let mut params = ParamSet::default_v1();
    params.break_params.trim_priority = TrimPriority::StartFirst;

    let error = break_paragraph(
        &trailing_comma_paragraph(),
        &SyntheticMetrics::default(),
        &BreakConfig::from_params(params),
    )
    .unwrap_err();

    match error {
        BreakError::Params(error) => {
            assert_eq!(error.path, "break_params.trim_priority");
        }
        other => panic!("expected a param error, got {other:?}"),
    }
}
