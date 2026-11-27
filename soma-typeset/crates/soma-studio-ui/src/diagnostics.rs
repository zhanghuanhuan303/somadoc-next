//! Source diagnostics for the Studio editor.
//!
//! The engine reports failures as strings and most parsers are deliberately
//! forgiving. This module adds a UI-side layer that finds high-confidence
//! authoring mistakes with byte spans, and maps a build error to a node span
//! when the error names a `node N`.
//!
//! It is intentionally conservative: a diagnostic must either be a structural
//! error we can point at (unclosed fence, undefined footnote, math parse error)
//! or a build failure. It does not guess at style preferences.

use std::collections::{BTreeMap, BTreeSet};

use soma_studio_core::SourceFormat;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Origin {
    /// A source lint with an exact byte span.
    Source,
    /// A build failure. It may carry a node span when one can be recovered.
    Build,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    /// Byte offsets into the source. Equal offsets mean "no underline".
    pub start: usize,
    pub end: usize,
    /// 1-based line and character column. Zero means "not in the source".
    pub line: usize,
    pub column: usize,
    pub message: String,
    pub suggestion: String,
    pub origin: Origin,
}

/// Run all source lints and append the build failure, if any.
pub fn analyze(source: &str, format: SourceFormat, build_error: Option<&str>) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    let fences = scan_fences(source, &mut diagnostics);
    let mut mask = fences.clone();
    if format == SourceFormat::Markdown {
        mask.extend(inline_code_ranges(source, &mask));
    }
    mask = merge_ranges(mask);

    let math = if matches!(format, SourceFormat::Markdown | SourceFormat::Latex) {
        scan_math(source, &mask, &mut diagnostics)
    } else {
        Vec::new()
    };

    if format == SourceFormat::Markdown {
        let mut text_mask = mask.clone();
        text_mask.extend(math.iter().copied());
        text_mask = merge_ranges(text_mask);
        scan_extensions(source, &text_mask, &mut diagnostics);
        scan_footnotes(source, &text_mask, &mut diagnostics);
    }

    if format == SourceFormat::Latex {
        let mut env_mask = mask.clone();
        env_mask.extend(math.iter().copied());
        env_mask = merge_ranges(env_mask);
        scan_environments(source, &env_mask, &mut diagnostics);
    }

    if let Some(error) = build_error {
        if !error.trim().is_empty() {
            diagnostics.push(build_diagnostic(source, format, error));
        }
    }

    normalize(source, diagnostics)
}

/* ------------------------------------------------------------------ helpers */

fn push(
    source: &str,
    diagnostics: &mut Vec<Diagnostic>,
    start: usize,
    end: usize,
    message: impl Into<String>,
    suggestion: impl Into<String>,
    origin: Origin,
) {
    let start = clamp_start(source, start);
    let end = clamp_end(source, start, end);
    let (line, column) = line_col(source, start);
    diagnostics.push(Diagnostic {
        start,
        end,
        line,
        column,
        message: message.into(),
        suggestion: suggestion.into(),
        origin,
    });
}

fn clamp_start(source: &str, byte: usize) -> usize {
    let mut byte = byte.min(source.len());
    while byte > 0 && !source.is_char_boundary(byte) {
        byte -= 1;
    }
    byte
}

fn clamp_end(source: &str, start: usize, end: usize) -> usize {
    let mut end = end.max(start).min(source.len());
    while end < source.len() && !source.is_char_boundary(end) {
        end += 1;
    }
    end
}

/// 1-based line and character column for a byte offset.
pub fn line_col(source: &str, byte: usize) -> (usize, usize) {
    let byte = clamp_start(source, byte);
    let before = &source[..byte];
    let line = before.matches('\n').count() + 1;
    let column = before.rsplit('\n').next().unwrap_or("").chars().count() + 1;
    (line, column)
}

fn merge_ranges(mut ranges: Vec<(usize, usize)>) -> Vec<(usize, usize)> {
    ranges.retain(|(start, end)| end > start);
    ranges.sort_unstable();
    let mut merged: Vec<(usize, usize)> = Vec::with_capacity(ranges.len());
    for (start, end) in ranges {
        match merged.last_mut() {
            Some((_, last_end)) if start <= *last_end => {
                *last_end = (*last_end).max(end);
            }
            _ => merged.push((start, end)),
        }
    }
    merged
}

fn in_ranges(byte: usize, ranges: &[(usize, usize)]) -> bool {
    ranges
        .iter()
        .any(|(start, end)| byte >= *start && byte < *end)
}

fn next_range_end(byte: usize, ranges: &[(usize, usize)]) -> usize {
    ranges
        .iter()
        .find(|(start, end)| byte >= *start && byte < *end)
        .map(|(_, end)| *end)
        .unwrap_or(byte + 1)
}

fn next_char_len(source: &str, byte: usize) -> usize {
    source[byte..]
        .chars()
        .next()
        .map(char::len_utf8)
        .unwrap_or(1)
}

fn count_run(source: &str, byte: usize, needle: char) -> usize {
    source[byte..]
        .chars()
        .take_while(|character| *character == needle)
        .map(char::len_utf8)
        .sum()
}

/* ------------------------------------------------------------------- fences */

fn scan_fences(source: &str, diagnostics: &mut Vec<Diagnostic>) -> Vec<(usize, usize)> {
    let mut ranges = Vec::new();
    let mut open: Option<(char, usize, usize, usize)> = None;
    let mut offset = 0;
    for line in source.split_inclusive('\n') {
        let trimmed = line.trim_start();
        let indent = line.len() - trimmed.len();
        if let Some((fence, width, start_line, _marker)) = open {
            if closing_fence(trimmed, fence, width) {
                ranges.push((start_line, offset + line.len()));
                open = None;
            }
        } else if let Some((fence, width)) = opening_fence(trimmed) {
            open = Some((fence, width, offset, offset + indent));
        }
        offset += line.len();
    }
    if let Some((_, _, start_line, marker)) = open {
        ranges.push((start_line, source.len()));
        push(
            source,
            diagnostics,
            marker,
            marker + 3,
            "代码围栏没有闭合",
            "在文末补一个同类型的闭合围栏，或删除起始围栏。",
            Origin::Source,
        );
    }
    ranges
}

fn opening_fence(trimmed: &str) -> Option<(char, usize)> {
    let first = trimmed.chars().next()?;
    if first != '`' && first != '~' {
        return None;
    }
    let width = trimmed
        .chars()
        .take_while(|character| *character == first)
        .count();
    (width >= 3).then_some((first, width))
}

fn closing_fence(trimmed: &str, fence: char, width: usize) -> bool {
    let count = trimmed
        .chars()
        .take_while(|character| *character == fence)
        .count();
    count >= width
        && trimmed
            .chars()
            .skip(count)
            .all(|character| matches!(character, ' ' | '\t' | '\r' | '\n'))
}

/* -------------------------------------------------------------- inline code */

fn inline_code_ranges(source: &str, fences: &[(usize, usize)]) -> Vec<(usize, usize)> {
    let mut ranges = Vec::new();
    let mut byte = 0;
    while byte < source.len() {
        if in_ranges(byte, fences) {
            byte = next_range_end(byte, fences);
            continue;
        }
        if source[byte..].starts_with('`') {
            let width = count_run(source, byte, '`');
            let mut cursor = byte + width;
            let mut found = None;
            while cursor < source.len() {
                if in_ranges(cursor, fences) {
                    cursor = next_range_end(cursor, fences);
                    continue;
                }
                if source[cursor..].starts_with('`') {
                    let closing = count_run(source, cursor, '`');
                    if closing == width {
                        found = Some(cursor + closing);
                        break;
                    }
                    cursor += closing;
                } else {
                    cursor += next_char_len(source, cursor);
                }
            }
            if let Some(end) = found {
                ranges.push((byte, end));
                byte = end;
            } else {
                byte += width;
            }
            continue;
        }
        byte += next_char_len(source, byte);
    }
    ranges
}

/* --------------------------------------------------------------------- math */

fn scan_math(
    source: &str,
    mask: &[(usize, usize)],
    diagnostics: &mut Vec<Diagnostic>,
) -> Vec<(usize, usize)> {
    let mut ranges = Vec::new();
    let mut byte = 0;
    while byte < source.len() {
        if in_ranges(byte, mask) {
            byte = next_range_end(byte, mask);
            continue;
        }
        let character = source[byte..].chars().next().unwrap_or('\0');
        if character == '\\' {
            byte += 1 + next_char_len(source, byte + 1);
            continue;
        }
        if source[byte..].starts_with("$$") {
            if let Some(end) = find_unescaped(source, byte + 2, mask, "$$") {
                check_math(source, byte + 2, &source[byte + 2..end], diagnostics);
                ranges.push((byte, end + 2));
                byte = end + 2;
            } else {
                push(
                    source,
                    diagnostics,
                    byte,
                    byte + 2,
                    "数学公式没有闭合",
                    "补一个 `$$`，或删除起始的 `$$`。",
                    Origin::Source,
                );
                byte += 2;
            }
            continue;
        }
        if character == '$' {
            if let Some(end) = find_unescaped(source, byte + 1, mask, "$") {
                let inner = &source[byte + 1..end];
                if !inner.starts_with('$') {
                    check_math(source, byte + 1, inner, diagnostics);
                    ranges.push((byte, end + 1));
                    byte = end + 1;
                    continue;
                }
            }
            // A lone `$` is commonly currency; do not guess.
            byte += 1;
            continue;
        }
        byte += character.len_utf8();
    }
    merge_ranges(ranges)
}

fn find_unescaped(
    source: &str,
    from: usize,
    mask: &[(usize, usize)],
    needle: &str,
) -> Option<usize> {
    let mut byte = from;
    while byte < source.len() {
        if in_ranges(byte, mask) {
            byte = next_range_end(byte, mask);
            continue;
        }
        if source[byte..].starts_with('\\') {
            byte += 1 + next_char_len(source, byte + 1);
            continue;
        }
        if source[byte..].starts_with(needle) {
            // Inside `$$...$$`, an inline `$` must not close the display pair.
            if needle == "$" && source[byte..].starts_with("$$") {
                byte += 2;
                continue;
            }
            return Some(byte);
        }
        byte += next_char_len(source, byte);
    }
    None
}

fn check_math(source: &str, base: usize, inner: &str, diagnostics: &mut Vec<Diagnostic>) {
    let Err(error) = soma_math::parse(inner) else {
        return;
    };
    let (start, end) = math_error_span(inner, &error);
    push(
        source,
        diagnostics,
        base + start,
        base + end,
        format!("数学公式：{error}"),
        math_suggestion(&error),
        Origin::Source,
    );
}

fn math_error_span(inner: &str, error: &soma_math::MathError) -> (usize, usize) {
    use soma_math::MathError;
    let whole = (0, inner.len());
    match error {
        MathError::Empty => whole,
        MathError::MissingArgument => last_command_span(inner).unwrap_or(whole),
        MathError::UnsupportedCommand(name) => {
            let needle = format!("\\{name}");
            inner
                .find(&needle)
                .map(|start| (start, start + needle.len()))
                .unwrap_or(whole)
        }
        MathError::UnbalancedGroup => unbalanced_group_span(inner).unwrap_or(whole),
        MathError::UnbalancedDelimiter => ["\\left", "\\right"]
            .iter()
            .filter_map(|needle| {
                inner
                    .rfind(needle)
                    .map(|start| (start, start + needle.len()))
            })
            .max_by_key(|(start, _)| *start)
            .unwrap_or(whole),
        MathError::UnbalancedEnvironment(name) => {
            let end = format!("\\end{{{name}}}");
            let begin = format!("\\begin{{{name}}}");
            inner
                .rfind(&end)
                .map(|start| (start, start + end.len()))
                .or_else(|| {
                    inner
                        .rfind(&begin)
                        .map(|start| (start, start + begin.len()))
                })
                .unwrap_or(whole)
        }
    }
}

fn last_command_span(inner: &str) -> Option<(usize, usize)> {
    let start = inner.rfind('\\')?;
    let end = inner[start + 1..]
        .char_indices()
        .find(|(_, character)| !character.is_ascii_alphabetic())
        .map(|(offset, _)| start + 1 + offset)
        .unwrap_or(inner.len());
    (end > start + 1).then_some((start, end))
}

fn unbalanced_group_span(inner: &str) -> Option<(usize, usize)> {
    let mut stack = Vec::new();
    let mut last_close = None;
    let mut byte = 0;
    while byte < inner.len() {
        let character = inner[byte..].chars().next()?;
        if character == '\\' {
            byte += 1 + next_char_len(inner, byte + 1);
            continue;
        }
        match character {
            '{' => stack.push(byte),
            '}' if stack.pop().is_none() => last_close = Some(byte),
            '}' => {}
            _ => {}
        }
        byte += character.len_utf8();
    }
    stack
        .pop()
        .map(|start| (start, start + 1))
        .or_else(|| last_close.map(|start| (start, start + 1)))
}

fn math_suggestion(error: &soma_math::MathError) -> &'static str {
    use soma_math::MathError;
    match error {
        MathError::Empty => "删除空公式，或在 `$...$` 之间补上内容。",
        MathError::MissingArgument => "给命令补上参数，例如 `\\sqrt{x}`。",
        MathError::UnsupportedCommand(_) => {
            "改用受支持的数学命令；不确定时把这段放进 `\\text{...}` 或普通文本。"
        }
        MathError::UnbalancedGroup => "补齐缺少的 `}`，或删除多余的 `{`。",
        MathError::UnbalancedDelimiter => "让 `\\left` 与 `\\right` 成对出现。",
        MathError::UnbalancedEnvironment(_) => "检查 `\\begin{...}` 与 `\\end{...}` 是否成对。",
    }
}

/* --------------------------------------------------------------- extensions */

fn scan_extensions(source: &str, mask: &[(usize, usize)], diagnostics: &mut Vec<Diagnostic>) {
    let mut byte = 0;
    while byte < source.len() {
        if in_ranges(byte, mask) {
            byte = next_range_end(byte, mask);
            continue;
        }
        if source[byte..].starts_with("[[") {
            if let Some(end) = source[byte + 2..].find("]]") {
                byte += 2 + end + 2;
            } else {
                push(
                    source,
                    diagnostics,
                    byte,
                    byte + 2,
                    "扩展标记没有闭合",
                    "补齐 `]]`，或检查 `[[ruby:...]]` / `[[index:...]]` 的写法。",
                    Origin::Source,
                );
                byte += 2;
            }
            continue;
        }
        byte += next_char_len(source, byte);
    }
}

/* ---------------------------------------------------------------- footnotes */

fn scan_footnotes(source: &str, mask: &[(usize, usize)], diagnostics: &mut Vec<Diagnostic>) {
    let mut definitions: BTreeMap<String, (usize, usize)> = BTreeMap::new();
    let mut definition_offsets = BTreeSet::new();
    let mut offset = 0;
    for line in source.split_inclusive('\n') {
        if !in_ranges(offset, mask) {
            let trimmed = line.trim_start();
            let indent = line.len() - trimmed.len();
            if let Some(rest) = trimmed.strip_prefix("[^") {
                if let Some(close) = rest.find(']') {
                    let label = &rest[..close];
                    let after = &rest[close + 1..];
                    if !label.is_empty() && after.trim_start().starts_with(':') {
                        let start = offset + indent;
                        let end = start + 2 + label.len() + 1;
                        definition_offsets.insert(start);
                        if let Some((_, first_end)) = definitions.get(label) {
                            push(
                                source,
                                diagnostics,
                                start,
                                end,
                                format!("脚注定义重复：`{label}`"),
                                format!("删除后面这条定义，或给其中一个改成 `[^{label}2]`。"),
                                Origin::Source,
                            );
                            let _ = first_end;
                        } else {
                            definitions.insert(label.to_string(), (start, end));
                        }
                    }
                }
            }
        }
        offset += line.len();
    }

    let mut byte = 0;
    while byte < source.len() {
        if in_ranges(byte, mask) {
            byte = next_range_end(byte, mask);
            continue;
        }
        if source[byte..].starts_with("[^") && !definition_offsets.contains(&byte) {
            if let Some(close) = source[byte + 2..].find(']') {
                let end = byte + 2 + close + 1;
                let label = &source[byte + 2..byte + 2 + close];
                if !label.is_empty() && !definitions.contains_key(label) {
                    push(
                        source,
                        diagnostics,
                        byte,
                        end,
                        format!("脚注引用没有定义：`{label}`"),
                        format!("补一行 `[^{label}]: 内容`，或删除这个引用。"),
                        Origin::Source,
                    );
                }
                byte = end;
                continue;
            }
        }
        byte += next_char_len(source, byte);
    }
}

/* -------------------------------------------------------------- environments */

fn scan_environments(source: &str, mask: &[(usize, usize)], diagnostics: &mut Vec<Diagnostic>) {
    let events = environment_events(source, mask);
    let mut stack: Vec<(String, usize, usize)> = Vec::new();
    for (is_begin, name, start, end) in events {
        if is_begin {
            stack.push((name, start, end));
            continue;
        }
        if stack
            .last()
            .map(|(open, _, _)| open == &name)
            .unwrap_or(false)
        {
            stack.pop();
            continue;
        }
        if let Some(position) = stack.iter().rposition(|(open, _, _)| open == &name) {
            for (unclosed, begin, begin_end) in stack.drain(position + 1..) {
                push(
                    source,
                    diagnostics,
                    begin,
                    begin_end,
                    format!("环境没有结束：`{unclosed}`"),
                    format!("补 `\\end{{{unclosed}}}`。"),
                    Origin::Source,
                );
            }
            stack.pop();
            continue;
        }
        push(
            source,
            diagnostics,
            start,
            end,
            format!("环境结束不匹配：`{name}`"),
            format!("检查 `\\begin{{{name}}}` 是否缺失，或改成正确的环境名。"),
            Origin::Source,
        );
    }
    for (name, start, end) in stack {
        push(
            source,
            diagnostics,
            start,
            end,
            format!("环境没有结束：`{name}`"),
            format!("补 `\\end{{{name}}}`。"),
            Origin::Source,
        );
    }
}

fn environment_events(source: &str, mask: &[(usize, usize)]) -> Vec<(bool, String, usize, usize)> {
    let mut events = Vec::new();
    let mut byte = 0;
    while byte < source.len() {
        if in_ranges(byte, mask) {
            byte = next_range_end(byte, mask);
            continue;
        }
        let is_begin = source[byte..].starts_with("\\begin{");
        let is_end = source[byte..].starts_with("\\end{");
        if is_begin || is_end {
            let prefix = if is_begin { 7 } else { 5 };
            if let Some(close) = source[byte + prefix..].find('}') {
                let name = &source[byte + prefix..byte + prefix + close];
                let end = byte + prefix + close + 1;
                if !name.is_empty() {
                    events.push((is_begin, name.to_string(), byte, end));
                    byte = end;
                    continue;
                }
            }
        }
        byte += next_char_len(source, byte);
    }
    events
}

/* -------------------------------------------------------------------- build */

fn build_diagnostic(source: &str, format: SourceFormat, error: &str) -> Diagnostic {
    let mut diagnostic = Diagnostic {
        start: 0,
        end: 0,
        line: 0,
        column: 0,
        message: error.to_string(),
        suggestion: build_suggestion(error),
        origin: Origin::Build,
    };
    if let Some(node) = node_id(error) {
        if let Some((start, end)) = node_span(source, format, node) {
            diagnostic.start = clamp_start(source, start);
            diagnostic.end = clamp_end(source, diagnostic.start, end);
            let (line, column) = line_col(source, diagnostic.start);
            diagnostic.line = line;
            diagnostic.column = column;
        }
    }
    diagnostic
}

fn node_id(error: &str) -> Option<u32> {
    let rest = error.strip_prefix("node ")?;
    let digits = rest
        .chars()
        .take_while(|character| character.is_ascii_digit())
        .collect::<String>();
    digits.parse().ok()
}

fn node_span(source: &str, format: SourceFormat, node: u32) -> Option<(usize, usize)> {
    if format == SourceFormat::Docx {
        return None;
    }
    let parsed = soma_studio_core::parse_document(format, source.as_bytes()).ok()?;
    let json: serde_json::Value = serde_json::from_str(&parsed.document.canonical_json()).ok()?;
    crate::source::find_span(&json, i64::from(node))
}

fn build_suggestion(error: &str) -> String {
    if let Some(rest) = error.strip_prefix("template: ") {
        if let Some((line, column)) = line_column_prefix(rest) {
            return format!("打开「模板 YAML」，检查第 {line} 行第 {column} 列。");
        }
        return String::from("打开「模板 YAML」检查；这是模板问题，不在源文里。");
    }
    if error.starts_with("metrics:") {
        return String::from("检查度量表（.smt）是否与模板字体匹配。");
    }
    if error.starts_with("node ") {
        return String::from("该节点无法完成断行或布局；检查超长英文、公式或表格宽度。");
    }
    if error.contains("footnote") || error.contains("reference") {
        return String::from("检查脚注引用与定义是否一一对应。");
    }
    if line_column_prefix(error).is_some() {
        return String::from("检查引擎参数；这是配置问题，不在源文里。");
    }
    String::from("检查源文与模板；如果错误来自模板或引擎，面板会标为「构建」。")
}

fn line_column_prefix(text: &str) -> Option<(u32, u32)> {
    let mut parts = text.splitn(2, ':');
    let line = parts.next()?.trim().parse().ok()?;
    let rest = parts.next()?;
    let column = rest.split(':').next()?.trim().parse().ok()?;
    Some((line, column))
}

/* ------------------------------------------------------------------ normalize */

fn normalize(source: &str, mut diagnostics: Vec<Diagnostic>) -> Vec<Diagnostic> {
    diagnostics.retain(|diagnostic| !diagnostic.message.is_empty());
    for diagnostic in &mut diagnostics {
        diagnostic.start = clamp_start(source, diagnostic.start);
        diagnostic.end = clamp_end(source, diagnostic.start, diagnostic.end);
    }
    diagnostics.sort_by(|left, right| {
        let left_line = if left.line == 0 {
            usize::MAX
        } else {
            left.line
        };
        let right_line = if right.line == 0 {
            usize::MAX
        } else {
            right.line
        };
        left_line
            .cmp(&right_line)
            .then(left.start.cmp(&right.start))
            .then(left.message.cmp(&right.message))
    });
    diagnostics.dedup_by(|left, right| {
        left.start == right.start && left.end == right.end && left.message == right.message
    });
    diagnostics.truncate(48);
    diagnostics
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unclosed_fence_is_reported() {
        let source = "# 标题\n\n```rust\nfn main() {}\n";
        let diagnostics = analyze(source, SourceFormat::Markdown, None);
        let diagnostic = diagnostics
            .iter()
            .find(|diagnostic| diagnostic.message.contains("代码围栏"))
            .expect("fence diagnostic");
        assert_eq!(diagnostic.line, 3);
        assert_eq!(&source[diagnostic.start..diagnostic.end], "```");
    }

    #[test]
    fn undefined_footnote_is_reported() {
        let source = "正文[^missing]。\n";
        let diagnostics = analyze(source, SourceFormat::Markdown, None);
        let diagnostic = diagnostics
            .iter()
            .find(|diagnostic| diagnostic.message.contains("脚注引用"))
            .expect("footnote diagnostic");
        assert_eq!(&source[diagnostic.start..diagnostic.end], "[^missing]");
        assert!(diagnostic.suggestion.contains("[^missing]:"));
    }

    #[test]
    fn unsupported_math_command_is_reported() {
        let source = "公式 $\\unknown{x}$ 结束\n";
        let diagnostics = analyze(source, SourceFormat::Markdown, None);
        let diagnostic = diagnostics
            .iter()
            .find(|diagnostic| diagnostic.message.contains("unsupported math command"))
            .expect("math diagnostic");
        assert_eq!(&source[diagnostic.start..diagnostic.end], "\\unknown");
    }

    #[test]
    fn unmatched_environment_is_reported() {
        let source = "\\begin{itemize}\n\\item x\n";
        let diagnostics = analyze(source, SourceFormat::Latex, None);
        let diagnostic = diagnostics
            .iter()
            .find(|diagnostic| diagnostic.message.contains("环境没有结束"))
            .expect("environment diagnostic");
        assert_eq!(
            &source[diagnostic.start..diagnostic.end],
            "\\begin{itemize}"
        );
        assert!(diagnostic.suggestion.contains("\\end{itemize}"));
    }

    #[test]
    fn build_error_maps_a_node_to_a_line() {
        let source = "Hello\n\nWorld\n";
        let diagnostics = analyze(
            source,
            SourceFormat::Markdown,
            Some("node 1: line breaking failed"),
        );
        assert!(diagnostics
            .iter()
            .any(|diagnostic| diagnostic.origin == Origin::Build && diagnostic.line > 0));
    }

    #[test]
    fn the_default_welcome_source_is_clean() {
        let source = include_str!("../../../examples/book/welcome.md");
        let diagnostics = analyze(source, SourceFormat::Markdown, None);
        assert!(
            diagnostics.is_empty(),
            "welcome.md should not produce diagnostics: {diagnostics:#?}"
        );
    }

    #[test]
    fn line_column_is_one_based() {
        assert_eq!(line_col("a\nbc", 0), (1, 1));
        assert_eq!(line_col("a\nbc", 2), (2, 1));
        assert_eq!(line_col("a\nbc", 3), (2, 2));
    }
}
