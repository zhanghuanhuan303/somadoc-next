//! ADR-0011: the span compiler must agree with the text compiler at run seams.

use soma_core::StyleId;
use soma_core::Width;
use soma_core::{AtomKind, BoundaryKind, BoundaryRole, Breakability, PunctClass};
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
fn a_footnote_mark_attaches_to_the_text_before_it() {
    let paragraph = paragraph_from_spans(
        &[
            InlineSpan::text("行", StyleId(0)),
            InlineSpan::footnote("\u{e000}", StyleId(0)),
            InlineSpan::text("。", StyleId(0)),
        ],
        style(),
    )
    .expect("paragraph");

    assert_eq!(paragraph.atoms.len(), 3);
    // No break and no CJK/Latin gap before a footnote reference: a line must
    // never start with a superscript number (ADR-0096).
    let before = paragraph.boundaries[0];
    assert_eq!(before.breakable, Breakability::Forbidden);
    assert_eq!(before.role, BoundaryRole::Plain);
    assert_eq!(before.glue.width, Width::ZERO);
    assert_eq!(before.glue.stretch, Width::ZERO);
    // The following full stop may not start a line either.
    assert_eq!(paragraph.boundaries[1].breakable, Breakability::Forbidden);
}

#[test]
fn inline_code_breaks_at_its_own_spaces() {
    let paragraph = paragraph_from_spans(
        &[
            InlineSpan::text("运行", StyleId(0)),
            InlineSpan::code("let x = 1;", StyleId(1)),
            InlineSpan::object("\u{fffc}", StyleId(2)),
        ],
        style(),
    )
    .expect("paragraph");

    let code: Vec<&soma_core::Atom> = paragraph
        .atoms
        .iter()
        .filter(|atom| atom.style == StyleId(1))
        .collect();
    assert_eq!(code.len(), 4);
    assert!(code.iter().all(|atom| atom.kind == AtomKind::Other));
    // The space stays at the tail of the piece before it, so the pieces
    // concatenate back to the exact source text.
    let texts: Vec<&str> = code.iter().map(|atom| atom.text.as_str()).collect();
    assert_eq!(texts, ["let ", "x ", "= ", "1;"]);
    let joined: String = code.iter().map(|atom| atom.text.as_str()).collect();
    assert_eq!(joined, "let x = 1;");
    // A break between two code pieces is allowed but priced.
    let first = paragraph
        .atoms
        .iter()
        .position(|atom| atom.style == StyleId(1))
        .expect("code start");
    for boundary in &paragraph.boundaries[first..first + 3] {
        assert_eq!(boundary.breakable, Breakability::Allowed);
        assert_eq!(boundary.kind, BoundaryKind::Normal);
        assert!(boundary.penalty > 0);
    }

    let object = paragraph
        .atoms
        .iter()
        .find(|atom| atom.text == "\u{fffc}")
        .expect("object atom");
    assert_eq!(object.kind, AtomKind::InlineObject);
    assert_eq!(object.style, StyleId(2));
}

#[test]
fn inline_code_without_a_space_breaks_only_at_a_separator() {
    let paragraph = paragraph_from_spans(&[InlineSpan::code("metronome-rs", StyleId(1))], style())
        .expect("paragraph");
    assert_eq!(paragraph.atoms.len(), 2);
    assert_eq!(paragraph.atoms[0].text, "metronome-");
    assert_eq!(paragraph.atoms[1].text, "rs");
    let boundary = paragraph.boundaries[0];
    assert_eq!(boundary.breakable, Breakability::Allowed);
    assert_eq!(boundary.kind, BoundaryKind::Normal);
    assert!(boundary.penalty > 0, "a code identifier break is priced");
    let joined: String = paragraph
        .atoms
        .iter()
        .map(|atom| atom.text.as_str())
        .collect();
    assert_eq!(joined, "metronome-rs");
}

#[test]
fn inline_code_never_breaks_before_a_separator() {
    let paragraph = paragraph_from_spans(
        &[InlineSpan::code("https://example.com/a/", StyleId(1))],
        style(),
    )
    .expect("paragraph");
    let texts: Vec<&str> = paragraph
        .atoms
        .iter()
        .map(|atom| atom.text.as_str())
        .collect();
    assert_eq!(texts, ["https:/", "/", "example.", "com/", "a/"]);
    // A break after `https://` is allowed …
    assert_eq!(paragraph.boundaries[1].breakable, Breakability::Allowed);
    assert!(paragraph.boundaries[1].penalty > 0);
    // … but not between `https/` and `//`, which would start a line with `/`.
    assert_eq!(paragraph.boundaries[0].breakable, Breakability::Forbidden);
}

#[test]
fn camel_case_seams_are_priced_word_breaks() {
    let paragraph = paragraph_from_spans(
        &[InlineSpan::code("someVeryLongIdentifier", StyleId(1))],
        style(),
    )
    .expect("paragraph");
    let texts: Vec<&str> = paragraph
        .atoms
        .iter()
        .map(|atom| atom.text.as_str())
        .collect();
    assert_eq!(texts, ["some", "Very", "Long", "Identifier"]);
    for boundary in &paragraph.boundaries {
        assert_eq!(boundary.breakable, Breakability::Allowed);
        assert_eq!(boundary.penalty, 500, "a case seam is a word break");
    }
}

#[test]
fn snake_case_and_dotted_paths_split_at_their_separators() {
    let paragraph =
        paragraph_from_spans(&[InlineSpan::code("some_long.path", StyleId(1))], style())
            .expect("paragraph");
    let texts: Vec<&str> = paragraph
        .atoms
        .iter()
        .map(|atom| atom.text.as_str())
        .collect();
    assert_eq!(texts, ["some_", "long.", "path"]);
    // A decimal separator is not a word seam.
    let number = paragraph_from_spans(&[InlineSpan::code("v1.2.3", StyleId(1))], style())
        .expect("paragraph");
    assert_eq!(number.atoms.len(), 1);
    assert_eq!(number.atoms[0].text, "v1.2.3");
}

#[test]
fn a_long_unseparated_code_run_breaks_per_character() {
    let text = "a1b2c3d4e5f6a7b8c9d0e1f2";
    let paragraph =
        paragraph_from_spans(&[InlineSpan::code(text, StyleId(1))], style()).expect("paragraph");
    assert_eq!(paragraph.atoms.len(), text.len());
    assert!(paragraph
        .boundaries
        .iter()
        .all(|boundary| boundary.breakable == Breakability::Allowed),);
    assert!(
        paragraph
            .boundaries
            .iter()
            .all(|boundary| boundary.penalty == 2_000),
        "an intrusive break is the most expensive ordinary break"
    );
    let joined: String = paragraph
        .atoms
        .iter()
        .map(|atom| atom.text.as_str())
        .collect();
    assert_eq!(joined, text);
}

#[test]
fn an_intrusive_split_keeps_a_trailing_space_at_the_tail() {
    let text = "abcdefghijklmnop qr";
    let paragraph =
        paragraph_from_spans(&[InlineSpan::code(text, StyleId(1))], style()).expect("paragraph");
    assert!(
        paragraph
            .atoms
            .iter()
            .all(|atom| !atom.text.starts_with(' ')),
        "no atom may start with the space: a line could begin with it"
    );
    assert!(paragraph.atoms.iter().any(|atom| atom.text == "p "));
    let joined: String = paragraph
        .atoms
        .iter()
        .map(|atom| atom.text.as_str())
        .collect();
    assert_eq!(joined, text);
    // The boundary before `qr` is the code space, not an intrusive break.
    let qr = paragraph
        .atoms
        .iter()
        .position(|atom| atom.text == "qr")
        .expect("the short piece");
    assert_eq!(paragraph.boundaries[qr - 1].penalty, 100);
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
