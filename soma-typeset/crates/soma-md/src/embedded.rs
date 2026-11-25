//! Scanners for embedded LaTeX and HTML inside Markdown.
//!
//! This module only finds syntax boundaries and decodes attributes/text. The
//! semantic mapping stays in `soma-md` (inline HTML) or is delegated to the
//! HTML/LaTeX frontends (block HTML and embedded LaTeX), so there is one
//! mapping per target syntax.

use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// One HTML start/end tag found at the beginning of a string.
pub(crate) struct Tag<'a> {
    pub name: &'a str,
    pub closing: bool,
    pub self_closing: bool,
    pub attrs: Vec<(&'a str, &'a str)>,
    /// Bytes consumed, including `<` and `>`.
    pub end: usize,
}

impl Tag<'_> {
    pub fn is(&self, name: &str) -> bool {
        self.name.eq_ignore_ascii_case(name)
    }

    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| *value)
    }
}

/// Parse an HTML tag at the start of `text`.
pub(crate) fn tag_at(text: &str) -> Option<Tag<'_>> {
    let bytes = text.as_bytes();
    if bytes.first() != Some(&b'<') {
        return None;
    }
    if text.starts_with("<!--") || text.starts_with("<!") || text.starts_with("<?") {
        return None;
    }
    let end = tag_end(text)?;
    let body = &text[1..end - 1];
    let (closing, body) = match body.strip_prefix('/') {
        Some(rest) => (true, rest),
        None => (false, body),
    };
    let name_len = body
        .bytes()
        .take_while(|byte| byte.is_ascii_alphanumeric() || *byte == b'-' || *byte == b':')
        .count();
    if name_len == 0 {
        return None;
    }
    let name = &body[..name_len];
    let after = &body[name_len..];
    if let Some(next) = after.chars().next() {
        if !next.is_whitespace() && next != '/' && next != '>' {
            return None;
        }
    }
    let self_closing = !closing && body.trim_end().ends_with('/');
    let attrs = if closing {
        Vec::new()
    } else {
        let after = after.trim_end();
        parse_attrs(after.strip_suffix('/').unwrap_or(after))
    };
    Some(Tag {
        name,
        closing,
        self_closing,
        attrs,
        end,
    })
}

/// Find the end of a tag, respecting quoted attribute values.
pub(crate) fn tag_end(text: &str) -> Option<usize> {
    let bytes = text.as_bytes();
    if bytes.first() != Some(&b'<') {
        return None;
    }
    let mut quote = None;
    for (index, byte) in bytes.iter().enumerate().skip(1) {
        match *byte {
            b'"' | b'\'' if quote.is_none() => quote = Some(*byte),
            b'"' | b'\'' if quote == Some(*byte) => quote = None,
            b'>' if quote.is_none() => return Some(index + 1),
            _ => {}
        }
    }
    None
}

fn parse_attrs(mut text: &str) -> Vec<(&str, &str)> {
    let mut attrs = Vec::new();
    loop {
        text = text.trim_start_matches(|c: char| c.is_whitespace() || c == '/');
        if text.is_empty() {
            break;
        }
        let name_len = text
            .bytes()
            .take_while(|byte| {
                !byte.is_ascii_whitespace() && *byte != b'=' && *byte != b'/' && *byte != b'>'
            })
            .count();
        if name_len == 0 {
            break;
        }
        let name = &text[..name_len];
        text = &text[name_len..];
        text = text.trim_start();
        if !text.starts_with('=') {
            attrs.push((name, ""));
            continue;
        }
        text = text[1..].trim_start();
        if let Some(quote) = text.chars().next().filter(|c| *c == '"' || *c == '\'') {
            text = &text[quote.len_utf8()..];
            let Some(end) = text.find(quote) else {
                break;
            };
            attrs.push((name, &text[..end]));
            text = &text[end + quote.len_utf8()..];
        } else {
            let end = text
                .find(|c: char| c.is_whitespace() || c == '>')
                .unwrap_or(text.len());
            attrs.push((name, &text[..end]));
            text = &text[end..];
        }
    }
    attrs
}

/// Find the matching close tag for `name` in `text`, which begins immediately
/// after the opening tag. Returns `(inner_end, after_close)`.
pub(crate) fn matching_close(text: &str, name: &str) -> Option<(usize, usize)> {
    let mut pos = 0usize;
    let mut depth = 1usize;
    while let Some(offset) = text[pos..].find('<') {
        let at = pos + offset;
        let Some(tag) = tag_at(&text[at..]) else {
            pos = at + 1;
            continue;
        };
        if tag.is(name) {
            if tag.closing {
                depth -= 1;
                if depth == 0 {
                    return Some((at, at + tag.end));
                }
            } else if !tag.self_closing {
                depth += 1;
            }
        }
        pos = at + tag.end.max(1);
    }
    None
}

/// The body of the first `<rt>` child, used for `<ruby>`.
pub(crate) fn ruby_annotation(text: &str) -> Option<String> {
    let mut pos = 0usize;
    while let Some(offset) = text[pos..].find('<') {
        let at = pos + offset;
        let Some(tag) = tag_at(&text[at..]) else {
            pos = at + 1;
            continue;
        };
        if !tag.closing && tag.is("rt") {
            let rest = &text[at + tag.end..];
            let (inner, _) = matching_close(rest, "rt")?;
            return Some(collapse(&strip_tags(&rest[..inner])));
        }
        pos = at + tag.end.max(1);
    }
    None
}

/// The base text of `<ruby>`, excluding any `<rt>` annotations.
pub(crate) fn ruby_base(text: &str) -> String {
    let mut out = String::new();
    let mut pos = 0usize;
    while pos < text.len() {
        let rest = &text[pos..];
        if let Some(tag) = tag_at(rest) {
            if !tag.closing && tag.is("rt") {
                let after = &rest[tag.end..];
                if let Some((_, consumed)) = matching_close(after, "rt") {
                    pos += tag.end + consumed;
                } else {
                    pos += tag.end;
                }
                continue;
            }
            pos += tag.end;
            continue;
        }
        out.push_str(&strip_tags(rest));
        break;
    }
    collapse(&out)
}

/// Remove tags, decode entities, and treat `<br>` as a line break.
pub(crate) fn strip_tags(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut pos = 0usize;
    while pos < text.len() {
        let rest = &text[pos..];
        if rest.starts_with('<') {
            if let Some(tag) = tag_at(rest) {
                if tag.is("br") {
                    out.push('\n');
                }
                pos += tag.end;
                continue;
            }
        }
        let character = rest.chars().next().expect("non-empty suffix");
        if character == '&' {
            let (decoded, consumed) = decode_entity(rest);
            out.push_str(&decoded);
            pos += consumed;
        } else {
            out.push(character);
            pos += character.len_utf8();
        }
    }
    out
}

/// Decode the common named and numeric HTML entities. Unknown entities keep
/// the ampersand and the rest of the text.
pub(crate) fn decode_entities(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut pos = 0usize;
    while pos < text.len() {
        let rest = &text[pos..];
        if let Some(stripped) = rest.strip_prefix('&') {
            let (decoded, consumed) = decode_entity(rest);
            if consumed > 1 {
                out.push_str(&decoded);
                pos += consumed;
                continue;
            }
            let _ = stripped;
        }
        let character = rest.chars().next().expect("non-empty suffix");
        out.push(character);
        pos += character.len_utf8();
    }
    out
}

fn decode_entity(text: &str) -> (String, usize) {
    let Some(end) = text.find(';') else {
        return (String::from("&"), 1);
    };
    let body = &text[1..end];
    let decoded = match body {
        "amp" => Some('&'),
        "lt" => Some('<'),
        "gt" => Some('>'),
        "quot" => Some('"'),
        "apos" => Some('\''),
        "nbsp" => Some('\u{a0}'),
        "ndash" => Some('–'),
        "mdash" => Some('—'),
        "hellip" => Some('…'),
        "copy" => Some('©'),
        "reg" => Some('®'),
        "times" => Some('×'),
        "divide" => Some('÷'),
        _ => {
            if let Some(hex) = body.strip_prefix("#x").or_else(|| body.strip_prefix("#X")) {
                u32::from_str_radix(hex, 16).ok().and_then(char::from_u32)
            } else if let Some(dec) = body.strip_prefix('#') {
                dec.parse::<u32>().ok().and_then(char::from_u32)
            } else {
                None
            }
        }
    };
    match decoded {
        Some(character) => {
            let mut out = String::new();
            out.push(character);
            (out, end + 1)
        }
        None => (String::from("&"), 1),
    }
}

/// Collapse whitespace runs to one space and trim the ends.
pub(crate) fn collapse(text: &str) -> String {
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

/// One inline LaTeX command and the bytes it occupies.
pub(crate) struct LatexCommand<'a> {
    pub name: &'a str,
    pub end: usize,
}

/// Recognise an inline LaTeX command whose arguments are complete. Unknown
/// commands and missing arguments stay ordinary Markdown text; `\footnote`
/// is recognised so the caller can preserve its source instead of emitting an
/// unresolved footnote reference.
pub(crate) fn latex_inline_at(text: &str) -> Option<LatexCommand<'_>> {
    let rest = text.strip_prefix('\\')?;
    let first = rest.chars().next()?;
    let (name, mut pos) = if first.is_ascii_alphabetic() {
        let len = rest
            .bytes()
            .take_while(|byte| byte.is_ascii_alphabetic())
            .count();
        (&rest[..len], 1 + len)
    } else {
        // `\(` and `\[` are handled by the math scanner; other one-character
        // commands keep Markdown's escape semantics.
        return None;
    };
    let args = match name {
        "textbf" | "emph" | "textit" | "textsl" | "underline" | "textsc" | "textsuperscript"
        | "textsubscript" | "texttt" | "url" | "index" | "label" | "cite" | "ref" | "pageref"
        | "footnote" => 1,
        "href" | "ruby" => 2,
        "ldots" | "dots" | "LaTeX" | "TeX" | "newline" => 0,
        "verb" => {
            if text[pos..].starts_with('*') {
                pos += 1;
            }
            let delimiter = text[pos..].chars().next()?;
            if delimiter.is_whitespace() || delimiter == '{' {
                return None;
            }
            let rest = &text[pos + delimiter.len_utf8()..];
            let offset = rest.find(delimiter)?;
            return Some(LatexCommand {
                name,
                end: pos + delimiter.len_utf8() + offset + delimiter.len_utf8(),
            });
        }
        _ => return None,
    };
    for _ in 0..args {
        pos = skip_spaces(text, pos);
        if !text[pos..].starts_with('{') {
            return None;
        }
        pos = brace_group_end(text, pos)?;
    }
    Some(LatexCommand { name, end: pos })
}

fn skip_spaces(text: &str, mut pos: usize) -> usize {
    while pos < text.len() {
        let character = text[pos..].chars().next().expect("non-empty suffix");
        if !character.is_whitespace() {
            break;
        }
        pos += character.len_utf8();
    }
    pos
}

fn brace_group_end(text: &str, start: usize) -> Option<usize> {
    if !text[start..].starts_with('{') {
        return None;
    }
    let mut depth = 0usize;
    let mut escaped = false;
    for (offset, character) in text[start..].char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        match character {
            '\\' => escaped = true,
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(start + offset + 1);
                }
            }
            _ => {}
        }
    }
    None
}

/// Tags that have a semantic block mapping in `soma-html`. A tag outside this
/// list keeps the existing visible RawBlock fallback instead of being guessed.
pub(crate) fn semantic_html_block(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "address"
            | "article"
            | "aside"
            | "blockquote"
            | "body"
            | "center"
            | "dd"
            | "div"
            | "dl"
            | "dt"
            | "figcaption"
            | "figure"
            | "footer"
            | "h1"
            | "h2"
            | "h3"
            | "h4"
            | "h5"
            | "h6"
            | "header"
            | "hr"
            | "html"
            | "li"
            | "main"
            | "nav"
            | "ol"
            | "p"
            | "pre"
            | "section"
            | "table"
            | "tbody"
            | "td"
            | "tfoot"
            | "th"
            | "thead"
            | "tr"
            | "ul"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tags_and_attributes_are_scanned() {
        let tag = tag_at("<a href=\"/x?a=1&amp;b=2\" title='t'>").expect("tag");
        assert!(tag.is("a"));
        assert_eq!(tag.attr("href"), Some("/x?a=1&amp;b=2"));
        assert_eq!(tag.attr("title"), Some("t"));
        assert_eq!(tag.end, 35);
        assert_eq!(decode_entities(tag.attr("href").unwrap()), "/x?a=1&b=2");
    }

    #[test]
    fn an_unquoted_self_closing_value_does_not_keep_the_slash() {
        let tag = tag_at("<img src=a.png/>").expect("tag");
        assert!(tag.self_closing);
        assert_eq!(tag.attr("src"), Some("a.png"));
    }

    #[test]
    fn matching_close_handles_nesting() {
        let text = "<b>x</b> y</b>";
        let (inner, after) = matching_close(text, "b").expect("close");
        assert_eq!(&text[..inner], "<b>x</b> y");
        assert_eq!(after, text.len());
    }

    #[test]
    fn latex_commands_take_balanced_groups() {
        let command = latex_inline_at("\\textbf{a {nested} b} tail").expect("command");
        assert_eq!(command.name, "textbf");
        assert_eq!(command.end, "\\textbf{a {nested} b}".len());
        assert_eq!(
            latex_inline_at("\\footnote{x}").expect("footnote").name,
            "footnote"
        );
        assert!(latex_inline_at("\\textbf{open").is_none());
    }
}
