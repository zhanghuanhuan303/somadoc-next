//! A deliberately small first-pass rule compiler.
//!
//! It turns a paragraph string into `soma_core::Paragraph`: CJK characters
//! become individual atoms, Latin and digit runs stay unbreakable, whitespace
//! becomes a space boundary, and a small punctuation table marks opening,
//! closing, and middle punctuation.
//!
//! This is not yet the versioned `clreq` rule set. It exists to exercise the
//! IR and the breaker end to end. Later revisions will move the table into
//! cited, versioned data and add pair compression/penalty rules.

#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

use soma_core::{
    Align, Atom, AtomKind, Boundary, BoundaryKind, BoundaryRole, Breakability, Glue, Paragraph,
    ParagraphStyle, PunctClass, StyleId, Width,
};

/// Stretch/shrink for a boundary between two CJK atoms.
pub const CJK_GAP_STRETCH: Width = Width::from_em_frac(1, 32);
pub const CJK_GAP_SHRINK: Width = Width::from_em_frac(1, 32);

/// Ideal CJK/Latin gap when the source has no whitespace.
pub const CJK_LATIN_GAP: Width = Width::from_em_frac(1, 4);
pub const CJK_LATIN_STRETCH: Width = Width::from_em_frac(1, 16);
pub const CJK_LATIN_SHRINK: Width = Width::from_em_frac(1, 16);

/// Stretch/shrink for a source whitespace boundary. The width itself comes
/// from `Metrics::space_width`.
pub const SPACE_STRETCH: Width = Width::from_em_frac(1, 8);
pub const SPACE_SHRINK: Width = Width::from_em_frac(1, 8);

/// Publication status of a rule.
///
/// `Tbd` rows must not be used as Gate Q evidence, and a hard constraint
/// (`Forbidden` / `Forced`) requires `Verified`. The punctuation hard rules are
/// `Verified` against GB/T 15834-2011 via W3C clreq §6.1.1 (basic); the trim and
/// gap ratios remain `ParamOnly` until they are calibrated.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuleStatus {
    Verified,
    Tbd,
    ParamOnly,
}

/// One row of the rule inventory. `id` is stable; add rows instead of renaming.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RuleInfo {
    pub id: &'static str,
    pub status: RuleStatus,
    pub source: &'static str,
}

/// The current rule inventory. This is metadata only: it does not change the
/// compiled `Paragraph`. Gate Q may only assert rows with `Verified`.
pub const RULES: &[RuleInfo] = &[
    RuleInfo {
        id: "segment.cjk",
        status: RuleStatus::Verified,
        source: "IR contract: one CJK character per atom",
    },
    RuleInfo {
        id: "segment.latin_run",
        status: RuleStatus::Verified,
        source: "IR contract: Latin and digit runs are unbreakable atoms",
    },
    RuleInfo {
        id: "segment.decimal_number",
        status: RuleStatus::Verified,
        source: "compiler invariant; decimal separators stay inside the run",
    },
    RuleInfo {
        id: "break.open.no_break_after",
        status: RuleStatus::Verified,
        source: "GB/T 15834-2011 via W3C clreq §6.1.1 (basic)",
    },
    RuleInfo {
        id: "break.close.no_break_before",
        status: RuleStatus::Verified,
        source: "GB/T 15834-2011 via W3C clreq §6.1.1 (basic)",
    },
    RuleInfo {
        id: "break.middle.no_line_start",
        status: RuleStatus::Verified,
        source: "GB/T 15834-2011 via W3C clreq §6.1.1 (basic)",
    },
    RuleInfo {
        id: "break.space_before_punct",
        status: RuleStatus::Verified,
        source: "GB/T 15834-2011 via W3C clreq §6.1.1 (no line-start punctuation)",
    },
    RuleInfo {
        id: "gap.cjk",
        status: RuleStatus::ParamOnly,
        source: "params.md §10.3",
    },
    RuleInfo {
        id: "gap.cjk_latin",
        status: RuleStatus::ParamOnly,
        source: "W3C clreq §6.3.3; default ≤1/4 em",
    },
    RuleInfo {
        id: "gap.space",
        status: RuleStatus::ParamOnly,
        source: "params.md §10.3",
    },
    RuleInfo {
        id: "trim.open_left",
        status: RuleStatus::ParamOnly,
        source: "W3C clreq §6.3.2.3; ratio Tbd",
    },
    RuleInfo {
        id: "trim.close_right",
        status: RuleStatus::ParamOnly,
        source: "GB/T 15834-2011 §5.1.10 via clreq §6.3.2.3; ratio Tbd",
    },
    RuleInfo {
        id: "trim.middle",
        status: RuleStatus::ParamOnly,
        source: "W3C clreq §6.3.2.1/§6.3.2.3; ratio Tbd",
    },
    RuleInfo {
        id: "hard_break.forced",
        status: RuleStatus::Verified,
        source: "author intent via Breakability::Forced",
    },
];

/// Side-bearing trims applied when a punctuation atom sits at a line edge.
///
/// These are Tier B rule parameters in base em. A caller that knows the real
/// font size may scale them before building the IR; the breaker only reads the
/// resulting widths. Values follow [`params.md`](../../../docs/params.md)
/// section 10.3 and are still `Tbd` against a cited `clreq` table.
pub const OPEN_TRIM_LEFT: Width = Width::from_em_frac(1, 4);
pub const CLOSE_TRIM_RIGHT: Width = Width::from_em_frac(1, 2);
pub const MIDDLE_TRIM_LEFT: Width = Width::from_em_frac(1, 2);
pub const MIDDLE_TRIM_RIGHT: Width = Width::from_em_frac(1, 2);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuleError {
    EmptyText,
}

impl core::fmt::Display for RuleError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::EmptyText => f.write_str("paragraph text contains no atoms"),
        }
    }
}

impl core::error::Error for RuleError {}

/// Compile one paragraph. The caller supplies paragraph geometry and a style
/// id; the rules layer does not know font sizes or line widths beyond the
/// paragraph width.
pub fn paragraph_from_text(
    text: &str,
    style: ParagraphStyle,
    style_id: StyleId,
) -> Result<Paragraph, RuleError> {
    let mut builder = Builder::new();
    builder.push_text(text, style_id);
    builder.finish(style)
}

/// What one inline span represents.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpanKind {
    /// Segmented by the normal rules; spaces collapse across spans.
    Text,
    /// One unbreakable atom (inline code).
    Code,
    /// One unbreakable atom (replaced object: image, math fallback).
    Object,
    /// A forced break between the previous and the next atom.
    HardBreak,
}

/// One styled run of inline content.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InlineSpan<'a> {
    pub text: &'a str,
    pub style: StyleId,
    pub kind: SpanKind,
}

impl<'a> InlineSpan<'a> {
    pub const fn text(text: &'a str, style: StyleId) -> Self {
        Self {
            text,
            style,
            kind: SpanKind::Text,
        }
    }

    pub const fn code(text: &'a str, style: StyleId) -> Self {
        Self {
            text,
            style,
            kind: SpanKind::Code,
        }
    }

    pub const fn object(text: &'a str, style: StyleId) -> Self {
        Self {
            text,
            style,
            kind: SpanKind::Object,
        }
    }

    pub const fn hard_break(style: StyleId) -> Self {
        Self {
            text: "",
            style,
            kind: SpanKind::HardBreak,
        }
    }
}

/// Compile a styled inline sequence. Shares the segmentation, glue, and
/// punctuation rules with [`paragraph_from_text`] (ADR-0011).
pub fn paragraph_from_spans(
    spans: &[InlineSpan<'_>],
    style: ParagraphStyle,
) -> Result<Paragraph, RuleError> {
    let mut builder = Builder::new();
    for span in spans {
        match span.kind {
            SpanKind::Text => builder.push_text(span.text, span.style),
            SpanKind::Code => {
                let trimmed = span.text.trim();
                if trimmed.is_empty() {
                    continue;
                }
                let text = String::from(trimmed);
                builder.push_atom(text, AtomKind::Other, PunctClass::None, span.style);
            }
            SpanKind::Object => {
                if span.text.is_empty() {
                    continue;
                }
                let text = String::from(span.text);
                builder.push_atom(text, AtomKind::InlineObject, PunctClass::None, span.style);
            }
            SpanKind::HardBreak => builder.hard_break(),
        }
    }
    builder.finish(style)
}

/// Shared state for both entry points. `pending_space` lives here so it carries
/// across spans: a trailing space in one run produces the same glue as a space
/// inside one run.
struct Builder {
    atoms: Vec<Atom>,
    boundaries: Vec<Boundary>,
    join: Join,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Join {
    Normal,
    Space,
    Forced,
}

impl Builder {
    fn new() -> Self {
        Self {
            atoms: Vec::new(),
            boundaries: Vec::new(),
            join: Join::Normal,
        }
    }

    fn space(&mut self) {
        self.join = Join::Space;
    }

    fn hard_break(&mut self) {
        if !self.atoms.is_empty() {
            self.join = Join::Forced;
        }
    }

    fn push_atom(&mut self, text: String, kind: AtomKind, class: PunctClass, style: StyleId) {
        let join = if self.atoms.is_empty() {
            Join::Normal
        } else {
            self.join
        };
        push_atom(
            &mut self.atoms,
            &mut self.boundaries,
            text,
            kind,
            class,
            style,
            join,
        );
        self.join = Join::Normal;
    }

    fn push_text(&mut self, text: &str, style_id: StyleId) {
        let chars: Vec<char> = text.chars().collect();
        let mut index = 0;

        while index < chars.len() {
            let current = chars[index];

            if current.is_whitespace() {
                self.space();
                index += 1;
                continue;
            }

            match char_kind(current) {
                CharKind::Cjk => {
                    self.push_atom(
                        single_char(current),
                        AtomKind::Cjk,
                        punct_class(current),
                        style_id,
                    );
                    index += 1;
                }
                CharKind::Digit => {
                    let mut value = String::new();
                    value.push(current);
                    index += 1;
                    while index < chars.len() {
                        let next = chars[index];
                        let is_digit = next.is_ascii_digit();
                        let is_decimal_separator = (next == '.' || next == ',')
                            && index + 1 < chars.len()
                            && chars[index + 1].is_ascii_digit();
                        if is_digit || is_decimal_separator {
                            value.push(next);
                            index += 1;
                        } else {
                            break;
                        }
                    }
                    if index < chars.len() && chars[index] == '%' {
                        value.push('%');
                        index += 1;
                    }
                    self.push_atom(value, AtomKind::DigitRun, PunctClass::None, style_id);
                }
                CharKind::Latin => {
                    let mut value = String::new();
                    value.push(current);
                    index += 1;
                    while index < chars.len() && is_latin_run_char(chars[index]) {
                        value.push(chars[index]);
                        index += 1;
                    }
                    self.push_atom(value, AtomKind::LatinRun, PunctClass::None, style_id);
                }
                CharKind::Other => {
                    self.push_atom(
                        single_char(current),
                        AtomKind::Other,
                        punct_class(current),
                        style_id,
                    );
                    index += 1;
                }
            }
        }
    }

    fn finish(self, style: ParagraphStyle) -> Result<Paragraph, RuleError> {
        if self.atoms.is_empty() {
            return Err(RuleError::EmptyText);
        }
        Ok(Paragraph {
            style,
            atoms: self.atoms,
            boundaries: self.boundaries,
        })
    }
}

/// Compile source lines that must stay on separate visual lines (verse or a
/// Markdown hard break). Every boundary between two input lines is emitted as
/// `Breakability::Forced`; inline rules still run inside each line.
///
/// Empty input lines are rejected rather than silently dropped: a blank line in
/// Markdown starts a new paragraph and belongs to the block front end.
pub fn paragraph_from_lines(
    lines: &[&str],
    style: ParagraphStyle,
    style_id: StyleId,
) -> Result<Paragraph, RuleError> {
    let mut merged: Option<Paragraph> = None;

    for line in lines {
        let part = paragraph_from_text(line, style, style_id)?;
        match merged.as_mut() {
            None => merged = Some(part),
            Some(paragraph) => {
                paragraph.boundaries.push(Boundary {
                    breakable: Breakability::Forced,
                    kind: BoundaryKind::Normal,
                    role: BoundaryRole::Plain,
                    glue: Glue::default(),
                    penalty: 0,
                });
                paragraph.atoms.extend(part.atoms);
                paragraph.boundaries.extend(part.boundaries);
            }
        }
    }

    merged.ok_or(RuleError::EmptyText)
}

fn push_atom(
    atoms: &mut Vec<Atom>,
    boundaries: &mut Vec<Boundary>,
    text: String,
    kind: AtomKind,
    class: PunctClass,
    style: StyleId,
    join: Join,
) {
    let (compress_left, compress_right) = trim_caps(class);
    let atom = Atom {
        text,
        kind,
        class,
        style,
        compress_left,
        compress_right,
    };

    if let Some(previous) = atoms.last() {
        let boundary = match join {
            Join::Normal => normal_boundary(previous, &atom),
            Join::Space => space_boundary(previous, &atom),
            Join::Forced => Boundary {
                breakable: Breakability::Forced,
                kind: BoundaryKind::Normal,
                role: BoundaryRole::Plain,
                glue: Glue::default(),
                penalty: 0,
            },
        };
        boundaries.push(boundary);
    }

    atoms.push(atom);
}

fn trim_caps(class: PunctClass) -> (Width, Width) {
    match class {
        PunctClass::Open => (OPEN_TRIM_LEFT, Width::ZERO),
        PunctClass::Close => (Width::ZERO, CLOSE_TRIM_RIGHT),
        PunctClass::Middle => (MIDDLE_TRIM_LEFT, MIDDLE_TRIM_RIGHT),
        PunctClass::None => (Width::ZERO, Width::ZERO),
    }
}

fn normal_boundary(previous: &Atom, next: &Atom) -> Boundary {
    let forbidden = previous.class == PunctClass::Open
        || next.class == PunctClass::Close
        || next.class == PunctClass::Middle
        || (is_word_like(previous.kind) && is_word_like(next.kind));

    // A forbidden boundary is still internal when the line does not break
    // there. Do not add a CJK/Latin gap before a comma or after an opening
    // bracket; the punctuation's own side bearing is enough.
    let glue = if forbidden {
        Glue::default()
    } else if is_cjk_kind(previous.kind) && is_cjk_kind(next.kind) {
        Glue {
            width: Width::ZERO,
            stretch: CJK_GAP_STRETCH,
            shrink: CJK_GAP_SHRINK,
        }
    } else if (is_cjk_kind(previous.kind) && is_word_like(next.kind))
        || (is_word_like(previous.kind) && is_cjk_kind(next.kind))
    {
        Glue {
            width: CJK_LATIN_GAP,
            stretch: CJK_LATIN_STRETCH,
            shrink: CJK_LATIN_SHRINK,
        }
    } else {
        Glue::default()
    };

    Boundary {
        breakable: if forbidden {
            Breakability::Forbidden
        } else {
            Breakability::Allowed
        },
        kind: BoundaryKind::Normal,
        role: if !forbidden
            && ((is_cjk_kind(previous.kind) && is_word_like(next.kind))
                || (is_word_like(previous.kind) && is_cjk_kind(next.kind)))
        {
            BoundaryRole::CjkLatin
        } else {
            BoundaryRole::Plain
        },
        glue,
        penalty: 0,
    }
}

fn space_boundary(previous: &Atom, next: &Atom) -> Boundary {
    let forbidden = previous.class == PunctClass::Open
        || next.class == PunctClass::Close
        || next.class == PunctClass::Middle;

    Boundary {
        breakable: if forbidden {
            Breakability::Forbidden
        } else {
            Breakability::Allowed
        },
        kind: BoundaryKind::Space,
        role: BoundaryRole::Plain,
        glue: Glue {
            width: Width::ZERO,
            stretch: SPACE_STRETCH,
            shrink: SPACE_SHRINK,
        },
        penalty: 0,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CharKind {
    Cjk,
    Digit,
    Latin,
    Other,
}

fn char_kind(c: char) -> CharKind {
    if is_cjk(c) {
        CharKind::Cjk
    } else if c.is_ascii_digit() {
        CharKind::Digit
    } else if c.is_ascii_alphabetic() || c == '_' {
        CharKind::Latin
    } else {
        CharKind::Other
    }
}

fn is_latin_run_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '\''
}

fn is_word_like(kind: AtomKind) -> bool {
    matches!(
        kind,
        AtomKind::LatinRun | AtomKind::DigitRun | AtomKind::InlineObject
    )
}

fn is_cjk_kind(kind: AtomKind) -> bool {
    kind == AtomKind::Cjk
}

fn single_char(c: char) -> String {
    let mut value = String::new();
    value.push(c);
    value
}

/// Deliberately small punctuation table. It will be replaced by a versioned
/// rule set with `clreq`/GB source references.
pub fn punct_class(c: char) -> PunctClass {
    match c {
        '（' | '【' | '《' | '〈' | '「' | '『' | '“' | '‘' | '(' | '[' | '{' => {
            PunctClass::Open
        }
        '）' | '】' | '》' | '〉' | '」' | '』' | '”' | '’' | ')' | ']' | '}' => {
            PunctClass::Close
        }
        '，' | '。' | '、' | '；' | '：' | '！' | '？' | '…' | ',' | '.' | ';' | ':' | '!'
        | '?' => PunctClass::Middle,
        _ => PunctClass::None,
    }
}

/// CJK ranges covered by the first-pass segmenter. This is a coarse script
/// test, not a Unicode line-breaking property table.
pub fn is_cjk(c: char) -> bool {
    matches!(
        c as u32,
        0x3000..=0x303F
            | 0x3040..=0x30FF
            | 0x3400..=0x4DBF
            | 0x4E00..=0x9FFF
            | 0xF900..=0xFAFF
            | 0xFF00..=0xFFEF
            | 0x20000..=0x2FA1F
    )
}

/// Convenience constructor for the common justified body style.
pub fn justified_style(width: Width) -> ParagraphStyle {
    ParagraphStyle {
        width,
        first_indent: Width::ZERO,
        line_height: Width::from_em_frac(3, 2),
        align: Align::Justify,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec::Vec;
    use soma_core::UNITS_PER_EM;

    fn compile(text: &str) -> Paragraph {
        paragraph_from_text(text, justified_style(Width::from_em(10)), StyleId(0)).unwrap()
    }

    #[test]
    fn segments_mixed_text() {
        let paragraph = compile("中文 English，混排");
        assert_eq!(paragraph.atoms.len(), 6);
        assert_eq!(paragraph.boundaries.len(), 5);

        assert_eq!(paragraph.atoms[0].text, "中");
        assert_eq!(paragraph.atoms[2].text, "English");
        assert_eq!(paragraph.atoms[2].kind, AtomKind::LatinRun);
        assert_eq!(paragraph.atoms[3].class, PunctClass::Middle);

        assert_eq!(paragraph.boundaries[1].kind, BoundaryKind::Space);
        assert_eq!(paragraph.boundaries[1].role, BoundaryRole::Plain);
        assert_eq!(paragraph.boundaries[1].breakable, Breakability::Allowed);
        assert_eq!(paragraph.boundaries[2].breakable, Breakability::Forbidden);
    }

    #[test]
    fn cjk_latin_without_space_gets_ideal_gap() {
        let paragraph = compile("中文English");
        let boundary = paragraph.boundaries[1];
        assert_eq!(boundary.breakable, Breakability::Allowed);
        assert_eq!(boundary.role, BoundaryRole::CjkLatin);
        assert_eq!(boundary.glue.width, Width::from_em_frac(1, 4));
    }

    #[test]
    fn decimal_number_is_one_atom() {
        let paragraph = compile("3.14");
        assert_eq!(paragraph.atoms.len(), 1);
        assert_eq!(paragraph.atoms[0].text, "3.14");
        assert_eq!(paragraph.atoms[0].kind, AtomKind::DigitRun);
    }

    #[test]
    fn opening_and_closing_punctuation_forbid_breaks() {
        let paragraph = compile("（引号）");
        assert_eq!(paragraph.atoms.len(), 4);
        assert_eq!(paragraph.boundaries[0].breakable, Breakability::Forbidden);
        assert_eq!(paragraph.boundaries[2].breakable, Breakability::Forbidden);
    }

    #[test]
    fn space_before_punctuation_is_forbidden() {
        let paragraph = compile("word ,");
        assert_eq!(paragraph.atoms.len(), 2);
        assert_eq!(paragraph.boundaries[0].kind, BoundaryKind::Space);
        assert_eq!(paragraph.boundaries[0].breakable, Breakability::Forbidden);
    }

    #[test]
    fn empty_text_is_rejected() {
        assert_eq!(
            paragraph_from_text(" \n\t", justified_style(Width::from_em(10)), StyleId(0)),
            Err(RuleError::EmptyText)
        );
    }

    #[test]
    fn empty_hard_break_line_is_rejected() {
        assert_eq!(
            paragraph_from_lines(&["甲", ""], justified_style(Width::from_em(10)), StyleId(0)),
            Err(RuleError::EmptyText)
        );
    }

    #[test]
    fn punctuation_trim_caps_follow_the_rule_table() {
        let paragraph = compile("（字）。");

        assert_eq!(paragraph.atoms[0].compress_left, OPEN_TRIM_LEFT);
        assert_eq!(paragraph.atoms[0].compress_right, Width::ZERO);
        assert_eq!(paragraph.atoms[1].compress_left, Width::ZERO);
        assert_eq!(paragraph.atoms[1].compress_right, Width::ZERO);
        assert_eq!(paragraph.atoms[2].compress_left, Width::ZERO);
        assert_eq!(paragraph.atoms[2].compress_right, CLOSE_TRIM_RIGHT);
        assert_eq!(paragraph.atoms[3].compress_left, MIDDLE_TRIM_LEFT);
        assert_eq!(paragraph.atoms[3].compress_right, MIDDLE_TRIM_RIGHT);
    }

    #[test]
    fn rule_inventory_is_complete_and_unique() {
        let mut ids: Vec<&str> = Vec::new();
        for rule in RULES {
            assert!(!rule.id.is_empty());
            assert!(!rule.source.is_empty());
            assert!(!ids.contains(&rule.id), "duplicate rule id {}", rule.id);
            ids.push(rule.id);
        }

        assert!(RULES
            .iter()
            .any(|rule| { rule.id == "hard_break.forced" && rule.status == RuleStatus::Verified }));
        assert!(RULES.iter().any(|rule| {
            rule.id == "break.middle.no_line_start" && rule.status == RuleStatus::Verified
        }));
    }

    #[test]
    fn verified_rules_do_not_carry_provisional_sources() {
        for rule in RULES {
            if rule.status == RuleStatus::Verified {
                let source = rule.source.to_ascii_lowercase();
                assert!(
                    !source.contains("tbd"),
                    "verified rule {} still has a provisional source: {}",
                    rule.id,
                    rule.source
                );
            }
        }
    }

    /// Rules that compile to `Forbidden` / `Forced`. The policy is that only
    /// `Verified` rules may be hard, so a `Tbd` row appearing here is a bug.
    const HARD_BREAK_RULE_IDS: &[&str] = &[
        "break.open.no_break_after",
        "break.close.no_break_before",
        "break.middle.no_line_start",
        "break.space_before_punct",
        "hard_break.forced",
    ];

    #[test]
    fn hard_break_rules_are_verified() {
        for id in HARD_BREAK_RULE_IDS {
            let rule = RULES
                .iter()
                .find(|rule| rule.id == *id)
                .unwrap_or_else(|| panic!("missing rule {id}"));
            assert_eq!(rule.status, RuleStatus::Verified, "{id} is a hard rule");
        }
    }

    #[test]
    fn tbd_rules_are_never_hard_break_rules() {
        for rule in RULES {
            if rule.status == RuleStatus::Tbd {
                assert!(
                    !HARD_BREAK_RULE_IDS.contains(&rule.id),
                    "Tbd rule {} is compiled as a hard break rule",
                    rule.id
                );
            }
        }
    }

    #[test]
    fn units_per_em_is_stable() {
        assert_eq!(UNITS_PER_EM, 4096);
    }
}
