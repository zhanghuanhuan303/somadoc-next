//! Extract numeric font metrics into the `soma.metrics/1` blob.
//!
//! The tool reads a TrueType/OpenType font (or a collection index) and writes
//! advances, line metrics, and per-codepoint widths. It never extracts outlines
//! and never ships a font: the output is a table of integers.
//!
//! Usage:
//!
//! ```text
//! metrics-gen --out data/noto-cjk.smt \
//!   --face serif:normal:/path/NotoSerifCJK-Regular.ttc#2 \
//!   --face sans:normal:/path/NotoSansCJK-Regular.ttc#2 \
//!   --face serif:bold:/path/NotoSerifCJK-Bold.ttc#2 \
//!   --face sans:bold:/path/NotoSansCJK-Bold.ttc#2
//! ```

use std::collections::BTreeMap;
use std::fs;
use std::process::ExitCode;

const UNIT: i32 = 4096;
const MAGIC: u32 = 0x534d_5431; // "SMT1"
const VERSION: u16 = 1;

/// Codepoint blocks whose advance is expected to be uniform (CJK).
const UNIFORM_BLOCKS: &[(u32, u32)] = &[
    (0x3000, 0x303f), // CJK symbols and punctuation
    (0x3040, 0x30ff), // kana
    (0x3400, 0x4dbf), // CJK extension A
    (0x4e00, 0x9fff), // CJK unified ideographs
    (0xf900, 0xfaff), // compatibility ideographs
    (0xfe30, 0xfe4f), // CJK compatibility forms
    (0xff00, 0xffef), // halfwidth and fullwidth forms
];

/// Explicitly enumerated ranges (proportional scripts and punctuation).
const ENUMERATED_RANGES: &[(u32, u32)] = &[(0x0020, 0x2fff)];

struct FaceSpec {
    family: u8,
    weight: u8,
    path: String,
    index: usize,
}

struct Font<'a> {
    data: &'a [u8],
    base: usize,
}

impl<'a> Font<'a> {
    fn open(data: &'a [u8], index: usize) -> Result<Self, String> {
        if data.len() < 12 {
            return Err("font is too short".into());
        }
        let tag = read_u32(data, 0)?;
        if tag == 0x7474_6366 {
            // "ttcf"
            let count = read_u32(data, 8)? as usize;
            if index >= count {
                return Err(format!(
                    "collection has {count} faces, index {index} is out of range"
                ));
            }
            let offset = read_u32(data, 12 + index * 4)? as usize;
            Ok(Self { data, base: offset })
        } else {
            if index != 0 {
                return Err("a single font has only face 0".into());
            }
            Ok(Self { data, base: 0 })
        }
    }

    fn table(&self, tag: &[u8; 4]) -> Option<(usize, usize)> {
        let count = read_u16(self.data, self.base + 4).ok()? as usize;
        for i in 0..count {
            let record = self.base + 12 + i * 16;
            let name = self.data.get(record..record + 4)?;
            if name == tag {
                let offset = read_u32(self.data, record + 8).ok()? as usize;
                let length = read_u32(self.data, record + 12).ok()? as usize;
                return Some((offset, length));
            }
        }
        None
    }

    fn required(&self, tag: &[u8; 4]) -> Result<(usize, usize), String> {
        self.table(tag)
            .ok_or_else(|| format!("missing table {}", String::from_utf8_lossy(tag)))
    }

    fn units_per_em(&self) -> Result<i32, String> {
        let (head, _) = self.required(b"head")?;
        let value = read_u16(self.data, head + 18)? as i32;
        if value == 0 {
            return Err("head.unitsPerEm is zero".into());
        }
        Ok(value)
    }

    fn line_metrics(&self) -> Result<(i32, i32, i32), String> {
        let (hhea, _) = self.required(b"hhea")?;
        let mut ascent = read_i16(self.data, hhea + 4)? as i32;
        let mut descent = -(read_i16(self.data, hhea + 6)? as i32);
        let mut leading = read_i16(self.data, hhea + 8)? as i32;

        if let Some((os2, length)) = self.table(b"OS/2") {
            if length >= 74 {
                let typo_ascent = read_i16(self.data, os2 + 68)? as i32;
                let typo_descent = read_i16(self.data, os2 + 70)? as i32;
                let typo_gap = read_i16(self.data, os2 + 72)? as i32;
                if typo_ascent > 0 && typo_descent < 0 {
                    ascent = typo_ascent;
                    descent = -typo_descent;
                    leading = typo_gap.max(0);
                }
            }
        }

        Ok((ascent, descent, leading))
    }

    fn cmap(&self) -> Result<BTreeMap<u32, u16>, String> {
        let (cmap, _) = self.required(b"cmap")?;
        let count = read_u16(self.data, cmap + 2)? as usize;
        let mut best: Option<(usize, u16)> = None;
        for i in 0..count {
            let record = cmap + 4 + i * 8;
            let platform = read_u16(self.data, record)?;
            let encoding = read_u16(self.data, record + 2)?;
            let offset = cmap + read_u32(self.data, record + 4)? as usize;
            let format = read_u16(self.data, offset)?;
            let rank = match (platform, encoding, format) {
                (3, 10, 12) => 4u16,
                (3, 1, 4) => 3,
                (0, _, 4) => 2,
                (0, _, 12) => 2,
                (3, 10, 4) => 1,
                _ => 0,
            };
            if rank == 0 {
                continue;
            }
            if best.map_or(true, |(_, current)| rank > current) {
                best = Some((offset, rank));
            }
        }
        let (offset, _) = best.ok_or("no usable cmap subtable")?;
        let format = read_u16(self.data, offset)?;
        match format {
            4 => self.cmap_format4(offset),
            12 => self.cmap_format12(offset),
            other => Err(format!("unsupported cmap format {other}")),
        }
    }

    fn cmap_format4(&self, offset: usize) -> Result<BTreeMap<u32, u16>, String> {
        let seg_count = read_u16(self.data, offset + 6)? as usize / 2;
        let end_codes = offset + 14;
        let start_codes = end_codes + seg_count * 2 + 2;
        let id_deltas = start_codes + seg_count * 2;
        let id_range_offsets = id_deltas + seg_count * 2;

        let mut map = BTreeMap::new();
        for i in 0..seg_count {
            let end = read_u16(self.data, end_codes + i * 2)? as u32;
            let start = read_u16(self.data, start_codes + i * 2)? as u32;
            if start > end {
                continue;
            }
            let delta = read_u16(self.data, id_deltas + i * 2)? as i32;
            let range_offset = read_u16(self.data, id_range_offsets + i * 2)? as usize;
            for cp in start..=end {
                if cp == 0xffff {
                    continue;
                }
                let glyph = if range_offset == 0 {
                    ((cp as i32 + delta) & 0xffff) as u16
                } else {
                    let address =
                        id_range_offsets + i * 2 + range_offset + (cp - start) as usize * 2;
                    let value = read_u16(self.data, address)?;
                    if value == 0 {
                        0
                    } else {
                        ((value as i32 + delta) & 0xffff) as u16
                    }
                };
                if glyph != 0 {
                    map.insert(cp, glyph);
                }
            }
        }
        Ok(map)
    }

    fn cmap_format12(&self, offset: usize) -> Result<BTreeMap<u32, u16>, String> {
        let groups = read_u32(self.data, offset + 12)? as usize;
        let mut map = BTreeMap::new();
        for i in 0..groups {
            let record = offset + 16 + i * 12;
            let start = read_u32(self.data, record)?;
            let end = read_u32(self.data, record + 4)?;
            let glyph = read_u32(self.data, record + 8)?;
            if end < start || end - start > 0x2_0000 {
                continue;
            }
            for cp in start..=end {
                let id = glyph + (cp - start);
                if id != 0 && id <= u16::MAX as u32 {
                    map.insert(cp, id as u16);
                }
            }
        }
        Ok(map)
    }

    fn advances(&self) -> Result<Vec<u16>, String> {
        let (maxp, _) = self.required(b"maxp")?;
        let glyphs = read_u16(self.data, maxp + 4)? as usize;
        let (hhea, _) = self.required(b"hhea")?;
        let metrics = read_u16(self.data, hhea + 34)? as usize;
        let (hmtx, _) = self.required(b"hmtx")?;
        if metrics == 0 {
            return Err("hhea.numberOfHMetrics is zero".into());
        }
        let mut out = Vec::with_capacity(glyphs);
        for gid in 0..glyphs {
            let index = gid.min(metrics - 1);
            let advance = read_u16(self.data, hmtx + index * 4)?;
            out.push(advance);
        }
        Ok(out)
    }
}

impl FaceSpec {
    fn parse(text: &str) -> Result<Self, String> {
        let mut parts = text.splitn(3, ':');
        let family = parts.next().ok_or("missing family")?;
        let weight = parts.next().ok_or("missing weight")?;
        let rest = parts.next().ok_or("missing path")?;
        let (path, index) = match rest.rsplit_once('#') {
            Some((path, index)) => (
                path.to_string(),
                index.parse::<usize>().map_err(|_| "bad face index")?,
            ),
            None => (rest.to_string(), 0),
        };
        let family = match family {
            "serif" => 0,
            "sans" => 1,
            other => return Err(format!("unknown family {other}")),
        };
        let weight = match weight {
            "normal" => 0,
            "bold" => 1,
            other => return Err(format!("unknown weight {other}")),
        };
        Ok(Self {
            family,
            weight,
            path,
            index,
        })
    }
}

fn read_u16(data: &[u8], offset: usize) -> Result<u16, String> {
    let bytes = data
        .get(offset..offset + 2)
        .ok_or_else(|| format!("read past end at {offset}"))?;
    Ok(u16::from_be_bytes([bytes[0], bytes[1]]))
}

fn read_i16(data: &[u8], offset: usize) -> Result<i16, String> {
    Ok(read_u16(data, offset)? as i16)
}

fn read_u32(data: &[u8], offset: usize) -> Result<u32, String> {
    let bytes = data
        .get(offset..offset + 4)
        .ok_or_else(|| format!("read past end at {offset}"))?;
    Ok(u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

fn scale(value: i32, upem: i32) -> i32 {
    let numerator = value as i64 * UNIT as i64;
    let denominator = upem as i64;
    if numerator >= 0 {
        ((numerator + denominator / 2) / denominator) as i32
    } else {
        -(((-numerator) + denominator / 2) / denominator) as i32
    }
}

fn modal(values: &[i32]) -> Option<(i32, usize)> {
    let mut counts: BTreeMap<i32, usize> = BTreeMap::new();
    for value in values {
        *counts.entry(*value).or_insert(0) += 1;
    }
    counts
        .into_iter()
        .max_by_key(|(_, count)| *count)
        .map(|(value, count)| (value, count))
}

struct OutFace {
    family: u8,
    weight: u8,
    ascent: i32,
    descent: i32,
    leading: i32,
    space: i32,
    default_advance: i32,
    blocks: Vec<(u32, u32, i32)>,
    entries: Vec<(u32, i32)>,
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("metrics-gen: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let mut out = String::from("crates/soma-metrics/data/noto-cjk.smt");
    let mut specs = Vec::new();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--out" => out = args.next().ok_or("--out needs a value")?,
            "--face" => specs.push(FaceSpec::parse(
                &args.next().ok_or("--face needs a value")?,
            )?),
            other => return Err(format!("unknown argument {other}")),
        }
    }
    if specs.is_empty() {
        return Err("at least one --face is required".into());
    }

    let mut faces = Vec::new();
    for spec in &specs {
        let data = fs::read(&spec.path).map_err(|error| format!("{}: {error}", spec.path))?;
        let font = Font::open(&data, spec.index)?;
        let upem = font.units_per_em()?;
        let (ascent, descent, leading) = font.line_metrics()?;
        let map = font.cmap()?;
        let advances = font.advances()?;

        let width = |cp: u32| -> Option<i32> {
            map.get(&cp)
                .and_then(|gid| advances.get(*gid as usize))
                .map(|advance| scale(*advance as i32, upem))
        };

        let space = width(0x20).unwrap_or(scale(upem / 2, upem));
        let mut entries: Vec<(u32, i32)> = Vec::new();
        let mut blocks: Vec<(u32, u32, i32)> = Vec::new();

        for &(start, end) in UNIFORM_BLOCKS {
            let mut values = Vec::new();
            let mut covered = Vec::new();
            for cp in start..=end {
                if let Some(advance) = width(cp) {
                    values.push(advance);
                    covered.push(cp);
                }
            }
            if covered.is_empty() {
                continue;
            }
            match modal(&values) {
                Some((value, count)) if count * 10 >= values.len() * 9 => {
                    blocks.push((start, end, value));
                    for cp in covered {
                        let advance = width(cp).unwrap();
                        if advance != value {
                            entries.push((cp, advance));
                        }
                    }
                }
                _ => {
                    for cp in covered {
                        entries.push((cp, width(cp).unwrap()));
                    }
                }
            }
        }

        for &(start, end) in ENUMERATED_RANGES {
            for cp in start..=end {
                if let Some(advance) = width(cp) {
                    entries.push((cp, advance));
                }
            }
        }

        entries.sort_unstable();
        entries.dedup_by_key(|(cp, _)| *cp);
        blocks.sort_unstable();

        let default_advance = blocks
            .iter()
            .find(|(start, _, _)| *start == 0x4e00)
            .map(|(_, _, advance)| *advance)
            .unwrap_or(UNIT);

        println!(
            "face {}/{}: upem={} ascii-entries={} blocks={} entries={}",
            spec.family,
            spec.weight,
            upem,
            entries.len(),
            blocks.len(),
            entries.len()
        );

        faces.push(OutFace {
            family: spec.family,
            weight: spec.weight,
            ascent: scale(ascent, upem),
            descent: scale(descent, upem),
            leading: scale(leading, upem),
            space,
            default_advance,
            blocks,
            entries,
        });
    }

    let mut bytes: Vec<u8> = Vec::new();
    bytes.extend_from_slice(&MAGIC.to_le_bytes());
    bytes.extend_from_slice(&VERSION.to_le_bytes());
    bytes.extend_from_slice(&(UNIT as u16).to_le_bytes());
    bytes.extend_from_slice(&(faces.len() as u16).to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    for face in &faces {
        bytes.push(face.family);
        bytes.push(face.weight);
        bytes.push(0);
        bytes.push(0);
        for value in [
            face.ascent,
            face.descent,
            face.leading,
            face.space,
            face.default_advance,
        ] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes.extend_from_slice(&(face.blocks.len() as u16).to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes());
        for (start, end, advance) in &face.blocks {
            bytes.extend_from_slice(&start.to_le_bytes());
            bytes.extend_from_slice(&end.to_le_bytes());
            bytes.extend_from_slice(&advance.to_le_bytes());
        }
        bytes.extend_from_slice(&(face.entries.len() as u32).to_le_bytes());
        for (cp, advance) in &face.entries {
            bytes.extend_from_slice(&cp.to_le_bytes());
            bytes.extend_from_slice(&advance.to_le_bytes());
        }
    }

    if let Some(parent) = std::path::Path::new(&out).parent() {
        fs::create_dir_all(parent).map_err(|error| format!("{}: {error}", parent.display()))?;
    }
    fs::write(&out, &bytes).map_err(|error| format!("{out}: {error}"))?;
    println!("wrote {out} ({} bytes, {} faces)", bytes.len(), faces.len());
    Ok(())
}
