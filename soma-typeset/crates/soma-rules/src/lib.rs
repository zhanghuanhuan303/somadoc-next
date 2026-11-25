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

use alloc::sync::Arc;

use soma_core::{
    Align, Atom, AtomKind, Boundary, BoundaryKind, BoundaryRole, Breakability, Decorations, Glue,
    Paragraph, ParagraphStyle, PunctClass, StyleId, Width,
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
        id: "trim.pair",
        status: RuleStatus::ParamOnly,
        source: "W3C clreq §6.3.2.2 (adjacent punctuation); ratio Tbd",
    },
    RuleInfo {
        id: "hard_break.forced",
        status: RuleStatus::Verified,
        source: "author intent via Breakability::Forced",
    },
];

/// Side-bearing trims applied when a punctuation atom sits at a line edge.
///
/// These are Tier B rule parameters in base em, and they are a *budget*, not a
/// measurement: the amount an atom may be pulled in when a line has nowhere
/// else to go. `docs/trim-calibration.md` measures the empty space each mark
/// actually leaves (0.627 em outside an opening mark, 0.184 em inside a middle
/// one), and setting the budget to those numbers made the corpus worse — two
/// avoidable fitness jumps appeared and ratio jitter rose — so the shipped
/// values stay (ADR-0051). A face whose metrics table carries outline ink gets
/// the exact gap per glyph anyway (ADR-0037), which is what keeps a glyph from
/// moving.
pub const OPEN_TRIM_LEFT: Width = Width::from_em_frac(1, 2);
pub const CLOSE_TRIM_RIGHT: Width = Width::from_em_frac(1, 2);
pub const MIDDLE_TRIM_LEFT: Width = Width::from_em_frac(1, 2);
pub const MIDDLE_TRIM_RIGHT: Width = Width::from_em_frac(1, 2);

/// Side bearing of half-width punctuation. A comma is roughly a quarter em
/// wide; trimming half an em would eat the glyph itself, and hanging the same
/// amount would push it into the margin.
pub const LATIN_TRIM: Width = Width::from_em_frac(1, 8);

/// Extra left-side compression for a punctuation mark that directly follows
/// another punctuation mark (`。」`, `），`, `？！」`).
pub const PAIR_TRIM: Width = Width::from_em_frac(1, 4);

/// Break penalty on a hyphenation point, in the kernel's penalty units. A
/// space has none, so the breaker only hyphenates when a line would otherwise
/// be badly loose or overfull.
const HYPHEN_PENALTY: i32 = 100;
/// A break inside inline code: at one of its spaces, or after one of its
/// separators. It is a last resort like a hyphenation point; a code run with
/// neither cannot be split.
const CODE_BREAK_PENALTY: i32 = 100;
/// A break after `-` or `/` inside a Latin run (`metronome-rs`,
/// `tactus-time/core`). A space is free, so the breaker splits such a run
/// only when keeping it whole would stretch or overfull the line worse than
/// this penalty (ADR-0111).
const WORD_BREAK_PENALTY: i32 = 500;
/// A break inside a long code run that has no space and no separator
/// (`a1b2c3d4e5f6a7b8`, a base64 token). The run is split per character and
/// the break is priced far above a separator break, so it only happens when
/// the line cannot fit the run any other way (D114).
const INTRUSIVE_BREAK_PENALTY: i32 = 2_000;
/// A code segment longer than this many characters is split per character.
/// Shorter segments stay one atom: a normal code word must not litter the
/// paragraph with price-only break points.
const CODE_INTRUSIVE_MIN_CHARS: usize = 12;

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

/// Compile one source line of a code block.
///
/// Whitespace becomes real atoms here: a command copied out of the page has to
/// keep its indentation and its argument spacing, so runs of spaces are not
/// collapsed and a leading run is not dropped.
pub fn paragraph_from_code_line(
    text: &str,
    style: ParagraphStyle,
    style_id: StyleId,
) -> Result<Paragraph, RuleError> {
    let mut builder = Builder::new();
    let mut run = String::new();
    for character in text.chars() {
        if character.is_whitespace() {
            if !run.is_empty() {
                builder.push_text(&run, style_id);
                run.clear();
            }
            builder.push_atom(
                String::from(" "),
                AtomKind::Other,
                PunctClass::None,
                style_id,
            );
        } else {
            run.push(character);
        }
    }
    if !run.is_empty() {
        builder.push_text(&run, style_id);
    }
    builder.finish(style)
}

/// What one inline span represents.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpanKind {
    /// Segmented by the normal rules; spaces collapse across spans.
    Text,
    /// Inline code: unbreakable within an identifier, but a line may wrap at
    /// the whitespace inside it (ADR-0111).
    Code,
    /// One unbreakable atom (replaced object: image, math fallback).
    Object,
    /// One unbreakable atom that must stay attached to the atom on its left:
    /// a footnote reference. A break before it would start a line with a
    /// superscript number, and the CJK/Latin gap would open a hole before it.
    Footnote,
    /// One unbreakable atom: a ruby base. The annotation travels beside it in
    /// the layout block, not in the atom text, so line breaking only sees the
    /// base width.
    Ruby,
    /// A forced break between the previous and the next atom.
    HardBreak,
    /// A continuation piece of a hyphenated word: this span starts at a
    /// hyphenation point, so a line may break before it and draw a hyphen.
    Hyphen,
    /// A break opportunity inside inline math, with the math spacing that the
    /// boundary carries. `InlineSpan::width` and `::penalty` give the glue and
    /// the TeX penalty (`\relpenalty`/`\binoppenalty`).
    MathBreak,
}

/// One styled run of inline content.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InlineSpan<'a> {
    pub text: &'a str,
    pub style: StyleId,
    pub kind: SpanKind,
    /// Underline and friends, carried to the renderers (ADR-0044).
    pub decor: Decorations,
    /// Link target, carried to the renderers.
    pub link: Option<&'a str>,
    /// Glue width for `SpanKind::MathBreak` (zero otherwise).
    pub width: Width,
    /// Break penalty for `SpanKind::MathBreak` (zero otherwise).
    pub penalty: i32,
}

impl<'a> InlineSpan<'a> {
    pub const fn text(text: &'a str, style: StyleId) -> Self {
        Self {
            text,
            style,
            kind: SpanKind::Text,
            decor: Decorations::NONE,
            link: None,
            width: Width::ZERO,
            penalty: 0,
        }
    }

    pub const fn code(text: &'a str, style: StyleId) -> Self {
        Self {
            text,
            style,
            kind: SpanKind::Code,
            decor: Decorations::NONE,
            link: None,
            width: Width::ZERO,
            penalty: 0,
        }
    }

    pub const fn object(text: &'a str, style: StyleId) -> Self {
        Self {
            text,
            style,
            kind: SpanKind::Object,
            decor: Decorations::NONE,
            link: None,
            width: Width::ZERO,
            penalty: 0,
        }
    }

    pub const fn footnote(text: &'a str, style: StyleId) -> Self {
        Self {
            text,
            style,
            kind: SpanKind::Footnote,
            decor: Decorations::NONE,
            link: None,
            width: Width::ZERO,
            penalty: 0,
        }
    }

    pub const fn hard_break(style: StyleId) -> Self {
        Self {
            text: "",
            style,
            kind: SpanKind::HardBreak,
            decor: Decorations::NONE,
            link: None,
            width: Width::ZERO,
            penalty: 0,
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
        builder.decor = span.decor;
        builder.link = span.link;
        match span.kind {
            SpanKind::Text => builder.push_text(span.text, span.style),
            SpanKind::Code => {
                let trimmed = span.text.trim();
                if trimmed.is_empty() {
                    continue;
                }
                // Inline code breaks at its own spaces and, at a price, after
                // `-`, `/`, `_` or `.`, and at camelCase seams: a long
                // expression such as `round(tick * tempo_num / den)` may wrap
                // after `tick`, and a URL may wrap after a path separator,
                // while `metronome-rs` stays whole unless the line cannot
                // afford it. A run with none of those (a hash, a base64 token)
                // is split per character behind a much higher penalty so it
                // can wrap at all (D114). White space stays at the tail of the
                // piece before it, so text copied out of the page keeps its
                // exact spacing (ADR-0111, ADR-0112).
                let mut first = true;
                for piece in code_pieces(trimmed) {
                    for (index, segment) in code_segments(&piece).into_iter().enumerate() {
                        let join = if index == 0 {
                            Join::Code
                        } else {
                            Join::WordBreak
                        };
                        if segment.chars().count() > CODE_INTRUSIVE_MIN_CHARS {
                            for (char_index, piece) in
                                intrusive_pieces(segment).into_iter().enumerate()
                            {
                                if !first {
                                    builder.join = if char_index == 0 {
                                        join
                                    } else {
                                        Join::Intrusive
                                    };
                                }
                                builder.push_atom(
                                    piece,
                                    AtomKind::Other,
                                    PunctClass::None,
                                    span.style,
                                );
                                first = false;
                            }
                        } else {
                            if !first {
                                builder.join = join;
                            }
                            builder.push_atom(
                                String::from(segment),
                                AtomKind::Other,
                                PunctClass::None,
                                span.style,
                            );
                            first = false;
                        }
                    }
                }
            }
            SpanKind::Object => {
                if span.text.is_empty() {
                    continue;
                }
                let text = String::from(span.text);
                builder.push_atom(text, AtomKind::InlineObject, PunctClass::None, span.style);
            }
            SpanKind::Footnote => {
                if span.text.is_empty() {
                    continue;
                }
                let text = String::from(span.text);
                builder.join = Join::Attach;
                builder.push_atom(text, AtomKind::InlineObject, PunctClass::None, span.style);
            }
            SpanKind::Ruby => {
                let trimmed = span.text.trim();
                if trimmed.is_empty() {
                    continue;
                }
                let text = String::from(trimmed);
                builder.push_atom(text, AtomKind::Ruby, PunctClass::None, span.style);
            }
            SpanKind::Hyphen => {
                if span.text.is_empty() {
                    continue;
                }
                builder.join = Join::Hyphen;
                builder.push_text(span.text, span.style);
            }
            SpanKind::MathBreak => builder.math_break(span.width, span.penalty),
            SpanKind::HardBreak => builder.hard_break(),
        }
    }
    builder.finish(style)
}

/// Split inline code into pieces that each end at a run of whitespace. The
/// pieces concatenate back to `text`, so text copied out of the page is the
/// exact source; the trailing space is what makes the boundary measurable
/// without a font in the rules layer.
fn code_pieces(text: &str) -> Vec<String> {
    let mut pieces: Vec<String> = Vec::new();
    for part in text.split_inclusive(char::is_whitespace) {
        if part.chars().all(char::is_whitespace) {
            if let Some(previous) = pieces.last_mut() {
                previous.push_str(part);
                continue;
            }
        }
        pieces.push(String::from(part));
    }
    pieces
}

/// Split one code piece after a separator or at a camelCase seam, keeping the
/// separator at the end of the segment before it. A separator next to a space
/// is left alone: the space is the better break, and a segment that begins
/// with one is never a legal line start.
///
/// `-`, `/` and `_` always open a seam; `.` does so only between word
/// characters and not between two digits (`1.5` is one token). A lower→upper
/// or acronym→word transition (`someName`, `HTTPServer`) opens a seam as well,
/// because long identifiers are otherwise unbreakable (D114).
fn code_segments(piece: &str) -> Vec<&str> {
    let chars: Vec<(usize, char)> = piece.char_indices().collect();
    let mut segments = Vec::new();
    let mut start = 0usize;
    for index in 0..chars.len() {
        let (offset, character) = chars[index];
        let end = offset + character.len_utf8();
        let Some((_, next)) = chars.get(index + 1) else {
            break;
        };
        if next.is_whitespace() {
            continue;
        }
        let previous = index.checked_sub(1).map(|previous| chars[previous].1);
        let split = match character {
            '-' | '/' | '_' => true,
            '.' => {
                previous.is_some_and(|previous| previous.is_ascii_alphanumeric())
                    && next.is_ascii_alphanumeric()
                    && !(previous.is_some_and(|previous| previous.is_ascii_digit())
                        && next.is_ascii_digit())
            }
            _ => {
                (next.is_ascii_uppercase()
                    && (character.is_ascii_lowercase() || character.is_ascii_digit()))
                    || (character.is_ascii_uppercase()
                        && next.is_ascii_uppercase()
                        && chars
                            .get(index + 2)
                            .is_some_and(|(_, lower)| lower.is_ascii_lowercase()))
            }
        };
        if split {
            segments.push(&piece[start..end]);
            start = end;
        }
    }
    segments.push(&piece[start..]);
    segments
}

/// One atom per character for a long code segment. Whitespace (which can only
/// sit at the tail of a segment) is glued to the atom before it, so a line
/// never starts with an invisible space atom.
fn intrusive_pieces(segment: &str) -> Vec<String> {
    let mut pieces: Vec<String> = Vec::new();
    for character in segment.chars() {
        if character.is_whitespace() {
            if let Some(previous) = pieces.last_mut() {
                previous.push(character);
                continue;
            }
        }
        pieces.push(single_char(character));
    }
    pieces
}

/// Shared state for both entry points. `pending_space` lives here so it carries
/// across spans: a trailing space in one run produces the same glue as a space
/// inside one run.
struct Builder<'a> {
    atoms: Vec<Atom>,
    boundaries: Vec<Boundary>,
    join: Join,
    /// Decorations and link of the span currently being pushed.
    decor: Decorations,
    link: Option<&'a str>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Join {
    Normal,
    Space,
    Forced,
    /// Attach this atom to the previous one: no break, no gap. Used by a
    /// footnote reference, which belongs to the character before it.
    Attach,
    /// Start of a continuation piece of a hyphenated word.
    Hyphen,
    /// A break opportunity inside inline math with its glue and penalty.
    Math(Width, i32),
    /// A break after a space inside inline code. The space is kept in the
    /// preceding atom, so copying the code still yields the exact source.
    Code,
    /// A break after a hyphen or a slash inside a Latin run or a code piece.
    /// The break is allowed — a long URL or identifier must be able to wrap —
    /// but it carries a penalty, so a word stays whole whenever the line can
    /// afford it.
    WordBreak,
    /// A break inside a long code run with no space and no separator. The run
    /// is split per character; the penalty is high enough that the breaker
    /// only reaches for it when the run cannot fit on the line at all.
    Intrusive,
}

impl<'a> Builder<'a> {
    fn new() -> Self {
        Self {
            atoms: Vec::new(),
            boundaries: Vec::new(),
            join: Join::Normal,
            decor: Decorations::NONE,
            link: None,
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

    fn math_break(&mut self, width: Width, penalty: i32) {
        if !self.atoms.is_empty() {
            self.join = Join::Math(width, penalty);
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
            self.decor,
            self.link,
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
                        if matches!(chars[index - 1], '-' | '/') {
                            break;
                        }
                    }
                    let separated = matches!(value.chars().last(), Some('-' | '/'));
                    self.push_atom(value, AtomKind::LatinRun, PunctClass::None, style_id);
                    if separated {
                        self.join = Join::WordBreak;
                    }
                }
                CharKind::Other => {
                    let separated = matches!(current, '-' | '/');
                    self.push_atom(
                        single_char(current),
                        AtomKind::Other,
                        punct_class(current),
                        style_id,
                    );
                    if separated {
                        self.join = Join::WordBreak;
                    }
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

#[allow(clippy::too_many_arguments)]
fn push_atom(
    atoms: &mut Vec<Atom>,
    boundaries: &mut Vec<Boundary>,
    text: String,
    kind: AtomKind,
    class: PunctClass,
    style: StyleId,
    join: Join,
    decor: Decorations,
    link: Option<&str>,
) {
    let full_width = text.chars().next().is_some_and(is_full_width_punct);
    let (mut compress_left, compress_right) = trim_caps(class, full_width);
    // Pair compression is about two marks set against each other. A space or a
    // forced break between them breaks the pair.
    if class != PunctClass::None && join == Join::Normal {
        if let Some(previous) = atoms.last() {
            if previous.class != PunctClass::None {
                compress_left = compress_left.saturating_add(PAIR_TRIM);
            }
        }
    }
    let atom = Atom {
        text,
        kind,
        class,
        style,
        decor,
        link: link.map(Arc::from),
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
            Join::Attach => Boundary {
                breakable: Breakability::Forbidden,
                kind: BoundaryKind::Normal,
                role: BoundaryRole::Plain,
                glue: Glue::default(),
                penalty: 0,
            },
            Join::Hyphen => Boundary {
                breakable: Breakability::Allowed,
                kind: BoundaryKind::Hyphen,
                role: BoundaryRole::Plain,
                glue: Glue::default(),
                // Lower than a space but not free: a break inside a word is a
                // last resort before overfull, not a preferred break.
                penalty: HYPHEN_PENALTY,
            },
            Join::Math(width, penalty) => Boundary {
                breakable: Breakability::Allowed,
                kind: BoundaryKind::Normal,
                role: BoundaryRole::Plain,
                glue: Glue {
                    width,
                    ..Glue::default()
                },
                penalty,
            },
            Join::Code => {
                let mut boundary = normal_boundary(previous, &atom);
                if boundary.breakable == Breakability::Allowed {
                    boundary.penalty = CODE_BREAK_PENALTY;
                }
                boundary
            }
            // Keep the ordinary mixed-script glue and the forbidden checks
            // (never start a line with a comma or a slash) and only raise the
            // price of breaking (ADR-0111).
            Join::WordBreak => {
                let mut boundary = normal_boundary(previous, &atom);
                if boundary.breakable == Breakability::Allowed {
                    boundary.penalty = WORD_BREAK_PENALTY;
                }
                boundary
            }
            // A character split inside a long code run: allowed only through
            // the fallback ladder's last resort, priced above every ordinary
            // break so a token that fits is never cut (D114).
            Join::Intrusive => {
                let mut boundary = normal_boundary(previous, &atom);
                if boundary.breakable == Breakability::Allowed {
                    boundary.penalty = INTRUSIVE_BREAK_PENALTY;
                }
                boundary
            }
        };
        boundaries.push(boundary);
    }

    atoms.push(atom);
}

fn trim_caps(class: PunctClass, full_width: bool) -> (Width, Width) {
    if !full_width {
        return match class {
            PunctClass::Open => (LATIN_TRIM, Width::ZERO),
            PunctClass::Close => (Width::ZERO, LATIN_TRIM),
            PunctClass::Middle => (LATIN_TRIM, LATIN_TRIM),
            PunctClass::Dash => (Width::ZERO, Width::ZERO),
            PunctClass::None => (Width::ZERO, Width::ZERO),
        };
    }
    match class {
        PunctClass::Open => (OPEN_TRIM_LEFT, Width::ZERO),
        PunctClass::Close => (Width::ZERO, CLOSE_TRIM_RIGHT),
        PunctClass::Middle => (MIDDLE_TRIM_LEFT, MIDDLE_TRIM_RIGHT),
        PunctClass::Dash => (Width::ZERO, Width::ZERO),
        PunctClass::None => (Width::ZERO, Width::ZERO),
    }
}

/// Full-width punctuation. The CJK quotation marks sit outside the CJK blocks
/// but are drawn one em wide in Chinese faces, so they trim like CJK marks.
pub fn is_full_width_punct(c: char) -> bool {
    is_cjk(c) || matches!(c, '\u{201c}' | '\u{201d}' | '\u{2018}' | '\u{2019}')
}

fn normal_boundary(previous: &Atom, next: &Atom) -> Boundary {
    let break_opportunity = is_break_opportunity(previous);
    // Kinsoku (禁则) checks are independent of a break opportunity: `well-，`
    // may not start a line with the comma even though `-` is a natural
    // place to break. The opportunity only relaxes the word-like rule.
    let forbidden = previous.class == PunctClass::Open
        || next.class == PunctClass::Close
        || next.class == PunctClass::Middle
        || next.class == PunctClass::Dash
        || starts_with_separator(next)
        || (!break_opportunity && is_word_like(previous.kind) && is_word_like(next.kind));

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
        || next.class == PunctClass::Middle
        || next.class == PunctClass::Dash;

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
    // A slash inside a run stays with the piece before it, so `time/core`
    // yields `time/` + `core`: a break after it is a priced word break, not
    // a free break around a stray `/` atom (ADR-0111).
    c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '/' || c == '\''
}

/// True when a break after this atom is a natural mixed-script opportunity:
/// after a hyphen, a slash, a middle dot, or a zero-width space.
fn is_break_opportunity(atom: &Atom) -> bool {
    atom.text
        .chars()
        .last()
        .map(|c| matches!(c, '-' | '/' | '\u{00b7}' | '\u{200b}'))
        .unwrap_or(false)
}

/// A line may not begin with a separator. An inline-code piece keeps its
/// trailing space, so the test is on the first character, not equality
/// (`/ ` must be caught as well as a lone `/`).
fn starts_with_separator(atom: &Atom) -> bool {
    atom.text.starts_with('/') || atom.text.starts_with('-')
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
        '，' | '。' | '、' | '；' | '：' | '！' | '？' | ',' | '.' | ';' | ':' | '!' | '?' => {
            PunctClass::Middle
        }
        // The ellipsis, the dashes and the middle dot carry their own space:
        // they belong at neither edge of a line and must not be squeezed.
        '…' | '—' | '–' | '·' => PunctClass::Dash,
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
        hang: Width::ZERO,
        tracking: Width::ZERO,
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
    fn a_hyphen_or_slash_break_carries_a_penalty() {
        let paragraph = compile("metronome-rs 与 tactus-time/core");
        let texts: Vec<&str> = paragraph
            .atoms
            .iter()
            .map(|atom| atom.text.as_str())
            .collect();
        assert_eq!(
            texts,
            ["metronome-", "rs", "与", "tactus-", "time/", "core"]
        );

        // Splitting an identifier stays possible, but only when the line
        // cannot afford to keep it whole (ADR-0111).
        assert_eq!(paragraph.boundaries[0].breakable, Breakability::Allowed);
        assert_eq!(paragraph.boundaries[0].penalty, WORD_BREAK_PENALTY);
        assert_eq!(paragraph.boundaries[3].penalty, WORD_BREAK_PENALTY);
        assert_eq!(paragraph.boundaries[4].penalty, WORD_BREAK_PENALTY);
        assert_eq!(paragraph.boundaries[0].glue.width, Width::ZERO);
        assert_eq!(paragraph.boundaries[0].glue.stretch, Width::ZERO);
        // A space is still a free break.
        assert_eq!(paragraph.boundaries[1].penalty, 0);
    }

    #[test]
    fn a_break_after_a_hyphen_keeps_the_mixed_script_gap() {
        let paragraph = compile("foo-中文");
        let boundary = paragraph.boundaries[0];
        assert_eq!(boundary.breakable, Breakability::Allowed);
        assert_eq!(boundary.role, BoundaryRole::CjkLatin);
        assert_eq!(boundary.glue.width, CJK_LATIN_GAP);
        assert_eq!(boundary.penalty, WORD_BREAK_PENALTY);
    }

    #[test]
    fn a_hyphen_never_opens_a_break_before_punctuation() {
        let paragraph = compile("well-，好");
        // `well-` is followed by a comma: the boundary stays forbidden.
        assert_eq!(paragraph.boundaries[0].breakable, Breakability::Forbidden);
        assert_eq!(paragraph.boundaries[0].penalty, 0);
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
    fn a_dash_keeps_its_width_and_never_starts_a_line() {
        // `——` and `……` are two full-width marks; trimming half an em off each
        // side would close the run up (D83).
        for mark in ['—', '–', '…', '·'] {
            assert_eq!(punct_class(mark), PunctClass::Dash, "{mark}");
        }
        let paragraph = compile("甲——乙");
        let dash = &paragraph.atoms[1];
        assert_eq!(dash.class, PunctClass::Dash);
        assert_eq!(dash.compress_left, Width::ZERO);
        assert_eq!(dash.compress_right, Width::ZERO);
        assert_eq!(
            paragraph.boundaries[0].breakable,
            Breakability::Forbidden,
            "a line may not start with a dash"
        );
        assert_eq!(
            paragraph.boundaries[2].breakable,
            Breakability::Allowed,
            "but a line may break after the run"
        );
    }

    #[test]
    fn a_code_line_keeps_its_spaces() {
        let paragraph =
            paragraph_from_code_line("  a b", justified_style(Width::from_em(10)), StyleId(0))
                .expect("a code line");
        let texts: Vec<&str> = paragraph
            .atoms
            .iter()
            .map(|atom| atom.text.as_str())
            .collect();
        assert_eq!(texts, [" ", " ", "a", " ", "b"]);
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
        assert_eq!(
            paragraph.atoms[3].compress_left,
            MIDDLE_TRIM_LEFT.saturating_add(PAIR_TRIM)
        );
        assert_eq!(paragraph.atoms[3].compress_right, MIDDLE_TRIM_RIGHT);
    }

    #[test]
    fn half_width_punctuation_gets_a_small_side_bearing() {
        let paragraph = compile("a, b. (c)");
        let texts: Vec<&str> = paragraph
            .atoms
            .iter()
            .map(|atom| atom.text.as_str())
            .collect();
        assert_eq!(texts, ["a", ",", "b", ".", "(", "c", ")"]);

        let comma = &paragraph.atoms[1];
        assert_eq!(comma.compress_left, LATIN_TRIM);
        assert_eq!(comma.compress_right, LATIN_TRIM);

        let period = &paragraph.atoms[3];
        assert_eq!(period.compress_right, LATIN_TRIM);

        let paren = &paragraph.atoms[4];
        assert_eq!(paren.compress_left, LATIN_TRIM);
        assert_eq!(paren.compress_right, Width::ZERO);
    }

    #[test]
    fn cjk_quotation_marks_trim_like_cjk_marks() {
        let paragraph = compile("“字”");

        assert_eq!(paragraph.atoms[0].compress_left, OPEN_TRIM_LEFT);
        assert_eq!(paragraph.atoms[2].compress_right, CLOSE_TRIM_RIGHT);
    }

    #[test]
    fn adjacent_punctuation_gets_extra_compression() {
        let paragraph = compile("（（字）。");

        assert_eq!(
            paragraph.atoms[1].compress_left,
            OPEN_TRIM_LEFT.saturating_add(PAIR_TRIM)
        );
        assert_eq!(
            paragraph.atoms[4].compress_left,
            MIDDLE_TRIM_LEFT.saturating_add(PAIR_TRIM)
        );
        assert_eq!(paragraph.atoms[2].compress_left, Width::ZERO);
    }

    #[test]
    fn latin_runs_break_after_hyphens_and_slashes() {
        let paragraph = compile("well-known path/to/file");
        let texts: Vec<&str> = paragraph
            .atoms
            .iter()
            .map(|atom| atom.text.as_str())
            .collect();

        let hyphen = texts
            .iter()
            .position(|text| *text == "well-")
            .expect("hyphen");
        assert_eq!(
            paragraph.boundaries[hyphen].breakable,
            Breakability::Allowed,
            "a break after a hyphen must be allowed"
        );
        let slash = texts
            .iter()
            .position(|text| *text == "path/")
            .expect("slash");
        assert_eq!(
            paragraph.boundaries[slash].breakable,
            Breakability::Allowed,
            "a break after a slash must be allowed"
        );
        assert_eq!(
            paragraph.boundaries[slash].penalty, WORD_BREAK_PENALTY,
            "but it is priced above a space"
        );
        // A standalone slash is still its own atom, and a line may not start
        // with one.
        let spaced = compile("中文/abc");
        let standalone = spaced
            .atoms
            .iter()
            .position(|atom| atom.text == "/")
            .expect("standalone slash");
        assert_eq!(
            spaced.boundaries[standalone - 1].breakable,
            Breakability::Forbidden,
            "a line must not start with a slash"
        );
        let plain = texts
            .iter()
            .position(|text| *text == "known")
            .expect("word");
        assert_eq!(
            paragraph.boundaries[plain].breakable,
            Breakability::Allowed,
            "the boundary after a word is the following space"
        );
        assert!(
            texts.contains(&"known"),
            "the word after a hyphen is still one atom"
        );
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
