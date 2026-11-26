//! The C ABI must produce exactly what the native engine produces for the same
//! paragraph and the same width table. This is the gate that keeps the browser
//! slice honest: it is the place where "wasm says X, the PDF says Y" can be
//! caught before a user sees it.

use soma_core::{
    break_paragraph, Align, Atom, BreakConfig, Decorations, GlyphMetrics, LineMetrics, Metrics,
    ParagraphStyle, StyleId, Width,
};
use soma_rules::{paragraph_from_spans, InlineSpan, SpanKind};
use soma_web::*;

const ASCENT: i32 = 3_604;
const DESCENT: i32 = 901;
const LEADING: i32 = 819;
const LINE_HEIGHT: i32 = 6_144;
const SPACE: i32 = 1_024;
const HYPHEN: i32 = 682;

/// Index-keyed metrics over a fixed width table: the same table the host hands
/// over the boundary.
struct FixedWidths<'a> {
    widths: &'a [i32],
}

impl Metrics for FixedWidths<'_> {
    fn advance(&self, _text: &str, _style: StyleId) -> Width {
        Width::ZERO
    }
    fn line_metrics(&self, _style: StyleId) -> LineMetrics {
        LineMetrics {
            ascent: Width(ASCENT),
            descent: Width(DESCENT),
            leading: Width(LEADING),
        }
    }
    fn atom_advance(&self, index: u32, _atom: &Atom, _style: StyleId) -> Width {
        Width(self.widths[index as usize])
    }
    fn atom_glyph_metrics(&self, index: u32, atom: &Atom, style: StyleId) -> GlyphMetrics {
        GlyphMetrics {
            advance: self.atom_advance(index, atom, style),
            ascent: Width(ASCENT),
            descent: Width(DESCENT),
            leading: Width(LEADING),
            baseline_shift: Width::ZERO,
            overhang_left: Width::ZERO,
            overhang_right: Width::ZERO,
        }
    }
    fn space_width(&self, _style: StyleId) -> Width {
        Width(SPACE)
    }
    fn hyphen_width(&self, _style: StyleId) -> Width {
        Width(HYPHEN)
    }
}

/// A deterministic width table: one em per CJK character, half an em per ASCII.
fn width_of(text: &str) -> i32 {
    text.chars()
        .map(|character| if character.is_ascii() { 512 } else { 4096 })
        .sum()
}

fn widths_for(atoms: &[Atom]) -> Vec<i32> {
    atoms.iter().map(|atom| width_of(&atom.text)).collect()
}

fn native(text: &str, width: i32) -> Vec<i32> {
    let style = ParagraphStyle {
        width: Width(width),
        first_indent: Width::ZERO,
        hang: Width::ZERO,
        tracking: Width::ZERO,
        line_height: Width(LINE_HEIGHT),
        align: Align::Justify,
    };
    let spans = [InlineSpan {
        text,
        style: StyleId(0),
        kind: SpanKind::Text,
        decor: Decorations::NONE,
        link: None,
        width: Width::ZERO,
        penalty: 0,
    }];
    let paragraph = paragraph_from_spans(&spans, style).expect("paragraph");
    let widths = widths_for(&paragraph.atoms);
    let metrics = FixedWidths { widths: &widths };
    let result = break_paragraph(&paragraph, &metrics, &BreakConfig::default()).expect("break");
    result.lines.iter().map(|line| line.end as i32).collect()
}

/// What `soma_para_new` + `soma_para_dump` give the host.
struct Dumped {
    handle: u32,
    atoms: Vec<String>,
}

fn dump_single_span(text: &str, width: i32) -> Dumped {
    let blob = text.as_bytes();
    let table = [0u32, blob.len() as u32, 0, 0, 0];
    let style = [width, 0, 0, 0, LINE_HEIGHT, 0, 0];
    let handle = unsafe {
        soma_para_new(
            style.as_ptr(),
            table.as_ptr(),
            1,
            blob.as_ptr(),
            blob.len() as u32,
        )
    };
    assert_ne!(handle, 0, "soma_para_new failed: {}", soma_last_error());

    let words = soma_para_words(handle);
    assert!(words > 0);
    let mut dump = vec![0u32; words as usize];
    let written = unsafe { soma_para_dump(handle, dump.as_mut_ptr(), words) };
    assert_eq!(written, words, "soma_para_dump");

    let atom_count = dump[0] as usize;
    let blob_words = dump[2] as usize;
    let blob_bytes = dump[1] as usize;
    let decoded = {
        // SAFETY: the dump buffer is word aligned and `dump[1]` is its byte length.
        let bytes = unsafe {
            std::slice::from_raw_parts(dump[blob_words..].as_ptr() as *const u8, blob_bytes)
        };
        std::str::from_utf8(bytes).expect("utf-8 blob").to_string()
    };
    let mut atoms = Vec::with_capacity(atom_count);
    let mut cursor = 3usize;
    let mut chars = 0usize;
    for _ in 0..atom_count {
        let utf16 = dump[cursor + 3] as usize;
        let atom: String = decoded.chars().skip(chars).take(utf16).collect();
        chars += utf16;
        atoms.push(atom);
        cursor += 6;
    }
    Dumped { handle, atoms }
}

fn abi_ends(dumped: &Dumped, greedy: bool) -> Vec<i32> {
    let widths: Vec<i32> = dumped.atoms.iter().map(|atom| width_of(atom)).collect();
    let atom_count = dumped.atoms.len();
    let mut input = vec![0i32; 8 + atom_count];
    input[0] = SPACE;
    input[1] = HYPHEN;
    input[2] = ASCENT;
    input[3] = DESCENT;
    input[4] = LEADING;
    input[8..].copy_from_slice(&widths);

    let run = |out: *mut i32, cap: u32| unsafe {
        if greedy {
            soma_break_greedy(dumped.handle, input.as_ptr(), input.len() as u32, out, cap)
        } else {
            soma_break(dumped.handle, input.as_ptr(), input.len() as u32, out, cap)
        }
    };
    let needed = run(std::ptr::null_mut(), 0);
    assert!(needed > 0, "soma_break failed: {}", soma_last_error());
    let mut out = vec![0i32; needed as usize];
    assert_eq!(run(out.as_mut_ptr(), needed), needed);

    let line_count = out[0] as usize;
    (0..line_count).map(|line| out[4 + line * 9 + 1]).collect()
}

const TEXT: &str =
    "中文English混排，数字 123，排版Engine v2。「引号」，（括号）；冒号：逗号，句号。";

#[test]
fn abi_matches_native_breaks() {
    for width in [3_200, 8_000, 16_384, 32_000] {
        let dumped = dump_single_span(TEXT, width);
        let native_ends = native(TEXT, width);
        let abi = abi_ends(&dumped, false);
        assert_eq!(abi, native_ends, "width {width}");
        soma_para_free(dumped.handle);
    }
}

#[test]
fn abi_atom_table_matches_the_rules_layer() {
    let dumped = dump_single_span(TEXT, 32_000);
    let style = ParagraphStyle {
        width: Width(32_000),
        first_indent: Width::ZERO,
        hang: Width::ZERO,
        tracking: Width::ZERO,
        line_height: Width(LINE_HEIGHT),
        align: Align::Justify,
    };
    let spans = [InlineSpan {
        text: TEXT,
        style: StyleId(0),
        kind: SpanKind::Text,
        decor: Decorations::NONE,
        link: None,
        width: Width::ZERO,
        penalty: 0,
    }];
    let paragraph = paragraph_from_spans(&spans, style).expect("paragraph");
    let expected: Vec<String> = paragraph.atoms.iter().map(|a| a.text.clone()).collect();
    assert_eq!(dumped.atoms, expected);
    soma_para_free(dumped.handle);
}

#[test]
fn atom_table_offsets_cover_the_blob_once() {
    let blob = TEXT.as_bytes();
    let table = [0u32, blob.len() as u32, 0, 0, 0];
    let style = [32_000, 0, 0, 0, LINE_HEIGHT, 0, 0];
    let handle = unsafe {
        soma_para_new(
            style.as_ptr(),
            table.as_ptr(),
            1,
            blob.as_ptr(),
            blob.len() as u32,
        )
    };
    assert_ne!(handle, 0);
    let words = soma_para_words(handle);
    let mut dump = vec![0u32; words as usize];
    assert_eq!(
        unsafe { soma_para_dump(handle, dump.as_mut_ptr(), words) },
        words
    );

    let atom_count = dump[0] as usize;
    let mut expected_start = 0u32;
    for index in 0..atom_count {
        let base = 3 + index * 6;
        assert_eq!(dump[base + 1], expected_start, "atom {index} start");
        assert!(dump[base + 2] > 0, "atom {index} is empty");
        expected_start += dump[base + 2];
    }
    assert_eq!(expected_start, dump[1], "atoms must cover the blob");
    soma_para_free(handle);
}

#[test]
fn hard_break_splits_into_segments() {
    let parts = ["第一段文字。", "第二段文字。"];
    let mut blob = Vec::new();
    let mut table = [0u32; 3 * 5];
    // span 0: text, span 1: hard break (empty), span 2: text.
    for (index, part) in parts.iter().enumerate() {
        let slot = if index == 0 { 0 } else { 2 };
        table[slot * 5] = blob.len() as u32;
        table[slot * 5 + 1] = part.len() as u32;
        blob.extend_from_slice(part.as_bytes());
    }
    table[5] = 0;
    table[6] = 0;
    table[7] = 3 << 8;

    let style = [32_000, 0, 0, 0, LINE_HEIGHT, 0, 0];
    let handle = unsafe {
        soma_para_new(
            style.as_ptr(),
            table.as_ptr(),
            3,
            blob.as_ptr(),
            blob.len() as u32,
        )
    };
    assert_ne!(handle, 0, "{}", soma_last_error());

    let words = soma_para_words(handle);
    let mut dump = vec![0u32; words as usize];
    unsafe { soma_para_dump(handle, dump.as_mut_ptr(), words) };
    let atom_count = dump[0] as usize;
    let mut input = vec![0i32; 8 + atom_count];
    input[0] = SPACE;
    input[1] = HYPHEN;
    input[2] = ASCENT;
    input[3] = DESCENT;
    input[4] = LEADING;
    for atom in 0..atom_count {
        input[8 + atom] = 4_096;
    }
    let needed = unsafe {
        soma_break(
            handle,
            input.as_ptr(),
            input.len() as u32,
            std::ptr::null_mut(),
            0,
        )
    };
    let mut out = vec![0i32; needed as usize];
    unsafe {
        soma_break(
            handle,
            input.as_ptr(),
            input.len() as u32,
            out.as_mut_ptr(),
            needed,
        )
    };
    // The forced break must produce at least two lines even though all fits.
    assert!(out[0] >= 2);
    soma_para_free(handle);
}

#[test]
fn error_codes_are_reported() {
    let style = [0, 0, 0, 0, LINE_HEIGHT, 0, 0];
    let table = [0u32; 5];
    let blob = b"x";
    let handle = unsafe { soma_para_new(style.as_ptr(), table.as_ptr(), 1, blob.as_ptr(), 1) };
    assert_eq!(handle, 0);
    assert_eq!(soma_last_error(), ERR_PARAMS);

    assert_eq!(soma_para_words(9_999), 0);
    let nothing = unsafe { soma_break(9_999, std::ptr::null(), 0, std::ptr::null_mut(), 0) };
    assert_eq!(nothing, 0);
    assert_eq!(soma_last_error(), ERR_HANDLE);
}

#[test]
fn free_is_idempotent() {
    let blob = b"abc";
    let table = [0u32, 3, 0, 0, 0];
    let style = [32_000, 0, 0, 0, LINE_HEIGHT, 0, 0];
    let handle = unsafe { soma_para_new(style.as_ptr(), table.as_ptr(), 1, blob.as_ptr(), 3) };
    assert_ne!(handle, 0);
    soma_para_free(handle);
    soma_para_free(handle);
    assert_eq!(soma_para_words(handle), 0);
}

#[test]
fn greedy_uses_the_same_atom_table() {
    let dumped = dump_single_span(TEXT, 6_000);
    let greedy = abi_ends(&dumped, true);
    assert!(!greedy.is_empty());
    soma_para_free(dumped.handle);
}

/// The committed fixture is the single source of truth for three consumers:
/// this test, `web/harness.html`, and the native example that regenerates it.
/// If the rules layer or the DP changes, the fixture must be regenerated (and
/// the change explained), otherwise the browser and the PDF can drift apart.
const PARITY: &str = include_str!("../../../web/parity.txt");

fn fixture() -> (String, i32, Vec<i32>, Vec<i32>) {
    let mut text = String::new();
    let mut width = 0;
    let mut widths = Vec::new();
    let mut ends = Vec::new();
    for line in PARITY.lines() {
        let Some((key, value)) = line.split_once('|') else {
            continue;
        };
        match key {
            "text" => text = value.to_string(),
            "width" => width = value.parse().expect("width"),
            "widths" => {
                widths = value
                    .split(',')
                    .map(|word| word.parse().expect("width"))
                    .collect()
            }
            "ends" => {
                ends = value
                    .split(',')
                    .map(|word| word.parse().expect("end"))
                    .collect()
            }
            _ => {}
        }
    }
    assert!(!text.is_empty() && width > 0 && !widths.is_empty() && !ends.is_empty());
    (text, width, widths, ends)
}

#[test]
fn the_abi_reproduces_the_committed_fixture() {
    let (text, width, widths, ends) = fixture();
    let dumped = dump_single_span(&text, width);
    assert_eq!(
        dumped.atoms.len(),
        widths.len(),
        "the rules layer produced a different atom count than the fixture"
    );
    // Use the fixture's widths verbatim, not the deterministic rule, so the test
    // pins the ABI rather than the width convention.
    let atom_count = dumped.atoms.len();
    let mut input = vec![0i32; 8 + atom_count];
    input[0] = SPACE;
    input[1] = HYPHEN;
    input[2] = ASCENT;
    input[3] = DESCENT;
    input[4] = LEADING;
    input[8..].copy_from_slice(&widths);
    let needed = unsafe {
        soma_break(
            dumped.handle,
            input.as_ptr(),
            input.len() as u32,
            std::ptr::null_mut(),
            0,
        )
    };
    assert!(needed > 0, "soma_break failed: {}", soma_last_error());
    let mut out = vec![0i32; needed as usize];
    assert_eq!(
        unsafe {
            soma_break(
                dumped.handle,
                input.as_ptr(),
                input.len() as u32,
                out.as_mut_ptr(),
                needed,
            )
        },
        needed
    );
    let line_count = out[0] as usize;
    let abi: Vec<i32> = (0..line_count).map(|line| out[4 + line * 9 + 1]).collect();
    assert_eq!(abi, ends);
    soma_para_free(dumped.handle);
}
