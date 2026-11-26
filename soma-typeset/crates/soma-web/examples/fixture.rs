//! Print the cross-language parity fixture.
//!
//! `scripts/web.sh` regenerates `web/parity.txt` from this binary; both the Rust
//! ABI test and the browser harness read that file and must agree with it.
//!
//! The width table is deterministic on purpose (1 em per CJK character, half an
//! em per ASCII character) so the fixture does not depend on any font.

use soma_core::{
    break_paragraph, Align, Atom, BreakConfig, Decorations, GlyphMetrics, LineMetrics, Metrics,
    ParagraphStyle, StyleId, Width,
};
use soma_rules::{paragraph_from_spans, InlineSpan, SpanKind};

const TEXT: &str =
    "中文English混排，数字 123，排版Engine v2。「引号」，（括号）；冒号：逗号，句号。";
const WIDTH: i32 = 24_000;
const LINE_HEIGHT: i32 = 6_144;
const SPACE: i32 = 1_024;
const HYPHEN: i32 = 682;
const ASCENT: i32 = 3_604;
const DESCENT: i32 = 901;
const LEADING: i32 = 819;

fn width_of(text: &str) -> i32 {
    text.chars()
        .map(|character| if character.is_ascii() { 512 } else { 4096 })
        .sum()
}

struct Fixed<'a> {
    widths: &'a [i32],
}

impl Metrics for Fixed<'_> {
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

fn main() {
    let style = ParagraphStyle {
        width: Width(WIDTH),
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
    let widths: Vec<i32> = paragraph.atoms.iter().map(|a| width_of(&a.text)).collect();
    let result = break_paragraph(
        &paragraph,
        &Fixed { widths: &widths },
        &BreakConfig::default(),
    )
    .expect("break");

    println!(
        "# soma-web parity fixture — regenerate with `cargo run -p soma-web --example fixture`"
    );
    println!("text|{TEXT}");
    println!("width|{WIDTH}");
    println!("firstIndent|0");
    println!("lineHeight|{LINE_HEIGHT}");
    println!("space|{SPACE}");
    println!("hyphen|{HYPHEN}");
    println!("ascent|{ASCENT}");
    println!("descent|{DESCENT}");
    println!("leading|{LEADING}");
    println!(
        "widths|{}",
        widths
            .iter()
            .map(|w| w.to_string())
            .collect::<Vec<_>>()
            .join(",")
    );
    println!(
        "ends|{}",
        result
            .lines
            .iter()
            .map(|line| line.end.to_string())
            .collect::<Vec<_>>()
            .join(",")
    );
}
