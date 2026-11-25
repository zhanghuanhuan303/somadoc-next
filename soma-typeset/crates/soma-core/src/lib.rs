//! `soma-core`: runtime-independent typesetting core.
//!
//! The crate is `no_std` in normal builds. It has no font, filesystem, DOM,
//! PDF, or platform dependency; hosts provide geometry through
//! [`metrics::Metrics`].
//!
//! Layout geometry is fixed-point: one em is [`units::UNITS_PER_EM`] units.
//! This is a deliberate part of the cross-runtime contract.

#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]

extern crate alloc;

pub mod breaker;
pub mod ir;
pub mod metrics;
pub mod params;
pub mod preset;
pub mod result;
pub mod units;
pub mod vertical;

pub use breaker::{
    break_paragraph, break_paragraph_greedy, break_paragraph_traced, BreakConfig, BreakError,
    GreedyMode, KpTrace, LineTrace, Window,
};
pub use ir::{
    Align, Atom, AtomKind, Boundary, BoundaryKind, BoundaryRole, Breakability, Glue, IrError,
    Paragraph, ParagraphStyle, PunctClass,
};
pub use metrics::{GlyphMetrics, LineMetrics, Metrics, StyleId, SyntheticMetrics};
pub use params::{
    BreakParams, CostParams, FallbackParams, FeatureFlags, ParamError, ParamHash, ParamSet,
    Permille, PresetId, TieBreak, TrimPriority, Weight,
};
pub use result::{AtomBox, BreakerKind, CjkLatinAdjust, FallbackStage, LayoutResult, Line};
pub use units::{Cost, Width, UNITS_PER_EM};
pub use vertical::VerticalParams;
