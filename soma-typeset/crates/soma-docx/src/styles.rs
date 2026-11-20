//! `word/styles.xml` and the run-format subset shared with `document.xml`.
//!
//! Only paragraph and character styles are read. A style's `w:rPr` carries the
//! properties DocIR can represent (bold, italic, underline, vertical alignment);
//! `w:basedOn` is followed to its root, and a cycle simply stops the walk.
//!
//! Nothing here is fatal. A missing, non-UTF-8, or malformed styles part yields
//! an empty table, and every lookup then returns defaults.

use alloc::string::String;
use alloc::vec::Vec;

use crate::xml::{StartTag, Token, Tokenizer};
use crate::zip::Archive;
use crate::DocxError;

/// Vertical alignment that DocIR can express.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Vertical {
    Superscript,
    Subscript,
}

/// `w:vertAlign` value, including an explicit return to the baseline.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum VerticalSetting {
    Superscript,
    Subscript,
    Baseline,
}

impl VerticalSetting {
    fn resolve(self) -> Option<Vertical> {
        match self {
            Self::Superscript => Some(Vertical::Superscript),
            Self::Subscript => Some(Vertical::Subscript),
            Self::Baseline => None,
        }
    }
}

/// Run properties as written on one element. `None` means "not specified", so
/// style layers can be merged without losing inheritance.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct CharFormat {
    bold: Option<bool>,
    italic: Option<bool>,
    underline: Option<bool>,
    vertical: Option<VerticalSetting>,
}

impl CharFormat {
    /// Overlay `other` on top of `self`.
    pub(crate) fn overlay(&mut self, other: Self) {
        if other.bold.is_some() {
            self.bold = other.bold;
        }
        if other.italic.is_some() {
            self.italic = other.italic;
        }
        if other.underline.is_some() {
            self.underline = other.underline;
        }
        if other.vertical.is_some() {
            self.vertical = other.vertical;
        }
    }

    /// Apply this layer to concrete run formatting. Later layers win because
    /// their `Some` values overwrite the earlier state.
    pub(crate) fn overlay_run(&self, target: &mut RunFormat) {
        if let Some(bold) = self.bold {
            target.bold = bold;
        }
        if let Some(italic) = self.italic {
            target.italic = italic;
        }
        if let Some(underline) = self.underline {
            target.underline = underline;
        }
        if let Some(vertical) = self.vertical {
            target.vertical = vertical.resolve();
        }
    }
}

/// Fully resolved run formatting used by [`crate::docx`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct RunFormat {
    pub(crate) bold: bool,
    pub(crate) italic: bool,
    pub(crate) underline: bool,
    pub(crate) vertical: Option<Vertical>,
}

/// One `w:rPr` in either a run or a style definition.
#[derive(Clone, Debug, Default)]
pub(crate) struct RunProperties {
    pub(crate) format: CharFormat,
    pub(crate) style_id: Option<String>,
}

/// Parse the inside of a `w:rPr`, stopping at its closing tag.
pub(crate) fn read_run_properties(
    tokens: &mut Tokenizer<'_>,
    self_closing: bool,
) -> Result<RunProperties, DocxError> {
    let mut properties = RunProperties::default();
    if self_closing {
        return Ok(properties);
    }
    while let Some(token) = tokens.next_token()? {
        match token {
            Token::Start(tag) => {
                match tag.name {
                    "b" => properties.format.bold = Some(on_off(tag.attr("val"))),
                    "i" => properties.format.italic = Some(on_off(tag.attr("val"))),
                    "u" => properties.format.underline = Some(on_off(tag.attr("val"))),
                    "vertAlign" => {
                        properties.format.vertical = vertical_setting(tag.attr("val"));
                    }
                    "rStyle" => {
                        if let Some(value) = tag.attr("val") {
                            let value = value.trim();
                            if !value.is_empty() {
                                properties.style_id = Some(String::from(value));
                            }
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
    Ok(properties)
}

fn vertical_setting(value: Option<&str>) -> Option<VerticalSetting> {
    match value?.trim() {
        value if value.eq_ignore_ascii_case("superscript") => Some(VerticalSetting::Superscript),
        value if value.eq_ignore_ascii_case("subscript") => Some(VerticalSetting::Subscript),
        value if value.eq_ignore_ascii_case("baseline") => Some(VerticalSetting::Baseline),
        _ => None,
    }
}

/// OOXML on/off property: an absent value means on, `0`/`false`/`off`/`none`
/// mean off.
pub(crate) fn on_off(value: Option<&str>) -> bool {
    value.is_none_or(|value| !is_off(value.trim()))
}

pub(crate) fn is_off(value: &str) -> bool {
    value == "0"
        || value.eq_ignore_ascii_case("false")
        || value.eq_ignore_ascii_case("off")
        || value.eq_ignore_ascii_case("none")
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum StyleKind {
    Paragraph,
    Character,
}

struct StyleEntry {
    id: String,
    kind: StyleKind,
    name: Option<String>,
    based_on: Option<String>,
    format: CharFormat,
}

/// The styles part, kept as a small lookup table.
#[derive(Default)]
pub(crate) struct Styles {
    entries: Vec<StyleEntry>,
}

impl Styles {
    /// Read `word/styles.xml` if it exists. The part is decoration: any read or
    /// parse failure yields no styles rather than a failed import.
    pub(crate) fn load(archive: &Archive<'_>) -> Self {
        let Some(bytes) = archive.read(b"word/styles.xml").ok().flatten() else {
            return Self::default();
        };
        let Ok(xml) = core::str::from_utf8(&bytes) else {
            return Self::default();
        };
        Self::parse(xml)
    }

    fn parse(xml: &str) -> Self {
        let mut styles = Self::default();
        let mut tokens = Tokenizer::new(xml);
        while let Ok(Some(token)) = tokens.next_token() {
            if let Token::Start(tag) = token {
                if tag.name == "style" {
                    if let Some(entry) = read_style(&mut tokens, &tag) {
                        styles.entries.push(entry);
                    }
                }
            }
        }
        styles
    }

    fn find(&self, id: &str, kind: StyleKind) -> Option<&StyleEntry> {
        self.entries
            .iter()
            .find(|entry| entry.kind == kind && entry.id == id)
    }

    /// Merge a style with its `w:basedOn` ancestors. The root is applied first
    /// so derived styles override inherited properties.
    fn effective(&self, id: &str, kind: StyleKind) -> CharFormat {
        let mut chain: Vec<&StyleEntry> = Vec::new();
        let mut seen: Vec<&str> = Vec::new();
        let mut current = self.find(id, kind);
        while let Some(entry) = current {
            if seen.contains(&entry.id.as_str()) {
                break;
            }
            seen.push(entry.id.as_str());
            chain.push(entry);
            current = entry
                .based_on
                .as_deref()
                .and_then(|base| self.find(base, kind));
        }

        let mut format = CharFormat::default();
        for entry in chain.into_iter().rev() {
            format.overlay(entry.format);
        }
        format
    }

    /// Character properties carried by a paragraph style.
    pub(crate) fn paragraph_format(&self, id: &str) -> CharFormat {
        self.effective(id, StyleKind::Paragraph)
    }

    /// Character properties carried by a character style. The built-in
    /// `Strong`/`Emphasis` ids keep their historical meaning when no styles
    /// part defines them.
    pub(crate) fn character_format(&self, id: &str) -> CharFormat {
        if self.find(id, StyleKind::Character).is_some() {
            return self.effective(id, StyleKind::Character);
        }
        if id.eq_ignore_ascii_case("Strong") {
            return CharFormat {
                bold: Some(true),
                ..CharFormat::default()
            };
        }
        if id.eq_ignore_ascii_case("Emphasis") {
            return CharFormat {
                italic: Some(true),
                ..CharFormat::default()
            };
        }
        CharFormat::default()
    }

    /// Resolve a paragraph style id to a heading level by its *name*. Without a
    /// styles part (or when the id is unknown) the id itself is the fallback,
    /// which keeps documents that use literal `Heading1` spellings readable.
    pub(crate) fn heading_level(&self, id: &str) -> Option<u8> {
        let id = id.trim();
        let name = match self.find(id, StyleKind::Paragraph) {
            Some(entry) => entry.name.as_deref().unwrap_or(entry.id.as_str()),
            None => id,
        };
        heading_name_level(name)
    }
}

fn read_style(tokens: &mut Tokenizer<'_>, tag: &StartTag<'_>) -> Option<StyleEntry> {
    let kind = match tag.attr("type").map(str::trim) {
        Some(value) if value.eq_ignore_ascii_case("paragraph") => Some(StyleKind::Paragraph),
        Some(value) if value.eq_ignore_ascii_case("character") => Some(StyleKind::Character),
        _ => None,
    };
    let id = tag
        .attr("styleId")
        .map(str::trim)
        .filter(|id| !id.is_empty());
    let mut name: Option<String> = None;
    let mut based_on: Option<String> = None;
    let mut format = CharFormat::default();

    if !tag.self_closing {
        while let Ok(Some(token)) = tokens.next_token() {
            match token {
                Token::Start(inner) => match inner.name {
                    "name" => {
                        if name.is_none() {
                            name = inner
                                .attr("val")
                                .map(str::trim)
                                .filter(|value| !value.is_empty())
                                .map(String::from);
                        }
                        if skip_element(tokens, inner.self_closing).is_err() {
                            break;
                        }
                    }
                    "basedOn" => {
                        if based_on.is_none() {
                            based_on = inner
                                .attr("val")
                                .map(str::trim)
                                .filter(|value| !value.is_empty())
                                .map(String::from);
                        }
                        if skip_element(tokens, inner.self_closing).is_err() {
                            break;
                        }
                    }
                    "rPr" => {
                        if let Ok(properties) = read_run_properties(tokens, inner.self_closing) {
                            format = properties.format;
                        }
                    }
                    _ => {
                        if skip_element(tokens, inner.self_closing).is_err() {
                            break;
                        }
                    }
                },
                Token::End(end) => {
                    if end == "style" {
                        break;
                    }
                }
                Token::Text(_) => {}
            }
        }
    }

    let kind = kind?;
    let id = id?;
    Some(StyleEntry {
        id: String::from(id),
        kind,
        name,
        based_on,
        format,
    })
}

fn heading_name_level(name: &str) -> Option<u8> {
    let name = name.trim();
    if name.eq_ignore_ascii_case("title") {
        return Some(1);
    }
    let lower = name.to_ascii_lowercase();
    let suffix = lower.strip_prefix("heading")?.trim();
    let level: u8 = suffix.parse().ok()?;
    if (1..=9).contains(&level) {
        Some(level.min(6))
    } else {
        None
    }
}

/// Skip an unknown element and its subtree. A skipped element contributes
/// nothing and never aborts the document.
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
