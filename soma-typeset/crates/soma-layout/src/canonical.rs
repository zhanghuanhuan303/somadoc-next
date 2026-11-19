//! Canonical JSON for page results.
//!
//! One hand-written writer, fixed field order, integers only. The resolved
//! `TextIndex` is derived from DocIR and therefore **not** part of the canonical
//! form: the hash covers geometry, node identity, and page structure.

use alloc::string::String;
use core::fmt::{self, Write};

use crate::PagedLayout;

pub(crate) const SCHEMA: &str = "soma.pages/6";

const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

struct FnvWriter {
    state: u64,
}

impl FnvWriter {
    const fn new() -> Self {
        Self { state: FNV_OFFSET }
    }
}

impl Write for FnvWriter {
    fn write_str(&mut self, text: &str) -> fmt::Result {
        for byte in text.as_bytes() {
            self.state ^= *byte as u64;
            self.state = self.state.wrapping_mul(FNV_PRIME);
        }
        Ok(())
    }
}

pub(crate) fn json(layout: &PagedLayout) -> String {
    let mut out = String::new();
    write_layout(&mut out, layout).expect("writing into a String cannot fail");
    out
}

pub(crate) fn hash(layout: &PagedLayout) -> u64 {
    let mut writer = FnvWriter::new();
    write_layout(&mut writer, layout).expect("hashing cannot fail");
    writer.state
}

fn write_layout(out: &mut impl Write, layout: &PagedLayout) -> fmt::Result {
    out.write_str("{\"schema\":\"")?;
    out.write_str(SCHEMA)?;
    out.write_str("\",\"pagination\":\"")?;
    out.write_str(layout.pagination.as_str())?;
    write!(
        out,
        "\",\"engineHash\":{},\"rulesVersion\":{},\"metricsVersion\":{},\"metricsHash\":{},\"verticalHash\":{}",
        layout.engine_hash,
        layout.rules_version,
        layout.metrics_version,
        layout.metrics_hash,
        layout.vertical_hash
    )?;

    let page = layout.page;
    write!(out, ",\"page\":{{\"width\":{}", page.width.0)?;
    write!(out, ",\"height\":{}", page.height.0)?;
    write!(out, ",\"marginTop\":{}", page.margin_top.0)?;
    write!(out, ",\"marginBottom\":{}", page.margin_bottom.0)?;
    write!(out, ",\"marginLeft\":{}", page.margin_left.0)?;
    write!(out, ",\"marginRight\":{}}}", page.margin_right.0)?;

    write!(
        out,
        ",\"pageCount\":{},\"footnotePasses\":{},\"pages\":[",
        layout.page_count(),
        layout.footnote_passes
    )?;

    for (index, page) in layout.pages.iter().enumerate() {
        if index > 0 {
            out.write_char(',')?;
        }
        write!(out, "{{\"number\":{}", page.number)?;
        write!(out, ",\"usedHeight\":{}", page.used_height.0)?;
        write!(out, ",\"footnoteHeight\":{}", page.footnote_height.0)?;
        write!(out, ",\"overflow\":{}", page.overflow)?;
        out.write_str(",\"blocks\":[")?;
        for (block_index, block) in page.blocks.iter().enumerate() {
            if block_index > 0 {
                out.write_char(',')?;
            }
            write_block(out, block)?;
        }
        out.write_str("],\"header\":[")?;
        for (block_index, block) in page.header.iter().enumerate() {
            if block_index > 0 {
                out.write_char(',')?;
            }
            write_block(out, block)?;
        }
        out.write_str("],\"footer\":[")?;
        for (block_index, block) in page.footer.iter().enumerate() {
            if block_index > 0 {
                out.write_char(',')?;
            }
            write_block(out, block)?;
        }
        out.write_str("],\"footnotes\":[")?;
        for (block_index, block) in page.footnotes.iter().enumerate() {
            if block_index > 0 {
                out.write_char(',')?;
            }
            write_block(out, block)?;
        }
        out.write_str("]}")?;
    }

    out.write_str("]}")
}

fn write_block(out: &mut impl Write, block: &crate::PlacedBlock) -> fmt::Result {
    write!(
        out,
        "{{\"node\":{},\"kind\":\"{}\",\"x\":{},\"y\":{},\"width\":{},\"height\":{},\"lineOffset\":{},\"linesTotal\":{},\"stage\":\"{}\"",
        block.node.0,
        block.kind.as_str(),
        block.x.0,
        block.y.0,
        block.width.0,
        block.height.0,
        block.line_offset,
        block.lines_total,
        block.stage.as_str()
    )?;

    out.write_str(",\"marks\":[")?;
    for (index, mark) in block.marks.iter().enumerate() {
        if index > 0 {
            out.write_char(',')?;
        }
        write!(
            out,
            "{{\"atom\":{},\"number\":{},\"line\":{}}}",
            mark.atom, mark.number, mark.line
        )?;
    }
    out.write_str("],\"lines\":[")?;

    for (index, placed) in block.lines.iter().enumerate() {
        if index > 0 {
            out.write_char(',')?;
        }
        write!(
            out,
            "{{\"y\":{},\"hang\":{},\"line\":",
            placed.y.0, placed.hang
        )?;
        placed.line.write_canonical(out)?;
        out.write_char('}')?;
    }

    out.write_str("],\"math\":")?;
    write_math(out, block.math.as_ref())?;
    out.write_str(",\"inlineMath\":[")?;
    for (index, inline) in block.inline_math.iter().enumerate() {
        if index > 0 {
            out.write_char(',')?;
        }
        write!(out, "{{\"atom\":{},\"source\":", inline.atom)?;
        write_string(out, &inline.source)?;
        out.write_str(",\"math\":")?;
        write_math_payload(out, &inline.math)?;
        out.write_char('}')?;
    }
    out.write_char(']')?;
    out.write_char('}')
}

/// `null` or the display-formula geometry. Item and rule `y` are relative to
/// `baseline`, which is measured from the block top.
fn write_math(out: &mut impl Write, math: Option<&crate::MathBox>) -> fmt::Result {
    match math {
        Some(math) => write_math_payload(out, math),
        None => out.write_str("null"),
    }
}

fn write_math_payload(out: &mut impl Write, math: &crate::MathBox) -> fmt::Result {
    write!(
        out,
        "{{\"width\":{},\"height\":{},\"baseline\":{},\"items\":[",
        math.width.0, math.height.0, math.baseline.0
    )?;
    for (index, item) in math.items.iter().enumerate() {
        if index > 0 {
            out.write_char(',')?;
        }
        out.write_str("{\"text\":")?;
        write_string(out, &item.text)?;
        write!(
            out,
            ",\"x\":{},\"y\":{},\"size\":{}}}",
            item.x.0, item.y.0, item.size.0
        )?;
    }
    out.write_str("],\"rules\":[")?;
    for (index, rule) in math.rules.iter().enumerate() {
        if index > 0 {
            out.write_char(',')?;
        }
        write!(
            out,
            "{{\"x\":{},\"y\":{},\"width\":{},\"thickness\":{}}}",
            rule.x.0, rule.y.0, rule.width.0, rule.thickness.0
        )?;
    }
    out.write_str("]}")
}

/// JSON string with the escapes the canonical form needs. Formula text is a
/// short symbol run, so the allocation-free loop is enough.
fn write_string(out: &mut impl Write, text: &str) -> fmt::Result {
    out.write_char('"')?;
    for character in text.chars() {
        match character {
            '"' => out.write_str("\\\"")?,
            '\\' => out.write_str("\\\\")?,
            '\n' => out.write_str("\\n")?,
            '\r' => out.write_str("\\r")?,
            '\t' => out.write_str("\\t")?,
            character if (character as u32) < 0x20 => {
                write!(out, "\\u{:04x}", character as u32)?;
            }
            character => out.write_char(character)?,
        }
    }
    out.write_char('"')
}
