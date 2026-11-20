//! Absolutely positioned HTML renderer.
//!
//! The renderer consumes `PagedLayout` + `StyleTable` + a `TextSource` and
//! nothing else (ADR-0010 §2). It never breaks lines, never paginates, and never
//! reads DocIR. Positions are integer-derived, so the output is stable for a
//! given layout.

#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]

extern crate alloc;

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use soma_core::Width;
use soma_layout::{BoxKind, MathBox, PagedLayout, PlacedBlock, TextSource};
use soma_template::{ComputedStyle, FontSlant, FontWeight, StyleTable};

const UNITS_PER_EM: i32 = 4096;

/// Render every page to one self-contained HTML document.
pub fn render(layout: &PagedLayout, styles: &StyleTable, text: &dyn TextSource) -> String {
    let mut html = String::new();
    html.push_str("<!DOCTYPE html>\n<html><head><meta charset=\"utf-8\">\n");
    html.push_str("<style>");
    html.push_str(
        "html,body{margin:0;padding:0;background:#f4f4f4;}\
.page{position:relative;background:#fff;margin:1em auto;box-shadow:0 0 4px #bbb;overflow:hidden;}\
.block{position:absolute;white-space:pre;}\
.atom{position:absolute;white-space:pre;}\
.math{position:absolute;white-space:pre;}\
.math-rule{position:absolute;}\
.figure{position:absolute;background:#eee;border:1px solid #ccc;}\
.rule{position:absolute;border-top:1px solid #333;}\
.table-rule{position:absolute;background:#333;}\
.code{font-family:monospace;}\
",
    );
    html.push_str("</style></head><body>\n");

    for page in &layout.pages {
        html.push_str(&format!(
            "<div class=\"page\" data-page=\"{}\" style=\"width:{};height:{};\">\n",
            page.number,
            em(layout.page.width),
            em(layout.page.height)
        ));

        for block in &page.blocks {
            render_block(&mut html, layout, styles, text, block, 0, 0);
        }
        for block in &page.header {
            render_block(&mut html, layout, styles, text, block, 0, 0);
        }
        for block in &page.footer {
            render_block(&mut html, layout, styles, text, block, 0, 0);
        }

        let footnote_top = layout.page.content_height().0 - page.footnote_height.0;
        for block in &page.footnotes {
            render_block(
                &mut html,
                layout,
                styles,
                text,
                block,
                0,
                footnote_top.max(0),
            );
        }

        html.push_str("</div>\n");
    }

    html.push_str("</body></html>\n");
    html
}

fn render_block(
    html: &mut String,
    layout: &PagedLayout,
    styles: &StyleTable,
    text: &dyn TextSource,
    block: &PlacedBlock,
    extra_x: i32,
    extra_y: i32,
) {
    let style = styles.get(block.style);
    let left = layout.page.margin_left.0 + block.x.0 + extra_x;
    let top = layout.page.margin_top.0 + block.y.0 + extra_y;

    match block.kind {
        BoxKind::Rule => {
            html.push_str(&format!(
                "<div class=\"rule\" style=\"left:{};top:{};width:{};\"></div>\n",
                em(Width(left)),
                em(Width(top)),
                em(block.width)
            ));
            return;
        }
        BoxKind::TableRule => {
            let paint = style
                .and_then(|style| style.color)
                .map(|color| format!("#{color:06x}"))
                .unwrap_or_else(|| String::from("#333"));
            html.push_str(&format!(
                "<div class=\"table-rule\" style=\"left:{};top:{};width:{};height:{};background:{};\"></div>\n",
                em(Width(left)),
                em(Width(top)),
                em(block.width),
                em(block.height),
                paint
            ));
            return;
        }
        BoxKind::Figure => {
            html.push_str(&format!(
                "<div class=\"figure\" style=\"left:{};top:{};width:{};height:{};\"></div>\n",
                em(Width(left)),
                em(Width(top)),
                em(block.width),
                em(block.height)
            ));
            return;
        }
        BoxKind::Math if block.math.is_some() => {
            render_math(html, style, block, left, top);
            return;
        }
        _ => {}
    }

    let class = if block.kind == BoxKind::Code {
        "block code"
    } else {
        "block"
    };
    html.push_str(&format!(
        "<div class=\"{}\" data-node=\"{}\" style=\"left:{};top:{};width:{};height:{};{}\">\n",
        class,
        block.node.0,
        em(Width(left)),
        em(Width(top)),
        em(block.width),
        em(block.height),
        style_attributes(style, Width::EM)
    ));

    for placed in &block.lines {
        for atom in &placed.line.boxes {
            // Inline formulas replace their marker atom: the box is drawn at
            // the atom origin, on the line baseline.
            if let Some(inline) = block.inline_math.iter().find(|m| m.atom == atom.atom) {
                let left = left + atom.x.0;
                let baseline = top + placed.y.0 + placed.line.ascent.0;
                paint_math(html, style, left, baseline, &inline.math);
                continue;
            }
            let atom_text = text.atom_text(block.node, atom.atom);
            if atom_text.is_empty() {
                continue;
            }
            let drawn = match block.marks.iter().find(|mark| mark.atom == atom.atom) {
                Some(mark) => format!("{}", mark.number),
                None => String::from(atom_text),
            };
            let atom_style = text
                .atom_style(block.node, atom.atom)
                .and_then(|id| styles.get(id));
            let font = match atom_style {
                Some(atom_style) if atom_style != style.unwrap_or(&ComputedStyle::base()) => {
                    style_attributes(
                        Some(atom_style),
                        style.map(|s| s.font_size).unwrap_or(Width::EM),
                    )
                }
                _ => String::new(),
            };
            let decorations = text.atom_decorations(block.node, atom.atom);
            let decoration = if decorations.contains(soma_core::Decorations::UNDERLINE) {
                "text-decoration:underline;"
            } else if decorations.contains(soma_core::Decorations::STRIKETHROUGH) {
                "text-decoration:line-through;"
            } else {
                ""
            };
            // Links stay inert here: the preview must not create external
            // references (docs/capabilities.md). The PDF carries them as
            // annotations, where a reader can act on them.
            html.push_str(&format!(
                "<span class=\"atom\" data-node=\"{}\" data-atom=\"{}\" style=\"left:{};top:{};{}{}\">{}{}</span>\n",
                block.node.0,
                atom.atom,
                em(atom.x),
                em(placed.y),
                font,
                decoration,
                escape(&drawn),
                ""
            ));
        }
    }

    html.push_str("</div>\n");
}

/// Draw a display formula as absolutely positioned spans and rules.
///
/// Item `y` is measured from the formula baseline. CSS has no baseline
/// positioning, so the span top is approximated as `baseline - 7/8 em` of the
/// item size: the metric faces sit at about 0.875 em ascent, and the browser
/// then places its own baseline inside that line box. SVG output is exact.
fn render_math(
    html: &mut String,
    style: Option<&ComputedStyle>,
    block: &PlacedBlock,
    left: i32,
    top: i32,
) {
    let Some(math) = block.math.as_ref() else {
        return;
    };
    paint_math(html, style, left, top + math.baseline.0, math);
}

/// Paint one formula whose baseline is at `baseline`. Shared by display math
/// (block top + `MathBox.baseline`) and inline math (line top + line ascent).
fn paint_math(
    html: &mut String,
    style: Option<&ComputedStyle>,
    left: i32,
    baseline: i32,
    math: &MathBox,
) {
    let paint = style
        .and_then(|style| style.color)
        .map(|color| format!("#{color:06x}"))
        .unwrap_or_else(|| String::from("#000"));

    for item in &math.items {
        let x = left + item.x.0;
        let y = baseline + item.y.0 - ascent_proxy(item.size);
        html.push_str(&format!(
            "<span class=\"math\" style=\"left:{};top:{};font-size:{};color:{};\">{}</span>\n",
            em(Width(x)),
            em(Width(y)),
            em(item.size),
            paint,
            escape(&item.text)
        ));
    }
    for rule in &math.rules {
        html.push_str(&format!(
            "<div class=\"math-rule\" style=\"left:{};top:{};width:{};height:{};background:{};\"></div>\n",
            em(Width(left + rule.x.0)),
            em(Width(baseline + rule.y.0)),
            em(rule.width),
            em(rule.thickness),
            paint
        ));
    }
}

/// Line-box top for a font size: `7/8 em`, the ascent the metric faces use.
fn ascent_proxy(size: Width) -> i32 {
    size.0 / 8 * 7
}

fn style_attributes(style: Option<&ComputedStyle>, base: Width) -> String {
    let Some(style) = style else {
        return format!("font-size:{};", em(base));
    };
    let mut out = format!("font-size:{};", em(style.font_size));
    if style.line_height.0 > 0 {
        out.push_str(&format!("line-height:{};", em(style.line_height)));
    }
    if let Some(family) = &style.family {
        out.push_str(&format!("font-family:{};", escape_attr(family)));
    }
    if let Some(color) = style.color {
        out.push_str(&format!(
            "color:#{:02x}{:02x}{:02x};",
            color >> 16,
            (color >> 8) & 0xff,
            color & 0xff
        ));
    }
    if style.weight == FontWeight::Bold {
        out.push_str("font-weight:bold;");
    }
    if style.slant == FontSlant::Italic {
        out.push_str("font-style:italic;");
    }
    if style.underline {
        out.push_str("text-decoration:underline;");
    }
    if style.small_caps {
        out.push_str("font-variant:small-caps;");
    }
    out
}

/// Format 1/4096 em units as a CSS `em` length using integer arithmetic only.
fn em(width: Width) -> String {
    let units = width.0;
    let sign = if units < 0 { "-" } else { "" };
    let absolute = units.abs();
    format!(
        "{}{}.{:03}em",
        sign,
        absolute / UNITS_PER_EM,
        (absolute % UNITS_PER_EM) * 1000 / UNITS_PER_EM
    )
}

fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for character in text.chars() {
        match character {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\u{e000}' => {}
            character => out.push(character),
        }
    }
    out
}

fn escape_attr(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for character in text.chars() {
        match character {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            ';' => out.push(' '),
            character => out.push(character),
        }
    }
    out
}

/// Build an HTML document for one page only (useful for previews and tests).
pub fn render_page(
    layout: &PagedLayout,
    styles: &StyleTable,
    text: &dyn TextSource,
    page: usize,
) -> String {
    let mut subset = layout.clone();
    subset.pages = subset
        .pages
        .into_iter()
        .skip(page)
        .take(1)
        .collect::<Vec<_>>();
    render(&subset, styles, text)
}
