//! Extract numeric font metrics into the `soma.metrics/2` blob.
//!
//! The tool reads a TrueType/OpenType font (or a collection index) and writes
//! advances, line metrics, per-codepoint widths, and — when the font has
//! `glyf` outlines — the ink gap of punctuation. It never extracts outlines
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
use std::fmt::Write as _;
use std::fs;
use std::process::ExitCode;

const UNIT: i32 = 4096;
const MAGIC: u32 = 0x534d_5431; // "SMT1"
const VERSION: u16 = 2;

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
const ENUMERATED_RANGES: &[(u32, u32)] = &[
    (0x0020, 0x2fff),
    // Mathematical alphanumeric symbols: italic/bold/script Latin and Greek.
    (0x1d400, 0x1d7ff),
];

/// Codepoints whose outline matters for line-edge alignment. Punctuation only:
/// recording ink for every Han glyph would triple the table for no gain.
const INK_RANGES: &[(u32, u32)] = &[
    (0x0021, 0x002f), // ! " # $ % & ' ( ) * + , - . /
    (0x003a, 0x0040), // : ; < = > ? @
    (0x005b, 0x0060), // [ \ ] ^ _ `
    (0x007b, 0x007e), // { | } ~
    (0x2000, 0x206f), // general punctuation: quotes, dashes, ellipsis
    (0x3000, 0x303f), // CJK symbols and punctuation
    (0xfe30, 0xfe4f), // CJK compatibility forms
    (0xff00, 0xffef), // halfwidth and fullwidth forms
];

/// Full-width marks a Chinese body face is expected to draw one em wide.
const FULL_WIDTH_MARKS: &[char] = &[
    '。', '，', '、', '；', '：', '？', '！', '（', '）', '「', '」', '《', '》',
];

/// The most frequent Han characters. A body face must have all of them.
const COMMON_HAN: &str = "的一是不了人我在有他这为之大来以个中上们到说国和地也子时道出而要于就下得可你年生自会那后能对着事其里所去行过家十用发天如然作方成者多日都三小军二无同么经法当起与好看学进种将还分此心前面又定见只主没公从";

/// How many evenly spaced codepoints stand in for the unified ideograph block.
const CJK_SAMPLE: u32 = 512;

/// Evenly spaced probes across CJK Unified Ideographs. Enough to separate a
/// symbol font from a GB2312 face from a full 20k-glyph face.
fn cjk_sample() -> impl Iterator<Item = u32> {
    let start = 0x4E00u32;
    let end = 0x9FFFu32;
    let step = (end - start) / CJK_SAMPLE;
    (0..CJK_SAMPLE).map(move |index| start + index * step)
}

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
            if match best {
                None => true,
                Some((_, current)) => rank > current,
            } {
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

    /// Horizontal extent of a glyph's outline in font units. `None` for an
    /// empty glyph (a space), for a composite that points at nothing, or for a
    /// font whose outlines are CFF rather than `glyf`.
    fn ink_bounds(&self, gid: u16) -> Result<Option<(i32, i32)>, String> {
        let Some((head, _)) = self.table(b"head") else {
            return Ok(None);
        };
        let (Some((loca, _)), Some((glyf, _))) = (self.table(b"loca"), self.table(b"glyf")) else {
            return Ok(None);
        };
        let long_offsets = read_i16(self.data, head + 50)? != 0;
        let index = gid as usize;
        let (start, end) = if long_offsets {
            (
                read_u32(self.data, loca + index * 4)? as usize,
                read_u32(self.data, loca + index * 4 + 4)? as usize,
            )
        } else {
            (
                read_u16(self.data, loca + index * 2)? as usize * 2,
                read_u16(self.data, loca + index * 2 + 2)? as usize * 2,
            )
        };
        if start >= end {
            return Ok(None);
        }
        let offset = glyf + start;
        let contours = read_i16(self.data, offset)?;
        if contours == 0 {
            return Ok(None);
        }
        let x_min = read_i16(self.data, offset + 2)? as i32;
        let x_max = read_i16(self.data, offset + 6)? as i32;
        Ok(Some((x_min, x_max)))
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
            // A dedicated math face: used for formula symbols, not for text.
            "math" => 2,
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
    counts.into_iter().max_by_key(|(_, count)| *count)
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
    ink: Vec<(u32, i32, i32)>,
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

/// One font face scored for Chinese body typesetting.
struct AuditRow {
    path: String,
    face: usize,
    outlines: bool,
    glyphs: usize,
    coverage: f64,
    uniform: f64,
    punctuation: f64,
    ink_range: f64,
    ink_mean: f64,
    ink_std: f64,
    line_ratio: f64,
    score: f64,
    verdict: &'static str,
}

fn audit(directories: &[String], top: usize, fingerprints_out: Option<&str>) -> Result<(), String> {
    let mut paths: Vec<std::path::PathBuf> = Vec::new();
    for directory in directories {
        collect_fonts(std::path::Path::new(directory), &mut paths);
    }
    paths.sort();
    paths.dedup();

    let mut measured: Vec<(String, usize, Measure)> = Vec::new();
    for path in &paths {
        let Ok(data) = fs::read(path) else { continue };
        let faces = face_count(&data);
        for face in 0..faces {
            if let Some(measure) = measure(&data, face) {
                measured.push((path.display().to_string(), face, measure));
            }
        }
    }

    if let Some(out) = fingerprints_out {
        write_fingerprints(&measured, out)?;
    }

    let mut rows: Vec<AuditRow> = measured
        .iter()
        .map(|(path, face, measure)| row(measure, path.clone(), *face))
        .collect();

    // Usable faces first, then by score, then by path: a stable order so a
    // rerun against the same font directory produces the same table.
    rows.sort_by(|left, right| {
        let left_usable = left.verdict == "usable";
        let right_usable = right.verdict == "usable";
        right_usable
            .cmp(&left_usable)
            .then(right.score.total_cmp(&left.score))
            .then(left.path.cmp(&right.path))
            .then(left.face.cmp(&right.face))
    });

    println!(
        "score,verdict,path,face,glyphs,coverage,uniform,punct,ink_range,ink_mean,ink_std,line,outlines"
    );
    for row in rows.iter().take(top) {
        println!(
            "{:.1},{},{},{},{},{:.3},{:.3},{:.3},{:.3},{:.3},{:.3},{:.3},{}",
            row.score,
            row.verdict,
            row.path,
            row.face,
            row.glyphs,
            row.coverage,
            row.uniform,
            row.punctuation,
            row.ink_range,
            row.ink_mean,
            row.ink_std,
            row.line_ratio,
            row.outlines
        );
    }
    let usable = rows.iter().filter(|row| row.verdict == "usable").count();
    eprintln!(
        "audited {} faces in {} files; {usable} usable",
        rows.len(),
        paths.len()
    );
    Ok(())
}

fn collect_fonts(directory: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_fonts(&path, out);
            continue;
        }
        let extension = path
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        if matches!(extension.as_str(), "ttf" | "ttc" | "otf") {
            out.push(path);
        }
    }
}

fn face_count(data: &[u8]) -> usize {
    if data.len() >= 12 && read_u32(data, 0).ok() == Some(0x7474_6366) {
        read_u32(data, 8).ok().unwrap_or(1) as usize
    } else {
        1
    }
}

/// Every number the audit and the fingerprint need, measured once per face.
/// Ratios are 0..1 unless the name says otherwise; lengths are in em.
struct Measure {
    glyphs: usize,
    outlines: bool,
    coverage: f64,
    uniform: f64,
    punct_em: f64,
    punct_ok: f64,
    ink_range: f64,
    ink_mean: f64,
    ink_std: f64,
    ink_left_mean: f64,
    latin_spread: f64,
    digit_uniform: f64,
    line_ratio: f64,
    weight_class: u16,
}

fn measure(data: &[u8], index: usize) -> Option<Measure> {
    let font = Font::open(data, index).ok()?;
    let upem = font.units_per_em().ok()?;
    let map = font.cmap().ok()?;
    let advances = font.advances().ok()?;
    let (ascent, descent, _) = font.line_metrics().ok()?;
    let outlines = font.table(b"glyf").is_some() && font.table(b"loca").is_some();

    let glyphs = map.len();
    if glyphs == 0 {
        return None;
    }
    let has = |codepoint: u32| map.contains_key(&codepoint);
    let width = |codepoint: u32| {
        map.get(&codepoint)
            .and_then(|gid| advances.get(*gid as usize))
            .map(|advance| scale(*advance as i32, upem))
    };

    let mut covered = 0usize;
    let mut total = 0usize;
    for codepoint in cjk_sample() {
        total += 1;
        if has(codepoint) {
            covered += 1;
        }
    }
    for mark in FULL_WIDTH_MARKS {
        total += 1;
        if has(*mark as u32) {
            covered += 1;
        }
    }
    for mark in COMMON_HAN.chars() {
        total += 1;
        if has(mark as u32) {
            covered += 1;
        }
    }
    let coverage = covered as f64 / total as f64;

    // Han must be one em wide or nothing downstream (justify, the baseline
    // grid, hanging marks) lines up.
    let mut uniform = 0usize;
    let mut uniform_total = 0usize;
    for codepoint in cjk_sample().take(256) {
        if let Some(advance) = width(codepoint) {
            uniform_total += 1;
            if advance == UNIT {
                uniform += 1;
            }
        }
    }

    let mut punctuation_ok = 0usize;
    let mut punctuation_total = 0usize;
    let mut punct_em_sum = 0.0f64;
    let mut ink_lefts: Vec<f64> = Vec::new();
    let mut ink_rights: Vec<f64> = Vec::new();
    for mark in FULL_WIDTH_MARKS {
        let codepoint = *mark as u32;
        let Some(advance) = width(codepoint) else {
            continue;
        };
        punctuation_total += 1;
        punct_em_sum += advance as f64 / UNIT as f64;
        if (advance - UNIT).abs() <= UNIT / 20 {
            punctuation_ok += 1;
        }
        let Some(gid) = map.get(&codepoint) else {
            continue;
        };
        let Some((x_min, x_max)) = font.ink_bounds(*gid).ok().flatten() else {
            continue;
        };
        ink_lefts.push(scale(x_min, upem) as f64 / UNIT as f64);
        ink_rights.push((advance - scale(x_max, upem)) as f64 / UNIT as f64);
    }

    let ink_total = ink_rights.len();
    // A mark whose ink fills 0.40-0.85 em of its advance leaves a bearing that
    // can carry a line end without a visible hole.
    let ink_ok = ink_rights
        .iter()
        .filter(|right| (0.40..=0.85).contains(*right))
        .count();
    let ink_mean = if ink_total == 0 {
        0.0
    } else {
        ink_rights.iter().sum::<f64>() / ink_total as f64
    };
    let ink_std = if ink_total == 0 {
        0.0
    } else {
        (ink_rights
            .iter()
            .map(|right| (right - ink_mean).powi(2))
            .sum::<f64>()
            / ink_total as f64)
            .sqrt()
    };
    let ink_left_mean = if ink_total == 0 {
        0.0
    } else {
        ink_lefts.iter().sum::<f64>() / ink_total as f64
    };

    // A proportional Latin face has a wide `W` and a narrow `i`; a monospaced
    // one has neither. Digits stand in for the same question.
    let latin_spread = match (width('W' as u32), width('i' as u32)) {
        (Some(wide), Some(narrow)) => ((wide - narrow) as f64 / UNIT as f64).clamp(0.0, 1.0),
        _ => 0.0,
    };
    let mut digit_widths: Vec<i32> = Vec::new();
    for digit in '0'..='9' {
        if let Some(advance) = width(digit as u32) {
            digit_widths.push(advance);
        }
    }
    let digit_uniform = if digit_widths.is_empty() {
        0.0
    } else {
        let first = digit_widths[0];
        ratio(
            digit_widths.iter().filter(|width| **width == first).count(),
            digit_widths.len(),
        )
    };

    let line_ratio = scale(ascent + descent, upem) as f64 / UNIT as f64;
    let weight_class = font
        .table(b"OS/2")
        .and_then(|(offset, length)| {
            if length >= 6 {
                read_u16(font.data, offset + 4).ok()
            } else {
                None
            }
        })
        .unwrap_or(400);

    Some(Measure {
        glyphs,
        outlines,
        coverage,
        uniform: ratio(uniform, uniform_total),
        punct_em: ratio_f64(punct_em_sum, punctuation_total),
        punct_ok: ratio(punctuation_ok, punctuation_total),
        ink_range: ratio(ink_ok, ink_total),
        ink_mean,
        ink_std,
        ink_left_mean,
        latin_spread,
        digit_uniform,
        line_ratio,
        weight_class,
    })
}

fn row(measure: &Measure, path: String, face: usize) -> AuditRow {
    // Marks in one face should share a style: a large spread means a line end
    // is tight on one line and loose on the next. A weight change moves every
    // mark together, so the spread — not the mean — is the quality signal.
    let ink_consistency = 1.0 - ((measure.ink_std - 0.10) / 0.25).clamp(0.0, 1.0);
    let line_score = (1.0 - ((measure.line_ratio - 1.15).abs() / 0.35)).clamp(0.0, 1.0);
    let score = 30.0 * measure.coverage
        + 15.0 * measure.uniform
        + 30.0 * ink_consistency
        + 15.0 * measure.punct_ok
        + 10.0 * line_score;

    let verdict = if !measure.outlines {
        "no-outlines"
    } else if measure.coverage < 0.30 {
        "no-cjk"
    } else if measure.coverage < 0.95 {
        "low-coverage"
    } else if measure.uniform < 0.95 {
        "proportional-han"
    } else if measure.punct_ok < 0.90 {
        "bad-punctuation"
    } else {
        "usable"
    };

    AuditRow {
        path,
        face,
        outlines: measure.outlines,
        glyphs: measure.glyphs,
        coverage: measure.coverage,
        uniform: measure.uniform,
        punctuation: measure.punct_ok,
        ink_range: measure.ink_range,
        ink_mean: measure.ink_mean,
        ink_std: measure.ink_std,
        line_ratio: measure.line_ratio,
        score,
        verdict,
    }
}

fn ratio(hit: usize, total: usize) -> f64 {
    if total == 0 {
        0.0
    } else {
        hit as f64 / total as f64
    }
}

/// Nine axes, every one 0..1. Chosen so an unrecognised face still lands
/// somewhere sensible: coverage and uniformity say what script it serves,
/// the ink axes say how its marks are drawn, and the Latin axes say whether
/// it was designed for text or for code.
fn fingerprint(measure: &Measure) -> ([f64; 9], bool) {
    let values = [
        measure.coverage,
        measure.uniform,
        (measure.punct_em / 1.2).clamp(0.0, 1.0),
        measure.ink_mean.clamp(0.0, 1.0),
        (measure.ink_std / 0.5).clamp(0.0, 1.0),
        ((measure.ink_left_mean - measure.ink_mean) + 1.0) / 2.0,
        measure.latin_spread,
        measure.digit_uniform,
        ((measure.line_ratio - 0.8) / 0.6).clamp(0.0, 1.0),
    ];
    // Without outlines the three ink axes carry no information. Comparing
    // them would make every CFF face look like a face with no ink.
    (values, measure.ink_mean > 0.0)
}

/// Coverage and uniformity decide whether a face can set a paragraph at all,
/// so they weigh more than the style axes.
const FINGERPRINT_WEIGHTS: [f64; 9] = [3.0, 2.0, 1.0, 1.0, 2.0, 1.0, 1.0, 1.0, 0.5];

fn fingerprint_distance(left: &[f64; 9], left_ink: bool, right: &[f64; 9], right_ink: bool) -> f64 {
    let compare_ink = left_ink && right_ink;
    let mut total = 0.0;
    let mut weight_sum = 0.0;
    for (index, weight) in FINGERPRINT_WEIGHTS.iter().enumerate() {
        if (3..=5).contains(&index) && !compare_ink {
            continue;
        }
        total += weight * (left[index] - right[index]).powi(2);
        weight_sum += weight;
    }
    if weight_sum == 0.0 {
        return 0.0;
    }
    (total / weight_sum).sqrt()
}

/// A human-readable classification derived only from measurements. Style
/// (Song vs Hei vs Kai) is a design decision and is deliberately not guessed.
fn profile(measure: &Measure) -> String {
    let script = if measure.coverage >= 0.90 {
        "cjk"
    } else if measure.coverage >= 0.20 {
        "cjk-partial"
    } else {
        "latin"
    };
    let width = if measure.uniform >= 0.95 && measure.digit_uniform >= 0.95 {
        "monospaced"
    } else {
        "proportional"
    };
    let punct = if measure.punct_em >= 0.95 {
        "full-width"
    } else if measure.punct_em >= 0.70 {
        "mixed"
    } else {
        "half-width"
    };
    format!("{script}/{width}/{punct}/w{}", measure.weight_class)
}

fn write_fingerprints(rows: &[(String, usize, Measure)], out: &str) -> Result<(), String> {
    let mut text = String::from("path,face,glyphs,profile,ink,f0,f1,f2,f3,f4,f5,f6,f7,f8\n");
    for (path, face, measure) in rows {
        let (values, has_ink) = fingerprint(measure);
        let _ = writeln!(
            text,
            "{},{},{},{},{},{:.4},{:.4},{:.4},{:.4},{:.4},{:.4},{:.4},{:.4},{:.4}",
            path,
            face,
            measure.glyphs,
            profile(measure),
            has_ink as u8,
            values[0],
            values[1],
            values[2],
            values[3],
            values[4],
            values[5],
            values[6],
            values[7],
            values[8]
        );
    }
    fs::write(out, text).map_err(|error| format!("{out}: {error}"))?;
    eprintln!("wrote {out}");
    Ok(())
}

fn identify(spec: &FaceSpec, library: &str) -> Result<(), String> {
    let data = fs::read(&spec.path).map_err(|error| format!("{}: {error}", spec.path))?;
    let measure = measure(&data, spec.index)
        .ok_or_else(|| format!("{}#{}: cannot measure", spec.path, spec.index))?;
    let (probe, probe_ink) = fingerprint(&measure);
    println!("profile: {}", profile(&measure));

    let text = fs::read_to_string(library).map_err(|error| format!("{library}: {error}"))?;
    let mut hits: Vec<(f64, String, usize, String)> = Vec::new();
    for line in text.lines().skip(1) {
        let columns: Vec<&str> = line.split(',').collect();
        if columns.len() < 14 {
            continue;
        }
        let mut reference = [0.0f64; 9];
        for (index, slot) in reference.iter_mut().enumerate() {
            *slot = columns
                .get(5 + index)
                .and_then(|v| v.parse().ok())
                .unwrap_or(0.0);
        }
        let reference_ink = columns[4] == "1";
        hits.push((
            fingerprint_distance(&probe, probe_ink, &reference, reference_ink),
            columns[0].to_string(),
            columns[1].parse().unwrap_or(0),
            columns[3].to_string(),
        ));
    }
    if hits.is_empty() {
        return Err(format!("{library}: no fingerprint rows"));
    }
    hits.sort_by(|left, right| left.0.total_cmp(&right.0));
    println!("distance,path,face,profile");
    // One row per family: eight weights of one design are one answer, not
    // eight. The closest weight represents it.
    let mut seen = std::collections::BTreeSet::new();
    let mut printed = 0;
    for (distance, path, face, profile) in &hits {
        if !seen.insert(family_key(path)) {
            continue;
        }
        println!("{distance:.4},{path},{face},{profile}");
        printed += 1;
        if printed == 5 {
            break;
        }
    }
    Ok(())
}

/// The family part of a file stem: `MiSans-Normal` and `MiSans-Bold` are one
/// family, `IBMPlexSansSC_Regular` keeps its prefix.
fn family_key(path: &str) -> String {
    const WEIGHTS: &[&str] = &[
        "Thin",
        "ExtraLight",
        "Light",
        "Regular",
        "Normal",
        "Book",
        "Text",
        "Medium",
        "DemiLight",
        "DemiBold",
        "SemiBold",
        "Bold",
        "ExtraBold",
        "Black",
        "Heavy",
        "Italic",
    ];
    let stem = path
        .rsplit('/')
        .next()
        .unwrap_or(path)
        .split('.')
        .next()
        .unwrap_or(path);
    let (head, tail) = match stem.rsplit_once(['-', '_']) {
        Some((head, tail)) => (head, tail),
        None => (stem, ""),
    };
    if WEIGHTS.contains(&tail) {
        head.to_string()
    } else {
        stem.to_string()
    }
}

fn ratio_f64(sum: f64, total: usize) -> f64 {
    if total == 0 {
        0.0
    } else {
        sum / total as f64
    }
}

fn run() -> Result<(), String> {
    let mut out = String::from("crates/soma-metrics/data/noto-cjk.smt");
    let mut specs = Vec::new();
    let mut audit_dirs: Vec<String> = Vec::new();
    let mut top = 40usize;
    let mut fingerprints_out: Option<String> = None;
    let mut identify_spec: Option<FaceSpec> = None;
    let mut library: Option<String> = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--out" => out = args.next().ok_or("--out needs a value")?,
            "--face" => specs.push(FaceSpec::parse(
                &args.next().ok_or("--face needs a value")?,
            )?),
            "--audit" => audit_dirs.push(args.next().ok_or("--audit needs a directory")?),
            "--top" => {
                top = args
                    .next()
                    .ok_or("--top needs a value")?
                    .parse()
                    .map_err(|_| "--top needs a number".to_string())?
            }
            "--fingerprints" => {
                fingerprints_out = Some(args.next().ok_or("--fingerprints needs a file")?)
            }
            "--identify" => {
                identify_spec = Some(FaceSpec::parse(
                    &args.next().ok_or("--identify needs a font")?,
                )?)
            }
            "--library" => library = Some(args.next().ok_or("--library needs a file")?),
            other => return Err(format!("unknown argument {other}")),
        }
    }
    if let Some(spec) = identify_spec {
        let library = library.ok_or("--identify needs --library <fingerprints.csv>")?;
        return identify(&spec, &library);
    }
    if !audit_dirs.is_empty() {
        return audit(&audit_dirs, top, fingerprints_out.as_deref());
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

        // Ink is recorded for punctuation only, and only when the font has
        // `glyf` outlines. The gaps say where the mark's ink ends inside its
        // advance box, which is what a justified line pays for (ADR-0037).
        let mut ink: Vec<(u32, i32, i32)> = Vec::new();
        for &(start, end) in INK_RANGES {
            for cp in start..=end {
                let Some(&gid) = map.get(&cp) else { continue };
                let Some(&advance) = advances.get(gid as usize) else {
                    continue;
                };
                let Some((x_min, x_max)) = font.ink_bounds(gid)? else {
                    continue;
                };
                let left = scale(x_min, upem).clamp(i16::MIN as i32, i16::MAX as i32);
                let right =
                    scale(advance as i32 - x_max, upem).clamp(i16::MIN as i32, i16::MAX as i32);
                ink.push((cp, left, right));
            }
        }
        ink.sort_unstable_by_key(|(cp, _, _)| *cp);
        ink.dedup_by_key(|(cp, _, _)| *cp);

        let default_advance = blocks
            .iter()
            .find(|(start, _, _)| *start == 0x4e00)
            .map(|(_, _, advance)| *advance)
            .unwrap_or(UNIT);

        println!(
            "face {}/{}: upem={} ascii-entries={} blocks={} entries={} ink={}",
            spec.family,
            spec.weight,
            upem,
            entries.len(),
            blocks.len(),
            entries.len(),
            ink.len()
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
            ink,
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
        bytes.extend_from_slice(&(face.ink.len() as u32).to_le_bytes());
        for (cp, left, right) in &face.ink {
            bytes.extend_from_slice(&cp.to_le_bytes());
            bytes.extend_from_slice(&(*left as i16).to_le_bytes());
            bytes.extend_from_slice(&(*right as i16).to_le_bytes());
        }
    }

    if let Some(parent) = std::path::Path::new(&out).parent() {
        fs::create_dir_all(parent).map_err(|error| format!("{}: {error}", parent.display()))?;
    }
    fs::write(&out, &bytes).map_err(|error| format!("{out}: {error}"))?;
    println!("wrote {out} ({} bytes, {} faces)", bytes.len(), faces.len());
    Ok(())
}
