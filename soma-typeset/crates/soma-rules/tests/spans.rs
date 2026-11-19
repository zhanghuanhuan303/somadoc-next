//! ADR-0011: the span compiler must agree with the text compiler at run seams.

use soma_core::StyleId;
use soma_core::Width;
use soma_core::{AtomKind, PunctClass};
use soma_rules::{
    is_cjk, justified_style, paragraph_from_spans, paragraph_from_text, punct_class, InlineSpan,
    SpanKind,
};

fn style() -> soma_core::ParagraphStyle {
    justified_style(Width::from_em(10))
}

#[test]
fn spans_share_the_glue_at_seams() {
    let whole = paragraph_from_text("中文English混排", style(), StyleId(0)).expect("paragraph");
    let split = paragraph_from_spans(
        &[
            InlineSpan::text("中文", StyleId(0)),
            InlineSpan::text("English", StyleId(0)),
            InlineSpan::text("混排", StyleId(0)),
        ],
        style(),
    )
    .expect("paragraph");

    assert_eq!(whole.atoms, split.atoms);
    assert_eq!(whole.boundaries, split.boundaries);
}

#[test]
fn a_trailing_space_carries_across_the_seam() {
    let whole = paragraph_from_text("中文 English", style(), StyleId(0)).expect("paragraph");
    let split = paragraph_from_spans(
        &[
            InlineSpan::text("中文 ", StyleId(0)),
            InlineSpan::text("English", StyleId(0)),
        ],
        style(),
    )
    .expect("paragraph");

    assert_eq!(whole.boundaries, split.boundaries);
    assert_eq!(whole.atoms.len(), split.atoms.len());
}

#[test]
fn styled_runs_keep_their_style_id() {
    let paragraph = paragraph_from_spans(
        &[
            InlineSpan::text("正文", StyleId(0)),
            InlineSpan::text("强调", StyleId(3)),
        ],
        style(),
    )
    .expect("paragraph");

    assert_eq!(paragraph.atoms[0].style, StyleId(0));
    assert_eq!(paragraph.atoms[2].style, StyleId(3));
    assert_eq!(paragraph.atoms[3].style, StyleId(3));
}

#[test]
fn code_and_object_spans_are_single_atoms() {
    let paragraph = paragraph_from_spans(
        &[
            InlineSpan::text("运行", StyleId(0)),
            InlineSpan::code("let x = 1;", StyleId(1)),
            InlineSpan::object("\u{fffc}", StyleId(2)),
        ],
        style(),
    )
    .expect("paragraph");

    let code = paragraph
        .atoms
        .iter()
        .find(|atom| atom.text == "let x = 1;")
        .expect("code atom");
    assert_eq!(code.kind, AtomKind::Other);
    assert_eq!(code.style, StyleId(1));

    let object = paragraph
        .atoms
        .iter()
        .find(|atom| atom.text == "\u{fffc}")
        .expect("object atom");
    assert_eq!(object.kind, AtomKind::InlineObject);
    assert_eq!(object.style, StyleId(2));
}

#[test]
fn empty_spans_are_rejected() {
    let error = paragraph_from_spans(&[], style()).expect_err("empty");
    assert_eq!(error, soma_rules::RuleError::EmptyText);

    let error =
        paragraph_from_spans(&[InlineSpan::text("   ", StyleId(0))], style()).expect_err("blank");
    assert_eq!(error, soma_rules::RuleError::EmptyText);
}

#[test]
fn punctuation_and_segmentation_helpers_stay_consistent() {
    // The public helpers used by the DocIR compiler must agree with the rules
    // the span compiler applies.
    assert!(is_cjk('中'));
    assert!(!is_cjk('A'));
    assert_eq!(punct_class('（'), PunctClass::Open);
    assert_eq!(punct_class('）'), PunctClass::Close);
    // CJK sentence punctuation is a `Middle` class in this table: it may not
    // start a line and carries both trim caps.
    assert_eq!(punct_class('。'), PunctClass::Middle);
    assert_eq!(SpanKind::Text, SpanKind::Text);
}
