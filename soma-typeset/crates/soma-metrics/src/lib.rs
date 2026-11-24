//! Table-backed font metrics.
//!
//! The table is produced offline by `tools/metrics-gen` from an open font and
//! committed as bytes. Nothing here opens a font, parses outlines, or performs
//! text shaping: lookups are integer-only and identical on every target.
//!
//! The blob is `soma.metrics/7`:
//!
//! ```text
//! u32 magic ("SMT1") | u16 version | u16 unit | u16 face_count | u16 reserved
//! per face:
//!   u8 family | u8 weight | u8 slant | u8 reserved
//!   i32 ascent | i32 descent | i32 leading | i32 space | i32 default_advance
//!   u16 block_count | u16 reserved
//!   per block: u32 start | u32 end | i32 advance
//!   u32 entry_count
//!   per entry: u32 codepoint | i32 advance
//!   u32 ink_count
//!   per ink: u32 codepoint | i16 ink_left | i16 ink_right
//!   u32 glyph_ink_count                         (version 3+)
//!   per glyph: varint(codepoint delta) | u8 left | u8 right
//!   u16 math_constant_count                     (version 5+)
//!   per constant: i32 value in per-mille of one em
//!   u32 math_glyph_count                        (version 5+)
//!   per glyph: varint(delta) | i16 top | i16 bottom | i16 italic
//!   u16 math_variant_base_count                 (version 6+)
//!   per base: varint(codepoint delta) | u16 count
//!     per variant: i32 advance | i16 top | i16 bottom | i16 right_gap
//!   u32 math_top_accent_count                   (version 7+)
//!   per glyph: varint(codepoint delta) | i16 attachment
//! ```
//!
//! Version 5 adds the math face's OpenType `MATH` constants and every glyph's
//! vertical ink. Version 6 adds the vertical `MathVariants` of the math face:
//! the list order is the font's (increasing design size), it is addressed by
//! ordinal, and it is what `soma-math` uses instead of scaling a text-size
//! glyph (ADR-0075). Version 7 appends six constants (the two radical gaps,
//! the two display-style fraction gaps, `subscriptTopMax` and
//! `spaceAfterScript`) and the `MATH` top accent attachments, which align
//! combining accents (ADR-0076). A face without variants writes one zero
//! count.
//!
//! `top`/`bottom`/`italic` are 1/4096 em; `top` is positive
//! upwards from the baseline and `bottom` positive downwards. A face with no
//! math data writes two zero counts, so a normal text face pays eight bytes.
//!
//! Version 3 stored the codepoint as a `u32`; version 4 uses a delta from the
//! previous entry as a LEB128 varint. Adjacent Han glyphs cost one byte instead
//! of four, which halves the blob without changing a single measurement.
//!
//! `ink_left`/`ink_right` are the gaps between the glyph outline and its
//! advance box. Version 2 recorded them for punctuation only (ADR-0037);
//! version 3 records them for every glyph the face has, quantised to one byte
//! (16 units of 1/4096 em), because a line edge can land on any character
//! (ADR-0038). A version 1 blob parses too and reports no ink at all.
//!
//! All lengths are in `unit`ths of an em (4096). The blob's hash is part of the
//! canonical page result, so a table change is visible in `metricsHash`.

#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]

extern crate alloc;

use alloc::vec::Vec;
use core::fmt;

pub const MAGIC: u32 = 0x534d_5431;
pub const VERSION: u16 = 7;
/// Oldest blob layout this parser still reads.
pub const VERSION_MIN: u16 = 1;
pub const UNIT: i32 = 4096;

/// Face family codes in the blob.
pub const FAMILY_SERIF: u8 = 0;
pub const FAMILY_SANS: u8 = 1;
pub const FAMILY_MATH: u8 = 2;
/// A rescue face: used when the style's face has no glyph for a character.
pub const FAMILY_FALLBACK: u8 = 3;
/// A monospaced text face (ADR-0089). Old blobs simply have none.
pub const FAMILY_MONO: u8 = 4;

const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// One contiguous codepoint range with a uniform advance.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FaceBlock {
    pub start: u32,
    pub end: u32,
    pub advance: i32,
}

/// One vertical variant of a math glyph, in 1/4096 em at the face's design
/// size. `advance` is horizontal (the cursor step); `above`/`below` are the
/// glyph's ink; `right_gap` is `advance - ink right`, clamped at zero.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MathVariant {
    pub advance: i32,
    pub above: i32,
    pub below: i32,
    pub right_gap: i32,
}

/// Math data from a face's OpenType `MATH` table.
///
/// `constants` are per-mille of one em in the order of
/// `soma_core::MathParam`. `glyphs` are
/// `(codepoint, top, bottom, italic)`, all in 1/4096 em, sorted by codepoint.
/// `variants` maps a base codepoint to its vertical `MathVariants`, in the
/// font's order (increasing design size). `top_accent` maps a codepoint to its
/// `MathTopAccentAttachment`, also 1/4096 em.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MathFace {
    constants: Vec<i32>,
    glyphs: Vec<(u32, i32, i32, i32)>,
    variants: Vec<(u32, Vec<MathVariant>)>,
    top_accent: Vec<(u32, i32)>,
}

impl MathFace {
    /// One constant in per-mille of one em. `None` when the table has fewer
    /// constants than the caller asked for (a text face, or an older blob).
    pub fn constant(&self, index: usize) -> Option<i32> {
        self.constants.get(index).copied()
    }

    /// Vertical ink and italic correction of one glyph, in 1/4096 em.
    pub fn glyph(&self, codepoint: u32) -> Option<(i32, i32, i32)> {
        self.glyphs
            .binary_search_by_key(&codepoint, |(cp, _, _, _)| *cp)
            .ok()
            .map(|index| {
                let (_, top, bottom, italic) = self.glyphs[index];
                (top, bottom, italic)
            })
    }

    pub fn constant_count(&self) -> usize {
        self.constants.len()
    }

    /// Vertical variants of the first character of `text`, in the font's order.
    /// Empty when the face has none (a text face, an older blob, or a glyph
    /// without design variants).
    pub fn variants(&self, codepoint: u32) -> &[MathVariant] {
        match self
            .variants
            .binary_search_by_key(&codepoint, |(cp, _)| *cp)
        {
            Ok(index) => &self.variants[index].1,
            Err(_) => &[],
        }
    }

    /// Number of base codepoints with variants (diagnostics).
    pub fn variant_base_count(&self) -> usize {
        self.variants.len()
    }

    /// Top accent attachment of a glyph, in 1/4096 em at the design size.
    /// `None` when the face has no `MATH` attachment for it.
    pub fn top_accent(&self, codepoint: u32) -> Option<i32> {
        self.top_accent
            .binary_search_by_key(&codepoint, |(cp, _)| *cp)
            .ok()
            .map(|index| self.top_accent[index].1)
    }

    pub fn glyph_count(&self) -> usize {
        self.glyphs.len()
    }

    /// Codepoint of the nth glyph row. Rows are sorted by codepoint, so this
    /// walks the face from Latin through symbols to the CJK rescue glyphs.
    pub fn glyph_at(&self, index: usize) -> Option<u32> {
        self.glyphs
            .get(index)
            .map(|(codepoint, _, _, _)| *codepoint)
    }

    pub fn is_empty(&self) -> bool {
        self.constants.is_empty()
            && self.glyphs.is_empty()
            && self.variants.is_empty()
            && self.top_accent.is_empty()
    }
}

/// One face: metrics for a family/weight/slant combination.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Face {
    pub family: u8,
    pub weight: u8,
    pub slant: u8,
    pub ascent: i32,
    pub descent: i32,
    pub leading: i32,
    pub space: i32,
    pub default_advance: i32,
    blocks: Vec<FaceBlock>,
    entries: Vec<(u32, i32)>,
    /// Punctuation-only `(codepoint, ink_left, ink_right)` sorted by codepoint.
    /// Empty in a version 1 blob.
    ink: Vec<(u32, i32, i32)>,
    /// Every glyph's `(codepoint, ink_left, ink_right)` sorted by codepoint,
    /// dequantised from one byte each. Empty before version 3.
    glyph_ink: Vec<(u32, i32, i32)>,
    /// Math constants and glyph ink. Empty before version 5.
    math: MathFace,
}

impl Face {
    /// Whether the face has a glyph for the codepoint.
    ///
    /// The version 3 glyph list is authoritative: a uniform block describes a
    /// range whose advances agree, not a promise that every codepoint in the
    /// range exists (extension B is sparsely covered even in a large face).
    /// Version 1 and 2 blobs fall back to blocks and can over-report.
    pub fn has(&self, codepoint: u32) -> bool {
        if !self.glyph_ink.is_empty() {
            return self
                .glyph_ink
                .binary_search_by_key(&codepoint, |(cp, _, _)| *cp)
                .is_ok();
        }
        if self
            .entries
            .binary_search_by_key(&codepoint, |(cp, _)| *cp)
            .is_ok()
        {
            return true;
        }
        self.blocks
            .iter()
            .any(|block| codepoint >= block.start && codepoint <= block.end)
    }

    /// Whether every character of `text` has a glyph.
    pub fn covers(&self, text: &str) -> bool {
        text.chars().all(|character| self.has(character as u32))
    }

    /// Advance of one codepoint, in 1/4096 em.
    pub fn advance_at(&self, codepoint: u32) -> i32 {
        self.advance_recorded(codepoint)
            .unwrap_or(self.default_advance)
    }

    /// Advance of one codepoint when the table records it explicitly (an
    /// entry or a uniform block). `None` when [`Self::advance_at`] would fall
    /// back to the face-wide default.
    ///
    /// The generator records proportional scripts up to U+2FFF and the
    /// mathematical alphanumeric block; a sparse math codepoint outside those
    /// ranges has no width in the table, so a font check must skip it rather
    /// than compare it with 4096 (ADR-0077).
    pub fn advance_recorded(&self, codepoint: u32) -> Option<i32> {
        if let Ok(index) = self.entries.binary_search_by_key(&codepoint, |(cp, _)| *cp) {
            return Some(self.entries[index].1);
        }
        self.blocks
            .iter()
            .find(|block| codepoint >= block.start && codepoint <= block.end)
            .map(|block| block.advance)
    }

    /// Whether a candidate face's advances agree with this face's math metrics.
    ///
    /// `advance` returns the candidate's advance in 1/4096 em for a codepoint,
    /// or `None` when it has no such glyph. Math rows are sampled evenly across
    /// the codepoint-sorted list, a codepoint the table does not record
    /// explicitly is skipped, and at least 16 comparisons must agree exactly.
    ///
    /// This is how the CLI recognises the installed face a table was measured
    /// from (ADR-0077). A name is not a metric: a different math face resolves
    /// the same variant ordinals at other sizes, and a subset build keeps the
    /// family name.
    pub fn math_matches<F>(&self, advance: F) -> bool
    where
        F: Fn(u32) -> Option<i32>,
    {
        let rows = self.math.glyph_count();
        if rows < 16 {
            return false;
        }
        let sample = rows.min(48);
        let mut compared = 0usize;
        for index in 0..sample {
            let at = if sample == 1 {
                0
            } else {
                index * (rows - 1) / (sample - 1)
            };
            let Some(codepoint) = self.math.glyph_at(at) else {
                continue;
            };
            if !self.has(codepoint) {
                continue;
            }
            let Some(expected) = self.advance_recorded(codepoint) else {
                continue;
            };
            let Some(actual) = advance(codepoint) else {
                continue;
            };
            if expected != actual {
                return false;
            }
            compared += 1;
        }
        compared >= 16
    }

    /// Sum of advances, in 1/4096 em.
    pub fn advance(&self, text: &str) -> i32 {
        let mut total: i64 = 0;
        for c in text.chars() {
            total += self.advance_at(c as u32) as i64;
        }
        saturating(total)
    }

    /// Number of explicitly stored codepoints (diagnostics).
    pub fn entry_count(&self) -> usize {
        self.entries.len()
    }

    /// Gaps between a glyph outline and its advance box, in 1/4096 em:
    /// `(left, right)`. `None` when the table has no outline data for the
    /// codepoint (including every face in a version 1 blob).
    pub fn ink(&self, codepoint: u32) -> Option<(i32, i32)> {
        if let Ok(index) = self
            .glyph_ink
            .binary_search_by_key(&codepoint, |(cp, _, _)| *cp)
        {
            return Some((self.glyph_ink[index].1, self.glyph_ink[index].2));
        }
        self.ink
            .binary_search_by_key(&codepoint, |(cp, _, _)| *cp)
            .ok()
            .map(|index| (self.ink[index].1, self.ink[index].2))
    }

    /// Gap between the glyph outline and the right edge of its advance.
    pub fn ink_right(&self, codepoint: u32) -> Option<i32> {
        self.ink(codepoint).map(|(_, right)| right)
    }

    /// Number of marks with a version 2 ink entry (diagnostics).
    pub fn ink_count(&self) -> usize {
        self.ink.len()
    }

    /// Number of glyphs with a version 3 entry (diagnostics).
    pub fn glyph_ink_count(&self) -> usize {
        self.glyph_ink.len()
    }

    /// Number of uniform blocks (diagnostics).
    pub fn block_count(&self) -> usize {
        self.blocks.len()
    }

    /// The face's math constants and glyph ink. Empty before version 5 and for
    /// every text face.
    pub fn math(&self) -> &MathFace {
        &self.math
    }

    /// True when the face was built for a bold weight.
    pub fn is_bold(&self) -> bool {
        self.weight != 0
    }
}

/// A parsed `soma.metrics/1` table.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MetricsTable {
    unit: i32,
    faces: Vec<Face>,
    hash: u64,
}

impl MetricsTable {
    /// Parse a blob. Every read is bounds-checked.
    pub fn parse(bytes: &[u8]) -> Result<Self, MetricsError> {
        let mut cursor = Cursor::new(bytes);
        let magic = cursor.u32()?;
        if magic != MAGIC {
            return Err(MetricsError::new("bad magic"));
        }
        let version = cursor.u16()?;
        if !(VERSION_MIN..=VERSION).contains(&version) {
            return Err(MetricsError::new("unsupported metrics version"));
        }
        let unit = cursor.u16()? as i32;
        if unit <= 0 {
            return Err(MetricsError::new("unit must be positive"));
        }
        let face_count = cursor.u16()? as usize;
        let _reserved = cursor.u16()?;

        let mut faces = Vec::with_capacity(face_count);
        for _ in 0..face_count {
            let family = cursor.u8()?;
            let weight = cursor.u8()?;
            let slant = cursor.u8()?;
            let _reserved = cursor.u8()?;
            let ascent = cursor.i32()?;
            let descent = cursor.i32()?;
            let leading = cursor.i32()?;
            let space = cursor.i32()?;
            let default_advance = cursor.i32()?;
            let block_count = cursor.u16()? as usize;
            let _reserved = cursor.u16()?;
            let mut blocks = Vec::with_capacity(block_count);
            for _ in 0..block_count {
                let start = cursor.u32()?;
                let end = cursor.u32()?;
                let advance = cursor.i32()?;
                if end < start {
                    return Err(MetricsError::new("block range is inverted"));
                }
                blocks.push(FaceBlock {
                    start,
                    end,
                    advance,
                });
            }
            let entry_count = cursor.u32()? as usize;
            let mut entries = Vec::with_capacity(entry_count.min(1 << 20));
            let mut previous = 0u32;
            for index in 0..entry_count {
                let codepoint = cursor.u32()?;
                let advance = cursor.i32()?;
                if index > 0 && codepoint <= previous {
                    return Err(MetricsError::new("entries are not sorted"));
                }
                previous = codepoint;
                entries.push((codepoint, advance));
            }
            let mut ink = Vec::new();
            if version >= 2 {
                let ink_count = cursor.u32()? as usize;
                ink.reserve(ink_count.min(1 << 16));
                let mut previous = 0u32;
                for index in 0..ink_count {
                    let codepoint = cursor.u32()?;
                    let left = cursor.i16()? as i32;
                    let right = cursor.i16()? as i32;
                    if index > 0 && codepoint <= previous {
                        return Err(MetricsError::new("ink entries are not sorted"));
                    }
                    previous = codepoint;
                    ink.push((codepoint, left, right));
                }
            }
            let mut glyph_ink = Vec::new();
            if version >= 3 {
                let count = cursor.u32()? as usize;
                glyph_ink.reserve(count.min(1 << 18));
                let mut previous = 0u32;
                for index in 0..count {
                    let codepoint = if version >= 4 {
                        previous
                            .checked_add(cursor.varint()?)
                            .ok_or(MetricsError::new("codepoint delta overflows"))?
                    } else {
                        cursor.u32()?
                    };
                    let left = dequantise(cursor.u8()?);
                    let right = dequantise(cursor.u8()?);
                    if index > 0 && codepoint <= previous {
                        return Err(MetricsError::new("glyph ink entries are not sorted"));
                    }
                    previous = codepoint;
                    glyph_ink.push((codepoint, left, right));
                }
            }
            let mut math = MathFace::default();
            if version >= 5 {
                let constant_count = cursor.u16()? as usize;
                math.constants.reserve(constant_count.min(256));
                for _ in 0..constant_count {
                    math.constants.push(cursor.i32()?);
                }
                let glyph_count = cursor.u32()? as usize;
                math.glyphs.reserve(glyph_count.min(1 << 18));
                let mut previous = 0u32;
                for index in 0..glyph_count {
                    let codepoint = previous
                        .checked_add(cursor.varint()?)
                        .ok_or(MetricsError::new("math codepoint delta overflows"))?;
                    let top = cursor.i16()? as i32;
                    let bottom = cursor.i16()? as i32;
                    let italic = cursor.i16()? as i32;
                    if index > 0 && codepoint <= previous {
                        return Err(MetricsError::new("math glyph entries are not sorted"));
                    }
                    previous = codepoint;
                    math.glyphs.push((codepoint, top, bottom, italic));
                }
            }
            if version >= 6 {
                let count = cursor.u16()? as usize;
                math.variants.reserve(count.min(1 << 12));
                let mut previous = 0u32;
                for index in 0..count {
                    let codepoint = previous
                        .checked_add(cursor.varint()?)
                        .ok_or(MetricsError::new("math variant delta overflows"))?;
                    let rows = cursor.u16()? as usize;
                    let mut variants = Vec::with_capacity(rows.min(1 << 10));
                    for _ in 0..rows {
                        variants.push(MathVariant {
                            advance: cursor.i32()?,
                            above: cursor.i16()? as i32,
                            below: cursor.i16()? as i32,
                            right_gap: cursor.i16()? as i32,
                        });
                    }
                    if index > 0 && codepoint <= previous {
                        return Err(MetricsError::new("math variant entries are not sorted"));
                    }
                    previous = codepoint;
                    math.variants.push((codepoint, variants));
                }
            }
            if version >= 7 {
                let count = cursor.u32()? as usize;
                math.top_accent.reserve(count.min(1 << 16));
                let mut previous = 0u32;
                for index in 0..count {
                    let codepoint = previous
                        .checked_add(cursor.varint()?)
                        .ok_or(MetricsError::new("top accent delta overflows"))?;
                    let value = cursor.i16()? as i32;
                    if index > 0 && codepoint <= previous {
                        return Err(MetricsError::new("top accent entries are not sorted"));
                    }
                    previous = codepoint;
                    math.top_accent.push((codepoint, value));
                }
            }
            faces.push(Face {
                family,
                weight,
                slant,
                ascent,
                descent,
                leading,
                space,
                default_advance,
                blocks,
                entries,
                ink,
                glyph_ink,
                math,
            });
        }
        if cursor.remaining() != 0 {
            return Err(MetricsError::new("trailing bytes after the last face"));
        }

        Ok(Self {
            unit,
            faces,
            hash: fnv(bytes),
        })
    }

    /// The committed table.
    pub fn embedded() -> Self {
        let bytes: &'static [u8] = include_bytes!("../data/noto-cjk.smt");
        Self::parse(bytes).expect("the embedded metrics table must parse")
    }

    pub fn unit(&self) -> i32 {
        self.unit
    }

    pub fn face(&self, index: usize) -> Option<&Face> {
        self.faces.get(index)
    }

    /// The dedicated math face (family `2`), if the table has one.
    ///
    /// Formula symbols (operators, radicals, Greek) come from this face so a
    /// `∑` or `√` has its real advance instead of the body font's fallback.
    pub fn math_face(&self) -> Option<&Face> {
        self.faces.iter().find(|face| face.family == FAMILY_MATH)
    }

    pub fn face_count(&self) -> usize {
        self.faces.len()
    }

    pub fn faces(&self) -> &[Face] {
        &self.faces
    }

    /// Content hash of the table bytes (`metricsHash` in the page result).
    pub fn hash(&self) -> u64 {
        self.hash
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MetricsError {
    reason: &'static str,
}

impl MetricsError {
    const fn new(reason: &'static str) -> Self {
        Self { reason }
    }

    pub fn reason(self) -> &'static str {
        self.reason
    }
}

impl fmt::Display for MetricsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.reason)
    }
}

/// One byte holds a bearing with 16 units of 1/4096 em of resolution.
fn dequantise(value: u8) -> i32 {
    (value as i32 * 4096 + 127) / 255
}

struct Cursor<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Cursor<'a> {
    const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, at: 0 }
    }

    fn take(&mut self, count: usize) -> Result<&'a [u8], MetricsError> {
        let end = self
            .at
            .checked_add(count)
            .ok_or(MetricsError::new("offset overflow"))?;
        let slice = self
            .bytes
            .get(self.at..end)
            .ok_or(MetricsError::new("truncated table"))?;
        self.at = end;
        Ok(slice)
    }

    fn u8(&mut self) -> Result<u8, MetricsError> {
        Ok(self.take(1)?[0])
    }

    fn u16(&mut self) -> Result<u16, MetricsError> {
        let bytes = self.take(2)?;
        Ok(u16::from_le_bytes([bytes[0], bytes[1]]))
    }

    fn i16(&mut self) -> Result<i16, MetricsError> {
        Ok(self.u16()? as i16)
    }

    fn u32(&mut self) -> Result<u32, MetricsError> {
        let bytes = self.take(4)?;
        Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    fn i32(&mut self) -> Result<i32, MetricsError> {
        Ok(self.u32()? as i32)
    }

    fn varint(&mut self) -> Result<u32, MetricsError> {
        let mut value = 0u32;
        let mut shift = 0u32;
        loop {
            let byte = self.u8()?;
            value |= ((byte & 0x7f) as u32) << shift;
            if byte & 0x80 == 0 {
                return Ok(value);
            }
            shift += 7;
            if shift >= 35 {
                return Err(MetricsError::new("varint is too long"));
            }
        }
    }

    fn remaining(&self) -> usize {
        self.bytes.len() - self.at
    }
}

fn fnv(bytes: &[u8]) -> u64 {
    let mut state = FNV_OFFSET;
    for byte in bytes {
        state ^= *byte as u64;
        state = state.wrapping_mul(FNV_PRIME);
    }
    state
}

fn saturating(value: i64) -> i32 {
    if value > i32::MAX as i64 {
        i32::MAX
    } else if value < i32::MIN as i64 {
        i32::MIN
    } else {
        value as i32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table() -> MetricsTable {
        MetricsTable::embedded()
    }

    fn write_varint(out: &mut Vec<u8>, value: u32) {
        let mut remaining = value;
        loop {
            let byte = (remaining & 0x7f) as u8;
            remaining >>= 7;
            if remaining == 0 {
                out.push(byte);
                break;
            }
            out.push(byte | 0x80);
        }
    }

    #[test]
    fn embedded_table_never_claims_a_codepoint_it_lacks() {
        // A block is both a width and a coverage claim, so a face without a
        // per-glyph list must split its blocks at holes (ADR-0074). Noto CJK SC
        // has no U+2A6A5: if the extension-B block swallowed it, the rescue face
        // would be skipped and the Character would render as .notdef.
        let table = MetricsTable::embedded();
        let cjk: Vec<_> = table
            .faces()
            .iter()
            .filter(|face| face.has(0x4e00))
            .collect();
        assert_eq!(cjk.len(), 4, "serif/sans, regular/bold");
        for face in cjk {
            assert!(!face.has(0x2a6a5), "no face claims the U+2A6A5 hole");
            assert!(
                face.ink(0xff0c).is_some(),
                "the full-width comma carries ink"
            );
        }
    }

    #[test]
    fn embedded_table_has_regular_and_bold_faces() {
        let table = table();
        assert_eq!(table.unit(), UNIT);
        assert_eq!(table.face_count(), 5);
        assert_eq!(table.face(0).unwrap().family, FAMILY_SERIF);
        assert_eq!(table.face(1).unwrap().family, FAMILY_SANS);
        assert!(!table.face(0).unwrap().is_bold());
        assert!(table.face(2).unwrap().is_bold());
        assert!(table.face(3).unwrap().is_bold());
        assert_eq!(table.face(2).unwrap().family, FAMILY_SERIF);
        assert_eq!(table.face(3).unwrap().family, FAMILY_SANS);
        assert_eq!(table.face(4).unwrap().family, FAMILY_MATH);
        assert!(table.face(5).is_none());
    }

    #[test]
    fn the_math_face_covers_operators_and_radicals() {
        let table = table();
        let math = table.math_face().expect("math face");
        // Latin Modern Math sets the summation sign and the radical wider than
        // one em, which is what makes formula widths right.
        assert!(math.advance_at('∑' as u32) > 0, "summation");
        assert!(math.advance_at('√' as u32) > 0, "radical");
        assert!(math.advance_at('∫' as u32) > 0, "integral");
        assert!(math.advance_at('α' as u32) > 0, "greek");
        assert!(math.ascent > 0 && math.descent >= 0);
    }

    #[test]
    fn bold_latin_differs_from_regular() {
        let table = table();
        let regular = table.face(0).unwrap();
        let bold = table.face(2).unwrap();
        assert!(bold.advance_at('W' as u32) >= regular.advance_at('W' as u32));
        assert_ne!(
            bold.advance("Soma typeset"),
            regular.advance("Soma typeset"),
            "the bold face must have its own advances"
        );
    }

    #[test]
    fn cjk_ideographs_are_full_width() {
        let face = table();
        let face = face.face(0).unwrap();
        assert_eq!(face.advance_at('中' as u32), UNIT);
        assert_eq!(face.advance_at('。' as u32), UNIT);
        assert_eq!(face.advance("中文"), 2 * UNIT);
    }

    #[test]
    fn latin_advances_are_proportional() {
        let table = table();
        let face = table.face(0).unwrap();
        let narrow = face.advance_at('i' as u32);
        let wide = face.advance_at('W' as u32);
        assert!(narrow > 0 && narrow < UNIT);
        assert!(wide > narrow, "{wide} should be wider than {narrow}");
        assert!(wide <= UNIT * 5 / 4, "W advance {wide} exceeds 1.25 em");
    }

    #[test]
    fn space_matches_the_space_codepoint() {
        let table = table();
        for face in table.faces() {
            assert_eq!(face.space, face.advance_at(' ' as u32));
        }
    }

    #[test]
    fn a_version_two_blob_carries_punctuation_ink() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&MAGIC.to_le_bytes());
        bytes.extend_from_slice(&2u16.to_le_bytes());
        bytes.extend_from_slice(&(UNIT as u16).to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes.extend_from_slice(&[FAMILY_SERIF, 0, 0, 0]);
        for value in [UNIT * 4 / 5, UNIT / 5, 0, UNIT / 4, UNIT] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(&2u32.to_le_bytes());
        for (cp, left, right) in [(0x3002u32, 128i16, 3072i16), (0xFF0C, 200, 2900)] {
            bytes.extend_from_slice(&cp.to_le_bytes());
            bytes.extend_from_slice(&left.to_le_bytes());
            bytes.extend_from_slice(&right.to_le_bytes());
        }

        let table = MetricsTable::parse(&bytes).expect("v2 blob parses");
        assert_eq!(table.face_count(), 1);
        let face = table.face(0).expect("face");
        assert_eq!(face.ink_count(), 2);
        assert_eq!(face.ink(0x3002), Some((128, 3072)));
        assert_eq!(face.ink_right(0xFF0C), Some(2900));
        assert_eq!(face.ink(0x4E2D), None, "Han characters carry no ink entry");
    }

    #[test]
    fn a_version_one_blob_reports_no_ink() {
        let bytes = [
            0x31, 0x54, 0x4d, 0x53, // "SMT1"
            0x01, 0x00, // version 1
            0x00, 0x10, // unit 4096
            0x01, 0x00, // one face
            0x00, 0x00, // reserved
            0x00, 0x00, 0x00, 0x00, // family/weight/slant/reserved
        ];
        // Extend with the five i32 metrics and an empty block/entry section.
        let mut bytes = bytes.to_vec();
        for value in [UNIT * 4 / 5, UNIT / 5, 0, UNIT / 4, UNIT] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes());

        let table = MetricsTable::parse(&bytes).expect("v1 blob still parses");
        let face = table.face(0).expect("face");
        assert_eq!(face.ink_count(), 0);
        assert_eq!(face.ink_right(0x3002), None);
    }

    #[test]
    fn unsorted_ink_is_rejected() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&MAGIC.to_le_bytes());
        bytes.extend_from_slice(&2u16.to_le_bytes());
        bytes.extend_from_slice(&(UNIT as u16).to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes.extend_from_slice(&[FAMILY_SERIF, 0, 0, 0]);
        for value in [UNIT * 4 / 5, UNIT / 5, 0, UNIT / 4, UNIT] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(&2u32.to_le_bytes());
        for (cp, left, right) in [(0xFF0Cu32, 200i16, 2900i16), (0x3002, 128, 3072)] {
            bytes.extend_from_slice(&cp.to_le_bytes());
            bytes.extend_from_slice(&left.to_le_bytes());
            bytes.extend_from_slice(&right.to_le_bytes());
        }
        assert!(MetricsTable::parse(&bytes).is_err());
    }

    #[test]
    fn a_version_five_blob_carries_math_constants_and_glyph_ink() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&MAGIC.to_le_bytes());
        bytes.extend_from_slice(&5u16.to_le_bytes());
        bytes.extend_from_slice(&(UNIT as u16).to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes.extend_from_slice(&[FAMILY_MATH, 0, 0, 0]);
        for value in [UNIT, 0, 0, 0, UNIT] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes());
        // Three constants: axis height 250, rule 40, script 700 (per-mille).
        bytes.extend_from_slice(&3u16.to_le_bytes());
        for value in [250i32, 40, 700] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        // Two glyphs: x and π, with vertical ink and an italic correction.
        bytes.extend_from_slice(&2u32.to_le_bytes());
        for (cp, top, bottom, italic) in [(0x78u32, 2600i16, 0i16, -80i16), (0x3c0, 2400, 0, 0)] {
            let delta = cp - if cp == 0x78 { 0 } else { 0x78 };
            write_varint(&mut bytes, delta);
            bytes.extend_from_slice(&top.to_le_bytes());
            bytes.extend_from_slice(&bottom.to_le_bytes());
            bytes.extend_from_slice(&italic.to_le_bytes());
        }

        let table = MetricsTable::parse(&bytes).expect("v5 blob parses");
        let face = table.face(0).expect("face");
        assert_eq!(face.math().constant_count(), 3);
        assert_eq!(face.math().constant(0), Some(250));
        assert_eq!(face.math().constant(2), Some(700));
        assert_eq!(face.math().constant(3), None);
        assert_eq!(face.math().glyph(0x78), Some((2600, 0, -80)));
        assert_eq!(face.math().glyph(0x3c0), Some((2400, 0, 0)));
        assert_eq!(face.math().glyph(0x3b1), None);
    }

    #[test]
    fn a_version_four_blob_has_no_math_data() {
        // Version 4 ends after the per-glyph ink list; only version 5 appends
        // the `MATH` constants. A parser that read that far anyway would see
        // the next face's header as math data.
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&MAGIC.to_le_bytes());
        bytes.extend_from_slice(&4u16.to_le_bytes());
        bytes.extend_from_slice(&(UNIT as u16).to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes.extend_from_slice(&[FAMILY_SERIF, 0, 0, 0]);
        for value in [UNIT * 4 / 5, UNIT / 5, 0, UNIT / 4, UNIT] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes.extend_from_slice(&0u16.to_le_bytes()); // blocks
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes()); // entries
        bytes.extend_from_slice(&0u32.to_le_bytes()); // punctuation ink
        bytes.extend_from_slice(&0u32.to_le_bytes()); // per-glyph ink

        let table = MetricsTable::parse(&bytes).expect("v4 blob parses");
        assert_eq!(table.face_count(), 1);
        assert!(table.faces()[0].math().is_empty());
    }

    #[test]
    fn unknown_codepoints_fall_back_to_the_default() {
        let table = table();
        let face = table.face(0).unwrap();
        assert_eq!(face.advance_at(0x10_fffd), face.default_advance);
    }

    #[test]
    fn hash_is_stable_and_content_addressed() {
        let first = table().hash();
        let second = table().hash();
        assert_eq!(first, second);

        // A second, committed table: the hash must separate content, not paths.
        let other: &[u8] = include_bytes!("../../../examples/book/metrics-lxgw.smt");
        let other = MetricsTable::parse(other).expect("lxgw table");
        assert_ne!(first, other.hash());
    }

    #[test]
    fn truncated_blobs_are_rejected() {
        let bytes: &[u8] = include_bytes!("../data/noto-cjk.smt");
        for cut in [0, 3, 7, 15, 31] {
            assert!(MetricsTable::parse(&bytes[..cut.min(bytes.len())]).is_err());
        }
        assert!(MetricsTable::parse(bytes).is_ok());
    }

    #[test]
    fn every_face_has_plausible_line_metrics() {
        for face in table().faces() {
            assert!(face.ascent > 0);
            assert!(face.descent > 0);
            assert!(face.leading >= 0);
            // The math face gets its extents from Latin Modern Math; body faces
            // are normalised to 1em.
            let slack = if face.family == FAMILY_MATH {
                UNIT
            } else {
                UNIT / 4
            };
            assert!(
                (face.ascent + face.descent + face.leading - UNIT).abs() < slack,
                "face {} extents {}/{}/{} (slack {slack})",
                face.family,
                face.ascent,
                face.descent,
                face.leading
            );
            assert!(face.entry_count() > 100);
            assert!(face.block_count() >= if face.family == FAMILY_MATH { 0 } else { 4 });
        }
    }

    #[test]
    fn a_version_six_blob_carries_math_variants() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&MAGIC.to_le_bytes());
        bytes.extend_from_slice(&6u16.to_le_bytes());
        bytes.extend_from_slice(&(UNIT as u16).to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes.extend_from_slice(&[FAMILY_MATH, 0, 0, 0]);
        for value in [UNIT * 4 / 5, UNIT / 5, 0, UNIT / 4, UNIT] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes.extend_from_slice(&0u16.to_le_bytes()); // blocks
        bytes.extend_from_slice(&0u16.to_le_bytes()); // reserved
        bytes.extend_from_slice(&0u32.to_le_bytes()); // entries
        bytes.extend_from_slice(&0u32.to_le_bytes()); // v2 ink
        bytes.extend_from_slice(&0u32.to_le_bytes()); // v3 glyph ink
        bytes.extend_from_slice(&0u16.to_le_bytes()); // constants
        bytes.extend_from_slice(&0u32.to_le_bytes()); // math glyphs
                                                      // Two bases; the deltas are relative.
        bytes.extend_from_slice(&2u16.to_le_bytes());
        for (cp, rows) in [
            (
                0x2211u32,
                [(954i32, 714i16, 240i16, 0i16), (1336, 905, 431, 0)],
            ),
            (0x221A, [(1057, 1057, 0, 156), (1419, 1419, 0, 578)]),
        ] {
            let mut previous = if cp == 0x2211 { 0 } else { 0x2211 };
            write_varint(&mut bytes, cp - previous);
            previous = cp;
            let _ = previous;
            bytes.extend_from_slice(&(rows.len() as u16).to_le_bytes());
            for (advance, top, bottom, right_gap) in rows {
                bytes.extend_from_slice(&advance.to_le_bytes());
                bytes.extend_from_slice(&top.to_le_bytes());
                bytes.extend_from_slice(&bottom.to_le_bytes());
                bytes.extend_from_slice(&right_gap.to_le_bytes());
            }
        }

        let table = MetricsTable::parse(&bytes).expect("v6 blob parses");
        let face = table.face(0).expect("face");
        assert_eq!(face.math().variant_base_count(), 2);
        let sum = face.math().variants(0x2211);
        assert_eq!(sum.len(), 2);
        assert_eq!(sum[1].advance, 1336);
        assert_eq!(sum[1].above, 905);
        assert_eq!(sum[1].below, 431);
        let radical = face.math().variants(0x221A);
        assert_eq!(radical[1].right_gap, 578, "the bar starts at the ink edge");
        assert!(face.math().variants(0x222B).is_empty());
    }

    #[test]
    fn the_embedded_math_face_carries_the_v7_constants() {
        let table = table();
        let math = table.math_face().expect("math face").math();
        assert_eq!(math.constant_count(), 41);
        for index in 35..=40 {
            assert!(
                math.constant(index).is_some_and(|value| value >= 0),
                "constant {index}"
            );
        }
    }

    #[test]
    fn the_embedded_math_face_carries_accent_attachments() {
        let table = table();
        let math = table.math_face().expect("math face");
        // The mark's attachment is signed: Latin Modern places the combining
        // circumflex to the left of its origin, so only presence is required.
        assert!(math.math().top_accent(0x0302).is_some());
        let x = math.math().top_accent('x' as u32);
        assert!(x.is_some_and(|value| value > 0));
        assert!(
            math.math().top_accent(0x2211).is_none(),
            "a large operator is not an accent base"
        );
    }

    #[test]
    fn the_embedded_math_face_has_design_variants() {
        let table = table();
        let math = table.math_face().expect("math face");
        let sum = math.math().variants('∑' as u32);
        assert!(sum.len() >= 2, "a display sum has a larger design glyph");
        assert!(
            sum.windows(2)
                .all(|pair| pair[0].above + pair[0].below <= pair[1].above + pair[1].below),
            "variants are ordered by increasing height"
        );
        let radical = math.math().variants('√' as u32);
        assert!(radical.len() >= 3, "a radical has several designs");
        assert!(radical[0].advance > 0 && radical[0].above > 0);
        assert!(
            math.math().variants('x' as u32).is_empty(),
            "letters have no vertical variants"
        );
    }

    #[test]
    fn math_matches_accepts_the_face_its_advances_come_from() {
        let table = table();
        let face = table.math_face().expect("math face");
        assert!(face.math_matches(|codepoint| face.advance_recorded(codepoint)));
    }

    #[test]
    fn math_matches_rejects_a_single_wrong_advance() {
        let table = table();
        let face = table.math_face().expect("math face");
        let rows = face.math().glyph_count();
        let sample = rows.min(48);
        let target = (0..sample)
            .find_map(|index| {
                let at = index * (rows - 1) / (sample - 1);
                let codepoint = face.math().glyph_at(at)?;
                face.advance_recorded(codepoint).map(|_| codepoint)
            })
            .expect("a recorded sample");
        assert!(!face.math_matches(|codepoint| {
            let value = face.advance_recorded(codepoint);
            if codepoint == target {
                value.map(|advance| advance + 1)
            } else {
                value
            }
        }));
    }

    #[test]
    fn math_matches_rejects_a_face_without_math_metrics() {
        let table = table();
        let face = table.face(0).expect("body face");
        assert!(!face.math_matches(|_| Some(4096)));
        let math = table.math_face().expect("math face");
        assert!(!math.math_matches(|_| None));
    }
}
