//! HTML subset front end.
//!
//! The front end is a **semantic whitelist**: it maps structural HTML to DocIR
//! and never carries presentation (CSS, inline styles, scripts) into the
//! document. Anything it does not recognise keeps its text and is otherwise
//! ignored; it never drops content silently.
//!
//! Supported: `h1`–`h6`, `p`, `strong`/`b`, `em`/`i`, `u`, `s`/`del`/`strike`,
//! `code`/`kbd`/`samp`/`var`, `a`, `img`, `br`, `sup`, `sub`, `ruby`/`rt`,
//! `ul`/`ol`/`li`, `blockquote`, `pre`, `hr`, `table`/`thead`/`tbody`/`tr`/
//! `th`/`td` (with `colspan`/`rowspan`), `figure`/`figcaption`, and the
//! transparent containers `div`, `section`, `article`, `main`, `header`,
//! `footer`, `nav`, `aside`, `body`, `html`.
//!
//! Not supported: CSS layout, scripts, frames, forms, footnotes, and HTML
//! entities beyond the common named set plus numeric references.
//!
//! The parser is lossy-but-safe: unclosed tags, stray closing tags and
//! malformed attributes are recovered instead of raising an error.

#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]

extern crate alloc;

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

use soma_doc::block::{
    BlockQuote, Cell, CodeBlock, Column, Figure, Heading, List, ListItem, Paragraph, Row, Table,
    ThematicBreak,
};
use soma_doc::inline::{Container, Image, Inline, LineBreak, LineBreakKind, Link, Ruby};
use soma_doc::meta::{DocumentMeta, ImageRef};
use soma_doc::node::NodeMeta;
use soma_doc::Attrs;
use soma_doc::{Block, Code, Document, Text};

pub const SYNTAX_ID: &str = "html";
pub const SYNTAX_VERSION: u16 = 1;

/// Tags whose text content is preserved by the parser.
pub const SUPPORTED_TAGS: &[&str] = &[
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "p",
    "strong",
    "b",
    "em",
    "i",
    "u",
    "s",
    "del",
    "strike",
    "code",
    "kbd",
    "samp",
    "var",
    "a",
    "img",
    "br",
    "sup",
    "sub",
    "ruby",
    "rt",
    "ul",
    "ol",
    "li",
    "blockquote",
    "pre",
    "hr",
    "table",
    "thead",
    "tbody",
    "tr",
    "th",
    "td",
    "figure",
    "figcaption",
    "div",
    "section",
    "article",
    "main",
    "header",
    "footer",
    "nav",
    "aside",
    "body",
    "html",
];

const VOID_TAGS: &[&str] = &[
    "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "source", "track",
    "wbr",
];

const BLOCK_TAGS: &[&str] = &[
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "p",
    "ul",
    "ol",
    "li",
    "blockquote",
    "pre",
    "hr",
    "table",
    "thead",
    "tbody",
    "tr",
    "th",
    "td",
    "figure",
    "figcaption",
    "div",
    "section",
    "article",
    "main",
    "header",
    "footer",
    "nav",
    "aside",
    "dl",
    "dt",
    "dd",
];

const TRANSPARENT_TAGS: &[&str] = &[
    "div",
    "section",
    "article",
    "main",
    "header",
    "footer",
    "nav",
    "aside",
    "body",
    "html",
    "dl",
    "dt",
    "dd",
    "figcaption",
];

#[derive(Clone, Debug, PartialEq, Eq)]
enum Node {
    Text(String),
    Element(Element),
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Element {
    tag: String,
    attrs: Vec<(String, String)>,
    children: Vec<Node>,
}

impl Element {
    fn attr(&self, name: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }

    fn class_names(&self) -> impl Iterator<Item = &str> {
        self.attr("class").unwrap_or("").split_whitespace()
    }
}

/// Parse HTML into DocIR with an empty metadata block.
pub fn parse_html(source: &str) -> Document {
    parse_html_with_meta(source, DocumentMeta::new())
}

/// Parse HTML into DocIR with caller-supplied metadata. `lang` on `<html>`
/// fills `language` when the caller did not set one.
pub fn parse_html_with_meta(source: &str, meta: DocumentMeta) -> Document {
    let nodes = parse_tree(source);
    let mut meta = meta;
    if meta.language.is_none() {
        if let Some(lang) = root_lang(&nodes) {
            meta.language = Some(lang);
        }
    }
    let mut blocks = Vec::new();
    build_blocks(&nodes, &mut blocks);
    let mut document = Document::new(meta);
    document.blocks = blocks;
    document.renumber();
    document
}

fn root_lang(nodes: &[Node]) -> Option<String> {
    for node in nodes {
        if let Node::Element(element) = node {
            if let Some(lang) = element.attr("lang") {
                return Some(lang.to_string());
            }
            if let Some(lang) = root_lang(&element.children) {
                return Some(lang);
            }
        }
    }
    None
}

// ---------------------------------------------------------------- tokenizer

fn parse_tree(source: &str) -> Vec<Node> {
    let bytes = source.as_bytes();
    let mut pos = 0usize;
    let mut root: Vec<Node> = Vec::new();
    let mut stack: Vec<Element> = Vec::new();

    while pos < bytes.len() {
        if bytes[pos] != b'<' {
            let start = pos;
            while pos < bytes.len() && bytes[pos] != b'<' {
                pos += 1;
            }
            let text = decode_entities(&source[start..pos]);
            if !text.is_empty() {
                push_node(&mut root, &mut stack, Node::Text(text));
            }
            continue;
        }

        if source[pos..].starts_with("<!--") {
            pos = source[pos..]
                .find("-->")
                .map(|offset| pos + offset + 3)
                .unwrap_or(bytes.len());
            continue;
        }
        if source[pos..].starts_with("<!") || source[pos..].starts_with("<?") {
            pos = source[pos..]
                .find('>')
                .map(|offset| pos + offset + 1)
                .unwrap_or(bytes.len());
            continue;
        }
        if source[pos..].starts_with("</") {
            let close = source[pos + 2..]
                .find('>')
                .map(|offset| pos + 2 + offset)
                .unwrap_or(bytes.len());
            let name = source[pos + 2..close]
                .split_whitespace()
                .next()
                .unwrap_or("")
                .to_ascii_lowercase();
            pos = (close + 1).min(bytes.len());
            close_element(&mut root, &mut stack, &name);
            continue;
        }

        match parse_start_tag(source, pos) {
            Some(start) => {
                pos = start.end;
                if start.tag == "script" || start.tag == "style" {
                    pos = skip_raw_text(source, pos, &start.tag);
                    continue;
                }
                while stack
                    .last()
                    .map(|open| closes(open.tag.as_str(), &start.tag))
                    .unwrap_or(false)
                {
                    let element = stack.pop().expect("checked above");
                    push_node(&mut root, &mut stack, Node::Element(element));
                }
                let element = Element {
                    tag: start.tag,
                    attrs: start.attrs,
                    children: Vec::new(),
                };
                if start.self_closing || VOID_TAGS.contains(&element.tag.as_str()) {
                    push_node(&mut root, &mut stack, Node::Element(element));
                } else {
                    stack.push(element);
                }
            }
            None => {
                // A bare `<` that is not a tag is text.
                push_node(&mut root, &mut stack, Node::Text(String::from("<")));
                pos += 1;
            }
        }
    }

    while let Some(element) = stack.pop() {
        push_node(&mut root, &mut stack, Node::Element(element));
    }
    root
}

fn push_node(root: &mut Vec<Node>, stack: &mut [Element], node: Node) {
    match stack.last_mut() {
        Some(top) => top.children.push(node),
        None => root.push(node),
    }
}

fn close_element(root: &mut Vec<Node>, stack: &mut Vec<Element>, name: &str) {
    let Some(position) = stack.iter().rposition(|element| element.tag == name) else {
        return;
    };
    while stack.len() > position {
        let element = stack.pop().expect("non-empty");
        push_node(root, stack, Node::Element(element));
    }
}

fn closes(open: &str, new: &str) -> bool {
    (open == "p" && BLOCK_TAGS.contains(&new))
        || (open == "li" && new == "li")
        || ((open == "td" || open == "th") && (new == "td" || new == "th" || new == "tr"))
        || (open == "tr" && new == "tr")
        || (open == "dt" && new == "dd")
        || (open == "dd" && new == "dt")
}

fn skip_raw_text(source: &str, mut pos: usize, tag: &str) -> usize {
    let close = alloc::format!("</{tag}");
    let lower = source[pos..].to_ascii_lowercase();
    match lower.find(&close) {
        Some(offset) => {
            pos += offset;
            match source[pos..].find('>') {
                Some(offset) => pos + offset + 1,
                None => source.len(),
            }
        }
        None => source.len(),
    }
}

struct StartTag {
    tag: String,
    attrs: Vec<(String, String)>,
    self_closing: bool,
    end: usize,
}

fn parse_start_tag(source: &str, pos: usize) -> Option<StartTag> {
    let bytes = source.as_bytes();
    let mut index = pos + 1;
    let start = index;
    while index < bytes.len() && is_name_byte(bytes[index]) {
        index += 1;
    }
    if index == start {
        return None;
    }
    let tag = source[start..index].to_ascii_lowercase();
    let mut attrs = Vec::new();
    loop {
        while index < bytes.len() && bytes[index].is_ascii_whitespace() {
            index += 1;
        }
        if index >= bytes.len() {
            return None;
        }
        match bytes[index] {
            b'>' => {
                return Some(StartTag {
                    tag,
                    attrs,
                    self_closing: false,
                    end: index + 1,
                })
            }
            b'/' => {
                if index + 1 < bytes.len() && bytes[index + 1] == b'>' {
                    return Some(StartTag {
                        tag,
                        attrs,
                        self_closing: true,
                        end: index + 2,
                    });
                }
                index += 1;
            }
            _ => {
                let name_start = index;
                while index < bytes.len()
                    && !bytes[index].is_ascii_whitespace()
                    && bytes[index] != b'='
                    && bytes[index] != b'>'
                    && bytes[index] != b'/'
                {
                    index += 1;
                }
                if index == name_start {
                    index += 1;
                    continue;
                }
                let name = source[name_start..index].to_ascii_lowercase();
                while index < bytes.len() && bytes[index].is_ascii_whitespace() {
                    index += 1;
                }
                let mut value = String::new();
                if index < bytes.len() && bytes[index] == b'=' {
                    index += 1;
                    while index < bytes.len() && bytes[index].is_ascii_whitespace() {
                        index += 1;
                    }
                    if index < bytes.len() && (bytes[index] == b'"' || bytes[index] == b'\'') {
                        let quote = bytes[index];
                        index += 1;
                        let value_start = index;
                        while index < bytes.len() && bytes[index] != quote {
                            index += 1;
                        }
                        value = decode_entities(&source[value_start..index.min(bytes.len())]);
                        if index < bytes.len() {
                            index += 1;
                        }
                    } else {
                        let value_start = index;
                        while index < bytes.len()
                            && !bytes[index].is_ascii_whitespace()
                            && bytes[index] != b'>'
                        {
                            index += 1;
                        }
                        value = decode_entities(&source[value_start..index]);
                    }
                }
                attrs.push((name, value));
            }
        }
    }
}

fn is_name_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'-' || byte == b':' || byte == b'_'
}

fn decode_entities(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(offset) = rest.find('&') {
        out.push_str(&rest[..offset]);
        rest = &rest[offset..];
        let Some(end) = rest[..rest.len().min(34)].find(';') else {
            out.push('&');
            rest = &rest[1..];
            continue;
        };
        let entity = &rest[1..end];
        let decoded = match entity {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            "nbsp" => Some(' '),
            "copy" => Some('©'),
            "hellip" => Some('…'),
            "mdash" => Some('—'),
            "ndash" => Some('–'),
            "ldquo" => Some('“'),
            "rdquo" => Some('”'),
            "lsquo" => Some('‘'),
            "rsquo" => Some('’'),
            "middot" => Some('·'),
            _ => entity
                .strip_prefix("#x")
                .or_else(|| entity.strip_prefix("#X"))
                .and_then(|hex| u32::from_str_radix(hex, 16).ok())
                .or_else(|| {
                    entity
                        .strip_prefix('#')
                        .and_then(|dec| dec.parse::<u32>().ok())
                })
                .and_then(char::from_u32),
        };
        match decoded {
            Some(character) => {
                out.push(character);
                rest = &rest[end + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

// ------------------------------------------------------------- block builder

fn build_blocks(nodes: &[Node], out: &mut Vec<Block>) {
    let mut pending: Vec<Inline> = Vec::new();
    for node in nodes {
        match node {
            Node::Text(text) => {
                let value = collapse(text);
                if !value.is_empty() {
                    pending.push(Inline::Text(Text::new(NodeMeta::synthesized(), value)));
                }
            }
            Node::Element(element) if is_inline(element.tag.as_str()) => {
                match build_inline(element) {
                    Some(inline) => pending.push(inline),
                    None => pending.extend(build_inlines(&element.children)),
                }
            }
            Node::Element(element) => {
                flush(&mut pending, out);
                build_block(element, out);
            }
        }
    }
    flush(&mut pending, out);
}

fn flush(pending: &mut Vec<Inline>, out: &mut Vec<Block>) {
    if pending.is_empty() {
        return;
    }
    let mut inlines = core::mem::take(pending);
    trim_inlines(&mut inlines);
    if inlines.is_empty()
        || inlines
            .iter()
            .all(|inline| matches!(inline, Inline::Text(text) if text.value.trim().is_empty()))
    {
        return;
    }
    out.push(Block::Paragraph(Paragraph::new(
        NodeMeta::synthesized(),
        inlines,
    )));
}

fn build_block(element: &Element, out: &mut Vec<Block>) {
    let meta = element_meta(element);
    match element.tag.as_str() {
        "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
            let level = element.tag.as_bytes()[1] - b'0';
            let content = build_inlines(&element.children);
            if !content.is_empty() {
                out.push(Block::Heading(Heading::new(meta, level, content)));
            }
        }
        "p" => {
            let content = build_inlines(&element.children);
            if !content.is_empty() {
                out.push(Block::Paragraph(Paragraph::new(meta, content)));
            }
        }
        "ul" | "ol" => {
            let ordered = element.tag == "ol";
            let start = element
                .attr("start")
                .and_then(|value| value.parse::<u32>().ok())
                .unwrap_or(1);
            let mut items = Vec::new();
            for child in &element.children {
                if let Node::Element(item) = child {
                    if item.tag == "li" {
                        let mut blocks = Vec::new();
                        build_blocks(&item.children, &mut blocks);
                        items.push(ListItem::new(element_meta(item), blocks));
                    }
                }
            }
            if !items.is_empty() {
                out.push(Block::List(List::new(meta, ordered, start, items)));
            }
        }
        "blockquote" => {
            let mut blocks = Vec::new();
            build_blocks(&element.children, &mut blocks);
            if !blocks.is_empty() {
                out.push(Block::BlockQuote(BlockQuote::new(meta, blocks)));
            }
        }
        "pre" => {
            let value = raw_text(&element.children);
            let value = value.strip_suffix('\n').unwrap_or(&value);
            let lang = element
                .class_names()
                .find_map(|class| class.strip_prefix("language-"))
                .map(|lang| lang.to_ascii_lowercase());
            out.push(Block::CodeBlock(CodeBlock::new(meta, lang, value)));
        }
        "hr" => out.push(Block::ThematicBreak(ThematicBreak::new(meta))),
        "table" => {
            if let Some(table) = build_table(element, meta) {
                out.push(Block::Table(table));
            }
        }
        "figure" => {
            if let Some(figure) = build_figure(element, meta) {
                out.push(Block::Figure(figure));
            } else {
                build_blocks(&element.children, out);
            }
        }
        _ => {
            // Unknown or transparent containers keep their content.
            build_blocks(&element.children, out);
        }
    }
}

fn build_figure(element: &Element, meta: NodeMeta) -> Option<Figure> {
    let mut image: Option<Image> = None;
    let mut caption: Vec<Inline> = Vec::new();
    for child in &element.children {
        match child {
            Node::Element(candidate) if candidate.tag == "img" && image.is_none() => {
                image = build_image(candidate);
            }
            Node::Element(candidate) if candidate.tag == "figcaption" => {
                caption = build_inlines(&candidate.children);
            }
            _ => {}
        }
    }
    let image = image?;
    let mut figure = Figure::new(meta, image);
    figure.caption = caption;
    Some(figure)
}

fn build_table(element: &Element, meta: NodeMeta) -> Option<Table> {
    let mut raw_rows: Vec<(Vec<Element>, bool)> = Vec::new();
    collect_rows(element, false, &mut raw_rows);
    if raw_rows.is_empty() {
        return None;
    }
    let row_count = raw_rows.len();
    let columns = column_count(&raw_rows, row_count);
    if columns == 0 {
        return None;
    }

    let mut occupied = vec![vec![false; columns]; row_count];
    let mut rows: Vec<Row> = Vec::with_capacity(row_count);
    let mut header_run = true;
    for (row_index, (cells, in_thead)) in raw_rows.iter().enumerate() {
        let mut column = 0usize;
        let mut placed: Vec<Cell> = Vec::new();
        let mut all_th = !cells.is_empty();
        for cell_element in cells {
            while column < columns && occupied[row_index][column] {
                column += 1;
            }
            if column >= columns {
                break;
            }
            let colspan = span(cell_element, "colspan", columns - column);
            let rowspan = span(cell_element, "rowspan", row_count - row_index);
            for occupied_row in occupied
                .iter_mut()
                .take(row_index + rowspan)
                .skip(row_index)
            {
                for slot in occupied_row.iter_mut().take(column + colspan).skip(column) {
                    *slot = true;
                }
            }
            let mut blocks = Vec::new();
            build_blocks(&cell_element.children, &mut blocks);
            let mut cell = Cell::new(element_meta(cell_element), blocks);
            cell.colspan = colspan as u16;
            cell.rowspan = rowspan as u16;
            placed.push(cell);
            if cell_element.tag != "th" {
                all_th = false;
            }
            column += colspan;
        }
        // Fill the remaining free columns so the DocIR grid stays valid.
        while column < columns {
            if occupied[row_index][column] {
                column += 1;
                continue;
            }
            occupied[row_index][column] = true;
            placed.push(Cell::new(NodeMeta::synthesized(), Vec::new()));
            column += 1;
        }
        let header = *in_thead || all_th;
        if header_run && header {
            // keep header true
        } else {
            header_run = false;
        }
        rows.push(Row::new(NodeMeta::synthesized(), header_run, placed));
    }

    let columns = (0..columns)
        .map(|_| Column::new(NodeMeta::synthesized(), soma_doc::CellAlign::Default))
        .collect();
    Some(Table::new(meta, columns, rows))
}

fn collect_rows(element: &Element, in_thead: bool, out: &mut Vec<(Vec<Element>, bool)>) {
    for child in &element.children {
        let Node::Element(candidate) = child else {
            continue;
        };
        match candidate.tag.as_str() {
            "tr" => {
                let cells = candidate
                    .children
                    .iter()
                    .filter_map(|node| match node {
                        Node::Element(cell) if cell.tag == "td" || cell.tag == "th" => {
                            Some(cell.clone())
                        }
                        _ => None,
                    })
                    .collect();
                out.push((cells, in_thead));
            }
            "thead" => collect_rows(candidate, true, out),
            "tbody" | "tfoot" => collect_rows(candidate, in_thead, out),
            _ => {}
        }
    }
}

fn column_count(rows: &[(Vec<Element>, bool)], row_count: usize) -> usize {
    const LIMIT: usize = 64;
    let mut occupied = vec![vec![false; LIMIT]; row_count];
    let mut used = 0usize;
    for (row_index, (cells, _)) in rows.iter().enumerate() {
        let mut column = 0usize;
        for cell in cells {
            while column < LIMIT && occupied[row_index][column] {
                column += 1;
            }
            if column >= LIMIT {
                break;
            }
            let colspan = span(cell, "colspan", LIMIT - column);
            let rowspan = span(cell, "rowspan", row_count - row_index);
            for occupied_row in occupied
                .iter_mut()
                .take(row_index + rowspan)
                .skip(row_index)
            {
                for slot in occupied_row.iter_mut().take(column + colspan).skip(column) {
                    *slot = true;
                }
            }
            column += colspan;
            used = used.max(column);
        }
    }
    used
}

fn span(element: &Element, name: &str, limit: usize) -> usize {
    element
        .attr(name)
        .and_then(|value| value.trim().parse::<usize>().ok())
        .map(|value| value.max(1).min(limit.max(1)))
        .unwrap_or(1)
}

// ------------------------------------------------------------ inline builder

fn is_inline(tag: &str) -> bool {
    !BLOCK_TAGS.contains(&tag) && !TRANSPARENT_TAGS.contains(&tag)
}

fn build_inlines(nodes: &[Node]) -> Vec<Inline> {
    let mut out = Vec::new();
    for node in nodes {
        match node {
            Node::Text(text) => {
                let value = collapse(text);
                if !value.is_empty() {
                    out.push(Inline::Text(Text::new(NodeMeta::synthesized(), value)));
                }
            }
            Node::Element(element) => match build_inline(element) {
                Some(inline) => out.push(inline),
                // Unknown or empty inline elements are transparent.
                None => out.extend(build_inlines(&element.children)),
            },
        }
    }
    trim_inlines(&mut out);
    out
}

fn build_inline(element: &Element) -> Option<Inline> {
    let meta = element_meta(element);
    let container = |tag_meta: NodeMeta| {
        let children = build_inlines(&element.children);
        (!children.is_empty())
            .then_some(children)
            .map(|children| Container::new(tag_meta, children))
    };
    let value = match element.tag.as_str() {
        "strong" | "b" => container(meta).map(Inline::Strong),
        "em" | "i" => container(meta).map(Inline::Emphasis),
        "u" => container(meta).map(Inline::Underline),
        "s" | "del" | "strike" => container(meta).map(Inline::Strikethrough),
        "sup" => container(meta).map(Inline::Superscript),
        "sub" => container(meta).map(Inline::Subscript),
        "code" | "kbd" | "samp" | "var" => {
            let text = raw_text(&element.children);
            let text = collapse(&text);
            (!text.is_empty()).then_some(Inline::Code(Code::new(meta, text)))
        }
        "a" => {
            let href = element.attr("href").unwrap_or("");
            let children = build_inlines(&element.children);
            (!children.is_empty() && !href.is_empty())
                .then_some(Inline::Link(Link::new(meta, href, children)))
        }
        "img" => build_image(element).map(Inline::Image),
        "br" => Some(Inline::LineBreak(LineBreak::new(meta, LineBreakKind::Hard))),
        "ruby" => {
            let base = raw_text(
                &element
                    .children
                    .iter()
                    .filter(|node| !matches!(node, Node::Element(element) if element.tag == "rt"))
                    .cloned()
                    .collect::<Vec<_>>(),
            );
            let annotation = element
                .children
                .iter()
                .find_map(|node| match node {
                    Node::Element(element) if element.tag == "rt" => {
                        Some(raw_text(&element.children))
                    }
                    _ => None,
                })
                .unwrap_or_default();
            (!base.is_empty()).then_some(Inline::Ruby(Ruby::new(meta, base, annotation)))
        }
        // Unknown inline tags are transparent; the caller splices children.
        _ => None,
    };
    value
}

fn build_image(element: &Element) -> Option<Image> {
    let src = element.attr("src").unwrap_or("");
    if src.is_empty() {
        return None;
    }
    let mut reference = ImageRef::new(src);
    if let Some(alt) = element.attr("alt") {
        reference = reference.with_alt(alt);
    }
    Some(Image::new(element_meta(element), reference))
}

fn trim_inlines(inlines: &mut Vec<Inline>) {
    if let Some(Inline::Text(text)) = inlines.first_mut() {
        text.value = text.value.trim_start().to_string();
    }
    if let Some(Inline::Text(text)) = inlines.last_mut() {
        text.value = text.value.trim_end().to_string();
    }
    inlines.retain(|inline| !matches!(inline, Inline::Text(text) if text.value.is_empty()));
}

fn collapse(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for character in text.chars() {
        if character.is_whitespace() {
            if !out.ends_with(' ') {
                out.push(' ');
            }
        } else {
            out.push(character);
        }
    }
    out
}

fn raw_text(nodes: &[Node]) -> String {
    let mut out = String::new();
    for node in nodes {
        out.push_str(&raw_text_node(node));
    }
    out
}

fn raw_text_node(node: &Node) -> String {
    match node {
        Node::Text(text) => text.clone(),
        Node::Element(element) if element.tag == "br" => String::from("\n"),
        Node::Element(element) => raw_text(&element.children),
    }
}

fn element_meta(element: &Element) -> NodeMeta {
    let mut attrs = Attrs::new();
    let mut any = false;
    for class in element.class_names() {
        attrs = attrs.with_class(class);
        any = true;
    }
    if let Some(role) = element.attr("role") {
        attrs = attrs.with_role(role);
        any = true;
    }
    if let Some(id) = element.attr("id") {
        attrs = attrs.with_anchor(id);
        any = true;
    }
    let meta = NodeMeta::synthesized();
    if any {
        meta.with_attrs(attrs)
    } else {
        meta
    }
}
