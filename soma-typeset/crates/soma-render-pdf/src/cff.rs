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
//! * the FDArray, the private dicts and the local subrs move as one blob, so
//!   only absolute `Private` offsets are re-encoded.
//!
//! Only CID-keyed programs are embedded. A name-keyed face would have to be
//! re-charseted, and that breaks the name lookup `seac` accent composition
//! depends on, so it is refused with an error that says so.

use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use crate::font::PdfError;
use crate::standard_encoding::STANDARD_ENCODING_SID;

const OP_CHARSET: u16 = 15;
const OP_ENCODING: u16 = 16;
const OP_CHARSTRINGS: u16 = 17;
const OP_PRIVATE: u16 = 18;
const OP_SUBRS: u16 = 19;
const OP_ROS: u16 = 0x0c1e;
const OP_CID_COUNT: u16 = 0x0c22;
const OP_FDARRAY: u16 = 0x0c24;
const OP_FDSELECT: u16 = 0x0c25;

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
                // Real numbers are never patched, so their value is not
                // needed — but the terminator has to be inside this dict.
                cursor += 1;
                loop {
                    if cursor >= to {
                        return Err(error("CFF DICT real number is truncated"));
                    }
                    let byte = data[cursor];
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

/// The SID of every glyph, read from the charset. Name-keyed faces only.
fn charset_sids(data: &[u8], offset: usize, count: usize) -> Result<Vec<u16>, PdfError> {
    let mut sids = vec![0u16; count];
    let format = *data
        .get(offset)
        .ok_or_else(|| error("CFF charset is out of bounds"))?;
    match format {
        0 => {
            for (index, sid) in sids.iter_mut().enumerate().take(count).skip(1) {
                *sid = read_u16(data, offset + 1 + (index - 1) * 2)?;
            }
        }
        1 | 2 => {
            let mut cursor = offset + 1;
            let mut gid = 1usize;
            while gid < count {
                let first = read_u16(data, cursor)?;
                let left = *data
                    .get(cursor + 2)
                    .ok_or_else(|| error("CFF charset is truncated"))?
                    as usize;
                cursor += 3;
                let mut sid = first;
                sids[gid] = sid;
                gid += 1;
                for _ in 0..left {
                    if gid >= count {
                        break;
                    }
                    let delta = if format == 1 {
                        let value = *data
                            .get(cursor)
                            .ok_or_else(|| error("CFF charset is truncated"))?
                            as u16;
                        cursor += 1;
                        value
                    } else {
                        let value = read_u16(data, cursor)?;
                        cursor += 2;
                        value
                    };
                    sid = sid.wrapping_add(delta);
                    sids[gid] = sid;
                    gid += 1;
                }
            }
        }
        other => return Err(error(format!("CFF charset format {other} is unsupported"))),
    }
    Ok(sids)
}

/// The local subroutines of a name-keyed face, from its single Private DICT.
fn local_subrs(data: &[u8], cff: &Cff<'_>) -> Result<Option<Index>, PdfError> {
    let Some(private) = find(&cff.top_dict, OP_PRIVATE) else {
        return Ok(None);
    };
    let (offset, end) = private_dict(data, private)?;
    let dict = parse_dict(data, offset, end)?;
    let Some(subrs) = find(&dict, OP_SUBRS).and_then(|entry| entry.args.first()) else {
        return Ok(None);
    };
    let relative = usize::try_from(*subrs).map_err(|_| error("CFF Subrs offset is negative"))?;
    let start = offset
        .checked_add(relative)
        .ok_or_else(|| error("CFF Subrs offset is out of bounds"))?;
    Ok(Some(read_index(data, start)?))
}

/// The bytes of a private dict itself, without the local subrs after it.
fn private_dict(data: &[u8], entry: &Entry) -> Result<(usize, usize), PdfError> {
    let (size, offset) = private_arguments(entry)?;
    let size = usize::try_from(size).map_err(|_| error("CFF Private size is negative"))?;
    let end = offset
        .checked_add(size)
        .filter(|end| *end <= data.len())
        .ok_or_else(|| error("CFF Private dict is out of bounds"))?;
    Ok((offset, end))
}

/// End of a private dict, including its local subrs.
fn private_end(data: &[u8], entry: &Entry) -> Result<usize, PdfError> {
    let (offset, end) = private_dict(data, entry)?;
    let dict = parse_dict(data, offset, end)?;
    let Some(subrs) = find(&dict, OP_SUBRS).and_then(|entry| entry.args.first()) else {
        return Ok(end);
    };
    let relative = usize::try_from(*subrs).map_err(|_| error("CFF Subrs offset is negative"))?;
    let start = offset
        .checked_add(relative)
        .ok_or_else(|| error("CFF Subrs offset is out of bounds"))?;
    read_index(data, start).map(|index| index.end)
}

fn subr_bias(count: usize) -> i32 {
    if count < 1240 {
        107
    } else if count < 33900 {
        1131
    } else {
        32768
    }
}

/// A Type 2 charstring number.
fn put_cs_int(value: i32, out: &mut Vec<u8>) {
    if (-107..=107).contains(&value) {
        out.push((value + 139) as u8);
    } else if (108..=1131).contains(&value) {
        let scaled = value - 108;
        out.push((247 + scaled / 256) as u8);
        out.push((scaled % 256) as u8);
    } else if (-1131..=-108).contains(&value) {
        let scaled = -value - 108;
        out.push((251 + scaled / 256) as u8);
        out.push((scaled % 256) as u8);
    } else {
        out.push(28);
        out.extend_from_slice(
            &(value.clamp(i16::MIN as i32, i16::MAX as i32) as i16).to_be_bytes(),
        );
    }
}

/// A Type 2 interpreter that re-emits a charstring as a flat outline.
///
/// Hints and subroutine calls are dropped, operands are written out as
/// numbers, and the current point is tracked so a composite can place its
/// second component. Type 1 leftovers (`hsbw`) are honoured as far as they
/// move the pen.
///
/// Re-emitting is what makes `seac` expandable: once the charset becomes
/// identity, the four-operand `endchar` can no longer name its components, so
/// the glyph is replaced by the two outlines drawn explicitly.
struct Type2<'a> {
    data: &'a [u8],
    locals: Option<&'a Index>,
    globals: &'a Index,
    stack: Vec<i32>,
    out: Vec<u8>,
    transient: [i32; 32],
    hints: usize,
    x: i32,
    y: i32,
    seac: Option<[i32; 4]>,
}

impl<'a> Type2<'a> {
    fn new(data: &'a [u8], locals: Option<&'a Index>, globals: &'a Index) -> Self {
        Self {
            data,
            locals,
            globals,
            stack: Vec::new(),
            out: Vec::new(),
            transient: [0; 32],
            hints: 0,
            x: 0,
            y: 0,
            seac: None,
        }
    }

    fn run(&mut self, program: &[u8], depth: usize) -> Result<(), PdfError> {
        if depth > 10 {
            return Err(error("CFF subroutine nesting is too deep"));
        }
        let mut cursor = 0usize;
        while cursor < program.len() {
            let byte = program[cursor];
            match byte {
                28 => {
                    self.stack.push(read_i16(program, cursor + 1)? as i32);
                    cursor += 3;
                }
                32..=246 => {
                    self.stack.push(byte as i32 - 139);
                    cursor += 1;
                }
                247..=250 => {
                    let next = *program
                        .get(cursor + 1)
                        .ok_or_else(|| error("charstring is truncated"))?;
                    self.stack
                        .push((byte as i32 - 247) * 256 + next as i32 + 108);
                    cursor += 2;
                }
                251..=254 => {
                    let next = *program
                        .get(cursor + 1)
                        .ok_or_else(|| error("charstring is truncated"))?;
                    self.stack
                        .push(-(byte as i32 - 251) * 256 - next as i32 - 108);
                    cursor += 2;
                }
                255 => {
                    self.stack.push(read_i32(program, cursor + 1)? >> 16);
                    cursor += 5;
                }
                _ => {
                    cursor += 1;
                    match byte {
                        1 | 3 | 18 | 23 => {
                            // A leading width operand makes the argument count
                            // odd; flooring the half leaves the stem count.
                            self.hints += self.stack.len() / 2;
                            self.stack.clear();
                        }
                        19 | 20 => {
                            self.hints += self.stack.len() / 2;
                            self.stack.clear();
                            cursor += self.hints.div_ceil(8);
                        }
                        4 | 21 | 22 => self.moveto(byte)?,
                        5 | 6 | 7 | 8 | 24 | 25 | 26 | 27 | 30 | 31 => self.path(byte)?,
                        9 => self.emit(9),
                        10 | 29 => {
                            let index = self.stack.pop().ok_or_else(|| {
                                error("charstring calls a subroutine with no index")
                            })?;
                            let table = if byte == 10 {
                                self.locals
                            } else {
                                Some(self.globals)
                            }
                            .ok_or_else(|| error("charstring calls a missing local subroutine"))?;
                            let subr = index + subr_bias(table.count());
                            if subr < 0 || subr as usize >= table.count() {
                                return Err(error(
                                    "charstring calls a subroutine that does not exist",
                                ));
                            }
                            self.run(table.item(self.data, subr as usize), depth + 1)?;
                        }
                        11 => return Ok(()),
                        13 => {
                            // hsbw: a Type 1 leftover. Its width is unused, but
                            // it does move the pen to the sidebearing.
                            if self.stack.len() >= 2 {
                                let sbx = self.stack[self.stack.len() - 2];
                                self.move_to(sbx);
                            }
                            self.stack.clear();
                        }
                        14 => {
                            self.endchar();
                            return Ok(());
                        }
                        12 => {
                            let op = *program
                                .get(cursor)
                                .ok_or_else(|| error("charstring ends mid-operator"))?;
                            cursor += 1;
                            self.escape(op)?;
                        }
                        other => {
                            return Err(error(format!(
                                "charstring operator {other} is unsupported"
                            )))
                        }
                    }
                }
            }
        }
        Ok(())
    }

    fn emit(&mut self, op: u8) {
        for value in &self.stack {
            put_cs_int(*value, &mut self.out);
        }
        self.out.push(op);
        self.stack.clear();
    }

    fn emit_escape(&mut self, op: u8) {
        for value in &self.stack {
            put_cs_int(*value, &mut self.out);
        }
        self.out.push(12);
        self.out.push(op);
        self.stack.clear();
    }

    fn moveto(&mut self, op: u8) -> Result<(), PdfError> {
        let arity = if op == 21 { 2 } else { 1 };
        if self.stack.len() > arity {
            self.stack.remove(0);
        }
        if self.stack.len() < arity {
            return Err(error("charstring moveto has too few operands"));
        }
        let (dx, dy) = match op {
            21 => (self.stack[0], self.stack[1]),
            22 => (self.stack[0], 0),
            _ => (0, self.stack[0]),
        };
        self.emit(op);
        self.x += dx;
        self.y += dy;
        Ok(())
    }

    fn move_to(&mut self, to_x: i32) {
        let dx = to_x - self.x;
        let dy = -self.y;
        put_cs_int(dx, &mut self.out);
        put_cs_int(dy, &mut self.out);
        self.out.push(21);
        self.x = to_x;
        self.y = 0;
    }

    fn path(&mut self, op: u8) -> Result<(), PdfError> {
        let (dx, dy) = self.delta(op)?;
        self.emit(op);
        self.x += dx;
        self.y += dy;
        Ok(())
    }

    /// How far the operator moves the pen.
    fn delta(&self, op: u8) -> Result<(i32, i32), PdfError> {
        let args = &self.stack;
        let alternating = |start_horizontal: bool| {
            let mut dx = 0;
            let mut dy = 0;
            for (index, value) in args.iter().enumerate() {
                let horizontal = (index % 2 == 0) == start_horizontal;
                if horizontal {
                    dx += value;
                } else {
                    dy += value;
                }
            }
            (dx, dy)
        };
        match op {
            // rlineto, rrcurveto, rcurveline and rlinecurve alternate x and y
            // in every operand, so the endpoint is just the two sums.
            5 | 8 | 24 | 25 => Ok(alternating(true)),
            6 => Ok(alternating(true)),
            7 => Ok(alternating(false)),
            26 => {
                // vvcurveto: {dx1?} dy1 dx2 dy2 dy3 per curve.
                let mut cursor = 0;
                let mut dx = 0;
                if args.len() % 4 == 1 {
                    dx = args[0];
                    cursor = 1;
                }
                let mut dy = 0;
                while cursor + 4 <= args.len() {
                    dx += args[cursor + 1];
                    dy += args[cursor] + args[cursor + 2] + args[cursor + 3];
                    cursor += 4;
                }
                Ok((dx, dy))
            }
            27 => {
                // hhcurveto: {dy1?} dx1 dx2 dy2 dx3 per curve.
                let mut cursor = 0;
                let mut dy = 0;
                if args.len() % 4 == 1 {
                    dy = args[0];
                    cursor = 1;
                }
                let mut dx = 0;
                while cursor + 4 <= args.len() {
                    dx += args[cursor] + args[cursor + 1] + args[cursor + 3];
                    dy += args[cursor + 2];
                    cursor += 4;
                }
                Ok((dx, dy))
            }
            30 | 31 => Ok(alternating_curves(args, op == 31)),
            _ => Err(error("charstring path operator is unsupported")),
        }
    }

    fn endchar(&mut self) {
        if self.stack.len() == 5 || self.stack.len() == 1 {
            self.stack.remove(0);
        }
        if self.stack.len() == 4 {
            self.seac = Some([self.stack[0], self.stack[1], self.stack[2], self.stack[3]]);
        }
        self.stack.clear();
    }

    fn pop(&mut self) -> Result<i32, PdfError> {
        self.stack
            .pop()
            .ok_or_else(|| error("charstring operator has too few operands"))
    }

    fn escape(&mut self, op: u8) -> Result<(), PdfError> {
        match op {
            0 => Ok(()),
            3 => {
                let b = self.pop()?;
                let a = self.pop()?;
                self.stack.push(i32::from(a != 0 && b != 0));
                Ok(())
            }
            4 => {
                let b = self.pop()?;
                let a = self.pop()?;
                self.stack.push(i32::from(a != 0 || b != 0));
                Ok(())
            }
            5 => {
                let a = self.pop()?;
                self.stack.push(i32::from(a == 0));
                Ok(())
            }
            9 => {
                let a = self.pop()?;
                self.stack.push(a.abs());
                Ok(())
            }
            10 => {
                let b = self.pop()?;
                let a = self.pop()?;
                self.stack.push(a + b);
                Ok(())
            }
            11 => {
                let b = self.pop()?;
                let a = self.pop()?;
                self.stack.push(a - b);
                Ok(())
            }
            12 => {
                let b = self.pop()?;
                let a = self.pop()?;
                if b == 0 {
                    return Err(error("charstring divides by zero"));
                }
                self.stack.push((a + b / 2) / b);
                Ok(())
            }
            14 => {
                let a = self.pop()?;
                self.stack.push(-a);
                Ok(())
            }
            15 => {
                let b = self.pop()?;
                let a = self.pop()?;
                self.stack.push(i32::from(a == b));
                Ok(())
            }
            18 => {
                self.pop()?;
                Ok(())
            }
            20 => {
                let index = self.pop()?;
                let value = self.pop()?;
                if !(0..32).contains(&index) {
                    return Err(error("charstring writes outside the transient array"));
                }
                self.transient[index as usize] = value;
                Ok(())
            }
            21 => {
                let index = self.pop()?;
                if !(0..32).contains(&index) {
                    return Err(error("charstring reads outside the transient array"));
                }
                self.stack.push(self.transient[index as usize]);
                Ok(())
            }
            22 => {
                let v2 = self.pop()?;
                let v1 = self.pop()?;
                let s2 = self.pop()?;
                let s1 = self.pop()?;
                self.stack.push(if v1 <= v2 { s1 } else { s2 });
                Ok(())
            }
            24 => {
                let b = self.pop()?;
                let a = self.pop()?;
                self.stack.push(a * b);
                Ok(())
            }
            26 => {
                let a = self.pop()?;
                let mut root = 0i32;
                while (root + 1) * (root + 1) <= a {
                    root += 1;
                }
                self.stack.push(root);
                Ok(())
            }
            27 => {
                let a = self.pop()?;
                self.stack.push(a);
                Ok(())
            }
            28 => {
                let b = self.pop()?;
                let a = self.pop()?;
                self.stack.push(b);
                self.stack.push(a);
                Ok(())
            }
            29 => {
                let index = self.pop()?;
                if index < 0 || index as usize >= self.stack.len() {
                    return Err(error("charstring indexes past the operand stack"));
                }
                let value = self.stack[self.stack.len() - 1 - index as usize];
                self.stack.push(value);
                Ok(())
            }
            30 => {
                let count = self.pop()?;
                let shift = self.pop()?;
                let len = self.stack.len();
                if count < 0 || count as usize > len {
                    return Err(error("charstring rolls past the operand stack"));
                }
                let count = count as usize;
                if count == 0 {
                    return Ok(());
                }
                let slice = &mut self.stack[len - count..];
                let shift = shift.rem_euclid(count as i32) as usize;
                slice.rotate_right(shift);
                Ok(())
            }
            34..=37 => {
                let (dx, dy) = self.flex_delta(op)?;
                self.emit_escape(op);
                self.x += dx;
                self.y += dy;
                Ok(())
            }
            other => Err(error(format!(
                "charstring escape operator 12 {other} is unsupported"
            ))),
        }
    }

    fn flex_delta(&self, op: u8) -> Result<(i32, i32), PdfError> {
        let args = &self.stack;
        match (op, args.len()) {
            // hflex: dx1 dx2 dy2 dx3 dx4 dx5 dx6 — ends level with its start.
            (34, 7) => Ok((args[0] + args[1] + args[3] + args[4] + args[5] + args[6], 0)),
            // flex: six deltas and a flex depth.
            (35, 13) => Ok((
                args[0] + args[2] + args[4] + args[6] + args[8] + args[10],
                args[1] + args[3] + args[5] + args[7] + args[9] + args[11],
            )),
            // hflex1: dx1 dy1 dx2 dy2 dx3 dx4 dx5 dy5 dx6.
            (36, 9) => Ok((args[0] + args[2] + args[4] + args[5] + args[6] + args[8], 0)),
            // flex1: the last point lands on whichever axis moved further.
            (37, 11) => {
                let dx = args[0] + args[2] + args[4] + args[6] + args[8];
                let dy = args[1] + args[3] + args[5] + args[7] + args[9];
                if dx.abs() > dy.abs() {
                    Ok((dx + args[10], 0))
                } else {
                    Ok((0, dy + args[10]))
                }
            }
            _ => Err(error("charstring flex has the wrong operand count")),
        }
    }
}

/// `hvcurveto` (starts horizontal) and `vhcurveto` (starts vertical).
fn alternating_curves(args: &[i32], start_horizontal: bool) -> (i32, i32) {
    let extra = if args.len() % 4 == 1 {
        Some(args[args.len() - 1])
    } else {
        None
    };
    let curves = args.len() / 4;
    let mut dx = 0;
    let mut dy = 0;
    let mut horizontal = start_horizontal;
    for curve in 0..curves {
        let base = curve * 4;
        let extra = if curve + 1 == curves {
            extra.unwrap_or(0)
        } else {
            0
        };
        if horizontal {
            dx += args[base] + args[base + 1] + extra;
            dy += args[base + 2] + args[base + 3];
        } else {
            dy += args[base] + args[base + 2] + extra;
            dx += args[base + 1] + args[base + 3];
        }
        horizontal = !horizontal;
    }
    (dx, dy)
}

/// The `seac` operands of a charstring, when it ends in one.
fn seac_of(
    data: &[u8],
    locals: Option<&Index>,
    globals: &Index,
    program: &[u8],
) -> Result<Option<[i32; 4]>, PdfError> {
    let mut machine = Type2::new(data, locals, globals);
    machine.run(program, 0)?;
    Ok(machine.seac)
}

/// Draw `base` and `accent` as one outline, with the accent at `(adx, ady)`.
fn expand_seac(
    data: &[u8],
    locals: Option<&Index>,
    globals: &Index,
    base: &[u8],
    accent: &[u8],
    adx: i32,
    ady: i32,
) -> Result<Vec<u8>, PdfError> {
    let mut machine = Type2::new(data, locals, globals);
    machine.run(base, 0)?;
    if machine.seac.is_some() {
        return Err(error("a seac composite refers to another seac composite"));
    }
    // The accent is a charstring of its own, so it starts at the origin. Move
    // the pen to where its origin belongs before drawing it.
    let dx = adx - machine.x;
    let dy = ady - machine.y;
    put_cs_int(dx, &mut machine.out);
    put_cs_int(dy, &mut machine.out);
    machine.out.push(21);
    machine.x = adx;
    machine.y = ady;
    machine.run(accent, 0)?;
    if machine.seac.is_some() {
        return Err(error("a seac composite refers to another seac composite"));
    }
    machine.out.push(14);
    Ok(machine.out)
}

/// Glyph id for a StandardEncoding code, through the charset.
fn resolve_seac(sids: &[u16], code: i32) -> Result<u16, PdfError> {
    let sid = usize::try_from(code)
        .ok()
        .and_then(|code| STANDARD_ENCODING_SID.get(code).copied())
        .filter(|sid| *sid != 0xffff)
        .ok_or_else(|| error(format!("seac names code {code}, which has no glyph")))?;
    sids.iter()
        .position(|candidate| *candidate == sid)
        .map(|gid| gid as u16)
        .ok_or_else(|| {
            error(format!(
                "seac names a glyph whose SID {sid} is not in this font's charset"
            ))
        })
}

/// Rebuild `data` as a CID-keyed CFF holding only `used` glyphs.
///
/// The CharStrings INDEX is compacted to `[.notdef] + used`, and the charset is
/// rewritten so that the CID of each surviving glyph stays its *original* glyph
/// id. That is the one invariant the rest of the pipeline depends on: the
/// content stream writes the original id as the two-byte code, and `/W` and
/// `ToUnicode` are keyed by it, so only the program is renumbered (ADR-0046).
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
    let charset_offset = find(&cff.top_dict, OP_CHARSET)
        .and_then(|entry| entry.args.first())
        .and_then(|value| usize::try_from(*value).ok())
        .ok_or_else(|| error("CFF has no charset"))?;

    let cid = find(&cff.top_dict, OP_ROS).is_some();

    let mut include = vec![false; count];
    include[0] = true;
    for gid in used {
        if let Some(slot) = include.get_mut(*gid as usize) {
            *slot = true;
        }
    }
    // The compact order: glyph 0 first, then every used glyph in ascending id
    // order. Glyph 0 has to stay at index 0 because that is `.notdef`.
    let order: Vec<u16> = (0..count)
        .filter(|gid| include[*gid])
        .map(|gid| gid as u16)
        .collect();
    let kept = order.len();
    let cid_count = order.last().map_or(1, |gid| usize::from(*gid) + 1);

    // A name-keyed face loses its glyph names when the charset becomes
    // identity, so a `seac` composite is drawn out instead of referenced.
    let mut expanded: Vec<Option<Vec<u8>>> = Vec::new();
    if !cid {
        let locals = local_subrs(data, &cff)?;
        let mut owned: Vec<Option<Vec<u8>>> = vec![None; count];
        let sids = if charset_offset >= 3 {
            Some(charset_sids(data, charset_offset, count)?)
        } else {
            None
        };
        for (gid, &keep) in include.iter().enumerate() {
            if !keep {
                continue;
            }
            let program = charstrings.item(data, gid);
            let Some(seac) = seac_of(data, locals.as_ref(), &cff.global, program)? else {
                continue;
            };
            let sids = sids.as_ref().ok_or_else(|| {
                error("a seac composite needs an explicit charset to find its components")
            })?;
            let base = resolve_seac(sids, seac[2])? as usize;
            let accent = resolve_seac(sids, seac[3])? as usize;
            if base >= count || accent >= count {
                return Err(error("a seac composite refers past the last glyph"));
            }
            owned[gid] = Some(expand_seac(
                data,
                locals.as_ref(),
                &cff.global,
                charstrings.item(data, base),
                charstrings.item(data, accent),
                seac[0],
                seac[1],
            )?);
        }
        expanded = owned;
    }

    // 1. The charset maps each new glyph id to the CID the text operator
    //    writes. Format 0 is a plain array, and the array is now short: only
    //    the glyphs that survived have an entry.
    let mut charset = Vec::with_capacity(2 * kept);
    charset.push(0u8);
    for gid in &order[1..] {
        charset.extend_from_slice(&gid.to_be_bytes());
    }

    // 2. CharStrings, compacted; unused glyphs are gone rather than blanked.
    let mut programs: Vec<&[u8]> = Vec::with_capacity(kept);
    for gid in &order {
        let gid = *gid as usize;
        let drawn = expanded.get(gid).and_then(|value| value.as_deref());
        programs.push(drawn.unwrap_or_else(|| charstrings.item(data, gid)));
    }
    let charstrings_data = write_index(&programs);

    // 3. FDSelect is rewritten into the compact id space. A CID-keyed face maps
    //    original ids to font dicts; a converted face gets a single range.
    let mut fdselect_owned = Vec::new();
    let fdselect: &[u8] = match (cid, find(&cff.top_dict, OP_FDSELECT)) {
        (true, Some(entry)) => {
            let offset = usize::try_from(entry.args[0])
                .map_err(|_| error("CFF FDSelect offset is negative"))?;
            let per_glyph = fd_per_glyph(data, offset, count)?;
            let compact: Vec<u8> = order.iter().map(|gid| per_glyph[*gid as usize]).collect();
            fdselect_owned = write_fdselect(&compact);
            &fdselect_owned
        }
        _ => {
            fdselect_owned.extend_from_slice(&[3, 0, 1, 0, 0, 0]);
            fdselect_owned.extend_from_slice(&(kept as u16).to_be_bytes());
            &fdselect_owned
        }
    };

    // 4. Font dicts and the private-dict region they point into.
    let mut font_dicts: Vec<Vec<Entry>> = Vec::new();
    let mut private_offsets: Vec<i32> = Vec::new();
    let (region_start, region_end) = if cid {
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
    } else if let Some(private) = find(&cff.top_dict, OP_PRIVATE) {
        let (size, offset) = private_arguments(private)?;
        font_dicts.push(vec![Entry {
            op: OP_PRIVATE,
            args: vec![size, 0],
            raw: private.raw,
        }]);
        private_offsets.push(offset as i32);
        (offset, private_end(data, private)?)
    } else {
        font_dicts.push(Vec::new());
        (data.len(), data.len())
    };
    if region_end < region_start || region_end > data.len() {
        return Err(error("CFF private dicts are out of bounds"));
    }
    let region = &data[region_start..region_end];

    // 5. Strings; a converted name-keyed face needs Adobe/Identity.
    let mut strings: Vec<&[u8]> = (0..cff.strings.count())
        .map(|index| cff.strings.item(data, index))
        .collect();
    let ros = if cid {
        find(&cff.top_dict, OP_ROS).map(|entry| entry.args.clone())
    } else {
        strings.push(b"Adobe");
        strings.push(b"Identity");
        let sid = 391 + cff.strings.count() as i32;
        Some(vec![sid, sid + 1, 0])
    };
    let strings_data = write_index(&strings);

    // 6. Sizes do not depend on the offsets: every patch is a five-byte int.
    let top_size = build_top(&cff, ros.as_deref(), &Offsets::default(), cid_count).len();
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

    let top = build_top(&cff, ros.as_deref(), &offsets, cid_count);
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

/// The font dict index of every glyph, in the source's id space.
fn fd_per_glyph(data: &[u8], offset: usize, count: usize) -> Result<Vec<u8>, PdfError> {
    let format = *data
        .get(offset)
        .ok_or_else(|| error("CFF FDSelect is out of bounds"))?;
    match format {
        0 => data
            .get(offset + 1..offset + 1 + count)
            .map(|slice| slice.to_vec())
            .ok_or_else(|| error("CFF FDSelect is truncated")),
        3 => {
            let ranges = read_u16(data, offset + 1)? as usize;
            let mut out = vec![0u8; count];
            for range in 0..ranges {
                let base = offset + 3 + range * 3;
                let first = read_u16(data, base)? as usize;
                let fd = *data
                    .get(base + 2)
                    .ok_or_else(|| error("CFF FDSelect is truncated"))?;
                let next = if range + 1 < ranges {
                    read_u16(data, base + 3)? as usize
                } else {
                    read_u16(data, offset + 3 + ranges * 3)? as usize
                };
                for slot in out.iter_mut().take(next.min(count)).skip(first) {
                    *slot = fd;
                }
            }
            Ok(out)
        }
        other => Err(error(format!("CFF FDSelect format {other} is unsupported"))),
    }
}

/// FDSelect format 3 over a fresh id space: one range per run of equal indices.
fn write_fdselect(per_glyph: &[u8]) -> Vec<u8> {
    let mut ranges: Vec<(u16, u8)> = Vec::new();
    for (gid, fd) in per_glyph.iter().enumerate() {
        if ranges.last().map(|(_, last)| last) != Some(fd) {
            ranges.push((gid as u16, *fd));
        }
    }
    let mut out = Vec::with_capacity(5 + ranges.len() * 3);
    out.push(3);
    out.extend_from_slice(&(ranges.len() as u16).to_be_bytes());
    for (first, fd) in &ranges {
        out.extend_from_slice(&first.to_be_bytes());
        out.push(*fd);
    }
    out.extend_from_slice(&(per_glyph.len() as u16).to_be_bytes());
    out
}

fn build_top(cff: &Cff<'_>, ros: Option<&[i32]>, offsets: &Offsets, cid_count: usize) -> Vec<u8> {
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
            OP_CID_COUNT => put_entry(OP_CID_COUNT, &[cid_count as i32], &mut out),
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
        put_entry(OP_CID_COUNT, &[cid_count as i32], &mut out);
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
    const SEAC: &[u8] = include_bytes!("../tests/fixtures/tiny-cff-seac.otf");

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
    fn a_subset_compacts_the_glyph_slots_and_keeps_the_cids() {
        let source = cff_table(CID);
        let (_, before) = charstrings(source);
        let used = [1u16, 3];
        let program = subset(source, &used, 6).expect("subset");
        let (cff, after) = charstrings(&program);

        assert_eq!(after.count(), 3, "only .notdef and the used glyphs remain");
        assert_eq!(after.item(&program, 0), before.item(source, 0), ".notdef");
        assert_eq!(after.item(&program, 1), before.item(source, 1));
        assert_eq!(
            after.item(&program, 2),
            before.item(source, 3),
            "glyph 3 moved to index 2"
        );

        // The charset is what keeps the rest of the pipeline honest: new glyph 2
        // answers to CID 3, which is the code the content stream writes.
        let charset = find(&cff.top_dict, OP_CHARSET).expect("charset").args[0] as usize;
        assert_eq!(program[charset], 0, "format 0");
        assert_eq!(read_u16(&program, charset + 1).unwrap(), 1);
        assert_eq!(read_u16(&program, charset + 3).unwrap(), 3);

        let ros = find(&cff.top_dict, OP_ROS).expect("ROS");
        assert_eq!(ros.args, vec![391, 392, 0]);
        assert_eq!(
            find(&cff.top_dict, OP_CID_COUNT).expect("CIDCount").args,
            vec![4],
            "CIDCount follows the highest CID, not the glyph count"
        );
    }

    #[test]
    fn the_compacted_font_is_smaller_than_the_source() {
        let source = cff_table(CID);
        let program = subset(source, &[1], 6).expect("subset");
        assert!(
            program.len() < source.len(),
            "{} vs {}",
            program.len(),
            source.len()
        );
    }

    #[test]
    fn a_name_keyed_face_is_promoted_to_cid_keyed() {
        let source = cff_table(NAME);
        let program = subset(source, &[1], 6).expect("subset");
        let cff = Cff::parse(&program).expect("parsed");
        assert!(find(&cff.top_dict, OP_ROS).is_some(), "ROS");
        assert!(
            find(&cff.top_dict, OP_PRIVATE).is_none(),
            "the Private dict moves into the FDArray"
        );
        assert_eq!(find(&cff.top_dict, OP_CID_COUNT).unwrap().args, vec![2]);

        let fdarray = read_index(
            &program,
            find(&cff.top_dict, OP_FDARRAY).unwrap().args[0] as usize,
        )
        .expect("fdarray");
        assert_eq!(fdarray.count(), 1);
        let dict = parse_dict(&program, fdarray.starts[0], fdarray.starts[1]).expect("font dict");
        let private = find(&dict, OP_PRIVATE).expect("private");
        let offset = private.args[1] as usize;
        assert_eq!(
            &program[offset..offset + 4],
            &[0x8b, 0x14, 0x8b, 0x15],
            "0 defaultWidthX 0 nominalWidthX, moved verbatim"
        );

        let fdselect = find(&cff.top_dict, OP_FDSELECT).expect("fdselect").args[0] as usize;
        assert_eq!(program[fdselect], 3, "format 3");
        assert_eq!(read_u16(&program, fdselect + 1).unwrap(), 1, "one range");
        assert_eq!(read_u16(&program, fdselect + 6).unwrap(), 2, "sentinel");
        assert!(
            program.windows(5).any(|window| window == b"Adobe"),
            "the ROS strings are appended to the string index"
        );
    }

    /// The pen position after every drawing operator, for a charstring the
    /// fixtures write (moveto, rlineto, closepath, endchar).
    fn points(program: &[u8]) -> Vec<(i32, i32)> {
        let mut stack: Vec<i32> = Vec::new();
        let (mut x, mut y) = (0, 0);
        let mut out = Vec::new();
        let mut cursor = 0;
        while cursor < program.len() {
            let byte = program[cursor];
            match byte {
                32..=246 => {
                    stack.push(byte as i32 - 139);
                    cursor += 1;
                }
                247..=250 => {
                    stack.push((byte as i32 - 247) * 256 + program[cursor + 1] as i32 + 108);
                    cursor += 2;
                }
                251..=254 => {
                    stack.push(-(byte as i32 - 251) * 256 - program[cursor + 1] as i32 - 108);
                    cursor += 2;
                }
                28 => {
                    stack.push(
                        i16::from_be_bytes([program[cursor + 1], program[cursor + 2]]) as i32,
                    );
                    cursor += 3;
                }
                21 => {
                    x += stack[0];
                    y += stack[1];
                    out.push((x, y));
                    stack.clear();
                    cursor += 1;
                }
                5 => {
                    for pair in stack.chunks(2) {
                        x += pair[0];
                        y += pair[1];
                        out.push((x, y));
                    }
                    stack.clear();
                    cursor += 1;
                }
                9 | 14 => cursor += 1,
                other => panic!("unexpected operator {other}"),
            }
        }
        out
    }

    #[test]
    fn a_seac_composite_is_drawn_out_at_the_right_place() {
        let source = cff_table(SEAC);
        let program = subset(source, &[1, 2, 3], 6).expect("subset");
        let (_, charstrings) = charstrings(&program);
        assert_eq!(charstrings.count(), 4, ".notdef plus the three used glyphs");
        // "A" is a 500x700 box; the accent is a 100x100 box at (200, 700) in
        // its own space, shifted to (25, 10). The composite walks the base,
        // moves to (25, 10), then draws the accent through a local subroutine.
        assert_eq!(
            points(charstrings.item(&program, 3)),
            vec![
                (0, 0),
                (500, 0),
                (500, 700),
                (0, 700),
                (25, 10),
                (225, 710),
                (325, 710),
                (325, 810),
                (225, 810),
            ]
        );
    }
}
