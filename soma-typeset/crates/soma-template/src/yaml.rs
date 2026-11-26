//! A deliberately small YAML subset.
//!
//! Full YAML is a huge surface with implicit typing rules that are hard to make
//! deterministic. Templates are our own DSL, so we support exactly what the DSL
//! needs and reject everything else with a line/column error:
//!
//! - block mappings (`key: value`), nested by indentation (spaces only);
//! - block sequences (`- value`, `- key: value`) nested by indentation;
//! - plain scalars, single-quoted scalars (`''` for a quote), double-quoted
//!   scalars (`\"`, `\\`, `\n`, `\t`);
//! - flow sequences (`[a, b]`) and flow mappings (`{a: b}`) on one line; they
//!   may nest, and may appear wherever a block scalar or block collection may;
//! - `#` comments outside quotes;
//! - empty values (a nested block, or null).
//!
//! Scalars are never implicitly typed: numbers stay strings so the unit parser
//! can convert them exactly, without floating point. `true`/`false` are decoded
//! where a boolean is expected.
//!
//! Not supported (explicit error or treated as text): anchors/aliases, tags,
//! multi-line scalars, multi-line flow collections, documents (`---`), tabs for
//! indentation.

use alloc::string::String;
use alloc::vec::Vec;

use crate::TemplateError;

/// One parsed value with the position it came from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Value {
    pub kind: ValueKind,
    pub line: u32,
    pub column: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ValueKind {
    Null,
    Str(String),
    Map(Vec<(Key, Value)>),
    Seq(Vec<Value>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Key {
    pub name: String,
    pub line: u32,
    pub column: u32,
}

impl Value {
    fn null(line: u32, column: u32) -> Self {
        Self {
            kind: ValueKind::Null,
            line,
            column,
        }
    }

    fn scalar(text: &str, line: u32, column: u32) -> Self {
        Self {
            kind: ValueKind::Str(text.into()),
            line,
            column,
        }
    }

    fn map(entries: Vec<(Key, Value)>, line: u32, column: u32) -> Self {
        Self {
            kind: ValueKind::Map(entries),
            line,
            column,
        }
    }

    fn seq(items: Vec<Value>, line: u32, column: u32) -> Self {
        Self {
            kind: ValueKind::Seq(items),
            line,
            column,
        }
    }

    pub fn is_null(&self) -> bool {
        matches!(self.kind, ValueKind::Null)
    }

    pub fn as_seq(&self, path: &str) -> Result<&[Value], TemplateError> {
        match &self.kind {
            ValueKind::Seq(items) => Ok(items),
            _ => Err(self.error(path, "expected a sequence")),
        }
    }

    pub fn as_str(&self, path: &str) -> Result<String, TemplateError> {
        match &self.kind {
            ValueKind::Str(text) => Ok(scalar_text(text)),
            _ => Err(self.error(path, "expected a scalar")),
        }
    }

    /// Error helper anchored at this value.
    pub fn error(&self, path: &str, reason: &'static str) -> TemplateError {
        TemplateError::new(self.line, self.column, path, reason)
    }
}

struct Line {
    indent: usize,
    text: String,
    number: u32,
}

/// Parse a template document. The top level must be a mapping.
pub(crate) fn parse(source: &str) -> Result<Value, TemplateError> {
    let lines = split_lines(source)?;
    if lines.is_empty() {
        return Err(TemplateError::new(1, 1, "", "template must not be empty"));
    }
    let mut index = 0usize;
    let base_indent = lines[0].indent;
    let value = parse_block(&lines, &mut index, base_indent)?;
    if index < lines.len() {
        return Err(TemplateError::new(
            lines[index].number,
            (lines[index].indent + 1) as u32,
            "",
            "unexpected indentation",
        ));
    }
    if !matches!(value.kind, ValueKind::Map(_)) {
        return Err(value.error("", "template root must be a mapping"));
    }
    Ok(value)
}

fn split_lines(source: &str) -> Result<Vec<Line>, TemplateError> {
    let mut lines = Vec::new();
    for (index, raw) in source.lines().enumerate() {
        let number = index as u32 + 1;
        let mut indent = 0usize;
        for character in raw.chars() {
            match character {
                ' ' => indent += 1,
                '\t' => {
                    return Err(TemplateError::new(
                        number,
                        indent as u32 + 1,
                        "",
                        "tabs are not allowed for indentation",
                    ))
                }
                _ => break,
            }
        }
        let rest = &raw[indent..];
        let stripped = strip_comment(rest);
        let trimmed = stripped.trim_end();
        if trimmed.is_empty() {
            continue;
        }
        lines.push(Line {
            indent,
            text: String::from(trimmed),
            number,
        });
    }
    Ok(lines)
}

/// Remove a trailing comment, ignoring `#` inside quotes.
fn strip_comment(text: &str) -> &str {
    let mut quote: Option<char> = None;
    let mut previous_space = true;
    for (offset, character) in text.char_indices() {
        match quote {
            Some(active) if character == active => quote = None,
            Some(_) => {}
            None => match character {
                '\'' | '"' => {
                    quote = Some(character);
                    previous_space = false;
                    continue;
                }
                '#' if previous_space => return &text[..offset],
                _ => {}
            },
        }
        previous_space = character == ' ' || character == '\t';
    }
    text
}

fn parse_block(lines: &[Line], index: &mut usize, indent: usize) -> Result<Value, TemplateError> {
    if lines[*index].text.starts_with('-') {
        parse_seq(lines, index, indent)
    } else {
        parse_map(lines, index, indent)
    }
}

fn parse_map(lines: &[Line], index: &mut usize, indent: usize) -> Result<Value, TemplateError> {
    let first = &lines[*index];
    let (line, column) = (first.number, (first.indent + 1) as u32);
    let mut entries: Vec<(Key, Value)> = Vec::new();

    while *index < lines.len() && lines[*index].indent == indent {
        let current = &lines[*index];
        if current.text.starts_with('-') {
            break;
        }
        parse_entry(lines, index, indent, &mut entries)?;
    }

    if entries.is_empty() {
        return Err(TemplateError::new(line, column, "", "empty mapping"));
    }
    Ok(Value::map(entries, line, column))
}

fn parse_entry(
    lines: &[Line],
    index: &mut usize,
    indent: usize,
    entries: &mut Vec<(Key, Value)>,
) -> Result<(), TemplateError> {
    let current = &lines[*index];
    let line = current.number;
    let text = current.text.clone();
    *index += 1;

    let (name, rest, value_offset) = split_key(&text).ok_or_else(|| {
        TemplateError::new(line, (indent + 1) as u32, "", "expected `key: value`")
    })?;
    if name.is_empty() {
        return Err(TemplateError::new(
            line,
            (indent + 1) as u32,
            "",
            "empty mapping key",
        ));
    }
    if entries.iter().any(|(key, _)| key.name == name) {
        return Err(TemplateError::new(
            line,
            (indent + 1) as u32,
            name,
            "duplicate key",
        ));
    }

    let column = (indent + 1) as u32;
    let value_column = (indent + 1 + text[..value_offset].chars().count()) as u32;
    let value = if rest.is_empty() {
        if *index < lines.len() && lines[*index].indent > indent {
            let child_indent = lines[*index].indent;
            parse_block(lines, index, child_indent)?
        } else {
            Value::null(line, column)
        }
    } else if starts_flow(rest) {
        parse_flow(rest, line, value_column)?
    } else {
        Value::scalar(rest, line, value_column)
    };

    entries.push((
        Key {
            name: name.into(),
            line,
            column,
        },
        value,
    ));
    Ok(())
}

fn parse_seq(lines: &[Line], index: &mut usize, indent: usize) -> Result<Value, TemplateError> {
    let first = &lines[*index];
    let (line, column) = (first.number, (first.indent + 1) as u32);
    let mut items = Vec::new();

    while *index < lines.len() && lines[*index].indent == indent {
        let current = &lines[*index];
        if !current.text.starts_with('-') {
            break;
        }
        let item_line = current.number;
        let rest = String::from(current.text[1..].trim_start());
        *index += 1;

        if rest.is_empty() {
            if *index < lines.len() && lines[*index].indent > indent {
                let child_indent = lines[*index].indent;
                items.push(parse_block(lines, index, child_indent)?);
            } else {
                items.push(Value::null(item_line, column));
            }
            continue;
        }

        // A flow collection is a complete item on one line; check it before
        // `split_key` so a colon inside `{a: b}` is not read as a block key.
        if starts_flow(&rest) {
            items.push(parse_flow(&rest, item_line, (indent + 3) as u32)?);
            continue;
        }

        // `- key: value` starts a mapping whose remaining keys align two columns
        // past the dash.
        if let Some((name, value_text, value_offset)) = split_key(&rest) {
            let map_indent = indent + 2;
            let entry_column = (indent + 3) as u32;
            let mut entries: Vec<(Key, Value)> = Vec::new();
            let value = if value_text.is_empty() {
                if *index < lines.len() && lines[*index].indent > map_indent {
                    let child_indent = lines[*index].indent;
                    parse_block(lines, index, child_indent)?
                } else {
                    Value::null(item_line, entry_column)
                }
            } else if starts_flow(value_text) {
                parse_flow(
                    value_text,
                    item_line,
                    entry_column + rest[..value_offset].chars().count() as u32,
                )?
            } else {
                Value::scalar(value_text, item_line, entry_column)
            };
            entries.push((
                Key {
                    name: name.into(),
                    line: item_line,
                    column: entry_column,
                },
                value,
            ));
            while *index < lines.len()
                && lines[*index].indent == map_indent
                && !lines[*index].text.starts_with('-')
            {
                parse_entry(lines, index, map_indent, &mut entries)?;
            }
            items.push(Value::map(entries, item_line, column));
        } else {
            items.push(Value::scalar(&rest, item_line, (indent + 3) as u32));
        }
    }

    if items.is_empty() {
        return Err(TemplateError::new(line, column, "", "empty sequence"));
    }
    Ok(Value::seq(items, line, column))
}

/// Split `key: value` at the first unquoted `:` that is followed by a space or
/// the end of the line. Returns the trimmed key, the trimmed value, and the
/// value's byte offset in `text` (for diagnostics).
fn split_key(text: &str) -> Option<(&str, &str, usize)> {
    let mut quote: Option<char> = None;
    let mut escaped = false;
    for (offset, character) in text.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        match quote {
            Some(active) => {
                if active == '"' && character == '\\' {
                    escaped = true;
                } else if character == active {
                    quote = None;
                }
            }
            None => match character {
                '\'' | '"' => quote = Some(character),
                ':' => {
                    let after = &text[offset + 1..];
                    if after.is_empty() || after.starts_with(' ') {
                        let key = text[..offset].trim();
                        let trimmed = after.trim_start();
                        let value_offset = offset + 1 + (after.len() - trimmed.len());
                        return Some((key, trimmed, value_offset));
                    }
                }
                _ => {}
            },
        }
    }
    None
}

/// True when a value begins a one-line flow collection.
fn starts_flow(value: &str) -> bool {
    matches!(value.chars().next(), Some('[') | Some('{'))
}

/// Parse one flow collection. The text must contain exactly the collection
/// (trailing spaces are fine); anything after the closing bracket is an error.
fn parse_flow(text: &str, line: u32, column: u32) -> Result<Value, TemplateError> {
    let mut parser = FlowParser {
        text,
        offset: 0,
        line,
        base_column: column,
    };
    let value = parser.parse_value()?;
    parser.skip_space();
    if parser.offset < text.len() {
        return Err(parser.error("unexpected text after flow collection"));
    }
    Ok(value)
}

/// Recursive one-line flow collection parser. Positions are derived from the
/// line the collection started on and the column of its first character.
struct FlowParser<'a> {
    text: &'a str,
    offset: usize,
    line: u32,
    base_column: u32,
}

impl<'a> FlowParser<'a> {
    fn column(&self) -> u32 {
        self.base_column + self.text[..self.offset].chars().count() as u32
    }

    fn error(&self, reason: &'static str) -> TemplateError {
        TemplateError::new(self.line, self.column(), "", reason)
    }

    fn peek(&self) -> Option<char> {
        self.text[self.offset..].chars().next()
    }

    fn bump(&mut self) -> Option<char> {
        let character = self.peek()?;
        self.offset += character.len_utf8();
        Some(character)
    }

    fn skip_space(&mut self) {
        while matches!(self.peek(), Some(' ') | Some('\t')) {
            self.offset += 1;
        }
    }

    fn parse_value(&mut self) -> Result<Value, TemplateError> {
        self.skip_space();
        let line = self.line;
        let column = self.column();
        match self.peek() {
            Some('[') => self.parse_seq(),
            Some('{') => self.parse_map(),
            Some('\'') | Some('"') => {
                let text = self.take_quoted()?;
                Ok(Value::scalar(&text, line, column))
            }
            Some(_) => {
                let text = self.take_plain(&[',', ']', '}']);
                if text.is_empty() {
                    return Err(self.error("expected a value"));
                }
                Ok(Value::scalar(text, line, column))
            }
            None => Err(self.error("expected a value")),
        }
    }

    fn parse_seq(&mut self) -> Result<Value, TemplateError> {
        let line = self.line;
        let column = self.column();
        self.bump();
        let mut items = Vec::new();
        loop {
            self.skip_space();
            match self.peek() {
                Some(']') => {
                    self.bump();
                    break;
                }
                None => return Err(self.error("unterminated flow sequence")),
                _ => {}
            }
            items.push(self.parse_value()?);
            self.skip_space();
            match self.peek() {
                Some(',') => {
                    self.bump();
                }
                Some(']') => {
                    self.bump();
                    break;
                }
                Some(_) => return Err(self.error("expected `,` or `]` in flow sequence")),
                None => return Err(self.error("unterminated flow sequence")),
            }
        }
        Ok(Value::seq(items, line, column))
    }

    fn parse_map(&mut self) -> Result<Value, TemplateError> {
        let line = self.line;
        let column = self.column();
        self.bump();
        let mut entries: Vec<(Key, Value)> = Vec::new();
        loop {
            self.skip_space();
            match self.peek() {
                Some('}') => {
                    self.bump();
                    break;
                }
                None => return Err(self.error("unterminated flow mapping")),
                _ => {}
            }

            let key_line = self.line;
            let key_column = self.column();
            let key = match self.peek() {
                Some('\'') | Some('"') => self.take_quoted()?,
                _ => {
                    let text = self.take_plain(&[':']);
                    if text.is_empty() {
                        return Err(self.error("empty mapping key"));
                    }
                    String::from(text)
                }
            };
            if entries.iter().any(|(seen, _)| seen.name == key) {
                return Err(TemplateError::new(
                    key_line,
                    key_column,
                    "",
                    "duplicate key",
                ));
            }

            self.skip_space();
            if self.peek() != Some(':') {
                return Err(self.error("expected `:` in flow mapping"));
            }
            self.bump();
            self.skip_space();
            let value = if matches!(self.peek(), Some(',') | Some('}')) {
                Value::null(key_line, self.column())
            } else {
                self.parse_value()?
            };
            entries.push((
                Key {
                    name: key,
                    line: key_line,
                    column: key_column,
                },
                value,
            ));

            self.skip_space();
            match self.peek() {
                Some(',') => {
                    self.bump();
                }
                Some('}') => {
                    self.bump();
                    break;
                }
                Some(_) => return Err(self.error("expected `,` or `}` in flow mapping")),
                None => return Err(self.error("unterminated flow mapping")),
            }
        }
        Ok(Value::map(entries, line, column))
    }

    /// Consume a quoted scalar (including both quotes) and decode it.
    fn take_quoted(&mut self) -> Result<String, TemplateError> {
        let start = self.offset;
        let column = self.column();
        let quote = self.bump().ok_or_else(|| {
            TemplateError::new(self.line, column, "", "unterminated quoted scalar")
        })?;
        let mut escaped = false;
        loop {
            let Some(character) = self.bump() else {
                return Err(TemplateError::new(
                    self.line,
                    column,
                    "",
                    "unterminated quoted scalar",
                ));
            };
            if escaped {
                escaped = false;
                continue;
            }
            if quote == '"' && character == '\\' {
                escaped = true;
                continue;
            }
            if character == quote {
                if quote == '\'' && self.peek() == Some('\'') {
                    self.bump();
                    continue;
                }
                break;
            }
        }
        Ok(scalar_text(&self.text[start..self.offset]))
    }

    fn take_plain(&mut self, stops: &[char]) -> &'a str {
        let start = self.offset;
        while let Some(character) = self.peek() {
            if stops.contains(&character) {
                break;
            }
            self.offset += character.len_utf8();
        }
        self.text[start..self.offset].trim()
    }
}

/// Decode a scalar: quoted forms are unescaped, everything else is returned
/// verbatim (numbers stay strings).
pub(crate) fn scalar_text(text: &str) -> String {
    let trimmed = text.trim();
    if trimmed.len() >= 2 {
        let bytes = trimmed.as_bytes();
        if bytes[0] == b'\'' && bytes[trimmed.len() - 1] == b'\'' {
            return trimmed[1..trimmed.len() - 1].replace("''", "'");
        }
        if bytes[0] == b'"' && bytes[trimmed.len() - 1] == b'"' {
            let mut out = String::new();
            let mut chars = trimmed[1..trimmed.len() - 1].chars();
            while let Some(character) = chars.next() {
                if character != '\\' {
                    out.push(character);
                    continue;
                }
                match chars.next() {
                    Some('n') => out.push('\n'),
                    Some('t') => out.push('\t'),
                    Some('"') => out.push('"'),
                    Some('\\') => out.push('\\'),
                    Some(other) => {
                        out.push('\\');
                        out.push(other);
                    }
                    None => out.push('\\'),
                }
            }
            return out;
        }
    }
    trimmed.into()
}
