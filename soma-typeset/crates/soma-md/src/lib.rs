//! Markdown subset front end.
//!
//! The front end is deliberately small and forgiving: anything it does not
//! recognise becomes text, never a dropped node. It produces DocIR only; it
//! knows nothing about templates, styles, or pages.
//!
//! Supported: YAML front matter (title, subtitle, author, date, language,
//! keywords), ATX and setext headings, paragraphs (soft line joins), hard
//! breaks (two trailing spaces or a trailing backslash), emphasis/strong,
//! inline code, links, images, fenced code blocks, nested lists, block quotes,
//! GFM tables, thematic breaks, footnote definitions and references, ruby
//! (`[[ruby:base|reading]]`), index markers, and backslash escapes.
//!
//! Not supported: HTML blocks and inline HTML are kept as literal text (never
//! executed); an unterminated inline-code run stays literal text.

#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]

extern crate alloc;

pub mod capabilities;

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use soma_doc::block::{
    BlockQuote, Cell, CodeBlock, Column, Heading, List, ListItem, Paragraph, RawBlock, Row, Table,
    ThematicBreak,
};
use soma_doc::inline::{
    Container, FootnoteRef, Image, IndexRef, Inline, LineBreak, LineBreakKind, Link, RawInline,
    Ruby,
};
use soma_doc::meta::{Author, CoverData, Date, DocumentMeta, ImageRef};
use soma_doc::node::NodeMeta;
use soma_doc::raw::{RawCapability, RawContent};
use soma_doc::CellAlign;
use soma_doc::{
    Block, Code, Document, FootnoteDef, SourceId, SourceInfo, SourceKind, SourceMap, SourceSpan,
    Text,
};

pub use capabilities::{capability, with_status, Capability, CapabilityStatus, CAPABILITIES};

pub const SYNTAX_ID: &str = "markdown";
pub const SYNTAX_VERSION: u16 = 1;

/// Parse Markdown into DocIR. A leading `---` block supplies document
/// metadata (title, subtitle, author, date, language, keywords); without one
/// the metadata block stays empty.
pub fn parse_markdown(source: &str) -> Document {
    let (meta, body) = split_front_matter(source);
    parse_markdown_with_meta(body, meta)
}

/// Split a leading YAML front matter block off the body.
///
/// The subset is deliberately tiny: `key: value` lines with optional quotes.
/// Anything else is ignored rather than guessed at, and a malformed block is
/// treated as ordinary text.
fn split_front_matter(source: &str) -> (DocumentMeta, &str) {
    let mut meta = DocumentMeta::new();
    let Some(rest) = source.strip_prefix("---\n") else {
        return (meta, source);
    };
    let Some(end) = rest.find("\n---\n") else {
        return (meta, source);
    };
    let yaml = &rest[..end];
    let body = &rest[end + 5..];

    let mut cover = CoverData::new();
    for line in yaml.lines() {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let key = key.trim();
        let value = value.trim().trim_matches(|c| c == '"' || c == '\'');
        if value.is_empty() {
            continue;
        }
        match key {
            "title" => {
                meta.title = Some(vec![Inline::Text(Text::new(
                    NodeMeta::synthesized(),
                    value,
                ))]);
                cover.title = Some(String::from(value));
            }
            "subtitle" => cover.subtitle = Some(String::from(value)),
            "author" | "authors" => {
                for name in value.split(',') {
                    let name = name.trim();
                    if !name.is_empty() {
                        meta.authors.push(Author::new(name));
                        cover.authors.push(String::from(name));
                    }
                }
            }
            "date" => {
                if let Some(date) = parse_iso_date(value) {
                    meta.date = Some(date);
                    cover.date = Some(String::from(value));
                }
            }
            "language" | "lang" => meta.language = Some(String::from(value)),
            "keywords" => {
                meta.keywords = value
                    .split(',')
                    .map(|word| String::from(word.trim()))
                    .filter(|word| !word.is_empty())
                    .collect();
            }
            _ => {}
        }
    }
    if cover.title.is_some() || cover.subtitle.is_some() || !cover.authors.is_empty() {
        meta.cover = Some(cover);
    }
    (meta, body)
}

fn parse_iso_date(value: &str) -> Option<Date> {
    let mut parts = value.split('-');
    let year = parts.next()?.trim().parse().ok()?;
    let month = parts.next()?.trim().parse().ok()?;
    let day = parts.next()?.trim().parse().ok()?;
    Some(Date::new(year, month, day))
}

/// Parse Markdown into DocIR with caller-supplied metadata.
pub fn parse_markdown_with_meta(source: &str, meta: DocumentMeta) -> Document {
    let lines = split_lines(source);
    let mut consumed = vec![false; lines.len()];
    let mut footnotes = Vec::new();

    collect_footnote_definitions(&lines, &mut consumed, &mut footnotes);
    let link_refs = collect_link_definitions(&lines, &mut consumed);

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

        if html_block_start(text) {
            let start = line.offset;
            let mut value = String::new();
            let mut end = start;
            while index < lines.len() && !lines[index].text.trim().is_empty() {
                if !value.is_empty() {
                    value.push('\n');
                }
                value.push_str(lines[index].text);
                end = lines[index].offset + lines[index].text.len() as u32;
                index += 1;
            }
            let fallback = value.replace('\n', " ");
            blocks.push(Block::Raw(RawBlock::new(
                NodeMeta::at(SourceSpan::at(SourceId(1), start, end)),
                RawContent::new("html", value)
                    .with_fallback(fallback)
                    .with_capability(RawCapability::Degraded),
            )));
            continue;
        }

        // GFM table: a header row followed by a delimiter row.
        if text.contains('|') {
            if let Some((aligns, shares)) = lines
                .get(index + 1)
                .and_then(|next| table_delimiter(next.text.trim_start()))
            {
                let (table, next_index) = parse_table(&lines, index, &aligns, &shares);
                blocks.push(Block::Table(table));
                index = next_index;
                continue;
            }
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
            let indent = line.text.len() - text.len();
            let list = parse_list(&lines, &mut index, indent, item.ordered, item.number);
            blocks.push(Block::List(list));
            continue;
        }

        // Paragraph: join lines until a blank line or another block start. A
        // setext underline turns the joined lines into a heading.
        let start = line.offset;
        let mut end = line.offset + line.text.len() as u32;
        let mut hard_break = ends_with_hard_break(text);
        let mut body = strip_hard_break(text);
        let body_start = line.offset + (line.text.len() - text.len()) as u32;
        let mut setext: Option<u8> = None;
        index += 1;
        while index < lines.len() {
            let next = &lines[index];
            let next_text = next.text.trim_start();
            if let Some(level) = setext_underline(next_text) {
                setext = Some(level);
                end = next.offset + next.text.len() as u32;
                index += 1;
                break;
            }
            if next_text.trim().is_empty()
                || heading(next_text).is_some()
                || fence_open(next_text).is_some()
                || is_thematic_break(next_text)
                || list_marker(next_text).is_some()
                || next_text.starts_with('>')
                || table_delimiter(next_text).is_some()
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
        let inlines = parse_inlines(&body, body_start);
        let meta = NodeMeta::at(SourceSpan::at(SourceId(1), start, end));
        blocks.push(match setext {
            Some(level) => Block::Heading(Heading::new(meta, level, inlines)),
            None => Block::Paragraph(Paragraph::new(meta, inlines)),
        });
    }

    let mut source_map = SourceMap::new();
    source_map.push(SourceInfo::new(
        SourceId(1),
        "input.md",
        SourceKind::Markdown,
    ));

    let mut document = Document::new(meta);
    document.source_map = source_map;
    resolve_link_references(&mut blocks, &mut footnotes, &link_refs);
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

/// `===` or `---` under a paragraph makes it a heading (setext).
fn setext_underline(text: &str) -> Option<u8> {
    let trimmed = text.trim_end();
    let marker = trimmed.chars().next()?;
    if marker != '=' && marker != '-' {
        return None;
    }
    if trimmed.chars().any(|character| character != marker) {
        return None;
    }
    Some(if marker == '=' { 1 } else { 2 })
}

/// GFM table delimiter row: `| --- | :-: | ---: |`.
///
/// The number of dashes is the column's relative width, as in pandoc's pipe
/// tables: more dashes means a wider column. The counts are normalised to
/// per-mille so the layout's `Column.share` can carry them without physical
/// units.
fn table_delimiter(text: &str) -> Option<(Vec<CellAlign>, Vec<u16>)> {
    if !text.contains('-') {
        return None;
    }
    let cells = table_cells(text);
    if cells.is_empty() {
        return None;
    }
    let mut aligns = Vec::with_capacity(cells.len());
    let mut dashes = Vec::with_capacity(cells.len());
    for cell in cells {
        let cell = cell.trim();
        let left = cell.starts_with(':');
        let right = cell.ends_with(':');
        let body = cell.trim_start_matches(':').trim_end_matches(':');
        if body.len() < 3 || !body.chars().all(|character| character == '-') {
            return None;
        }
        aligns.push(match (left, right) {
            (true, true) => CellAlign::Center,
            (true, false) => CellAlign::Left,
            (false, true) => CellAlign::Right,
            (false, false) => CellAlign::Default,
        });
        dashes.push(body.chars().count() as u32);
    }
    Some((aligns, shares_from_dashes(&dashes)))
}

fn shares_from_dashes(dashes: &[u32]) -> Vec<u16> {
    // Equal dashes are the ordinary GFM spelling and carry no preference;
    // keeping them zero preserves the measured column widths. A differing
    // dash run is the author asking for a different relative width.
    if dashes.windows(2).all(|pair| pair[0] == pair[1]) {
        return vec![0; dashes.len()];
    }
    let total: u32 = dashes.iter().sum();
    if total == 0 {
        return vec![0; dashes.len()];
    }
    let mut shares: Vec<u16> = dashes
        .iter()
        .map(|dashes| (*dashes * 1000 / total) as u16)
        .collect();
    let assigned: u32 = shares.iter().map(|share| u32::from(*share)).sum();
    let mut left = 1000u32.saturating_sub(assigned);
    // Give rounding remainders to the largest dash counts; ties stay left to
    // right, so the result is deterministic.
    let mut order: Vec<usize> = (0..dashes.len()).collect();
    order.sort_by(|a, b| dashes[*b].cmp(&dashes[*a]).then(a.cmp(b)));
    for index in order {
        if left == 0 {
            break;
        }
        shares[index] += 1;
        left -= 1;
    }
    shares
}

/// Split one table row. A leading and trailing pipe are decoration; an escaped
/// `\|` is a literal bar.
fn table_cells(text: &str) -> Vec<String> {
    let mut trimmed = text.trim();
    if let Some(rest) = trimmed.strip_prefix('|') {
        trimmed = rest;
    }
    if let Some(rest) = trimmed.strip_suffix('|') {
        trimmed = rest;
    }
    let mut cells = Vec::new();
    let mut current = String::new();
    let mut characters = trimmed.chars();
    while let Some(character) = characters.next() {
        match character {
            '\\' => {
                if let Some(next) = characters.next() {
                    current.push(next);
                }
            }
            '|' => {
                cells.push(String::from(current.trim()));
                current.clear();
            }
            other => current.push(other),
        }
    }
    cells.push(String::from(current.trim()));
    cells
}

fn parse_table(
    lines: &[Line<'_>],
    index: usize,
    aligns: &[CellAlign],
    shares: &[u16],
) -> (Table, usize) {
    let header = &lines[index];
    let header_cells = table_cells(header.text.trim_start());
    let columns = aligns
        .iter()
        .enumerate()
        .map(|(column, align)| {
            Column::new(
                NodeMeta::at(SourceSpan::at(
                    SourceId(1),
                    header.offset + column as u32,
                    header.offset + column as u32 + 1,
                )),
                *align,
            )
            .with_share(shares.get(column).copied().unwrap_or(0))
        })
        .collect::<Vec<_>>();
    let mut rows = Vec::new();
    let start = header.offset;
    let mut end = header.offset + header.text.len() as u32;
    rows.push(table_row(header, &header_cells, aligns.len(), true));
    let mut next = index + 2;
    while next < lines.len() {
        let line = &lines[next];
        let text = line.text.trim_start();
        if text.trim().is_empty() || !text.contains('|') {
            break;
        }
        let cells = table_cells(text);
        rows.push(table_row(line, &cells, aligns.len(), false));
        end = line.offset + line.text.len() as u32;
        next += 1;
    }
    (
        Table::new(
            NodeMeta::at(SourceSpan::at(SourceId(1), start, end)),
            columns,
            rows,
        ),
        next,
    )
}

fn table_row(line: &Line<'_>, cells: &[String], columns: usize, header: bool) -> Row {
    let mut built = Vec::with_capacity(columns);
    for column in 0..columns {
        let text = cells.get(column).map(String::as_str).unwrap_or("");
        let blocks = if text.is_empty() {
            Vec::new()
        } else {
            vec![Block::Paragraph(Paragraph::new(
                NodeMeta::at(SourceSpan::at(
                    SourceId(1),
                    line.offset,
                    line.offset + line.text.len() as u32,
                )),
                parse_inlines(text, line.offset),
            ))]
        };
        built.push(Cell::new(
            NodeMeta::at(SourceSpan::at(
                SourceId(1),
                line.offset,
                line.offset + line.text.len() as u32,
            )),
            blocks,
        ));
    }
    Row::new(
        NodeMeta::at(SourceSpan::at(
            SourceId(1),
            line.offset,
            line.offset + line.text.len() as u32,
        )),
        header,
        built,
    )
}

/// Parse one list, recursing into items indented further than the list's own
/// marker column.
fn parse_list(
    lines: &[Line<'_>],
    index: &mut usize,
    indent: usize,
    ordered: bool,
    number: u32,
) -> List {
    let start = lines.get(*index).map(|line| line.offset).unwrap_or(0);
    let mut items: Vec<ListItem> = Vec::new();
    while *index < lines.len() {
        let line = &lines[*index];
        let text = line.text.trim_start();
        if text.trim().is_empty() {
            break;
        }
        let line_indent = line.text.len() - text.len();
        let Some(marker) = list_marker(text) else {
            break;
        };
        if line_indent > indent {
            // A deeper marker belongs to the item above it.
            if let Some(last) = items.last_mut() {
                let nested = parse_list(lines, index, line_indent, marker.ordered, marker.number);
                last.blocks.push(Block::List(nested));
                continue;
            }
        }
        if line_indent != indent || marker.ordered != ordered {
            break;
        }
        let item_offset =
            line.offset + (line.text.len() - text.len()) as u32 + marker.content_offset as u32;
        let mut body = String::from(marker.content);
        *index += 1;
        let mut nested: Vec<Block> = Vec::new();
        while *index < lines.len() {
            let next = &lines[*index];
            let next_text = next.text.trim_start();
            if next_text.trim().is_empty() {
                break;
            }
            let next_indent = next.text.len() - next_text.len();
            if next_indent <= indent {
                break;
            }
            if let Some(nested_marker) = list_marker(next_text) {
                nested.push(Block::List(parse_list(
                    lines,
                    index,
                    next_indent,
                    nested_marker.ordered,
                    nested_marker.number,
                )));
                continue;
            }
            if !body.is_empty() {
                body.push(' ');
            }
            body.push_str(next_text);
            *index += 1;
        }
        let end = lines
            .get(index.saturating_sub(1))
            .map(|line| line.offset + line.text.len() as u32)
            .unwrap_or(item_offset);
        let meta = NodeMeta::at(SourceSpan::at(SourceId(1), item_offset, end));
        let mut blocks = vec![Block::Paragraph(Paragraph::new(
            meta.clone(),
            parse_inlines(&body, item_offset),
        ))];
        blocks.extend(nested);
        items.push(ListItem::new(meta, blocks));
    }
    let end = lines
        .get(index.saturating_sub(1))
        .map(|line| line.offset + line.text.len() as u32)
        .unwrap_or(start);
    let mut list = List::new(
        NodeMeta::at(SourceSpan::at(SourceId(1), start, end)),
        ordered,
        if ordered { number } else { 1 },
        items,
    );
    list.tight = true;
    list
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
// Link reference definitions
// ---------------------------------------------------------------------------

type LinkRef = (String, String, Option<String>);

fn normalize_label(label: &str) -> String {
    let mut out = String::new();
    for word in label.split_whitespace() {
        if !out.is_empty() {
            out.push(' ');
        }
        for character in word.chars() {
            out.push(character.to_ascii_lowercase());
        }
    }
    out
}

fn collect_link_definitions(lines: &[Line<'_>], consumed: &mut [bool]) -> Vec<LinkRef> {
    let mut out = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        if consumed[index] {
            continue;
        }
        let text = line.text.trim_start();
        if let Some((label, destination, title)) = link_definition(text) {
            if !out.iter().any(|(existing, _, _)| *existing == label) {
                out.push((label, destination, title));
            }
            consumed[index] = true;
        }
    }
    out
}

fn link_definition(text: &str) -> Option<(String, String, Option<String>)> {
    if !text.starts_with('[') || text.starts_with("[^") {
        return None;
    }
    let rest = &text[1..];
    let close = rest.find("]:")?;
    let label = rest[..close].trim();
    if label.is_empty() {
        return None;
    }
    let after = rest[close + 2..].trim();
    if after.is_empty() {
        return None;
    }
    let (destination, tail) = if let Some(inner) = after.strip_prefix('<') {
        let end = inner.find('>')?;
        (&inner[..end], &inner[end + 1..])
    } else {
        let end = after.find(char::is_whitespace).unwrap_or(after.len());
        (&after[..end], &after[end..])
    };
    if destination.is_empty() {
        return None;
    }
    let tail = tail.trim();
    let title = if tail.len() >= 2
        && ((tail.starts_with('"') && tail.ends_with('"'))
            || (tail.starts_with('(') && tail.ends_with(')')))
    {
        Some(String::from(&tail[1..tail.len() - 1]))
    } else {
        None
    };
    Some((normalize_label(label), String::from(destination), title))
}

fn resolve_link_references(
    blocks: &mut Vec<Block>,
    footnotes: &mut [FootnoteDef],
    refs: &[LinkRef],
) {
    if refs.is_empty() {
        return;
    }
    resolve_blocks(blocks, refs);
    for footnote in footnotes {
        resolve_blocks(&mut footnote.blocks, refs);
    }
}

fn resolve_blocks(blocks: &mut Vec<Block>, refs: &[LinkRef]) {
    for block in blocks {
        match block {
            Block::Paragraph(paragraph) => {
                let inlines = core::mem::take(&mut paragraph.inlines);
                paragraph.inlines = resolve_inlines(inlines, refs);
            }
            Block::Heading(heading) => {
                let inlines = core::mem::take(&mut heading.inlines);
                heading.inlines = resolve_inlines(inlines, refs);
            }
            Block::List(list) => {
                for item in &mut list.items {
                    resolve_blocks(&mut item.blocks, refs);
                }
            }
            Block::BlockQuote(quote) => resolve_blocks(&mut quote.blocks, refs),
            Block::Table(table) => {
                let caption = core::mem::take(&mut table.caption);
                table.caption = resolve_inlines(caption, refs);
                for row in &mut table.rows {
                    for cell in &mut row.cells {
                        resolve_blocks(&mut cell.blocks, refs);
                    }
                }
            }
            _ => {}
        }
    }
}

fn resolve_inlines(inlines: Vec<Inline>, refs: &[LinkRef]) -> Vec<Inline> {
    let mut out = Vec::new();
    for inline in inlines {
        match inline {
            Inline::Text(text) => resolve_text(text, refs, &mut out),
            Inline::Strong(mut container) => {
                container.children = resolve_inlines(container.children, refs);
                out.push(Inline::Strong(container));
            }
            Inline::Emphasis(mut container) => {
                container.children = resolve_inlines(container.children, refs);
                out.push(Inline::Emphasis(container));
            }
            Inline::Underline(mut container) => {
                container.children = resolve_inlines(container.children, refs);
                out.push(Inline::Underline(container));
            }
            Inline::Strikethrough(mut container) => {
                container.children = resolve_inlines(container.children, refs);
                out.push(Inline::Strikethrough(container));
            }
            Inline::SmallCaps(mut container) => {
                container.children = resolve_inlines(container.children, refs);
                out.push(Inline::SmallCaps(container));
            }
            Inline::Superscript(mut container) => {
                container.children = resolve_inlines(container.children, refs);
                out.push(Inline::Superscript(container));
            }
            Inline::Subscript(mut container) => {
                container.children = resolve_inlines(container.children, refs);
                out.push(Inline::Subscript(container));
            }
            Inline::Link(mut link) => {
                link.children = resolve_inlines(link.children, refs);
                out.push(Inline::Link(link));
            }
            other => out.push(other),
        }
    }
    out
}

fn lookup_ref<'a>(label: &str, refs: &'a [LinkRef]) -> Option<&'a LinkRef> {
    let wanted = normalize_label(label);
    refs.iter().find(|(existing, _, _)| *existing == wanted)
}

fn resolve_text(text: Text, refs: &[LinkRef], out: &mut Vec<Inline>) {
    let value = text.value;
    let mut cursor = 0usize;
    let mut plain_start = 0usize;
    while cursor < value.len() {
        let rest = &value[cursor..];
        let image = rest.starts_with("![");
        if !image && !rest.starts_with('[') {
            cursor += rest.chars().next().map(char::len_utf8).unwrap_or(1);
            continue;
        }
        let open = if image { 2 } else { 1 };
        let Some(close) = rest[open..].find(']') else {
            cursor += open;
            continue;
        };
        let label_start = open;
        let label_end = open + close;
        let after = &rest[label_end + 1..];
        if !after.starts_with('[') {
            cursor += label_end + 1;
            continue;
        }
        let Some(end) = after.find(']') else {
            cursor += label_end + 1;
            continue;
        };
        let reference = &after[1..end];
        let label = if reference.trim().is_empty() {
            &rest[label_start..label_end]
        } else {
            reference
        };
        let Some((_, destination, title)) = lookup_ref(label, refs) else {
            cursor += label_end + 1;
            continue;
        };
        if cursor > plain_start {
            out.push(
                Text::new(text.meta.clone(), String::from(&value[plain_start..cursor])).into(),
            );
        }
        let consumed = label_end + 2 + end;
        if image {
            let mut source = ImageRef::new(destination.clone());
            source.alt = Some(String::from(&rest[label_start..label_end]));
            source.title = title.clone();
            out.push(Inline::Image(Image::new(text.meta.clone(), source)));
        } else {
            let children = parse_inlines(
                &rest[label_start..label_end],
                text.meta.span.start + cursor as u32 + label_start as u32,
            );
            let mut link = Link::new(text.meta.clone(), destination.clone(), children);
            link.title = title.clone();
            out.push(Inline::Link(link));
        }
        cursor += consumed;
        plain_start = cursor;
    }
    if plain_start < value.len() {
        out.push(Text::new(text.meta, String::from(&value[plain_start..])).into());
    }
}

// ---------------------------------------------------------------------------
// Inlines
// ---------------------------------------------------------------------------

/// Parse inline Markdown. Unmatched delimiters are literal text.
/// What an inline delimiter run produces.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Mark {
    Strong,
    Emphasis,
    Strikethrough,
}

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

        if rest.starts_with('<') {
            if let Some(length) = html_tag_end(rest) {
                flush!();
                let tag = &rest[..length];
                out.push(
                    RawInline::new(
                        NodeMeta::at(SourceSpan::at(
                            SourceId(1),
                            offset + cursor as u32,
                            offset + cursor as u32 + length as u32,
                        )),
                        RawContent::new("html", tag)
                            .with_fallback(tag)
                            .with_capability(RawCapability::Degraded),
                    )
                    .into(),
                );
                cursor += length;
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

        if let Some(after) = rest.strip_prefix("[[ruby:") {
            if let Some(close) = after.find("]]") {
                let inner = &after[..close];
                if let Some(bar) = inner.find('|') {
                    let base = inner[..bar].trim();
                    let annotation = inner[bar + 1..].trim();
                    if !base.is_empty() && !annotation.is_empty() {
                        let length = "[[ruby:".len() + close + 2;
                        flush!();
                        out.push(
                            Ruby::new(
                                NodeMeta::at(SourceSpan::at(
                                    SourceId(1),
                                    offset + cursor as u32,
                                    offset + cursor as u32 + length as u32,
                                )),
                                base,
                                annotation,
                            )
                            .into(),
                        );
                        cursor += length;
                        continue;
                    }
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
        // GFM strikes through with `~~`; the delimiters are tried longest
        // first so `**` never wins over a single `*`.
        for (delimiter, role) in [
            ("**", Mark::Strong),
            ("__", Mark::Strong),
            ("~~", Mark::Strikethrough),
            ("*", Mark::Emphasis),
            ("_", Mark::Emphasis),
        ] {
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
                out.push(match role {
                    Mark::Strong => Inline::Strong(container),
                    Mark::Emphasis => Inline::Emphasis(container),
                    Mark::Strikethrough => Inline::Strikethrough(container),
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

fn html_block_start(text: &str) -> bool {
    let text = text.trim_start();
    if text.starts_with("<!--") || text.starts_with("<?") || text.starts_with("<!") {
        return true;
    }
    let Some(rest) = text.strip_prefix('<') else {
        return false;
    };
    let rest = rest.strip_prefix('/').unwrap_or(rest);
    let name: String = rest
        .chars()
        .take_while(|character| character.is_ascii_alphanumeric())
        .collect();
    if name.is_empty() || rest[name.len()..].starts_with(':') {
        return false;
    }
    matches!(
        name.to_ascii_lowercase().as_str(),
        "address"
            | "article"
            | "aside"
            | "base"
            | "basefont"
            | "blockquote"
            | "body"
            | "caption"
            | "center"
            | "col"
            | "colgroup"
            | "dd"
            | "details"
            | "dialog"
            | "dir"
            | "div"
            | "dl"
            | "dt"
            | "fieldset"
            | "figcaption"
            | "figure"
            | "footer"
            | "form"
            | "frame"
            | "frameset"
            | "h1"
            | "h2"
            | "h3"
            | "h4"
            | "h5"
            | "h6"
            | "head"
            | "header"
            | "hr"
            | "html"
            | "iframe"
            | "legend"
            | "li"
            | "link"
            | "main"
            | "menu"
            | "menuitem"
            | "nav"
            | "noframes"
            | "ol"
            | "optgroup"
            | "option"
            | "p"
            | "param"
            | "section"
            | "source"
            | "summary"
            | "table"
            | "tbody"
            | "td"
            | "tfoot"
            | "th"
            | "thead"
            | "title"
            | "tr"
            | "track"
            | "ul"
    )
}

fn html_tag_end(text: &str) -> Option<usize> {
    let rest = text.strip_prefix('<')?;
    if rest.starts_with('!') || rest.starts_with('?') {
        return rest.find('>').map(|index| index + 2);
    }
    let (prefix, rest) = match rest.strip_prefix('/') {
        Some(rest) => (1, rest),
        None => (0, rest),
    };
    let _ = prefix;
    let name_len = rest
        .chars()
        .take_while(|character| character.is_ascii_alphanumeric())
        .count();
    if name_len == 0 {
        return None;
    }
    let next = rest[name_len..].chars().next()?;
    if !(next == '>' || next == '/' || next.is_ascii_whitespace()) {
        return None;
    }
    rest.find('>').map(|index| index + 2)
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

#[cfg(test)]
mod unit_tests {
    use super::*;

    #[test]
    fn delimiter_is_recognised() {
        assert_eq!(
            table_cells("| :--- | ---: |"),
            vec![String::from(":---"), String::from("---:")]
        );
        assert_eq!(
            table_delimiter("| :--- | ---: |"),
            Some((vec![CellAlign::Left, CellAlign::Right], vec![0, 0]))
        );
        assert_eq!(
            table_delimiter("| --- | -------- |"),
            Some((vec![CellAlign::Default, CellAlign::Default], vec![272, 728])),
            "dash counts become relative column shares"
        );
    }
}
