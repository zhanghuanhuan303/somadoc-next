//! SVG renderer over [`PagedLayout`].
//!
//! This renderer exists for two reasons: it makes the layout *visible* without
//! a browser reflowing anything, and it gives the project a second renderer so
//! coordinates can be compared (Gate R). It is not a print path: text is
//! emitted as `<text>` with a font family, no outlines are embedded.
//!
//! Every atom element carries `data-page`, `data-node`, and `data-atom`, so a
//! test can check that the rendered coordinate is exactly the layout
//! coordinate.

#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]

extern crate alloc;

use alloc::format;
use alloc::string::String;

use soma_core::Width;
use soma_layout::{BoxKind, ImageSource, MathBox, NoImages, PagedLayout, PlacedBlock, TextSource};
use soma_template::{ComputedStyle, StyleTable};

const UNITS_PER_EM: i32 = 4096;

/// One em is 16 CSS pixels in the preview. Layout units are 1/4096 em, so a
/// page is `units / 256` pixels across, and the root viewport can be given an
/// absolute size while every coordinate stays in layout units (ADR-0049).
const PX_PER_EM: i64 = 16;

/// Render every page as its own `<svg>` element, with no image assets: figures
/// keep their placeholder box.
pub fn render(layout: &PagedLayout, styles: &StyleTable, text: &dyn TextSource) -> String {
    render_with(layout, styles, text, &NoImages)
}

/// Render with an image source. A figure whose asset resolves is inlined as a
/// `data:` URI, so the SVG stays a single file and never names an external
/// resource (ADR-0048).
pub fn render_with(
    layout: &PagedLayout,
    styles: &StyleTable,
    text: &dyn TextSource,
    assets: &dyn ImageSource,
) -> String {
    let sources = Sources { text, assets };
    let mut out = String::new();
    for page in &layout.pages {
        out.push_str(&format!(
            "<svg xmlns=\"http://www.w3.org/2000/svg\" data-page=\"{}\" width=\"{}\" height=\"{}\" viewBox=\"0 0 {} {}\">\n",
            page.number,
            px(layout.page.width),
            px(layout.page.height),
            layout.page.width.0,
            layout.page.height.0
        ));

        for block in &page.blocks {
            render_block(&mut out, layout, styles, &sources, page.number, block);
        }
        for block in &page.header {
            render_block(&mut out, layout, styles, &sources, page.number, block);
        }
        for block in &page.footer {
            render_block(&mut out, layout, styles, &sources, page.number, block);
        }
        for block in &page.footnotes {
            let top = layout.page.content_height().0 - page.footnote_height.0;
            render_block_at(&mut out, layout, styles, &sources, page.number, block, top);
        }

        out.push_str("</svg>\n");
    }
    out
}

/// The two renderer inputs that are not part of the layout: the text index and
/// the image bytes. Bundled so `render_block_at` stays a sane signature.
struct Sources<'a> {
    text: &'a dyn TextSource,
    assets: &'a dyn ImageSource,
}

fn render_block(
    out: &mut String,
    layout: &PagedLayout,
    styles: &StyleTable,
    sources: &Sources<'_>,
    page: u32,
    block: &PlacedBlock,
) {
    render_block_at(out, layout, styles, sources, page, block, 0);
}

fn render_block_at(
    out: &mut String,
    layout: &PagedLayout,
    styles: &StyleTable,
    sources: &Sources<'_>,
    page: u32,
    block: &PlacedBlock,
    extra_y: i32,
) {
    let text = sources.text;
    let style = styles.get(block.style);
    match block.kind {
        BoxKind::Rule | BoxKind::TableRule => {
            let paint = match style.and_then(|style| style.color) {
                Some(value) => color(Some(value)),
                None => String::from("#333"),
            };
            out.push_str(&format!(
                "<rect data-page=\"{page}\" data-node=\"{}\" x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"{}\"/>\n",
                block.node.0,
                units(Width(layout.page.margin_left.0 + block.x.0)),
                units(Width(layout.page.margin_top.0 + block.y.0 + extra_y)),
                units(block.width),
                units(block.height),
                paint
            ));
            return;
        }
        BoxKind::Figure => {
            let x = Width(layout.page.margin_left.0 + block.x.0);
            let y = Width(layout.page.margin_top.0 + block.y.0 + extra_y);
            let uri = text
                .block_image(block.node)
                .and_then(|src| sources.assets.image(src))
                .and_then(soma_layout::data_uri);
            match uri {
                Some(uri) => out.push_str(&format!(
                    "<image data-page=\"{page}\" data-node=\"{}\" x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" preserveAspectRatio=\"none\" href=\"{}\"/>\n",
                    block.node.0,
                    units(x),
                    units(y),
                    units(block.width),
                    units(block.height),
                    escape(&uri)
                )),
                None => out.push_str(&format!(
                    "<rect data-page=\"{page}\" data-node=\"{}\" x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"#eee\" stroke=\"#ccc\"/>\n",
                    block.node.0,
                    units(x),
                    units(y),
                    units(block.width),
                    units(block.height)
                )),
            }
            return;
        }
        BoxKind::Math if block.math.is_some() => {
            render_math(out, layout, style, page, block, extra_y);
            return;
        }
        _ => {}
    }

    for placed in &block.lines {
        for atom in &placed.line.boxes {
            // Inline formulas replace their marker atom and sit on the line
            // baseline.
            if let Some(inline) = block.inline_math.iter().find(|m| m.atom == atom.atom) {
                let left = layout.page.margin_left.0 + block.x.0 + atom.x.0;
                let baseline = layout.page.margin_top.0
                    + block.y.0
                    + extra_y
                    + placed.y.0
                    + placed.line.ascent.0;
                paint_math(
                    out,
                    page,
                    block.node.0,
                    color(style.and_then(|style| style.color)),
                    left,
                    baseline,
                    &inline.math,
                );
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
            let effective = atom_style
                .or(style)
                .cloned()
                .unwrap_or_else(ComputedStyle::base);
            let x = layout.page.margin_left.0 + block.x.0 + atom.x.0;
            let y =
                layout.page.margin_top.0 + block.y.0 + extra_y + placed.y.0 + placed.line.ascent.0;
            let decorations = text.atom_decorations(block.node, atom.atom);
            let decoration = if decorations.contains(soma_core::Decorations::UNDERLINE) {
                " text-decoration=\"underline\""
            } else if decorations.contains(soma_core::Decorations::STRIKETHROUGH) {
                " text-decoration=\"line-through\""
            } else {
                ""
            };
            // SVG is a preview like HTML, so it does not create external
            // references either; the PDF annotation is the actionable copy.
            out.push_str(&format!(
                "<text data-page=\"{page}\" data-node=\"{}\" data-atom=\"{}\" x=\"{}\" y=\"{}\" font-size=\"{}\" fill=\"{}\"{decoration}>{}</text>\n",
                block.node.0,
                atom.atom,
                units(Width(x)),
                units(Width(y)),
                units(effective.font_size),
                color(effective.color),
                escape(&drawn)
            ));
            // Ruby: a second text element, centred on the base and raised by
            // the shift the layout reserved (ADR-0060).
            if let Some(note) = block.ruby.iter().find(|note| note.atom == atom.atom) {
                out.push_str(&format!(
                    "<text data-page=\"{page}\" data-node=\"{}\" data-atom=\"{}\" data-ruby=\"1\" text-anchor=\"middle\" x=\"{}\" y=\"{}\" font-size=\"{}\" fill=\"{}\">{}</text>\n",
                    block.node.0,
                    atom.atom,
                    units(Width(x + atom.advance.0 / 2)),
                    units(Width(y - note.shift.0)),
                    units(note.size),
                    color(effective.color),
                    escape(&note.text)
                ));
            }
        }
        // A hyphenated line ends in a real hyphen, already counted in its
        // width (ADR-0059).
        if placed.line.hyphenated {
            if let Some(last) = placed.line.boxes.last() {
                let atom_style = text
                    .atom_style(block.node, last.atom)
                    .and_then(|id| styles.get(id));
                let effective = atom_style
                    .or(style)
                    .cloned()
                    .unwrap_or_else(ComputedStyle::base);
                let left = layout.page.margin_left.0 + block.x.0 + last.x.0 + last.advance.0;
                let baseline = layout.page.margin_top.0
                    + block.y.0
                    + extra_y
                    + placed.y.0
                    + placed.line.ascent.0;
                out.push_str(&format!(
                    "<text data-page=\"{page}\" data-node=\"{}\" data-atom=\"{}\" data-hyphen=\"1\" x=\"{}\" y=\"{}\" font-size=\"{}\" fill=\"{}\">-</text>\n",
                    block.node.0,
                    last.atom,
                    units(Width(left)),
                    units(Width(baseline)),
                    units(effective.font_size),
                    color(effective.color)
                ));
            }
        }
    }
}

/// Draw a display formula. Item `y` is relative to the formula baseline, which
/// SVG text positioning uses directly, so the output is exact.
fn render_math(
    out: &mut String,
    layout: &PagedLayout,
    style: Option<&ComputedStyle>,
    page: u32,
    block: &PlacedBlock,
    extra_y: i32,
) {
    let Some(math) = block.math.as_ref() else {
        return;
    };
    let left = layout.page.margin_left.0 + block.x.0;
    let baseline = layout.page.margin_top.0 + block.y.0 + extra_y + math.baseline.0;
    paint_math(
        out,
        page,
        block.node.0,
        color(style.and_then(|style| style.color)),
        left,
        baseline,
        math,
    );
}

/// Paint one formula whose baseline is at `baseline`. Shared by display math
/// (block top + `MathBox.baseline`) and inline math (line top + line ascent).
fn paint_math(
    out: &mut String,
    page: u32,
    node: u32,
    paint: String,
    left: i32,
    baseline: i32,
    math: &MathBox,
) {
    for item in &math.items {
        let fill = match item.color {
            Some(value) => color(Some(value)),
            None => paint.clone(),
        };
        if item.stretch == 1000 {
            out.push_str(&format!(
                "<text data-page=\"{page}\" data-node=\"{node}\" data-math=\"{}\" x=\"{}\" y=\"{}\" font-size=\"{}\" fill=\"{fill}\">{}</text>\n",
                escape(&item.text),
                units(Width(left + item.x.0)),
                units(Width(baseline + item.y.0)),
                units(item.size),
                escape(&item.text)
            ));
        } else {
            // A stretched glyph keeps its baseline: translate first, then
            // scale on x only.
            let ratio = ratio(item.stretch);
            out.push_str(&format!(
                "<text data-page=\"{page}\" data-node=\"{node}\" data-math=\"{}\" x=\"0\" y=\"0\" font-size=\"{}\" fill=\"{fill}\" transform=\"translate({} {}) scale({ratio} 1)\">{}</text>\n",
                escape(&item.text),
                units(item.size),
                units(Width(left + item.x.0)),
                units(Width(baseline + item.y.0)),
                escape(&item.text)
            ));
        }
    }
    for rule in &math.rules {
        out.push_str(&format!(
            "<rect data-page=\"{page}\" data-node=\"{node}\" data-rule=\"1\" x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"{}\"/>\n",
            units(Width(left + rule.x.0)),
            units(Width(baseline + rule.y.0)),
            units(rule.width),
            units(rule.thickness),
            paint
        ));
    }
}

/// A coordinate in the root coordinate system: layout units, unitless.
///
/// The `viewBox` is in the same units, so a coordinate needs no unit suffix.
/// Writing `4.319em` here — as this renderer used to — made every position
/// depend on the surrounding font size, which is not what the layout computed.
pub fn units(width: Width) -> String {
    format!("{}", width.0)
}

/// The root viewport size in pixels, at one em = 16 px.
fn px(width: Width) -> i64 {
    width.0 as i64 * PX_PER_EM / UNITS_PER_EM as i64
}

/// A permille scale as an SVG number, without floating point.
fn ratio(permille: u16) -> String {
    format!("{}.{:03}", permille / 1000, permille % 1000)
}

fn color(value: Option<u32>) -> String {
    match value {
        Some(value) => format!(
            "#{:02x}{:02x}{:02x}",
            value >> 16,
            (value >> 8) & 0xff,
            value & 0xff
        ),
        None => String::from("#000"),
    }
}

fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for character in text.chars() {
        match character {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            character => out.push(character),
        }
    }
    out
}
