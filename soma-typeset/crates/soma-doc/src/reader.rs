//! Reader for the canonical JSON emitted by [`crate::canonical`].
//!
//! The writer is the contract: this parser consumes the field names and field
//! order it emits, accepts only integers (never floats), decodes JSON string
//! escapes including surrogate pairs, and reports a typed [`ReadError`] for
//! unknown fields and malformed input. Insignificant whitespace between tokens
//! is tolerated; unknown fields are not.
//!
//! It is intentionally not a general JSON parser: no map-backed document is
//! built, and nothing but the writer's shape is accepted. Reading is
//! structural, not validating. A document parsed from untrusted bytes should
//! still go through [`crate::Document::validate`].

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::attrs::Attrs;
use crate::block::{
    Block, Cell, CellAlign, CodeBlock, Column, GeneratedBlock, GeneratedKind, Heading, List,
    ListItem, MathBlock, Paragraph, RawBlock, Row, Section, Table,
};
use crate::doc::{Document, FootnoteDef, DOC_IR_VERSION, SCHEMA};
use crate::ids::{NodeId, SourceId, SourceInfo, SourceKind, SourceMap, SourceSpan};
use crate::inline::{
    Code, Container, FootnoteRef, Image, IndexRef, Inline, LineBreak, LineBreakKind, Link,
    MathInline, RawInline, Ruby, Text,
};
use crate::meta::{
    Author, CoverData, Date, DocumentMeta, ImageIntrinsic, ImageRef, MetaField, TemplateRef,
};
use crate::node::NodeMeta;
use crate::raw::{RawCapability, RawContent};

/// One failure while reading canonical DocIR.
///
/// `path` is a dotted/indexed path that identifies the offending node or field;
/// an unknown-field error appends the field name to the enclosing path.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReadError {
    /// Best-effort node or field path.
    pub path: String,
    /// Short, stable reason. Not localized.
    pub reason: &'static str,
    /// Byte offset in the input where the parser stopped.
    pub offset: usize,
}

impl ReadError {
    pub fn new(path: impl Into<String>, reason: &'static str, offset: usize) -> Self {
        Self {
            path: path.into(),
            reason,
            offset,
        }
    }
}

impl core::fmt::Display for ReadError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}: {} (byte {})", self.path, self.reason, self.offset)
    }
}

impl core::error::Error for ReadError {}

/// Parse a `Document` from canonical DocIR JSON.
///
/// This is the inverse of [`crate::Document::canonical_json`] for the shape the
/// writer emits. The document is returned unvalidated.
pub fn read_document(input: &str) -> Result<Document, ReadError> {
    let mut parser = Parser::new(input);
    let document = parser.document()?;
    parser.finish()?;
    Ok(document)
}

struct Parser<'a> {
    input: &'a str,
    pos: usize,
}

impl<'a> Parser<'a> {
    const fn new(input: &'a str) -> Self {
        Self { input, pos: 0 }
    }

    fn error<T>(&self, path: &str, reason: &'static str) -> Result<T, ReadError> {
        Err(ReadError::new(path, reason, self.pos))
    }

    fn peek(&self) -> Option<u8> {
        self.input.as_bytes().get(self.pos).copied()
    }

    fn skip_ws(&mut self) {
        while let Some(byte) = self.peek() {
            if matches!(byte, b' ' | b'\t' | b'\n' | b'\r') {
                self.pos += 1;
            } else {
                break;
            }
        }
    }

    fn eat_literal(&mut self, literal: &str) -> bool {
        if self.input[self.pos..].starts_with(literal) {
            self.pos += literal.len();
            true
        } else {
            false
        }
    }

    fn expect_byte(&mut self, byte: u8, path: &str, reason: &'static str) -> Result<(), ReadError> {
        self.skip_ws();
        if self.peek() == Some(byte) {
            self.pos += 1;
            Ok(())
        } else {
            self.error(path, reason)
        }
    }

    fn open_object(&mut self, path: &str) -> Result<(), ReadError> {
        self.expect_byte(b'{', path, "expected `{`")
    }

    fn close_object(&mut self, path: &str) -> Result<(), ReadError> {
        self.skip_ws();
        match self.peek() {
            Some(b'}') => {
                self.pos += 1;
                Ok(())
            }
            Some(b',') => {
                // The canonical writer never emits more fields than the ones
                // already consumed; name the extra field instead of reporting a
                // generic punctuation error.
                self.pos += 1;
                self.skip_ws();
                let start = self.pos;
                let field = self.string(path)?;
                Err(ReadError::new(
                    format!("{path}.{field}"),
                    "unknown field",
                    start,
                ))
            }
            _ => self.error(path, "expected `}`"),
        }
    }

    fn comma(&mut self, path: &str) -> Result<(), ReadError> {
        self.expect_byte(b',', path, "expected `,`")
    }

    /// Consume the next field name and colon, requiring canonical order.
    fn field(&mut self, expected: &str, path: &str) -> Result<(), ReadError> {
        self.skip_ws();
        let start = self.pos;
        let actual = self.string(path)?;
        if actual != expected {
            return Err(ReadError::new(
                format!("{path}.{actual}"),
                "unknown field",
                start,
            ));
        }
        self.expect_byte(b':', path, "expected `:`")
    }

    fn finish(&mut self) -> Result<(), ReadError> {
        self.skip_ws();
        if self.pos == self.input.len() {
            Ok(())
        } else {
            self.error("document", "trailing input")
        }
    }

    fn string(&mut self, path: &str) -> Result<String, ReadError> {
        self.skip_ws();
        self.expect_byte(b'"', path, "expected string")?;
        let mut out = String::new();
        loop {
            let byte = match self.peek() {
                Some(byte) => byte,
                None => return self.error(path, "unterminated string"),
            };
            match byte {
                b'"' => {
                    self.pos += 1;
                    return Ok(out);
                }
                b'\\' => {
                    self.pos += 1;
                    self.escape(&mut out, path)?;
                }
                0x00..=0x1f => return self.error(path, "unescaped control character"),
                _ => {
                    let character = self.input[self.pos..]
                        .chars()
                        .next()
                        .expect("parser position is on a UTF-8 boundary");
                    out.push(character);
                    self.pos += character.len_utf8();
                }
            }
        }
    }

    fn escape(&mut self, out: &mut String, path: &str) -> Result<(), ReadError> {
        let byte = match self.peek() {
            Some(byte) => byte,
            None => return self.error(path, "unterminated string"),
        };
        self.pos += 1;
        match byte {
            b'"' => out.push('"'),
            b'\\' => out.push('\\'),
            b'/' => out.push('/'),
            b'b' => out.push('\u{8}'),
            b'f' => out.push('\u{c}'),
            b'n' => out.push('\n'),
            b'r' => out.push('\r'),
            b't' => out.push('\t'),
            b'u' => {
                let first = self.hex4(path)?;
                let scalar = if (0xd800..=0xdbff).contains(&first) {
                    if self.peek() != Some(b'\\') {
                        return self.error(path, "invalid surrogate pair");
                    }
                    self.pos += 1;
                    if self.peek() != Some(b'u') {
                        return self.error(path, "invalid surrogate pair");
                    }
                    self.pos += 1;
                    let second = self.hex4(path)?;
                    if !(0xdc00..=0xdfff).contains(&second) {
                        return self.error(path, "invalid surrogate pair");
                    }
                    0x1_0000 + ((first - 0xd800) << 10) + (second - 0xdc00)
                } else if (0xdc00..=0xdfff).contains(&first) {
                    return self.error(path, "invalid surrogate pair");
                } else {
                    first
                };
                match char::from_u32(scalar) {
                    Some(character) => out.push(character),
                    None => return self.error(path, "invalid unicode escape"),
                }
            }
            _ => return self.error(path, "invalid escape"),
        }
        Ok(())
    }

    fn hex4(&mut self, path: &str) -> Result<u32, ReadError> {
        let mut value = 0u32;
        for _ in 0..4 {
            let byte = match self.peek() {
                Some(byte) => byte,
                None => return self.error(path, "unterminated string"),
            };
            let digit = match byte {
                b'0'..=b'9' => u32::from(byte - b'0'),
                b'a'..=b'f' => u32::from(byte - b'a') + 10,
                b'A'..=b'F' => u32::from(byte - b'A') + 10,
                _ => return self.error(path, "invalid unicode escape"),
            };
            value = value * 16 + digit;
            self.pos += 1;
        }
        Ok(value)
    }

    fn integer(&mut self, path: &str) -> Result<i64, ReadError> {
        self.skip_ws();
        let start = self.pos;
        let negative = self.peek() == Some(b'-');
        if negative {
            self.pos += 1;
        }
        let digits_start = self.pos;
        let mut value = 0i64;
        while let Some(byte) = self.peek() {
            if !byte.is_ascii_digit() {
                break;
            }
            let digit = i64::from(byte - b'0');
            match value
                .checked_mul(10)
                .and_then(|value| value.checked_add(digit))
            {
                Some(next) => value = next,
                None => return Err(ReadError::new(path, "integer out of range", start)),
            }
            self.pos += 1;
        }
        if self.pos == digits_start {
            return Err(ReadError::new(path, "expected integer", start));
        }
        Ok(if negative { -value } else { value })
    }

    fn integer_as<T: TryFrom<i64>>(&mut self, path: &str) -> Result<T, ReadError> {
        let start = self.pos;
        let value = self.integer(path)?;
        T::try_from(value).map_err(|_| ReadError::new(path, "integer out of range", start))
    }

    fn boolean(&mut self, path: &str) -> Result<bool, ReadError> {
        self.skip_ws();
        if self.eat_literal("true") {
            Ok(true)
        } else if self.eat_literal("false") {
            Ok(false)
        } else {
            self.error(path, "expected boolean")
        }
    }

    fn null_or<T>(
        &mut self,
        _path: &str,
        parse: impl FnOnce(&mut Self) -> Result<T, ReadError>,
    ) -> Result<Option<T>, ReadError> {
        self.skip_ws();
        if self.eat_literal("null") {
            Ok(None)
        } else {
            Ok(Some(parse(self)?))
        }
    }

    fn array<T>(
        &mut self,
        path: &str,
        mut parse: impl FnMut(&mut Self, &str) -> Result<T, ReadError>,
    ) -> Result<Vec<T>, ReadError> {
        self.skip_ws();
        self.expect_byte(b'[', path, "expected `[`")?;
        let mut items = Vec::new();
        self.skip_ws();
        if self.peek() == Some(b']') {
            self.pos += 1;
            return Ok(items);
        }
        loop {
            let item_path = format!("{path}[{}]", items.len());
            items.push(parse(self, &item_path)?);
            self.skip_ws();
            match self.peek() {
                Some(b',') => self.pos += 1,
                Some(b']') => {
                    self.pos += 1;
                    return Ok(items);
                }
                _ => return self.error(path, "expected `,` or `]`"),
            }
        }
    }

    fn document(&mut self) -> Result<Document, ReadError> {
        let path = "document";
        self.open_object(path)?;
        self.field("schema", path)?;
        let schema = self.string(path)?;
        if schema != SCHEMA {
            return self.error("document.schema", "unsupported schema");
        }
        self.comma(path)?;
        self.field("docIrVersion", path)?;
        let version: u16 = self.integer_as(path)?;
        if version != DOC_IR_VERSION {
            return self.error("document.docIrVersion", "unsupported docIrVersion");
        }
        self.comma(path)?;
        self.field("nodeCount", path)?;
        let node_count: u32 = self.integer_as(path)?;
        self.comma(path)?;
        self.field("meta", path)?;
        let meta = self.meta(path)?;
        self.comma(path)?;
        self.field("blocks", path)?;
        let blocks = self.blocks(path)?;
        self.comma(path)?;
        self.field("footnotes", path)?;
        let footnotes = self.array(path, |parser, item_path| parser.footnote(item_path))?;
        self.comma(path)?;
        self.field("sourceMap", path)?;
        let source_map = self.source_map(path)?;
        self.close_object(path)?;

        let document = Document {
            meta,
            blocks,
            footnotes,
            source_map,
        };
        if document.node_count() != node_count {
            return self.error("document.nodeCount", "node count mismatch");
        }
        Ok(document)
    }

    fn meta(&mut self, path: &str) -> Result<DocumentMeta, ReadError> {
        self.open_object(path)?;
        self.field("title", path)?;
        let title = self.null_or(path, |parser| parser.inlines(path))?;
        self.comma(path)?;
        self.field("authors", path)?;
        let authors = self.array(path, |parser, item_path| parser.author(item_path))?;
        self.comma(path)?;
        self.field("language", path)?;
        let language = self.null_or(path, |parser| parser.string(path))?;
        self.comma(path)?;
        self.field("date", path)?;
        let date = self.null_or(path, |parser| parser.date(path))?;
        self.comma(path)?;
        self.field("keywords", path)?;
        let keywords = self.array(path, |parser, item_path| parser.string(item_path))?;
        self.comma(path)?;
        self.field("template", path)?;
        let template = self.null_or(path, |parser| parser.template(path))?;
        self.comma(path)?;
        self.field("cover", path)?;
        let cover = self.null_or(path, |parser| parser.cover(path))?;
        self.comma(path)?;
        self.field("fields", path)?;
        let fields = self.array(path, |parser, item_path| parser.meta_field(item_path))?;
        self.close_object(path)?;
        Ok(DocumentMeta {
            title,
            authors,
            language,
            date,
            keywords,
            template,
            cover,
            fields,
        })
    }

    fn author(&mut self, path: &str) -> Result<Author, ReadError> {
        self.open_object(path)?;
        self.field("name", path)?;
        let name = self.string(path)?;
        self.comma(path)?;
        self.field("affiliation", path)?;
        let affiliation = self.null_or(path, |parser| parser.string(path))?;
        self.comma(path)?;
        self.field("email", path)?;
        let email = self.null_or(path, |parser| parser.string(path))?;
        self.comma(path)?;
        self.field("orcid", path)?;
        let orcid = self.null_or(path, |parser| parser.string(path))?;
        self.close_object(path)?;
        Ok(Author {
            name,
            affiliation,
            email,
            orcid,
        })
    }

    fn date(&mut self, path: &str) -> Result<Date, ReadError> {
        self.open_object(path)?;
        self.field("year", path)?;
        let year: i32 = self.integer_as(path)?;
        self.comma(path)?;
        self.field("month", path)?;
        let month: u8 = self.integer_as(path)?;
        self.comma(path)?;
        self.field("day", path)?;
        let day: u8 = self.integer_as(path)?;
        self.close_object(path)?;
        Ok(Date { year, month, day })
    }

    fn template(&mut self, path: &str) -> Result<TemplateRef, ReadError> {
        self.open_object(path)?;
        self.field("name", path)?;
        let name = self.string(path)?;
        self.comma(path)?;
        self.field("version", path)?;
        let version = self.null_or(path, |parser| parser.string(path))?;
        self.close_object(path)?;
        Ok(TemplateRef { name, version })
    }

    fn cover(&mut self, path: &str) -> Result<CoverData, ReadError> {
        self.open_object(path)?;
        self.field("title", path)?;
        let title = self.null_or(path, |parser| parser.string(path))?;
        self.comma(path)?;
        self.field("subtitle", path)?;
        let subtitle = self.null_or(path, |parser| parser.string(path))?;
        self.comma(path)?;
        self.field("authors", path)?;
        let authors = self.array(path, |parser, item_path| parser.string(item_path))?;
        self.comma(path)?;
        self.field("date", path)?;
        let date = self.null_or(path, |parser| parser.string(path))?;
        self.comma(path)?;
        self.field("logo", path)?;
        let logo = self.null_or(path, |parser| parser.image_ref(path))?;
        self.comma(path)?;
        self.field("fields", path)?;
        let fields = self.array(path, |parser, item_path| parser.meta_field(item_path))?;
        self.close_object(path)?;
        Ok(CoverData {
            title,
            subtitle,
            authors,
            date,
            logo,
            fields,
        })
    }

    fn meta_field(&mut self, path: &str) -> Result<MetaField, ReadError> {
        self.open_object(path)?;
        self.field("key", path)?;
        let key = self.string(path)?;
        self.comma(path)?;
        self.field("value", path)?;
        let value = self.string(path)?;
        self.close_object(path)?;
        Ok(MetaField { key, value })
    }

    fn image_ref(&mut self, path: &str) -> Result<ImageRef, ReadError> {
        self.open_object(path)?;
        self.field("src", path)?;
        let src = self.string(path)?;
        let image = self.image_ref_tail(path, src)?;
        self.close_object(path)?;
        Ok(image)
    }

    /// Fields shared by a bare `ImageRef` object and an image node. The caller
    /// has already consumed `src`; this consumes the leading comma.
    fn image_ref_tail(&mut self, path: &str, src: String) -> Result<ImageRef, ReadError> {
        self.comma(path)?;
        self.field("alt", path)?;
        let alt = self.null_or(path, |parser| parser.string(path))?;
        self.comma(path)?;
        self.field("title", path)?;
        let title = self.null_or(path, |parser| parser.string(path))?;
        self.comma(path)?;
        self.field("intrinsic", path)?;
        let intrinsic = self.null_or(path, |parser| parser.intrinsic(path))?;
        Ok(ImageRef {
            src,
            alt,
            title,
            intrinsic,
        })
    }

    fn intrinsic(&mut self, path: &str) -> Result<ImageIntrinsic, ReadError> {
        self.open_object(path)?;
        self.field("widthPx", path)?;
        let width_px: u32 = self.integer_as(path)?;
        self.comma(path)?;
        self.field("heightPx", path)?;
        let height_px: u32 = self.integer_as(path)?;
        self.close_object(path)?;
        Ok(ImageIntrinsic {
            width_px,
            height_px,
        })
    }

    fn source_map(&mut self, path: &str) -> Result<SourceMap, ReadError> {
        let entries = self.array(path, |parser, item_path| parser.source_info(item_path))?;
        Ok(SourceMap { entries })
    }

    fn source_info(&mut self, path: &str) -> Result<SourceInfo, ReadError> {
        self.open_object(path)?;
        self.field("id", path)?;
        let id: u16 = self.integer_as(path)?;
        self.comma(path)?;
        self.field("name", path)?;
        let name = self.string(path)?;
        self.comma(path)?;
        self.field("kind", path)?;
        let kind = self.kind(path)?;
        self.close_object(path)?;
        Ok(SourceInfo {
            id: SourceId(id),
            name,
            kind,
        })
    }

    fn footnote(&mut self, path: &str) -> Result<FootnoteDef, ReadError> {
        self.open_object(path)?;
        self.expect_kind("footnoteDef", path)?;
        self.comma(path)?;
        let meta = self.node_head(path)?;
        self.comma(path)?;
        self.field("label", path)?;
        let label = self.string(path)?;
        self.comma(path)?;
        self.field("blocks", path)?;
        let blocks = self.blocks(path)?;
        self.close_object(path)?;
        Ok(FootnoteDef {
            meta,
            label,
            blocks,
        })
    }

    fn node_head(&mut self, path: &str) -> Result<NodeMeta, ReadError> {
        self.field("id", path)?;
        let id = NodeId(self.integer_as(path)?);
        self.comma(path)?;
        self.field("span", path)?;
        self.open_object(path)?;
        self.field("source", path)?;
        let source = SourceId(self.integer_as(path)?);
        self.comma(path)?;
        self.field("start", path)?;
        let start: u32 = self.integer_as(path)?;
        self.comma(path)?;
        self.field("end", path)?;
        let end: u32 = self.integer_as(path)?;
        self.close_object(path)?;
        self.comma(path)?;
        self.field("attrs", path)?;
        self.open_object(path)?;
        self.field("role", path)?;
        let role = self.null_or(path, |parser| parser.string(path))?;
        self.comma(path)?;
        self.field("classes", path)?;
        let classes = self.array(path, |parser, item_path| parser.string(item_path))?;
        self.comma(path)?;
        self.field("anchor", path)?;
        let anchor = self.null_or(path, |parser| parser.string(path))?;
        self.comma(path)?;
        self.field("lang", path)?;
        let lang = self.null_or(path, |parser| parser.string(path))?;
        self.close_object(path)?;
        Ok(NodeMeta {
            id,
            span: SourceSpan { source, start, end },
            attrs: Attrs {
                role,
                classes,
                anchor,
                lang,
            },
        })
    }

    /// Consume `"kind":"<expected>"` after the opening brace of a part object.
    fn expect_kind(&mut self, expected: &str, path: &str) -> Result<(), ReadError> {
        self.field("kind", path)?;
        let kind = self.string(path)?;
        if kind != expected {
            return self.error(path, "unknown kind");
        }
        Ok(())
    }

    fn blocks(&mut self, path: &str) -> Result<Vec<Block>, ReadError> {
        self.array(path, |parser, item_path| parser.block(item_path))
    }

    fn block(&mut self, path: &str) -> Result<Block, ReadError> {
        self.open_object(path)?;
        self.field("kind", path)?;
        let kind = self.string(path)?;
        self.comma(path)?;
        let meta = self.node_head(path)?;

        let block = match kind.as_str() {
            "paragraph" => {
                self.comma(path)?;
                self.field("inlines", path)?;
                Block::Paragraph(Paragraph::new(meta, self.inlines(path)?))
            }
            "heading" => {
                self.comma(path)?;
                self.field("level", path)?;
                let level: u8 = self.integer_as(path)?;
                self.comma(path)?;
                self.field("inlines", path)?;
                Block::Heading(Heading::new(meta, level, self.inlines(path)?))
            }
            "list" => {
                self.comma(path)?;
                self.field("ordered", path)?;
                let ordered = self.boolean(path)?;
                self.comma(path)?;
                self.field("start", path)?;
                let start: u32 = self.integer_as(path)?;
                self.comma(path)?;
                self.field("tight", path)?;
                let tight = self.boolean(path)?;
                self.comma(path)?;
                self.field("items", path)?;
                let items = self.array(path, |parser, item_path| parser.list_item(item_path))?;
                Block::List(List {
                    meta,
                    ordered,
                    start,
                    tight,
                    items,
                })
            }
            "blockQuote" => {
                self.comma(path)?;
                self.field("blocks", path)?;
                Block::BlockQuote(crate::block::BlockQuote::new(meta, self.blocks(path)?))
            }
            "codeBlock" => {
                self.comma(path)?;
                self.field("lang", path)?;
                let lang = self.null_or(path, |parser| parser.string(path))?;
                self.comma(path)?;
                self.field("text", path)?;
                let value = self.string(path)?;
                Block::CodeBlock(CodeBlock { meta, lang, value })
            }
            "mathBlock" => {
                self.comma(path)?;
                self.field("source", path)?;
                let source = self.string(path)?;
                Block::MathBlock(MathBlock { meta, source })
            }
            "figure" => {
                self.comma(path)?;
                self.field("image", path)?;
                let image = self.image_node(path)?;
                self.comma(path)?;
                self.field("caption", path)?;
                let caption = self.inlines(path)?;
                self.comma(path)?;
                self.field("label", path)?;
                let label = self.null_or(path, |parser| parser.string(path))?;
                Block::Figure(crate::block::Figure {
                    meta,
                    image,
                    caption,
                    label,
                })
            }
            "table" => {
                self.comma(path)?;
                self.field("columns", path)?;
                let columns = self.array(path, |parser, item_path| parser.column(item_path))?;
                self.comma(path)?;
                self.field("caption", path)?;
                let caption = self.inlines(path)?;
                self.comma(path)?;
                self.field("label", path)?;
                let label = self.null_or(path, |parser| parser.string(path))?;
                self.comma(path)?;
                self.field("rows", path)?;
                let rows = self.array(path, |parser, item_path| parser.row(item_path))?;
                Block::Table(Table {
                    meta,
                    columns,
                    caption,
                    label,
                    rows,
                })
            }
            "thematicBreak" => Block::ThematicBreak(crate::block::ThematicBreak { meta }),
            "pageBreak" => Block::PageBreak(crate::block::PageBreak { meta }),
            "section" => {
                self.comma(path)?;
                self.field("level", path)?;
                let level: u8 = self.integer_as(path)?;
                self.comma(path)?;
                self.field("numbered", path)?;
                let numbered = self.boolean(path)?;
                self.comma(path)?;
                self.field("pageTemplate", path)?;
                let page_template = self.null_or(path, |parser| parser.string(path))?;
                self.comma(path)?;
                self.field("title", path)?;
                let title = self.null_or(path, |parser| parser.inlines(path))?;
                Block::Section(Section {
                    meta,
                    level,
                    numbered,
                    page_template,
                    title,
                })
            }
            "generated" => {
                self.comma(path)?;
                self.field("generatedKind", path)?;
                let generated_kind = self.generated_kind(path)?;
                self.comma(path)?;
                self.field("slot", path)?;
                let slot = self.null_or(path, |parser| parser.string(path))?;
                self.comma(path)?;
                self.field("blocks", path)?;
                let blocks = self.blocks(path)?;
                Block::Generated(GeneratedBlock {
                    meta,
                    kind: generated_kind,
                    slot,
                    blocks,
                })
            }
            "raw" => Block::Raw(RawBlock::new(meta, self.raw_content(path)?)),
            _ => return self.error(path, "unknown kind"),
        };

        self.close_object(path)?;
        Ok(block)
    }

    fn list_item(&mut self, path: &str) -> Result<ListItem, ReadError> {
        self.open_object(path)?;
        self.expect_kind("listItem", path)?;
        self.comma(path)?;
        let meta = self.node_head(path)?;
        self.comma(path)?;
        self.field("blocks", path)?;
        let blocks = self.blocks(path)?;
        self.close_object(path)?;
        Ok(ListItem { meta, blocks })
    }

    fn column(&mut self, path: &str) -> Result<Column, ReadError> {
        self.open_object(path)?;
        self.expect_kind("column", path)?;
        self.comma(path)?;
        let meta = self.node_head(path)?;
        self.comma(path)?;
        self.field("align", path)?;
        let (align, share) = self.column_align(path)?;
        self.close_object(path)?;
        Ok(Column { meta, align, share })
    }

    fn row(&mut self, path: &str) -> Result<Row, ReadError> {
        self.open_object(path)?;
        self.expect_kind("row", path)?;
        self.comma(path)?;
        let meta = self.node_head(path)?;
        self.comma(path)?;
        self.field("header", path)?;
        let header = self.boolean(path)?;
        self.comma(path)?;
        self.field("cells", path)?;
        let cells = self.array(path, |parser, item_path| parser.cell(item_path))?;
        self.close_object(path)?;
        Ok(Row {
            meta,
            header,
            cells,
        })
    }

    fn cell(&mut self, path: &str) -> Result<Cell, ReadError> {
        self.open_object(path)?;
        self.expect_kind("cell", path)?;
        self.comma(path)?;
        let meta = self.node_head(path)?;
        self.comma(path)?;
        self.field("colspan", path)?;
        let colspan: u16 = self.integer_as(path)?;
        self.comma(path)?;
        self.field("rowspan", path)?;
        let rowspan: u16 = self.integer_as(path)?;
        self.comma(path)?;
        self.field("blocks", path)?;
        let blocks = self.blocks(path)?;
        self.close_object(path)?;
        Ok(Cell {
            meta,
            colspan,
            rowspan,
            blocks,
        })
    }

    /// An image node nested inside a figure: `{"kind":"image", <head>, ...}`.
    fn image_node(&mut self, path: &str) -> Result<Image, ReadError> {
        self.open_object(path)?;
        self.expect_kind("image", path)?;
        self.comma(path)?;
        let meta = self.node_head(path)?;
        self.comma(path)?;
        self.field("src", path)?;
        let src = self.string(path)?;
        let source = self.image_ref_tail(path, src)?;
        self.close_object(path)?;
        Ok(Image { meta, source })
    }

    fn inlines(&mut self, path: &str) -> Result<Vec<Inline>, ReadError> {
        self.array(path, |parser, item_path| parser.inline(item_path))
    }

    fn inline(&mut self, path: &str) -> Result<Inline, ReadError> {
        self.open_object(path)?;
        self.field("kind", path)?;
        let kind = self.string(path)?;
        self.comma(path)?;
        let meta = self.node_head(path)?;

        let inline = match kind.as_str() {
            "text" => {
                self.comma(path)?;
                self.field("text", path)?;
                let value = self.string(path)?;
                Inline::Text(Text { meta, value })
            }
            "code" => {
                self.comma(path)?;
                self.field("text", path)?;
                let value = self.string(path)?;
                Inline::Code(Code { meta, value })
            }
            "strong" => Inline::Strong(self.container(meta, path)?),
            "emphasis" => Inline::Emphasis(self.container(meta, path)?),
            "underline" => Inline::Underline(self.container(meta, path)?),
            "strikethrough" => Inline::Strikethrough(self.container(meta, path)?),
            "smallCaps" => Inline::SmallCaps(self.container(meta, path)?),
            "superscript" => Inline::Superscript(self.container(meta, path)?),
            "subscript" => Inline::Subscript(self.container(meta, path)?),
            "link" => {
                self.comma(path)?;
                self.field("href", path)?;
                let href = self.string(path)?;
                self.comma(path)?;
                self.field("title", path)?;
                let title = self.null_or(path, |parser| parser.string(path))?;
                self.comma(path)?;
                self.field("inlines", path)?;
                let children = self.inlines(path)?;
                Inline::Link(Link {
                    meta,
                    href,
                    title,
                    children,
                })
            }
            "image" => {
                self.comma(path)?;
                self.field("src", path)?;
                let src = self.string(path)?;
                let source = self.image_ref_tail(path, src)?;
                Inline::Image(Image { meta, source })
            }
            "math" => {
                self.comma(path)?;
                self.field("source", path)?;
                let source = self.string(path)?;
                Inline::Math(MathInline { meta, source })
            }
            "footnoteRef" => {
                self.comma(path)?;
                self.field("target", path)?;
                let target = self.string(path)?;
                self.comma(path)?;
                self.field("label", path)?;
                let label = self.null_or(path, |parser| parser.string(path))?;
                Inline::FootnoteRef(FootnoteRef {
                    meta,
                    target,
                    label,
                })
            }
            "index" => {
                self.comma(path)?;
                self.field("term", path)?;
                let term = self.string(path)?;
                Inline::Index(IndexRef { meta, term })
            }
            "ruby" => {
                self.comma(path)?;
                self.field("base", path)?;
                let base = self.string(path)?;
                self.comma(path)?;
                self.field("annotation", path)?;
                let annotation = self.string(path)?;
                Inline::Ruby(Ruby {
                    meta,
                    base,
                    annotation,
                })
            }
            "lineBreak" => {
                self.comma(path)?;
                self.field("break", path)?;
                let kind = self.line_break_kind(path)?;
                Inline::LineBreak(LineBreak { meta, kind })
            }
            "raw" => Inline::Raw(RawInline::new(meta, self.raw_content(path)?)),
            _ => return self.error(path, "unknown kind"),
        };

        self.close_object(path)?;
        Ok(inline)
    }

    fn container(&mut self, meta: NodeMeta, path: &str) -> Result<Container, ReadError> {
        self.comma(path)?;
        self.field("inlines", path)?;
        Ok(Container::new(meta, self.inlines(path)?))
    }

    /// Raw content always follows a node head, so it consumes the leading comma.
    fn raw_content(&mut self, path: &str) -> Result<RawContent, ReadError> {
        self.comma(path)?;
        self.field("format", path)?;
        let format = self.string(path)?;
        self.comma(path)?;
        self.field("value", path)?;
        let value = self.string(path)?;
        self.comma(path)?;
        self.field("fallback", path)?;
        let fallback = self.string(path)?;
        self.comma(path)?;
        self.field("capability", path)?;
        let capability = self.raw_capability(path)?;
        Ok(RawContent {
            format,
            value,
            fallback,
            capability,
        })
    }

    fn generated_kind(&mut self, path: &str) -> Result<GeneratedKind, ReadError> {
        let value = self.string(path)?;
        match value.as_str() {
            "toc" => Ok(GeneratedKind::Toc),
            "listOfFigures" => Ok(GeneratedKind::ListOfFigures),
            "listOfTables" => Ok(GeneratedKind::ListOfTables),
            "index" => Ok(GeneratedKind::Index),
            "bibliography" => Ok(GeneratedKind::Bibliography),
            "cover" => Ok(GeneratedKind::Cover),
            "custom" => Ok(GeneratedKind::Custom),
            _ => self.error(path, "unknown generated kind"),
        }
    }

    /// Read a column's `align` value and its optional width share.
    ///
    /// The writer omits `share` when it is zero and writes
    /// `"align":"left","share":600` otherwise. The reader also accepts the
    /// historical malformed spelling (`"align":"left,"share":600"}`) so a
    /// document written by an older build can still be read.
    fn column_align(&mut self, path: &str) -> Result<(CellAlign, u16), ReadError> {
        self.skip_ws();
        self.expect_byte(b'"', path, "expected string")?;
        let start = self.pos;
        while let Some(byte) = self.peek() {
            if byte == b'"' || byte == b',' {
                break;
            }
            if !byte.is_ascii_alphabetic() {
                return self.error(path, "unknown cell alignment");
            }
            self.pos += 1;
        }
        let align = match &self.input[start..self.pos] {
            "default" => CellAlign::Default,
            "left" => CellAlign::Left,
            "center" => CellAlign::Center,
            "right" => CellAlign::Right,
            _ => return self.error(path, "unknown cell alignment"),
        };

        match self.peek() {
            Some(b'"') => {
                self.pos += 1;
                self.skip_ws();
                if self.peek() != Some(b',') {
                    return Ok((align, 0));
                }
                self.pos += 1;
                self.field("share", path)?;
                let share: u16 = self.integer_as(path)?;
                Ok((align, share))
            }
            Some(b',') => {
                self.pos += 1;
                self.skip_ws();
                self.expect_byte(b'"', path, "expected column share")?;
                if !self.eat_literal("share") {
                    return self.error(path, "invalid column share");
                }
                self.expect_byte(b'"', path, "expected column share")?;
                self.expect_byte(b':', path, "expected `:`")?;
                let share = self.share_digits(path)?;
                self.expect_byte(b'"', path, "expected closing quote")?;
                Ok((align, share))
            }
            _ => self.error(path, "unterminated string"),
        }
    }

    fn share_digits(&mut self, path: &str) -> Result<u16, ReadError> {
        let start = self.pos;
        let mut value = 0u16;
        while let Some(byte) = self.peek() {
            if !byte.is_ascii_digit() {
                break;
            }
            value = value
                .checked_mul(10)
                .and_then(|value| value.checked_add(u16::from(byte - b'0')))
                .ok_or_else(|| ReadError::new(path, "integer out of range", start))?;
            self.pos += 1;
        }
        if self.pos == start {
            return self.error(path, "expected integer");
        }
        Ok(value)
    }

    fn line_break_kind(&mut self, path: &str) -> Result<LineBreakKind, ReadError> {
        let value = self.string(path)?;
        match value.as_str() {
            "soft" => Ok(LineBreakKind::Soft),
            "hard" => Ok(LineBreakKind::Hard),
            _ => self.error(path, "unknown line break kind"),
        }
    }

    fn raw_capability(&mut self, path: &str) -> Result<RawCapability, ReadError> {
        let value = self.string(path)?;
        match value.as_str() {
            "preserved" => Ok(RawCapability::Preserved),
            "degraded" => Ok(RawCapability::Degraded),
            "dropped" => Ok(RawCapability::Dropped),
            _ => self.error(path, "unknown raw capability"),
        }
    }

    fn kind(&mut self, path: &str) -> Result<SourceKind, ReadError> {
        let value = self.string(path)?;
        match value.as_str() {
            "unknown" => Ok(SourceKind::Unknown),
            "markdown" => Ok(SourceKind::Markdown),
            "html" => Ok(SourceKind::Html),
            "latex" => Ok(SourceKind::Latex),
            "word" => Ok(SourceKind::Word),
            "synthetic" => Ok(SourceKind::Synthetic),
            _ => self.error(path, "unknown source kind"),
        }
    }
}
