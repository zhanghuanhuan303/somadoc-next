//! `soma-web`: the browser slice of the typesetting engine.
//!
//! The module exposes a small C ABI over `soma-core` (Knuth–Plass) and
//! `soma-rules` (Chinese punctuation, forbidden breaks, edge trim). It is built
//! for `wasm32-unknown-unknown` and has **no imports**: no WASI, no JS glue, no
//! `wasm-bindgen`. A host instantiates it with `{}`.
//!
//! Division of labour:
//!
//! * the host measures text (canvas `measureText`) and hands over one integer
//!   advance per atom, in 1/4096 em;
//! * the module compiles the text into atoms/boundaries, runs the DP, and
//!   returns line and box coordinates.
//!
//! Everything crossing the boundary is an integer in linear memory. See
//! `web/README.md` for the buffer layouts and `docs/adr/0071` for the decision
//! record.

use std::sync::Mutex;

use soma_core::{
    break_paragraph, break_paragraph_greedy, Align, Atom, BreakConfig, Decorations, GlyphMetrics,
    GreedyMode, LayoutResult, LineMetrics, Metrics, Paragraph, ParagraphStyle, StyleId, Width,
};
use soma_rules::{paragraph_from_spans, InlineSpan, SpanKind};

/// Width unit: 1/4096 em, the engine-wide fixed point.
pub const UNITS_PER_EM: i32 = 4096;

/// Words at the head of the `soma_break` input, before the per-atom widths.
const IN_HEADER_WORDS: u32 = 8;
/// Words per atom in the `soma_para_dump` table.
const ATOM_WORDS: u32 = 6;
/// Words per line in the `soma_break` output.
pub const LINE_WORDS: u32 = 9;
/// Words per box in the `soma_break` output.
pub const BOX_WORDS: u32 = 3;
/// Words at the head of the `soma_break` output.
pub const OUT_HEADER_WORDS: u32 = 4;
/// Words per span in the `soma_para_new` input.
const SPAN_WORDS: u32 = 5;

/// Result flag: the fallback ladder was used.
pub const RESULT_FALLBACK: u32 = 1;
/// Result flag: `max_kp_atoms` downgraded the run to greedy.
pub const RESULT_LENGTH_LIMITED: u32 = 2;

/// Per-line flags.
pub const LINE_OVERFULL: u32 = 1;
pub const LINE_EMERGENCY: u32 = 2;
pub const LINE_FORCED_END: u32 = 4;
pub const LINE_HYPHENATED: u32 = 8;

/// Error codes reported by [`soma_last_error`].
pub const ERR_NONE: i32 = 0;
pub const ERR_HANDLE: i32 = 1;
pub const ERR_UTF8: i32 = 2;
pub const ERR_RULES: i32 = 3;
pub const ERR_BREAK: i32 = 4;
pub const ERR_BUFFER: i32 = 5;
pub const ERR_PARAMS: i32 = 6;

static HANDLES: Mutex<Vec<Option<Paragraph>>> = Mutex::new(Vec::new());

thread_local! {
    /// Last error on this thread. The browser build is single-threaded; the
    /// thread-local here keeps native tests from clobbering each other.
    static LAST_ERROR: std::cell::Cell<i32> = const { std::cell::Cell::new(ERR_NONE) };
}

fn set_error(code: i32) {
    LAST_ERROR.with(|slot| slot.set(code));
}

/// The last error code, for the host to read after a zero return.
#[no_mangle]
pub extern "C" fn soma_last_error() -> i32 {
    LAST_ERROR.with(std::cell::Cell::get)
}

/// Reserve `len` bytes inside the module's memory.
///
/// The host writes its span table, text blob, and buffers here. The pointer
/// stays valid until [`soma_dealloc`] is called with the same length.
#[no_mangle]
pub extern "C" fn soma_alloc(len: u32) -> *mut u8 {
    if len == 0 {
        return std::ptr::null_mut();
    }
    match std::alloc::Layout::from_size_align(len as usize, 8) {
        // SAFETY: the layout is non-zero, and `soma_dealloc` passes the same
        // length back to `dealloc`.
        Ok(layout) => unsafe { std::alloc::alloc(layout) },
        Err(_) => std::ptr::null_mut(),
    }
}

/// Release a buffer from [`soma_alloc`].
///
/// # Safety
///
/// `ptr` must come from `soma_alloc(len)` and must not be used afterwards.
#[no_mangle]
pub unsafe extern "C" fn soma_dealloc(ptr: *mut u8, len: u32) {
    if ptr.is_null() || len == 0 {
        return;
    }
    if let Ok(layout) = std::alloc::Layout::from_size_align(len as usize, 8) {
        // SAFETY: the caller passes back the pair it received.
        unsafe { std::alloc::dealloc(ptr, layout) };
    }
}

/// Metrics that answer by atom index, straight from the host's width array.
struct WireMetrics<'a> {
    widths: &'a [i32],
    space: Width,
    hyphen: Width,
    line: LineMetrics,
}

impl Metrics for WireMetrics<'_> {
    fn advance(&self, _text: &str, _style: StyleId) -> Width {
        // The breaker asks by index through `atom_advance`; a text-keyed call
        // carries no index to answer with. Hosts that need text keys load the
        // `soma.metrics` table instead.
        Width::ZERO
    }

    fn line_metrics(&self, _style: StyleId) -> LineMetrics {
        self.line
    }

    fn atom_advance(&self, index: u32, _atom: &Atom, _style: StyleId) -> Width {
        Width(self.widths.get(index as usize).copied().unwrap_or(0))
    }

    fn atom_glyph_metrics(&self, index: u32, atom: &Atom, style: StyleId) -> GlyphMetrics {
        GlyphMetrics {
            advance: self.atom_advance(index, atom, style),
            ascent: self.line.ascent,
            descent: self.line.descent,
            leading: self.line.leading,
            baseline_shift: Width::ZERO,
            overhang_left: Width::ZERO,
            overhang_right: Width::ZERO,
        }
    }

    fn space_width(&self, _style: StyleId) -> Width {
        self.space
    }

    fn hyphen_width(&self, _style: StyleId) -> Width {
        self.hyphen
    }
}

/// Read `count` i32 from `ptr`; empty when the pointer is null.
unsafe fn read_i32(ptr: *const i32, count: usize) -> Vec<i32> {
    if ptr.is_null() || count == 0 {
        return Vec::new();
    }
    // SAFETY: the host promises `count` words at `ptr`.
    unsafe { std::slice::from_raw_parts(ptr, count) }.to_vec()
}

fn kind_from_word(word: u32) -> SpanKind {
    match (word >> 8) & 0xff {
        1 => SpanKind::Code,
        2 => SpanKind::Object,
        3 => SpanKind::HardBreak,
        _ => SpanKind::Text,
    }
}

/// Compile a styled inline sequence into a breakable paragraph.
///
/// * `style`: 7 words — `width, first_indent, hang, tracking, line_height,
///   style_id, flags` (bit 0 = left align).
/// * `spans`: `span_count * 5` words — `byte_offset, byte_len,
///   style_id | kind << 8, decorations, reserved`.
/// * `blob`: the concatenated UTF-8 the offsets point into.
///
/// Returns a handle, or `0` on error (see [`soma_last_error`]).
///
/// # Safety
///
/// Every pointer must be valid for the stated length.
#[no_mangle]
pub unsafe extern "C" fn soma_para_new(
    style: *const i32,
    spans: *const u32,
    span_count: u32,
    blob: *const u8,
    blob_len: u32,
) -> u32 {
    set_error(ERR_NONE);
    let style = unsafe { read_i32(style, 7) };
    if style.len() < 5 || style[0] <= 0 || style[4] <= 0 {
        set_error(ERR_PARAMS);
        return 0;
    }
    if span_count > 0 && spans.is_null() {
        set_error(ERR_PARAMS);
        return 0;
    }
    let raw_spans: Vec<u32> = if span_count == 0 {
        Vec::new()
    } else {
        // SAFETY: the host promises `span_count * SPAN_WORDS` words.
        unsafe {
            std::slice::from_raw_parts(spans, span_count as usize * SPAN_WORDS as usize).to_vec()
        }
    };
    let blob: &[u8] = if blob_len == 0 || blob.is_null() {
        &[]
    } else {
        // SAFETY: the host promises `blob_len` bytes.
        unsafe { std::slice::from_raw_parts(blob, blob_len as usize) }
    };

    let paragraph_style = ParagraphStyle {
        width: Width(style[0]),
        first_indent: Width(style[1].max(0)),
        hang: Width(style[2].max(0)),
        tracking: Width(style[3].max(0)),
        line_height: Width(style[4].max(1)),
        align: if style[6] & 1 == 1 {
            Align::Left
        } else {
            Align::Justify
        },
    };

    // Decode each span into its own `String` so the borrowed `InlineSpan`s live
    // long enough for the compiler to walk them.
    let mut texts: Vec<String> = Vec::with_capacity(span_count as usize);
    for chunk in raw_spans.chunks_exact(SPAN_WORDS as usize) {
        let offset = chunk[0] as usize;
        let len = chunk[1] as usize;
        let end = offset.saturating_add(len);
        if end > blob.len() {
            set_error(ERR_UTF8);
            return 0;
        }
        match std::str::from_utf8(&blob[offset..end]) {
            Ok(text) => texts.push(text.to_string()),
            Err(_) => {
                set_error(ERR_UTF8);
                return 0;
            }
        }
    }

    let mut inline: Vec<InlineSpan<'_>> = Vec::with_capacity(texts.len());
    for (chunk, text) in raw_spans.chunks_exact(SPAN_WORDS as usize).zip(&texts) {
        let meta = chunk[2];
        inline.push(InlineSpan {
            text,
            style: StyleId((meta & 0xff) as u16),
            kind: kind_from_word(meta),
            decor: Decorations::from_bits(chunk[3] as u8),
            link: None,
            width: Width(0),
            penalty: 0,
        });
    }

    let paragraph = match paragraph_from_spans(&inline, paragraph_style) {
        Ok(paragraph) => paragraph,
        Err(_) => {
            set_error(ERR_RULES);
            return 0;
        }
    };

    let mut handles = match HANDLES.lock() {
        Ok(handles) => handles,
        Err(_) => {
            set_error(ERR_HANDLE);
            return 0;
        }
    };
    if let Some(index) = handles.iter().position(Option::is_none) {
        handles[index] = Some(paragraph);
        return index as u32 + 1;
    }
    handles.push(Some(paragraph));
    handles.len() as u32
}

/// Release a paragraph handle.
#[no_mangle]
pub extern "C" fn soma_para_free(handle: u32) {
    if handle == 0 {
        return;
    }
    if let Ok(mut handles) = HANDLES.lock() {
        if let Some(slot) = handles.get_mut(handle as usize - 1) {
            *slot = None;
        }
    }
}

fn with_paragraph<R>(handle: u32, body: impl FnOnce(&Paragraph) -> R) -> Option<R> {
    let handles = HANDLES.lock().ok()?;
    let slot = handles.get(handle.checked_sub(1)? as usize)?;
    slot.as_ref().map(body)
}

/// Atom-table word count for `handle`, or 0 when the handle is invalid.
#[no_mangle]
pub extern "C" fn soma_para_words(handle: u32) -> u32 {
    with_paragraph(handle, |paragraph| {
        let blob: usize = paragraph.atoms.iter().map(|atom| atom.text.len()).sum();
        let table = paragraph.atoms.len() as u32 * ATOM_WORDS;
        3 + table + (blob as u32).div_ceil(4)
    })
    .unwrap_or(0)
}

/// Write the atom table, then the UTF-8 text blob.
///
/// Layout (all `u32`):
///
/// ```text
/// [0] atom_count
/// [1] blob_byte_len
/// [2] blob_word_offset      (from `out`)
/// [3 ..] atom_count * 6:
///     kind | class << 8 | style << 16
///     text_byte_start, text_byte_len, text_utf16_len
///     compress_left (i32), compress_right (i32)
/// [blob_word_offset ..] UTF-8 bytes, zero padded to a word
/// ```
///
/// Returns the number of words written, or the number needed when `cap` is too
/// small (with [`ERR_BUFFER`] set).
///
/// # Safety
///
/// `out` must be writable for `cap` words.
#[no_mangle]
pub unsafe extern "C" fn soma_para_dump(handle: u32, out: *mut u32, cap: u32) -> u32 {
    set_error(ERR_NONE);
    let needed = soma_para_words(handle);
    if needed == 0 {
        set_error(ERR_HANDLE);
        return 0;
    }
    if out.is_null() || cap < needed {
        set_error(ERR_BUFFER);
        return needed;
    }

    let mut table: Vec<u32> = Vec::with_capacity(needed as usize);
    let mut blob: Vec<u8> = Vec::new();
    let built = with_paragraph(handle, |paragraph| {
        table.push(paragraph.atoms.len() as u32);
        table.push(0); // blob length, filled below
        table.push(0); // blob offset, filled below
        for atom in &paragraph.atoms {
            let start = blob.len() as u32;
            blob.extend_from_slice(atom.text.as_bytes());
            let kind = match atom.kind {
                soma_core::AtomKind::Cjk => 0u32,
                soma_core::AtomKind::LatinRun => 1,
                soma_core::AtomKind::DigitRun => 2,
                soma_core::AtomKind::InlineObject => 3,
                soma_core::AtomKind::Ruby => 4,
                soma_core::AtomKind::Other => 5,
            };
            let class = match atom.class {
                soma_core::PunctClass::None => 0u32,
                soma_core::PunctClass::Open => 1,
                soma_core::PunctClass::Close => 2,
                soma_core::PunctClass::Middle => 3,
                soma_core::PunctClass::Dash => 4,
            };
            table.push(kind | class << 8 | (atom.style.0 as u32) << 16);
            table.push(start);
            table.push(atom.text.len() as u32);
            table.push(atom.text.chars().count() as u32);
            table.push(atom.compress_left.0 as u32);
            table.push(atom.compress_right.0 as u32);
        }
        table[1] = blob.len() as u32;
        table[2] = 3 + paragraph.atoms.len() as u32 * ATOM_WORDS;
    });
    if built.is_none() {
        set_error(ERR_HANDLE);
        return 0;
    }

    // SAFETY: `cap >= needed` was checked above.
    let out = unsafe { std::slice::from_raw_parts_mut(out, needed as usize) };
    for (slot, word) in out.iter_mut().zip(&table) {
        *slot = *word;
    }
    let blob_offset = table[2] as usize;
    let blob_words = &mut out[blob_offset..needed as usize];
    // SAFETY: viewing a `u32` slice as bytes is well defined, and the blob is
    // copied into a region the table sized for it.
    let bytes =
        unsafe { std::slice::from_raw_parts_mut(blob_words.as_mut_ptr() as *mut u8, blob.len()) };
    bytes.copy_from_slice(&blob);
    needed
}

fn write_lines(
    out: &mut [i32],
    result: &LayoutResult,
    fallback: bool,
    length_limited: bool,
) -> u32 {
    let line_count = result.lines.len() as u32;
    let box_count: u32 = result
        .lines
        .iter()
        .map(|line| line.boxes.len() as u32)
        .sum();
    let needed = OUT_HEADER_WORDS + line_count * LINE_WORDS + box_count * BOX_WORDS;
    if out.len() < needed as usize {
        return needed;
    }
    let mut flags = 0u32;
    if fallback {
        flags |= RESULT_FALLBACK;
    }
    if length_limited {
        flags |= RESULT_LENGTH_LIMITED;
    }
    out[0] = line_count as i32;
    out[1] = box_count as i32;
    out[2] = flags as i32;
    out[3] = 0;

    let mut cursor = OUT_HEADER_WORDS as usize;
    for line in &result.lines {
        let mut line_flags = 0u32;
        if line.overfull {
            line_flags |= LINE_OVERFULL;
        }
        if line.emergency {
            line_flags |= LINE_EMERGENCY;
        }
        if line.forced_end {
            line_flags |= LINE_FORCED_END;
        }
        if line.hyphenated {
            line_flags |= LINE_HYPHENATED;
        }
        out[cursor] = line.start as i32;
        out[cursor + 1] = line.end as i32;
        out[cursor + 2] = line.trim_left.0;
        out[cursor + 3] = line.trim_right.0;
        out[cursor + 4] = line.adjustment;
        out[cursor + 5] = line.ascent.0;
        out[cursor + 6] = line.descent.0;
        out[cursor + 7] = line.leading.0;
        out[cursor + 8] = line_flags as i32;
        cursor += LINE_WORDS as usize;
    }
    for line in &result.lines {
        for atom in &line.boxes {
            out[cursor] = atom.atom as i32;
            out[cursor + 1] = atom.x.0;
            out[cursor + 2] = atom.advance.0;
            cursor += BOX_WORDS as usize;
        }
    }
    needed
}

unsafe fn break_inner(
    handle: u32,
    input: *const i32,
    in_len: u32,
    out: *mut i32,
    out_cap: u32,
    greedy: bool,
) -> u32 {
    set_error(ERR_NONE);
    let Some(atom_count) = with_paragraph(handle, |paragraph| paragraph.atoms.len()) else {
        set_error(ERR_HANDLE);
        return 0;
    };
    let expected = IN_HEADER_WORDS as usize + atom_count;
    if input.is_null() || (in_len as usize) < expected {
        set_error(ERR_PARAMS);
        return 0;
    }
    // SAFETY: the host promises `in_len` words and `in_len >= expected`.
    let all = unsafe { std::slice::from_raw_parts(input, expected) };
    let metrics = WireMetrics {
        widths: &all[IN_HEADER_WORDS as usize..],
        space: Width(all[0]),
        hyphen: Width(all[1]),
        line: LineMetrics {
            ascent: Width(all[2]),
            descent: Width(all[3]),
            leading: Width(all[4]),
        },
    };
    let config = BreakConfig::default();

    let broken = with_paragraph(handle, |paragraph| {
        if greedy {
            break_paragraph_greedy(paragraph, &metrics, &config, GreedyMode::MinCost)
        } else {
            break_paragraph(paragraph, &metrics, &config)
        }
    });
    let result = match broken {
        None => {
            set_error(ERR_HANDLE);
            return 0;
        }
        Some(Err(_)) => {
            set_error(ERR_BREAK);
            return 0;
        }
        Some(Ok(result)) => result,
    };
    let fallback = result.fallback_stage != soma_core::FallbackStage::Normal;
    let length_limited = result.length_limited;

    let needed = OUT_HEADER_WORDS
        + result.lines.len() as u32 * LINE_WORDS
        + result
            .lines
            .iter()
            .map(|line| line.boxes.len() as u32)
            .sum::<u32>()
            * BOX_WORDS;
    if out.is_null() || out_cap < needed {
        set_error(ERR_BUFFER);
        return needed;
    }
    // SAFETY: `out_cap >= needed` was checked above.
    let out = unsafe { std::slice::from_raw_parts_mut(out, needed as usize) };
    write_lines(out, &result, fallback, length_limited);
    needed
}

/// Break a paragraph with Knuth–Plass.
///
/// Input words: `space_width, hyphen_width, ascent, descent, leading,
/// reserved x3`, then one advance per atom.
///
/// Returns the number of output words the result needs (writing them when
/// `out_cap` is large enough), or `0` on error.
///
/// # Safety
///
/// `input` must be valid for `in_len` words and `out` for `out_cap` words.
#[no_mangle]
pub unsafe extern "C" fn soma_break(
    handle: u32,
    input: *const i32,
    in_len: u32,
    out: *mut i32,
    out_cap: u32,
) -> u32 {
    unsafe { break_inner(handle, input, in_len, out, out_cap, false) }
}

/// Greedy-C break, for the host's degraded path (D94).
///
/// # Safety
///
/// Same contract as [`soma_break`].
#[no_mangle]
pub unsafe extern "C" fn soma_break_greedy(
    handle: u32,
    input: *const i32,
    in_len: u32,
    out: *mut i32,
    out_cap: u32,
) -> u32 {
    unsafe { break_inner(handle, input, in_len, out, out_cap, true) }
}
