//! OOXML (WordprocessingML) to DocIR mapping.
//!
//! The walk is deliberately shallow: it understands the block and run
//! containers DocIR can represent, and skips everything else without failing.
//! Word properties are read only when they have a semantic DocIR counterpart;
//! fonts, spacing, and colours are ignored. Styles are resolved through
//! `word/styles.xml` when it is present, and drawings become figures.

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use soma_doc::block::{
    Block, Cell, CellAlign, Column, Figure, Heading, List, ListItem, Paragraph, Row, Table,
};
use soma_doc::inline::{Container, Image, Inline, LineBreak, LineBreakKind};
use soma_doc::meta::{Author, CoverData, DocumentMeta, ImageRef};
use soma_doc::node::NodeMeta;
use soma_doc::Text;

use crate::media::ImageIndex;
use crate::styles::{
    on_off, read_run_properties, CharFormat, RunFormat, RunProperties, Styles, Vertical,
};
use crate::xml::{Token, Tokenizer};
use crate::DocxError;

/// EMU per CSS pixel at 96 dpi: 914400 / 96.
const EMU_PER_PIXEL: u64 = 9525;

/// Parse the `w:body` of `word/document.xml`.
pub(crate) fn parse_blocks(
    xml: &str,
    styles: &Styles,
    images: &ImageIndex,
) -> Result<Vec<Block>, DocxError> {
    let mut tokens = Tokenizer::new(xml);
    loop {
        match tokens.next_token()? {
            None => return Err(DocxError::new("word/document.xml has no body")),
            Some(Token::Start(tag)) if tag.name == "body" => {
                if tag.self_closing {
                    return Ok(Vec::new());
                }
                return parse_children(&mut tokens, "body", styles, images);
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
fn parse_children(
    tokens: &mut Tokenizer<'_>,
    scope: &str,
    styles: &Styles,
    images: &ImageIndex,
) -> Result<Vec<Block>, DocxError> {
    let mut blocks = Vec::new();
    let mut pending: Option<PendingList> = None;
    let mut closed = false;

    while let Some(token) = tokens.next_token()? {
        match token {
            Token::Start(tag) if tag.name == "p" => {
                let paragraph = parse_paragraph(tokens, tag.self_closing, styles, images)?;
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
                            list.items
                                .push(ListItem::new(NodeMeta::synthesized(), paragraph.blocks));
                        }
                    }
                    None => {
                        flush_list(&mut blocks, &mut pending);
                        blocks.extend(paragraph.blocks);
                    }
                }
            }
            Token::Start(tag) if tag.name == "tbl" => {
                flush_list(&mut blocks, &mut pending);
                blocks.push(parse_table(tokens, tag.self_closing, styles, images)?);
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
    /// A paragraph can split around figures: text before the image, the figure,
    /// then text after it. A paragraph without images yields exactly one block.
    blocks: Vec<Block>,
    /// `Some(ordered)` when `w:numPr` was present.
    numbering: Option<bool>,
}

/// A resolved inline run, or an image lifted out to block level.
enum Piece {
    Run(RunSegment),
    Figure(ImageRef),
}

struct RunSegment {
    inlines: Vec<Inline>,
    /// Character + direct run properties. Paragraph-style properties are
    /// merged in when the paragraph's style is known.
    format: CharFormat,
}

/// Part of one `w:r`, before formatting is applied. A run can contain text on
/// both sides of a drawing, so images split it into segments.
enum RunPart {
    Inline(Inline),
    Figure(ImageRef),
}

fn parse_paragraph(
    tokens: &mut Tokenizer<'_>,
    self_closing: bool,
    styles: &Styles,
    images: &ImageIndex,
) -> Result<ParagraphResult, DocxError> {
    if self_closing {
        return Ok(ParagraphResult {
            blocks: vec![Block::Paragraph(Paragraph::new(
                NodeMeta::synthesized(),
                Vec::new(),
            ))],
            numbering: None,
        });
    }

    let mut style: Option<String> = None;
    let mut numbering: Option<bool> = None;
    let mut pieces: Vec<Piece> = Vec::new();

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
                "r" => parse_run(tokens, tag.self_closing, styles, images, &mut pieces)?,
                "hyperlink" => {
                    parse_hyperlink(tokens, tag.self_closing, styles, images, &mut pieces)?;
                }
                "drawing" | "pict" => {
                    if let Some(image) = parse_image(tokens, tag.self_closing, images)? {
                        pieces.push(Piece::Figure(image));
                    }
                }
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

    let heading = style.as_deref().and_then(|id| styles.heading_level(id));
    let paragraph_format = style
        .as_deref()
        .map_or_else(CharFormat::default, |id| styles.paragraph_format(id));

    let mut blocks = Vec::new();
    let mut pending: Vec<Inline> = Vec::new();
    for piece in pieces {
        match piece {
            Piece::Run(segment) => {
                let mut format = RunFormat::default();
                paragraph_format.overlay_run(&mut format);
                segment.format.overlay_run(&mut format);
                pending.extend(wrap_run(format, segment.inlines));
            }
            Piece::Figure(source) => {
                flush_text(&mut blocks, heading, &mut pending);
                let image = Image::new(NodeMeta::synthesized(), source);
                blocks.push(Block::Figure(Figure::new(NodeMeta::synthesized(), image)));
            }
        }
    }
    flush_text(&mut blocks, heading, &mut pending);

    if blocks.is_empty() {
        // An empty paragraph still occupies a block. Keep its heading identity
        // rather than dropping the document structure.
        let inlines = Vec::new();
        blocks.push(match heading {
            Some(level) => Block::Heading(Heading::new(NodeMeta::synthesized(), level, inlines)),
            None => Block::Paragraph(Paragraph::new(NodeMeta::synthesized(), inlines)),
        });
    }

    Ok(ParagraphResult { blocks, numbering })
}

fn flush_text(blocks: &mut Vec<Block>, heading: Option<u8>, pending: &mut Vec<Inline>) {
    if pending.is_empty() {
        return;
    }
    let inlines = core::mem::take(pending);
    blocks.push(match heading {
        Some(level) => Block::Heading(Heading::new(NodeMeta::synthesized(), level, inlines)),
        None => Block::Paragraph(Paragraph::new(NodeMeta::synthesized(), inlines)),
    });
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
/// `document.xml.rels`, which is read for images only, so creating a `Link`
/// with a guessed URL would be worse than keeping the text.
fn parse_hyperlink(
    tokens: &mut Tokenizer<'_>,
    self_closing: bool,
    styles: &Styles,
    images: &ImageIndex,
    pieces: &mut Vec<Piece>,
) -> Result<(), DocxError> {
    if self_closing {
        return Ok(());
    }
    while let Some(token) = tokens.next_token()? {
        match token {
            Token::Start(tag) => {
                if tag.name == "r" {
                    parse_run(tokens, tag.self_closing, styles, images, pieces)?;
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

fn parse_run(
    tokens: &mut Tokenizer<'_>,
    self_closing: bool,
    styles: &Styles,
    images: &ImageIndex,
    pieces: &mut Vec<Piece>,
) -> Result<(), DocxError> {
    if self_closing {
        return Ok(());
    }

    let mut properties = RunProperties::default();
    let mut parts: Vec<RunPart> = Vec::new();

    while let Some(token) = tokens.next_token()? {
        match token {
            Token::Start(tag) => match tag.name {
                "rPr" => properties = read_run_properties(tokens, tag.self_closing)?,
                "t" => {
                    let preserve = tag
                        .attr("space")
                        .is_some_and(|value| value.eq_ignore_ascii_case("preserve"));
                    if let Some(text) = read_text(tokens, tag.self_closing, preserve)? {
                        parts.push(RunPart::Inline(Inline::Text(Text::new(
                            NodeMeta::synthesized(),
                            text,
                        ))));
                    }
                }
                "tab" => {
                    // A tab is not modelled as spacing; one literal space keeps
                    // the text projection honest and never creates empty text.
                    parts.push(RunPart::Inline(Inline::Text(Text::new(
                        NodeMeta::synthesized(),
                        " ",
                    ))));
                    skip_element(tokens, tag.self_closing)?;
                }
                "br" | "cr" => {
                    parts.push(RunPart::Inline(Inline::LineBreak(LineBreak::new(
                        NodeMeta::synthesized(),
                        LineBreakKind::Hard,
                    ))));
                    skip_element(tokens, tag.self_closing)?;
                }
                "drawing" | "pict" => {
                    if let Some(image) = parse_image(tokens, tag.self_closing, images)? {
                        parts.push(RunPart::Figure(image));
                    }
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

    // A paragraph style is the lowest layer, a character style sits above it,
    // and properties written on the run win over both.
    let mut format = match properties.style_id.as_deref() {
        Some(id) => styles.character_format(id),
        None => CharFormat::default(),
    };
    format.overlay(properties.format);

    let mut inlines = Vec::new();
    for part in parts {
        match part {
            RunPart::Inline(inline) => inlines.push(inline),
            RunPart::Figure(source) => {
                if !inlines.is_empty() {
                    pieces.push(Piece::Run(RunSegment {
                        inlines: core::mem::take(&mut inlines),
                        format,
                    }));
                }
                pieces.push(Piece::Figure(source));
            }
        }
    }
    if !inlines.is_empty() {
        pieces.push(Piece::Run(RunSegment { inlines, format }));
    }
    Ok(())
}

/// Scan a `w:drawing` or `w:pict` subtree for an image relationship and an
/// extent. The first relationship wins; nested `mc:AlternateContent` fallbacks
/// therefore cannot duplicate an image.
fn parse_image(
    tokens: &mut Tokenizer<'_>,
    self_closing: bool,
    images: &ImageIndex,
) -> Result<Option<ImageRef>, DocxError> {
    if self_closing {
        return Ok(None);
    }

    let mut relationship: Option<String> = None;
    let mut width: Option<u32> = None;
    let mut height: Option<u32> = None;
    let mut depth = 1usize;

    while let Some(token) = tokens.next_token()? {
        match token {
            Token::Start(tag) => {
                match tag.name {
                    "extent" => {
                        width = pixels(tag.attr("cx")).or(width);
                        height = pixels(tag.attr("cy")).or(height);
                    }
                    // DrawingML and legacy VML spell the relationship attribute
                    // differently; both arrive as local name `id` after the
                    // tokenizer drops the namespace prefix.
                    "blip" if relationship.is_none() => {
                        relationship = tag
                            .attr("embed")
                            .or_else(|| tag.attr("id"))
                            .map(String::from);
                    }
                    "imagedata" if relationship.is_none() => {
                        relationship = tag
                            .attr("id")
                            .or_else(|| tag.attr("embed"))
                            .map(String::from);
                    }
                    _ => {}
                }
                if !tag.self_closing {
                    depth += 1;
                }
            }
            Token::End(_) => {
                if depth <= 1 {
                    break;
                }
                depth -= 1;
            }
            Token::Text(_) => {}
        }
    }

    Ok(resolve_image(
        relationship.as_deref(),
        width,
        height,
        images,
    ))
}

fn resolve_image(
    relationship: Option<&str>,
    width: Option<u32>,
    height: Option<u32>,
    images: &ImageIndex,
) -> Option<ImageRef> {
    let source = images.get(relationship?)?;
    let mut image = ImageRef::new(source);
    if let (Some(width), Some(height)) = (width, height) {
        image = image.with_intrinsic(width, height);
    }
    Some(image)
}

/// Convert a `wp:extent` coordinate (EMU) to pixels at 96 dpi. A missing or
/// non-positive extent leaves the intrinsic size unknown.
fn pixels(value: Option<&str>) -> Option<u32> {
    let emu: u64 = value?.trim().parse().ok()?;
    let px = emu / EMU_PER_PIXEL;
    if px == 0 {
        return None;
    }
    u32::try_from(px).ok()
}

/// Apply run properties as nested containers. The order is fixed so canonical
/// output is stable: Strong, Emphasis, Underline, then Superscript/Subscript.
fn wrap_run(style: RunFormat, inlines: Vec<Inline>) -> Vec<Inline> {
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

fn parse_table(
    tokens: &mut Tokenizer<'_>,
    self_closing: bool,
    styles: &Styles,
    images: &ImageIndex,
) -> Result<Block, DocxError> {
    let mut rows: Vec<Row> = Vec::new();
    if !self_closing {
        while let Some(token) = tokens.next_token()? {
            match token {
                Token::Start(tag) if tag.name == "tr" => {
                    rows.push(parse_row(tokens, tag.self_closing, styles, images)?);
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

fn parse_row(
    tokens: &mut Tokenizer<'_>,
    self_closing: bool,
    styles: &Styles,
    images: &ImageIndex,
) -> Result<Row, DocxError> {
    let mut cells = Vec::new();
    let mut header = false;
    if !self_closing {
        while let Some(token) = tokens.next_token()? {
            match token {
                Token::Start(tag) if tag.name == "tc" => {
                    cells.push(parse_cell(tokens, tag.self_closing, styles, images)?);
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

fn parse_cell(
    tokens: &mut Tokenizer<'_>,
    self_closing: bool,
    styles: &Styles,
    images: &ImageIndex,
) -> Result<Cell, DocxError> {
    let blocks = if self_closing {
        Vec::new()
    } else {
        parse_children(tokens, "tc", styles, images)?
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
