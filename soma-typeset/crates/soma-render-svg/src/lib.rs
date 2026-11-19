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
use soma_layout::{BoxKind, PagedLayout, PlacedBlock, TextSource};
use soma_template::{ComputedStyle, StyleTable};

const UNITS_PER_EM: i32 = 4096;

/// Render every page as its own `<svg>` element.
pub fn render(layout: &PagedLayout, styles: &StyleTable, text: &dyn TextSource) -> String {
    let mut out = String::new();
    for page in &layout.pages {
        out.push_str(&format!(
            "<svg xmlns=\"http://www.w3.org/2000/svg\" data-page=\"{}\" width=\"{}\" height=\"{}\" viewBox=\"0 0 {} {}\">\n",
            page.number,
            em(layout.page.width),
            em(layout.page.height),
            layout.page.width.0,
            layout.page.height.0
        ));

        for block in &page.blocks {
            render_block(&mut out, layout, styles, text, page.number, block);
        }
        for block in &page.header {
            render_block(&mut out, layout, styles, text, page.number, block);
        }
        for block in &page.footer {
            render_block(&mut out, layout, styles, text, page.number, block);
        }
        for block in &page.footnotes {
            let top = layout.page.content_height().0 - page.footnote_height.0;
            render_block_at(&mut out, layout, styles, text, page.number, block, top);
        }

        out.push_str("</svg>\n");
    }
    out
}

fn render_block(
    out: &mut String,
    layout: &PagedLayout,
    styles: &StyleTable,
    text: &dyn TextSource,
    page: u32,
    block: &PlacedBlock,
) {
    render_block_at(out, layout, styles, text, page, block, 0);
}

fn render_block_at(
    out: &mut String,
    layout: &PagedLayout,
    styles: &StyleTable,
    text: &dyn TextSource,
    page: u32,
    block: &PlacedBlock,
    extra_y: i32,
) {
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
                em(Width(layout.page.margin_left.0 + block.x.0)),
                em(Width(layout.page.margin_top.0 + block.y.0 + extra_y)),
                em(block.width),
                em(block.height),
                paint
            ));
            return;
        }
        BoxKind::Figure => {
            out.push_str(&format!(
                "<rect data-page=\"{page}\" data-node=\"{}\" x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"#eee\" stroke=\"#ccc\"/>\n",
                block.node.0,
                em(Width(layout.page.margin_left.0 + block.x.0)),
                em(Width(layout.page.margin_top.0 + block.y.0 + extra_y)),
                em(block.width),
                em(block.height)
            ));
            return;
        }
        _ => {}
    }

    for placed in &block.lines {
        for atom in &placed.line.boxes {
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
            out.push_str(&format!(
                "<text data-page=\"{page}\" data-node=\"{}\" data-atom=\"{}\" x=\"{}\" y=\"{}\" font-size=\"{}\" fill=\"{}\">{}</text>\n",
                block.node.0,
                atom.atom,
                em(Width(x)),
                em(Width(y)),
                em(effective.font_size),
                color(effective.color),
                escape(&drawn)
            ));
        }
    }
}

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
