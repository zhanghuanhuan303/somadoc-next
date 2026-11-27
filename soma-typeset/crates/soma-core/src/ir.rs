//! Typesetting IR.
//!
//! Two layers are intentionally separate:
//!
//! - the semantic layer lives in the `soma-doc` crate (DocIR);
//! - this module describes a *paragraph ready to break*: ink atoms plus the
//!   boundaries between them.
//!
//! Chinese-specific rules are compiled into `Boundary` values by the rules
//! layer. The breaker itself only sees widths, glue, and breakability, which
//! keeps it script-agnostic.

use alloc::string::String;
use alloc::vec::Vec;

use crate::metrics::StyleId;
use crate::units::Width;

/// Paragraph alignment.
///
/// The full set is kept in the type so that adding the non-justify paths later
/// is not a canonical-schema change. Model v1 implements `Justify` only; every
/// other variant returns `BreakError::UnsupportedAlign`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Align {
    /// Justify every line except the last.
    #[default]
    Justify,
    /// Do not distribute leftover space. Useful for diagnostics.
    Left,
    /// Center each line in the paragraph width. Not implemented in model v1.
    Center,
    /// Flush each line to the right edge. Not implemented in model v1.
    Right,
}

/// The kind of an ink atom. The rules layer assigns this from the source text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AtomKind {
    /// One CJK character or CJK punctuation mark.
    Cjk,
    /// A run of Latin letters that must not be split.
    LatinRun,
    /// A run of digits (possibly including `.` or `,` between digits).
    DigitRun,
    /// A replaced inline object. MVP keeps it unbreakable.
    InlineObject,
    /// A ruby base: the characters an annotation sits over. One atom, so the
    /// annotation can never be separated from what it annotates.
    Ruby,
    /// Any other character not covered above.
    Other,
}

/// Punctuation semantics carried by the IR, not by `Metrics`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PunctClass {
    #[default]
    None,
    /// Opening punctuation: no break immediately after it.
    Open,
    /// Closing punctuation: no break immediately before it.
    Close,
    /// Middle punctuation (comma, full stop, colon, ...): no line start.
    Middle,
    /// Wide marks that carry their own space on both sides: the em dash and
    /// the ellipsis. They may not start a line, and neither side is trimmed —
    /// half an em off a `——` pair would close the break in the dash (D83).
    Dash,
}

/// Whether a boundary may be used as a line break.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Breakability {
    Allowed,
    Forbidden,
    /// A hard break: the paragraph must be split here. The breaker pre-splits
    /// the paragraph into segments at these boundaries (K5).
    Forced,
}

/// Whether the boundary carries an actual inter-word space or not.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BoundaryKind {
    /// Inter-character or inter-run spacing, possibly zero.
    Normal,
    /// A source whitespace boundary. Its width is resolved through
    /// `Metrics::space_width` at layout time.
    Space,
    /// A hyphenation point inside a Latin word. The line may break here, and
    /// the broken line ends with a hyphen glyph (ADR-0059).
    Hyphen,
}

/// Glue width plus its allowed stretch and shrink.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Glue {
    pub width: Width,
    pub stretch: Width,
    pub shrink: Width,
}

/// Whether the boundary carries a CJK/Latin spacing role. The breaker does not
/// interpret the script; it only uses this tag for the boundary-deviation term.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BoundaryRole {
    #[default]
    Plain,
    CjkLatin,
}

/// A break opportunity and the glue at that boundary.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Boundary {
    pub breakable: Breakability,
    pub kind: BoundaryKind,
    pub role: BoundaryRole,
    pub glue: Glue,
    /// Additional cost for breaking here. Forbidden breaks are expressed with
    /// `breakable`, not with an infinite penalty.
    pub penalty: i32,
    /// Fixed tightening of the gap between two adjacent punctuation atoms.
    /// Measured in the same units as `Glue::width`; zero means “not a pair”.
    /// The breaker subtracts it from the natural width of a line that keeps
    /// the boundary inside, and closes the box gap by the same amount.
    pub pair_squeeze: Width,
}

/// Inline decorations carried from the source to the renderers.
///
/// The breaker never reads this: an underline does not change a width or a
/// break opportunity, it only changes what the renderer paints.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Decorations(u8);

impl Decorations {
    pub const NONE: Self = Self(0);
    pub const UNDERLINE: Self = Self(1);
    pub const STRIKETHROUGH: Self = Self(2);

    pub const fn empty() -> Self {
        Self(0)
    }

    pub const fn from_bits(bits: u8) -> Self {
        Self(bits)
    }

    pub const fn bits(self) -> u8 {
        self.0
    }

    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }
}

/// One class of a highlighted code token.
///
/// This is shared vocabulary only: the breaker never reads it, layout stores
/// it beside an atom, and a renderer resolves it through the template's
/// `syntax:` palette. Keeping it here lets the rule layer, layout and every
/// backend name the same class without depending on a lexer crate.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SyntaxClass {
    Comment,
    Keyword,
    String,
    Number,
    Type,
    Function,
    Variable,
    Meta,
    Literal,
}

impl SyntaxClass {
    pub const COUNT: usize = 9;
    pub const ALL: [Self; Self::COUNT] = [
        Self::Comment,
        Self::Keyword,
        Self::String,
        Self::Number,
        Self::Type,
        Self::Function,
        Self::Variable,
        Self::Meta,
        Self::Literal,
    ];

    pub const fn index(self) -> usize {
        match self {
            Self::Comment => 0,
            Self::Keyword => 1,
            Self::String => 2,
            Self::Number => 3,
            Self::Type => 4,
            Self::Function => 5,
            Self::Variable => 6,
            Self::Meta => 7,
            Self::Literal => 8,
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Comment => "comment",
            Self::Keyword => "keyword",
            Self::String => "string",
            Self::Number => "number",
            Self::Type => "type",
            Self::Function => "function",
            Self::Variable => "variable",
            Self::Meta => "meta",
            Self::Literal => "literal",
        }
    }
}

/// One unbreakable unit of ink.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Atom {
    pub text: String,
    pub kind: AtomKind,
    pub class: PunctClass,
    pub style: StyleId,
    /// Underline and friends, from the inline that produced this atom.
    pub decor: Decorations,
    /// Link target, when the atom came from a link.
    pub link: Option<alloc::sync::Arc<str>>,
    /// Maximum amount that may be trimmed from the left edge when this atom
    /// starts a line.
    pub compress_left: Width,
    /// Maximum amount that may be trimmed from the right edge when this atom
    /// ends a line.
    pub compress_right: Width,
}

/// Paragraph-level geometry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ParagraphStyle {
    /// Content width, excluding any first-line indent.
    pub width: Width,
    pub first_indent: Width,
    /// Hanging indent: the first line starts this far to the *left* of the
    /// content column and is this much wider, which is how a list marker sits
    /// outside the text it marks. Zero for every paragraph that is not a list
    /// item; `first_indent` and `hang` are never both non-zero (ADR-0058).
    pub hang: Width,
    /// Extra advance between atoms (tracking). Applied to every atom but the
    /// last, so a line's width never includes a trailing gap (ADR-0059).
    pub tracking: Width,
    pub line_height: Width,
    pub align: Align,
}

impl Default for ParagraphStyle {
    fn default() -> Self {
        Self {
            width: Width::from_em(40),
            first_indent: Width::from_em(2),
            hang: Width::ZERO,
            tracking: Width::ZERO,
            line_height: Width::from_em_frac(3, 2),
            align: Align::Justify,
        }
    }
}

/// A paragraph in breakable form.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Paragraph {
    pub style: ParagraphStyle,
    pub atoms: Vec<Atom>,
    /// `boundaries.len()` must be `atoms.len() - 1`.
    pub boundaries: Vec<Boundary>,
}

impl Paragraph {
    /// Check the invariants the breaker relies on.
    pub fn validate(&self) -> Result<(), IrError> {
        if self.atoms.is_empty() {
            return Err(IrError::EmptyParagraph);
        }
        if self.style.width <= Width::ZERO {
            return Err(IrError::InvalidWidth);
        }
        if self.style.first_indent < Width::ZERO || self.style.first_indent >= self.style.width {
            return Err(IrError::InvalidWidth);
        }
        if self.style.hang < Width::ZERO || self.style.hang >= self.style.width {
            return Err(IrError::InvalidWidth);
        }
        if self.style.tracking < Width::ZERO {
            return Err(IrError::InvalidWidth);
        }
        if self.style.line_height <= Width::ZERO {
            return Err(IrError::InvalidLineHeight);
        }
        let expected = self.atoms.len().saturating_sub(1);
        if self.boundaries.len() != expected {
            return Err(IrError::InvalidBoundaries {
                atoms: self.atoms.len() as u32,
                boundaries: self.boundaries.len() as u32,
            });
        }
        for (index, atom) in self.atoms.iter().enumerate() {
            if atom.text.is_empty() {
                return Err(IrError::EmptyAtom {
                    index: index as u32,
                });
            }
            if atom.compress_left < Width::ZERO || atom.compress_right < Width::ZERO {
                return Err(IrError::InvalidCompression {
                    index: index as u32,
                });
            }
        }
        for (index, boundary) in self.boundaries.iter().enumerate() {
            if boundary.glue.width < Width::ZERO
                || boundary.glue.stretch < Width::ZERO
                || boundary.glue.shrink < Width::ZERO
            {
                return Err(IrError::InvalidGlue {
                    index: index as u32,
                });
            }
            if boundary.pair_squeeze < Width::ZERO {
                return Err(IrError::InvalidPairSqueeze {
                    index: index as u32,
                });
            }
        }
        Ok(())
    }
}

/// Invalid IR, reported before any layout work starts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IrError {
    EmptyParagraph,
    InvalidWidth,
    InvalidLineHeight,
    InvalidBoundaries { atoms: u32, boundaries: u32 },
    EmptyAtom { index: u32 },
    InvalidCompression { index: u32 },
    InvalidGlue { index: u32 },
    InvalidPairSqueeze { index: u32 },
}

impl core::fmt::Display for IrError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::EmptyParagraph => f.write_str("paragraph has no atoms"),
            Self::InvalidWidth => f.write_str("paragraph width/indent is invalid"),
            Self::InvalidLineHeight => f.write_str("paragraph line height must be positive"),
            Self::InvalidBoundaries { atoms, boundaries } => write!(
                f,
                "paragraph has {atoms} atoms but {boundaries} boundaries; expected {}",
                atoms.saturating_sub(1)
            ),
            Self::EmptyAtom { index } => write!(f, "atom {index} is empty"),
            Self::InvalidCompression { index } => {
                write!(f, "atom {index} has a negative compression limit")
            }
            Self::InvalidGlue { index } => {
                write!(f, "boundary {index} has negative glue")
            }
            Self::InvalidPairSqueeze { index } => {
                write!(f, "boundary {index} has a negative pair squeeze")
            }
        }
    }
}

impl core::error::Error for IrError {}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::string::ToString;
    use alloc::vec;

    fn atom(text: &str) -> Atom {
        Atom {
            text: text.to_string(),
            kind: AtomKind::Cjk,
            class: PunctClass::None,
            style: StyleId(0),
            decor: Decorations::NONE,
            link: None,
            compress_left: Width::ZERO,
            compress_right: Width::ZERO,
        }
    }

    fn boundary() -> Boundary {
        Boundary {
            breakable: Breakability::Allowed,
            kind: BoundaryKind::Normal,
            role: BoundaryRole::Plain,
            glue: Glue::default(),
            penalty: 0,
            pair_squeeze: Width::ZERO,
        }
    }

    fn paragraph() -> Paragraph {
        Paragraph {
            style: ParagraphStyle::default(),
            atoms: vec![atom("中"), atom("文")],
            boundaries: vec![boundary()],
        }
    }

    #[test]
    fn validates_minimal_paragraph() {
        assert_eq!(paragraph().validate(), Ok(()));
    }

    #[test]
    fn rejects_missing_boundary() {
        let mut para = paragraph();
        para.boundaries.clear();
        assert_eq!(
            para.validate(),
            Err(IrError::InvalidBoundaries {
                atoms: 2,
                boundaries: 0
            })
        );
    }

    #[test]
    fn rejects_empty_atom() {
        let mut para = paragraph();
        para.atoms[0] = atom("");
        assert_eq!(para.validate(), Err(IrError::EmptyAtom { index: 0 }));
    }

    #[test]
    fn rejects_negative_glue() {
        let mut para = paragraph();
        para.boundaries[0].glue.shrink = Width(-1);
        assert_eq!(para.validate(), Err(IrError::InvalidGlue { index: 0 }));
    }
}
