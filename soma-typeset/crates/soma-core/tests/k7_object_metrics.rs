//! K7: object atoms measure through the object hooks.
//!
//! The kernel measures `AtomKind::InlineObject` with `object_advance` /
//! `object_metrics` so a host can typeset objects (inline math) without changing
//! the DP. Every other atom keeps using `advance` / `glyph_metrics`, and the
//! default hooks reproduce exactly that, so an adapter that does not override
//! them produces byte-identical results.

use soma_core::{
    break_paragraph, Atom, AtomKind, Boundary, BreakConfig, Decorations, GlyphMetrics, LineMetrics,
    Metrics, Paragraph, ParagraphStyle, PunctClass, StyleId, Width,
};

const STYLE: StyleId = StyleId(0);

/// Text is 1em per character; objects are whatever the table says.
struct ObjectMetrics {
    object_width: Width,
    object_ascent: Width,
    object_descent: Width,
}

impl Metrics for ObjectMetrics {
    fn advance(&self, text: &str, _style: StyleId) -> Width {
        Width::EM * text.chars().count() as i32
    }

    fn line_metrics(&self, _style: StyleId) -> LineMetrics {
        LineMetrics {
            ascent: Width::from_em_frac(4, 5),
            descent: Width::from_em_frac(1, 5),
            leading: Width::ZERO,
        }
    }

    fn object_advance(&self, _text: &str, _style: StyleId) -> Width {
        self.object_width
    }

    fn object_metrics(&self, _text: &str, _style: StyleId) -> GlyphMetrics {
        GlyphMetrics {
            advance: self.object_width,
            ascent: self.object_ascent,
            descent: self.object_descent,
            leading: Width::ZERO,
            baseline_shift: Width::ZERO,
            overhang_left: Width::ZERO,
            overhang_right: Width::ZERO,
        }
    }
}

fn atom(text: &str, kind: AtomKind) -> Atom {
    Atom {
        text: String::from(text),
        kind,
        class: PunctClass::None,
        style: STYLE,
        decor: Decorations::NONE,
        link: None,
        compress_left: Width::ZERO,
        compress_right: Width::ZERO,
    }
}

fn paragraph(atoms: Vec<Atom>) -> Paragraph {
    let boundaries = atoms
        .iter()
        .skip(1)
        .map(|_| Boundary {
            breakable: soma_core::Breakability::Allowed,
            kind: soma_core::BoundaryKind::Normal,
            role: soma_core::BoundaryRole::Plain,
            glue: soma_core::Glue {
                width: Width::ZERO,
                stretch: Width::ZERO,
                shrink: Width::ZERO,
            },
            penalty: 0,
        })
        .collect();
    Paragraph {
        style: ParagraphStyle {
            width: Width::from_em(3),
            first_indent: Width::ZERO,
            line_height: Width::EM,
            align: soma_core::Align::Justify,
        },
        atoms,
        boundaries,
    }
}

fn metrics() -> ObjectMetrics {
    ObjectMetrics {
        object_width: Width::from_em(3),
        object_ascent: Width::from_em_frac(3, 2),
        object_descent: Width::from_em_frac(1, 2),
    }
}

#[test]
fn an_object_uses_the_object_advance() {
    // Two 1-em characters plus a 3-em object fit on a 3-em line only if the
    // object is measured by `object_advance`; measuring the marker text would
    // give a different break.
    let result = break_paragraph(
        &paragraph(vec![
            atom("中", AtomKind::Cjk),
            atom("\u{e001}0", AtomKind::InlineObject),
        ]),
        &metrics(),
        &BreakConfig::default(),
    )
    .expect("break");

    assert_eq!(result.lines.len(), 2, "1em + 3em does not fit in 3em");
    assert_eq!(result.lines[0].natural_width, Width::EM);
    assert_eq!(result.lines[1].natural_width, Width::from_em(3));
}

#[test]
fn an_object_raises_the_line_box() {
    let text_only = break_paragraph(
        &paragraph(vec![atom("中", AtomKind::Cjk)]),
        &metrics(),
        &BreakConfig::default(),
    )
    .expect("break");
    let with_object = break_paragraph(
        &paragraph(vec![
            atom("中", AtomKind::Cjk),
            atom("\u{e001}0", AtomKind::InlineObject),
        ]),
        &metrics(),
        &BreakConfig::default(),
    )
    .expect("break");

    let tall = with_object
        .lines
        .iter()
        .find(|line| line.boxes.len() == 1 && line.natural_width == Width::from_em(3))
        .expect("the object line");
    assert_eq!(
        tall.ascent,
        Width::from_em_frac(3, 2),
        "the line ascent comes from object_metrics"
    );
    assert_eq!(tall.descent, Width::from_em_frac(1, 2));
    assert!(tall.height > text_only.lines[0].height);
}

#[test]
fn the_default_hooks_match_the_text_paths() {
    struct Plain;

    impl Metrics for Plain {
        fn advance(&self, text: &str, _style: StyleId) -> Width {
            Width::EM * text.chars().count() as i32
        }

        fn line_metrics(&self, _style: StyleId) -> LineMetrics {
            LineMetrics {
                ascent: Width::EM,
                descent: Width::ZERO,
                leading: Width::ZERO,
            }
        }
    }

    let text = "\u{e001}0";
    assert_eq!(
        Plain.object_advance(text, STYLE),
        Plain.advance(text, STYLE),
        "the default object advance is the text advance"
    );
    assert_eq!(
        Plain.object_metrics(text, STYLE),
        Plain.glyph_metrics(text, STYLE)
    );

    let with_hooks = break_paragraph(
        &paragraph(vec![atom(text, AtomKind::InlineObject)]),
        &metrics(),
        &BreakConfig::default(),
    )
    .expect("break");
    let defaulted = break_paragraph(
        &paragraph(vec![atom(text, AtomKind::InlineObject)]),
        &Plain,
        &BreakConfig::default(),
    )
    .expect("break");
    assert_ne!(
        with_hooks.lines[0].natural_width,
        defaulted.lines[0].natural_width
    );
}

#[test]
fn text_atoms_never_use_the_object_hooks() {
    // A CJK atom whose text happens to equal the marker still measures as text.
    let result = break_paragraph(
        &paragraph(vec![atom("\u{e001}0", AtomKind::Cjk)]),
        &metrics(),
        &BreakConfig::default(),
    )
    .expect("break");

    assert_eq!(result.lines[0].natural_width, Width::EM * 2);
}
