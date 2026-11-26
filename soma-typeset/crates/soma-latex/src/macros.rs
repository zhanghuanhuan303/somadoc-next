//! Source-level expansion of user macros and counters.
//!
//! Expansion happens once, before the block/inline parser runs, so a macro
//! works in both contexts. The pass is textual and forgiving: definitions and
//! counter commands disappear from the output, calls are replaced by their
//! body with `#1`…`#9` substituted, and anything unrecognized is copied
//! through unchanged for the regular parser.
//!
//! Verbatim material (`verbatim`, `lstlisting`, and inline `\verb`) is copied
//! without inspecting it, so macro-like text inside it stays literal.

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// Maximum nesting of macro expansions. A call reached at the limit is left
/// in the source, so self-referential macros terminate without panicking.
const MAX_DEPTH: usize = 64;

struct MacroDef {
    name: String,
    params: u8,
    body: String,
}

struct Counter {
    name: String,
    value: i64,
}

/// Expand user macros and counters over `source`.
pub(crate) fn expand(source: &str) -> String {
    Expander::new().run(source, 0)
}

struct Expander {
    macros: Vec<MacroDef>,
    counters: Vec<Counter>,
}

impl Expander {
    fn new() -> Self {
        Self {
            macros: Vec::new(),
            counters: Vec::new(),
        }
    }

    fn run(&mut self, source: &str, depth: usize) -> String {
        let mut out = String::with_capacity(source.len());
        let mut pos = 0usize;
        while pos < source.len() {
            let character = source[pos..].chars().next().expect("not eof");
            if character == '%' {
                let end = source[pos..]
                    .find('\n')
                    .map_or(source.len(), |offset| pos + offset);
                out.push_str(&source[pos..end]);
                pos = end;
                continue;
            }
            if character != '\\' {
                out.push(character);
                pos += character.len_utf8();
                continue;
            }

            let start = pos;
            let (name, command_end) = control_sequence(source, start);

            if name == "begin" {
                if let Some(after) = verbatim_span(source, command_end) {
                    out.push_str(&source[start..after]);
                    pos = after;
                    continue;
                }
            }
            if name == "verb" {
                if let Some(after) = inline_verbatim_end(source, command_end) {
                    out.push_str(&source[start..after]);
                    pos = after;
                    continue;
                }
            }

            // An explicitly defined macro wins over a counter-derived
            // `\the<name>`, mirroring `\renewcommand{\thefoo}`.
            if let Some(index) = self
                .macros
                .iter()
                .position(|definition| definition.name == name)
            {
                if let Some(after) =
                    self.expand_macro(source, start, command_end, index, depth, &mut out)
                {
                    pos = after;
                    continue;
                }
            }

            let handled = match name {
                "newcommand" | "renewcommand" | "providecommand" => {
                    self.read_definition(source, command_end, name)
                }
                "def" => self.read_def(source, command_end),
                "newcounter" => self.define_counter(source, command_end),
                "setcounter" => self.set_counter(source, command_end),
                "stepcounter" | "refstepcounter" => self.step_counter(source, command_end),
                "arabic" | "roman" | "Roman" | "alph" | "Alph" | "value" => {
                    self.render_counter(source, command_end, name, &mut out)
                }
                _ => None,
            };
            if let Some(after) = handled {
                pos = after;
                continue;
            }

            // `\the<name>` only applies to counters declared with
            // `\newcounter` in this document; `\thesection` & co. are left for
            // the template-generated numbering.
            if let Some(counter_name) = name.strip_prefix("the") {
                if let Some(value) = self.counter_value(counter_name) {
                    out.push_str(&value.to_string());
                    pos = command_end;
                    continue;
                }
            }

            // Unknown command: copy it and keep scanning its arguments.
            out.push_str(&source[start..command_end]);
            pos = command_end;
        }
        out
    }

    fn expand_macro(
        &mut self,
        source: &str,
        start: usize,
        mut pos: usize,
        index: usize,
        depth: usize,
        out: &mut String,
    ) -> Option<usize> {
        let params = self.macros[index].params;
        let mut arguments: Vec<&str> = Vec::with_capacity(params as usize);
        for _ in 0..params {
            let (argument, after) = group_after(source, pos)?;
            arguments.push(argument);
            pos = after;
        }
        if depth >= MAX_DEPTH {
            out.push_str(&source[start..pos]);
            return Some(pos);
        }
        let body = self.macros[index].body.clone();
        let substituted = substitute(&body, &arguments);
        out.push_str(&self.run(&substituted, depth + 1));
        Some(pos)
    }

    fn read_definition(&mut self, source: &str, pos: usize, kind: &str) -> Option<usize> {
        let (name, after_name) = command_name_at(source, pos)?;
        let mut cursor = skip_spaces(source, after_name);
        let mut params = 0u8;
        if source.as_bytes().get(cursor) == Some(&b'[') {
            let (count, after_count) = bracketed_group(source, cursor)?;
            params = count.trim().parse::<u8>().ok()?;
            if params > 9 {
                return None;
            }
            cursor = skip_spaces(source, after_count);
            // Optional default arguments are not supported. Consume the group
            // so it does not leak into the document; callers must still pass
            // every argument in braces.
            if source.as_bytes().get(cursor) == Some(&b'[') {
                let (_, after_default) = bracketed_group(source, cursor)?;
                cursor = skip_spaces(source, after_default);
            }
        }
        let (body, after_body) = braced_group(source, cursor)?;
        let definition = MacroDef {
            name: name.to_string(),
            params,
            body: body.to_string(),
        };
        if kind == "providecommand" {
            if !self.macros.iter().any(|existing| existing.name == name) {
                self.macros.push(definition);
            }
        } else if let Some(existing) = self
            .macros
            .iter_mut()
            .find(|existing| existing.name == name)
        {
            *existing = definition;
        } else {
            self.macros.push(definition);
        }
        Some(after_body)
    }

    fn read_def(&mut self, source: &str, pos: usize) -> Option<usize> {
        let (name, after_name) = command_name_at(source, pos)?;
        let body_start = skip_spaces(source, after_name);
        let (body, after_body) = braced_group(source, body_start)?;
        let definition = MacroDef {
            name: name.to_string(),
            params: 0,
            body: body.to_string(),
        };
        if let Some(existing) = self
            .macros
            .iter_mut()
            .find(|existing| existing.name == name)
        {
            *existing = definition;
        } else {
            self.macros.push(definition);
        }
        Some(after_body)
    }

    fn define_counter(&mut self, source: &str, pos: usize) -> Option<usize> {
        let (name, after) = group_after(source, pos)?;
        let name = name.trim();
        if name.is_empty() {
            return None;
        }
        let after = skip_optional_bracket(source, after);
        if !self.counters.iter().any(|counter| counter.name == name) {
            self.counters.push(Counter {
                name: name.to_string(),
                value: 0,
            });
        }
        Some(after)
    }

    fn set_counter(&mut self, source: &str, pos: usize) -> Option<usize> {
        let (name, after_name) = group_after(source, pos)?;
        let (value, after_value) = group_after(source, after_name)?;
        let value = value.trim().parse::<i64>().ok()?;
        let counter = self
            .counters
            .iter_mut()
            .find(|counter| counter.name == name.trim())?;
        counter.value = value;
        Some(after_value)
    }

    fn step_counter(&mut self, source: &str, pos: usize) -> Option<usize> {
        let (name, after) = group_after(source, pos)?;
        let counter = self
            .counters
            .iter_mut()
            .find(|counter| counter.name == name.trim())?;
        counter.value = counter.value.saturating_add(1);
        Some(after)
    }

    fn render_counter(
        &self,
        source: &str,
        pos: usize,
        style: &str,
        out: &mut String,
    ) -> Option<usize> {
        let (name, after) = group_after(source, pos)?;
        let counter = self
            .counters
            .iter()
            .find(|counter| counter.name == name.trim())?;
        out.push_str(&render_counter_value(counter.value, style));
        Some(after)
    }

    fn counter_value(&self, name: &str) -> Option<i64> {
        self.counters
            .iter()
            .find(|counter| counter.name == name)
            .map(|counter| counter.value)
    }
}

/// Read `\letters` or `\x` starting at `start`; returns the name without the
/// backslash and the position after the control sequence.
fn control_sequence(source: &str, start: usize) -> (&str, usize) {
    let bytes = source.as_bytes();
    let mut end = start + 1;
    if end < bytes.len() && bytes[end].is_ascii_alphabetic() {
        end += 1;
        while end < bytes.len() && bytes[end].is_ascii_alphabetic() {
            end += 1;
        }
    } else if let Some(character) = source[end..].chars().next() {
        end += character.len_utf8();
    }
    (&source[start + 1..end], end)
}

/// Read a `{...}` group starting exactly at `start`, honoring nested braces
/// and backslash escapes. Returns the contents and the position after `}`.
fn braced_group(source: &str, start: usize) -> Option<(&str, usize)> {
    if source.as_bytes().get(start) != Some(&b'{') {
        return None;
    }
    let bytes = source.as_bytes();
    let mut depth = 0usize;
    let mut index = start;
    while index < bytes.len() {
        if bytes[index] == b'\\' {
            index += 2;
            continue;
        }
        match bytes[index] {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some((&source[start + 1..index], index + 1));
                }
            }
            _ => {}
        }
        index += 1;
    }
    None
}

/// Read a `{...}` group after optional whitespace.
fn group_after(source: &str, pos: usize) -> Option<(&str, usize)> {
    let start = skip_spaces(source, pos);
    braced_group(source, start)
}

/// Read a `[...]` group starting exactly at `start`.
fn bracketed_group(source: &str, start: usize) -> Option<(&str, usize)> {
    if source.as_bytes().get(start) != Some(&b'[') {
        return None;
    }
    let bytes = source.as_bytes();
    let mut index = start + 1;
    while index < bytes.len() {
        if bytes[index] == b'\\' {
            index += 2;
            continue;
        }
        if bytes[index] == b']' {
            return Some((&source[start + 1..index], index + 1));
        }
        index += 1;
    }
    None
}

fn skip_spaces(source: &str, mut pos: usize) -> usize {
    let bytes = source.as_bytes();
    while pos < bytes.len() && bytes[pos].is_ascii_whitespace() {
        pos += 1;
    }
    pos
}

/// Consume a `[...]` immediately after optional whitespace when present.
fn skip_optional_bracket(source: &str, pos: usize) -> usize {
    let start = skip_spaces(source, pos);
    if source.as_bytes().get(start) != Some(&b'[') {
        return pos;
    }
    bracketed_group(source, start).map_or(pos, |(_, after)| after)
}

/// Read the macro name in a definition: `{\name}` or `\name`.
fn command_name_at(source: &str, pos: usize) -> Option<(&str, usize)> {
    let start = skip_spaces(source, pos);
    if source.as_bytes().get(start) == Some(&b'{') {
        let (inner, after) = braced_group(source, start)?;
        let name = inner.trim().strip_prefix('\\')?;
        if name.is_empty() || !name.bytes().all(|byte| byte.is_ascii_alphabetic()) {
            return None;
        }
        Some((name, after))
    } else if source.as_bytes().get(start) == Some(&b'\\') {
        let (name, after) = control_sequence(source, start);
        if name.is_empty() || !name.bytes().all(|byte| byte.is_ascii_alphabetic()) {
            return None;
        }
        Some((name, after))
    } else {
        None
    }
}

/// Substitute `#1`…`#9` in a macro body; `##` yields a literal `#`, and
/// `\#` stays escaped.
fn substitute(body: &str, arguments: &[&str]) -> String {
    let mut out = String::with_capacity(body.len());
    let mut chars = body.chars().peekable();
    while let Some(character) = chars.next() {
        if character == '\\' {
            out.push('\\');
            if let Some(next) = chars.next() {
                out.push(next);
            }
            continue;
        }
        if character != '#' {
            out.push(character);
            continue;
        }
        match chars.peek().copied() {
            Some('#') => {
                let _ = chars.next();
                out.push('#');
            }
            Some(digit @ '1'..='9') => {
                let _ = chars.next();
                if let Some(argument) = arguments.get(digit as usize - '1' as usize) {
                    out.push_str(argument);
                } else {
                    out.push('#');
                    out.push(digit);
                }
            }
            _ => out.push('#'),
        }
    }
    out
}

/// Position after `\end{env}` when `\begin{env}` at `pos` opens a verbatim
/// environment. Returns `None` for ordinary environments.
fn verbatim_span(source: &str, pos: usize) -> Option<usize> {
    let (environment, after) = group_after(source, pos)?;
    let environment = environment.trim();
    if !matches!(
        environment,
        "verbatim" | "verbatim*" | "lstlisting" | "lstlisting*"
    ) {
        return None;
    }
    let marker = format!("\\end{{{environment}}}");
    let end = source[after..]
        .find(&marker)
        .map_or(source.len(), |offset| after + offset + marker.len());
    Some(end)
}

/// Position after the closing delimiter of `\verb`, or after the rest of the
/// source when the delimiter is missing.
fn inline_verbatim_end(source: &str, mut pos: usize) -> Option<usize> {
    if source.as_bytes().get(pos) == Some(&b'*') {
        pos += 1;
    }
    let delimiter = source[pos..].chars().next()?;
    if delimiter.is_alphabetic() || delimiter.is_whitespace() {
        return None;
    }
    let mut cursor = pos + delimiter.len_utf8();
    for character in source[cursor..].chars() {
        cursor += character.len_utf8();
        if character == delimiter {
            return Some(cursor);
        }
    }
    Some(source.len())
}

fn render_counter_value(value: i64, style: &str) -> String {
    match style {
        "roman" => roman(value, false),
        "Roman" => roman(value, true),
        "alph" => letters(value, false),
        "Alph" => letters(value, true),
        _ => value.to_string(),
    }
}

const ROMAN_NUMERALS: [(i64, &str); 13] = [
    (1000, "m"),
    (900, "cm"),
    (500, "d"),
    (400, "cd"),
    (100, "c"),
    (90, "xc"),
    (50, "l"),
    (40, "xl"),
    (10, "x"),
    (9, "ix"),
    (5, "v"),
    (4, "iv"),
    (1, "i"),
];

/// Roman numerals for 1…3999. Other values fall back to Arabic digits, which
/// keeps the output deterministic instead of erroring mid-expansion.
fn roman(value: i64, upper: bool) -> String {
    if value <= 0 || value > 3999 {
        return value.to_string();
    }
    let mut remaining = value;
    let mut out = String::new();
    for (amount, digits) in ROMAN_NUMERALS {
        while remaining >= amount {
            out.push_str(digits);
            remaining -= amount;
        }
    }
    if upper {
        out.make_ascii_uppercase();
    }
    out
}

/// Bijective base-26 letters: 1 → a, 26 → z, 27 → aa, 28 → ab …
fn letters(value: i64, upper: bool) -> String {
    if value <= 0 {
        return value.to_string();
    }
    let mut remaining = value;
    let mut digits = Vec::new();
    while remaining > 0 {
        let index = ((remaining - 1) % 26) as u8;
        digits.push(if upper { b'A' + index } else { b'a' + index });
        remaining = (remaining - 1) / 26;
    }
    digits.reverse();
    digits.into_iter().map(char::from).collect()
}

#[cfg(test)]
mod tests {
    use super::expand;

    #[test]
    fn zero_parameter_macro_expands() {
        assert_eq!(expand(r"\newcommand{\hi}{你好}\hi"), "你好");
    }

    #[test]
    fn arguments_keep_nested_braces() {
        assert_eq!(
            expand(r"\newcommand{\pair}[2]{#1 与 #2}\pair{甲{乙}丙}{丁}"),
            "甲{乙}丙 与 丁"
        );
    }

    #[test]
    fn recursive_macro_stops_at_the_depth_limit() {
        assert_eq!(expand(r"\newcommand{\loop}{\loop}\loop"), r"\loop");
    }

    #[test]
    fn definitions_in_comments_are_ignored() {
        let source = "% \\newcommand{\\hidden}{bad}\n\\hidden";
        assert_eq!(expand(source), source);
    }

    #[test]
    fn escaped_percent_does_not_start_a_comment() {
        assert_eq!(expand(r"50\% \newcommand{\x}{X}\x"), r"50\% X");
    }

    #[test]
    fn verbatim_environments_are_not_expanded() {
        let source = r"\newcommand{\x}{X}\begin{verbatim}\x\end{verbatim}\begin{lstlisting}\x\end{lstlisting}";
        assert_eq!(
            expand(source),
            r"\begin{verbatim}\x\end{verbatim}\begin{lstlisting}\x\end{lstlisting}"
        );
    }

    #[test]
    fn template_counters_are_not_hijacked() {
        assert_eq!(
            expand(r"\newcounter{foo}\thefoo \thesection"),
            r"0 \thesection"
        );
    }

    #[test]
    fn letters_continue_past_z() {
        assert_eq!(
            expand(r"\newcounter{n}\setcounter{n}{27}\alph{n}\Alph{n}"),
            "aaAA"
        );
    }
}
