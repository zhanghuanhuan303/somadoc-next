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
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use soma_core::Width;
use soma_layout::{
    BoxKind, ImageSource, MathBox, NoImages, PagedLayout, PlacedBlock, TextSource, TikzPoint,
    TikzSegment,
};
use soma_template::{ComputedStyle, FontSlant, FontWeight, StyleTable};

const UNITS_PER_EM: i32 = 4096;

/// Render every page to one self-contained HTML document, with no image
/// assets: figures keep their placeholder box.
pub fn render(layout: &PagedLayout, styles: &StyleTable, text: &dyn TextSource) -> String {
    render_with(layout, styles, text, &NoImages)
}

/// Render with an image source. A figure whose asset resolves is inlined as a
/// `data:` URI, so the document stays self-contained and never names an
/// external resource (ADR-0048).
pub fn render_with(
    layout: &PagedLayout,
    styles: &StyleTable,
    text: &dyn TextSource,
    assets: &dyn ImageSource,
) -> String {
    let sources = Sources { text, assets };
    let mut html = String::new();
    html.push_str("<!DOCTYPE html>\n<html><head><meta charset=\"utf-8\">\n");
    html.push_str("<style>");
    html.push_str(
        "html,body{margin:0;padding:0;background:#f4f4f4;}\
.page{position:relative;background:#fff;margin:1em auto;box-shadow:0 0 4px #bbb;overflow:hidden;}\
.block{position:absolute;white-space:pre;}\
.atom{position:absolute;white-space:pre;}\
.footnote-number{position:absolute;white-space:pre;}\
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
            render_block(&mut html, layout, styles, &sources, block, 0, 0);
        }
        for block in &page.header {
            render_block(&mut html, layout, styles, &sources, block, 0, 0);
        }
        for block in &page.footer {
            render_block(&mut html, layout, styles, &sources, block, 0, 0);
        }

        let footnote_top = (layout.page.content_height().0 - page.footnote_height.0).max(0);
        if !page.footnotes.is_empty() {
            html.push_str(&format!(
                "<div class=\"table-rule\" style=\"left:{};top:{};width:{};height:{};\"></div>\n",
                em(layout.page.margin_left),
                em(Width(layout.page.margin_top.0 + footnote_top)),
                em(soma_layout::footnote_rule_width(
                    layout.page.content_width()
                )),
                em(soma_layout::FOOTNOTE_RULE_THICKNESS)
            ));
        }
        for (index, block) in page.footnotes.iter().enumerate() {
            if let Some(number) = page.footnote_numbers.get(index).copied().flatten() {
                render_footnote_number(&mut html, layout, styles, block, number, footnote_top);
            }
            render_block(&mut html, layout, styles, &sources, block, 0, footnote_top);
        }

        html.push_str("</div>\n");
    }

    html.push_str("</body></html>\n");
    html
}

/// A note number in the left column the layout reserved for it. The preview
/// uses the same smaller size and raised baseline as the PDF.
fn render_footnote_number(
    html: &mut String,
    layout: &PagedLayout,
    styles: &StyleTable,
    block: &PlacedBlock,
    number: u32,
    footnote_top: i32,
) {
    let Some(first) = block.lines.first() else {
        return;
    };
    let Some(style) = styles.get(block.style) else {
        return;
    };
    let mut number_style = style.clone();
    number_style.font_size = Width(style.font_size.0 * 2 / 3);
    number_style.line_height = number_style.font_size;
    let left = layout.page.margin_left.0;
    let top = layout.page.margin_top.0 + footnote_top + block.y.0 + first.y.0;
    html.push_str(&format!(
        "<span class=\"footnote-number\" data-footnote-number=\"{number}\" style=\"left:{};top:{};{}\">{number}</span>\n",
        em(Width(left)),
        em(Width(top)),
        style_attributes(Some(&number_style), number_style.font_size)
    ));
}

/// The two renderer inputs that are not part of the layout: the text index and
/// the image bytes.
struct Sources<'a> {
    text: &'a dyn TextSource,
    assets: &'a dyn ImageSource,
}

fn render_block(
    html: &mut String,
    layout: &PagedLayout,
    styles: &StyleTable,
    sources: &Sources<'_>,
    block: &PlacedBlock,
    extra_x: i32,
    extra_y: i32,
) {
    let text = sources.text;
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
            let uri = text
                .block_image(block.node)
                .and_then(|src| sources.assets.image(src))
                .and_then(soma_layout::data_uri);
            let style = format!(
                "left:{};top:{};width:{};height:{};",
                em(Width(left)),
                em(Width(top)),
                em(block.width),
                em(block.height)
            );
            match uri {
                Some(uri) => html.push_str(&format!(
                    "<img class=\"figure\" alt=\"{}\" style=\"{}\" src=\"{}\">\n",
                    escape(text.block_alt(block.node).unwrap_or("")),
                    style,
                    escape(&uri)
                )),
                None => html.push_str(&format!(
                    "<div class=\"figure\" style=\"{}\"></div>\n",
                    style
                )),
            }
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
                let baseline = top + placed.y.0 + placed.line.ascent.0 + placed.line.leading.0 / 2;
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
            let syntax = text
                .atom_syntax(block.node, atom.atom)
                .and_then(|class| styles.syntax_color(class))
                .map(|color| {
                    format!(
                        "color:#{:02x}{:02x}{:02x};",
                        color >> 16,
                        (color >> 8) & 0xff,
                        color & 0xff
                    )
                })
                .unwrap_or_default();
            // Ruby is an inline annotation: `<ruby>` keeps the base in the
            // text flow and puts the reading above it, centred, without the
            // renderer doing any measurement (ADR-0060).
            let body = match block.ruby.iter().find(|note| note.atom == atom.atom) {
                Some(note) => format!(
                    "<ruby>{}\u{200b}<rt style=\"font-size:{}\">{}</rt></ruby>",
                    escape(&drawn),
                    em(note.size),
                    escape(&note.text)
                ),
                None => escape(&drawn),
            };
            // Links stay inert here: the preview must not create external
            // references (docs/capabilities.md). The PDF carries them as
            // annotations, where a reader can act on them.
            html.push_str(&format!(
                "<span class=\"atom\" data-node=\"{}\" data-atom=\"{}\" style=\"left:{};top:{};{}{}{}\">{}{}</span>\n",
                block.node.0,
                atom.atom,
                em(atom.x),
                em(placed.y),
                font,
                decoration,
                syntax,
                body,
                ""
            ));
        }
        // A hyphenated line ends in a real hyphen, already counted in its
        // width (ADR-0059).
        if placed.line.hyphenated {
            if let Some(last) = placed.line.boxes.last() {
                html.push_str(&format!(
                    "<span class=\"atom\" data-node=\"{}\" data-atom=\"{}\" data-hyphen=\"1\" style=\"left:{};top:{};\">-</span>\n",
                    block.node.0,
                    last.atom,
                    em(Width(last.x.0 + last.advance.0)),
                    em(placed.y)
                ));
            }
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
fn paint_tikz_paths(html: &mut String, left: i32, baseline: i32, math: &MathBox) {
    if math.paths.is_empty() {
        return;
    }
    let top = baseline - math.height.0;
    html.push_str(&format!(
        "<svg class=\"tikz\" style=\"position:absolute;left:{};top:{};width:{};height:{};overflow:visible\" viewBox=\"0 0 {} {}\">\n",
        em(Width(left)),
        em(Width(top)),
        em(math.width),
        em(math.height),
        math.width.0,
        math.height.0
    ));
    for path in &math.paths {
        if path.segments.is_empty() {
            continue;
        }
        let x = |point: &TikzPoint| point.x.0;
        let y = |point: &TikzPoint| point.y.0 + math.height.0;
        let mut d = format!("M {} {}", x(&path.start), y(&path.start));
        for segment in &path.segments {
            match segment {
                TikzSegment::Line(to) => d.push_str(&format!(" L {} {}", x(to), y(to))),
                TikzSegment::Cubic { c1, c2, to } => d.push_str(&format!(
                    " C {} {} {} {} {} {}",
                    x(c1),
                    y(c1),
                    x(c2),
                    y(c2),
                    x(to),
                    y(to)
                )),
                TikzSegment::Close => d.push_str(" Z"),
            }
        }
        let fill = path
            .fill
            .map(|value| format!("#{value:06x}"))
            .unwrap_or_else(|| String::from("none"));
        let stroke = match &path.stroke {
            Some(stroke) => format!(
                " stroke=\"#{:06x}\" stroke-width=\"{}\"",
                stroke.color, stroke.width.0
            ),
            None => String::new(),
        };
        let dash = match &path.stroke {
            Some(stroke) if !stroke.dash.is_empty() => format!(
                " stroke-dasharray=\"{}\"",
                stroke
                    .dash
                    .iter()
                    .map(|value| value.0.to_string())
                    .collect::<Vec<_>>()
                    .join(" ")
            ),
            _ => String::new(),
        };
        html.push_str(&format!(
            "<path d=\"{d}\" fill=\"{fill}\"{stroke}{dash}/>\n"
        ));
    }
    html.push_str("</svg>\n");
}

fn paint_math(
    html: &mut String,
    style: Option<&ComputedStyle>,
    left: i32,
    baseline: i32,
    math: &MathBox,
) {
    paint_tikz_paths(html, left, baseline, math);
    let paint = style
        .and_then(|style| style.color)
        .map(|color| format!("#{color:06x}"))
        .unwrap_or_else(|| String::from("#000"));

    for item in &math.items {
        let x = left + item.x.0;
        let y = baseline + item.y.0 - ascent_proxy(item.size);
        let paint = match item.color {
            Some(value) => format!("#{value:06x}"),
            None => paint.clone(),
        };
        // A stretched glyph (an over/underbrace) is scaled on x only; the
        // transform origin keeps its left edge where the layout put it.
        let transform = if item.stretch == 1000 {
            String::new()
        } else {
            format!(
                "transform:scaleX({}.{:03});transform-origin:left bottom;",
                item.stretch / 1000,
                item.stretch % 1000
            )
        };
        html.push_str(&format!(
            "<span class=\"math\" style=\"left:{};top:{};font-size:{};color:{};{}\">{}</span>\n",
            em(Width(x)),
            em(Width(y)),
            em(item.size),
            paint,
            transform,
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
        let family = if family.to_ascii_lowercase().contains("mono")
            || family.to_ascii_lowercase().contains("code")
        {
            "monospace"
        } else if family.to_ascii_lowercase().contains("sans")
            || family.to_ascii_lowercase().contains("hei")
            || family.to_ascii_lowercase().contains("gothic")
        {
            "sans-serif"
        } else {
            "serif"
        };
        out.push_str(&format!("font-family:{};", escape_attr(family)));
    }
    if let Some(background) = style.background {
        out.push_str(&format!(
            "background:#{:02x}{:02x}{:02x};",
            background >> 16,
            (background >> 8) & 0xff,
            background & 0xff
        ));
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
    render_page_with(layout, styles, text, page, &NoImages)
}

pub fn render_page_with(
    layout: &PagedLayout,
    styles: &StyleTable,
    text: &dyn TextSource,
    page: usize,
    assets: &dyn ImageSource,
) -> String {
    let mut subset = layout.clone();
    subset.pages = subset
        .pages
        .into_iter()
        .skip(page)
        .take(1)
        .collect::<Vec<_>>();
    render_with(&subset, styles, text, assets)
}
