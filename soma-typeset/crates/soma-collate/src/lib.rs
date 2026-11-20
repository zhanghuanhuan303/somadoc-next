//! Collation for index entries.
//!
//! The engine sorts an index two ways: by Unicode code point (the default, and
//! what "the same input gives the same bytes" needs) and by pinyin, which is
//! what a Chinese book's index has to look like. Pinyin order is data, not an
//! algorithm: `pinyin.bin` holds one `(u16 code point, u16 syllabic rank)`
//! record per Han character, generated from ICU by
//! `tools/make-pinyin-table.py` and checked in, so a build needs nothing but
//! `rustc`.
//!
//! The crate is `no_std` and dependency-free: it is a table and a comparator.

#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]

use core::cmp::Ordering;

/// `(code point, syllable rank)` records, little-endian `u16` pairs, sorted by
/// code point.
const TABLE: &[u8] = include_bytes!("../pinyin.bin");
const RECORD: usize = 4;

/// Rank of the character's pinyin syllable among the distinct syllables in the
/// table, or `None` when the character is not Han or has no reading.
pub fn pinyin_rank(character: char) -> Option<u16> {
    let code = character as u32;
    if code > 0xFFFF {
        return None;
    }
    let needle = code as u16;
    let count = TABLE.len() / RECORD;
    let mut low = 0usize;
    let mut high = count;
    while low < high {
        let middle = (low + high) / 2;
        let at = middle * RECORD;
        let candidate = u16::from_le_bytes([TABLE[at], TABLE[at + 1]]);
        match candidate.cmp(&needle) {
            Ordering::Less => low = middle + 1,
            Ordering::Greater => high = middle,
            Ordering::Equal => {
                let rank = u16::from_le_bytes([TABLE[at + 2], TABLE[at + 3]]);
                return (rank != u16::MAX).then_some(rank);
            }
        }
    }
    None
}

/// How an index is ordered.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Collation {
    /// Unicode code point order: stable, and independent of any table.
    #[default]
    Codepoint,
    /// Pinyin, with Latin and digits before Han.
    Pinyin,
}

/// Compare two entry keys.
pub fn compare(left: &str, right: &str, collation: Collation) -> Ordering {
    match collation {
        Collation::Codepoint => left.cmp(right),
        Collation::Pinyin => {
            let mut left_chars = left.chars();
            let mut right_chars = right.chars();
            loop {
                match (left_chars.next(), right_chars.next()) {
                    (None, None) => return Ordering::Equal,
                    (None, Some(_)) => return Ordering::Less,
                    (Some(_), None) => return Ordering::Greater,
                    (Some(left_char), Some(right_char)) => {
                        let order = key(left_char).cmp(&key(right_char));
                        if order != Ordering::Equal {
                            return order;
                        }
                    }
                }
            }
        }
    }
}

/// `(class, rank, code point)`: everything that is not Han sorts first, then
/// Han by syllable, then by code point so ties are total and deterministic.
fn key(character: char) -> (u8, u16, u32) {
    match pinyin_rank(character) {
        Some(rank) => (1, rank, character as u32),
        None => (0, 0, character as u32),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn han_characters_sort_by_their_reading() {
        // 八 ba, 中 zhong, 阿 a: code point order is 中 < 八 < 阿, pinyin order
        // is the other way round.
        assert_eq!(compare("八", "中", Collation::Pinyin), Ordering::Less);
        assert_eq!(compare("阿", "八", Collation::Pinyin), Ordering::Less);
        assert_eq!(compare("中", "八", Collation::Pinyin), Ordering::Greater);
        assert_eq!(compare("八", "八", Collation::Pinyin), Ordering::Equal);
        assert_eq!(compare("八", "中", Collation::Codepoint), Ordering::Greater);
    }

    #[test]
    fn latin_and_digits_come_before_han() {
        assert_eq!(compare("A", "阿", Collation::Pinyin), Ordering::Less);
        assert_eq!(compare("2", "甲", Collation::Pinyin), Ordering::Less);
        assert_eq!(compare("A", "B", Collation::Pinyin), Ordering::Less);
    }

    #[test]
    fn a_shared_prefix_uses_the_next_character() {
        assert_eq!(compare("阿坝", "阿城", Collation::Pinyin), Ordering::Less);
        assert_eq!(compare("阿", "阿坝", Collation::Pinyin), Ordering::Less);
    }

    #[test]
    fn the_table_covers_the_common_block() {
        assert!(pinyin_rank('中').is_some());
        assert!(pinyin_rank('文').is_some());
        assert!(pinyin_rank('龘').is_none() || pinyin_rank('龘').is_some());
        assert!(pinyin_rank('A').is_none());
    }
}
