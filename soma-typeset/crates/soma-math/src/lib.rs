//! TeX-subset math parsing and integer math-box layout.
//!
//! Supported: symbols, groups, scripts, fractions, radicals, big operators with
//! limits, upright function names, `\left…\right` with stretched fences, and
//! `matrix` / `pmatrix` / `bmatrix` / `vmatrix` / `cases` / `aligned`. Unknown
//! commands are errors so the caller can fall back to the source text.
//!
//! ```text
//! x^2 + y_1 = \frac{a}{b} \quad \left( \begin{aligned} a &= 1 \\ b &= 2 \end{aligned} \right)
//! ```
//!
//! The layout is integer-only: every length is in 1/4096 em (the kernel unit),
//! and the result contains positioned glyphs plus rule rectangles. A renderer
//! only paints; it never re-lays-out.

#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]

extern crate alloc;

use alloc::boxed::Box;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::fmt;

use soma_core::{MathParam, Width};

/// Advances and vertical metrics for math glyphs at a given size.
pub trait MathMetrics {
    /// Advance of a short symbol run at `size` (1/4096 em).
    fn advance(&self, text: &str, size: Width) -> Width;
    /// Ascent of the font at `size` (above the baseline).
    fn ascent(&self, size: Width) -> Width;
    /// Descent of the font at `size` (below the baseline).
    fn descent(&self, size: Width) -> Width;

    /// Ink extent of a run at `size`: `(above baseline, below baseline)`.
    /// `None` when the host has no per-glyph outline data; callers then use
    /// the face's em box.
    fn ink(&self, _text: &str, _size: Width) -> Option<(Width, Width)> {
        None
    }

    /// Italic correction of the run's last glyph at `size`.
    fn italic_correction(&self, _text: &str, _size: Width) -> Width {
        Width::ZERO
    }

    /// One `MATH` constant, in per-mille of one em. `None` when the face has
    /// no `MATH` table and the style's TeX fallback should be kept.
    fn param(&self, _param: MathParam) -> Option<Width> {
        None
    }

    /// Whether the face has a glyph for `character`. Used to decide whether a
    /// variable may be set in the Mathematical Italic block (U+1D434…): a face
    /// without those glyphs keeps the upright letter rather than drawing
    /// `.notdef`, so the default is "no".
    fn has(&self, _character: char) -> bool {
        false
    }

    /// Number of vertical design variants of `text`'s first glyph. `0` when the
    /// host has no `MATH` variant data; callers then scale the text glyph
    /// (ADR-0075).
    fn vertical_variant_count(&self, _text: &str) -> u16 {
        0
    }

    /// One vertical design variant, measured at `size`.
    ///
    /// The ordinal is the font's storage order (increasing design size).
    /// Rendering resolves it in the actual font, so nothing here depends on
    /// glyph ids.
    fn vertical_variant(&self, _text: &str, _index: u16, _size: Width) -> Option<VerticalVariant> {
        None
    }

    /// Whether the *math* face (not the text face) has `character`. Accents
    /// use the combining form when the math face carries it (ADR-0076); the
    /// default trusts [`Self::has`], which is the union.
    fn math_has(&self, character: char) -> bool {
        self.has(character)
    }

    /// Top accent attachment of `text`'s first glyph at `size`.
    fn top_accent(&self, _text: &str, _size: Width) -> Option<Width> {
        None
    }

    /// Signed ink bottom of `text`'s first glyph at `size`; negative when the
    /// whole glyph is above its baseline. Accents need this to avoid floating
    /// by their own ink height.
    fn accent_bottom(&self, _text: &str, _size: Width) -> Option<Width> {
        None
    }
}

/// One vertical design glyph, measured at a concrete size.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct VerticalVariant {
    /// Horizontal advance (the cursor step).
    pub advance: Width,
    /// Ink above the baseline.
    pub above: Width,
    /// Ink below the baseline.
    pub below: Width,
    /// `advance - ink right`, never negative: a vinculum starts at the ink
    /// edge, not at the advance.
    pub right_gap: Width,
}

impl VerticalVariant {
    pub fn height(self) -> i32 {
        self.above.0 + self.below.0
    }
}

/// Ink band of the stretchy brace glyphs, in permille of the glyph's own
/// size, as distances *above* its baseline (negative means below). Measured
/// from Noto Sans Math: `U+23DE` (over-brace) occupies 0.656…0.893 em above
/// the baseline, `U+23DF` (under-brace) 0.075…0.310 em below it. The glyph's
/// font-wide ascent is more than twice its ink, so placing by ascent floats
/// the brace a full em above the body.
const OVER_BRACE_INK: (i32, i32) = (893, 656);
const UNDER_BRACE_INK: (i32, i32) = (-75, -310);

/// Above this sideways stretch the font's brace glyph stops looking like a
/// brace: its hooks become ramps. Wider bodies get a brace assembled from
/// rules instead (ADR-0056).
const BRACE_GLYPH_STRETCH: u16 = 2000;

/// `(ink_top, ink_bottom)` above the baseline for the two stretchy braces.
/// Public because both the layout and the renderers need to know where the
/// glyph's ink actually sits: its baseline is not a useful reference.
pub fn brace_ink(text: &str) -> Option<(i32, i32)> {
    match text.chars().next()? {
        '\u{23de}' => Some(OVER_BRACE_INK),
        '\u{23df}' => Some(UNDER_BRACE_INK),
        _ => None,
    }
}

fn per_mille(size: Width, permille: i32) -> i32 {
    (size.0 as i64 * i64::from(permille) / 1000) as i32
}

/// Integer square root by Newton's method: no float, same answer everywhere.
fn isqrt(value: i32) -> i32 {
    if value <= 0 {
        return 0;
    }
    let value = i64::from(value);
    let mut x = value;
    let mut y = (x + 1) / 2;
    while y < x {
        x = y;
        y = (x + value / x) / 2;
    }
    x as i32
}

/// Named colours accepted by `\color{...}`, plus `#rrggbb`.
fn named_color(name: &str) -> Option<u32> {
    let value = match name.trim() {
        "red" => 0xcc0000,
        "green" => 0x007700,
        "blue" => 0x0000cc,
        "black" => 0x000000,
        "white" => 0xffffff,
        "gray" | "grey" => 0x808080,
        "orange" => 0xd35400,
        other => u32::from_str_radix(other.strip_prefix('#')?, 16).ok()?,
    };
    (value <= 0xff_ffff).then_some(value)
}

/// The Mathematical Italic code point for an ASCII letter.
///
/// TeX sets variables in italic; the Unicode math alphabets carry those
/// shapes as separate characters, so no synthetic slant is involved.
fn italic_letter(character: char) -> Option<char> {
    let code = match character {
        'A'..='Z' => 0x1d434 + (character as u32 - 'A' as u32),
        'a'..='z' => 0x1d44e + (character as u32 - 'a' as u32),
        _ => return None,
    };
    char::from_u32(code)
}

/// Math layout constants. Scales are permille of the base size.
///
/// The defaults are TeX's Latin Modern values, so a host without a `MATH`
/// table still gets TeX geometry. [`Self::apply_params`] replaces them with
/// the face's own constants when the metrics source has them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MathStyle {
    pub size: Width,
    /// Display style: big-operator limits stack above and below. Inline style
    /// keeps them to the right, as TeX's text style does.
    pub display: bool,
    pub script_scale: u16,
    pub scriptscript_scale: u16,
    pub sup_shift: u16,
    pub sup_shift_cramped: u16,
    pub sub_shift: u16,
    pub sup_drop: u16,
    pub sub_drop: u16,
    pub sub_sup_gap: u16,
    pub sup_bottom_min: u16,
    pub sup_bottom_max_sub: u16,
    pub axis_height: u16,
    pub rule_thickness: u16,
    pub num_shift: u16,
    pub num_shift_display: u16,
    pub denom_shift: u16,
    pub denom_shift_display: u16,
    pub num_gap: u16,
    pub denom_gap: u16,
    pub limit_above_gap: u16,
    pub limit_above_rise: u16,
    pub limit_below_gap: u16,
    pub limit_below_drop: u16,
    pub radical_rule: u16,
    pub radical_extra: u16,
    /// Clearance between the radicand's ink top and the vinculum's bottom.
    /// Text style and display style have their own values in `MATH`.
    pub radical_gap: u16,
    pub radical_display_gap: u16,
    /// Fraction gap minima in display style; the text-style values are
    /// `num_gap`/`denom_gap`.
    pub num_display_gap: u16,
    pub denom_display_gap: u16,
    /// Cap on a subscript's ascent above the base's baseline (TeX rule 18).
    pub sub_top_max: u16,
    /// Signed: the fonts supply −450…−526‰ for the kern after the degree.
    pub radical_kern_before: i32,
    pub radical_kern_after: i32,
    pub radical_degree_raise: u16,
    pub display_operator_min: u16,
    pub overbar_gap: u16,
    pub overbar_rule: u16,
    pub overbar_extra: u16,
    pub underbar_gap: u16,
    pub underbar_rule: u16,
    pub underbar_extra: u16,
    pub accent_base_height: u16,
    /// Space between two rows of an `aligned`/`matrix` (`\jot`, 3pt).
    pub jot: u16,
    /// Space after a script before the next atom (`\scriptspace`).
    pub script_space: u16,
    /// Gap for stacked boxes (`\substack`, brace labels).
    pub stack_gap: u16,
    pub bin_gap: u16,
    pub rel_gap: u16,
    pub punct_gap: u16,
}

impl MathStyle {
    /// Replace the TeX fallback constants with the face's `MATH` values.
    /// Zero counts as "no value" for a length, and a zero percentage keeps
    /// the fallback.
    pub fn apply_params(&mut self, metrics: &dyn MathMetrics) {
        let set = |param: MathParam, slot: &mut u16, allow_zero: bool| {
            if let Some(value) = metrics.param(param) {
                let value = value.0.clamp(0, u16::MAX as i32) as u16;
                if allow_zero || value > 0 {
                    *slot = value;
                }
            }
        };
        set(MathParam::ScriptPercent, &mut self.script_scale, false);
        set(
            MathParam::ScriptScriptPercent,
            &mut self.scriptscript_scale,
            false,
        );
        set(MathParam::AxisHeight, &mut self.axis_height, true);
        set(MathParam::RuleThickness, &mut self.rule_thickness, false);
        set(MathParam::SupShift, &mut self.sup_shift, true);
        set(
            MathParam::SupShiftCramped,
            &mut self.sup_shift_cramped,
            true,
        );
        set(MathParam::SubShift, &mut self.sub_shift, true);
        set(MathParam::SupDrop, &mut self.sup_drop, true);
        set(MathParam::SubDrop, &mut self.sub_drop, true);
        set(MathParam::SubSupGap, &mut self.sub_sup_gap, true);
        set(MathParam::SupBottomMin, &mut self.sup_bottom_min, true);
        set(
            MathParam::SupBottomMaxWithSub,
            &mut self.sup_bottom_max_sub,
            true,
        );
        set(MathParam::NumShift, &mut self.num_shift, true);
        set(
            MathParam::NumShiftDisplay,
            &mut self.num_shift_display,
            true,
        );
        set(MathParam::DenomShift, &mut self.denom_shift, true);
        set(
            MathParam::DenomShiftDisplay,
            &mut self.denom_shift_display,
            true,
        );
        set(MathParam::NumGapMin, &mut self.num_gap, true);
        set(MathParam::DenomGapMin, &mut self.denom_gap, true);
        set(MathParam::LimitAboveGap, &mut self.limit_above_gap, true);
        set(MathParam::LimitAboveRise, &mut self.limit_above_rise, true);
        set(MathParam::LimitBelowGap, &mut self.limit_below_gap, true);
        set(MathParam::LimitBelowDrop, &mut self.limit_below_drop, true);
        set(MathParam::RadicalRule, &mut self.radical_rule, false);
        set(MathParam::RadicalExtra, &mut self.radical_extra, true);
        set(MathParam::RadicalGap, &mut self.radical_gap, true);
        set(
            MathParam::RadicalDisplayGap,
            &mut self.radical_display_gap,
            true,
        );
        set(MathParam::NumDisplayGap, &mut self.num_display_gap, true);
        set(
            MathParam::DenomDisplayGap,
            &mut self.denom_display_gap,
            true,
        );
        set(MathParam::SubTopMax, &mut self.sub_top_max, true);
        set(MathParam::SpaceAfterScript, &mut self.script_space, true);
        let set_signed = |param: MathParam, slot: &mut i32| {
            if let Some(value) = metrics.param(param) {
                *slot = value.0;
            }
        };
        set_signed(MathParam::RadicalKernBefore, &mut self.radical_kern_before);
        set_signed(MathParam::RadicalKernAfter, &mut self.radical_kern_after);
        set(
            MathParam::RadicalDegreeRaise,
            &mut self.radical_degree_raise,
            false,
        );
        set(
            MathParam::DisplayOperatorMin,
            &mut self.display_operator_min,
            false,
        );
        set(MathParam::OverbarGap, &mut self.overbar_gap, true);
        set(MathParam::OverbarRule, &mut self.overbar_rule, false);
        set(MathParam::OverbarExtra, &mut self.overbar_extra, true);
        set(MathParam::UnderbarGap, &mut self.underbar_gap, true);
        set(MathParam::UnderbarRule, &mut self.underbar_rule, false);
        set(MathParam::UnderbarExtra, &mut self.underbar_extra, true);
        set(
            MathParam::AccentBaseHeight,
            &mut self.accent_base_height,
            true,
        );
    }
}

impl Default for MathStyle {
    fn default() -> Self {
        Self {
            size: Width::EM,
            display: true,
            script_scale: 700,
            scriptscript_scale: 500,
            sup_shift: 363,
            sup_shift_cramped: 289,
            sub_shift: 247,
            sup_drop: 250,
            sub_drop: 200,
            sub_sup_gap: 160,
            sup_bottom_min: 108,
            sup_bottom_max_sub: 344,
            axis_height: 250,
            rule_thickness: 40,
            num_shift: 394,
            num_shift_display: 677,
            denom_shift: 345,
            denom_shift_display: 686,
            num_gap: 40,
            denom_gap: 120,
            limit_above_gap: 200,
            limit_above_rise: 111,
            limit_below_gap: 167,
            limit_below_drop: 600,
            radical_rule: 40,
            radical_extra: 40,
            radical_gap: 50,
            radical_display_gap: 100,
            num_display_gap: 100,
            denom_display_gap: 100,
            sub_top_max: 300,
            radical_kern_before: 278,
            radical_kern_after: 0,
            radical_degree_raise: 600,
            display_operator_min: 1300,
            overbar_gap: 120,
            overbar_rule: 40,
            overbar_extra: 40,
            underbar_gap: 120,
            underbar_rule: 40,
            underbar_extra: 40,
            accent_base_height: 450,
            jot: 300,
            script_space: 50,
            stack_gap: 120,
            bin_gap: 222,
            rel_gap: 278,
            punct_gap: 167,
        }
    }
}

/// One positioned glyph. `y` is relative to the math baseline; negative is up.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MathItem {
    pub text: String,
    pub x: Width,
    pub y: Width,
    /// Font size for drawing `text`. A renderer that cannot address an
    /// unencoded design glyph (SVG/HTML) uses this.
    pub size: Width,
    /// Font size for drawing the selected design glyph. Equal to `size` when
    /// `glyph` is [`MathGlyph::Text`].
    pub design: Width,
    /// Which glyph of the math font draws this item.
    pub glyph: MathGlyph,
    /// Horizontal scale in permille, for a glyph that has to span a body:
    /// an `\overbrace` is one brace glyph stretched sideways (1000 = as is).
    pub stretch: u16,
    /// Explicit colour, from `\color{...}`. `None` means "the style's colour".
    pub color: Option<u32>,
}

impl MathItem {
    /// A plain text run at `size`: the shape every constructor used before
    /// design variants existed (ADR-0075).
    pub fn text(text: impl Into<String>, x: Width, y: Width, size: Width) -> Self {
        Self {
            text: text.into(),
            x,
            y,
            size,
            design: size,
            glyph: MathGlyph::Text,
            stretch: 1000,
            color: None,
        }
    }

    /// The vertical design-variant ordinal, when this item selects one.
    pub fn variant(&self) -> Option<u16> {
        match self.glyph {
            MathGlyph::Text => None,
            MathGlyph::Vertical(index) => Some(index),
        }
    }
}

/// Which glyph draws a [`MathItem`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MathGlyph {
    /// The glyph the text's first character maps to.
    #[default]
    Text,
    /// The nth vertical design variant of that character, in font order.
    Vertical(u16),
}

/// One rule (a fraction bar) in the math box.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MathRule {
    pub x: Width,
    pub y: Width,
    pub width: Width,
    pub thickness: Width,
}

/// A laid-out formula.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MathLayout {
    pub width: Width,
    /// Extent above the baseline.
    pub height: Width,
    /// Extent below the baseline.
    pub depth: Width,
    pub items: Vec<MathItem>,
    pub rules: Vec<MathRule>,
    /// Rows of a top-level `aligned`/`align` block, relative to the formula
    /// baseline. Empty for a formula with no such block, which callers read as
    /// "one row at the baseline". Nested matrices — an `aligned` inside an
    /// `equation`, a `cases` inside an `aligned` — never add rows: only the
    /// outermost block owns them.
    pub rows: Vec<MathRow>,
    /// The formula's own number when it has no rows, from a `\tag` written
    /// directly in the body.
    pub number: RowNumber,
}

/// One numbered row of a display formula.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MathRow {
    /// Baseline, relative to the formula baseline.
    pub baseline: Width,
    pub number: RowNumber,
}

impl MathLayout {
    /// Total height of the box (height + depth).
    pub fn total_height(&self) -> Width {
        Width(self.height.0 + self.depth.0)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MathError {
    Empty,
    MissingArgument,
    UnsupportedCommand(String),
    UnbalancedGroup,
    UnbalancedDelimiter,
    UnbalancedEnvironment(String),
}

impl fmt::Display for MathError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => f.write_str("empty formula"),
            Self::MissingArgument => f.write_str("missing command argument"),
            Self::UnsupportedCommand(name) => write!(f, "unsupported math command \\{name}"),
            Self::UnbalancedGroup => f.write_str("unbalanced braces in formula"),
            Self::UnbalancedDelimiter => f.write_str("unbalanced \\left/\\right"),
            Self::UnbalancedEnvironment(name) => {
                write!(f, "unbalanced or unexpected \\end{{{name}}}")
            }
        }
    }
}

impl core::error::Error for MathError {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Class {
    Ord,
    /// A large operator or an upright function name (`\sum`, `\sin`).
    Op,
    Bin,
    Rel,
    Open,
    Close,
    Punct,
    /// A fraction, a binomial, or a `\left…\right` group.
    Inner,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Node {
    Symbol {
        text: String,
        class: Class,
    },
    /// A bare ASCII letter: a variable. Laid out italic when the face has the
    /// Mathematical Italic glyph for it, upright otherwise.
    Letter(char),
    Group(Vec<Node>),
    Fraction {
        numerator: Box<Node>,
        denominator: Box<Node>,
        /// `\binom` draws no rule; everything else is a fraction.
        rule: bool,
    },
    Script {
        base: Box<Node>,
        sup: Option<Box<Node>>,
        sub: Option<Box<Node>>,
    },
    Radical {
        /// `\sqrt[3]{x}` degree.
        index: Option<Box<Node>>,
        body: Box<Node>,
    },
    /// A large operator with optional limits.
    Big {
        text: &'static str,
        /// Limits stack above and below; integrals keep them to the right.
        stacked: bool,
        sup: Option<Box<Node>>,
        sub: Option<Box<Node>>,
    },
    /// `\left…\right`: delimiters stretched to the body.
    Delimited {
        /// Empty string for the null delimiter `.`.
        left: String,
        body: Box<Node>,
        right: String,
    },
    /// `\begin{matrix}` and friends.
    Matrix {
        kind: MatrixKind,
        /// `rows[row][column]`; every row is padded to the widest row.
        rows: Vec<Vec<Vec<Node>>>,
        /// Per-column alignment from `\begin{array}{lcr}`. Empty means "use the
        /// matrix kind's rule", which every other environment does.
        aligns: Vec<ColumnAlign>,
    },
    /// `\substack{a\\b}`: a stack of lines, used inside a limit.
    Substack(Vec<Vec<Node>>),
    /// `\overbrace{...}` / `\underbrace{...}`: one brace glyph stretched over
    /// the body.
    Brace {
        body: Box<Node>,
        over: bool,
    },
    /// Everything after `\color{...}`, until the end of its group.
    Color {
        color: u32,
        body: Vec<Node>,
    },
    /// `\tag{...}` / `\tag*{...}`: a hand-written label for this row.
    Tag {
        text: String,
        bare: bool,
    },
    /// `\notag` / `\nonumber`: this row takes no number.
    NoTag,
    /// `\hat`, `\bar`, `\vec`, `\tilde`, `\widehat`: a mark above the body.
    Accent {
        mark: &'static str,
        body: Box<Node>,
        wide: bool,
    },
    /// `\overline` / `\underline`: one rule over/under the body.
    Bar {
        body: Box<Node>,
        over: bool,
    },
    /// `\operatorname{...}` / `\operatorname*{...}`: upright text as an Op.
    OpName {
        text: String,
        limits: bool,
        sup: Option<Box<Node>>,
        sub: Option<Box<Node>>,
    },
    /// `\mathop{...}` and friends: force the spacing class.
    Classed {
        class: Class,
        body: Box<Node>,
    },
    /// `\stackrel{over}{base}`: a script-size label above a base.
    Stack {
        over: Box<Node>,
        base: Box<Node>,
    },
    /// `\displaystyle` and friends: the rest of the group in another style.
    Style {
        level: StyleLevel,
        body: Vec<Node>,
    },
    /// `\middle|` inside `\left…\right`: a delimiter at the group's size.
    Middle(String),
    Space(i32),
}

/// TeX's four math styles. A script inside a script stays at ScriptScript.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StyleLevel {
    Display,
    Text,
    Script,
    ScriptScript,
}

impl StyleLevel {
    fn script(self) -> Self {
        match self {
            Self::Display | Self::Text => Self::Script,
            Self::Script | Self::ScriptScript => Self::ScriptScript,
        }
    }

    fn is_script(self) -> bool {
        matches!(self, Self::Script | Self::ScriptScript)
    }
}

/// What a formula row does about its number.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub enum RowNumber {
    /// Take the template's equation number, if numbering is on.
    #[default]
    Auto,
    /// `\notag` / `\nonumber`.
    Suppressed,
    /// `\tag{...}`: print this text. `bare` (`\tag*`) drops the parentheses.
    Tag { text: String, bare: bool },
}

/// How a matrix is fenced and aligned.
/// Column alignment in an `array`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColumnAlign {
    Left,
    Center,
    Right,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MatrixKind {
    /// No fence, centres every column.
    Plain,
    Paren,
    Bracket,
    /// `\begin{vmatrix}`: vertical bars.
    Vert,
    /// `\begin{cases}`: one left brace, left-aligned columns.
    Cases,
    /// `\begin{aligned}`: columns alternate right/left around `&`.
    Aligned,
}

impl Node {
    /// The class of this node when it stands alone. A group takes the class of
    /// its first meaningful child; use [`Self::last_class`] for the right-hand
    /// side of a spacing pair.
    fn class(&self) -> Class {
        match self {
            Self::Symbol { class, .. } => *class,
            Self::Group(_) | Self::Style { .. } | Self::Color { .. } => self.first_class(),
            Self::Fraction { .. } => Class::Inner,
            Self::Script { base, .. } => base.last_class(),
            Self::Radical { .. } => Class::Ord,
            Self::Big { .. } => Class::Op,
            Self::Delimited { .. } => Class::Inner,
            Self::Matrix { .. } => Class::Ord,
            Self::Space(_) => Class::Ord,
            Self::Letter(_) => Class::Ord,
            Self::Substack(_) => Class::Ord,
            Self::Brace { .. } => Class::Ord,
            Self::Tag { .. } | Self::NoTag => Class::Ord,
            Self::Accent { .. } => Class::Ord,
            Self::Bar { .. } => Class::Ord,
            Self::OpName { .. } => Class::Op,
            Self::Classed { class, .. } => *class,
            Self::Stack { .. } => Class::Ord,
            Self::Middle(_) => Class::Ord,
        }
    }

    fn first_class(&self) -> Class {
        match self {
            Self::Group(nodes)
            | Self::Style { body: nodes, .. }
            | Self::Color { body: nodes, .. } => nodes
                .iter()
                .find(|node| {
                    !matches!(node, Self::Tag { .. } | Self::NoTag) && !node.is_empty_group()
                })
                .map(Node::first_class)
                .unwrap_or(Class::Ord),
            _ => self.class(),
        }
    }

    fn last_class(&self) -> Class {
        match self {
            Self::Group(nodes)
            | Self::Style { body: nodes, .. }
            | Self::Color { body: nodes, .. } => nodes
                .iter()
                .rev()
                .find(|node| {
                    !matches!(node, Self::Tag { .. } | Self::NoTag) && !node.is_empty_group()
                })
                .map(Node::last_class)
                .unwrap_or(Class::Ord),
            _ => self.class(),
        }
    }

    fn is_empty_group(&self) -> bool {
        matches!(self, Self::Group(nodes) if nodes.is_empty())
    }
}

/// Parse a formula into a list of nodes.
pub fn parse(source: &str) -> Result<Vec<Node>, MathError> {
    let mut parser = Parser {
        chars: source.chars().collect(),
        pos: 0,
    };
    let nodes = parser.parse_list(false)?;
    if nodes.is_empty() {
        return Err(MathError::Empty);
    }
    Ok(nodes)
}

struct Parser {
    chars: Vec<char>,
    pos: usize,
}

impl Parser {
    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn bump(&mut self) -> Option<char> {
        let character = self.peek()?;
        self.pos += 1;
        Some(character)
    }

    fn skip_spaces(&mut self) {
        while let Some(character) = self.peek() {
            if character.is_whitespace() {
                self.pos += 1;
            } else {
                break;
            }
        }
    }

    fn parse_list(&mut self, in_group: bool) -> Result<Vec<Node>, MathError> {
        let mut nodes = Vec::new();
        loop {
            self.skip_spaces();
            match self.peek() {
                None => {
                    if in_group {
                        return Err(MathError::UnbalancedGroup);
                    }
                    break;
                }
                Some('}') => {
                    if in_group {
                        self.pos += 1;
                        break;
                    }
                    return Err(MathError::UnbalancedGroup);
                }
                Some('^') | Some('_') => {
                    let marker = self.bump().expect("peeked");
                    let argument = self.parse_argument()?;
                    let base = nodes.pop().unwrap_or(Node::Group(Vec::new()));
                    // A big operator takes its limits directly: stacking keeps
                    // `\sum_{i}^{n}` above and below the operator.
                    if let Node::Big {
                        text,
                        stacked,
                        sup,
                        sub,
                    } = base
                    {
                        let (sup, sub) = if marker == '^' {
                            (Some(Box::new(argument)), sub)
                        } else {
                            (sup, Some(Box::new(argument)))
                        };
                        nodes.push(Node::Big {
                            text,
                            stacked,
                            sup,
                            sub,
                        });
                        continue;
                    }
                    let (base, mut sup, mut sub) = match base {
                        Node::Script { base, sup, sub } => (base, sup, sub),
                        Node::OpName {
                            text,
                            limits,
                            sup,
                            sub,
                        } => {
                            let (sup, sub) = if marker == '^' {
                                (Some(Box::new(argument)), sub)
                            } else {
                                (sup, Some(Box::new(argument)))
                            };
                            nodes.push(Node::OpName {
                                text,
                                limits,
                                sup,
                                sub,
                            });
                            continue;
                        }
                        other => (Box::new(other), None, None),
                    };
                    if marker == '^' {
                        sup = Some(Box::new(argument));
                    } else {
                        sub = Some(Box::new(argument));
                    }
                    nodes.push(Node::Script { base, sup, sub });
                }
                Some('\\') => {
                    self.pos += 1;
                    let command = self.read_command();
                    match command.as_str() {
                        "frac" => {
                            let numerator = self.parse_group()?;
                            let denominator = self.parse_group()?;
                            nodes.push(Node::Fraction {
                                numerator: Box::new(numerator),
                                denominator: Box::new(denominator),
                                rule: true,
                            });
                        }
                        "binom" => {
                            let numerator = self.parse_group()?;
                            let denominator = self.parse_group()?;
                            nodes.push(Node::Delimited {
                                left: String::from("("),
                                body: Box::new(Node::Fraction {
                                    numerator: Box::new(numerator),
                                    denominator: Box::new(denominator),
                                    rule: false,
                                }),
                                right: String::from(")"),
                            });
                        }
                        "dfrac" | "cfrac" | "tfrac" => {
                            let numerator = self.parse_group()?;
                            let denominator = self.parse_group()?;
                            let level = if command == "tfrac" {
                                StyleLevel::Text
                            } else {
                                StyleLevel::Display
                            };
                            nodes.push(Node::Style {
                                level,
                                body: alloc::vec![Node::Fraction {
                                    numerator: Box::new(numerator),
                                    denominator: Box::new(denominator),
                                    rule: true,
                                }],
                            });
                        }
                        "genfrac" => {
                            let left = self.read_raw_group()?;
                            let right = self.read_raw_group()?;
                            let thickness = self.read_raw_group()?;
                            let _style = self.read_raw_group()?;
                            let numerator = self.parse_group()?;
                            let denominator = self.parse_group()?;
                            let rule = !thickness.trim().is_empty() && thickness.trim() != "0";
                            let fraction = Node::Fraction {
                                numerator: Box::new(numerator),
                                denominator: Box::new(denominator),
                                rule,
                            };
                            if left.trim().is_empty() && right.trim().is_empty() {
                                nodes.push(fraction);
                            } else {
                                nodes.push(Node::Delimited {
                                    left: if left.trim().is_empty() {
                                        String::new()
                                    } else {
                                        left.trim().to_string()
                                    },
                                    body: Box::new(fraction),
                                    right: if right.trim().is_empty() {
                                        String::new()
                                    } else {
                                        right.trim().to_string()
                                    },
                                });
                            }
                        }
                        "sqrt" => {
                            let index = self.parse_optional_degree()?;
                            let body = self.parse_group()?;
                            nodes.push(Node::Radical {
                                index,
                                body: Box::new(body),
                            });
                        }
                        "left" => {
                            let left = self.read_delimiter_text()?;
                            let (body, right) = self.read_until_right()?;
                            let body = parse_body(&body)?;
                            nodes.push(Node::Delimited {
                                left,
                                body: Box::new(Node::Group(body)),
                                right,
                            });
                        }
                        "right" => {
                            // A `\right` without `\left`: keep the delimiter so
                            // the source is not silently dropped.
                            let text = self.read_delimiter_text()?;
                            if !text.is_empty() {
                                nodes.push(Node::Symbol {
                                    class: class_of(&text),
                                    text,
                                });
                            }
                        }
                        "begin" => {
                            let name = self.read_raw_group()?;
                            // `array` takes a column format before its body.
                            let mut aligns = Vec::new();
                            if name == "array" {
                                let format = self.read_raw_group()?;
                                for spec in format.chars() {
                                    aligns.push(match spec {
                                        'l' => ColumnAlign::Left,
                                        'r' => ColumnAlign::Right,
                                        _ => ColumnAlign::Center,
                                    });
                                }
                            }
                            let kind = match name.as_str() {
                                "matrix" => MatrixKind::Plain,
                                "pmatrix" => MatrixKind::Paren,
                                "bmatrix" => MatrixKind::Bracket,
                                "vmatrix" => MatrixKind::Vert,
                                "cases" => MatrixKind::Cases,
                                "aligned" | "align" | "align*" | "split" | "equation"
                                | "equation*" | "gather" | "gather*" | "multline" | "multline*" => {
                                    MatrixKind::Aligned
                                }
                                "array" => MatrixKind::Plain,
                                other => {
                                    return Err(MathError::UnsupportedCommand(format!(
                                        "begin{{{other}}}"
                                    )))
                                }
                            };
                            let rows = self.read_environment(&name)?;
                            nodes.push(Node::Matrix { kind, rows, aligns });
                        }
                        "end" => {
                            let name = self.read_raw_group()?;
                            return Err(MathError::UnbalancedEnvironment(name));
                        }
                        "sum" | "prod" | "int" | "oint" | "lim" | "sup" | "inf" => {
                            let (text, stacked) = match command.as_str() {
                                "sum" => ("∑", true),
                                "prod" => ("∏", true),
                                "oint" => ("∮", false),
                                "int" => ("∫", false),
                                "lim" => ("lim", true),
                                "sup" => ("sup", true),
                                _ => ("inf", true),
                            };
                            nodes.push(Node::Big {
                                text,
                                stacked,
                                sup: None,
                                sub: None,
                            });
                        }
                        "log" | "ln" | "exp" | "sin" | "cos" | "tan" | "max" | "min" | "det"
                        | "gcd" | "deg" | "arg" | "ker" | "hom" => {
                            nodes.push(Node::Symbol {
                                text: command,
                                class: Class::Op,
                            });
                        }
                        "operatorname" => {
                            self.skip_spaces();
                            let star = if self.peek() == Some('*') {
                                self.pos += 1;
                                true
                            } else {
                                false
                            };
                            let text = self.read_raw_group()?;
                            nodes.push(Node::OpName {
                                text,
                                limits: star,
                                sup: None,
                                sub: None,
                            });
                        }
                        "limits" | "nolimits" => {
                            let want = command == "limits";
                            if let Some(last) = nodes.last_mut() {
                                match last {
                                    Node::Big { stacked, .. } => *stacked = want,
                                    Node::OpName { limits, .. } => *limits = want,
                                    _ => {}
                                }
                            }
                        }
                        "mathop" | "mathbin" | "mathrel" | "mathopen" | "mathclose"
                        | "mathpunct" | "mathinner" | "mathord" => {
                            let class = match command.as_str() {
                                "mathop" => Class::Op,
                                "mathbin" => Class::Bin,
                                "mathrel" => Class::Rel,
                                "mathopen" => Class::Open,
                                "mathclose" => Class::Close,
                                "mathpunct" => Class::Punct,
                                "mathinner" => Class::Inner,
                                _ => Class::Ord,
                            };
                            let body = self.parse_group()?;
                            nodes.push(Node::Classed {
                                class,
                                body: Box::new(body),
                            });
                        }
                        "hat" | "widehat" | "bar" | "vec" | "tilde" => {
                            let body = self.parse_group()?;
                            let (mark, wide) = match command.as_str() {
                                "hat" => ("\u{2c6}", false),
                                "widehat" => ("\u{2c6}", true),
                                "bar" => ("\u{af}", false),
                                "vec" => ("\u{20d7}", false),
                                _ => ("\u{2dc}", false),
                            };
                            nodes.push(Node::Accent {
                                mark,
                                body: Box::new(body),
                                wide,
                            });
                        }
                        "overline" | "underline" => {
                            let body = self.parse_group()?;
                            nodes.push(Node::Bar {
                                body: Box::new(body),
                                over: command == "overline",
                            });
                        }
                        "stackrel" => {
                            let over = self.parse_group()?;
                            let base = self.parse_group()?;
                            nodes.push(Node::Stack {
                                over: Box::new(over),
                                base: Box::new(base),
                            });
                        }
                        "displaystyle" | "textstyle" | "scriptstyle" | "scriptscriptstyle" => {
                            let level = match command.as_str() {
                                "displaystyle" => StyleLevel::Display,
                                "textstyle" => StyleLevel::Text,
                                "scriptstyle" => StyleLevel::Script,
                                _ => StyleLevel::ScriptScript,
                            };
                            let body = self.parse_list(in_group)?;
                            nodes.push(Node::Style { level, body });
                            return Ok(nodes);
                        }
                        "middle" => {
                            let text = self.read_delimiter_text()?;
                            if !text.is_empty() {
                                nodes.push(Node::Middle(text));
                            }
                        }
                        "," => nodes.push(Node::Space(167)),
                        ":" => nodes.push(Node::Space(222)),
                        ";" => nodes.push(Node::Space(278)),
                        "!" => nodes.push(Node::Space(-167)),
                        "thinspace" => nodes.push(Node::Space(167)),
                        "negthinspace" => nodes.push(Node::Space(-167)),
                        "quad" => nodes.push(Node::Space(1000)),
                        "qquad" => nodes.push(Node::Space(2000)),
                        "overbrace" | "underbrace" => {
                            let body = self.parse_group()?;
                            nodes.push(Node::Brace {
                                body: Box::new(body),
                                over: command == "overbrace",
                            });
                        }
                        "color" => {
                            // TeX applies `\color` to the rest of its group;
                            // so does this: parse the remainder and wrap it.
                            let name = self.read_raw_group()?;
                            let color = named_color(&name).ok_or_else(|| {
                                MathError::UnsupportedCommand(format!("color{{{name}}}"))
                            })?;
                            let body = self.parse_list(in_group)?;
                            nodes.push(Node::Color { color, body });
                            return Ok(nodes);
                        }
                        "substack" => {
                            let raw = self.read_raw_group()?;
                            let mut rows = Vec::new();
                            for line in raw.split("\\\\") {
                                rows.push(parse_body(line)?);
                            }
                            nodes.push(Node::Substack(rows));
                        }
                        "text" => {
                            // Text mode still recognises `$...$`: a phrase like
                            // `\text{if $x>0$}` mixes upright words with
                            // variables set the way math sets them.
                            let raw = self.read_raw_group()?;
                            nodes.extend(text_mode(&raw)?);
                        }
                        "tag" => {
                            // `\tag*{...}` drops the parentheses; the star is a
                            // character, not part of the command name.
                            self.skip_spaces();
                            let bare = if self.peek() == Some('*') {
                                self.pos += 1;
                                true
                            } else {
                                false
                            };
                            let text = self.read_raw_group()?;
                            if text.trim().is_empty() {
                                return Err(MathError::MissingArgument);
                            }
                            nodes.push(Node::Tag {
                                text: text.trim().to_string(),
                                bare,
                            });
                        }
                        "notag" | "nonumber" => nodes.push(Node::NoTag),
                        _ => {
                            if let Some(text) = symbol(&command) {
                                nodes.push(Node::Symbol {
                                    text: text.to_string(),
                                    class: class_of(text),
                                });
                            } else {
                                return Err(MathError::UnsupportedCommand(command));
                            }
                        }
                    }
                }
                Some('{') => {
                    let group = self.parse_group()?;
                    nodes.push(group);
                }
                Some(character) => {
                    self.pos += 1;
                    // A bare letter is a variable; everything else is a symbol.
                    if character.is_ascii_alphabetic() {
                        nodes.push(Node::Letter(character));
                    } else {
                        let text = character.to_string();
                        nodes.push(Node::Symbol {
                            class: class_of(&text),
                            text,
                        });
                    }
                }
            }
        }
        Ok(nodes)
    }

    /// Optional `[n]` degree of a radical.
    fn parse_optional_degree(&mut self) -> Result<Option<Box<Node>>, MathError> {
        self.skip_spaces();
        if self.peek() != Some('[') {
            return Ok(None);
        }
        self.pos += 1;
        let mut nodes = Vec::new();
        loop {
            self.skip_spaces();
            match self.bump() {
                None => return Err(MathError::UnbalancedGroup),
                Some(']') => break,
                Some('{') => nodes.push(self.parse_group()?),
                Some(character) => {
                    if character.is_ascii_alphabetic() {
                        nodes.push(Node::Letter(character));
                    } else {
                        let text = character.to_string();
                        nodes.push(Node::Symbol {
                            class: class_of(&text),
                            text,
                        });
                    }
                }
            }
        }
        Ok(Some(Box::new(Node::Group(nodes))))
    }

    /// The delimiter after `\left` / `\right`. A `.` means "no delimiter".
    fn read_delimiter_text(&mut self) -> Result<String, MathError> {
        self.skip_spaces();
        let character = self.bump().ok_or(MathError::MissingArgument)?;
        if character == '.' {
            return Ok(String::new());
        }
        if character == '\\' {
            let command = self.read_command();
            let text = match command.as_str() {
                "{" => "{",
                "}" => "}",
                "|" => "‖",
                "langle" => "⟨",
                "rangle" => "⟩",
                "lfloor" => "⌊",
                "rfloor" => "⌋",
                "lceil" => "⌈",
                "rceil" => "⌉",
                _ => return Err(MathError::UnsupportedCommand(command)),
            };
            return Ok(text.to_string());
        }
        Ok(character.to_string())
    }

    /// Scan to the matching `\right` and return `(body, right delimiter)`.
    fn read_until_right(&mut self) -> Result<(String, String), MathError> {
        let chars = &self.chars;
        let mut depth = 0usize;
        let mut index = self.pos;
        let mut found = None;
        while index < chars.len() {
            if chars[index] != '\\' {
                index += 1;
                continue;
            }
            let (command, next) = read_command_at(chars, index);
            match command.as_str() {
                "left" => depth += 1,
                "right" if depth == 0 => {
                    found = Some((index, next));
                    break;
                }
                "right" => depth -= 1,
                _ => {}
            }
            index = next;
        }
        let Some((right_start, after_right)) = found else {
            return Err(MathError::UnbalancedDelimiter);
        };
        let body: String = chars[self.pos..right_start].iter().collect();
        self.pos = after_right;
        let right = self.read_delimiter_text()?;
        Ok((body, right))
    }

    /// Read `\begin{name}…\end{name}` into rows of cells.
    fn read_environment(&mut self, name: &str) -> Result<Vec<Vec<Vec<Node>>>, MathError> {
        let chars = &self.chars;
        let mut depth = 0usize;
        let mut index = self.pos;
        let mut found = None;
        while index < chars.len() {
            if chars[index] != '\\' {
                index += 1;
                continue;
            }
            let (command, next) = read_command_at(chars, index);
            match command.as_str() {
                "begin" => depth += 1,
                "end" if depth == 0 => {
                    found = Some((index, next));
                    break;
                }
                "end" => depth -= 1,
                _ => {}
            }
            index = next;
        }
        let Some((end_start, after_end)) = found else {
            return Err(MathError::UnbalancedEnvironment(name.to_string()));
        };
        let body: String = chars[self.pos..end_start].iter().collect();

        // `\end{name}`: the environment must close with the same name.
        self.pos = after_end;
        self.skip_spaces();
        if self.peek() != Some('{') {
            return Err(MathError::MissingArgument);
        }
        let close = self.read_raw_group()?;
        if close != name {
            return Err(MathError::UnbalancedEnvironment(close));
        }

        split_matrix(&body)
            .into_iter()
            .map(|row| {
                row.into_iter()
                    .map(|cell| parse_body(&cell))
                    .collect::<Result<Vec<_>, _>>()
            })
            .collect()
    }

    fn parse_group(&mut self) -> Result<Node, MathError> {
        self.skip_spaces();
        if self.peek() != Some('{') {
            return Err(MathError::MissingArgument);
        }
        self.pos += 1;
        let nodes = self.parse_list(true)?;
        Ok(Node::Group(nodes))
    }

    fn parse_argument(&mut self) -> Result<Node, MathError> {
        self.skip_spaces();
        match self.peek() {
            Some('{') => self.parse_group(),
            Some('\\') => {
                self.pos += 1;
                let command = self.read_command();
                if let Some(text) = symbol(&command) {
                    return Ok(Node::Symbol {
                        text: text.to_string(),
                        class: class_of(text),
                    });
                }
                if matches!(
                    command.as_str(),
                    "log"
                        | "ln"
                        | "exp"
                        | "sin"
                        | "cos"
                        | "tan"
                        | "max"
                        | "min"
                        | "det"
                        | "gcd"
                        | "deg"
                        | "arg"
                        | "ker"
                        | "hom"
                ) {
                    return Ok(Node::Symbol {
                        text: command,
                        class: Class::Op,
                    });
                }
                Err(MathError::UnsupportedCommand(command))
            }
            Some(_) => {
                let character = self.bump().expect("peeked");
                Ok(Node::Symbol {
                    text: character.to_string(),
                    class: class_of(&character.to_string()),
                })
            }
            None => Err(MathError::MissingArgument),
        }
    }

    fn read_command(&mut self) -> String {
        let mut name = String::new();
        if let Some(character) = self.peek() {
            if !character.is_ascii_alphabetic() {
                self.pos += 1;
                return character.to_string();
            }
        }
        while let Some(character) = self.peek() {
            if character.is_ascii_alphabetic() {
                name.push(character);
                self.pos += 1;
            } else {
                break;
            }
        }
        name
    }

    fn read_raw_group(&mut self) -> Result<String, MathError> {
        self.skip_spaces();
        if self.peek() != Some('{') {
            return Err(MathError::MissingArgument);
        }
        self.pos += 1;
        let mut out = String::new();
        let mut depth = 1usize;
        while let Some(character) = self.bump() {
            match character {
                '{' => {
                    depth += 1;
                    out.push(character);
                }
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        return Ok(out);
                    }
                    out.push(character);
                }
                _ => out.push(character),
            }
        }
        Err(MathError::UnbalancedGroup)
    }
}

/// Read one command at `index` (which points at a backslash), without a
/// `Parser`. Returns `(name, index after the name)`.
fn read_command_at(chars: &[char], index: usize) -> (String, usize) {
    let mut cursor = index + 1;
    if cursor >= chars.len() {
        return (String::new(), cursor);
    }
    if !chars[cursor].is_ascii_alphabetic() {
        return (chars[cursor].to_string(), cursor + 1);
    }
    let mut name = String::new();
    while cursor < chars.len() && chars[cursor].is_ascii_alphabetic() {
        name.push(chars[cursor]);
        cursor += 1;
    }
    (name, cursor)
}

/// Parse a fragment that may be empty (a matrix cell).
fn parse_body(source: &str) -> Result<Vec<Node>, MathError> {
    let mut parser = Parser {
        chars: source.chars().collect(),
        pos: 0,
    };
    parser.parse_list(false)
}

/// Compile the body of `\text{...}`: literal runs stay text, `$...$` splices in
/// math nodes at the same size. `\$` is a literal dollar sign.
fn text_mode(source: &str) -> Result<Vec<Node>, MathError> {
    let mut nodes = Vec::new();
    let mut run = String::new();
    let mut chars = source.chars().peekable();
    while let Some(character) = chars.next() {
        match character {
            '\\' if chars.peek() == Some(&'$') => {
                chars.next();
                run.push('$');
            }
            '$' => {
                if !run.is_empty() {
                    nodes.push(Node::Symbol {
                        text: core::mem::take(&mut run),
                        class: Class::Ord,
                    });
                }
                let mut inner = String::new();
                for character in chars.by_ref() {
                    if character == '$' {
                        break;
                    }
                    inner.push(character);
                }
                if !inner.trim().is_empty() {
                    nodes.extend(parse_body(&inner)?);
                }
            }
            other => run.push(other),
        }
    }
    if !run.is_empty() {
        nodes.push(Node::Symbol {
            text: run,
            class: Class::Ord,
        });
    }
    Ok(nodes)
}

/// Split a matrix body into rows and cells on top-level `\\` and `&`.
///
/// Nested environments and braces are respected, so a nested `matrix` or a
/// `\text{a & b}` never splits its parent.
fn split_matrix(body: &str) -> Vec<Vec<String>> {
    let chars: Vec<char> = body.chars().collect();
    let mut rows: Vec<Vec<String>> = Vec::new();
    let mut row: Vec<String> = Vec::new();
    let mut cell_start = 0usize;
    let mut depth = 0usize;
    let mut brace = 0i32;
    let mut index = 0usize;
    while index < chars.len() {
        match chars[index] {
            '{' => {
                brace += 1;
                index += 1;
            }
            '}' => {
                brace -= 1;
                index += 1;
            }
            '&' if brace == 0 && depth == 0 => {
                row.push(chars[cell_start..index].iter().collect());
                index += 1;
                cell_start = index;
            }
            '\\' => {
                let (command, next) = read_command_at(&chars, index);
                match command.as_str() {
                    "begin" => {
                        depth += 1;
                        index = next;
                    }
                    "end" => {
                        depth = depth.saturating_sub(1);
                        index = next;
                    }
                    "\\" if depth == 0 && brace == 0 => {
                        row.push(chars[cell_start..index].iter().collect());
                        rows.push(core::mem::take(&mut row));
                        index = next;
                        cell_start = index;
                    }
                    _ => index = next,
                }
            }
            _ => index += 1,
        }
    }
    row.push(chars[cell_start..].iter().collect());
    // A trailing `\\` leaves one empty row: drop it.
    if row.len() == 1 && row[0].trim().is_empty() && !rows.is_empty() {
        return rows;
    }
    rows.push(row);
    rows
}

fn class_of(text: &str) -> Class {
    match text {
        "+" | "-" | "−" | "±" | "×" | "·" | "/" => Class::Bin,
        "=" | "<" | ">" | "≤" | "≥" | "≠" | "→" | "≈" => Class::Rel,
        "(" | "[" | "{" => Class::Open,
        ")" | "]" | "}" => Class::Close,
        "," | ";" => Class::Punct,
        _ => Class::Ord,
    }
}

fn symbol(command: &str) -> Option<&'static str> {
    Some(match command {
        "cdot" => "·",
        "times" => "×",
        "div" => "÷",
        "pm" => "±",
        "mp" => "∓",
        "le" | "leq" => "≤",
        "ge" | "geq" => "≥",
        "ne" | "neq" => "≠",
        "equiv" => "≡",
        "approx" => "≈",
        "sim" => "∼",
        "propto" => "∝",
        "to" | "rightarrow" => "→",
        "leftarrow" => "←",
        "leftrightarrow" => "↔",
        "Rightarrow" => "⇒",
        "Leftarrow" => "⇐",
        "Leftrightarrow" => "⇔",
        "in" => "∈",
        "notin" => "∉",
        "subset" => "⊂",
        "subseteq" => "⊆",
        "supset" => "⊃",
        "supseteq" => "⊇",
        "cup" => "∪",
        "cap" => "∩",
        "emptyset" => "∅",
        "forall" => "∀",
        "exists" => "∃",
        "partial" => "∂",
        "nabla" => "∇",
        "infty" => "∞",
        "prime" => "′",
        "cdots" => "⋯",
        "ldots" | "dots" => "…",
        "degree" => "°",
        "angle" => "∠",
        "perp" => "⊥",
        "parallel" => "∥",
        "alpha" => "α",
        "beta" => "β",
        "gamma" => "γ",
        "delta" => "δ",
        "epsilon" => "ε",
        "zeta" => "ζ",
        "eta" => "η",
        "theta" => "θ",
        "iota" => "ι",
        "kappa" => "κ",
        "lambda" => "λ",
        "mu" => "μ",
        "nu" => "ν",
        "xi" => "ξ",
        "pi" => "π",
        "rho" => "ρ",
        "sigma" => "σ",
        "tau" => "τ",
        "upsilon" => "υ",
        "phi" => "φ",
        "chi" => "χ",
        "psi" => "ψ",
        "omega" => "ω",
        "Gamma" => "Γ",
        "Delta" => "Δ",
        "Theta" => "Θ",
        "Lambda" => "Λ",
        "Xi" => "Ξ",
        "Pi" => "Π",
        "Sigma" => "Σ",
        "Phi" => "Φ",
        "Psi" => "Ψ",
        "Omega" => "Ω",
        "{" => "{",
        "}" => "}",
        "|" => "‖",
        _ => return None,
    })
}

/// Lay out a formula at the base size from `style`.
pub fn layout(
    source: &str,
    style: &MathStyle,
    metrics: &dyn MathMetrics,
) -> Result<MathLayout, MathError> {
    let nodes = parse(source)?;
    Ok(lay_out(&nodes, style.size, style, metrics))
}

/// One piece of an inline formula that may sit on its own line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Segment {
    pub nodes: Vec<Node>,
    /// Space before this piece, already scaled to the formula's base size.
    /// The first piece has zero.
    pub gap: Width,
    /// Cost of breaking before this piece (`\relpenalty` / `\binoppenalty`).
    pub penalty: i32,
}

/// Split a formula at the relations and binaries TeX allows a line break at.
///
/// The split points are the boundaries *before* a Rel or a Bin (after Bin
/// normalisation), so a broken line starts with the operator. The spacing that
/// the boundary removes from the line is returned so the caller can put it back
/// as inter-atom glue.
pub fn segments(source: &str, style: &MathStyle) -> Result<Vec<Segment>, MathError> {
    let nodes = parse(source)?;
    let (lefts, rights) = effective_classes(&nodes);
    let mut starts = alloc::vec![0usize];
    for index in 1..nodes.len() {
        if matches!(rights[index], Class::Rel | Class::Bin)
            && !matches!(lefts[index - 1], Class::Open | Class::Punct)
        {
            starts.push(index);
        }
    }
    let mut out = Vec::with_capacity(starts.len());
    for (part, &start) in starts.iter().enumerate() {
        let end = starts.get(part + 1).copied().unwrap_or(nodes.len());
        let (gap, penalty) = if part == 0 {
            (Width::ZERO, 0)
        } else {
            let gap = gap(
                lefts[start - 1],
                rights[start],
                StyleLevel::Text,
                style,
                style.size,
            );
            let penalty = if rights[start] == Class::Rel {
                500
            } else {
                700
            };
            (Width(gap), penalty)
        };
        out.push(Segment {
            nodes: nodes[start..end].to_vec(),
            gap,
            penalty,
        });
    }
    Ok(out)
}

/// Lay out a parsed node list at `size`.
pub fn lay_out(
    nodes: &[Node],
    size: Width,
    style: &MathStyle,
    metrics: &dyn MathMetrics,
) -> MathLayout {
    let mut context = Context {
        style,
        metrics,
        items: Vec::new(),
        rules: Vec::new(),
        rows: Vec::new(),
        number: RowNumber::Auto,
        matrix_depth: 0,
        cursor: 0,
        base_size: style.size,
        level: if style.display {
            StyleLevel::Display
        } else {
            StyleLevel::Text
        },
        cramped: false,
        fence_size: None,
    };
    let (lefts, rights) = effective_classes(nodes);
    for (index, node) in nodes.iter().enumerate() {
        if index > 0 {
            context.cursor += gap(lefts[index - 1], rights[index], context.level, style, size);
        }
        context.node(node, size, 0);
    }
    let items = context.items;
    let rules = context.rules;
    let mut height = 0i32;
    let mut depth = 0i32;
    for item in &items {
        let (above, below) = item_ink(metrics, item);
        height = height.max(-item.y.0 + above);
        depth = depth.max(item.y.0 + below);
    }
    for rule in &rules {
        height = height.max(-rule.y.0);
        depth = depth.max(rule.y.0 + rule.thickness.0);
    }
    MathLayout {
        width: Width(context.cursor),
        height: Width(height.max(1)),
        depth: Width(depth.max(0)),
        items,
        rules,
        rows: context.rows,
        number: context.number,
    }
}

/// A design glyph chosen for a body, with the fallback size a text renderer
/// should use when it cannot address the unencoded glyph (ADR-0075).
#[derive(Clone, Copy, Debug)]
struct Design {
    glyph: MathGlyph,
    /// Size at which the design glyph is drawn.
    design: Width,
    /// Size at which the text glyph approximates the design glyph's height.
    fallback: Width,
    advance: i32,
    above: i32,
    below: i32,
    right_gap: i32,
}

struct Context<'a> {
    style: &'a MathStyle,
    metrics: &'a dyn MathMetrics,
    items: Vec<MathItem>,
    rules: Vec<MathRule>,
    /// Aligned row baselines, filled by the outermost `matrix`.
    rows: Vec<MathRow>,
    /// The formula's own number, set by a top-level `\tag`/`\notag`.
    number: RowNumber,
    /// How many `matrix` bodies enclose the node being laid out. Only the
    /// outermost one contributes rows, so an `aligned` nested in an `equation`
    /// does not turn one numbered formula into several.
    matrix_depth: u32,
    cursor: i32,
    /// The formula's base size; script sizes are percentages of this, never of
    /// the enclosing script's size.
    base_size: Width,
    /// Current TeX style.
    level: StyleLevel,
    /// Cramped variant: the denominator of a fraction, and anything inside it.
    cramped: bool,
    /// Size chosen by an enclosing `\left…\right`, for `\middle`: the needed
    /// height and the body's ink box, so a middle fence selects the same class
    /// of variant.
    fence_size: Option<(i32, i32, i32)>,
}

impl Context<'_> {
    /// The design glyph that covers `needed` vertical units, or `None` when the
    /// font has no variants (the caller then scales the text glyph, as before).
    ///
    /// TeX asks for the smallest variant whose height reaches the target and
    /// falls back to the largest one it has, never to an enlarged base glyph.
    fn design_variant(&self, text: &str, needed: i32, size: Width) -> Option<Design> {
        let count = self.metrics.vertical_variant_count(text);
        if count == 0 {
            return None;
        }
        let mut chosen: Option<(u16, VerticalVariant)> = None;
        for index in 0..count {
            let Some(variant) = self.metrics.vertical_variant(text, index, size) else {
                continue;
            };
            chosen = Some((index, variant));
            if variant.height() >= needed {
                break;
            }
        }
        let (index, variant) = chosen?;
        let (base_above, base_below) = self
            .metrics
            .ink(text, size)
            .map(|(above, below)| (above.0, below.0))
            .unwrap_or((self.metrics.ascent(size).0, self.metrics.descent(size).0));
        let base_height = (base_above + base_below).max(1);
        let ratio = ((variant.height() as i64 * 1000 + base_height as i64 / 2) / base_height as i64)
            .clamp(1000, 4000) as i32;
        let fallback = Width(scale(size, ratio));
        Some(Design {
            glyph: MathGlyph::Vertical(index),
            design: size,
            fallback,
            advance: variant.advance.0,
            above: variant.above.0,
            below: variant.below.0,
            right_gap: variant.right_gap.0,
        })
    }

    fn node(&mut self, node: &Node, size: Width, baseline: i32) {
        match node {
            Node::Symbol { text, .. } => {
                self.items.push(MathItem::text(
                    text.clone(),
                    Width(self.cursor),
                    Width(baseline),
                    size,
                ));
                self.cursor += self.metrics.advance(text, size).0;
            }
            Node::Letter(character) => {
                // A variable is italic when the face carries the italic letter;
                // otherwise it stays upright, which is ugly but honest — the
                // alternative is `.notdef` in the middle of a formula.
                let text = self.letter_text(*character);
                self.items.push(MathItem::text(
                    text.clone(),
                    Width(self.cursor),
                    Width(baseline),
                    size,
                ));
                self.cursor += self.metrics.advance(&text, size).0;
            }
            Node::Group(nodes) => {
                let mut previous: Option<Class> = None;
                for child in nodes {
                    let class = child.first_class();
                    if let Some(left) = previous {
                        self.cursor += gap(left, class, self.level, self.style, size);
                    }
                    self.node(child, size, baseline);
                    previous = Some(child.last_class());
                }
            }
            Node::Space(permille) => {
                self.cursor += scale(size, *permille);
            }
            Node::Fraction {
                numerator,
                denominator,
                rule,
            } => {
                self.fraction(numerator, denominator, *rule, size, baseline);
            }
            Node::Radical { index, body } => {
                self.radical(index.as_deref(), body, size, baseline);
            }
            Node::Big {
                text,
                stacked,
                sup,
                sub,
            } => {
                self.big(
                    text,
                    *stacked,
                    sup.as_deref(),
                    sub.as_deref(),
                    size,
                    baseline,
                );
            }
            Node::Delimited { left, body, right } => {
                let body_box = self.measure_ink(body, size);
                let needed = (body_box.1 + body_box.2).max(1);
                self.delimiter(left, needed, body_box.1, body_box.2, baseline, size);
                let body_x = self.cursor;
                let saved = self.fence_size;
                self.fence_size = Some((needed, body_box.1, body_box.2));
                self.node_with_x(body, size, baseline, body_x);
                self.fence_size = saved;
                self.cursor = body_x + body_box.0;
                self.delimiter(right, needed, body_box.1, body_box.2, baseline, size);
            }
            Node::Matrix { kind, rows, aligns } => self.matrix(*kind, rows, aligns, size, baseline),
            Node::Substack(rows) => self.substack(rows, size, baseline),
            Node::Brace { body, over } => {
                self.brace(body, *over, size, baseline);
            }
            Node::Tag { text, bare } => {
                // A tag in the body of a formula (rather than in one of its
                // rows) numbers the formula itself.
                if self.matrix_depth == 0 {
                    self.number = RowNumber::Tag {
                        text: text.clone(),
                        bare: *bare,
                    };
                }
            }
            Node::NoTag => {
                if self.matrix_depth == 0 {
                    self.number = RowNumber::Suppressed;
                }
            }
            Node::Color { color, body } => {
                // `\color{...}` colours the rest of its group; painting the
                // items it produced is simpler than threading a colour through
                // every branch.
                let first = self.items.len();
                self.list(body, size, baseline);
                for item in &mut self.items[first..] {
                    if item.color.is_none() {
                        item.color = Some(*color);
                    }
                }
            }
            Node::Script { base, sup, sub } => {
                self.script(base, sup.as_deref(), sub.as_deref(), size, baseline);
            }
            Node::Accent { mark, body, wide } => {
                self.accent(mark, body, *wide, size, baseline);
            }
            Node::Bar { body, over } => {
                self.bar(body, *over, size, baseline);
            }
            Node::OpName {
                text,
                limits,
                sup,
                sub,
            } => {
                self.op_name(
                    text,
                    *limits,
                    sup.as_deref(),
                    sub.as_deref(),
                    size,
                    baseline,
                );
            }
            Node::Classed { body, .. } => self.node(body, size, baseline),
            Node::Stack { over, base } => {
                self.stack(over, base, size, baseline);
            }
            Node::Style { level, body } => {
                let saved = (self.level, self.cramped);
                self.level = *level;
                self.cramped = false;
                let restyled = size_for(*level, self.base_size, self.style);
                self.list(body, restyled, baseline);
                self.level = saved.0;
                self.cramped = saved.1;
            }
            Node::Middle(text) => {
                if let Some((needed, height, depth)) = self.fence_size {
                    self.delimiter(text, needed, height, depth, baseline, size);
                }
            }
        }
    }

    /// Lay out a node list with inter-atom spacing, using the current style.
    fn list(&mut self, nodes: &[Node], size: Width, baseline: i32) {
        let (lefts, rights) = effective_classes(nodes);
        for (index, child) in nodes.iter().enumerate() {
            if index > 0 {
                self.cursor += gap(
                    lefts[index - 1],
                    rights[index],
                    self.level,
                    self.style,
                    size,
                );
            }
            self.node(child, size, baseline);
        }
    }

    /// Absolute size of a script at the current style. Percentages are of the
    /// formula's base size, so a script inside a script does not shrink again.
    fn script_size(&self, _size: Width) -> Width {
        let percent = if self.level.is_script() {
            self.style.scriptscript_scale
        } else {
            self.style.script_scale
        };
        Width(scale(self.base_size, percent).max(1))
    }

    /// Run `apply` with a temporarily changed style.
    fn in_style<T>(
        &mut self,
        level: StyleLevel,
        cramped: bool,
        apply: impl FnOnce(&mut Self) -> T,
    ) -> T {
        let saved = (self.level, self.cramped);
        self.level = level;
        self.cramped = cramped;
        let out = apply(self);
        self.level = saved.0;
        self.cramped = saved.1;
        out
    }

    fn letter_text(&self, character: char) -> String {
        italic_letter(character)
            .filter(|italic| self.metrics.has(*italic))
            .unwrap_or(character)
            .to_string()
    }

    /// A fraction or binomial at the current style.
    fn fraction(
        &mut self,
        numerator: &Node,
        denominator: &Node,
        rule: bool,
        size: Width,
        baseline: i32,
    ) {
        let level = fraction_level(self.level);
        let frac_size = size_for(level, size, self.style);
        let num_cramped = self.cramped;
        let (num_w, num_h, num_d) =
            self.in_style(level, num_cramped, |c| c.measure_ink(numerator, frac_size));
        let (den_w, den_h, _den_d) =
            self.in_style(level, true, |c| c.measure_ink(denominator, frac_size));
        let _ = num_h;
        let width = num_w.max(den_w).max(1);
        let axis = scale(size, self.style.axis_height);
        let thickness = if rule {
            scale(size, self.style.rule_thickness).max(1)
        } else {
            0
        };
        // The rule is centred on the axis (Typst `fraction.rs`). The shifts are
        // distances from the formula baseline to each part's baseline; the ink
        // plus the minimum gap can push a part further out.
        let bar_y = baseline - axis - thickness / 2;
        let num_shift = scale(
            size,
            if self.level == StyleLevel::Display {
                self.style.num_shift_display
            } else {
                self.style.num_shift
            },
        );
        let den_shift = scale(
            size,
            if self.level == StyleLevel::Display {
                self.style.denom_shift_display
            } else {
                self.style.denom_shift
            },
        );
        let display = self.level == StyleLevel::Display;
        let num_gap = scale(
            size,
            if display {
                self.style.num_display_gap
            } else {
                self.style.num_gap
            },
        )
        .max(1);
        let den_gap = scale(
            size,
            if display {
                self.style.denom_display_gap
            } else {
                self.style.denom_gap
            },
        )
        .max(1);
        let num_baseline = baseline - num_shift.max(axis + thickness / 2 + num_d + num_gap);
        let den_baseline = baseline + den_shift.max(-axis + thickness / 2 + den_h + den_gap);
        let left = self.cursor;
        self.in_style(level, num_cramped, |c| {
            c.node_with_x(
                numerator,
                frac_size,
                num_baseline,
                left + (width - num_w) / 2,
            )
        });
        self.in_style(level, true, |c| {
            c.node_with_x(
                denominator,
                frac_size,
                den_baseline,
                left + (width - den_w) / 2,
            )
        });
        if rule {
            self.rules.push(MathRule {
                x: Width(left),
                y: Width(bar_y),
                width: Width(width),
                thickness: Width(thickness),
            });
        }
        self.cursor = left + width;
    }

    /// `\sqrt[...]{...}`, with the vinculum on the body's ink top.
    ///
    /// The surd is a **design variant** (ADR-0075). Its ink top meets the
    /// vinculum, and the vinculum starts at the ink's right edge — not at the
    /// advance, which would leave the gap the bug reports called a broken leg.
    fn radical(&mut self, index: Option<&Node>, body: &Node, size: Width, baseline: i32) {
        let rule = scale(size, self.style.radical_rule).max(1);
        let gap = scale(
            size,
            if self.level == StyleLevel::Display {
                self.style.radical_display_gap
            } else {
                self.style.radical_gap
            },
        );
        let (body_w, body_h, body_d) = self.measure_ink(body, size);
        let needed = (body_h + body_d + rule + gap).max(1);
        let start = self.cursor;

        // The degree's width decides where the surd starts, so measure it first.
        let script_size = self.script_size(size);
        let degree = index.map(|index| {
            self.in_style(StyleLevel::Script, self.cramped, |c| {
                c.measure_ink(index, script_size)
            })
        });

        let choice = self.design_variant("√", needed, size);
        let (glyph, design, fallback, advance, above, below, right_gap) = match choice {
            Some(design) => (
                design.glyph,
                design.design,
                design.fallback,
                design.advance.max(rule),
                design.above,
                design.below,
                design.right_gap,
            ),
            None => {
                // No variant data: grow the text glyph until its ink covers the
                // body, as before.
                let surd_extent = |metrics: &dyn MathMetrics, at: Width| -> i64 {
                    match metrics.ink("√", at) {
                        Some((above, below)) => (above.0 + below.0).max(1) as i64,
                        None => (metrics.ascent(at).0 + metrics.descent(at).0).max(1) as i64,
                    }
                };
                let unit = surd_extent(self.metrics, size);
                let mut radical_size = Width(
                    ((size.0 as i64 * needed as i64 + unit - 1) / unit)
                        .clamp(size.0 as i64 / 2, size.0 as i64 * 4) as i32,
                );
                let mut guard = 0;
                while surd_extent(self.metrics, radical_size) < needed as i64 && guard < 8 {
                    radical_size = Width((radical_size.0 * 9 / 8).max(1));
                    guard += 1;
                }
                let (surd_above, surd_below) = self.metrics.ink("√", radical_size).unwrap_or((
                    self.metrics.ascent(radical_size),
                    self.metrics.descent(radical_size),
                ));
                let advance = self.metrics.advance("√", radical_size).0.max(rule);
                (
                    MathGlyph::Text,
                    radical_size,
                    radical_size,
                    advance,
                    surd_above.0,
                    surd_below.0,
                    0,
                )
            }
        };
        let bar_y = {
            // TeXbook p 443 / Typst: if the chosen surd is taller than the
            // target, distribute the surplus equally above and below instead of
            // leaving one long leg.
            let height = above + below;
            let surplus = height - rule - (body_h + body_d);
            let balanced = (surplus + gap) / 2;
            let gap = gap.max(balanced).max(0);
            baseline - body_h - gap - rule
        };

        let (before, after) = if index.is_some() {
            (
                scale(size, self.style.radical_kern_before),
                scale(size, self.style.radical_kern_after),
            )
        } else {
            (0, 0)
        };
        let degree_width = degree.map(|box_| box_.0).unwrap_or(0);
        let offset = before + degree_width + after;
        // Never hang the surd left of the cursor. When the degree kerns make
        // the offset negative, move the degree right instead (Typst/TeXbook).
        let surd_x = start + offset.max(0);
        let degree_x = start + before - offset.min(0);

        if let (Some(index), Some((_, _, degree_depth))) = (index, degree) {
            // OpenType: the degree's bottom is radicalDegreeBottomRaisePercent
            // of the radical's height above the radical's bottom.
            let height = above + below;
            let raise = (height as i64 * self.style.radical_degree_raise as i64 / 1000) as i32;
            let degree_bottom = bar_y + height - raise;
            let degree_baseline = degree_bottom - degree_depth;
            self.in_style(StyleLevel::Script, self.cramped, |c| {
                c.node_with_x(index, script_size, degree_baseline, degree_x)
            });
        }

        self.items.push(MathItem {
            text: String::from("√"),
            x: Width(surd_x),
            // The surd's ink top meets the vinculum.
            y: Width(bar_y + above),
            size: fallback,
            design,
            glyph,
            stretch: 1000,
            color: None,
        });
        let body_x = surd_x + advance;
        self.node_with_x(body, size, baseline, body_x);
        // Start the rule at the hook's ink, overlapping it by one rule width.
        // A text glyph has no ink data here, so it keeps the old start.
        let rule_x = if matches!(glyph, MathGlyph::Text) {
            body_x
        } else {
            (surd_x + advance - right_gap - rule).max(surd_x)
        };
        self.rules.push(MathRule {
            x: Width(rule_x),
            y: Width(bar_y),
            width: Width((body_x + body_w - rule_x).max(1)),
            thickness: Width(rule),
        });
        self.cursor = body_x + body_w;
    }

    /// A large operator, with stacked limits in display style.
    fn big(
        &mut self,
        text: &str,
        stacked: bool,
        sup: Option<&Node>,
        sub: Option<&Node>,
        size: Width,
        baseline: i32,
    ) {
        // Display operators use a design variant at least
        // displayOperatorMinHeight tall (TeX / OpenType MATH). A font without
        // variants keeps the old behaviour: the base glyph scaled up.
        let display = self.level == StyleLevel::Display;
        let needed = if display {
            scale(size, self.style.display_operator_min).max(size.0)
        } else {
            0
        };
        let choice = if display {
            self.design_variant(text, needed, size)
        } else {
            None
        };
        let (advance, above, below, text_size, design, glyph) = match choice {
            Some(design) => (
                design.advance,
                design.above,
                design.below,
                design.fallback,
                design.design,
                design.glyph,
            ),
            None => {
                let big = if display {
                    Width(needed.max(size.0))
                } else {
                    size
                };
                let advance = self.metrics.advance(text, big).0;
                let (above, below) = self
                    .metrics
                    .ink(text, big)
                    .map(|(a, d)| (a.0, d.0))
                    .unwrap_or((self.metrics.ascent(big).0, self.metrics.descent(big).0));
                (advance, above, below, big, big, MathGlyph::Text)
            }
        };
        // TeXbook p 155: a large operator symbol is centred on the math axis.
        // A word (`\lim`, `\sup`, `\inf`) is not a glyph and keeps its
        // baseline.
        let axis = scale(size, self.style.axis_height);
        let op_y = if text.chars().count() == 1 {
            -axis + (above - below) / 2
        } else {
            0
        };
        if stacked && self.level == StyleLevel::Display {
            let script_size = self.script_size(size);
            let level = self.level.script();
            let sup_box = sup.map(|node| {
                self.in_style(level, self.cramped, |c| c.measure_ink(node, script_size))
            });
            let sub_box =
                sub.map(|node| self.in_style(level, true, |c| c.measure_ink(node, script_size)));
            let width = advance
                .max(sup_box.map(|b| b.0).unwrap_or(0))
                .max(sub_box.map(|b| b.0).unwrap_or(0));
            let left = self.cursor;
            self.items.push(MathItem {
                text: text.to_string(),
                x: Width(left + (width - advance) / 2),
                y: Width(op_y),
                size: text_size,
                design,
                glyph,
                stretch: 1000,
                color: None,
            });
            let gap = scale(size, self.style.limit_above_gap);
            let rise = scale(size, self.style.limit_above_rise);
            if let (Some(node), Some(box_)) = (sup, sup_box) {
                let baseline = op_y - (above + (gap + box_.2).max(rise));
                self.in_style(level, self.cramped, |c| {
                    c.node_with_x(node, script_size, baseline, left + (width - box_.0) / 2)
                });
            }
            let gap = scale(size, self.style.limit_below_gap);
            let drop = scale(size, self.style.limit_below_drop);
            if let (Some(node), Some(box_)) = (sub, sub_box) {
                let baseline = op_y + below + (gap + box_.1).max(drop);
                self.in_style(level, true, |c| {
                    c.node_with_x(node, script_size, baseline, left + (width - box_.0) / 2)
                });
            }
            self.cursor = left + width;
        } else {
            let left = self.cursor;
            self.items.push(MathItem {
                text: text.to_string(),
                x: Width(left),
                y: Width(op_y),
                size: text_size,
                design,
                glyph,
                stretch: 1000,
                color: None,
            });
            self.cursor = left + advance;
            let end = self.side_scripts((advance, above, below), op_y, 0, sup, sub, size, baseline);
            self.cursor = end;
        }
    }

    /// `\hat`, `\bar`, `\vec`, `\tilde`.
    ///
    /// A math face usually carries the combining accents (U+0302/U+0303/U+0304)
    /// and not the spacing modifiers (U+02C6/U+02DC/U+00AF), so prefer the
    /// combining form and align it by the `MATH` top accent attachments
    /// (Typst `accent.rs`, ADR-0076).
    fn accent(&mut self, mark: &str, body: &Node, wide: bool, size: Width, baseline: i32) {
        let (width, height, _depth) = self.measure_ink(body, size);
        let body_x = self.cursor;
        self.node_with_x(body, size, baseline, body_x);

        let combining = match mark {
            "\u{2c6}" => Some("\u{302}"),
            "\u{2dc}" => Some("\u{303}"),
            "\u{af}" => Some("\u{304}"),
            "\u{20d7}" => Some("\u{20d7}"),
            _ => None,
        };
        let mut glyph = mark;
        if let Some(candidate) = combining {
            if candidate
                .chars()
                .next()
                .is_some_and(|character| self.metrics.math_has(character))
            {
                glyph = candidate;
            }
        }
        let mut advance = self.metrics.advance(glyph, size).0;
        let has_glyph = glyph.chars().next().is_some_and(|character| {
            self.metrics.math_has(character) || self.metrics.has(character)
        });
        if !has_glyph {
            // Last resort for a face with neither the combining nor the
            // spacing mark: a drawn arrow, which every math face has.
            glyph = "→";
            advance = self.metrics.advance(glyph, size).0;
        }
        let (mark_above, mark_below) = self
            .metrics
            .ink(glyph, size)
            .map(|(a, d)| (a.0, d.0))
            .unwrap_or((self.metrics.ascent(size).0, self.metrics.descent(size).0));
        let _ = mark_above;
        let gap = scale(size, 60).max(1);
        // A combining accent's ink is entirely above its own baseline, so the
        // signed bottom is negative and the glyph's origin must move *down* to
        // put the ink just above the base.
        let bottom = self
            .metrics
            .accent_bottom(glyph, size)
            .map(|value| value.0)
            .unwrap_or(mark_below);
        let y = baseline - height - gap - bottom;

        let base_attach = self
            .accent_base_text(body)
            .and_then(|text| self.metrics.top_accent(&text, size))
            .map(|value| value.0)
            .unwrap_or(width / 2);
        let accent_attach = self
            .metrics
            .top_accent(glyph, size)
            .map(|value| value.0)
            .unwrap_or(advance / 2);

        let stretch = if wide && advance > 0 && width > advance {
            ((width as i64 * 1000) / advance as i64).clamp(1000, 6000) as u16
        } else {
            1000
        };
        // The renderers scale from the glyph's origin, so the attachment moves
        // with the scale.
        let attach = (accent_attach as i64 * stretch as i64 / 1000) as i32;
        let mut item = MathItem::text(
            glyph.to_string(),
            Width(body_x + base_attach - attach),
            Width(y),
            size,
        );
        item.stretch = stretch;
        self.items.push(item);
        self.cursor = body_x + width;
    }

    /// The text form of a single-glyph accent base, for its attach lookup.
    fn accent_base_text(&self, base: &Node) -> Option<String> {
        match base {
            Node::Letter(character) => Some(self.letter_text(*character)),
            Node::Symbol { text, .. } if text.chars().count() == 1 => Some(text.clone()),
            _ => None,
        }
    }

    /// `\overline` / `\underline`.
    fn bar(&mut self, body: &Node, over: bool, size: Width, baseline: i32) {
        let (width, height, depth) = self.measure_ink(body, size);
        let body_x = self.cursor;
        self.node_with_x(body, size, baseline, body_x);
        let rule = scale(
            size,
            if over {
                self.style.overbar_rule
            } else {
                self.style.underbar_rule
            },
        )
        .max(1);
        let gap = scale(
            size,
            if over {
                self.style.overbar_gap
            } else {
                self.style.underbar_gap
            },
        );
        let y = if over {
            baseline - height - gap - rule
        } else {
            baseline + depth + gap
        };
        self.rules.push(MathRule {
            x: Width(body_x),
            y: Width(y),
            width: Width(width),
            thickness: Width(rule),
        });
        self.cursor = body_x + width;
    }

    /// `\operatorname` and its starred form.
    fn op_name(
        &mut self,
        text: &str,
        limits: bool,
        sup: Option<&Node>,
        sub: Option<&Node>,
        size: Width,
        baseline: i32,
    ) {
        let advance = self.metrics.advance(text, size).0;
        let (above, below) = self
            .metrics
            .ink(text, size)
            .map(|(a, d)| (a.0, d.0))
            .unwrap_or((self.metrics.ascent(size).0, self.metrics.descent(size).0));
        let left = self.cursor;
        if limits && self.level == StyleLevel::Display && (sup.is_some() || sub.is_some()) {
            let script_size = self.script_size(size);
            let level = self.level.script();
            let sup_box = sup.map(|node| {
                self.in_style(level, self.cramped, |c| c.measure_ink(node, script_size))
            });
            let sub_box =
                sub.map(|node| self.in_style(level, true, |c| c.measure_ink(node, script_size)));
            let width = advance
                .max(sup_box.map(|b| b.0).unwrap_or(0))
                .max(sub_box.map(|b| b.0).unwrap_or(0));
            self.items.push(MathItem::text(
                text.to_string(),
                Width(left + (width - advance) / 2),
                Width::ZERO,
                size,
            ));
            let gap = scale(size, self.style.limit_above_gap);
            let rise = scale(size, self.style.limit_above_rise);
            if let (Some(node), Some(box_)) = (sup, sup_box) {
                let baseline = -(above + (gap + box_.2).max(rise));
                self.in_style(level, self.cramped, |c| {
                    c.node_with_x(node, script_size, baseline, left + (width - box_.0) / 2)
                });
            }
            let gap = scale(size, self.style.limit_below_gap);
            let drop = scale(size, self.style.limit_below_drop);
            if let (Some(node), Some(box_)) = (sub, sub_box) {
                let baseline = below + (gap + box_.1).max(drop);
                self.in_style(level, true, |c| {
                    c.node_with_x(node, script_size, baseline, left + (width - box_.0) / 2)
                });
            }
            self.cursor = left + width;
        } else {
            self.items.push(MathItem::text(
                text.to_string(),
                Width(left),
                Width::ZERO,
                size,
            ));
            self.cursor = left + advance;
            let end = self.side_scripts((advance, above, below), 0, 0, sup, sub, size, baseline);
            self.cursor = end;
        }
    }

    /// `\stackrel`.
    fn stack(&mut self, over: &Node, base: &Node, size: Width, baseline: i32) {
        let (base_w, base_h, _base_d) = self.measure_ink(base, size);
        let script_size = self.script_size(size);
        let level = self.level.script();
        let (over_w, _over_h, over_d) =
            self.in_style(level, self.cramped, |c| c.measure_ink(over, script_size));
        let width = base_w.max(over_w);
        let left = self.cursor;
        self.node_with_x(base, size, baseline, left + (width - base_w) / 2);
        let gap = scale(size, self.style.stack_gap).max(1);
        let y = baseline - base_h - gap - over_d;
        self.in_style(level, self.cramped, |c| {
            c.node_with_x(over, script_size, y, left + (width - over_w) / 2)
        });
        self.cursor = left + width;
    }

    /// A base with its scripts to the right. Returns the cursor after them.
    #[allow(clippy::too_many_arguments)]
    fn side_scripts(
        &mut self,
        base: (i32, i32, i32),
        offset: i32,
        italic: i32,
        sup: Option<&Node>,
        sub: Option<&Node>,
        size: Width,
        baseline: i32,
    ) -> i32 {
        let script_size = self.script_size(size);
        let level = self.level.script();
        let sup_box = sup
            .map(|node| self.in_style(level, self.cramped, |c| c.measure_ink(node, script_size)));
        let sub_box =
            sub.map(|node| self.in_style(level, true, |c| c.measure_ink(node, script_size)));
        let x = self.cursor + italic;
        // TeX rule 18: the superscript's baseline is at least `sup_shift` above
        // the base's, and at most `sup_drop` below the base's *top*. The
        // subscript's baseline is at least `sub_shift` below the base's, and at
        // most `sub_drop` below the base's *bottom*.
        let mut lift = scale(
            size,
            if self.cramped {
                self.style.sup_shift_cramped
            } else {
                self.style.sup_shift
            },
        );
        lift = lift.max(base.1 - scale(size, self.style.sup_drop));
        if let Some((_, _, depth)) = sup_box {
            lift = lift.max(depth + scale(size, self.style.sup_bottom_min));
        }
        let mut drop = scale(size, self.style.sub_shift);
        drop = drop.max(base.2 + scale(size, self.style.sub_drop));
        if let Some((_, subscript_height, _)) = sub_box {
            // TeX rule 18: a tall subscript may not rise too far into the base.
            drop = drop.max(subscript_height - scale(size, self.style.sub_top_max));
        }
        if let (Some((_, _, sd)), Some((_, sh, _))) = (sup_box, sub_box) {
            // With a subscript present the superscript may not float higher
            // than `sup_bottom_max_sub` above the baseline, as long as the
            // minimum gap still holds afterwards.
            let cap = scale(size, self.style.sup_bottom_max_sub) + sd;
            let floor = sd + scale(size, self.style.sup_bottom_min);
            if cap < lift && cap >= floor {
                lift = cap;
            }
            let wanted = scale(size, self.style.sub_sup_gap);
            let present = lift + drop - sd - sh;
            if present < wanted {
                lift += wanted - present;
            }
        }
        let mut end = x;
        let mut had_script = false;
        if let (Some(node), Some((width, _, _))) = (sup, sup_box) {
            self.in_style(level, self.cramped, |c| {
                c.node_with_x(node, script_size, baseline + offset - lift, x)
            });
            end = end.max(x + width);
            had_script = true;
        }
        if let (Some(node), Some((width, _, _))) = (sub, sub_box) {
            self.in_style(level, true, |c| {
                c.node_with_x(node, script_size, baseline + offset + drop, x)
            });
            end = end.max(x + width);
            had_script = true;
        }
        if had_script {
            end + scale(size, self.style.script_space)
        } else {
            end
        }
    }

    fn script(
        &mut self,
        base: &Node,
        sup: Option<&Node>,
        sub: Option<&Node>,
        size: Width,
        baseline: i32,
    ) {
        // A brace takes its label centred over (or under) the ink band rather
        // than after it: `\overbrace{a+b}^{n}` means the `n` sits on top of the
        // brace, not to its right.
        if let Node::Brace { body, over } = base {
            let geo = self.brace(body, *over, size, baseline);
            let script_size = self.script_size(size);
            let level = self.level.script();
            let gap = scale(size, 80).max(1);
            if *over {
                if let Some(sup) = sup {
                    let measured =
                        self.in_style(level, self.cramped, |c| c.measure_ink(sup, script_size));
                    let depth = measured.2.min(scale(script_size, 150));
                    let y = geo.ink_top - gap - depth;
                    let x = geo.left + (geo.width - measured.0).max(0) / 2;
                    self.in_style(level, self.cramped, |c| {
                        c.node_with_x(sup, script_size, y, x)
                    });
                }
            } else if let Some(sub) = sub {
                let measured = self.in_style(level, true, |c| c.measure_ink(sub, script_size));
                let height = measured.1.min(scale(script_size, 720));
                let y = geo.ink_bottom + gap + height;
                let x = geo.left + (geo.width - measured.0).max(0) / 2;
                self.in_style(level, true, |c| c.node_with_x(sub, script_size, y, x));
            }
            self.cursor = self.cursor.max(geo.left + geo.width);
            return;
        }
        let base_box = self.measure_ink(base, size);
        let italic = self.italic_correction(base, size);
        self.node(base, size, baseline);
        let end = self.side_scripts(
            (base_box.0, base_box.1, base_box.2),
            0,
            italic,
            sup,
            sub,
            size,
            baseline,
        );
        self.cursor = end;
    }

    fn italic_correction(&self, base: &Node, size: Width) -> i32 {
        let text = match base {
            Node::Letter(character) => self.letter_text(*character),
            Node::Symbol { text, .. } if text.chars().count() == 1 => text.clone(),
            _ => return 0,
        };
        self.metrics.italic_correction(&text, size).0.max(0)
    }

    fn node_with_x(&mut self, node: &Node, size: Width, baseline: i32, x: i32) {
        let saved = self.cursor;
        self.cursor = x;
        self.node(node, size, baseline);
        let end = self.cursor;
        self.cursor = saved.max(end);
    }

    /// Measure the *ink* box of a node: words count for a cap height and a
    /// short descender rather than the font's full ascent and descent.
    ///
    /// Constructs that stack tight — the stretchy braces and the limits of a
    /// display operator — need this. A brace is placed against the body's
    /// edge, and the font-wide ascent (1.05 em) would leave a visible hole
    /// above a line of lowercase letters, whose ink tops out near 0.7 em; the
    /// same overshoot doubles the leading of a `\substack`.
    fn measure_ink(&mut self, node: &Node, size: Width) -> (i32, i32, i32) {
        self.measure_impl(core::slice::from_ref(node), size, true)
    }

    fn measure_nodes_ink(&mut self, nodes: &[Node], size: Width) -> (i32, i32, i32) {
        self.measure_impl(nodes, size, true)
    }

    fn measure_impl(&mut self, nodes: &[Node], size: Width, ink: bool) -> (i32, i32, i32) {
        if nodes.is_empty() {
            return (0, 0, 0);
        }
        let saved_items = self.items.len();
        let saved_rules = self.rules.len();
        let saved_rows = self.rows.len();
        let saved_cursor = self.cursor;
        self.cursor = 0;
        let (lefts, rights) = effective_classes(nodes);
        for (index, child) in nodes.iter().enumerate() {
            if index > 0 {
                self.cursor += gap(
                    lefts[index - 1],
                    rights[index],
                    self.level,
                    self.style,
                    size,
                );
            }
            self.node(child, size, 0);
        }
        let width = self.cursor;
        let mut height = 0i32;
        let mut depth = 0i32;
        for item in &self.items[saved_items..] {
            let (above, below) = if ink {
                item_ink(self.metrics, item)
            } else {
                (
                    self.metrics.ascent(item.size).0,
                    self.metrics.descent(item.size).0,
                )
            };
            height = height.max(-item.y.0 + above);
            depth = depth.max(item.y.0 + below);
        }
        for rule in &self.rules[saved_rules..] {
            height = height.max(-rule.y.0);
            depth = depth.max(rule.y.0 + rule.thickness.0);
        }
        self.items.truncate(saved_items);
        self.rules.truncate(saved_rules);
        self.rows.truncate(saved_rows);
        self.cursor = saved_cursor;
        (width, height, depth)
    }

    /// Place a node list at an absolute `x` without changing the final cursor.
    fn place_nodes(&mut self, nodes: &[Node], size: Width, baseline: i32, x: i32) {
        let saved = self.cursor;
        self.cursor = x;
        let (lefts, rights) = effective_classes(nodes);
        for (index, child) in nodes.iter().enumerate() {
            if index > 0 {
                self.cursor += gap(
                    lefts[index - 1],
                    rights[index],
                    self.level,
                    self.style,
                    size,
                );
            }
            self.node(child, size, baseline);
        }
        let end = self.cursor;
        self.cursor = saved.max(end);
    }

    /// Push one delimiter glyph at the cursor; the empty string is the null
    /// delimiter and advances nothing.
    ///
    /// A fence is a design variant when the font has one (ADR-0075); its ink is
    /// centred on the body's ink. Without variants the text glyph is grown
    /// until it covers `needed`.
    fn delimiter(
        &mut self,
        text: &str,
        needed: i32,
        height: i32,
        depth: i32,
        baseline: i32,
        size: Width,
    ) {
        if text.is_empty() {
            return;
        }
        let choice = self.design_variant(text, needed, size);
        let (glyph, design_size, fallback, advance, above, below) = match choice {
            Some(design) => (
                design.glyph,
                design.design,
                design.fallback,
                design.advance,
                design.above,
                design.below,
            ),
            None => {
                let extent = |metrics: &dyn MathMetrics, at: Width| -> i32 {
                    match metrics.ink(text, at) {
                        Some((above, below)) => (above.0 + below.0).max(1),
                        None => (metrics.ascent(at).0 + metrics.descent(at).0).max(1),
                    }
                };
                let unit = extent(self.metrics, size);
                let mut delim_size = if unit < needed {
                    let estimate = size.0 as i64 * needed as i64;
                    let target = (estimate + unit as i64 - 1) / unit as i64;
                    Width(target.clamp(size.0 as i64, size.0 as i64 * 4) as i32)
                } else {
                    size
                };
                let mut guard = 0;
                while extent(self.metrics, delim_size) < needed
                    && delim_size.0 < size.0 * 4
                    && guard < 64
                {
                    delim_size = Width(delim_size.0 + (delim_size.0 / 64).max(1));
                    guard += 1;
                }
                let (above, below) = self.metrics.ink(text, delim_size).unwrap_or((
                    self.metrics.ascent(delim_size),
                    self.metrics.descent(delim_size),
                ));
                let advance = self.metrics.advance(text, delim_size).0;
                (
                    MathGlyph::Text,
                    delim_size,
                    delim_size,
                    advance,
                    above.0,
                    below.0,
                )
            }
        };
        // A stretched design glyph is centred on the math axis (TeXbook rule
        // 15 / Typst). A base glyph is already axis-centred by design, so it
        // keeps its baseline; a scaled text glyph centres on the body's ink.
        let off = match glyph {
            MathGlyph::Text => (depth - height) / 2 - (below - above) / 2,
            MathGlyph::Vertical(0) => 0,
            MathGlyph::Vertical(_) => -scale(size, self.style.axis_height) + (above - below) / 2,
        };
        self.items.push(MathItem {
            text: text.to_string(),
            x: Width(self.cursor),
            y: Width(baseline + off),
            size: fallback,
            design: design_size,
            glyph,
            stretch: 1000,
            color: None,
        });
        self.cursor += advance;
    }

    fn matrix(
        &mut self,
        kind: MatrixKind,
        rows: &[Vec<Vec<Node>>],
        aligns: &[ColumnAlign],
        size: Width,
        baseline: i32,
    ) {
        let cols = rows.iter().map(|row| row.len()).max().unwrap_or(0);
        if cols == 0 || rows.is_empty() {
            return;
        }
        // Only the outermost block of a formula owns its rows: an `aligned`
        // inside an `equation`, or a `cases` inside an `aligned`, must not add
        // numbers of its own.
        let outermost =
            self.matrix_depth == 0 && kind == MatrixKind::Aligned && self.rows.is_empty();
        self.matrix_depth += 1;
        let row_gap = scale(size, self.style.jot).max(1);
        let col_gap = if kind == MatrixKind::Aligned {
            0
        } else {
            scale(size, self.style.rel_gap)
        };

        let mut widths = alloc::vec![0i32; cols];
        let mut ascent = alloc::vec![0i32; rows.len()];
        let mut descent = alloc::vec![0i32; rows.len()];
        let mut cells: Vec<Vec<(i32, i32, i32)>> = Vec::with_capacity(rows.len());
        for (r, row) in rows.iter().enumerate() {
            let mut line = Vec::with_capacity(cols);
            for (c, width) in widths.iter_mut().enumerate() {
                let empty: &[Node] = &[];
                let nodes = row.get(c).map(|nodes| nodes.as_slice()).unwrap_or(empty);
                let measured = self.measure_nodes_ink(nodes, size);
                *width = (*width).max(measured.0);
                ascent[r] = ascent[r].max(measured.1);
                descent[r] = descent[r].max(measured.2);
                line.push(measured);
            }
            cells.push(line);
        }

        let total: i32 = widths.iter().sum::<i32>() + col_gap * cols.saturating_sub(1) as i32;
        let mut baselines = alloc::vec![0i32; rows.len()];
        for r in 1..rows.len() {
            baselines[r] = baselines[r - 1] + descent[r - 1] + row_gap + ascent[r];
        }
        let mut height = (0..rows.len())
            .map(|r| ascent[r] - baselines[r])
            .max()
            .unwrap_or(0);
        let mut depth = (0..rows.len())
            .map(|r| descent[r] + baselines[r])
            .max()
            .unwrap_or(0);

        // A matrix is centred on the math axis, not on its first row
        // (TeXbook / Typst `table.rs`). Shifting every baseline keeps the row
        // spacing and moves the block as a whole.
        let shift = (height - depth) / 2 - scale(size, self.style.axis_height);
        for baseline in &mut baselines {
            *baseline += shift;
        }
        height -= shift;
        depth += shift;

        let needed = (height + depth).max(1);
        let (left, right) = fence_text(kind);
        self.delimiter(left, needed, height, depth, baseline, size);
        let origin = self.cursor;

        let mut col_x = alloc::vec![0i32; cols];
        let mut acc = 0i32;
        for (c, width) in widths.iter().enumerate() {
            col_x[c] = acc;
            acc += width + col_gap;
        }

        for r in 0..rows.len() {
            for c in 0..cols {
                let width = cells[r][c].0;
                let cell_x = match aligns.get(c) {
                    Some(ColumnAlign::Left) => col_x[c],
                    Some(ColumnAlign::Right) => col_x[c] + widths[c] - width,
                    Some(ColumnAlign::Center) => col_x[c] + (widths[c] - width) / 2,
                    None => match kind {
                        MatrixKind::Aligned if c % 2 == 0 => col_x[c] + widths[c] - width,
                        MatrixKind::Aligned | MatrixKind::Cases => col_x[c],
                        _ => col_x[c] + (widths[c] - width) / 2,
                    },
                };
                self.place_nodes(&rows[r][c], size, baseline + baselines[r], origin + cell_x);
            }
        }
        if outermost {
            for (r, row_baseline) in baselines.iter().enumerate() {
                self.rows.push(MathRow {
                    baseline: Width(baseline + *row_baseline),
                    number: row_number(&rows[r]).unwrap_or_default(),
                });
            }
        }
        self.matrix_depth -= 1;
        self.cursor = origin + total;
        self.delimiter(right, needed, height, depth, baseline, size);
    }

    /// Stack lines centred on each other, as `\substack` does inside a limit.
    fn substack(&mut self, rows: &[Vec<Node>], size: Width, baseline: i32) {
        if rows.is_empty() {
            return;
        }
        // `\substack` lines are set on a `\jot` of leading, as TeX does
        // inside limits.
        let gap = scale(size, self.style.jot).max(1);
        let mut widths = Vec::with_capacity(rows.len());
        let mut heights = Vec::with_capacity(rows.len());
        let mut depths = Vec::with_capacity(rows.len());
        for row in rows {
            let (width, height, depth) = self.measure_nodes_ink(row, size);
            widths.push(width);
            heights.push(height);
            depths.push(depth);
        }
        let width = widths.iter().copied().max().unwrap_or(0);
        let left = self.cursor;
        let mut line_baseline = baseline;
        for (index, row) in rows.iter().enumerate() {
            if index > 0 {
                // Clear the previous line's descender by `gap` and this line's
                // ascender by its own height.
                line_baseline += depths[index - 1] + gap + heights[index];
            }
            let x = left + (width - widths[index]) / 2;
            self.place_nodes(row, size, line_baseline, x);
        }
        self.cursor = left + width;
    }

    /// Draw the stretchy brace over/under `body` and report its geometry.
    ///
    /// Two shapes, one rule: a brace never gets flatter as it gets wider. A
    /// narrow body keeps the font's brace glyph, grown uniformly (which keeps
    /// its hooks) and then stretched sideways for the remainder. Past
    /// [`BRACE_GLYPH_STRETCH`] that sideways stretch turns the hooks into
    /// ramps, so the brace is assembled from rules instead — a flat run, two
    /// hooks and a cusp, which is the same set of pieces TeX uses, drawn here
    /// because the font carries the single-character brace only.
    fn brace(&mut self, body: &Node, over: bool, size: Width, baseline: i32) -> BraceGeo {
        let glyph = if over { "\u{23de}" } else { "\u{23df}" };
        let (width, height, depth) = self.measure_ink(body, size);
        let left = self.cursor;
        self.node_with_x(body, size, baseline, left);
        let gap = scale(size, self.style.stack_gap).max(1);
        let natural = self.metrics.advance(glyph, size).0.max(1);
        let target = width.max(1) as i64;
        let grown = (target * 1000 / natural as i64).clamp(1000, 2200) as u16;
        let grown_size = Width((size.0 as i64 * i64::from(grown) / 1000).max(1) as i32);
        let grown_advance = self.metrics.advance(glyph, grown_size).0.max(1);
        let stretch = ((target * 1000) / grown_advance as i64).clamp(1000, 60000) as u16;
        let geo = if stretch <= BRACE_GLYPH_STRETCH {
            let (ink_top, ink_bottom) = brace_ink(glyph).expect("both braces have an ink band");
            let ink_top = per_mille(grown_size, ink_top);
            let ink_bottom = per_mille(grown_size, ink_bottom);
            // `y` is the glyph's baseline; the ink band sits a fixed distance
            // from it. Over: put the band's bottom `gap` above the body's top.
            // Under: put its top `gap` below the body's depth.
            let y = if over {
                baseline - (height + gap) + ink_bottom
            } else {
                baseline + depth + gap + ink_top
            };
            if self.metrics.has(glyph.chars().next().unwrap_or('\u{23de}')) {
                let mut item =
                    MathItem::text(String::from(glyph), Width(left), Width(y), grown_size);
                item.stretch = stretch;
                self.items.push(item);
            }
            BraceGeo {
                left,
                width,
                ink_top: y - ink_top,
                ink_bottom: y - ink_bottom,
            }
        } else {
            let tip = if over { height + gap } else { depth + gap };
            self.assemble_brace(left, width, tip, over, size, baseline)
        };
        self.cursor = left + width;
        geo
    }

    /// Draw a brace from rules: flat run, two hooks, one cusp.
    ///
    /// `tip` is the distance from the baseline to the ink nearest the body.
    /// Every segment is one rule whose outer edge follows the curve; the step
    /// is half a stroke width, which is finer than a pixel at any print size.
    fn assemble_brace(
        &mut self,
        left: i32,
        width: i32,
        tip: i32,
        over: bool,
        size: Width,
        baseline: i32,
    ) -> BraceGeo {
        // Stroke, hook and cusp are proportions of the formula size: at 10.5
        // pt the stroke is 0.66 pt, the hook drops a quarter em, the cusp rises
        // a fifth. The numbers are the ones a designed brace glyph carries,
        // measured off the same Noto glyph the narrow path uses.
        let thickness = (size.0 / 11).max(1);
        let hook = (size.0 / 4).max(thickness * 3).min(width / 4).max(1);
        let cusp = (size.0 / 5).max(thickness * 3).min(width / 4).max(1);
        let half_cusp = hook;
        let bar = tip + hook;
        let peak = bar + cusp;
        let step = (thickness / 2).max(1);
        let middle = width / 2;

        // Distance from the baseline to the stroke's outer edge at `x`.
        let level_at = |x: i32| -> i32 {
            if x < hook {
                let dx = hook - x;
                // Quarter circle: vertical tangent at the tip, flat at the bar.
                bar - hook + isqrt((hook * hook - dx * dx).max(0))
            } else if x < middle - half_cusp {
                bar
            } else if x < middle + half_cusp {
                let d = (x - middle).abs();
                bar + cusp * (half_cusp - d) / half_cusp
            } else if x < width - hook {
                bar
            } else {
                let dx = x - (width - hook);
                bar - hook + isqrt((hook * hook - dx * dx).max(0))
            }
        };

        // One rule per slice, spanning the whole level range the slice covers.
        // A stroke drawn only as horizontal ribbons would break apart where the
        // curve is steep (the tangent at a hook tip is vertical); covering both
        // ends keeps the ink continuous.
        let mut stroke = |x: i32, w: i32| {
            if w <= 0 {
                return;
            }
            let high = level_at(x).max(level_at(x + w));
            let low = level_at(x).min(level_at(x + w));
            let thickness = high - low + thickness;
            let y = if over {
                baseline - high
            } else {
                baseline + low - thickness
            };
            self.rules.push(MathRule {
                x: Width(left + x),
                y: Width(y),
                width: Width(w),
                thickness: Width(thickness),
            });
        };

        let mut x = 0;
        while x < width {
            stroke(x, step.min(width - x));
            x += step;
        }

        let (ink_top, ink_bottom) = if over {
            (baseline - peak, baseline - (bar - hook))
        } else {
            (baseline + (bar - hook), baseline + peak)
        };
        BraceGeo {
            left,
            width,
            ink_top,
            ink_bottom,
        }
    }
}
/// Where a stretchy brace landed: its horizontal span and the ink band,
/// both relative to the math baseline (negative is up).
struct BraceGeo {
    left: i32,
    width: i32,
    ink_top: i32,
    ink_bottom: i32,
}

/// The number a matrix row asks for, if its cells say anything about it.
fn row_number(row: &[Vec<Node>]) -> Option<RowNumber> {
    for cell in row {
        for node in cell {
            match node {
                Node::Tag { text, bare } => {
                    return Some(RowNumber::Tag {
                        text: text.clone(),
                        bare: *bare,
                    });
                }
                Node::NoTag => return Some(RowNumber::Suppressed),
                _ => {}
            }
        }
    }
    None
}

/// Fences for a matrix kind.
fn fence_text(kind: MatrixKind) -> (&'static str, &'static str) {
    match kind {
        MatrixKind::Plain | MatrixKind::Aligned => ("", ""),
        MatrixKind::Paren => ("(", ")"),
        MatrixKind::Bracket => ("[", "]"),
        MatrixKind::Vert => ("|", "|"),
        MatrixKind::Cases => ("{", ""),
    }
}

/// TeX's inter-atom spacing table: 0 = none, 1 = thin (3mu), 2 = medium
/// (4mu), 3 = thick (5mu), -1 = impossible (a Bin there is really an Ord).
const SPACING: [[i8; 8]; 8] = [
    // Ord  Op  Bin  Rel  Open Close Punct Inner
    [0, 1, 2, 3, 0, 0, 0, 1],       // Ord
    [1, 1, -1, 3, 0, 0, 0, 1],      // Op
    [2, -1, -1, -1, 2, -1, -1, -1], // Bin
    [3, 3, -1, 0, 3, 0, 0, 3],      // Rel
    [0, 0, -1, 0, 0, 0, 0, 0],      // Open
    [0, 1, 2, 3, 0, 0, 0, 1],       // Close
    [1, 1, -1, 1, 1, 1, 1, 1],      // Punct
    [1, 1, 2, 3, 1, 0, 1, 1],       // Inner
];

fn class_index(class: Class) -> usize {
    match class {
        Class::Ord => 0,
        Class::Op => 1,
        Class::Bin => 2,
        Class::Rel => 3,
        Class::Open => 4,
        Class::Close => 5,
        Class::Punct => 6,
        Class::Inner => 7,
    }
}

fn gap(left: Class, right: Class, level: StyleLevel, style: &MathStyle, size: Width) -> i32 {
    let mut value = SPACING[class_index(left)][class_index(right)];
    if value < 0 {
        // A `*` entry: that Bin is really an Ord.
        let left = if left == Class::Bin { Class::Ord } else { left };
        let right = if right == Class::Bin {
            Class::Ord
        } else {
            right
        };
        value = SPACING[class_index(left)][class_index(right)].max(0);
    }
    if level.is_script() && value >= 2 {
        // `\nonscript`: medium and thick spaces vanish in script styles.
        return 0;
    }
    let permille = match value {
        1 => style.punct_gap,
        2 => style.bin_gap,
        3 => style.rel_gap,
        _ => 0,
    };
    scale(size, permille)
}

/// Effective classes for a list: a Bin at a list edge, or after another
/// Bin/Op/Rel/Open/Punct, is an Ord (TeX rules 5/6). The pair is returned as
/// "class used on the left of each node" and "class used on the right".
fn effective_classes(nodes: &[Node]) -> (Vec<Class>, Vec<Class>) {
    let firsts: Vec<Class> = nodes.iter().map(Node::first_class).collect();
    let lasts: Vec<Class> = nodes.iter().map(Node::last_class).collect();
    let mut rights = firsts.clone();
    let mut lefts = lasts.clone();
    // Left to right: a Bin at the start, or after Bin/Op/Rel/Open/Punct, is an
    // Ord. A lone atom keeps the same class on both sides, so the conversion
    // also applies to the gap that follows it.
    for index in 0..nodes.len() {
        if rights[index] == Class::Bin
            && (index == 0
                || matches!(
                    lefts[index - 1],
                    Class::Bin | Class::Op | Class::Rel | Class::Open | Class::Punct
                ))
        {
            rights[index] = Class::Ord;
            if firsts[index] == lasts[index] {
                lefts[index] = Class::Ord;
            }
        }
    }
    // Right to left: a Bin before Rel/Close/Punct, or at the end, is an Ord.
    for index in (0..nodes.len()).rev() {
        if lefts[index] == Class::Bin
            && (index + 1 == nodes.len()
                || matches!(rights[index + 1], Class::Rel | Class::Close | Class::Punct))
        {
            lefts[index] = Class::Ord;
            if firsts[index] == lasts[index] {
                rights[index] = Class::Ord;
            }
        }
    }
    (lefts, rights)
}

fn fraction_level(level: StyleLevel) -> StyleLevel {
    match level {
        StyleLevel::Display => StyleLevel::Text,
        StyleLevel::Text => StyleLevel::Script,
        StyleLevel::Script | StyleLevel::ScriptScript => StyleLevel::ScriptScript,
    }
}

fn size_for(level: StyleLevel, size: Width, style: &MathStyle) -> Width {
    match level {
        StyleLevel::Display | StyleLevel::Text => size,
        StyleLevel::Script => Width(scale(size, style.script_scale).max(1)),
        StyleLevel::ScriptScript => Width(scale(size, style.scriptscript_scale).max(1)),
    }
}

/// Ink extents of one item, with the brace band and the em box as fallbacks.
fn item_ink(metrics: &dyn MathMetrics, item: &MathItem) -> (i32, i32) {
    if let Some((top, bottom)) = brace_ink(&item.text) {
        return (per_mille(item.size, top), per_mille(item.size, bottom));
    }
    if let MathGlyph::Vertical(index) = item.glyph {
        if let Some(variant) = metrics.vertical_variant(&item.text, index, item.design) {
            return (variant.above.0, variant.below.0);
        }
    }
    match metrics.ink(&item.text, item.size) {
        Some((above, below)) => (above.0, below.0),
        None => (metrics.ascent(item.size).0, metrics.descent(item.size).0),
    }
}

fn scale(size: Width, permille: impl Into<i64>) -> i32 {
    (size.0 as i64 * permille.into() / 1000) as i32
}
