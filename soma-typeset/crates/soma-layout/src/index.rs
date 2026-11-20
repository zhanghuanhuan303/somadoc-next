//! Index terms: structure in, entries out.
//!
//! A marker carries LaTeX's index grammar, which is what a book author already
//! knows:
//!
//! ```text
//! 宋体                     an entry
//! 宋体!字面                a sub-entry
//! 宋体@songti              sort under `songti`, print `宋体`
//! 宋体!字面@ziti           per-part sort keys
//! 宋体|see{汉字}           "see" instead of page numbers
//! 宋体|seealso{黑体}       "see also"
//! ```
//!
//! The frontends pass the raw string through (`soma-latex` from `\index{}`,
//! `soma-md` from `[[index:...]]`), and the layout parses it once, when it
//! builds the index. Keeping the parse here — rather than in each frontend —
//! means a new frontend gets the whole grammar for free.

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::cmp::Ordering;

use soma_collate::Collation;

/// A cross-reference instead of page numbers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SeeKind {
    /// "see" — the term is not used, look there instead.
    See,
    /// "see also" — the term is used, and there is related material.
    SeeAlso,
}

impl SeeKind {
    /// The word a rendered index prints before the target.
    pub const fn label(self) -> &'static str {
        match self {
            Self::See => "见",
            Self::SeeAlso => "参见",
        }
    }
}

/// One parsed marker.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IndexTerm {
    /// The text this marker contributes.
    pub display: String,
    /// The text it sorts under (the display text unless `@` said otherwise).
    pub sort: String,
    /// Zero for a main entry, one for a sub-entry, and so on.
    pub level: u8,
    /// `(display, sort)` of every ancestor, outermost first.
    pub parents: Vec<(String, String)>,
    pub see: Option<(SeeKind, String)>,
}

/// Parse one marker. Malformed directives are ignored rather than fatal: an
/// index entry is never worth failing a page over.
pub fn parse(raw: &str) -> IndexTerm {
    let (path, directive) = split_once(raw, '|');
    let mut parts: Vec<(String, String)> = Vec::new();
    for part in split_parts(path) {
        let (display, sort) = split_once(&part, '@');
        let display = unescape(display.trim());
        let sort = if sort.trim().is_empty() {
            display.clone()
        } else {
            unescape(sort.trim())
        };
        if !display.is_empty() {
            parts.push((display, sort));
        }
    }
    if parts.is_empty() {
        return IndexTerm {
            display: String::new(),
            sort: String::new(),
            level: 0,
            parents: Vec::new(),
            see: None,
        };
    }
    let see = parse_see(directive.trim());
    let level = (parts.len() - 1).min(u8::MAX as usize) as u8;
    let (display, sort) = parts[parts.len() - 1].clone();
    let parents = parts[..parts.len() - 1].to_vec();
    IndexTerm {
        display,
        sort,
        level,
        parents,
        see,
    }
}

fn parse_see(directive: &str) -> Option<(SeeKind, String)> {
    for (prefix, kind) in [("seealso{", SeeKind::SeeAlso), ("see{", SeeKind::See)] {
        if let Some(rest) = directive.strip_prefix(prefix) {
            let target = rest.strip_suffix('}').unwrap_or(rest).trim();
            if !target.is_empty() {
                return Some((kind, target.to_string()));
            }
        }
    }
    None
}

/// `a!b!c` → `["a", "b", "c"]`, keeping `\!` and `\@` for later.
fn split_parts(path: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut characters = path.chars();
    while let Some(character) = characters.next() {
        match character {
            '\\' => {
                current.push('\\');
                if let Some(next) = characters.next() {
                    current.push(next);
                }
            }
            '!' => parts.push(core::mem::take(&mut current)),
            other => current.push(other),
        }
    }
    parts.push(current);
    parts
}

/// Drop the backslashes that protected a separator.
fn unescape(text: &str) -> String {
    if !text.contains('\\') {
        return text.to_string();
    }
    let mut out = String::new();
    let mut characters = text.chars();
    while let Some(character) = characters.next() {
        if character == '\\' {
            if let Some(next) = characters.next() {
                out.push(next);
            }
        } else {
            out.push(character);
        }
    }
    out
}

fn split_once(text: &str, separator: char) -> (&str, &str) {
    let mut characters = text.char_indices();
    while let Some((index, character)) = characters.next() {
        if character == '\\' {
            characters.next();
            continue;
        }
        if character == separator {
            return (&text[..index], &text[index + character.len_utf8()..]);
        }
    }
    (text, "")
}

/// One printed index row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    /// Nesting depth: 0 for a main entry.
    pub level: u8,
    /// Text printed in the entry column.
    pub text: String,
    /// Page labels, in document order, without repeats.
    pub pages: Vec<String>,
    /// `see` / `see also` target, when the marker asked for one.
    pub see: Option<(SeeKind, String)>,
}

/// A group of markers that share one path through the index tree.
struct Group {
    keys: Vec<String>,
    display: Vec<String>,
    pages: Vec<String>,
    see: Option<(SeeKind, String)>,
}

/// Turn markers into printed rows: merge duplicates, invent the ancestors of
/// orphan sub-entries, order by the requested collation, and flatten to a
/// depth-first list that a renderer can indent.
pub fn rows(
    markers: &[(IndexTerm, usize)],
    label_of: &dyn Fn(usize) -> String,
    collation: Collation,
) -> Vec<Row> {
    let mut groups: Vec<Group> = Vec::new();
    for (term, item) in markers {
        if term.display.is_empty() {
            continue;
        }
        let mut keys: Vec<String> = term.parents.iter().map(|(_, sort)| sort.clone()).collect();
        keys.push(term.sort.clone());
        let mut display: Vec<String> = term.parents.iter().map(|(name, _)| name.clone()).collect();
        display.push(term.display.clone());
        let at = match groups.iter().position(|group| group.keys == keys) {
            Some(at) => at,
            None => {
                groups.push(Group {
                    keys: keys.clone(),
                    display: display.clone(),
                    pages: Vec::new(),
                    see: None,
                });
                groups.len() - 1
            }
        };
        let group = &mut groups[at];
        if group.see.is_none() {
            group.see = term.see.clone();
        }
        // A `see` entry points elsewhere; listing pages as well would claim the
        // term is discussed here when the marker says it is not.
        if group.see.is_none() || matches!(group.see, Some((SeeKind::SeeAlso, _))) {
            let label = label_of(*item);
            if !label.is_empty() && !group.pages.contains(&label) {
                group.pages.push(label);
            }
        }
    }

    // A sub-entry whose parent was never marked still needs its parent line.
    let orphans: Vec<Group> = groups
        .iter()
        .filter_map(|group| {
            let last = group.keys.len();
            if last < 2
                || groups
                    .iter()
                    .any(|other| other.keys == group.keys[..last - 1])
            {
                return None;
            }
            Some(Group {
                keys: group.keys[..last - 1].to_vec(),
                display: group.display[..last - 1].to_vec(),
                pages: Vec::new(),
                see: None,
            })
        })
        .collect();
    for orphan in orphans {
        if !groups.iter().any(|group| group.keys == orphan.keys) {
            groups.push(orphan);
        }
    }

    groups.sort_by(|left, right| compare_paths(&left.keys, &right.keys, collation));
    groups
        .into_iter()
        .map(|group| Row {
            level: (group.keys.len().saturating_sub(1)).min(3) as u8,
            text: group.display.last().cloned().unwrap_or_default(),
            pages: group.pages,
            see: group.see,
        })
        .collect()
}

fn compare_paths(left: &[String], right: &[String], collation: Collation) -> Ordering {
    for (left, right) in left.iter().zip(right) {
        let order = soma_collate::compare(left, right, collation);
        if order != Ordering::Equal {
            return order;
        }
    }
    left.len().cmp(&right.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plain_term_is_a_main_entry() {
        let term = parse("宋体");
        assert_eq!(term.display, "宋体");
        assert_eq!(term.sort, "宋体");
        assert_eq!(term.level, 0);
        assert!(term.parents.is_empty());
        assert!(term.see.is_none());
    }

    #[test]
    fn a_bang_makes_a_sub_entry_and_keeps_its_parent() {
        let term = parse("宋体!字面");
        assert_eq!(term.display, "字面");
        assert_eq!(term.level, 1);
        assert_eq!(term.parents, [("宋体".to_string(), "宋体".to_string())]);
    }

    #[test]
    fn an_at_sign_replaces_the_sort_key() {
        let term = parse("宋体@R");
        assert_eq!(term.display, "宋体");
        assert_eq!(term.sort, "R");
        let nested = parse("宋体!字面@zi");
        assert_eq!(nested.sort, "zi");
        assert_eq!(nested.parents[0].1, "宋体");
    }

    #[test]
    fn see_and_see_also_are_recognised() {
        assert_eq!(
            parse("宋体|see{汉字}").see,
            Some((SeeKind::See, "汉字".to_string()))
        );
        assert_eq!(
            parse("宋体|seealso{黑体}").see,
            Some((SeeKind::SeeAlso, "黑体".to_string()))
        );
    }

    #[test]
    fn an_escaped_separator_stays_in_the_text() {
        let term = parse("a\\!b");
        assert_eq!(term.display, "a!b");
        assert_eq!(term.level, 0);
        let nested = parse("a\\@b");
        assert_eq!(nested.display, "a@b");
    }

    #[test]
    fn an_empty_marker_is_empty() {
        let term = parse("  ");
        assert!(term.display.is_empty());
        assert_eq!(term.level, 0);
    }

    fn label_of(_: usize) -> String {
        String::from("7")
    }

    #[test]
    fn rows_merge_duplicates_and_keep_page_order() {
        let markers = [(parse("排版"), 0), (parse("引擎"), 1), (parse("排版"), 2)];
        let rows = rows(&markers, &label_of, Collation::Codepoint);
        assert_eq!(rows.len(), 2);
        // 引擎 (U+5F15) sorts before 排版 (U+6392).
        assert_eq!(rows[0].text, "引擎");
        assert_eq!(rows[1].text, "排版");
        assert_eq!(rows[1].pages, [String::from("7")]);
    }

    #[test]
    fn rows_sort_by_the_at_sign_key() {
        let markers = [(parse("阿@Z"), 0), (parse("八@A"), 1)];
        let rows = rows(&markers, &label_of, Collation::Codepoint);
        assert_eq!(rows[0].text, "八", "A sorts before Z");
        assert_eq!(rows[1].text, "阿");
    }

    #[test]
    fn rows_give_an_orphan_sub_entry_its_parent_line() {
        let markers = [(parse("排版!行宽"), 0)];
        let rows = rows(&markers, &label_of, Collation::Codepoint);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].text, "排版");
        assert_eq!(rows[0].level, 0);
        assert!(rows[0].pages.is_empty());
        assert_eq!(rows[1].text, "行宽");
        assert_eq!(rows[1].level, 1);
    }

    #[test]
    fn a_see_marker_suppresses_its_page_numbers() {
        let markers = [(parse("排版|see{汉字}"), 0)];
        let rows = super::rows(&markers, &label_of, Collation::Codepoint);
        assert_eq!(rows.len(), 1);
        assert!(rows[0].pages.is_empty());
        assert_eq!(rows[0].see, Some((SeeKind::See, String::from("汉字"))));
        // `see also` is an additional pointer, so pages are still listed.
        let also = [(parse("排版|seealso{汉字}"), 0)];
        let with_pages = super::rows(&also, &label_of, Collation::Codepoint);
        assert_eq!(with_pages[0].pages, [String::from("7")]);
    }
}
