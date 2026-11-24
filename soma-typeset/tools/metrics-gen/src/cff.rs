//! CFF (Type 2) outline extents.
//!
//! A `glyf` face carries each glyph's bounding box in the font; a CFF face does
//! not. The only way to know where a mark's ink ends inside its advance is to
//! run the charstring, so this module is a bbox-only Type 2 interpreter: no
//! hinting, no outline output, no rewriting, no subroutinisation.
//!
//! It exists because `soma.metrics` records the ink gap that a justified line
//! pays for at its edges (ADR-0037). Without it every CFF face falls back to the
//! rule table's trim *budget* — a constant, not a measurement — and a line
//! ending on a comma leaves a visible 0.28 em hole (the comma's ink sits in the
//! left fifth of its em box in Source Han).

/// One CFF INDEX: a header plus a run of items in `data`.
#[derive(Clone, Copy, Default)]
struct Index {
    count: usize,
    off_size: usize,
    offsets_at: usize,
    data_at: usize,
}

impl Index {
    /// Parse an INDEX at `at`; returns the index and the byte after it.
    fn parse(data: &[u8], at: usize) -> Option<(Self, usize)> {
        let count = u16::from_be_bytes([*data.get(at)?, *data.get(at + 1)?]) as usize;
        if count == 0 {
            return Some((Self::default(), at + 2));
        }
        let off_size = *data.get(at + 2)? as usize;
        if !(1..=4).contains(&off_size) {
            return None;
        }
        let offsets_at = at + 3;
        let data_at = offsets_at + (count + 1) * off_size - 1;
        let last = read_offset(data, offsets_at + count * off_size, off_size)?;
        let end = data_at.checked_add(last)?;
        if end > data.len() {
            return None;
        }
        Some((
            Self {
                count,
                off_size,
                offsets_at,
                data_at,
            },
            end,
        ))
    }

    fn item<'a>(&self, data: &'a [u8], index: usize) -> Option<&'a [u8]> {
        if index >= self.count {
            return None;
        }
        let start = read_offset(data, self.offsets_at + index * self.off_size, self.off_size)?;
        let end = read_offset(
            data,
            self.offsets_at + (index + 1) * self.off_size,
            self.off_size,
        )?;
        if start >= end {
            return Some(&[]);
        }
        data.get(self.data_at + start..self.data_at + end)
    }
}

fn read_offset(data: &[u8], at: usize, size: usize) -> Option<usize> {
    let mut value = 0usize;
    for byte in 0..size {
        value = (value << 8) | *data.get(at + byte)? as usize;
    }
    Some(value)
}

/// A parsed CID-keyed (or single-FD) CFF face.
pub(crate) struct Cff<'a> {
    data: &'a [u8],
    charstrings: Index,
    global_subrs: Index,
    /// Local subrs per font dict. One entry for a non-CID face.
    local_subrs: Vec<Index>,
    /// `gid -> font dict index`; empty means "always 0".
    fd_select: Vec<u16>,
}

impl<'a> Cff<'a> {
    /// Parse the CFF table at `base`.
    pub(crate) fn parse(data: &'a [u8], base: usize) -> Option<Self> {
        let header_size = *data.get(base + 2)? as usize;
        let (_, after_names) = Index::parse(data, base + header_size)?;
        let (top, after_top) = Index::parse(data, after_names)?;
        let (_, after_strings) = Index::parse(data, after_top)?;
        let (global_subrs, _) = Index::parse(data, after_strings)?;
        let dict = top.item(data, 0)?;

        let mut charstrings_offset = None;
        let mut private = None;
        let mut fd_array = None;
        let mut fd_select_offset = None;
        let mut cid = false;
        for (operator, operands) in dict_entries(dict) {
            match operator {
                17 => charstrings_offset = operands.first().copied(),
                18 => {
                    if operands.len() == 2 {
                        private = Some((operands[0] as usize, operands[1] as usize));
                    }
                }
                0x0c1e => cid = true,
                0x0c24 => fd_array = operands.first().copied(),
                0x0c25 => fd_select_offset = operands.first().copied(),
                _ => {}
            }
        }
        let charstrings_offset = charstrings_offset? as usize;
        let (charstrings, _) = Index::parse(data, base + charstrings_offset)?;

        let mut local_subrs = Vec::new();
        let mut fd_select = Vec::new();
        if cid {
            let array_offset = fd_array? as usize;
            let (array, _) = Index::parse(data, base + array_offset)?;
            for index in 0..array.count {
                let entry = array.item(data, index)?;
                let private = dict_entries(entry)
                    .into_iter()
                    .find(|(operator, _)| *operator == 18)
                    .filter(|(_, operands)| operands.len() == 2);
                let subrs = match private {
                    Some((_, operands)) => {
                        subrs_of(data, base, operands[0] as usize, operands[1] as usize)
                    }
                    None => Index::default(),
                };
                local_subrs.push(subrs);
            }
            if let Some(offset) = fd_select_offset {
                fd_select = parse_fd_select(data, base + offset as usize, charstrings.count)?;
            }
        } else {
            let subrs = match private {
                Some((size, offset)) => subrs_of(data, base, size, offset),
                None => Index::default(),
            };
            local_subrs.push(subrs);
        }

        Some(Self {
            data,
            charstrings,
            global_subrs,
            local_subrs,
            fd_select,
        })
    }

    /// Ink box of one glyph in font units: `(x_min, y_min, x_max, y_max)`.
    pub(crate) fn ink_box(&self, gid: u16) -> Option<(i32, i32, i32, i32)> {
        let program = self.charstrings.item(self.data, gid as usize)?;
        if program.is_empty() {
            return None;
        }
        let fd = self.fd_select.get(gid as usize).copied().unwrap_or(0) as usize;
        let local = self.local_subrs.get(fd).copied().unwrap_or_default();
        let mut pen = Pen::new();
        let mut stack = Stack::new();
        self.run(program, &local, &mut pen, &mut stack, 0)?;
        pen.bounds()
    }

    fn run(
        &self,
        program: &[u8],
        local: &Index,
        pen: &mut Pen,
        stack: &mut Stack,
        depth: usize,
    ) -> Option<()> {
        if depth > 8 {
            return None;
        }
        let mut at = 0usize;
        while at < program.len() {
            let byte = *program.get(at)?;
            at += 1;
            if byte >= 32 || byte == 28 {
                stack.push(decode_number(program, &mut at, byte)?);
                continue;
            }
            match byte {
                // Hints: the arguments are stems, and `hintmask` needs their
                // count to skip the right number of mask bytes.
                1 | 3 | 18 | 23 => {
                    // Stem operators consume their arguments: leaving them on
                    // the stack shifts every later operand by one.
                    stack.stems += stack.values.len() / 2;
                    stack.values.clear();
                }
                19 | 20 => {
                    stack.stems += stack.values.len() / 2;
                    stack.values.clear();
                    at += stack.stems.div_ceil(8);
                }
                4 => {
                    let dy = stack.take(1)?[0];
                    pen.move_by(0.0, dy);
                }
                21 => {
                    let values = stack.take(2)?;
                    pen.move_by(values[0], values[1]);
                }
                22 => {
                    let dx = stack.take(1)?[0];
                    pen.move_by(dx, 0.0);
                }
                5 => {
                    let values = stack.take_all()?;
                    let mut at = 0usize;
                    while at + 1 < values.len() {
                        pen.line_by(values[at], values[at + 1]);
                        at += 2;
                    }
                }
                6 | 7 => {
                    let values = stack.take_all()?;
                    let mut horizontal = byte == 6;
                    for value in values {
                        if horizontal {
                            pen.line_by(value, 0.0);
                        } else {
                            pen.line_by(0.0, value);
                        }
                        horizontal = !horizontal;
                    }
                }
                8 => {
                    let values = stack.take_all()?;
                    let mut at = 0usize;
                    while at + 5 < values.len() {
                        pen.curve_by(
                            values[at],
                            values[at + 1],
                            values[at + 2],
                            values[at + 3],
                            values[at + 4],
                            values[at + 5],
                        );
                        at += 6;
                    }
                }
                24 => {
                    let mut values = stack.take_all()?;
                    if values.len() >= 2 {
                        let tail = values.split_off(values.len() - 2);
                        let mut at = 0usize;
                        while at + 1 < values.len() {
                            pen.line_by(values[at], values[at + 1]);
                            at += 2;
                        }
                        pen.line_by(tail[0], tail[1]);
                    }
                }
                25 => {
                    let mut values = stack.take_all()?;
                    if values.len() >= 6 {
                        let curves = values.split_off(values.len() - 6);
                        let mut at = 0usize;
                        while at + 1 < values.len() {
                            pen.line_by(values[at], values[at + 1]);
                            at += 2;
                        }
                        let mut at = 0usize;
                        while at + 5 < curves.len() {
                            pen.curve_by(
                                curves[at],
                                curves[at + 1],
                                curves[at + 2],
                                curves[at + 3],
                                curves[at + 4],
                                curves[at + 5],
                            );
                            at += 6;
                        }
                    }
                }
                26 | 27 => {
                    // vvcurveto / hhcurveto: the first group may carry the
                    // leading delta that the operator leaves at zero.
                    let values = stack.take_all()?;
                    let mut at = 0usize;
                    let mut lead = 0.0;
                    if values.len() % 4 == 1 {
                        lead = values[0];
                        at = 1;
                    }
                    while at + 3 < values.len() {
                        if byte == 26 {
                            // vvcurveto: dx1 dy1 dx2 dy2 dy3
                            pen.curve_by(
                                lead,
                                values[at],
                                values[at + 1],
                                values[at + 2],
                                0.0,
                                values[at + 3],
                            );
                        } else {
                            // hhcurveto: dy1 dx2 dy2 dx3
                            pen.curve_by(
                                values[at],
                                lead,
                                values[at + 1],
                                values[at + 2],
                                values[at + 3],
                                0.0,
                            );
                        }
                        lead = 0.0;
                        at += 4;
                    }
                }
                30 | 31 => {
                    // vhcurveto / hvcurveto alternate their free axis. A group
                    // is `dy1 dx2 dy2 dx3` (vertical first) or
                    // `dx1 dx2 dy2 dy3` (horizontal first); a fifth argument
                    // on the last group closes the other axis.
                    let values = stack.take_all()?;
                    let mut at = 0usize;
                    let mut horizontal = byte == 31;
                    while at + 3 < values.len() {
                        let last = values.len() - at == 5;
                        let extra = if last { values[at + 4] } else { 0.0 };
                        if horizontal {
                            pen.curve_by(
                                values[at],
                                0.0,
                                values[at + 1],
                                values[at + 2],
                                extra,
                                values[at + 3],
                            );
                        } else {
                            pen.curve_by(
                                0.0,
                                values[at],
                                values[at + 1],
                                values[at + 2],
                                values[at + 3],
                                extra,
                            );
                        }
                        at += if last { 5 } else { 4 };
                        horizontal = !horizontal;
                    }
                }
                10 | 29 => {
                    let raw = stack.pop()?;
                    let table = if byte == 10 {
                        local
                    } else {
                        &self.global_subrs
                    };
                    let index = raw as i32 + bias(table.count);
                    let body = table.item(self.data, index.max(0) as usize)?;
                    self.run(body, local, pen, stack, depth + 1)?;
                }
                11 => return Some(()),
                14 => {
                    // `endchar` with four arguments draws a `seac`-style
                    // accented glyph; the components are not in this program.
                    return Some(());
                }
                12 => {
                    let operator = *program.get(at)?;
                    at += 1;
                    flex(operator, pen, stack);
                }
                _ => stack.values.clear(),
            }
        }
        Some(())
    }
}

/// The four flex operators (`12 34`…`12 37`). They are rare in CJK programs,
/// but a comma is exactly the kind of glyph a hinting toolchain rewrites, so
/// skipping them would lose the bbox that this whole module exists for.
fn flex(operator: u8, pen: &mut Pen, stack: &mut Stack) {
    let Some(v) = stack.take_all() else { return };
    match (operator, v.len()) {
        // hflex: dx1 dx2 dy2 dx3 dx4 dx5 dx6
        (34, 7) => {
            pen.curve_by(v[0], 0.0, v[1], v[2], v[3], 0.0);
            pen.curve_by(v[4], 0.0, v[5], -v[2], v[6], 0.0);
        }
        // flex: dx1 dy1 … dx6 dy6 fd
        (35, 13) => {
            pen.curve_by(v[0], v[1], v[2], v[3], v[4], v[5]);
            pen.curve_by(v[6], v[7], v[8], v[9], v[10], v[11]);
        }
        // hflex1: dx1 dy1 dx2 dy2 dx3 dx4 dx5 dy5 dx6
        (36, 9) => {
            pen.curve_by(v[0], v[1], v[2], v[3], v[4], 0.0);
            let dy6 = -(v[1] + v[3] + v[7]);
            pen.curve_by(v[5], 0.0, v[6], v[7], v[8], dy6);
        }
        // flex1: dx1 dy1 … dx5 dy5 d6
        (37, 11) => {
            pen.curve_by(v[0], v[1], v[2], v[3], v[4], v[5]);
            let dx = v[0] + v[2] + v[4] + v[6] + v[8];
            let dy = v[1] + v[3] + v[5] + v[7] + v[9];
            if dx.abs() > dy.abs() {
                let dy6 = -(v[1] + v[3] + v[5] + v[7] + v[9]);
                pen.curve_by(v[6], v[7], v[8], v[9], v[10], dy6);
            } else {
                let dx6 = -(v[0] + v[2] + v[4] + v[6] + v[8]);
                pen.curve_by(v[6], v[7], v[8], v[9], dx6, v[10]);
            }
        }
        _ => stack.values.clear(),
    }
}

fn bias(count: usize) -> i32 {
    if count < 1240 {
        107
    } else if count < 33900 {
        1131
    } else {
        32768
    }
}

/// A number inside a DICT. Unlike a charstring, byte 29 is a 32-bit integer
/// and byte 30 is a real, so this cannot share the charstring decoder.
fn decode_dict_number(dict: &[u8], at: &mut usize, byte: u8) -> Option<f64> {
    match byte {
        29 => {
            let bytes = dict.get(*at..*at + 4)?;
            *at += 4;
            Some(f64::from(i32::from_be_bytes([
                bytes[0], bytes[1], bytes[2], bytes[3],
            ])))
        }
        30 => {
            let mut text = String::new();
            loop {
                let byte = *dict.get(*at)?;
                *at += 1;
                let mut done = false;
                for nibble in [byte >> 4, byte & 0x0f] {
                    match nibble {
                        0..=9 => text.push(char::from(b'0' + nibble)),
                        0x0a => text.push('.'),
                        0x0b => text.push('E'),
                        0x0c => text.push_str("E-"),
                        0x0e => text.push('-'),
                        0x0f => {
                            done = true;
                            break;
                        }
                        _ => {}
                    }
                }
                if done {
                    break;
                }
            }
            text.parse().ok().or(Some(0.0))
        }
        _ => decode_number(dict, at, byte),
    }
}

fn decode_number(program: &[u8], at: &mut usize, byte: u8) -> Option<f64> {
    match byte {
        28 => {
            let high = *program.get(*at)? as i8 as i16;
            let low = *program.get(*at + 1)? as i16;
            *at += 2;
            Some(f64::from((high << 8) | low))
        }
        32..=246 => Some(f64::from(byte) - 139.0),
        247..=250 => {
            let next = *program.get(*at)?;
            *at += 1;
            Some(f64::from(byte - 247) * 256.0 + f64::from(next) + 108.0)
        }
        251..=254 => {
            let next = *program.get(*at)?;
            *at += 1;
            Some(-(f64::from(byte - 251) * 256.0) - f64::from(next) - 108.0)
        }
        255 => {
            let bytes = program.get(*at..*at + 4)?;
            *at += 4;
            let raw = i32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
            Some(f64::from(raw) / 65536.0)
        }
        _ => None,
    }
}

struct Stack {
    values: Vec<f64>,
    stems: usize,
}

impl Stack {
    fn new() -> Self {
        Self {
            values: Vec::new(),
            stems: 0,
        }
    }
    fn push(&mut self, value: f64) {
        self.values.push(value);
    }
    fn pop(&mut self) -> Option<f64> {
        self.values.pop()
    }
    fn take(&mut self, count: usize) -> Option<Vec<f64>> {
        if self.values.len() < count {
            return None;
        }
        let values = self.values.split_off(self.values.len() - count);
        Some(values)
    }
    fn take_all(&mut self) -> Option<Vec<f64>> {
        Some(std::mem::take(&mut self.values))
    }
}

/// Accumulates the extents of a charstring's path. Curves are sampled at their
/// extrema, not at their control points: a control point can sit well outside
/// the ink, and a trim that overshoots pushes a mark into the margin.
struct Pen {
    x: f64,
    y: f64,
    min_x: f64,
    min_y: f64,
    max_x: f64,
    max_y: f64,
    touched: bool,
}

impl Pen {
    fn new() -> Self {
        Self {
            x: 0.0,
            y: 0.0,
            min_x: f64::MAX,
            min_y: f64::MAX,
            max_x: f64::MIN,
            max_y: f64::MIN,
            touched: false,
        }
    }

    fn touch(&mut self, x: f64, y: f64) {
        self.touched = true;
        self.min_x = self.min_x.min(x);
        self.min_y = self.min_y.min(y);
        self.max_x = self.max_x.max(x);
        self.max_y = self.max_y.max(y);
    }

    fn move_by(&mut self, dx: f64, dy: f64) {
        self.x += dx;
        self.y += dy;
    }

    fn line_by(&mut self, dx: f64, dy: f64) {
        self.x += dx;
        self.y += dy;
        self.touch(self.x, self.y);
    }

    fn curve_by(&mut self, dx1: f64, dy1: f64, dx2: f64, dy2: f64, dx3: f64, dy3: f64) {
        let x0 = self.x;
        let y0 = self.y;
        let x1 = x0 + dx1;
        let y1 = y0 + dy1;
        let x2 = x1 + dx2;
        let y2 = y1 + dy2;
        let x3 = x2 + dx3;
        let y3 = y2 + dy3;
        for t in extrema(x0, x1, x2, x3)
            .into_iter()
            .chain(extrema(y0, y1, y2, y3))
        {
            let u = 1.0 - t;
            let x = u * u * u * x0 + 3.0 * u * u * t * x1 + 3.0 * u * t * t * x2 + t * t * t * x3;
            let y = u * u * u * y0 + 3.0 * u * u * t * y1 + 3.0 * u * t * t * y2 + t * t * t * y3;
            self.touch(x, y);
        }
        self.touch(x3, y3);
        self.x = x3;
        self.y = y3;
    }

    fn bounds(&self) -> Option<(i32, i32, i32, i32)> {
        if !self.touched {
            return None;
        }
        let round = |value: f64| value.round() as i32;
        Some((
            round(self.min_x),
            round(self.min_y),
            round(self.max_x),
            round(self.max_y),
        ))
    }
}

/// Parameters where a cubic's derivative vanishes, plus the endpoints.
fn extrema(p0: f64, p1: f64, p2: f64, p3: f64) -> Vec<f64> {
    // B'(t) = 3[(p1-p0)(1-t)^2 + 2(p2-p1)(1-t)t + (p3-p2)t^2]
    let a = -p0 + 3.0 * p1 - 3.0 * p2 + p3;
    let b = 2.0 * (p0 - 2.0 * p1 + p2);
    let c = p1 - p0;
    let mut roots = Vec::new();
    let mut push = |t: f64| {
        if t > 0.0 && t < 1.0 {
            roots.push(t);
        }
    };
    if a.abs() < 1e-9 {
        if b.abs() > 1e-9 {
            push(-c / b);
        }
    } else {
        let discriminant = b * b - 4.0 * a * c;
        if discriminant >= 0.0 {
            let root = discriminant.sqrt();
            push((-b + root) / (2.0 * a));
            push((-b - root) / (2.0 * a));
        }
    }
    roots
}

/// Parse a DICT, calling back with `(operator, operands)`.
fn dict_entries(dict: &[u8]) -> Vec<(u16, Vec<f64>)> {
    let mut out = Vec::new();
    let mut operands: Vec<f64> = Vec::new();
    let mut at = 0usize;
    while at < dict.len() {
        let byte = dict[at];
        at += 1;
        if byte >= 32 || (28..=30).contains(&byte) {
            match decode_dict_number(dict, &mut at, byte) {
                Some(value) => operands.push(value),
                None => break,
            }
            continue;
        }
        if byte == 12 {
            let operator = 0x0c00 | *dict.get(at).unwrap_or(&0) as u16;
            at += 1;
            out.push((operator, std::mem::take(&mut operands)));
        } else {
            out.push((byte as u16, std::mem::take(&mut operands)));
        }
    }
    out
}

/// Local subrs of one `Private` dict, addressed by its `(size, offset)` pair.
fn subrs_of(data: &[u8], base: usize, size: usize, offset: usize) -> Index {
    let at = base + offset;
    let Some(private) = data.get(at..at.saturating_add(size)) else {
        return Index::default();
    };
    let Some(subrs) = dict_entries(private)
        .into_iter()
        .find(|(operator, _)| *operator == 19)
        .and_then(|(_, operands)| operands.first().copied())
    else {
        return Index::default();
    };
    if subrs < 0.0 {
        return Index::default();
    }
    Index::parse(data, at + subrs as usize)
        .map(|(index, _)| index)
        .unwrap_or_default()
}

/// Format 0 and 3 `FDSelect` tables: one font-dict index per glyph.
fn parse_fd_select(data: &[u8], at: usize, glyphs: usize) -> Option<Vec<u16>> {
    let format = *data.get(at)?;
    let mut out = vec![0u16; glyphs];
    match format {
        0 => {
            for (index, slot) in out.iter_mut().enumerate() {
                *slot = *data.get(at + 1 + index)? as u16;
            }
        }
        3 => {
            let ranges = u16::from_be_bytes([*data.get(at + 1)?, *data.get(at + 2)?]) as usize;
            for range in 0..ranges {
                let base = at + 3 + range * 3;
                let first = u16::from_be_bytes([*data.get(base)?, *data.get(base + 1)?]) as usize;
                let fd = *data.get(base + 2)? as u16;
                let next = if range + 1 < ranges {
                    let next_base = at + 3 + (range + 1) * 3;
                    u16::from_be_bytes([*data.get(next_base)?, *data.get(next_base + 1)?]) as usize
                } else {
                    u16::from_be_bytes([
                        *data.get(at + 3 + ranges * 3)?,
                        *data.get(at + 4 + ranges * 3)?,
                    ]) as usize
                };
                for slot in out.iter_mut().take(next.min(glyphs)).skip(first) {
                    *slot = fd;
                }
            }
        }
        _ => return None,
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Offset of the `CFF ` table inside an sfnt.
    fn cff_offset(data: &[u8]) -> usize {
        let count = u16::from_be_bytes([data[4], data[5]]) as usize;
        for index in 0..count {
            let record = 12 + index * 16;
            if &data[record..record + 4] == b"CFF " {
                return u32::from_be_bytes([
                    data[record + 8],
                    data[record + 9],
                    data[record + 10],
                    data[record + 11],
                ]) as usize;
            }
        }
        panic!("fixture has no CFF table");
    }

    /// The fixtures draw axis-aligned rectangles, so the expected ink is exact:
    /// `rect(w, h)` is `(0, 0) - (w, h)`.
    fn assert_fixture(fixture: &[u8]) {
        let cff = Cff::parse(fixture, cff_offset(fixture)).expect("parse");
        assert_eq!(cff.ink_box(1), Some((0, 0, 500, 700)), "gid 1");
        assert_eq!(cff.ink_box(2), Some((0, 0, 600, 700)), "gid 2");
        assert_eq!(cff.ink_box(3), Some((0, 0, 1000, 800)), "gid 3");
        // `.notdef` and the two blank glyphs are a bare `endchar`.
        assert_eq!(cff.ink_box(0), None);
        assert_eq!(cff.ink_box(5), None);
    }

    #[test]
    fn cid_keyed_fixture_matches_its_outlines() {
        assert_fixture(include_bytes!(
            "../../../crates/soma-render-pdf/tests/fixtures/tiny-cff.otf"
        ));
    }

    #[test]
    fn name_keyed_fixture_matches_its_outlines() {
        assert_fixture(include_bytes!(
            "../../../crates/soma-render-pdf/tests/fixtures/tiny-cff-name.otf"
        ));
    }

    #[test]
    fn cubic_extrema_are_inside_the_curve() {
        // A curve that bulges above the line between its endpoints.
        let roots = extrema(0.0, 100.0, 100.0, 0.0);
        assert_eq!(roots.len(), 1);
        assert!((roots[0] - 0.5).abs() < 1e-9);
    }

    #[test]
    fn a_straight_cubic_has_no_interior_extremum() {
        assert!(extrema(0.0, 1.0, 2.0, 3.0).is_empty());
    }
}
