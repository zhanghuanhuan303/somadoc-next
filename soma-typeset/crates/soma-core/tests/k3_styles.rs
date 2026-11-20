//! K3 public tests: style metrics, line boxes, large inline atoms, baseline
//! shifts, and word-internal style boundaries.

use soma_core::{
    break_paragraph, break_paragraph_traced, Align, Atom, AtomKind, Boundary, BoundaryKind,
    BoundaryRole, BreakConfig, Breakability, Decorations, Glue, GlyphMetrics, LineMetrics, Metrics,
    Paragraph, ParagraphStyle, PunctClass, StyleId, SyntheticMetrics, Weight, Width,
};

struct StyleMetrics;

impl Metrics for StyleMetrics {
    fn advance(&self, _text: &str, style: StyleId) -> Width {
        match style.0 {
            1 => Width::from_em(2),
            _ => Width::EM,
        }
    }

    fn line_metrics(&self, style: StyleId) -> LineMetrics {
        match style.0 {
            1 => LineMetrics {
                ascent: Width::from_em(2),
                descent: Width::from_em_frac(1, 2),
                leading: Width::from_em_frac(1, 2),
            },
            2 => LineMetrics {
                ascent: Width::from_em_frac(4, 5),
                descent: Width::from_em_frac(1, 5),
                leading: Width::from_em_frac(1, 5),
            },
            _ => LineMetrics {
                ascent: Width::EM,
                descent: Width::from_em_frac(1, 4),
                leading: Width::from_em_frac(1, 4),
            },
        }
    }

    fn glyph_metrics(&self, text: &str, style: StyleId) -> GlyphMetrics {
        let line = self.line_metrics(style);
        GlyphMetrics {
            advance: self.advance(text, style),
            ascent: line.ascent,
            descent: line.descent,
            leading: line.leading,
            baseline_shift: if style.0 == 2 { Width::EM } else { Width::ZERO },
            overhang_left: Width::ZERO,
            overhang_right: Width::ZERO,
        }
    }
}

fn atom(text: &str, kind: AtomKind, style: StyleId) -> Atom {
    Atom {
        text: text.into(),
        kind,
        class: PunctClass::None,
        style,
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

#[test]
fn glyph_metrics_default_uses_advance_and_line_metrics() {
    let metrics = SyntheticMetrics::default();
    let line = metrics.line_metrics(StyleId(0));
    let glyph = metrics.glyph_metrics("中", StyleId(0));

    assert_eq!(glyph.advance, metrics.advance("中", StyleId(0)));
    assert_eq!(glyph.ascent, line.ascent);
    assert_eq!(glyph.descent, line.descent);
    assert_eq!(glyph.leading, line.leading);
    assert_eq!(glyph.baseline_shift, Width::ZERO);
}

#[test]
fn large_inline_atom_increases_the_line_box() {
    let paragraph = paragraph(
        Width::from_em(3),
        vec![
            atom("中", AtomKind::Cjk, StyleId(0)),
            atom("大", AtomKind::Cjk, StyleId(1)),
        ],
        vec![boundary(Breakability::Allowed)],
    );

    let result = break_paragraph(&paragraph, &StyleMetrics, &BreakConfig::default()).unwrap();
    let line = &result.lines[0];

    assert_eq!(line.ascent, Width::from_em(2));
    assert_eq!(line.descent, Width::from_em_frac(1, 2));
    assert_eq!(line.leading, Width::from_em_frac(1, 2));
    assert_eq!(line.height, Width::from_em(3));
}

#[test]
fn baseline_shift_changes_ascent_and_descent() {
    let paragraph = paragraph(
        Width::from_em(2),
        vec![
            atom("x", AtomKind::LatinRun, StyleId(0)),
            atom("²", AtomKind::Cjk, StyleId(2)),
        ],
        vec![boundary(Breakability::Allowed)],
    );

    let result = break_paragraph(&paragraph, &StyleMetrics, &BreakConfig::default()).unwrap();
    let line = &result.lines[0];

    assert_eq!(line.ascent, Width::from_em_frac(9, 5));
    assert_eq!(line.descent, Width::from_em_frac(1, 4));
    assert_eq!(line.height, Width::from_em_frac(23, 10));
}

#[test]
fn word_internal_style_split_stays_unbreakable() {
    let paragraph = paragraph(
        Width::from_em(3),
        vec![
            atom("un", AtomKind::LatinRun, StyleId(0)),
            atom("believable", AtomKind::LatinRun, StyleId(1)),
        ],
        vec![boundary(Breakability::Forbidden)],
    );

    let result = break_paragraph(&paragraph, &StyleMetrics, &BreakConfig::default()).unwrap();
    assert_eq!(result.lines.len(), 1);
    assert_eq!((result.lines[0].start, result.lines[0].end), (0, 2));
}

#[test]
fn canonical_result_contains_line_box_metrics() {
    let paragraph = paragraph(
        Width::from_em(3),
        vec![
            atom("中", AtomKind::Cjk, StyleId(0)),
            atom("大", AtomKind::Cjk, StyleId(1)),
        ],
        vec![boundary(Breakability::Allowed)],
    );

    let json = break_paragraph(&paragraph, &StyleMetrics, &BreakConfig::default())
        .unwrap()
        .canonical_json();

    assert!(json.contains("\"schema\":\"soma.layout/6\""));
    assert!(json.contains("\"ascent\":8192"));
    assert!(json.contains("\"descent\":2048"));
    assert!(json.contains("\"leading\":2048"));
    assert!(json.contains("\"height\":12288"));
}

#[test]
fn height_cost_is_enabled_by_flag() {
    let paragraph = paragraph(
        Width::from_em(3),
        vec![
            atom("中", AtomKind::Cjk, StyleId(0)),
            atom("大", AtomKind::Cjk, StyleId(1)),
        ],
        vec![boundary(Breakability::Allowed)],
    );

    let mut params = soma_core::ParamSet::default_v1();
    params.features.height_cost = true;
    params.break_params.costs.height_scale = Weight(100);
    params.break_params.costs.height_threshold = Some(Width::from_em_frac(3, 2));
    let config = BreakConfig::from_params(params);

    let (result, trace) = break_paragraph_traced(&paragraph, &StyleMetrics, &config).unwrap();

    assert_eq!(trace.lines[0].height, Width::from_em(3).0 as i64);
    assert_eq!(result.lines[0].cost, soma_core::Cost(110));
}
