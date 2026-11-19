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
}

/// One unbreakable unit of ink.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Atom {
    pub text: String,
    pub kind: AtomKind,
    pub class: PunctClass,
    pub style: StyleId,
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
    pub line_height: Width,
    pub align: Align,
}

impl Default for ParagraphStyle {
    fn default() -> Self {
        Self {
            width: Width::from_em(40),
            first_indent: Width::from_em(2),
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
