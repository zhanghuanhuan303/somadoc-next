//! A small, forgiving XML tokenizer for OOXML parts.
//!
//! It emits start tags (with attributes), end tags, and text. Namespace prefixes
//! are dropped: every consumer matches on the local name (`w:p` becomes `p`).
//! Comments, processing instructions, DOCTYPE declarations, and CDATA are
//! handled, and the five predefined entities plus numeric character references
//! are decoded.
//!
//! "Forgiving" means stray end tags and unknown markup never abort a parse.
//! A *truncated* construct (unterminated tag, attribute value, comment, or a
//! reserved delimiter sequence) is still an error: continuing would silently
//! invent structure.

use alloc::borrow::Cow;
use alloc::string::String;
use alloc::vec::Vec;

use crate::DocxError;

/// Longest entity name worth scanning for. `&#x10FFFF;` is the longest legal
/// numeric reference; named references are shorter.
const MAX_ENTITY_LEN: usize = 16;

pub(crate) enum Token<'a> {
    Start(StartTag<'a>),
    End(&'a str),
    Text(Cow<'a, str>),
}

pub(crate) struct StartTag<'a> {
    /// Local name (`p`, `t`, `tbl`), with any namespace prefix removed.
    pub(crate) name: &'a str,
    pub(crate) attrs: Vec<Attribute<'a>>,
    pub(crate) self_closing: bool,
}

impl StartTag<'_> {
    /// First attribute with this local name. Namespaces are ignored, so `r:id`
    /// and `w:id` both answer to `id`; for OOXML elements this reader looks at,
    /// only one spelling is meaningful.
    pub(crate) fn attr(&self, name: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|attribute| attribute.name == name)
            .map(|attribute| attribute.value.as_ref())
    }
}

pub(crate) struct Attribute<'a> {
    name: &'a str,
    value: Cow<'a, str>,
}

pub(crate) struct Tokenizer<'a> {
    input: &'a str,
    position: usize,
}

impl<'a> Tokenizer<'a> {
    pub(crate) fn new(input: &'a str) -> Self {
        // A UTF-8 BOM is not text and is not markup.
        let input = input.strip_prefix('\u{feff}').unwrap_or(input);
        Self { input, position: 0 }
    }

    pub(crate) fn next_token(&mut self) -> Result<Option<Token<'a>>, DocxError> {
        loop {
            let input = self.input;
            let rest = &input[self.position..];
            if rest.is_empty() {
                return Ok(None);
            }

            if !rest.starts_with('<') {
                let end = rest.find('<').unwrap_or(rest.len());
                let raw = &rest[..end];
                self.position += end;
                if !raw.is_empty() {
                    return Ok(Some(Token::Text(decode_entities(raw))));
                }
                continue;
            }

            if rest.starts_with("<!--") {
                let end = rest.find("-->").ok_or_else(malformed)?;
                self.position += end + 3;
                continue;
            }
            if rest.starts_with("<![CDATA[") {
                let end = rest.find("]]>").ok_or_else(malformed)?;
                let text = &rest[9..end];
                self.position += end + 3;
                return Ok(Some(Token::Text(Cow::Borrowed(text))));
            }
            if rest.starts_with("<?") {
                let end = rest.find("?>").ok_or_else(malformed)?;
                self.position += end + 2;
                continue;
            }
            if rest.starts_with("<!") {
                // DOCTYPE and friends. OOXML does not use an internal subset;
                // scanning to the next `>` is enough and keeps the reader small.
                let end = rest.find('>').ok_or_else(malformed)?;
                self.position += end + 1;
                continue;
            }
            if let Some(after) = rest.strip_prefix("</") {
                let end = after.find('>').ok_or_else(malformed)?;
                let raw = after[..end].trim();
                if raw.is_empty() {
                    return Err(malformed());
                }
                self.position += 2 + end + 1;
                return Ok(Some(Token::End(local_name(raw))));
            }

            // A bare `<` in text (`a < b`) is not markup. Returning it as text
            // keeps the character instead of dropping it.
            let after = &rest[1..];
            if !after
                .as_bytes()
                .first()
                .is_some_and(|byte| is_name_start(*byte))
            {
                self.position += 1;
                return Ok(Some(Token::Text(Cow::Borrowed("<"))));
            }

            return self.start_tag().map(Some);
        }
    }

    fn start_tag(&mut self) -> Result<Token<'a>, DocxError> {
        let input = self.input;
        let mut position = self.position + 1;

        let name_start = position;
        while let Some(byte) = input.as_bytes().get(position) {
            if is_name_end(*byte) {
                break;
            }
            position += 1;
        }
        let raw_name = &input[name_start..position];
        if raw_name.is_empty() {
            return Err(malformed());
        }
        let name = local_name(raw_name);

        let mut attrs = Vec::new();
        let mut self_closing = false;
        loop {
            position = skip_whitespace(input, position);
            let Some(byte) = input.as_bytes().get(position).copied() else {
                return Err(malformed());
            };
            match byte {
                b'>' => {
                    position += 1;
                    break;
                }
                b'/' => {
                    position += 1;
                    position = skip_whitespace(input, position);
                    if input.as_bytes().get(position) == Some(&b'>') {
                        position += 1;
                        self_closing = true;
                        break;
                    }
                    // A stray slash in a tag name is malformed but harmless;
                    // keep scanning instead of losing the rest of the tag.
                }
                _ => {
                    let (attribute, next) = parse_attribute(input, position)?;
                    attrs.push(attribute);
                    position = next;
                }
            }
        }

        self.position = position;
        Ok(Token::Start(StartTag {
            name,
            attrs,
            self_closing,
        }))
    }
}

fn parse_attribute<'a>(input: &'a str, start: usize) -> Result<(Attribute<'a>, usize), DocxError> {
    let mut position = start;
    let name_start = position;
    while let Some(byte) = input.as_bytes().get(position).copied() {
        if byte.is_ascii_whitespace() || matches!(byte, b'=' | b'/' | b'>') {
            break;
        }
        position += 1;
    }
    if position == name_start {
        return Err(malformed());
    }
    let name = local_name(&input[name_start..position]);

    let mut after_name = skip_whitespace(input, position);
    let mut value = Cow::Borrowed("");
    if input.as_bytes().get(after_name) == Some(&b'=') {
        after_name += 1;
        after_name = skip_whitespace(input, after_name);
        match input.as_bytes().get(after_name).copied() {
            Some(quote @ (b'"' | b'\'')) => {
                let value_start = after_name + 1;
                let rest = &input[value_start..];
                let offset = rest
                    .as_bytes()
                    .iter()
                    .position(|byte| *byte == quote)
                    .ok_or_else(malformed)?;
                value = decode_entities(&rest[..offset]);
                position = value_start + offset + 1;
            }
            Some(_) => {
                let value_start = after_name;
                while let Some(byte) = input.as_bytes().get(after_name).copied() {
                    if byte.is_ascii_whitespace() || byte == b'>' {
                        break;
                    }
                    after_name += 1;
                }
                value = decode_entities(&input[value_start..after_name]);
                position = after_name;
            }
            None => return Err(malformed()),
        }
    } else {
        // A value-less attribute is invalid XML but cannot hide content; treat
        // it as an empty string and move on.
        position = after_name;
    }

    Ok((Attribute { name, value }, position))
}

fn skip_whitespace(input: &str, mut position: usize) -> usize {
    while let Some(byte) = input.as_bytes().get(position).copied() {
        if !byte.is_ascii_whitespace() {
            break;
        }
        position += 1;
    }
    position
}

fn is_name_start(byte: u8) -> bool {
    byte.is_ascii_alphabetic() || matches!(byte, b'_' | b':') || byte >= 0x80
}

fn is_name_end(byte: u8) -> bool {
    byte.is_ascii_whitespace() || matches!(byte, b'/' | b'>')
}

fn local_name(name: &str) -> &str {
    match name.rfind(':') {
        Some(colon) => &name[colon + 1..],
        None => name,
    }
}

fn malformed() -> DocxError {
    DocxError::new("XML is malformed")
}

fn decode_entities(raw: &str) -> Cow<'_, str> {
    if !raw.as_bytes().contains(&b'&') {
        return Cow::Borrowed(raw);
    }
    let mut out = String::with_capacity(raw.len());
    let mut rest = raw;
    while let Some(index) = rest.find('&') {
        out.push_str(&rest[..index]);
        let after = &rest[index + 1..];
        if let Some(semicolon) = after
            .as_bytes()
            .iter()
            .take(MAX_ENTITY_LEN)
            .position(|byte| *byte == b';')
        {
            if let Some(decoded) = entity(&after[..semicolon]) {
                out.push(decoded);
                rest = &after[semicolon + 1..];
                continue;
            }
        }
        // Unknown or malformed references stay literal; dropping them would
        // lose bytes the author wrote.
        out.push('&');
        rest = after;
    }
    out.push_str(rest);
    Cow::Owned(out)
}

fn entity(name: &str) -> Option<char> {
    match name {
        "amp" => return Some('&'),
        "lt" => return Some('<'),
        "gt" => return Some('>'),
        "quot" => return Some('"'),
        "apos" => return Some('\''),
        _ => {}
    }
    let number = name.strip_prefix('#')?;
    let value = match number
        .strip_prefix('x')
        .or_else(|| number.strip_prefix('X'))
    {
        Some(hex) => u32::from_str_radix(hex, 16).ok()?,
        None => number.parse::<u32>().ok()?,
    };
    // XML 1.0 forbids most control characters. A reference that is not a legal
    // character becomes U+FFFD instead of poisoning canonical output.
    Some(
        char::from_u32(value)
            .filter(|character| is_xml_char(*character))
            .unwrap_or('\u{fffd}'),
    )
}

fn is_xml_char(character: char) -> bool {
    matches!(character, '\t' | '\n' | '\r') || (character as u32) >= 0x20
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::string::String;

    fn next<'a>(tokenizer: &mut Tokenizer<'a>) -> Token<'a> {
        tokenizer
            .next_token()
            .expect("tokenizes")
            .expect("a token remains")
    }

    #[test]
    fn prefixes_are_dropped_and_attributes_are_decoded() {
        let mut tokenizer = Tokenizer::new(
            "<w:p w:val=\"1\"><w:t xml:space=\"preserve\">a &amp; b&#33;&#x42;</w:t></w:p>",
        );

        match next(&mut tokenizer) {
            Token::Start(tag) => {
                assert_eq!(tag.name, "p");
                assert_eq!(tag.attr("val"), Some("1"));
                assert!(!tag.self_closing);
            }
            _ => panic!("expected a start tag"),
        }
        match next(&mut tokenizer) {
            Token::Start(tag) => {
                assert_eq!(tag.name, "t");
                assert_eq!(tag.attr("space"), Some("preserve"));
            }
            _ => panic!("expected a start tag"),
        }
        match next(&mut tokenizer) {
            Token::Text(text) => assert_eq!(text.as_ref(), "a & b!B"),
            _ => panic!("expected text"),
        }
        assert!(matches!(next(&mut tokenizer), Token::End("t")));
        assert!(matches!(next(&mut tokenizer), Token::End("p")));
        assert!(tokenizer.next_token().expect("tokenizes").is_none());
    }

    #[test]
    fn self_closing_tags_report_themselves() {
        let mut tokenizer = Tokenizer::new("<w:br />");
        match next(&mut tokenizer) {
            Token::Start(tag) => {
                assert_eq!(tag.name, "br");
                assert!(tag.self_closing);
            }
            _ => panic!("expected a start tag"),
        }
    }

    #[test]
    fn comments_and_declarations_are_skipped() {
        let mut tokenizer = Tokenizer::new(
            "<?xml version=\"1.0\"?><!-- note --><w:body><![CDATA[a < b]]></w:body>",
        );
        assert!(matches!(next(&mut tokenizer), Token::Start(tag) if tag.name == "body"));
        match next(&mut tokenizer) {
            Token::Text(text) => assert_eq!(text.as_ref(), "a < b"),
            _ => panic!("expected CDATA text"),
        }
        assert!(matches!(next(&mut tokenizer), Token::End("body")));
    }

    #[test]
    fn unknown_entities_stay_literal() {
        let mut tokenizer = Tokenizer::new("a &bogus; b & c");
        match next(&mut tokenizer) {
            Token::Text(text) => assert_eq!(text.as_ref(), "a &bogus; b & c"),
            _ => panic!("expected text"),
        }
    }

    #[test]
    fn a_bare_less_than_is_text() {
        let mut tokenizer = Tokenizer::new("a < b");
        let mut out = String::new();
        while let Some(token) = tokenizer.next_token().expect("tokenizes") {
            if let Token::Text(text) = token {
                out.push_str(&text);
            }
        }
        assert_eq!(out, "a < b");
    }

    #[test]
    fn truncation_is_an_error() {
        assert!(Tokenizer::new("<w:p").next_token().is_err());
        assert!(Tokenizer::new("<w:p a=\"x").next_token().is_err());
        assert!(Tokenizer::new("<!--").next_token().is_err());
        assert!(Tokenizer::new("</>").next_token().is_err());
    }
}
