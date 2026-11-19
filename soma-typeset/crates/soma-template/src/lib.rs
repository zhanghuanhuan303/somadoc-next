//! `soma-template`: pure-YAML templates, exact units, style resolution.
//!
//! Layers (architecture v2 §7, ADR-0003, ADR-0009, ADR-0010):
//!
//! - a template is *presentation only*: page geometry, style rules, footnote
//!   numbering. It cannot write engine knobs;
//! - parsing produces a validated [`Template`] plus a deterministic hash;
//! - [`resolve`] combines a [`soma_doc::Document`] with a [`Template`] and
//!   produces [`StyledDoc`] — pure data with no lifetime, so it can be cached,
//!   hashed, and shared across threads;
//! - units are exact: decimals become rationals and every length resolves to
//!   1/4096 em with truncation toward zero. There is no floating point.
//!
//! The YAML dialect is a documented subset (`crate::yaml`); everything outside
//! it fails with a line/column error instead of being guessed.

#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]

extern crate alloc;

pub mod canonical;
pub mod style;
pub mod template;
pub mod units;
mod yaml;

pub use style::{resolve, ComputedStyle, ResolvedPage, StyleTable, StyledDoc, TableRules};
pub use template::{
    parse_template, Declaration, FontSlant, FontWeight, FootnoteNumbering, FootnoteSpec, PageSize,
    PageSpec, Property, PropertyValue, Selector, StyleRule, TableSpec, Template, ELEMENTS, SCHEMA,
};
pub use units::{LengthSpec, LineHeightSpec, Rational, Unit, UNITS_PER_EM};

use alloc::string::String;
use core::fmt;

/// Hash of a template's canonical JSON.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TemplateHash(pub u64);

/// Hash of a resolved style layer's canonical JSON.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct StyleHash(pub u64);

/// A template parse/resolve failure with a source position.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TemplateError {
    /// 1-based line.
    pub line: u32,
    /// 1-based column.
    pub column: u32,
    /// Logical path, e.g. `styles[2].declarations`.
    pub path: String,
    /// Short, stable reason. Not localized.
    pub reason: &'static str,
}

impl TemplateError {
    pub fn new(line: u32, column: u32, path: impl Into<String>, reason: &'static str) -> Self {
        Self {
            line,
            column,
            path: path.into(),
            reason,
        }
    }
}

impl fmt::Display for TemplateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.path.is_empty() {
            write!(f, "{}:{}: {}", self.line, self.column, self.reason)
        } else {
            write!(
                f,
                "{}:{}: {}: {}",
                self.line, self.column, self.path, self.reason
            )
        }
    }
}
