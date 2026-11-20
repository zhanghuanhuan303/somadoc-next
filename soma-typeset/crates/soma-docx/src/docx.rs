//! OOXML (WordprocessingML) to DocIR mapping.
//!
//! The walk is deliberately shallow: it understands the block and run
//! containers DocIR can represent, and skips everything else without failing.
//! Word properties are read only when they have a semantic DocIR counterpart;
//! fonts, spacing, colours, and style-sheet inheritance are ignored.

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use soma_doc::block::{
    Block, Cell, CellAlign, Column, Heading, List, ListItem, Paragraph, Row, Table,
};
use soma_doc::inline::{Container, Inline, LineBreak, LineBreakKind};
use soma_doc::meta::{Author, CoverData, DocumentMeta};
use soma_doc::node::NodeMeta;
use soma_doc::Text;

use crate::xml::{Token, Tokenizer};
use crate::DocxError;

/// Parse the `w:body` of `word/document.xml`.
pub(crate) fn parse_blocks(xml: &str) -> Result<Vec<Block>, DocxError> {
    let mut tokens = Tokenizer::new(xml);
    loop {
        match tokens.next_token()? {
            None => return Err(DocxError::new("word/document.xml has no body")),
            Some(Token::Start(tag)) if tag.name == "body" => {
                if tag.self_closing {
                    return Ok(Vec::new());
                }
                return parse_children(&mut tokens, "body");
            }
            Some(_) => {}
        }
    }
}

/// Read block children until `scope` closes.
///
/// Paragraphs carrying `w:numPr` and separated only by other numbered
/// paragraphs become one flat `List`. A non-numbered block always closes the
/// list; Word's own list continuation is intentionally not modelled.
fn parse_children(tokens: &mut Tokenizer<'_>, scope: &str) -> Result<Vec<Block>, DocxError> {
    let mut blocks = Vec::new();
    let mut pending: Option<PendingList> = None;
    let mut closed = false;

    while let Some(token) = tokens.next_token()? {
        match token {
            Token::Start(tag) if tag.name == "p" => {
                let paragraph = parse_paragraph(tokens, tag.self_closing)?;
                match paragraph.numbering {
                    Some(ordered) => {
                        let starts_new =
                            pending.as_ref().is_none_or(|list| list.ordered != ordered);
                        if starts_new {
                            flush_list(&mut blocks, &mut pending);
                            pending = Some(PendingList {
                                ordered,
                                items: Vec::new(),
                            });
                        }
                        if let Some(list) = pending.as_mut() {
                            list.items.push(ListItem::new(
                                NodeMeta::synthesized(),
                                vec![paragraph.block],
                            ));
                        }
                    }
                    None => {
                        flush_list(&mut blocks, &mut pending);
                        blocks.push(paragraph.block);
                    }
                }
            }
            Token::Start(tag) if tag.name == "tbl" => {
                flush_list(&mut blocks, &mut pending);
                blocks.push(parse_table(tokens, tag.self_closing)?);
            }
            Token::Start(tag) => skip_element(tokens, tag.self_closing)?,
            Token::End(name) => {
                if name == scope {
                    closed = true;
                    break;
                }
            }
            Token::Text(_) => {}
        }
    }

    if !closed {
        return Err(DocxError::new("XML is malformed"));
    }
    flush_list(&mut blocks, &mut pending);
    Ok(blocks)
}

struct PendingList {
    ordered: bool,
    items: Vec<ListItem>,
}

fn flush_list(blocks: &mut Vec<Block>, pending: &mut Option<PendingList>) {
    if let Some(list) = pending.take() {
        if !list.items.is_empty() {
            blocks.push(Block::List(List::new(
                NodeMeta::synthesized(),
                list.ordered,
                1,
                list.items,
            )));
        }
    }
}

struct ParagraphResult {
    block: Block,
    /// `Some(ordered)` when `w:numPr` was present.
    numbering: Option<bool>,
}

fn parse_paragraph(
    tokens: &mut Tokenizer<'_>,
    self_closing: bool,
) -> Result<ParagraphResult, DocxError> {
    if self_closing {
        return Ok(ParagraphResult {
            block: Block::Paragraph(Paragraph::new(NodeMeta::synthesized(), Vec::new())),
            numbering: None,
        });
    }

    let mut style: Option<String> = None;
    let mut numbering: Option<bool> = None;
    let mut inlines = Vec::new();

    while let Some(token) = tokens.next_token()? {
        match token {
            Token::Start(tag) => match tag.name {
                "pPr" => {
                    parse_paragraph_properties(
                        tokens,
                        tag.self_closing,
                        &mut style,
                        &mut numbering,
                    )?;
                }
                "r" => parse_run(tokens, tag.self_closing, &mut inlines)?,
                "hyperlink" => parse_hyperlink(tokens, tag.self_closing, &mut inlines)?,
                _ => skip_element(tokens, tag.self_closing)?,
            },
            Token::End(name) => {
                if name == "p" {
                    break;
                }
            }
            Token::Text(_) => {}
        }
    }

    let block = match style.as_deref().and_then(heading_level) {
        Some(level) => Block::Heading(Heading::new(NodeMeta::synthesized(), level, inlines)),
        None => Block::Paragraph(Paragraph::new(NodeMeta::synthesized(), inlines)),
    };
    Ok(ParagraphResult { block, numbering })
}

fn parse_paragraph_properties(
    tokens: &mut Tokenizer<'_>,
    self_closing: bool,
    style: &mut Option<String>,
    numbering: &mut Option<bool>,
) -> Result<(), DocxError> {
    if self_closing {
        return Ok(());
    }
    while let Some(token) = tokens.next_token()? {
        match token {
            Token::Start(tag) => match tag.name {
                "pStyle" => {
                    if let Some(value) = tag.attr("val") {
                        let value = value.trim();
                        if !value.is_empty() {
                            *style = Some(String::from(value));
                        }
                    }
                    skip_element(tokens, tag.self_closing)?;
                }
                "numPr" => {
                    // The presence of numPr is the signal; a list without numId
                    // cannot be proven ordered, so it becomes unordered.
                    *numbering = Some(false);
                    parse_numbering(tokens, tag.self_closing, numbering)?;
                }
                _ => skip_element(tokens, tag.self_closing)?,
            },
            Token::End(name) => {
                if name == "pPr" {
                    break;
                }
            }
            Token::Text(_) => {}
        }
    }
    Ok(())
}

fn parse_numbering(
    tokens: &mut Tokenizer<'_>,
    self_closing: bool,
    numbering: &mut Option<bool>,
) -> Result<(), DocxError> {
    if self_closing {
        return Ok(());
    }
    while let Some(token) = tokens.next_token()? {
        match token {
            Token::Start(tag) => {
                if tag.name == "numId" {
                    // Whether the list is bulleted or numbered lives in
                    // numbering.xml, which is not loaded. Per this front end's
                    // contract, a numId makes it ordered.
                    *numbering = Some(true);
                }
                skip_element(tokens, tag.self_closing)?;
            }
            Token::End(name) => {
                if name == "numPr" {
                    break;
                }
            }
            Token::Text(_) => {}
        }
    }
    Ok(())
}

/// `w:hyperlink` is flattened to its child runs. The `r:id` target lives in
/// `document.xml.rels`, a part this reader does not load, so creating a
/// `Link` with a guessed URL would be worse than keeping the text.
fn parse_hyperlink(
    tokens: &mut Tokenizer<'_>,
    self_closing: bool,
    out: &mut Vec<Inline>,
) -> Result<(), DocxError> {
    if self_closing {
        return Ok(());
    }
    while let Some(token) = tokens.next_token()? {
        match token {
            Token::Start(tag) => {
                if tag.name == "r" {
                    parse_run(tokens, tag.self_closing, out)?;
                } else {
                    skip_element(tokens, tag.self_closing)?;
                }
            }
            Token::End(name) => {
                if name == "hyperlink" {
                    break;
                }
            }
            Token::Text(_) => {}
        }
    }
    Ok(())
}

#[derive(Default)]
struct RunStyle {
    bold: bool,
    italic: bool,
    underline: bool,
    vertical: Option<Vertical>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Vertical {
    Superscript,
    Subscript,
}

fn parse_run(
    tokens: &mut Tokenizer<'_>,
    self_closing: bool,
    out: &mut Vec<Inline>,
) -> Result<(), DocxError> {
    if self_closing {
        return Ok(());
    }

    let mut style = RunStyle::default();
    let mut inlines = Vec::new();

    while let Some(token) = tokens.next_token()? {
        match token {
            Token::Start(tag) => match tag.name {
                "rPr" => parse_run_properties(tokens, tag.self_closing, &mut style)?,
                "t" => {
                    let preserve = tag
                        .attr("space")
                        .is_some_and(|value| value.eq_ignore_ascii_case("preserve"));
                    if let Some(text) = read_text(tokens, tag.self_closing, preserve)? {
                        inlines.push(Inline::Text(Text::new(NodeMeta::synthesized(), text)));
                    }
                }
                "tab" => {
                    // A tab is not modelled as spacing; one literal space keeps
                    // the text projection honest and never creates empty text.
                    inlines.push(Inline::Text(Text::new(NodeMeta::synthesized(), " ")));
                    skip_element(tokens, tag.self_closing)?;
                }
                "br" | "cr" => {
                    inlines.push(Inline::LineBreak(LineBreak::new(
                        NodeMeta::synthesized(),
                        LineBreakKind::Hard,
                    )));
                    skip_element(tokens, tag.self_closing)?;
                }
                _ => skip_element(tokens, tag.self_closing)?,
            },
            Token::End(name) => {
                if name == "r" {
                    break;
                }
            }
            Token::Text(_) => {}
        }
    }

    out.extend(wrap_run(style, inlines));
    Ok(())
}

fn parse_run_properties(
    tokens: &mut Tokenizer<'_>,
    self_closing: bool,
    style: &mut RunStyle,
) -> Result<(), DocxError> {
    if self_closing {
        return Ok(());
    }
    while let Some(token) = tokens.next_token()? {
        match token {
            Token::Start(tag) => {
                match tag.name {
                    "b" => style.bold = on_off(tag.attr("val")),
                    "i" => style.italic = on_off(tag.attr("val")),
                    "u" => style.underline = on_off(tag.attr("val")),
                    "vertAlign" => {
                        style.vertical = match tag.attr("val") {
                            Some(value) if value.eq_ignore_ascii_case("superscript") => {
                                Some(Vertical::Superscript)
                            }
                            Some(value) if value.eq_ignore_ascii_case("subscript") => {
                                Some(Vertical::Subscript)
                            }
                            _ => None,
                        };
                    }
                    "rStyle" => {
                        if let Some(value) = tag.attr("val") {
                            let value = value.trim();
                            if value.eq_ignore_ascii_case("Strong") {
                                style.bold = true;
                            } else if value.eq_ignore_ascii_case("Emphasis") {
                                style.italic = true;
                            }
                            // `Hyperlink` is recognised but stays text: the
                            // relationship target is not available here.
                        }
                    }
                    _ => {}
                }
                skip_element(tokens, tag.self_closing)?;
            }
            Token::End(name) => {
                if name == "rPr" {
                    break;
                }
            }
            Token::Text(_) => {}
        }
    }
    Ok(())
}

/// Apply run properties as nested containers. The order is fixed so canonical
/// output is stable: Strong, Emphasis, Underline, then Superscript/Subscript.
fn wrap_run(style: RunStyle, inlines: Vec<Inline>) -> Vec<Inline> {
    if inlines.is_empty() {
        return inlines;
    }
    let mut wrapped = inlines;
    if let Some(vertical) = style.vertical {
        let children = wrapped;
        wrapped = vec![match vertical {
            Vertical::Superscript => {
                Inline::Superscript(Container::new(NodeMeta::synthesized(), children))
            }
            Vertical::Subscript => {
                Inline::Subscript(Container::new(NodeMeta::synthesized(), children))
            }
        }];
    }
    if style.underline {
        wrapped = vec![Inline::Underline(Container::new(
            NodeMeta::synthesized(),
            wrapped,
        ))];
    }
    if style.italic {
        wrapped = vec![Inline::Emphasis(Container::new(
            NodeMeta::synthesized(),
            wrapped,
        ))];
    }
    if style.bold {
        wrapped = vec![Inline::Strong(Container::new(
            NodeMeta::synthesized(),
            wrapped,
        ))];
    }
    wrapped
}

/// Read the text of a `w:t`. Without `xml:space="preserve"` Word treats edge
/// whitespace as insignificant; interior spaces always survive.
fn read_text(
    tokens: &mut Tokenizer<'_>,
    self_closing: bool,
    preserve: bool,
) -> Result<Option<String>, DocxError> {
    if self_closing {
        return Ok(None);
    }
    let mut text = String::new();
    while let Some(token) = tokens.next_token()? {
        match token {
            Token::Text(value) => text.push_str(&value),
            Token::Start(tag) => skip_element(tokens, tag.self_closing)?,
            Token::End(name) => {
                if name == "t" {
                    break;
                }
            }
        }
    }
    let text = if preserve {
        text
    } else {
        String::from(text.trim())
    };
    if text.is_empty() {
        Ok(None)
    } else {
        Ok(Some(text))
    }
}

fn parse_table(tokens: &mut Tokenizer<'_>, self_closing: bool) -> Result<Block, DocxError> {
    let mut rows: Vec<Row> = Vec::new();
    if !self_closing {
        while let Some(token) = tokens.next_token()? {
            match token {
                Token::Start(tag) if tag.name == "tr" => {
                    rows.push(parse_row(tokens, tag.self_closing)?);
                }
                Token::Start(tag) => skip_element(tokens, tag.self_closing)?,
                Token::End(name) => {
                    if name == "tbl" {
                        break;
                    }
                }
                Token::Text(_) => {}
            }
        }
    }

    // The grid width comes from the first row. Later rows are padded with
    // empty cells and longer rows are truncated, because DocIR tables are
    // rectangular and validation rejects a ragged grid.
    let mut width = rows.first().map_or(0, |row| row.cells.len());
    if width == 0 && !rows.is_empty() {
        width = 1;
    }
    for row in &mut rows {
        row.cells.truncate(width);
        while row.cells.len() < width {
            row.cells
                .push(Cell::new(NodeMeta::synthesized(), Vec::new()));
        }
    }

    let columns = (0..width)
        .map(|_| Column::new(NodeMeta::synthesized(), CellAlign::Default))
        .collect();
    Ok(Block::Table(Table::new(
        NodeMeta::synthesized(),
        columns,
        rows,
    )))
}

fn parse_row(tokens: &mut Tokenizer<'_>, self_closing: bool) -> Result<Row, DocxError> {
    let mut cells = Vec::new();
    let mut header = false;
    if !self_closing {
        while let Some(token) = tokens.next_token()? {
            match token {
                Token::Start(tag) if tag.name == "tc" => {
                    cells.push(parse_cell(tokens, tag.self_closing)?);
                }
                Token::Start(tag) if tag.name == "trPr" => {
                    header |= parse_row_properties(tokens, tag.self_closing)?;
                }
                Token::Start(tag) => skip_element(tokens, tag.self_closing)?,
                Token::End(name) => {
                    if name == "tr" {
                        break;
                    }
                }
                Token::Text(_) => {}
            }
        }
    }
    Ok(Row::new(NodeMeta::synthesized(), header, cells))
}

fn parse_row_properties(tokens: &mut Tokenizer<'_>, self_closing: bool) -> Result<bool, DocxError> {
    let mut header = false;
    if self_closing {
        return Ok(header);
    }
    while let Some(token) = tokens.next_token()? {
        match token {
            Token::Start(tag) => {
                if tag.name == "tblHeader" {
                    header = on_off(tag.attr("val"));
                }
                skip_element(tokens, tag.self_closing)?;
            }
            Token::End(name) => {
                if name == "trPr" {
                    break;
                }
            }
            Token::Text(_) => {}
        }
    }
    Ok(header)
}

fn parse_cell(tokens: &mut Tokenizer<'_>, self_closing: bool) -> Result<Cell, DocxError> {
    let blocks = if self_closing {
        Vec::new()
    } else {
        parse_children(tokens, "tc")?
    };
    Ok(Cell::new(NodeMeta::synthesized(), blocks))
}

/// Skip an unknown element and its subtree. A skipped element contributes
/// nothing and never aborts the document; this is the "unknown is not an
/// error" half of the front-end contract.
fn skip_element(tokens: &mut Tokenizer<'_>, self_closing: bool) -> Result<(), DocxError> {
    if self_closing {
        return Ok(());
    }
    let mut depth = 1usize;
    while let Some(token) = tokens.next_token()? {
        match token {
            Token::Start(tag) => {
                if !tag.self_closing {
                    depth += 1;
                }
            }
            Token::End(_) => {
                depth -= 1;
                if depth == 0 {
                    break;
                }
            }
            Token::Text(_) => {}
        }
    }
    Ok(())
}

/// Map a word style id to a DocIR heading level. `Title` is level 1; Word's
/// levels 7-9 clamp to 6 because DocIR headings stop at 6.
fn heading_level(style: &str) -> Option<u8> {
    let style = style.trim();
    if style.eq_ignore_ascii_case("title") {
        return Some(1);
    }
    let lower = style.to_ascii_lowercase();
    let suffix = lower.strip_prefix("heading")?.trim();
    let level: u8 = suffix.parse().ok()?;
    if (1..=9).contains(&level) {
        Some(level.min(6))
    } else {
        None
    }
}

fn on_off(value: Option<&str>) -> bool {
    value.is_none_or(|value| !is_off(value.trim()))
}

fn is_off(value: &str) -> bool {
    value == "0"
        || value.eq_ignore_ascii_case("false")
        || value.eq_ignore_ascii_case("off")
        || value.eq_ignore_ascii_case("none")
}

/// Parse `docProps/core.xml`. Metadata is best-effort: malformed XML yields
/// whatever was readable before the error, never a failed import.
pub(crate) fn parse_core_metadata(xml: &str) -> DocumentMeta {
    let mut meta = DocumentMeta::new();
    let mut title: Option<String> = None;
    let mut authors: Vec<String> = Vec::new();

    let mut tokens = Tokenizer::new(xml);
    while let Ok(Some(token)) = tokens.next_token() {
        match token {
            Token::Start(tag) if tag.name == "title" => {
                let text = read_element_text(&mut tokens, tag.self_closing, "title");
                let text = text.trim();
                if title.is_none() && !text.is_empty() {
                    title = Some(String::from(text));
                }
            }
            Token::Start(tag) if tag.name == "creator" => {
                let text = read_element_text(&mut tokens, tag.self_closing, "creator");
                for name in text.split(&[';', ','][..]) {
                    let name = name.trim();
                    if !name.is_empty() {
                        authors.push(String::from(name));
                    }
                }
            }
            _ => {}
        }
    }

    if let Some(title) = title {
        meta.title = Some(vec![Inline::Text(Text::new(
            NodeMeta::synthesized(),
            title.clone(),
        ))]);
        let mut cover = CoverData::new();
        cover.title = Some(title);
        meta.cover = Some(cover);
    }
    if !authors.is_empty() {
        meta.authors = authors.into_iter().map(Author::new).collect();
    }
    meta
}

fn read_element_text(tokens: &mut Tokenizer<'_>, self_closing: bool, name: &str) -> String {
    let mut text = String::new();
    if self_closing {
        return text;
    }
    loop {
        match tokens.next_token() {
            Ok(Some(Token::Text(value))) => text.push_str(&value),
            Ok(Some(Token::Start(tag))) => {
                if skip_element(tokens, tag.self_closing).is_err() {
                    break;
                }
            }
            Ok(Some(Token::End(end))) => {
                if end == name {
                    break;
                }
            }
            Ok(None) | Err(_) => break,
        }
    }
    text
}
