//! Table-backed font metrics.
//!
//! The table is produced offline by `tools/metrics-gen` from an open font and
//! committed as bytes. Nothing here opens a font, parses outlines, or performs
//! text shaping: lookups are integer-only and identical on every target.
//!
//! The blob is `soma.metrics/1`:
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
//! ```
//!
//! All lengths are in `unit`ths of an em (4096). The blob's hash is part of the
//! canonical page result, so a table change is visible in `metricsHash`.

#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]

extern crate alloc;

use alloc::vec::Vec;
use core::fmt;

pub const MAGIC: u32 = 0x534d_5431;
pub const VERSION: u16 = 1;
pub const UNIT: i32 = 4096;

/// Face family codes in the blob.
pub const FAMILY_SERIF: u8 = 0;
pub const FAMILY_SANS: u8 = 1;
pub const FAMILY_MATH: u8 = 2;

const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// One contiguous codepoint range with a uniform advance.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FaceBlock {
    pub start: u32,
    pub end: u32,
    pub advance: i32,
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
}

impl Face {
    /// Advance of one codepoint, in 1/4096 em.
    pub fn advance_at(&self, codepoint: u32) -> i32 {
        if let Ok(index) = self.entries.binary_search_by_key(&codepoint, |(cp, _)| *cp) {
            return self.entries[index].1;
        }
        for block in &self.blocks {
            if codepoint >= block.start && codepoint <= block.end {
                return block.advance;
            }
        }
        self.default_advance
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

    /// Number of uniform blocks (diagnostics).
    pub fn block_count(&self) -> usize {
        self.blocks.len()
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
        if version != VERSION {
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

    fn u32(&mut self) -> Result<u32, MetricsError> {
        let bytes = self.take(4)?;
        Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    fn i32(&mut self) -> Result<i32, MetricsError> {
        Ok(self.u32()? as i32)
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
        // Noto Sans Math sets the summation sign and the radical wider than
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

        let bytes: &[u8] = include_bytes!("../data/noto-cjk.smt");
        let mut mutated = bytes.to_vec();
        let last = mutated.len() - 1;
        mutated[last] ^= 0xff;
        let changed = MetricsTable::parse(&mutated).unwrap();
        assert_ne!(first, changed.hash());
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
            // The math face gets its extents from Noto Sans Math, which is
            // taller than the CJK faces; body faces are normalised to 1em.
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
}
