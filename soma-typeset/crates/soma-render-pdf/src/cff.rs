//! CFF (Type 2 charstring) parsing and subsetting for the PDF backend.
//!
//! The TrueType path keeps every glyph id and empties the unused `loca`
//! ranges. CFF is subset the same way, so the shaper, the text operator, `/W`
//! and `ToUnicode` keep sharing one id space (ADR-0039):
//!
//! * the CharStrings INDEX keeps its count, used programs are copied verbatim
//!   and every other glyph becomes a single `endchar`;
//! * the charset is rewritten to identity, so the CID a PDF viewer reads out
//!   of the embedded font is the glyph id this crate already uses;
//! * a name-keyed face is promoted to CID-keyed with a synthesised
//!   `ROS`/`FDArray`/`FDSelect`;
//! * the FDArray, the private dicts and the local subrs move as one blob, so
//!   only absolute `Private` offsets are re-encoded.

use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use crate::font::PdfError;

const OP_CHARSET: u16 = 15;
const OP_ENCODING: u16 = 16;
const OP_CHARSTRINGS: u16 = 17;
const OP_PRIVATE: u16 = 18;
const OP_ROS: u16 = 0x0c1e;
const OP_CID_COUNT: u16 = 0x0c22;
const OP_FDARRAY: u16 = 0x0c24;
const OP_FDSELECT: u16 = 0x0c25;

/// A Type 2 charstring that paints nothing.
const BLANK_GLYPH: [u8; 1] = [0x0e];

fn error(reason: impl Into<String>) -> PdfError {
    PdfError::new(reason)
}

/// The `ROS` of an embedded CID-keyed font, for the PDF `/CIDSystemInfo`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CidSystem {
    pub registry: String,
    pub ordering: String,
    pub supplement: i32,
}

impl Default for CidSystem {
    fn default() -> Self {
        Self {
            registry: String::from("Adobe"),
            ordering: String::from("Identity"),
            supplement: 0,
        }
    }
}

// ---------------------------------------------------------------------------
// INDEX
// ---------------------------------------------------------------------------

/// An INDEX: where every item starts and where the structure ends.
struct Index {
    starts: Vec<usize>,
    end: usize,
}

impl Index {
    fn count(&self) -> usize {
        self.starts.len().saturating_sub(1)
    }

    fn item<'a>(&self, data: &'a [u8], index: usize) -> &'a [u8] {
        &data[self.starts[index]..self.starts[index + 1]]
    }
}

fn read_index(data: &[u8], offset: usize) -> Result<Index, PdfError> {
    let count = read_u16(data, offset)? as usize;
    if count == 0 {
        return Ok(Index {
            starts: Vec::new(),
            end: offset + 2,
        });
    }
    let size = *data
        .get(offset + 2)
        .ok_or_else(|| error("CFF INDEX is truncated"))? as usize;
    if !(1..=4).contains(&size) {
        return Err(error(format!("CFF INDEX offset size {size} is invalid")));
    }
    let table = offset + 3;
    let base = table + (count + 1) * size - 1;
    if base >= data.len() {
        return Err(error("CFF INDEX is truncated"));
    }
    let mut starts = Vec::with_capacity(count + 1);
    for entry in 0..=count {
        let at = table + entry * size;
        let mut value = 0usize;
        for byte in 0..size {
            value = (value << 8)
                | *data
                    .get(at + byte)
                    .ok_or_else(|| error("CFF INDEX is truncated"))? as usize;
        }
        if value == 0 {
            return Err(error("CFF INDEX has a zero offset"));
        }
        starts.push(base + value);
    }
    if starts[0] < base || starts[count] > data.len() {
        return Err(error("CFF INDEX points outside the table"));
    }
    if starts.windows(2).any(|pair| pair[1] < pair[0]) {
        return Err(error("CFF INDEX offsets are not monotonic"));
    }
    let end = starts[count];
    Ok(Index { starts, end })
}

fn write_index(items: &[&[u8]]) -> Vec<u8> {
    if items.is_empty() {
        return vec![0, 0];
    }
    let mut total = 1usize;
    for item in items {
        total += item.len();
    }
    let size = if total <= 0xff {
        1
    } else if total <= 0xffff {
        2
    } else if total <= 0xff_ffff {
        3
    } else {
        4
    };
    let mut out = Vec::with_capacity(3 + (items.len() + 1) * size + total);
    out.extend_from_slice(&(items.len() as u16).to_be_bytes());
    out.push(size as u8);
    let mut offset = 1usize;
    for item in items {
        for byte in (0..size).rev() {
            out.push((offset >> (byte * 8)) as u8);
        }
        offset += item.len();
    }
    for byte in (0..size).rev() {
        out.push((offset >> (byte * 8)) as u8);
    }
    for item in items {
        out.extend_from_slice(item);
    }
    out
}

// ---------------------------------------------------------------------------
// DICT
// ---------------------------------------------------------------------------

/// One DICT entry. `raw` covers the original bytes, operands and operator, so
/// an untouched entry is copied without re-encoding anything.
#[derive(Clone)]
struct Entry {
    op: u16,
    args: Vec<i32>,
    raw: (usize, usize),
}

fn parse_dict(data: &[u8], from: usize, to: usize) -> Result<Vec<Entry>, PdfError> {
    let mut entries = Vec::new();
    let mut args = Vec::new();
    let mut start = from;
    let mut cursor = from;
    while cursor < to {
        let byte = data[cursor];
        match byte {
            0..=21 => {
                let mut op = byte as u16;
                let mut width = 1;
                if byte == 12 {
                    op = 0x0c00
                        | *data
                            .get(cursor + 1)
                            .ok_or_else(|| error("CFF DICT ends mid-operator"))?
                            as u16;
                    width = 2;
                }
                entries.push(Entry {
                    op,
                    args: core::mem::take(&mut args),
                    raw: (start, cursor + width),
                });
                cursor += width;
                start = cursor;
                continue;
            }
            28 => {
                args.push(read_i16(data, cursor + 1)? as i32);
                cursor += 3;
            }
            29 => {
                args.push(read_i32(data, cursor + 1)?);
                cursor += 5;
            }
            30 => {
                // Real numbers are never patched, so their value is not needed.
                cursor += 1;
                loop {
                    let byte = *data
                        .get(cursor)
                        .ok_or_else(|| error("CFF DICT real number is truncated"))?;
                    cursor += 1;
                    if byte >> 4 == 0x0f || byte & 0x0f == 0x0f {
                        break;
                    }
                }
                args.push(0);
            }
            32..=246 => {
                args.push(byte as i32 - 139);
                cursor += 1;
            }
            247..=250 => {
                args.push(
                    (byte as i32 - 247) * 256
                        + *data
                            .get(cursor + 1)
                            .ok_or_else(|| error("CFF DICT is truncated"))?
                            as i32
                        + 108,
                );
                cursor += 2;
            }
            251..=254 => {
                args.push(
                    -(byte as i32 - 251) * 256
                        - *data
                            .get(cursor + 1)
                            .ok_or_else(|| error("CFF DICT is truncated"))?
                            as i32
                        - 108,
                );
                cursor += 2;
            }
            _ => {
                // 255 is a 16.16 fixed point number, still never patched.
                args.push(read_i32(data, cursor + 1)? >> 16);
                cursor += 5;
            }
        }
    }
    Ok(entries)
}

fn put_int(value: i32, out: &mut Vec<u8>) {
    // Always the five-byte form: a rebuilt INDEX then has a size that does not
    // depend on the offsets it carries.
    out.push(29);
    out.extend_from_slice(&value.to_be_bytes());
}

fn put_op(op: u16, out: &mut Vec<u8>) {
    if op & 0xff00 == 0x0c00 {
        out.push(12);
        out.push((op & 0xff) as u8);
    } else {
        out.push(op as u8);
    }
}

fn find(entries: &[Entry], op: u16) -> Option<&Entry> {
    entries.iter().find(|entry| entry.op == op)
}

// ---------------------------------------------------------------------------
// Faces
// ---------------------------------------------------------------------------

struct Cff<'a> {
    data: &'a [u8],
    name: Index,
    strings: Index,
    global: Index,
    top_dict: Vec<Entry>,
}

impl<'a> Cff<'a> {
    fn parse(data: &'a [u8]) -> Result<Self, PdfError> {
        if data.len() < 4 {
            return Err(error("CFF table is too short"));
        }
        let header = data[2] as usize;
        if header < 4 || header > data.len() {
            return Err(error("CFF header size is invalid"));
        }
        let name = read_index(data, header)?;
        let top = read_index(data, name.end)?;
        let strings = read_index(data, top.end)?;
        let global = read_index(data, strings.end)?;
        if top.count() != 1 {
            return Err(error("CFF has no top DICT"));
        }
        let top_dict = parse_dict(data, top.starts[0], top.starts[1])?;
        Ok(Self {
            data,
            name,
            strings,
            global,
            top_dict,
        })
    }

    fn string(&self, sid: i32) -> Option<String> {
        let sid = usize::try_from(sid).ok()?;
        let index = sid.checked_sub(391)?;
        if index >= self.strings.count() {
            return None;
        }
        let bytes = self.strings.item(self.data, index);
        Some(String::from_utf8_lossy(bytes).into_owned())
    }
}

/// The `/CIDSystemInfo` an embedded program declares, if it is CID-keyed.
pub(crate) fn cid_system(data: &[u8]) -> Option<CidSystem> {
    let cff = Cff::parse(data).ok()?;
    let ros = find(&cff.top_dict, OP_ROS)?;
    if ros.args.len() < 3 {
        return None;
    }
    let fallback = CidSystem::default();
    Some(CidSystem {
        registry: cff.string(ros.args[0]).unwrap_or(fallback.registry),
        ordering: cff.string(ros.args[1]).unwrap_or(fallback.ordering),
        supplement: ros.args[2],
    })
}

/// Offsets of the structures a rebuilt Top DICT points at.
#[derive(Default, Clone, Copy)]
struct Offsets {
    charset: i32,
    charstrings: i32,
    fdarray: i32,
    fdselect: i32,
    /// New start of the copied private-dict region.
    region: i32,
}

fn put_entry(op: u16, args: &[i32], out: &mut Vec<u8>) {
    for value in args {
        put_int(*value, out);
    }
    put_op(op, out);
}

/// Rebuild `data` as a CID-keyed CFF holding only `used` glyphs.
///
/// Glyph ids are preserved: the returned program has the same glyph count as
/// the source, and unused glyphs paint nothing.
pub(crate) fn subset(data: &[u8], used: &[u16], num_glyphs: u16) -> Result<Vec<u8>, PdfError> {
    let cff = Cff::parse(data)?;
    let count = num_glyphs as usize;
    if count == 0 {
        return Err(error("CFF font has no glyphs"));
    }

    let charstrings_offset = find(&cff.top_dict, OP_CHARSTRINGS)
        .and_then(|entry| entry.args.first())
        .and_then(|value| usize::try_from(*value).ok())
        .ok_or_else(|| error("CFF has no CharStrings index"))?;
    let charstrings = read_index(data, charstrings_offset)?;
    if charstrings.count() != count {
        return Err(error(format!(
            "CFF has {} charstrings but maxp says {count} glyphs",
            charstrings.count()
        )));
    }
    if find(&cff.top_dict, OP_CHARSET).is_none() {
        return Err(error("CFF has no charset"));
    }

    let cid = find(&cff.top_dict, OP_ROS).is_some();
    if !cid {
        return Err(error(
            "this face is a name-keyed CFF; the PDF backend embeds CID-keyed CFF only, \
             because re-charseting a name-keyed program breaks its accent composition \
             (seac). Use a CID-keyed OTF (Noto/Source Han) or a TrueType/glyf face.",
        ));
    }

    let mut include = vec![false; count];
    include[0] = true;
    for gid in used {
        if let Some(slot) = include.get_mut(*gid as usize) {
            *slot = true;
        }
    }
    // 1. An identity charset; glyph 0 is implicit.
    let mut charset = Vec::with_capacity(2 * count);
    charset.push(0u8);
    for gid in 1..count {
        charset.extend_from_slice(&(gid as u16).to_be_bytes());
    }

    // 2. CharStrings with the unused glyphs blanked.
    let mut programs: Vec<&[u8]> = Vec::with_capacity(count);
    for (gid, &keep) in include.iter().enumerate() {
        programs.push(if keep {
            charstrings.item(data, gid)
        } else {
            &BLANK_GLYPH
        });
    }
    let charstrings_data = write_index(&programs);

    // 3. FDSelect is copied when the face has one; a converted name-keyed
    //    face gets a single range pointing at its one Font DICT.
    let mut fdselect_owned = Vec::new();
    let fdselect: &[u8] = match find(&cff.top_dict, OP_FDSELECT) {
        Some(entry) => {
            let offset = usize::try_from(entry.args[0])
                .map_err(|_| error("CFF FDSelect offset is negative"))?;
            let format = *data
                .get(offset)
                .ok_or_else(|| error("CFF FDSelect is out of bounds"))?;
            let length = match format {
                0 => 1 + count,
                3 => 1 + 2 + read_u16(data, offset + 1)? as usize * 3 + 2,
                other => return Err(error(format!("CFF FDSelect format {other} is unsupported"))),
            };
            data.get(offset..offset + length)
                .ok_or_else(|| error("CFF FDSelect is truncated"))?
        }
        None => {
            fdselect_owned.extend_from_slice(&[3, 0, 1, 0, 0, 0]);
            fdselect_owned.extend_from_slice(&(count as u16).to_be_bytes());
            &fdselect_owned
        }
    };

    // 4. Font dicts and the private-dict region they point into.
    let mut font_dicts: Vec<Vec<Entry>> = Vec::new();
    let mut private_offsets: Vec<i32> = Vec::new();
    let (region_start, region_end) = {
        let offset = find(&cff.top_dict, OP_FDARRAY)
            .and_then(|entry| entry.args.first())
            .and_then(|value| usize::try_from(*value).ok())
            .ok_or_else(|| error("CID-keyed CFF has no FDArray"))?;
        let fdarray = read_index(data, offset)?;
        for index in 0..fdarray.count() {
            let dict = parse_dict(data, fdarray.starts[index], fdarray.starts[index + 1])?;
            if let Some(private) = find(&dict, OP_PRIVATE) {
                private_arguments(private)?;
                private_offsets.push(private.args[1]);
            }
            font_dicts.push(dict);
        }
        let start = private_offsets
            .iter()
            .filter_map(|value| usize::try_from(*value).ok())
            .min()
            .unwrap_or(fdarray.end)
            .min(fdarray.end);
        (start, data.len())
    };
    if region_end < region_start || region_end > data.len() {
        return Err(error("CFF private dicts are out of bounds"));
    }
    let region = &data[region_start..region_end];

    // 5. Strings; a converted name-keyed face needs Adobe/Identity.
    let strings: Vec<&[u8]> = (0..cff.strings.count())
        .map(|index| cff.strings.item(data, index))
        .collect();
    let ros = find(&cff.top_dict, OP_ROS).map(|entry| entry.args.clone());
    let strings_data = write_index(&strings);

    // 6. Sizes do not depend on the offsets: every patch is a five-byte int.
    let top_size = build_top(&cff, ros.as_deref(), &Offsets::default(), count).len();
    let fdarray_size = build_fdarray(&font_dicts, data, region_start, &Offsets::default()).len();

    let mut offsets = Offsets::default();
    let mut position = cff.name.end + top_size + strings_data.len();
    position += cff.global.end - cff.strings.end;
    offsets.charset = position as i32;
    position += charset.len();
    offsets.fdselect = position as i32;
    position += fdselect.len();
    offsets.charstrings = position as i32;
    position += charstrings_data.len();
    offsets.fdarray = position as i32;
    position += fdarray_size;
    offsets.region = position as i32;

    let top = build_top(&cff, ros.as_deref(), &offsets, count);
    let fdarray = build_fdarray(&font_dicts, data, region_start, &offsets);
    debug_assert_eq!(top.len(), top_size);
    debug_assert_eq!(fdarray.len(), fdarray_size);

    // 7. Assemble.
    let mut out = Vec::with_capacity(position + region.len());
    out.extend_from_slice(&data[..cff.name.end]);
    out.extend_from_slice(&top);
    out.extend_from_slice(&strings_data);
    out.extend_from_slice(&data[cff.strings.end..cff.global.end]);
    debug_assert_eq!(out.len(), offsets.charset as usize);
    out.extend_from_slice(&charset);
    out.extend_from_slice(fdselect);
    out.extend_from_slice(&charstrings_data);
    debug_assert_eq!(out.len(), offsets.fdarray as usize);
    out.extend_from_slice(&fdarray);
    debug_assert_eq!(out.len(), offsets.region as usize);
    out.extend_from_slice(region);
    Ok(out)
}

fn build_top(cff: &Cff<'_>, ros: Option<&[i32]>, offsets: &Offsets, count: usize) -> Vec<u8> {
    let mut out = Vec::new();
    if let Some(args) = ros {
        put_entry(OP_ROS, args, &mut out);
    }
    for entry in &cff.top_dict {
        match entry.op {
            OP_ROS => {}
            OP_CHARSET => put_entry(OP_CHARSET, &[offsets.charset], &mut out),
            OP_CHARSTRINGS => put_entry(OP_CHARSTRINGS, &[offsets.charstrings], &mut out),
            OP_FDARRAY => put_entry(OP_FDARRAY, &[offsets.fdarray], &mut out),
            OP_FDSELECT => put_entry(OP_FDSELECT, &[offsets.fdselect], &mut out),
            OP_CID_COUNT => put_entry(OP_CID_COUNT, &[count as i32], &mut out),
            // The Private dict moves into the single Font DICT, and a
            // CID-keyed font must not carry an Encoding.
            OP_PRIVATE | OP_ENCODING => {}
            _ => out.extend_from_slice(&cff.data[entry.raw.0..entry.raw.1]),
        }
    }
    if find(&cff.top_dict, OP_FDARRAY).is_none() {
        put_entry(OP_FDARRAY, &[offsets.fdarray], &mut out);
    }
    if find(&cff.top_dict, OP_FDSELECT).is_none() {
        put_entry(OP_FDSELECT, &[offsets.fdselect], &mut out);
    }
    if find(&cff.top_dict, OP_CID_COUNT).is_none() {
        put_entry(OP_CID_COUNT, &[count as i32], &mut out);
    }
    write_index(&[&out])
}

fn build_fdarray(
    font_dicts: &[Vec<Entry>],
    data: &[u8],
    region_start: usize,
    offsets: &Offsets,
) -> Vec<u8> {
    let mut items: Vec<Vec<u8>> = Vec::with_capacity(font_dicts.len());
    for dict in font_dicts {
        let mut out = Vec::new();
        for entry in dict {
            if entry.op == OP_PRIVATE {
                let old = usize::try_from(entry.args[1]).unwrap_or(region_start);
                let new = offsets.region + old.saturating_sub(region_start) as i32;
                put_entry(OP_PRIVATE, &[entry.args[0], new], &mut out);
            } else {
                out.extend_from_slice(&data[entry.raw.0..entry.raw.1]);
            }
        }
        items.push(out);
    }
    let items: Vec<&[u8]> = items.iter().map(|item| item.as_slice()).collect();
    write_index(&items)
}

fn private_arguments(entry: &Entry) -> Result<(i32, usize), PdfError> {
    if entry.args.len() < 2 {
        return Err(error("CFF Private entry has fewer than two operands"));
    }
    let offset =
        usize::try_from(entry.args[1]).map_err(|_| error("CFF Private offset is negative"))?;
    Ok((entry.args[0], offset))
}

fn read_u16(data: &[u8], offset: usize) -> Result<u16, PdfError> {
    let bytes = data
        .get(offset..offset + 2)
        .ok_or_else(|| error("CFF table is truncated"))?;
    Ok(u16::from_be_bytes([bytes[0], bytes[1]]))
}

fn read_i16(data: &[u8], offset: usize) -> Result<i16, PdfError> {
    Ok(read_u16(data, offset)? as i16)
}

#[cfg(test)]
fn read_u32(data: &[u8], offset: usize) -> Result<u32, PdfError> {
    let bytes = data
        .get(offset..offset + 4)
        .ok_or_else(|| error("CFF table is truncated"))?;
    Ok(u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

fn read_i32(data: &[u8], offset: usize) -> Result<i32, PdfError> {
    let bytes = data
        .get(offset..offset + 4)
        .ok_or_else(|| error("CFF table is truncated"))?;
    Ok(i32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cff_table(file: &[u8]) -> &[u8] {
        let count = read_u16(file, 4).expect("sfnt") as usize;
        for index in 0..count {
            let record = 12 + index * 16;
            if &file[record..record + 4] != b"CFF " {
                continue;
            }
            let offset = read_u32(file, record + 8).expect("offset") as usize;
            let length = read_u32(file, record + 12).expect("length") as usize;
            return &file[offset..offset + length];
        }
        panic!("no CFF table");
    }

    fn charstrings<'a>(program: &'a [u8]) -> (Cff<'a>, Index) {
        let cff = Cff::parse(program).expect("parsed");
        let offset = find(&cff.top_dict, OP_CHARSTRINGS)
            .expect("charstrings")
            .args[0] as usize;
        let index = read_index(program, offset).expect("index");
        (cff, index)
    }

    const CID: &[u8] = include_bytes!("../tests/fixtures/tiny-cff.otf");
    const NAME: &[u8] = include_bytes!("../tests/fixtures/tiny-cff-name.otf");

    #[test]
    fn an_index_round_trips() {
        let items: [&[u8]; 3] = [b"a", b"", b"bcd"];
        let bytes = write_index(&items);
        let index = read_index(&bytes, 0).expect("index");
        assert_eq!(index.count(), 3);
        assert_eq!(index.item(&bytes, 0), b"a");
        assert_eq!(index.item(&bytes, 1), b"");
        assert_eq!(index.item(&bytes, 2), b"bcd");
        assert_eq!(write_index(&[]), vec![0, 0]);
    }

    #[test]
    fn a_subset_keeps_glyph_ids_charsets_and_blanks_the_rest() {
        let source = cff_table(CID);
        let (_, before) = charstrings(source);
        let used = [1u16, 3];
        let program = subset(source, &used, 6).expect("subset");
        let (cff, after) = charstrings(&program);

        assert_eq!(after.count(), 6, "every glyph slot is kept");
        assert_eq!(after.item(&program, 1), before.item(source, 1));
        assert_eq!(after.item(&program, 3), before.item(source, 3));
        assert_eq!(after.item(&program, 2), &BLANK_GLYPH);
        assert_eq!(after.item(&program, 5), &BLANK_GLYPH);

        let charset = find(&cff.top_dict, OP_CHARSET).expect("charset").args[0] as usize;
        assert_eq!(program[charset], 0, "format 0");
        for gid in 0..5usize {
            assert_eq!(
                read_u16(&program, charset + 1 + gid * 2).unwrap() as usize,
                gid + 1,
                "the charset is identity, so CID equals glyph id"
            );
        }

        let ros = find(&cff.top_dict, OP_ROS).expect("ROS");
        assert_eq!(ros.args, vec![391, 392, 0]);
        assert_eq!(
            find(&cff.top_dict, OP_CID_COUNT).expect("CIDCount").args,
            vec![6]
        );
    }

    #[test]
    fn a_name_keyed_face_is_refused_with_an_actionable_error() {
        let source = cff_table(NAME);
        let error = subset(source, &[1], 6).expect_err("name-keyed CFF is out of scope");
        assert!(error.reason.contains("name-keyed"), "{}", error.reason);
        assert!(error.reason.contains("CID-keyed"), "{}", error.reason);
    }
}
