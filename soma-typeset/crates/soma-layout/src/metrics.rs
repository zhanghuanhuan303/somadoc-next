//! Metrics adapters that turn a real font table into the kernel seam.
//!
//! Two adapters live here:
//!
//! - [`TableMetrics`] measures everything at one face and one size. It is the
//!   minimal adapter for demos and kernel-level experiments.
//! - [`StyledMetrics`] maps each `StyleId` to a face and a font size through the
//!   resolved style table, so headings and inline emphasis measure at their own
//!   size. This is the adapter the document pipeline uses.
//!
//! Both are pure functions of `(table, style)`; no state, no cache, no fonts.

use soma_core::{LineMetrics, Metrics, StyleId, Width};
use soma_metrics::MetricsTable;
use soma_template::{ComputedStyle, FontWeight, StyledDoc};

/// Em size used by the kernel: one em is 4096 units.
const EM: i64 = 4096;

/// One face at one font size.
pub struct TableMetrics<'a> {
    table: &'a MetricsTable,
    face: usize,
    size: Width,
}

impl<'a> TableMetrics<'a> {
    pub fn new(table: &'a MetricsTable, face: usize, size: Width) -> Self {
        Self { table, face, size }
    }

    fn face(&self) -> Option<&soma_metrics::Face> {
        self.table.face(self.face)
    }
}

impl Metrics for TableMetrics<'_> {
    fn advance(&self, text: &str, _style: StyleId) -> Width {
        match self.face() {
            Some(face) => Width(scale(face.advance(text), self.size.0)),
            None => Width::ZERO,
        }
    }

    fn line_metrics(&self, _style: StyleId) -> LineMetrics {
        match self.face() {
            Some(face) => LineMetrics {
                ascent: Width(scale(face.ascent, self.size.0)),
                descent: Width(scale(face.descent, self.size.0)),
                leading: Width(scale(face.leading, self.size.0)),
            },
            None => LineMetrics::default(),
        }
    }

    fn space_width(&self, style: StyleId) -> Width {
        match self.face() {
            Some(face) => Width(scale(face.space, self.size.0)),
            None => self.advance(" ", style),
        }
    }

    fn version(&self) -> u16 {
        soma_metrics::VERSION
    }

    fn hash(&self) -> u64 {
        self.table.hash()
    }
}

/// Per-style metrics from the resolved style table.
pub struct StyledMetrics<'a> {
    table: &'a MetricsTable,
    styled: &'a StyledDoc,
}

impl<'a> StyledMetrics<'a> {
    pub fn new(table: &'a MetricsTable, styled: &'a StyledDoc) -> Self {
        Self { table, styled }
    }

    pub fn table(&self) -> &MetricsTable {
        self.table
    }

    fn computed(&self, style: StyleId) -> ComputedStyle {
        self.styled
            .style(style)
            .cloned()
            .unwrap_or_else(ComputedStyle::base)
    }

    fn face_of(&self, computed: &ComputedStyle) -> Option<&soma_metrics::Face> {
        let index = face_index(computed);
        // Faces are laid out as regular(serif, sans), bold(serif, sans).
        // Fall back to the same family when the bold face is missing.
        self.table
            .face(index)
            .or_else(|| self.table.face(index % 2))
            .or_else(|| self.table.face(0))
    }
}

impl Metrics for StyledMetrics<'_> {
    fn advance(&self, text: &str, style: StyleId) -> Width {
        let computed = self.computed(style);
        match self.face_of(&computed) {
            Some(face) => Width(scale(face.advance(text), computed.font_size.0)),
            None => Width::ZERO,
        }
    }

    fn line_metrics(&self, style: StyleId) -> LineMetrics {
        let computed = self.computed(style);
        match self.face_of(&computed) {
            Some(face) => LineMetrics {
                ascent: Width(scale(face.ascent, computed.font_size.0)),
                descent: Width(scale(face.descent, computed.font_size.0)),
                leading: Width(scale(face.leading, computed.font_size.0)),
            },
            None => LineMetrics::default(),
        }
    }

    fn space_width(&self, style: StyleId) -> Width {
        let computed = self.computed(style);
        match self.face_of(&computed) {
            Some(face) => Width(scale(face.space, computed.font_size.0)),
            None => self.advance(" ", style),
        }
    }

    fn version(&self) -> u16 {
        soma_metrics::VERSION
    }

    fn hash(&self) -> u64 {
        self.table.hash()
    }
}

/// `sans`/`mono`/`hei`/`gothic` select the second face; everything else the
/// first. Matching is ASCII case-insensitive and allocation-free.
fn face_index(computed: &ComputedStyle) -> usize {
    let family = computed.family.as_deref().unwrap_or("");
    let sans = contains_ignore_ascii_case(family, "sans")
        || contains_ignore_ascii_case(family, "mono")
        || contains_ignore_ascii_case(family, "hei")
        || contains_ignore_ascii_case(family, "gothic");
    let base = if sans { 1 } else { 0 };
    if computed.weight == FontWeight::Bold {
        base + 2
    } else {
        base
    }
}

fn contains_ignore_ascii_case(haystack: &str, needle: &str) -> bool {
    let haystack = haystack.as_bytes();
    let needle = needle.as_bytes();
    if needle.is_empty() {
        return true;
    }
    if needle.len() > haystack.len() {
        return false;
    }
    haystack
        .windows(needle.len())
        .any(|window| window.eq_ignore_ascii_case(needle))
}

/// Scale a table value (1/4096 em) by a font size (also 1/4096 em).
fn scale(value: i32, size: i32) -> i32 {
    let scaled = value as i64 * size as i64 / EM;
    if scaled > i32::MAX as i64 {
        i32::MAX
    } else if scaled < i32::MIN as i64 {
        i32::MIN
    } else {
        scaled as i32
    }
}
