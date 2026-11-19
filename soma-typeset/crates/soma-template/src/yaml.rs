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
//! - `#` comments outside quotes;
//! - empty values (a nested block, or null).
//!
//! Scalars are never implicitly typed: numbers stay strings so the unit parser
//! can convert them exactly, without floating point. `true`/`false` are decoded
//! where a boolean is expected.
//!
//! Not supported (explicit error or treated as text): flow collections
//! (`[a, b]`, `{a: b}`), anchors/aliases, tags, multi-line scalars, documents
//! (`---`), tabs for indentation.

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

    let (name, rest) = split_key(&text).ok_or_else(|| {
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
    let value = if rest.is_empty() {
        if *index < lines.len() && lines[*index].indent > indent {
            let child_indent = lines[*index].indent;
            parse_block(lines, index, child_indent)?
        } else {
            Value::null(line, column)
        }
    } else {
        Value::scalar(rest, line, column)
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

        // `- key: value` starts a mapping whose remaining keys align two columns
        // past the dash.
        if let Some((name, value_text)) = split_key(&rest) {
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
/// the end of the line. Returns unquoted, trimmed key and value.
fn split_key(text: &str) -> Option<(&str, &str)> {
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
                        let value = after.trim_start();
                        return Some((key, value));
                    }
                }
                _ => {}
            },
        }
    }
    None
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
