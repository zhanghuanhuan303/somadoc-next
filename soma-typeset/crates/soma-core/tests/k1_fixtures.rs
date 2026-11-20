//! K1 public fixture tests.
//!
//! These use the public breaker seam and deliberately simple geometry so the
//! expected line breaks are hand-checkable.

use soma_core::{
    break_paragraph, Align, Atom, AtomKind, Boundary, BoundaryKind, BreakConfig, Breakability,
    Decorations, Glue, LayoutResult, Paragraph, ParagraphStyle, PunctClass, StyleId,
    SyntheticMetrics, Weight, Width,
};

fn atom(text: &str, kind: AtomKind, class: PunctClass) -> Atom {
    Atom {
        text: text.into(),
        kind,
        class,
        style: StyleId(0),
        decor: Decorations::NONE,
        link: None,
        compress_left: Width::ZERO,
        compress_right: Width::ZERO,
    }
}

fn boundary(breakable: Breakability, kind: BoundaryKind, glue: Glue) -> Boundary {
    Boundary {
        breakable,
        kind,
        role: soma_core::BoundaryRole::Plain,
        glue,
        penalty: 0,
    }
}

fn paragraph(width: Width, atoms: Vec<Atom>, boundaries: Vec<Boundary>) -> Paragraph {
    Paragraph {
        style: ParagraphStyle {
            width,
            first_indent: Width::ZERO,
            line_height: Width::from_em_frac(3, 2),
            align: Align::Justify,
        },
        atoms,
        boundaries,
    }
}

fn run(paragraph: &Paragraph) -> LayoutResult {
    break_paragraph(
        paragraph,
        &SyntheticMetrics::default(),
        &BreakConfig::default(),
    )
    .expect("fixture must be breakable")
}

fn assert_breaks(result: &LayoutResult, expected: &[(u32, u32)]) {
    let actual: Vec<(u32, u32)> = result
        .lines
        .iter()
        .map(|line| (line.start, line.end))
        .collect();
    assert_eq!(actual, expected);
}

fn flat_boundaries(count: usize) -> Vec<Boundary> {
    (0..count)
        .map(|_| boundary(Breakability::Allowed, BoundaryKind::Normal, Glue::default()))
        .collect()
}

#[test]
fn pure_cjk_breaks_on_exact_five_em_lines() {
    let atoms = (0..10)
        .map(|_| atom("中", AtomKind::Cjk, PunctClass::None))
        .collect();
    let paragraph = paragraph(Width::from_em(5), atoms, flat_boundaries(9));

    assert_breaks(&run(&paragraph), &[(0, 5), (5, 10)]);
}

#[test]
fn narrow_cjk_breaks_on_exact_three_em_lines() {
    let atoms = (0..6)
        .map(|_| atom("中", AtomKind::Cjk, PunctClass::None))
        .collect();
    let paragraph = paragraph(Width::from_em(3), atoms, flat_boundaries(5));

    assert_breaks(&run(&paragraph), &[(0, 3), (3, 6)]);
}

#[test]
fn mixed_space_uses_the_metrics_space_width() {
    let paragraph = paragraph(
        Width::from_em_frac(9, 4),
        vec![
            atom("中", AtomKind::Cjk, PunctClass::None),
            atom("ab", AtomKind::LatinRun, PunctClass::None),
        ],
        vec![boundary(
            Breakability::Allowed,
            BoundaryKind::Space,
            Glue::default(),
        )],
    );

    assert_breaks(&run(&paragraph), &[(0, 2)]);
}

#[test]
fn mixed_nospace_uses_the_rule_glue() {
    let paragraph = paragraph(
        Width::from_em_frac(9, 4),
        vec![
            atom("中", AtomKind::Cjk, PunctClass::None),
            atom("ab", AtomKind::LatinRun, PunctClass::None),
        ],
        vec![boundary(
            Breakability::Allowed,
            BoundaryKind::Normal,
            Glue {
                width: Width::from_em_frac(1, 4),
                stretch: Width::ZERO,
                shrink: Width::ZERO,
            },
        )],
    );

    assert_breaks(&run(&paragraph), &[(0, 2)]);
}

#[test]
fn long_latin_word_is_overfull_not_split() {
    let paragraph = paragraph(
        Width::from_em(2),
        vec![atom("abcdefghij", AtomKind::LatinRun, PunctClass::None)],
        vec![],
    );

    let result = run(&paragraph);
    assert_eq!(result.lines.len(), 1);
    assert_eq!((result.lines[0].start, result.lines[0].end), (0, 1));
    assert!(result.lines[0].overfull);
    assert!(result.lines[0].emergency);
}

#[test]
fn opening_and_closing_punctuation_keep_the_phrase_together() {
    let paragraph = paragraph(
        Width::from_em(3),
        vec![
            atom("（", AtomKind::Cjk, PunctClass::Open),
            atom("中", AtomKind::Cjk, PunctClass::None),
            atom("）", AtomKind::Cjk, PunctClass::Close),
        ],
        vec![
            boundary(
                Breakability::Forbidden,
                BoundaryKind::Normal,
                Glue::default(),
            ),
            boundary(
                Breakability::Forbidden,
                BoundaryKind::Normal,
                Glue::default(),
            ),
        ],
    );

    assert_breaks(&run(&paragraph), &[(0, 3)]);
}

#[test]
fn style_switches_do_not_change_the_break_units() {
    let mut first = atom("中", AtomKind::Cjk, PunctClass::None);
    first.style = StyleId(0);
    let mut second = atom("ab", AtomKind::LatinRun, PunctClass::None);
    second.style = StyleId(1);

    let paragraph = paragraph(
        Width::from_em_frac(9, 4),
        vec![first, second],
        vec![boundary(
            Breakability::Allowed,
            BoundaryKind::Normal,
            Glue {
                width: Width::from_em_frac(1, 4),
                stretch: Width::ZERO,
                shrink: Width::ZERO,
            },
        )],
    );

    assert_breaks(&run(&paragraph), &[(0, 2)]);
}

#[test]
fn inline_object_is_one_unbreakable_atom() {
    let paragraph = paragraph(
        Width::from_em(2),
        vec![atom("code", AtomKind::InlineObject, PunctClass::None)],
        vec![],
    );

    assert_breaks(&run(&paragraph), &[(0, 1)]);
}

#[test]
fn tie_prefers_the_longer_last_line() {
    let mut params = soma_core::ParamSet::default_v1();
    params.break_params.costs.badness_scale = Weight(0);
    params.break_params.costs.short_last_scale = Weight(0);

    let paragraph = paragraph(
        Width::from_em(2),
        vec![
            atom("甲甲", AtomKind::Cjk, PunctClass::None),
            atom("乙", AtomKind::Cjk, PunctClass::None),
            atom("丙", AtomKind::Cjk, PunctClass::None),
        ],
        vec![
            boundary(
                Breakability::Allowed,
                BoundaryKind::Normal,
                Glue {
                    width: Width::ZERO,
                    stretch: Width::ZERO,
                    shrink: Width::from_em(1),
                },
            ),
            boundary(Breakability::Allowed, BoundaryKind::Normal, Glue::default()),
        ],
    );

    let result = break_paragraph(
        &paragraph,
        &SyntheticMetrics::default(),
        &BreakConfig::from_params(params),
    )
    .unwrap();

    assert_breaks(&result, &[(0, 1), (1, 3)]);
}

#[test]
fn single_atom_line_counts_one_line_cost() {
    let paragraph = paragraph(
        Width::from_em(2),
        vec![atom("中中", AtomKind::Cjk, PunctClass::None)],
        vec![],
    );

    let result = run(&paragraph);
    assert_breaks(&result, &[(0, 1)]);
    assert_eq!(result.total_cost, soma_core::Cost(10));
}
