//! Plain-text projection of DocIR.
//!
//! This is not a renderer. It exists for titles, table-of-contents entries,
//! alt-text fallbacks, footnote previews, and diagnostics. Layout and rendering
//! never use it.

use alloc::string::String;

use crate::block::{Block, GeneratedBlock, List, Row, Section, Table};
use crate::inline::Inline;
use crate::meta::ImageRef;

/// Append the text of `inlines`.
pub fn write_inlines(out: &mut String, inlines: &[Inline]) {
    for inline in inlines {
        write_inline(out, inline);
    }
}

/// Text of `inlines` as a new `String`.
pub fn inlines_text(inlines: &[Inline]) -> String {
    let mut out = String::new();
    write_inlines(&mut out, inlines);
    out
}

fn write_inline(out: &mut String, inline: &Inline) {
    match inline {
        Inline::Text(node) => out.push_str(&node.value),
        Inline::Code(node) => out.push_str(&node.value),
        Inline::Strong(_)
        | Inline::Emphasis(_)
        | Inline::Underline(_)
        | Inline::Strikethrough(_)
        | Inline::SmallCaps(_)
        | Inline::Superscript(_)
        | Inline::Subscript(_) => write_inlines(out, inline.children()),
        Inline::Link(node) => write_inlines(out, &node.children),
        Inline::Image(node) => {
            if let Some(alt) = &node.source.alt {
                out.push_str(alt);
            }
        }
        Inline::Math(node) => out.push_str(&node.source),
        // Reference markers are not text; numbering is generated later.
        Inline::FootnoteRef(_) | Inline::Index(_) => {}
        // A ruby annotation is a pronunciation hint, not part of the base text.
        Inline::Ruby(node) => out.push_str(&node.base),
        Inline::LineBreak(_) => out.push(' '),
        Inline::Raw(node) => out.push_str(&node.content.fallback),
    }
}

/// Append the text of `blocks`, one line per block-level unit.
pub fn write_blocks(out: &mut String, blocks: &[Block]) {
    for block in blocks {
        write_block(out, block);
    }
}

fn write_block(out: &mut String, block: &Block) {
    match block {
        Block::Paragraph(node) => {
            write_inlines(out, &node.inlines);
            out.push('\n');
        }
        Block::Heading(node) => {
            write_inlines(out, &node.inlines);
            out.push('\n');
        }
        Block::List(node) => write_list(out, node),
        Block::BlockQuote(node) => write_blocks(out, &node.blocks),
        Block::CodeBlock(node) => {
            out.push_str(&node.value);
            out.push('\n');
        }
        Block::MathBlock(node) => {
            out.push_str(&node.source);
            out.push('\n');
        }
        Block::Diagram(node) => {
            out.push_str(&node.source);
            out.push('\n');
        }
        Block::Figure(node) => {
            if node.caption.is_empty() {
                write_image_ref(out, &node.image.source);
                out.push('\n');
            } else {
                write_inlines(out, &node.caption);
                out.push('\n');
            }
        }
        Block::Table(node) => write_table(out, node),
        Block::ThematicBreak(_) | Block::PageBreak(_) => out.push('\n'),
        Block::Section(node) => write_section(out, node),
        Block::Generated(node) => write_generated(out, node),
        Block::Raw(node) => {
            out.push_str(&node.content.fallback);
            out.push('\n');
        }
    }
}

fn write_list(out: &mut String, list: &List) {
    for item in &list.items {
        write_blocks(out, &item.blocks);
    }
}

fn write_table(out: &mut String, table: &Table) {
    if !table.caption.is_empty() {
        write_inlines(out, &table.caption);
        out.push('\n');
    }
    for row in &table.rows {
        write_row(out, row);
    }
}

fn write_row(out: &mut String, row: &Row) {
    for (index, cell) in row.cells.iter().enumerate() {
        if index > 0 {
            out.push('\t');
        }
        write_blocks(out, &cell.blocks);
    }
    out.push('\n');
}

fn write_section(out: &mut String, section: &Section) {
    if let Some(title) = &section.title {
        write_inlines(out, title);
        out.push('\n');
    }
}

fn write_generated(out: &mut String, generated: &GeneratedBlock) {
    write_blocks(out, &generated.blocks);
}

fn write_image_ref(out: &mut String, image: &ImageRef) {
    if let Some(alt) = &image.alt {
        out.push_str(alt);
    }
}
