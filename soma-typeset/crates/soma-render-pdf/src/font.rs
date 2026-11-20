//! TrueType and CFF parsing and subsetting for the PDF backend.
//!
//! A TrueType program is built by copying the used glyphs, rebuilding
//! `loca`/`glyf`, and embedding the result as `FontFile2`. A CFF program is
//! rebuilt as CID-keyed Type 2 charstrings and embedded as `FontFile3`
//! (ADR-0039). Both keep glyph ids, so one id space reaches the text operator.

use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use crate::cff::{self, CidSystem};

/// A PDF renderer error (font, layout, or document size).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PdfError {
    pub reason: String,
}

impl PdfError {
    pub fn new(reason: impl Into<String>) -> Self {
        Self {
            reason: reason.into(),
        }
    }
}

impl core::fmt::Display for PdfError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter.write_str(&self.reason)
    }
}

#[derive(Clone, Copy)]
struct Table {
    tag: [u8; 4],
    offset: usize,
    length: usize,
}

/// Where the outlines live.
#[derive(Clone)]
enum Outlines {
    TrueType {
        loca: Vec<u32>,
        glyf_offset: usize,
        glyf_length: usize,
    },
    Cff {
        offset: usize,
        length: usize,
    },
}

/// Parsed tables that do not depend on the used glyph set.
#[derive(Clone)]
struct Face {
    tables: Vec<Table>,
    outlines: Outlines,
    num_glyphs: u16,
}

/// An embeddable TrueType face.
#[derive(Clone)]
pub struct PdfFont {
    data: Vec<u8>,
    face: Face,
    /// Sorted by codepoint.
    cmap: Vec<(u32, u16)>,
    /// Advance width per glyph, in font units.
    widths: Vec<u16>,
    upem: u16,
    ascent: i16,
    descent: i16,
    bbox: [i16; 4],
    italic_angle: i16,
    underline_position: i16,
    underline_thickness: i16,
    cap_height: i16,
    bold: bool,
    serif: bool,
    cid: Option<CidSystem>,
}

impl PdfFont {
    /// Parse a face from a TrueType file or a TrueType collection.
    ///
    /// `index` selects the face inside a `ttcf` collection; a single font only
    /// has index 0.
    pub fn from_bytes(data: Vec<u8>, index: u32) -> Result<Self, PdfError> {
        let face = Face::open(&data, index)?;
        let cmap = parse_cmap(&data, &face)?;
        let upem = read_u16(&data, face.required(b"head")?.offset + 18)?;
        if upem == 0 {
            return Err(PdfError::new("font unitsPerEm is zero"));
        }
        let bbox = {
            let head = face.required(b"head")?.offset;
            [
                read_i16(&data, head + 36)?,
                read_i16(&data, head + 38)?,
                read_i16(&data, head + 40)?,
                read_i16(&data, head + 42)?,
            ]
        };
        let hhea = face.required(b"hhea")?.offset;
        let ascent = read_i16(&data, hhea + 4)?;
        let descent = read_i16(&data, hhea + 6)?;
        let widths = parse_widths(&data, &face)?;
        let mut italic_angle = 0i16;
        // `post`'s header carries the underline geometry in every version, so
        // a version 3 face still answers with real values (ADR-0044).
        let mut underline_position = -(upem as i16) / 10;
        let mut underline_thickness = (upem as i16) / 20;
        if let Some(post) = face.find(b"post") {
            if post.length >= 8 {
                let raw = read_u32(&data, post.offset + 4)?;
                italic_angle = ((raw >> 16) & 0xFFFF) as u16 as i16;
            }
            if post.length >= 12 {
                let position = read_i16(&data, post.offset + 8)?;
                let thickness = read_i16(&data, post.offset + 10)?;
                if position < 0 {
                    underline_position = position;
                }
                if thickness > 0 {
                    underline_thickness = thickness;
                }
            }
        }
        let mut cap_height = ascent;
        let mut bold = false;
        if let Some(os2) = face.find(b"OS/2") {
            if os2.length >= 74 {
                let weight = read_u16(&data, os2.offset + 4)?;
                bold = weight >= 600;
                let cap = read_i16(&data, os2.offset + 88).unwrap_or(ascent);
                if cap > 0 {
                    cap_height = cap;
                }
            }
        }
        let cid = match face.outlines {
            Outlines::Cff { offset, length } => cff::cid_system(&data[offset..offset + length]),
            Outlines::TrueType { .. } => None,
        };
        Ok(Self {
            data,
            face,
            cmap,
            widths,
            upem,
            ascent,
            descent,
            bbox,
            italic_angle,
            underline_position,
            underline_thickness,
            cap_height,
            bold,
            serif: false,
            cid,
        })
    }

    /// Declare the face as serif or sans-serif; used only for the PDF font
    /// descriptor's flags.
    pub fn with_serif(mut self, serif: bool) -> Self {
        self.serif = serif;
        self
    }

    /// Glyph id for a Unicode scalar. Missing characters map to `.notdef` (0).
    pub fn glyph(&self, codepoint: u32) -> u16 {
        match self.cmap.binary_search_by_key(&codepoint, |(cp, _)| *cp) {
            Ok(index) => self.cmap[index].1,
            Err(_) => 0,
        }
    }

    pub fn is_bold(&self) -> bool {
        self.bold
    }

    pub fn units_per_em(&self) -> u16 {
        self.upem
    }

    fn advance_1000(&self, gid: u16) -> u32 {
        let advance = self.widths.get(gid as usize).copied().unwrap_or(0) as u32;
        advance * 1000 / self.upem.max(1) as u32
    }

    /// Build a font program containing only `used` glyphs.
    ///
    /// Glyph ids are preserved (unused glyphs become empty), so `Identity-H`
    /// CIDs stay equal to the ids the shaper and this renderer already use.
    pub fn subset(&self, used: &[u16]) -> Result<Subset, PdfError> {
        match self.face.outlines {
            Outlines::Cff { offset, length } => Ok(Subset {
                program: cff::subset(&self.data[offset..offset + length], used, self.num_glyphs())?,
                kind: ProgramKind::CidCff,
            }),
            Outlines::TrueType { .. } => Ok(Subset {
                program: self.subset_true_type(used),
                kind: ProgramKind::TrueType,
            }),
        }
    }

    /// What kind of program [`PdfFont::subset`] builds for this face.
    pub fn program_kind(&self) -> ProgramKind {
        match self.face.outlines {
            Outlines::Cff { .. } => ProgramKind::CidCff,
            Outlines::TrueType { .. } => ProgramKind::TrueType,
        }
    }

    /// `/CIDSystemInfo` values for an embedded CID-keyed program.
    pub fn cid_system(&self) -> CidSystem {
        self.cid.clone().unwrap_or_default()
    }

    fn num_glyphs(&self) -> u16 {
        self.face.num_glyphs
    }

    fn subset_true_type(&self, used: &[u16]) -> Vec<u8> {
        let (loca_table, glyf_offset, glyf_length) = match self.face.outlines {
            Outlines::TrueType {
                ref loca,
                glyf_offset,
                glyf_length,
            } => (loca, glyf_offset, glyf_length),
            Outlines::Cff { .. } => unreachable!("only called for TrueType outlines"),
        };
        let mut include = vec![false; self.face.num_glyphs as usize];
        let mut queue: Vec<u16> = Vec::with_capacity(used.len() + 1);
        include[0] = true;
        queue.push(0);
        for &gid in used {
            if (gid as usize) < include.len() && !include[gid as usize] {
                include[gid as usize] = true;
                queue.push(gid);
            }
        }
        while let Some(gid) = queue.pop() {
            for component in self.components(gid) {
                if (component as usize) < include.len() && !include[component as usize] {
                    include[component as usize] = true;
                    queue.push(component);
                }
            }
        }

        let mut glyf = Vec::new();
        let mut offsets = Vec::with_capacity(include.len() + 1);
        offsets.push(0u32);
        for (gid, &used) in include.iter().enumerate() {
            if used {
                let start = loca_table[gid] as usize;
                let end = loca_table[gid + 1] as usize;
                if end > start && end <= glyf_length {
                    glyf.extend_from_slice(&self.data[glyf_offset + start..glyf_offset + end]);
                    while glyf.len() % 4 != 0 {
                        glyf.push(0);
                    }
                }
            }
            offsets.push(glyf.len() as u32);
        }
        let mut loca = Vec::with_capacity((include.len() + 1) * 4);
        for offset in &offsets {
            loca.extend_from_slice(&offset.to_be_bytes());
        }

        let mut tables: Vec<([u8; 4], Vec<u8>)> = Vec::new();
        let mut head = self.table_copy(b"head").unwrap_or_default();
        if head.len() >= 52 {
            // Long loca format and a checksum adjustment filled in later.
            head[50..52].copy_from_slice(&1u16.to_be_bytes());
            head[8..12].copy_from_slice(&[0, 0, 0, 0]);
        }
        tables.push((*b"head", head));
        if let Some(table) = self.table_copy(b"hhea") {
            tables.push((*b"hhea", table));
        }
        if let Some(table) = self.table_copy(b"hmtx") {
            tables.push((*b"hmtx", table));
        }
        if let Some(table) = self.table_copy(b"maxp") {
            tables.push((*b"maxp", table));
        }
        tables.push((*b"loca", loca));
        tables.push((*b"glyf", glyf));
        for tag in [b"cvt ".as_slice(), b"fpgm", b"prep", b"gasp", b"OS/2"] {
            let mut key = [0u8; 4];
            key.copy_from_slice(tag);
            if let Some(table) = self.table_copy(&key) {
                tables.push((key, table));
            }
        }
        tables.sort_by_key(|table| table.0);
        assemble(&tables)
    }

    /// PDF font descriptor values in 1000-unit space.
    pub fn descriptor(&self) -> PdfFontDescriptor {
        let scale = |value: i16| -> i32 { value as i32 * 1000 / self.upem.max(1) as i32 };
        PdfFontDescriptor {
            ascent: scale(self.ascent),
            descent: scale(self.descent),
            cap_height: scale(self.cap_height),
            bbox: [
                scale(self.bbox[0]),
                scale(self.bbox[1]),
                scale(self.bbox[2]),
                scale(self.bbox[3]),
            ],
            italic_angle: self.italic_angle as i32,
            underline_position: scale(self.underline_position),
            underline_thickness: scale(self.underline_thickness).max(1),
            flags: 4 | if self.serif { 2 } else { 0 } | if self.bold { 262144 } else { 0 },
        }
    }

    /// Advance width in 1000-unit glyph space.
    pub fn width_1000(&self, gid: u16) -> u32 {
        self.advance_1000(gid)
    }

    fn table_copy(&self, tag: &[u8; 4]) -> Option<Vec<u8>> {
        let table = self.face.find(tag)?;
        Some(self.data[table.offset..table.offset + table.length].to_vec())
    }

    /// Composite glyph components, resolved from `glyf`.
    fn components(&self, gid: u16) -> Vec<u16> {
        let mut out = Vec::new();
        let Outlines::TrueType {
            ref loca,
            glyf_offset,
            glyf_length,
        } = self.face.outlines
        else {
            return out;
        };
        let index = gid as usize;
        if index + 1 >= loca.len() {
            return out;
        }
        let start = loca[index] as usize;
        let end = loca[index + 1] as usize;
        if end <= start || end > glyf_length {
            return out;
        }
        let data = &self.data[glyf_offset + start..glyf_offset + end];
        if data.len() < 10 {
            return out;
        }
        let contours = read_i16(data, 0).unwrap_or(0);
        if contours >= 0 {
            return out;
        }
        let mut cursor = 10usize;
        while let Ok(flags) = read_u16(data, cursor) {
            let Ok(component) = read_u16(data, cursor + 2) else {
                break;
            };
            out.push(component);
            cursor += 4;
            cursor += if flags & 0x0001 != 0 { 4 } else { 2 };
            if flags & 0x0008 != 0 {
                cursor += 2;
            } else if flags & 0x0040 != 0 {
                cursor += 4;
            } else if flags & 0x0080 != 0 {
                cursor += 8;
            }
            if flags & 0x0020 == 0 {
                break;
            }
        }
        out
    }
}

/// Which stream a subset has to be written as.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProgramKind {
    /// `glyf` outlines: `/FontFile2` and `/CIDFontType2`.
    TrueType,
    /// CID-keyed Type 2 charstrings: `/FontFile3` and `/CIDFontType0`.
    CidCff,
}

/// A font program ready for `/FontFile2` or `/FontFile3`.
#[derive(Clone, Debug)]
pub struct Subset {
    pub program: Vec<u8>,
    pub kind: ProgramKind,
}

/// Descriptor values already scaled to 1000 units per em.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PdfFontDescriptor {
    pub ascent: i32,
    pub descent: i32,
    pub cap_height: i32,
    /// Baseline-relative underline geometry, in 1000-unit space. The position
    /// is negative below the baseline.
    pub underline_position: i32,
    pub underline_thickness: i32,
    pub bbox: [i32; 4],
    pub italic_angle: i32,
    pub flags: u32,
}

impl Face {
    fn open(data: &[u8], index: u32) -> Result<Self, PdfError> {
        if data.len() < 12 {
            return Err(PdfError::new("font is too short"));
        }
        let tag = read_u32(data, 0)?;
        let base = if tag == 0x7474_6366 {
            let count = read_u32(data, 8)?;
            if index >= count {
                return Err(PdfError::new(format!(
                    "font collection has {count} faces, index {index} is out of range"
                )));
            }
            read_u32(data, 12 + index as usize * 4)? as usize
        } else {
            if index != 0 {
                return Err(PdfError::new("a single font only has index 0"));
            }
            0
        };
        let version = read_u32(data, base)?;
        if version != 0x0001_0000 && version != 0x7472_7565 && version != 0x4F54_544F {
            return Err(PdfError::new("unsupported sfnt version"));
        }
        let count = read_u16(data, base + 4)? as usize;
        let mut tables = Vec::with_capacity(count);
        for index in 0..count {
            let record = base + 12 + index * 16;
            let mut tag = [0u8; 4];
            tag.copy_from_slice(&data[record..record + 4]);
            let offset = read_u32(data, record + 8)? as usize;
            let length = read_u32(data, record + 12)? as usize;
            if offset > data.len() || length > data.len() - offset {
                return Err(PdfError::new("font table is out of bounds"));
            }
            tables.push(Table {
                tag,
                offset,
                length,
            });
        }
        let mut face = Self {
            tables,
            outlines: Outlines::Cff {
                offset: 0,
                length: 0,
            },
            num_glyphs: 0,
        };
        let head = face.required(b"head")?;
        let maxp = face.required(b"maxp")?;
        let num_glyphs = read_u16(data, maxp.offset + 4)?;
        face.num_glyphs = num_glyphs;
        if let Some(table) = face.find(b"CFF2") {
            let _ = table;
            return Err(PdfError::new(
                "CFF2 (variable) outlines are not supported; use a static OTF or a TrueType face",
            ));
        }
        if let Some(table) = face.find(b"CFF ") {
            face.outlines = Outlines::Cff {
                offset: table.offset,
                length: table.length,
            };
            return Ok(face);
        }
        let glyf = face.required(b"glyf")?;
        let loca = face.required(b"loca")?;
        let _ = head;
        let format = read_i16(data, head.offset + 50)?;
        let mut offsets = Vec::with_capacity(num_glyphs as usize + 1);
        for index in 0..=num_glyphs as usize {
            let value = if format == 0 {
                read_u16(data, loca.offset + index * 2)? as u32 * 2
            } else {
                read_u32(data, loca.offset + index * 4)?
            };
            offsets.push(value);
        }
        for pair in offsets.windows(2) {
            if pair[1] < pair[0] {
                return Err(PdfError::new("font loca is not monotonic"));
            }
        }
        face.outlines = Outlines::TrueType {
            loca: offsets,
            glyf_offset: glyf.offset,
            glyf_length: glyf.length,
        };
        Ok(face)
    }

    fn find(&self, tag: &[u8; 4]) -> Option<Table> {
        self.tables.iter().find(|table| &table.tag == tag).copied()
    }

    fn required(&self, tag: &[u8; 4]) -> Result<Table, PdfError> {
        self.find(tag).ok_or_else(|| {
            PdfError::new(format!(
                "font is missing the {} table",
                String::from_utf8_lossy(tag)
            ))
        })
    }
}

fn parse_widths(data: &[u8], face: &Face) -> Result<Vec<u16>, PdfError> {
    let hhea = face.required(b"hhea")?.offset;
    let metrics = read_u16(data, hhea + 34)? as usize;
    let hmtx = face.required(b"hmtx")?.offset;
    if metrics == 0 {
        return Err(PdfError::new("font has no horizontal metrics"));
    }
    let mut widths = Vec::with_capacity(face.num_glyphs as usize);
    let mut last = 0u16;
    for gid in 0..face.num_glyphs as usize {
        if gid < metrics {
            last = read_u16(data, hmtx + gid * 4)?;
        }
        widths.push(last);
    }
    Ok(widths)
}

fn parse_cmap(data: &[u8], face: &Face) -> Result<Vec<(u32, u16)>, PdfError> {
    let cmap = face.required(b"cmap")?;
    let count = read_u16(data, cmap.offset + 2)? as usize;
    let mut best: Option<(usize, u16)> = None;
    for index in 0..count {
        let record = cmap.offset + 4 + index * 8;
        let platform = read_u16(data, record)?;
        let encoding = read_u16(data, record + 2)?;
        let offset = cmap.offset + read_u32(data, record + 4)? as usize;
        let format = read_u16(data, offset)?;
        let rank = match (platform, encoding, format) {
            (3, 10, 12) => 4u16,
            (3, 1, 4) => 3,
            (0, _, 4) => 2,
            (0, _, 12) => 2,
            (3, 10, 4) => 1,
            _ => 0,
        };
        if rank > 0 && best.map(|(_, current)| current).unwrap_or(0) < rank {
            best = Some((offset, rank));
        }
    }
    let (offset, _) = best.ok_or_else(|| PdfError::new("font has no usable cmap"))?;
    let mut map = match read_u16(data, offset)? {
        4 => cmap_format4(data, offset)?,
        12 => cmap_format12(data, offset)?,
        other => return Err(PdfError::new(format!("unsupported cmap format {other}"))),
    };
    map.sort_by_key(|(codepoint, _)| *codepoint);
    map.dedup_by_key(|(codepoint, _)| *codepoint);
    Ok(map)
}

fn cmap_format4(data: &[u8], offset: usize) -> Result<Vec<(u32, u16)>, PdfError> {
    let seg_count = read_u16(data, offset + 6)? as usize / 2;
    let end_codes = offset + 14;
    let start_codes = end_codes + seg_count * 2 + 2;
    let deltas = start_codes + seg_count * 2;
    let range_offsets = deltas + seg_count * 2;
    let mut map = Vec::new();
    for index in 0..seg_count {
        let start = read_u16(data, start_codes + index * 2)? as u32;
        let end = read_u16(data, end_codes + index * 2)? as u32;
        if end < start {
            continue;
        }
        let delta = read_u16(data, deltas + index * 2)? as i32;
        let range = read_u16(data, range_offsets + index * 2)? as usize;
        for codepoint in start..=end {
            if codepoint == 0xFFFF {
                continue;
            }
            let gid = if range == 0 {
                ((codepoint as i32 + delta) & 0xFFFF) as u16
            } else {
                let address = range_offsets + index * 2 + range + (codepoint - start) as usize * 2;
                let value = read_u16(data, address)?;
                if value == 0 {
                    0
                } else {
                    ((value as i32 + delta) & 0xFFFF) as u16
                }
            };
            if gid != 0 {
                map.push((codepoint, gid));
            }
        }
    }
    Ok(map)
}

fn cmap_format12(data: &[u8], offset: usize) -> Result<Vec<(u32, u16)>, PdfError> {
    let groups = read_u32(data, offset + 12)? as usize;
    let mut map = Vec::new();
    for group in 0..groups {
        let record = offset + 16 + group * 12;
        let start = read_u32(data, record)?;
        let end = read_u32(data, record + 4)?;
        let glyph = read_u32(data, record + 8)?;
        if end < start || end - start > 0x2_0000 {
            continue;
        }
        for codepoint in start..=end {
            let gid = glyph + (codepoint - start);
            if gid != 0 && gid <= u16::MAX as u32 {
                map.push((codepoint, gid as u16));
            }
        }
    }
    Ok(map)
}

fn assemble(tables: &[([u8; 4], Vec<u8>)]) -> Vec<u8> {
    let count = tables.len() as u16;
    let mut selector = 0u16;
    while (1u32 << (selector + 1)) <= count as u32 {
        selector += 1;
    }
    let search_range = 16 * (1u16 << selector);
    let range_shift = count * 16 - search_range;

    let mut out = Vec::new();
    out.extend_from_slice(&0x0001_0000u32.to_be_bytes());
    out.extend_from_slice(&count.to_be_bytes());
    out.extend_from_slice(&search_range.to_be_bytes());
    out.extend_from_slice(&selector.to_be_bytes());
    out.extend_from_slice(&range_shift.to_be_bytes());

    let mut body = Vec::new();
    let mut records = Vec::new();
    let mut head_offset = None;
    let mut offset = 12 + 16 * tables.len();
    for (tag, data) in tables {
        records.extend_from_slice(tag);
        records.extend_from_slice(&checksum(data).to_be_bytes());
        records.extend_from_slice(&(offset as u32).to_be_bytes());
        records.extend_from_slice(&(data.len() as u32).to_be_bytes());
        if tag == b"head" {
            head_offset = Some((offset, offset + 8));
        }
        body.extend_from_slice(data);
        while body.len() % 4 != 0 {
            body.push(0);
        }
        offset += data.len() + (4 - data.len() % 4) % 4;
    }
    out.extend_from_slice(&records);
    out.extend_from_slice(&body);
    if let Some((_, adjustment)) = head_offset {
        let value = (0xB1B0_AFBAu32).wrapping_sub(checksum(&out));
        out[adjustment..adjustment + 4].copy_from_slice(&value.to_be_bytes());
    }
    out
}

fn checksum(data: &[u8]) -> u32 {
    let mut sum = 0u32;
    let mut chunk = [0u8; 4];
    for (index, byte) in data.iter().enumerate() {
        chunk[index % 4] = *byte;
        if index % 4 == 3 {
            sum = sum.wrapping_add(u32::from_be_bytes(chunk));
            chunk = [0u8; 4];
        }
    }
    if data.len() % 4 != 0 {
        sum = sum.wrapping_add(u32::from_be_bytes(chunk));
    }
    sum
}

fn read_u16(data: &[u8], offset: usize) -> Result<u16, PdfError> {
    let bytes = data
        .get(offset..offset + 2)
        .ok_or_else(|| PdfError::new("font read out of bounds"))?;
    Ok(u16::from_be_bytes([bytes[0], bytes[1]]))
}

fn read_i16(data: &[u8], offset: usize) -> Result<i16, PdfError> {
    Ok(read_u16(data, offset)? as i16)
}

fn read_u32(data: &[u8], offset: usize) -> Result<u32, PdfError> {
    let bytes = data
        .get(offset..offset + 4)
        .ok_or_else(|| PdfError::new("font read out of bounds"))?;
    Ok(u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}
