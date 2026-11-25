//! Markdown subset front end.
//!
//! The front end is deliberately small and forgiving: anything it does not
//! recognise becomes text, never a dropped node. It produces DocIR only; it
//! knows nothing about templates, styles, or pages.
//!
//! Supported: ATX headings, paragraphs (soft line joins), hard breaks (two
//! trailing spaces or a trailing backslash), emphasis/strong, inline code,
//! links, images, fenced code blocks, flat lists, block quotes, thematic
//! breaks, footnote definitions and references, and backslash escapes.
//!
//! Not supported (recorded in `docs/debt.md`): nested lists, setext headings,
//! tables, HTML blocks, link reference definitions, and multi-line inline code.

#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]

extern crate alloc;

pub mod capabilities;

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use soma_doc::block::{BlockQuote, CodeBlock, Heading, List, ListItem, Paragraph, ThematicBreak};
use soma_doc::inline::{
    Container, FootnoteRef, Image, IndexRef, Inline, LineBreak, LineBreakKind, Link,
};
use soma_doc::meta::{DocumentMeta, ImageRef};
use soma_doc::node::NodeMeta;
use soma_doc::{
    Block, Code, Document, FootnoteDef, SourceId, SourceInfo, SourceKind, SourceMap, SourceSpan,
    Text,
};

pub use capabilities::{capability, with_status, Capability, CapabilityStatus, CAPABILITIES};

pub const SYNTAX_ID: &str = "markdown";
pub const SYNTAX_VERSION: u16 = 1;

/// Parse Markdown into DocIR with an empty metadata block.
pub fn parse_markdown(source: &str) -> Document {
    parse_markdown_with_meta(source, DocumentMeta::new())
}

/// Parse Markdown into DocIR with caller-supplied metadata.
pub fn parse_markdown_with_meta(source: &str, meta: DocumentMeta) -> Document {
    let lines = split_lines(source);
    let mut consumed = vec![false; lines.len()];
    let mut footnotes = Vec::new();

    collect_footnote_definitions(&lines, &mut consumed, &mut footnotes);

    let mut blocks = Vec::new();
    let mut index = 0usize;

    while index < lines.len() {
        if consumed[index] {
            index += 1;
            continue;
        }
        let line = &lines[index];
        let text = line.text.trim_start();
        if text.trim().is_empty() {
            index += 1;
            continue;
        }

        if let Some((lang, fence)) = fence_open(text) {
            index += 1;
            let mut value = String::new();
            while index < lines.len() {
                let candidate = lines[index].text.trim_start();
                if fence_close(candidate, fence) {
                    index += 1;
                    break;
                }
                if !value.is_empty() {
                    value.push('\n');
                }
                value.push_str(lines[index].text);
                index += 1;
            }
            let meta = node(&lines[index.saturating_sub(1)], 1);
            blocks.push(Block::CodeBlock(CodeBlock::new(meta, lang, value)));
            continue;
        }

        if let Some((level, rest)) = heading(text) {
            let offset = line.offset + (line.text.len() - text.len()) as u32 + level as u32 + 1;
            blocks.push(Block::Heading(Heading::new(
                node(line, line.text.len() as u32),
                level,
                parse_inlines(rest, offset),
            )));
            index += 1;
            continue;
        }

        if is_thematic_break(text) {
            blocks.push(Block::ThematicBreak(ThematicBreak::new(node(line, 3))));
            index += 1;
            continue;
        }

        if text.starts_with('>') {
            let mut parts: Vec<String> = Vec::new();
            let start = line.offset;
            let mut end = line.offset + line.text.len() as u32;
            while index < lines.len() {
                let candidate = lines[index].text.trim_start();
                if !candidate.starts_with('>') {
                    break;
                }
                parts.push(candidate.trim_start_matches('>').trim_start().into());
                end = lines[index].offset + lines[index].text.len() as u32;
                index += 1;
            }
            let joined = parts.join(" ");
            let paragraph = Paragraph::new(
                NodeMeta::at(SourceSpan::at(SourceId(1), start, end)),
                parse_inlines(&joined, start),
            );
            blocks.push(Block::BlockQuote(BlockQuote::new(
                NodeMeta::at(SourceSpan::at(SourceId(1), start, end)),
                vec![Block::Paragraph(paragraph)],
            )));
            continue;
        }

        if let Some(item) = list_marker(text) {
            let start = line.offset;
            let mut items: Vec<ListItem> = Vec::new();
            let mut ordered = item.ordered;
            let first_number = item.number;
            let mut end = start;

            while index < lines.len() {
                let candidate = lines[index].text.trim_start();
                if candidate.trim().is_empty() {
                    break;
                }
                let Some(marker) = list_marker(candidate) else {
                    break;
                };
                if marker.ordered != ordered {
                    break;
                }
                let item_offset = lines[index].offset
                    + (lines[index].text.len() - candidate.len()) as u32
                    + marker.content_offset as u32;
                let mut body = String::from(marker.content);
                end = lines[index].offset + lines[index].text.len() as u32;
                index += 1;
                while index < lines.len() {
                    let next = &lines[index];
                    let next_text = next.text.trim_start();
                    if next_text.trim().is_empty()
                        || list_marker(next_text).is_some()
                        || heading(next_text).is_some()
                        || is_thematic_break(next_text)
                        || fence_open(next_text).is_some()
                    {
                        break;
                    }
                    if !body.is_empty() {
                        body.push(' ');
                    }
                    body.push_str(next_text);
                    end = next.offset + next.text.len() as u32;
                    index += 1;
                }
                items.push(ListItem::new(
                    NodeMeta::at(SourceSpan::at(SourceId(1), item_offset, end)),
                    vec![Block::Paragraph(Paragraph::new(
                        NodeMeta::at(SourceSpan::at(SourceId(1), item_offset, end)),
                        parse_inlines(&body, item_offset),
                    ))],
                ));
                ordered = marker.ordered;
            }

            let mut list = List::new(
                NodeMeta::at(SourceSpan::at(SourceId(1), start, end)),
                ordered,
                if ordered { first_number } else { 1 },
                items,
            );
            list.tight = true;
            blocks.push(Block::List(list));
            continue;
        }

        // Paragraph: join lines until a blank line or another block start.
        let start = line.offset;
        let mut end = line.offset + line.text.len() as u32;
        let mut hard_break = ends_with_hard_break(text);
        let mut body = strip_hard_break(text);
        let body_start = line.offset + (line.text.len() - text.len()) as u32;
        index += 1;
        while index < lines.len() {
            let next = &lines[index];
            let next_text = next.text.trim_start();
            if next_text.trim().is_empty()
                || heading(next_text).is_some()
                || fence_open(next_text).is_some()
                || is_thematic_break(next_text)
                || list_marker(next_text).is_some()
                || next_text.starts_with('>')
            {
                break;
            }
            end = next.offset + next.text.len() as u32;
            let next_break = ends_with_hard_break(next_text);
            body.push(if hard_break { '\n' } else { ' ' });
            let trimmed = next_text.trim_end();
            let trimmed = if next_break {
                trimmed[..trimmed.len() - 1].trim_end()
            } else {
                trimmed
            };
            body.push_str(trimmed);
            hard_break = next_break;
            index += 1;
        }
        blocks.push(Block::Paragraph(Paragraph::new(
            NodeMeta::at(SourceSpan::at(SourceId(1), start, end)),
            parse_inlines(&body, body_start),
        )));
    }

    let mut source_map = SourceMap::new();
    source_map.push(SourceInfo::new(
        SourceId(1),
        "input.md",
        SourceKind::Markdown,
    ));

    let mut document = Document::new(meta);
    document.source_map = source_map;
    document.blocks = blocks;
    document.footnotes = footnotes;
    document.renumber();
    document
}

// ---------------------------------------------------------------------------
// Lines
// ---------------------------------------------------------------------------

struct Line<'a> {
    text: &'a str,
    offset: u32,
}

fn split_lines(source: &str) -> Vec<Line<'_>> {
    let mut lines = Vec::new();
    let mut offset = 0u32;
    for raw in source.split('\n') {
        let text = raw.strip_suffix('\r').unwrap_or(raw);
        lines.push(Line { text, offset });
        offset += raw.len() as u32 + 1;
    }
    lines
}

fn node(line: &Line<'_>, width: u32) -> NodeMeta {
    NodeMeta::at(SourceSpan::at(
        SourceId(1),
        line.offset,
        line.offset + width,
    ))
}

fn heading(text: &str) -> Option<(u8, &str)> {
    let hashes = text.chars().take_while(|c| *c == '#').count();
    if hashes == 0 || hashes > 6 {
        return None;
    }
    let rest = &text[hashes..];
    if rest.is_empty() {
        return Some((hashes as u8, ""));
    }
    if !rest.starts_with(' ') {
        return None;
    }
    Some((hashes as u8, rest.trim_start()))
}

fn is_thematic_break(text: &str) -> bool {
    let compact: String = text.chars().filter(|c| !c.is_whitespace()).collect();
    if compact.len() < 3 {
        return false;
    }
    let first = compact.chars().next().unwrap_or(' ');
    matches!(first, '-' | '*' | '_') && compact.chars().all(|c| c == first)
}

fn fence_open(text: &str) -> Option<(Option<String>, char)> {
    let marker = text.chars().next()?;
    if marker != '`' && marker != '~' {
        return None;
    }
    let count = text.chars().take_while(|c| *c == marker).count();
    if count < 3 {
        return None;
    }
    let info = text[count..].trim();
    Some((
        if info.is_empty() {
            None
        } else {
            Some(String::from(info.split_whitespace().next().unwrap_or(info)))
        },
        marker,
    ))
}

fn fence_close(text: &str, marker: char) -> bool {
    let count = text.chars().take_while(|c| *c == marker).count();
    count >= 3 && text[count..].trim().is_empty()
}

struct Marker<'a> {
    ordered: bool,
    number: u32,
    content_offset: usize,
    content: &'a str,
}

fn list_marker(text: &str) -> Option<Marker<'_>> {
    let bytes = text.as_bytes();
    if let Some(first) = bytes.first() {
        if matches!(first, b'-' | b'*' | b'+')
            && bytes.get(1).map(|b| b.is_ascii_whitespace()) == Some(true)
        {
            let content_offset = 2;
            return Some(Marker {
                ordered: false,
                number: 1,
                content_offset,
                content: text[content_offset..].trim_start(),
            });
        }
    }
    let digits = text.chars().take_while(|c| c.is_ascii_digit()).count();
    if digits == 0 || digits > 9 {
        return None;
    }
    let rest = &text[digits..];
    if !rest.starts_with('.') && !rest.starts_with(')') {
        return None;
    }
    if !rest[1..].starts_with(' ') {
        return None;
    }
    let number: u32 = text[..digits].parse().ok()?;
    Some(Marker {
        ordered: true,
        number,
        content_offset: digits + 2,
        content: rest[2..].trim_start(),
    })
}

fn ends_with_hard_break(text: &str) -> bool {
    text.ends_with("  ") || text.ends_with('\\')
}

// ---------------------------------------------------------------------------
// Footnotes
// ---------------------------------------------------------------------------

fn collect_footnote_definitions(
    lines: &[Line<'_>],
    consumed: &mut [bool],
    out: &mut Vec<FootnoteDef>,
) {
    let mut index = 0usize;
    while index < lines.len() {
        let text = lines[index].text.trim_start();
        if let Some((label, rest)) = footnote_definition(text) {
            let start = lines[index].offset;
            let mut body = String::from(rest);
            let mut end = start + lines[index].text.len() as u32;
            consumed[index] = true;
            index += 1;
            while index < lines.len() {
                let next = lines[index].text.trim_start();
                if next.trim().is_empty() {
                    break;
                }
                if footnote_definition(next).is_some() || heading(next).is_some() {
                    break;
                }
                body.push(' ');
                body.push_str(next);
                end = lines[index].offset + lines[index].text.len() as u32;
                consumed[index] = true;
                index += 1;
            }
            let paragraph = Paragraph::new(
                NodeMeta::at(SourceSpan::at(SourceId(1), start, end)),
                parse_inlines(&body, start),
            );
            out.push(FootnoteDef::new(
                NodeMeta::at(SourceSpan::at(SourceId(1), start, end)),
                label,
                vec![Block::Paragraph(paragraph)],
            ));
            continue;
        }
        index += 1;
    }
}

fn footnote_definition(text: &str) -> Option<(String, &str)> {
    let rest = text.strip_prefix("[^")?;
    let close = rest.find("]:")?;
    let label = &rest[..close];
    if label.is_empty() {
        return None;
    }
    Some((String::from(label), rest[close + 2..].trim_start()))
}

// ---------------------------------------------------------------------------
// Inlines
// ---------------------------------------------------------------------------

/// Parse inline Markdown. Unmatched delimiters are literal text.
pub fn parse_inlines(text: &str, offset: u32) -> Vec<Inline> {
    let mut out = Vec::new();
    let mut cursor = 0usize;
    let mut plain = String::new();
    let mut plain_start = offset;

    macro_rules! flush {
        () => {
            if !plain.is_empty() {
                out.push(
                    Text::new(
                        NodeMeta::at(SourceSpan::at(
                            SourceId(1),
                            plain_start,
                            plain_start + plain.len() as u32,
                        )),
                        plain.clone(),
                    )
                    .into(),
                );
                plain.clear();
            }
        };
    }

    while cursor < text.len() {
        let rest = &text[cursor..];

        if rest.starts_with('\\') && rest.len() >= 2 {
            let mut chars = rest.chars();
            chars.next();
            if let Some(escaped) = chars.next() {
                if plain.is_empty() {
                    plain_start = offset + cursor as u32;
                }
                plain.push(escaped);
                cursor += 1 + escaped.len_utf8();
                continue;
            }
        }

        if let Some(after) = rest.strip_prefix("[[index:") {
            if let Some(close) = after.find("]]") {
                let term = after[..close].trim();
                if !term.is_empty() {
                    let length = "[[index:".len() + close + 2;
                    flush!();
                    out.push(
                        IndexRef::new(
                            NodeMeta::at(SourceSpan::at(
                                SourceId(1),
                                offset + cursor as u32,
                                offset + cursor as u32 + length as u32,
                            )),
                            term,
                        )
                        .into(),
                    );
                    cursor += length;
                    continue;
                }
            }
        }

        if rest.starts_with("![") || rest.starts_with('[') {
            if let Some((inline, length)) = link_or_image(rest, offset + cursor as u32) {
                flush!();
                out.push(inline);
                cursor += length;
                continue;
            }
        }

        if let Some(after_tick) = rest.strip_prefix('`') {
            if let Some(close) = after_tick.find('`') {
                let value = &after_tick[..close];
                flush!();
                out.push(
                    Code::new(
                        NodeMeta::at(SourceSpan::at(
                            SourceId(1),
                            offset + cursor as u32,
                            offset + (cursor + close + 2) as u32,
                        )),
                        value,
                    )
                    .into(),
                );
                cursor += close + 2;
                continue;
            }
        }

        let mut matched = false;
        for (delimiter, strong) in [("**", true), ("__", true), ("*", false), ("_", false)] {
            if !rest.starts_with(delimiter) {
                continue;
            }
            if let Some(close) = rest[delimiter.len()..].find(delimiter) {
                let inner = &rest[delimiter.len()..delimiter.len() + close];
                if inner.is_empty() {
                    continue;
                }
                let start = offset + cursor as u32;
                let end = start + (delimiter.len() * 2 + inner.len()) as u32;
                let meta = NodeMeta::at(SourceSpan::at(SourceId(1), start, end));
                let children = parse_inlines(inner, start + delimiter.len() as u32);
                flush!();
                let container = Container::new(meta, children);
                out.push(if strong {
                    Inline::Strong(container)
                } else {
                    Inline::Emphasis(container)
                });
                cursor += delimiter.len() * 2 + close;
                matched = true;
                break;
            }
        }
        if matched {
            continue;
        }

        let character = rest.chars().next().expect("cursor is on a boundary");
        if character == '\n' {
            flush!();
            out.push(
                LineBreak::new(
                    NodeMeta::at(SourceSpan::at(
                        SourceId(1),
                        offset + cursor as u32,
                        offset + cursor as u32 + 1,
                    )),
                    LineBreakKind::Hard,
                )
                .into(),
            );
            cursor += 1;
            continue;
        }
        if plain.is_empty() {
            plain_start = offset + cursor as u32;
        }
        plain.push(character);
        cursor += character.len_utf8();
    }

    flush!();
    out
}

fn link_or_image(text: &str, offset: u32) -> Option<(Inline, usize)> {
    let is_image = text.starts_with("![");
    let open = if is_image { 2 } else { 1 };
    let body = &text[open..];

    // Footnote reference: `[^label]`.
    if !is_image {
        if let Some(rest) = body.strip_prefix('^') {
            if let Some(close) = rest.find(']') {
                let label = &rest[..close];
                if !label.is_empty() {
                    let length = open + 1 + close + 1;
                    return Some((
                        FootnoteRef::new(
                            NodeMeta::at(SourceSpan::at(
                                SourceId(1),
                                offset,
                                offset + length as u32,
                            )),
                            label,
                        )
                        .into(),
                        length,
                    ));
                }
            }
        }
    }

    let close = body.find(']')?;
    let label = &body[..close];
    let after = &body[close + 1..];
    if !after.starts_with('(') {
        return None;
    }
    let end = after.find(')')?;
    let target = after[1..end].trim();
    let (destination, title) = match target.split_once(' ') {
        Some((destination, title)) => (destination, Some(String::from(title.trim_matches('"')))),
        None => (target, None),
    };
    if destination.is_empty() {
        return None;
    }
    let length = open + close + 1 + end + 1;
    let meta = NodeMeta::at(SourceSpan::at(SourceId(1), offset, offset + length as u32));

    if is_image {
        let mut image = ImageRef::new(destination);
        if !label.is_empty() {
            image = image.with_alt(label);
        }
        if let Some(title) = title {
            image = image.with_title(title);
        }
        Some((Image::new(meta, image).into(), length))
    } else {
        let children = parse_inlines(label, offset + open as u32);
        let mut link = Link::new(meta, destination, children);
        link.title = title;
        Some((Inline::Link(link), length))
    }
}

fn strip_hard_break(text: &str) -> String {
    let trimmed = text.trim_end();
    match trimmed.strip_suffix('\\') {
        Some(stripped) => String::from(stripped.trim_end()),
        None => String::from(trimmed),
    }
}
