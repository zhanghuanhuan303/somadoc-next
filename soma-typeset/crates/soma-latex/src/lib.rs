//! LaTeX subset front end.
//!
//! The front end reads a documented subset of LaTeX and produces DocIR. It is
//! deliberately small and forgiving: unknown commands are transparent (their
//! braced content is kept), unknown environments keep their blocks, and
//! malformed input never panics.
//!
//! Supported: sections/paragraphs, `\textbf`/`\emph`/`\textit`/`\underline`/
//! `\textsc`/`\textsuperscript`/`\textsubscript`/`\texttt`/`\verb`,
//! `\href`/`\url`, `\footnote`, inline `$...$`/`\(...\)` and display
//! `\[...\]`/`$$...$$`/`equation`, `itemize`/`enumerate`, `quote`/`quotation`,
//! `verbatim`, `tabular` (with `\hline`/booktabs rules, `\multicolumn`),
//! `\title`/`\author`/`\date`, and `\newpage`/`\clearpage`.
//!
//! Not supported: macros, `\usepackage` semantics, counters/cross-references,
//! `\includegraphics`, bibliography, and a full math implementation (the math
//! source is stored verbatim for a future math adapter).

#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]

extern crate alloc;

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

use soma_doc::block::{
    BlockQuote, Cell, CodeBlock, Column, Figure, Heading, List, ListItem, MathBlock, PageBreak,
    Paragraph, Row, Table,
};
use soma_doc::inline::FootnoteRef;
use soma_doc::inline::{
    Container, Image, IndexRef, Inline, LineBreak, LineBreakKind, Link, MathInline,
};
use soma_doc::meta::{Author, CoverData, Date, DocumentMeta, ImageRef};
use soma_doc::node::NodeMeta;
use soma_doc::{Block, Code, Document, FootnoteDef, Text};

pub const SYNTAX_ID: &str = "latex";
pub const SYNTAX_VERSION: u16 = 1;

/// Parse LaTeX into DocIR with an empty metadata block.
pub fn parse_latex(source: &str) -> Document {
    parse_latex_with_meta(source, DocumentMeta::new())
}

/// Parse LaTeX into DocIR. Preamble `\title`/`\author`/`\date` fill the
/// metadata when the caller did not set it.
pub fn parse_latex_with_meta(source: &str, mut meta: DocumentMeta) -> Document {
    let cleaned = strip_comments(source);
    let body = match cleaned.find("\\begin{document}") {
        Some(offset) => &cleaned[offset + "\\begin{document}".len()..],
        None => cleaned.as_str(),
    };
    if meta.title.is_none() {
        if let Some(title) = preamble_group(&cleaned, "title") {
            let mut parser = Parser::new(title);
            let inlines = parser.parse_inlines(false);
            if !inlines.is_empty() {
                meta.title = Some(inlines);
            }
        }
    }
    if meta.authors.is_empty() {
        if let Some(authors) = preamble_group(&cleaned, "author") {
            for author in split_authors(authors) {
                meta.authors.push(Author::new(author));
            }
        }
    }
    if meta.date.is_none() {
        if let Some(date) = preamble_group(&cleaned, "date") {
            if let Some(value) = parse_date(date.trim()) {
                meta.date = Some(value);
            }
        }
    }
    // `\subtitle{...}` has no DocIR field of its own; it belongs to the cover,
    // which the layout generates from the metadata.
    if let Some(subtitle) = preamble_group(&cleaned, "subtitle") {
        let text = subtitle.trim();
        if !text.is_empty() {
            let cover = meta.cover.get_or_insert_with(CoverData::new);
            if cover.subtitle.is_none() {
                cover.subtitle = Some(String::from(text));
            }
        }
    }

    let mut parser = Parser::new(body);
    let mut blocks = parser.parse_blocks(None, false);
    let footnotes = core::mem::take(&mut parser.footnotes);
    if blocks.is_empty() {
        blocks = Vec::new();
    }
    let mut document = Document::new(meta);
    document.blocks = blocks;
    document.footnotes = footnotes;
    document.renumber();
    document
}

fn strip_comments(source: &str) -> String {
    let mut out = String::with_capacity(source.len());
    let mut chars = source.chars().peekable();
    while let Some(character) = chars.next() {
        match character {
            '%' => {
                // A backslash before % was already handled as an escape.
                while let Some(&next) = chars.peek() {
                    if next == '\n' {
                        break;
                    }
                    chars.next();
                }
            }
            '\\' => {
                out.push('\\');
                if let Some(next) = chars.next() {
                    out.push(next);
                    if next == '%' {
                        // Keep the escaped percent; it is not a comment.
                        continue;
                    }
                }
            }
            _ => out.push(character),
        }
    }
    out
}

fn preamble_group<'a>(source: &'a str, command: &str) -> Option<&'a str> {
    let needle = alloc::format!("\\{command}");
    let mut search = 0usize;
    while let Some(offset) = source[search..].find(&needle) {
        let start = search + offset + needle.len();
        let bytes = source.as_bytes();
        let mut index = start;
        while index < bytes.len() && bytes[index].is_ascii_whitespace() {
            index += 1;
        }
        if index < bytes.len() && bytes[index] == b'{' {
            let mut depth = 0usize;
            let mut cursor = index;
            while cursor < bytes.len() {
                match bytes[cursor] {
                    b'{' => depth += 1,
                    b'}' => {
                        depth -= 1;
                        if depth == 0 {
                            return Some(&source[index + 1..cursor]);
                        }
                    }
                    _ => {}
                }
                cursor += 1;
            }
        }
        search = start;
    }
    None
}

/// Byte range (including the command and braces) and body of
/// `\command{...}`, so a caller can remove the whole command from a body.
fn command_group_span<'a>(source: &'a str, command: &str) -> Option<(usize, usize, &'a str)> {
    let needle = alloc::format!("\\{command}");
    let mut search = 0usize;
    while let Some(offset) = source[search..].find(&needle) {
        let start = search + offset;
        let bytes = source.as_bytes();
        let mut index = start + needle.len();
        while index < bytes.len() && bytes[index].is_ascii_whitespace() {
            index += 1;
        }
        if bytes.get(index) == Some(&b'{') {
            let mut depth = 0usize;
            let mut cursor = index;
            while cursor < bytes.len() {
                match bytes[cursor] {
                    b'{' => depth += 1,
                    b'}' => {
                        depth -= 1;
                        if depth == 0 {
                            return Some((start, cursor + 1, &source[index + 1..cursor]));
                        }
                    }
                    _ => {}
                }
                cursor += 1;
            }
        }
        search = start + needle.len();
    }
    None
}

/// Split a float body into its caption text and the body without the
/// `\caption{...}` command.
fn split_caption(body: &str) -> (Option<String>, String) {
    match command_group_span(body, "caption") {
        Some((start, end, text)) => (
            Some(String::from(text)),
            alloc::format!("{}{}", &body[..start], &body[end..]),
        ),
        None => (None, String::from(body)),
    }
}

/// The path argument of `\includegraphics[options]{path}`.
fn graphics_path(source: &str) -> Option<&str> {
    let needle = "\\includegraphics";
    let mut search = 0usize;
    while let Some(offset) = source[search..].find(needle) {
        let start = search + offset + needle.len();
        let bytes = source.as_bytes();
        let mut index = start;
        while index < bytes.len() && bytes[index].is_ascii_whitespace() {
            index += 1;
        }
        if bytes.get(index) == Some(&b'[') {
            while index < bytes.len() && bytes[index] != b']' {
                index += 1;
            }
            index += 1;
            while index < bytes.len() && bytes[index].is_ascii_whitespace() {
                index += 1;
            }
        }
        if bytes.get(index) == Some(&b'{') {
            let mut depth = 0usize;
            let mut cursor = index;
            while cursor < bytes.len() {
                match bytes[cursor] {
                    b'{' => depth += 1,
                    b'}' => {
                        depth -= 1;
                        if depth == 0 {
                            return Some(&source[index + 1..cursor]);
                        }
                    }
                    _ => {}
                }
                cursor += 1;
            }
        }
        search = start;
    }
    None
}

fn split_authors(source: &str) -> Vec<String> {
    let mut authors = Vec::new();
    let mut current = String::new();
    let mut depth = 0usize;
    let mut chars = source.chars().peekable();
    while let Some(character) = chars.next() {
        match character {
            '{' => {
                depth += 1;
                current.push(character);
            }
            '}' => {
                depth = depth.saturating_sub(1);
                current.push(character);
            }
            '\\' => {
                let mut name = String::new();
                while let Some(&next) = chars.peek() {
                    if next.is_ascii_alphabetic() {
                        name.push(next);
                        chars.next();
                    } else {
                        break;
                    }
                }
                if name == "and" && depth == 0 {
                    let value = current.trim();
                    if !value.is_empty() {
                        authors.push(value.to_string());
                    }
                    current.clear();
                } else {
                    current.push('\\');
                    current.push_str(&name);
                }
            }
            _ => current.push(character),
        }
    }
    let value = current.trim();
    if !value.is_empty() {
        authors.push(value.to_string());
    }
    authors
}

fn parse_date(text: &str) -> Option<Date> {
    let mut parts = text.split('-');
    let year = parts.next()?.parse::<i32>().ok()?;
    let month = parts.next()?.parse::<u8>().ok()?;
    let day = parts.next()?.parse::<u8>().ok()?;
    let date = Date::new(year, month, day);
    date.is_in_range().then_some(date)
}

enum Command {
    One(Inline),
    Many(Vec<Inline>),
    None,
}

struct Parser<'a> {
    input: &'a str,
    pos: usize,
    footnotes: Vec<FootnoteDef>,
    next_note: u32,
}

impl<'a> Parser<'a> {
    fn new(input: &'a str) -> Self {
        Self {
            input,
            pos: 0,
            footnotes: Vec::new(),
            next_note: 1,
        }
    }

    fn bytes(&self) -> &'a [u8] {
        self.input.as_bytes()
    }

    fn eof(&self) -> bool {
        self.pos >= self.input.len()
    }

    fn starts(&self, text: &str) -> bool {
        self.input[self.pos..].starts_with(text)
    }

    fn peek_char(&self) -> Option<char> {
        self.input[self.pos..].chars().next()
    }

    fn bump(&mut self) -> Option<char> {
        let character = self.peek_char()?;
        self.pos += character.len_utf8();
        Some(character)
    }

    fn skip_spaces(&mut self) {
        while let Some(character) = self.peek_char() {
            if character == ' ' || character == '\t' || character == '\r' {
                self.pos += character.len_utf8();
            } else {
                break;
            }
        }
    }

    /// Skip whitespace; returns true when a blank line was crossed.
    fn skip_blank(&mut self) -> bool {
        let mut newlines = 0usize;
        while let Some(character) = self.peek_char() {
            match character {
                '\n' => {
                    newlines += 1;
                    self.pos += 1;
                }
                ' ' | '\t' | '\r' => {
                    self.pos += 1;
                }
                _ => break,
            }
        }
        newlines >= 2
    }

    fn peek_command(&self) -> Option<&'a str> {
        if self.peek_char() != Some('\\') {
            return None;
        }
        let bytes = self.bytes();
        let mut index = self.pos + 1;
        if index >= bytes.len() {
            return None;
        }
        if bytes[index].is_ascii_alphabetic() {
            let start = index;
            while index < bytes.len() && bytes[index].is_ascii_alphabetic() {
                index += 1;
            }
            Some(&self.input[start..index])
        } else {
            Some(&self.input[index..index + 1])
        }
    }

    /// Consume a command name (`\section`, `\%`, `\\`) plus an optional `*`.
    fn read_command(&mut self) -> &'a str {
        let bytes = self.bytes();
        let mut index = self.pos + 1;
        if index >= bytes.len() {
            self.pos = bytes.len();
            return "";
        }
        let start;
        let end;
        if bytes[index].is_ascii_alphabetic() {
            start = index;
            while index < bytes.len() && bytes[index].is_ascii_alphabetic() {
                index += 1;
            }
            end = index;
            if index < bytes.len() && bytes[index] == b'*' {
                index += 1;
            }
        } else {
            start = index;
            end = index + 1;
            index += 1;
        }
        self.pos = index;
        &self.input[start..end]
    }

    fn read_group(&mut self) -> Option<&'a str> {
        self.skip_spaces();
        if self.peek_char() != Some('{') {
            return None;
        }
        let bytes = self.bytes();
        let start = self.pos;
        let mut depth = 0usize;
        let mut index = start;
        while index < bytes.len() {
            match bytes[index] {
                b'\\' => {
                    index += 1;
                }
                b'{' => depth += 1,
                b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        self.pos = index + 1;
                        return Some(&self.input[start + 1..index]);
                    }
                }
                _ => {}
            }
            index += 1;
        }
        // Unterminated group: keep the remaining text instead of dropping it.
        self.pos = bytes.len();
        (start < bytes.len()).then(|| &self.input[start + 1..])
    }

    fn read_optional(&mut self) -> Option<&'a str> {
        self.skip_spaces();
        if self.peek_char() != Some('[') {
            return None;
        }
        let bytes = self.bytes();
        let start = self.pos;
        let mut index = start;
        while index < bytes.len() {
            match bytes[index] {
                b'\\' => index += 1,
                b']' => {
                    self.pos = index + 1;
                    return Some(&self.input[start + 1..index]);
                }
                _ => {}
            }
            index += 1;
        }
        self.pos = bytes.len();
        None
    }

    fn peek_env(&self) -> Option<&'a str> {
        let start = if self.starts("\\begin{") {
            self.pos + 7
        } else if self.starts("\\end{") {
            self.pos + 5
        } else {
            return None;
        };
        let rest = &self.input[start..];
        let end = rest.find('}')?;
        Some(&rest[..end])
    }

    fn consume_env_marker(&mut self) {
        if let Some(offset) = self.input[self.pos..].find('}') {
            self.pos += offset + 1;
        } else {
            self.pos = self.input.len();
        }
    }

    // ------------------------------------------------------------- blocks

    fn parse_blocks(&mut self, end: Option<&str>, at_item: bool) -> Vec<Block> {
        let mut blocks = Vec::new();
        loop {
            self.skip_blank();
            self.skip_spaces();
            if self.eof() {
                break;
            }
            if let Some(name) = self.peek_env() {
                if self.starts("\\end{") {
                    if end == Some(name) {
                        break;
                    }
                    self.consume_env_marker();
                    continue;
                }
            }
            if at_item && self.peek_command() == Some("item") {
                break;
            }
            if self.starts("\\[") || self.starts("$$") {
                blocks.push(self.parse_display_math());
                continue;
            }
            if let Some(level) = self.peek_section() {
                blocks.push(self.parse_section(level));
                continue;
            }
            match self.peek_command() {
                Some("newpage") | Some("clearpage") => {
                    self.read_command();
                    blocks.push(Block::PageBreak(PageBreak::new(NodeMeta::synthesized())));
                    continue;
                }
                Some("par") | Some("maketitle") | Some("tableofcontents") | Some("noindent") => {
                    self.read_command();
                    continue;
                }
                Some("begin") => {
                    if let Some(inner) = self.parse_environment() {
                        blocks.extend(inner);
                    }
                    continue;
                }
                Some("item") | Some("end") => {
                    self.read_command();
                    continue;
                }
                Some(_) => {
                    // An unknown block command: keep a braced argument as text.
                    self.read_command();
                    if let Some(group) = self.read_group() {
                        let inlines = self.fragment(group);
                        if !inlines.is_empty() {
                            blocks.push(Block::Paragraph(Paragraph::new(
                                NodeMeta::synthesized(),
                                inlines,
                            )));
                        }
                    }
                    continue;
                }
                None => {}
            }

            let before = self.pos;
            let inlines = self.parse_inlines(false);
            if !inlines.is_empty() {
                blocks.push(Block::Paragraph(Paragraph::new(
                    NodeMeta::synthesized(),
                    inlines,
                )));
            }
            if self.pos == before {
                // Safety valve for unexpected input.
                self.bump();
            }
        }
        blocks
    }

    fn peek_section(&self) -> Option<u8> {
        let command = self.peek_command()?;
        let level = match command {
            "section" => 1,
            "subsection" => 2,
            "subsubsection" => 3,
            "paragraph" => 4,
            "subparagraph" => 5,
            _ => return None,
        };
        Some(level)
    }

    fn parse_section(&mut self, level: u8) -> Block {
        self.read_command();
        self.read_optional();
        let inlines = match self.read_group() {
            Some(group) => self.fragment(group),
            None => Vec::new(),
        };
        Block::Heading(Heading::new(NodeMeta::synthesized(), level, inlines))
    }

    fn parse_display_math(&mut self) -> Block {
        let source = if self.starts("\\[") {
            self.pos += 2;
            let rest = &self.input[self.pos..];
            match rest.find("\\]") {
                Some(offset) => {
                    let source = &rest[..offset];
                    self.pos += offset + 2;
                    source
                }
                None => {
                    self.pos = self.input.len();
                    rest
                }
            }
        } else {
            self.pos += 2;
            let rest = &self.input[self.pos..];
            match rest.find("$$") {
                Some(offset) => {
                    let source = &rest[..offset];
                    self.pos += offset + 2;
                    source
                }
                None => {
                    self.pos = self.input.len();
                    rest
                }
            }
        };
        Block::MathBlock(MathBlock::new(
            NodeMeta::synthesized(),
            source.trim().to_string(),
        ))
    }

    fn parse_environment(&mut self) -> Option<Vec<Block>> {
        let env = self.peek_env()?.to_string();
        self.consume_env_marker();
        let blocks = match env.as_str() {
            "itemize" | "enumerate" => {
                let items = self.parse_items(&env);
                if items.is_empty() {
                    Vec::new()
                } else {
                    vec![Block::List(List::new(
                        NodeMeta::synthesized(),
                        env == "enumerate",
                        1,
                        items,
                    ))]
                }
            }
            "quote" | "quotation" => {
                let inner = self.parse_blocks(Some(&env), false);
                self.consume_end(&env);
                if inner.is_empty() {
                    Vec::new()
                } else {
                    vec![Block::BlockQuote(BlockQuote::new(
                        NodeMeta::synthesized(),
                        inner,
                    ))]
                }
            }
            "center" | "flushleft" | "flushright" => {
                let inner = self.parse_blocks(Some(&env), false);
                self.consume_end(&env);
                inner
            }
            "verbatim" => {
                let rest = &self.input[self.pos..];
                let marker = alloc::format!("\\end{{{env}}}");
                let (value, end) = match rest.find(&marker) {
                    Some(offset) => (&rest[..offset], self.pos + offset + marker.len()),
                    None => (rest, self.input.len()),
                };
                self.pos = end;
                let value = value.strip_prefix('\n').unwrap_or(value);
                let value = value.strip_suffix('\n').unwrap_or(value);
                vec![Block::CodeBlock(CodeBlock::new(
                    NodeMeta::synthesized(),
                    None,
                    value,
                ))]
            }
            "tabular" | "tabular*" => {
                self.read_optional();
                let _ = self.read_group();
                let rest = &self.input[self.pos..];
                let marker = alloc::format!("\\end{{{env}}}");
                let (value, end) = match rest.find(&marker) {
                    Some(offset) => (&rest[..offset], self.pos + offset + marker.len()),
                    None => (rest, self.input.len()),
                };
                self.pos = end;
                self.parse_tabular(value).into_iter().collect()
            }
            "table" | "table*" => {
                let raw = self.environment_body(&env);
                let (caption, body) = split_caption(&raw);
                let mut parser = Parser::new(&body);
                let mut blocks = parser.parse_blocks(None, false);
                self.footnotes
                    .extend(core::mem::take(&mut parser.footnotes));
                if let Some(caption) = caption {
                    let inlines = self.caption_inlines(&caption);
                    if let Some(Block::Table(table)) = blocks
                        .iter_mut()
                        .find(|block| matches!(block, Block::Table(_)))
                    {
                        table.caption = inlines;
                    }
                }
                blocks
            }
            "figure" | "figure*" => {
                let raw = self.environment_body(&env);
                let (caption, body) = split_caption(&raw);
                let Some(path) = graphics_path(&body) else {
                    return Some(Vec::new());
                };
                let mut figure = Figure::new(
                    NodeMeta::synthesized(),
                    Image::new(NodeMeta::synthesized(), ImageRef::new(path)),
                );
                if let Some(caption) = caption {
                    figure.caption = self.caption_inlines(&caption);
                }
                vec![Block::Figure(figure)]
            }
            // Environments that own their display: the block keeps the
            // `\begin{...}` wrapper so the math parser sees the rows and the
            // layout can number the formula.
            "equation" | "equation*" | "align" | "align*" | "gather" | "gather*" | "multline"
            | "multline*" => {
                let body = self.environment_body(&env);
                let source = alloc::format!("\\begin{{{env}}}{}\\end{{{env}}}", body.trim());
                vec![Block::MathBlock(MathBlock::new(
                    NodeMeta::synthesized(),
                    source,
                ))]
            }
            "displaymath" | "math" => {
                let body = self.environment_body(&env);
                vec![Block::MathBlock(MathBlock::new(
                    NodeMeta::synthesized(),
                    body.trim().to_string(),
                ))]
            }
            _ => {
                let inner = self.parse_blocks(Some(&env), false);
                self.consume_end(&env);
                inner
            }
        };
        Some(blocks)
    }

    fn consume_end(&mut self, env: &str) {
        let marker = alloc::format!("\\end{{{env}}}");
        if self.starts(&marker) {
            self.pos += marker.len();
        }
    }

    /// Consume the rest of the current environment and return its body.
    fn environment_body(&mut self, env: &str) -> String {
        let rest = &self.input[self.pos..];
        let marker = alloc::format!("\\end{{{env}}}");
        let (body, end) = match rest.find(&marker) {
            Some(offset) => (&rest[..offset], self.pos + offset + marker.len()),
            None => (rest, self.input.len()),
        };
        self.pos = end;
        String::from(body)
    }

    /// Parse a caption body as inline content, lifting any footnotes into the
    /// document.
    fn caption_inlines(&mut self, source: &str) -> Vec<Inline> {
        let mut parser = Parser::new(source);
        let inlines = parser.parse_inlines(false);
        self.footnotes
            .extend(core::mem::take(&mut parser.footnotes));
        inlines
    }

    fn parse_items(&mut self, env: &str) -> Vec<ListItem> {
        let mut items = Vec::new();
        loop {
            self.skip_blank();
            self.skip_spaces();
            if self.eof() {
                break;
            }
            if self.starts("\\end{") {
                self.consume_end(env);
                break;
            }
            if self.peek_command() == Some("item") {
                self.read_command();
                self.read_optional();
            }
            let blocks = self.parse_blocks(Some(env), true);
            if !blocks.is_empty() || !items.is_empty() {
                items.push(ListItem::new(NodeMeta::synthesized(), blocks));
            }
            if self.peek_command() == Some("item") {
                continue;
            }
            if self.starts("\\end{") {
                self.consume_end(env);
                break;
            }
            if self.eof() {
                break;
            }
            self.bump();
        }
        items
    }

    fn parse_tabular(&mut self, source: &str) -> Option<Block> {
        let mut rows: Vec<Vec<(usize, String)>> = Vec::new();
        let mut current: Vec<(usize, String)> = Vec::new();
        let mut cell = String::new();
        let mut pending_span = 1usize;
        let mut header_end = None;
        let mut rule_after_row = false;
        let mut saw_row = false;
        let mut index = 0usize;
        let bytes = source.as_bytes();
        while index < bytes.len() {
            if bytes[index] == b'\\' {
                let rest = &source[index + 1..];
                if rest.starts_with('\\') {
                    // Row separator.
                    current.push((pending_span, trim_cell(&cell)));
                    pending_span = 1;
                    cell.clear();
                    rows.push(core::mem::take(&mut current));
                    saw_row = true;
                    if rule_after_row && header_end.is_none() {
                        header_end = Some(rows.len());
                    }
                    rule_after_row = false;
                    index += 2;
                    continue;
                }
                if rest.starts_with("hline") {
                    index += 6;
                    if saw_row && header_end.is_none() {
                        header_end = Some(rows.len());
                    }
                    rule_after_row = true;
                    continue;
                }
                if rest.starts_with("toprule") || rest.starts_with("bottomrule") {
                    index += if rest.starts_with("toprule") { 8 } else { 11 };
                    continue;
                }
                if rest.starts_with("midrule") {
                    index += 8;
                    if saw_row && header_end.is_none() {
                        header_end = Some(rows.len());
                    }
                    continue;
                }
                if rest.starts_with("multicolumn") {
                    let mut cursor = index + "\\multicolumn".len();
                    let span = read_group_at(source, &mut cursor)
                        .and_then(|value| value.trim().parse::<usize>().ok())
                        .unwrap_or(1);
                    let _ = read_group_at(source, &mut cursor);
                    let text = read_group_at(source, &mut cursor).unwrap_or("");
                    cell.clear();
                    cell.push_str(text);
                    pending_span = span.max(1);
                    index = cursor;
                    continue;
                }
                // Unknown command: skip it and its name.
                let mut cursor = index + 1;
                while cursor < bytes.len() && bytes[cursor].is_ascii_alphabetic() {
                    cursor += 1;
                }
                index = cursor;
                continue;
            }
            if bytes[index] == b'&' {
                current.push((pending_span, trim_cell(&cell)));
                pending_span = 1;
                cell.clear();
                index += 1;
                continue;
            }
            if bytes[index] == b'%' {
                while index < bytes.len() && bytes[index] != b'\n' {
                    index += 1;
                }
                continue;
            }
            let character = source[index..].chars().next()?;
            cell.push(character);
            index += character.len_utf8();
        }
        if !cell.trim().is_empty() || !current.is_empty() {
            current.push((pending_span, trim_cell(&cell)));
            rows.push(current);
        }
        if rows.is_empty() {
            return None;
        }
        let columns = rows
            .iter()
            .map(|row| row.iter().map(|(span, _)| *span).sum::<usize>())
            .max()
            .unwrap_or(0);
        if columns == 0 {
            return None;
        }
        let header_rows = header_end.unwrap_or(0);
        let mut doc_rows = Vec::with_capacity(rows.len());
        for (index, row) in rows.iter().enumerate() {
            let mut cells = Vec::new();
            let mut used = 0usize;
            for (span, text) in row {
                let span = (*span).min(columns.saturating_sub(used).max(1));
                if span == 0 {
                    continue;
                }
                let mut cell = Cell::new(
                    NodeMeta::synthesized(),
                    vec![Block::Paragraph(Paragraph::new(
                        NodeMeta::synthesized(),
                        vec![Inline::Text(Text::new(
                            NodeMeta::synthesized(),
                            render_cell_text(text),
                        ))],
                    ))],
                );
                cell.colspan = span as u16;
                cells.push(cell);
                used += span;
            }
            while used < columns {
                cells.push(Cell::new(NodeMeta::synthesized(), Vec::new()));
                used += 1;
            }
            doc_rows.push(Row::new(
                NodeMeta::synthesized(),
                index < header_rows,
                cells,
            ));
        }
        let columns = (0..columns)
            .map(|_| Column::new(NodeMeta::synthesized(), soma_doc::CellAlign::Default))
            .collect();
        Some(Block::Table(Table::new(
            NodeMeta::synthesized(),
            columns,
            doc_rows,
        )))
    }

    // ------------------------------------------------------------- inlines

    fn parse_inlines(&mut self, at_group_end: bool) -> Vec<Inline> {
        let mut out: Vec<Inline> = Vec::new();
        let mut buffer = String::new();
        loop {
            if self.eof() {
                break;
            }
            let character = self.peek_char().expect("not eof");
            if character == '\n' {
                // Blank line ends the paragraph; a single newline is a space.
                let saved = self.pos;
                if self.skip_blank() {
                    break;
                }
                let _ = saved;
                push_space(&mut buffer);
                continue;
            }
            if character == ' ' || character == '\t' || character == '\r' {
                self.pos += 1;
                push_space(&mut buffer);
                continue;
            }
            if character == '}' {
                if at_group_end {
                    break;
                }
                self.pos += 1;
                continue;
            }
            if character == '{' {
                if let Some(group) = self.read_group() {
                    flush_buffer(&mut out, &mut buffer);
                    out.extend(self.fragment(group));
                }
                continue;
            }
            if character == '~' {
                self.pos += 1;
                push_space(&mut buffer);
                continue;
            }
            if character == '$' {
                flush_buffer(&mut out, &mut buffer);
                if self.starts("$$") {
                    self.pos += 2;
                    let rest = &self.input[self.pos..];
                    let (source, end) = match rest.find("$$") {
                        Some(offset) => (&rest[..offset], self.pos + offset + 2),
                        None => (rest, self.input.len()),
                    };
                    self.pos = end;
                    out.push(Inline::Math(MathInline::new(
                        NodeMeta::synthesized(),
                        source.to_string(),
                    )));
                } else {
                    self.pos += 1;
                    let rest = &self.input[self.pos..];
                    let (source, end) = match rest.find('$') {
                        Some(offset) => (&rest[..offset], self.pos + offset + 1),
                        None => (rest, self.input.len()),
                    };
                    self.pos = end;
                    out.push(Inline::Math(MathInline::new(
                        NodeMeta::synthesized(),
                        source.to_string(),
                    )));
                }
                continue;
            }
            if character == '\\' {
                if self.starts_block_command(at_group_end) {
                    break;
                }
                flush_buffer(&mut out, &mut buffer);
                match self.parse_command(at_group_end) {
                    Command::One(inline) => out.push(inline),
                    Command::Many(inlines) => out.extend(inlines),
                    Command::None => {}
                }
                continue;
            }
            buffer.push(character);
            self.pos += character.len_utf8();
        }
        flush_buffer(&mut out, &mut buffer);
        trim_inlines(&mut out);
        out
    }

    fn starts_block_command(&self, at_group_end: bool) -> bool {
        if at_group_end {
            return false;
        }
        if self.starts("\\end{") || self.starts("\\begin{") {
            return true;
        }
        matches!(
            self.peek_command(),
            Some(
                "item"
                    | "par"
                    | "newpage"
                    | "clearpage"
                    | "maketitle"
                    | "tableofcontents"
                    | "section"
                    | "subsection"
                    | "subsubsection"
                    | "paragraph"
                    | "subparagraph"
            )
        )
    }

    fn parse_command(&mut self, at_group_end: bool) -> Command {
        let name = self.read_command().to_string();
        match name.as_str() {
            "\\" => {
                self.read_optional();
                Command::One(Inline::LineBreak(LineBreak::new(
                    NodeMeta::synthesized(),
                    LineBreakKind::Hard,
                )))
            }
            "newline" => Command::One(Inline::LineBreak(LineBreak::new(
                NodeMeta::synthesized(),
                LineBreakKind::Hard,
            ))),
            "%" | "&" | "_" | "#" | "$" | "{" | "}" => {
                Command::One(Inline::Text(Text::new(NodeMeta::synthesized(), name)))
            }
            "," | ";" | ":" | " " => Command::One(Inline::Text(Text::new(
                NodeMeta::synthesized(),
                String::from(" "),
            ))),
            "!" => Command::None,
            "ldots" | "dots" => Command::One(Inline::Text(Text::new(
                NodeMeta::synthesized(),
                String::from("…"),
            ))),
            "LaTeX" => Command::One(Inline::Text(Text::new(
                NodeMeta::synthesized(),
                String::from("LaTeX"),
            ))),
            "TeX" => Command::One(Inline::Text(Text::new(
                NodeMeta::synthesized(),
                String::from("TeX"),
            ))),
            "(" => self.read_math_inline("\\)"),
            "[" => self.read_math_inline("\\]"),
            "textbf" => self.container(InlineKind::Strong),
            "emph" | "textit" | "textsl" => self.container(InlineKind::Emphasis),
            "underline" => self.container(InlineKind::Underline),
            "textsc" => self.container(InlineKind::SmallCaps),
            "textsuperscript" => self.container(InlineKind::Superscript),
            "textsubscript" => self.container(InlineKind::Subscript),
            "texttt" | "verb" => {
                if name == "verb" {
                    let delimiter = self.bump().unwrap_or('|');
                    let rest = &self.input[self.pos..];
                    let (value, end) = match rest.find(delimiter) {
                        Some(offset) => (&rest[..offset], self.pos + offset + delimiter.len_utf8()),
                        None => (rest, self.input.len()),
                    };
                    self.pos = end;
                    Command::One(Inline::Code(Code::new(
                        NodeMeta::synthesized(),
                        value.to_string(),
                    )))
                } else {
                    let value = self.read_group().unwrap_or("");
                    Command::One(Inline::Code(Code::new(
                        NodeMeta::synthesized(),
                        collapse(value),
                    )))
                }
            }
            "href" => {
                let href = self.read_group().unwrap_or("").to_string();
                let children = self
                    .read_group()
                    .map(|group| self.fragment(group))
                    .unwrap_or_default();
                if href.is_empty() || children.is_empty() {
                    Command::Many(children)
                } else {
                    Command::One(Inline::Link(Link::new(
                        NodeMeta::synthesized(),
                        href,
                        children,
                    )))
                }
            }
            "url" => {
                let href = self.read_group().unwrap_or("").to_string();
                if href.is_empty() {
                    Command::None
                } else {
                    let text = Inline::Text(Text::new(NodeMeta::synthesized(), href.clone()));
                    Command::One(Inline::Link(Link::new(
                        NodeMeta::synthesized(),
                        href,
                        vec![text],
                    )))
                }
            }
            "footnote" => {
                let inlines = self
                    .read_group()
                    .map(|group| self.fragment(group))
                    .unwrap_or_default();
                let label = alloc::string::ToString::to_string(&self.next_note);
                self.next_note += 1;
                let mut blocks = Vec::new();
                if !inlines.is_empty() {
                    blocks.push(Block::Paragraph(Paragraph::new(
                        NodeMeta::synthesized(),
                        inlines,
                    )));
                }
                self.footnotes.push(FootnoteDef::new(
                    NodeMeta::synthesized(),
                    label.clone(),
                    blocks,
                ));
                Command::One(Inline::FootnoteRef(FootnoteRef::new(
                    NodeMeta::synthesized(),
                    label,
                )))
            }
            "index" => {
                let term = self.read_group().unwrap_or("").trim();
                if term.is_empty() {
                    Command::None
                } else {
                    Command::One(Inline::Index(IndexRef::new(NodeMeta::synthesized(), term)))
                }
            }
            "label" | "cite" | "ref" | "pageref" => {
                let _ = self.read_group();
                Command::None
            }
            _ => {
                // Unknown command: transparent, keep a braced argument.
                let _ = at_group_end;
                if self.peek_char_after_spaces() == Some('{') {
                    if let Some(group) = self.read_group() {
                        return Command::Many(self.fragment(group));
                    }
                }
                Command::None
            }
        }
    }

    fn peek_char_after_spaces(&self) -> Option<char> {
        let mut index = self.pos;
        let bytes = self.bytes();
        while index < bytes.len() && (bytes[index] == b' ' || bytes[index] == b'\t') {
            index += 1;
        }
        self.input[index..].chars().next()
    }

    fn container(&mut self, kind: InlineKind) -> Command {
        let Some(group) = self.read_group() else {
            return Command::None;
        };
        let children = self.fragment(group);
        if children.is_empty() {
            return Command::None;
        }
        let meta = NodeMeta::synthesized();
        Command::One(match kind {
            InlineKind::Strong => Inline::Strong(Container::new(meta, children)),
            InlineKind::Emphasis => Inline::Emphasis(Container::new(meta, children)),
            InlineKind::Underline => Inline::Underline(Container::new(meta, children)),
            InlineKind::SmallCaps => Inline::SmallCaps(Container::new(meta, children)),
            InlineKind::Superscript => Inline::Superscript(Container::new(meta, children)),
            InlineKind::Subscript => Inline::Subscript(Container::new(meta, children)),
        })
    }

    fn read_math_inline(&mut self, close: &str) -> Command {
        let rest = &self.input[self.pos..];
        let (source, end) = match rest.find(close) {
            Some(offset) => (&rest[..offset], self.pos + offset + close.len()),
            None => (rest, self.input.len()),
        };
        self.pos = end;
        Command::One(Inline::Math(MathInline::new(
            NodeMeta::synthesized(),
            source.to_string(),
        )))
    }

    fn fragment(&mut self, source: &'a str) -> Vec<Inline> {
        let mut child = Parser {
            input: source,
            pos: 0,
            footnotes: core::mem::take(&mut self.footnotes),
            next_note: self.next_note,
        };
        let inlines = child.parse_inlines(true);
        self.footnotes = core::mem::take(&mut child.footnotes);
        self.next_note = child.next_note;
        inlines
    }
}

#[derive(Clone, Copy)]
enum InlineKind {
    Strong,
    Emphasis,
    Underline,
    SmallCaps,
    Superscript,
    Subscript,
}

fn push_space(buffer: &mut String) {
    if !buffer.ends_with(' ') {
        buffer.push(' ');
    }
}

fn flush_buffer(out: &mut Vec<Inline>, buffer: &mut String) {
    if !buffer.is_empty() {
        out.push(Inline::Text(Text::new(
            NodeMeta::synthesized(),
            buffer.clone(),
        )));
    }
    buffer.clear();
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
    out.trim().to_string()
}

fn trim_cell(text: &str) -> String {
    collapse(text)
}

fn render_cell_text(text: &str) -> String {
    text.replace("---", "—").replace("--", "–")
}

fn read_group_at<'a>(source: &'a str, cursor: &mut usize) -> Option<&'a str> {
    let bytes = source.as_bytes();
    let mut index = *cursor;
    while index < bytes.len() && bytes[index].is_ascii_whitespace() {
        index += 1;
    }
    if index >= bytes.len() || bytes[index] != b'{' {
        return None;
    }
    let start = index;
    let mut depth = 0usize;
    while index < bytes.len() {
        match bytes[index] {
            b'\\' => index += 1,
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    *cursor = index + 1;
                    return Some(&source[start + 1..index]);
                }
            }
            _ => {}
        }
        index += 1;
    }
    None
}
