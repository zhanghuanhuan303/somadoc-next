//! Liang hyphenation.
//!
//! Patterns are data supplied by the host: the crate ships no dictionary, so a
//! build has no language tables baked in and a caller chooses (and licences)
//! its own set.
//!
//! Matching happens on characters because a pattern is a sequence of letters,
//! but the caller receives UTF-8 byte offsets because that is what a text
//! buffer needs in order to split a word. Lowercasing can turn one character
//! into several (for example `İ`), so matching keeps a map back to the
//! original character boundaries and never reports a break inside a code
//! point.

#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]

extern crate alloc;

use alloc::boxed::Box;
use alloc::vec::Vec;

/// One parsed pattern: its letters and the weights between them.
///
/// `weights.len() == letters.len() + 1`: the first weight precedes the first
/// letter and the last one follows the last letter.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Pattern {
    letters: Box<[char]>,
    weights: Box<[u8]>,
}

/// A hyphenation pattern set.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Patterns {
    /// Sorted by `letters`; duplicate letter sequences are merged by taking
    /// the maximum weight at each position.
    patterns: Vec<Pattern>,
    /// Longest letter sequence, which bounds the windows tried at each start.
    max_letters: usize,
}

impl Patterns {
    /// Parse a TeX-style pattern file.
    ///
    /// A pattern is a run of letters with optional digits between or at its
    /// ends; the digits are weights. `%` starts a comment, blank lines are
    /// ignored, and a `\patterns{ ... }` wrapper is accepted (parsing stops at
    /// its closing brace). Other lines beginning with `\` are TeX commands
    /// rather than patterns and are ignored. Pattern letters are lowercased so
    /// a table written in either case still matches the lowercased word.
    pub fn parse(source: &str) -> Self {
        let mut parsed = Vec::new();
        let mut in_block = false;

        for raw_line in source.lines() {
            let line = raw_line
                .split_once('%')
                .map_or(raw_line, |(before, _)| before);
            if in_block {
                let (content, closed) = match line.find('}') {
                    Some(close) => (&line[..close], true),
                    None => (line, false),
                };
                parse_patterns(content, &mut parsed);
                if closed {
                    break;
                }
            } else {
                let line = line.trim();
                if line.is_empty() {
                    continue;
                }
                if let Some(rest) = line.strip_prefix("\\patterns") {
                    let rest = rest.trim_start();
                    if let Some(rest) = rest.strip_prefix('{') {
                        in_block = true;
                        let (content, closed) = match rest.find('}') {
                            Some(close) => (&rest[..close], true),
                            None => (rest, false),
                        };
                        parse_patterns(content, &mut parsed);
                        if closed {
                            break;
                        }
                    }
                    continue;
                }
                if line.starts_with('\\') {
                    continue;
                }
                parse_patterns(line, &mut parsed);
            }
        }

        parsed.sort_unstable_by(|left, right| left.letters.cmp(&right.letters));
        let mut patterns: Vec<Pattern> = Vec::with_capacity(parsed.len());
        for pattern in parsed {
            match patterns.last_mut() {
                Some(previous) if previous.letters == pattern.letters => {
                    for (slot, weight) in previous.weights.iter_mut().zip(pattern.weights.iter()) {
                        *slot = (*slot).max(*weight);
                    }
                }
                _ => patterns.push(pattern),
            }
        }
        let max_letters = patterns
            .iter()
            .map(|pattern| pattern.letters.len())
            .max()
            .unwrap_or(0);
        Self {
            patterns,
            max_letters,
        }
    }

    /// Whether the set contains no usable pattern.
    pub fn is_empty(&self) -> bool {
        self.patterns.is_empty()
    }

    /// Byte offsets inside `word` where a hyphen may be inserted, using
    /// [`Limits::default`].
    pub fn breaks(&self, word: &str) -> Vec<u32> {
        self.breaks_with(word, Limits::default())
    }

    /// Byte offsets inside `word` where a hyphen may be inserted.
    ///
    /// Offsets are ascending and unique, and always fall on a character
    /// boundary. A break leaves at least `limits.left` original characters
    /// before it and `limits.right` after it; the positions before the first
    /// letter and after the last letter are never returned.
    ///
    /// Only words made entirely of alphabetic characters are considered.
    /// Callers pass Latin runs, but the guard keeps punctuation, digits, and
    /// mixed text from producing a break next to a character that the
    /// patterns cannot describe.
    pub fn breaks_with(&self, word: &str, limits: Limits) -> Vec<u32> {
        if self.is_empty() || word.is_empty() || !word.chars().all(char::is_alphabetic) {
            return Vec::new();
        }

        // A dot on each side lets patterns describe word starts and ends, as
        // TeX pattern files do.
        let mut padded = Vec::with_capacity(word.len() + 2);
        padded.push('.');
        let mut char_ends = Vec::with_capacity(word.len());
        let mut break_positions = Vec::with_capacity(word.len());
        for (start, character) in word.char_indices() {
            for lowercase in character.to_lowercase() {
                padded.push(lowercase);
            }
            char_ends.push(start + character.len_utf8());
            // Padded index immediately after this original character. If one
            // character lowercases to several, this skips the interior
            // positions, so a break can only land on an original boundary.
            break_positions.push(padded.len());
        }
        padded.push('.');

        let mut values = alloc::vec![0u8; padded.len() + 1];
        // Every matching pattern contributes; TeX keeps the maximum weight
        // at each boundary rather than only the longest match.
        for start in 0..padded.len() {
            let longest = self.max_letters.min(padded.len() - start);
            for length in 1..=longest {
                let candidate = &padded[start..start + length];
                let found = self
                    .patterns
                    .binary_search_by(|pattern| pattern.letters.as_ref().cmp(candidate));
                if let Ok(index) = found {
                    for (offset, &weight) in self.patterns[index].weights.iter().enumerate() {
                        let at = start + offset;
                        values[at] = values[at].max(weight);
                    }
                }
            }
        }

        let left = usize::from(limits.left);
        let right = usize::from(limits.right);
        let characters = char_ends.len();
        let mut breaks = Vec::new();
        for (index, &position) in break_positions.iter().enumerate() {
            // The final letter has no following letter to carry the rest of
            // the word, so a break there is not an insertion point.
            if index + 1 == characters {
                continue;
            }
            let before = index + 1;
            let after = characters - before;
            if before < left || after < right {
                continue;
            }
            if values[position] % 2 == 1 {
                if let Ok(offset) = u32::try_from(char_ends[index]) {
                    breaks.push(offset);
                }
            }
        }
        breaks.dedup();
        breaks
    }
}

/// Minimum letters to leave on each side of a hyphen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    /// Characters that must remain before the hyphen.
    pub left: u8,
    /// Characters that must remain after the hyphen.
    pub right: u8,
}

impl Default for Limits {
    /// The usual English defaults: `left = 2`, `right = 3`.
    fn default() -> Self {
        Self { left: 2, right: 3 }
    }
}

/// Byte offsets inside `word` where a hyphen may be inserted, using
/// [`Limits::default`].
///
/// This is the free-function form of [`Patterns::breaks`].
pub fn breaks(patterns: &Patterns, word: &str) -> Vec<u32> {
    patterns.breaks(word)
}

/// Parse the whitespace-separated patterns in one source line or block.
///
/// A digit sets the weight at the current letter boundary; a second digit in
/// the same gap replaces the first, which is how TeX reads `a12b`.
fn parse_patterns(content: &str, patterns: &mut Vec<Pattern>) {
    for token in content.split_whitespace() {
        // A brace can be left on its own line when `\patterns` and `{` are
        // separated; it is wrapper syntax, not a pattern.
        if token == "{" || token == "}" {
            continue;
        }
        let mut letters = Vec::new();
        let mut weights = Vec::new();
        weights.push(0u8);
        for character in token.chars() {
            if character.is_ascii_digit() {
                if let Some(slot) = weights.last_mut() {
                    *slot = character as u8 - b'0';
                }
            } else {
                for lowercase in character.to_lowercase() {
                    letters.push(lowercase);
                    weights.push(0u8);
                }
            }
        }
        if !letters.is_empty() {
            patterns.push(Pattern {
                letters: letters.into_boxed_slice(),
                weights: weights.into_boxed_slice(),
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hyphenation_patterns() -> Patterns {
        Patterns::parse(".hy3ph\nhe2n\nhena4\nhen5at\nn2at\no1t\no1n\n2io\n1tion\n")
    }

    #[test]
    fn parses_comments_blank_lines_and_a_patterns_block() {
        let source = r#"% a TeX-like table

\patterns{
.hy3ph % break after "hy"
he2n

hena4
hen5at
n2at
o1t
2io
1tion
}
ignored-after-the-block
"#;
        let patterns = Patterns::parse(source);
        assert!(!patterns.is_empty());
        assert_eq!(patterns.breaks("hyphenation"), vec![2, 6]);
    }

    #[test]
    fn a_patterns_block_stops_at_its_closing_brace() {
        let patterns = Patterns::parse("\\patterns{a1b}\nc1d\n");
        assert!(!patterns.is_empty());

        let limits = Limits { left: 1, right: 1 };
        assert_eq!(patterns.breaks_with("ab", limits), vec![1]);
        // `c1d` is after the closing brace and must not have been parsed.
        assert!(patterns.breaks_with("cd", limits).is_empty());
    }

    #[test]
    fn digits_at_pattern_edges_are_weights() {
        let patterns = Patterns::parse("1ab\nab1\n");
        let limits = Limits { left: 1, right: 1 };
        assert_eq!(patterns.breaks_with("xabx", limits), vec![1, 3]);
    }

    #[test]
    fn classic_patterns_hyphenate_hyphenation() {
        let patterns = hyphenation_patterns();
        assert_eq!(patterns.breaks("hyphenation"), vec![2, 6]);
    }

    #[test]
    fn limits_keep_hyphens_away_from_the_edges() {
        let patterns = hyphenation_patterns();

        assert_eq!(
            patterns.breaks_with("hyphenation", Limits { left: 2, right: 1 },),
            vec![2, 6, 10]
        );
        assert_eq!(
            patterns.breaks_with("hyphenation", Limits { left: 3, right: 3 },),
            vec![6]
        );
        assert_eq!(
            patterns.breaks_with("hyphenation", Limits { left: 2, right: 6 },),
            vec![2]
        );
        assert!(patterns
            .breaks_with("hyphenation", Limits { left: 8, right: 3 },)
            .is_empty());
    }

    #[test]
    fn utf8_words_report_byte_offsets_on_character_boundaries() {
        let patterns = Patterns::parse("f1é\né1s\n");
        let limits = Limits { left: 2, right: 1 };

        let cafe = "café";
        let offsets = patterns.breaks_with(cafe, limits);
        assert_eq!(offsets, vec![3]);
        assert!(offsets
            .iter()
            .all(|&offset| cafe.is_char_boundary(offset as usize)));

        let cafes = "cafés";
        let offsets = patterns.breaks_with(cafes, limits);
        assert_eq!(offsets, vec![3, 5]);
        assert!(offsets
            .iter()
            .all(|&offset| cafes.is_char_boundary(offset as usize)));
        assert_eq!(cafes.len(), 6);
    }

    #[test]
    fn duplicate_patterns_do_not_duplicate_breaks() {
        let patterns = Patterns::parse("a1a\na3a\na1a\n");
        let offsets = patterns.breaks_with("aaaa", Limits { left: 1, right: 1 });
        assert_eq!(offsets, vec![1, 2, 3]);
        assert!(offsets.windows(2).all(|pair| pair[0] < pair[1]));
    }

    #[test]
    fn empty_input_and_non_letter_text_are_safe() {
        let patterns = hyphenation_patterns();
        assert!(patterns.breaks("").is_empty());
        assert!(patterns
            .breaks_with("", Limits { left: 0, right: 0 },)
            .is_empty());
        assert!(patterns.breaks("123").is_empty());
        assert!(patterns.breaks("hy-phen").is_empty());
        assert!(patterns.breaks("café123").is_empty());
        assert!(patterns.breaks("🙂").is_empty());

        let empty = Patterns::parse("% only comments\n\n");
        assert!(empty.is_empty());
        assert!(empty.breaks("hyphenation").is_empty());
    }

    #[test]
    fn matching_is_case_insensitive() {
        let patterns = Patterns::parse(".HY3PH\n");
        assert_eq!(patterns.breaks("Hyphenation"), vec![2]);
    }

    #[test]
    fn other_tex_commands_are_ignored() {
        let patterns = Patterns::parse("\\hyphenation{hy-phen}\n.hy3ph\n");
        assert_eq!(patterns.breaks("hyphenation"), vec![2]);
    }
}
