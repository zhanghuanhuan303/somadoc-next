//! Raw (front-end-specific) content that DocIR cannot model.
//!
//! Raw nodes never lose data: the original `value` is always preserved so a
//! later stage can re-parse or re-emit it. `fallback` is the plain-text
//! projection used when the raw payload cannot be handled.

use alloc::string::String;

/// What a downstream stage should do with a raw payload.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RawCapability {
    /// A later adapter may handle it (math adapter, HTML sanitizer, ...).
    #[default]
    Preserved,
    /// Render `fallback` instead; the raw payload stays for diagnostics.
    Degraded,
    /// Not rendered at all; the raw payload stays for diagnostics and re-emit.
    Dropped,
}

impl RawCapability {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Preserved => "preserved",
            Self::Degraded => "degraded",
            Self::Dropped => "dropped",
        }
    }
}

/// Front-end-specific payload carried by `RawBlock` / `RawInline`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RawContent {
    /// Format tag: `html`, `latex`, `typst`, ... Lowercase, non-empty.
    pub format: String,
    /// Original payload, verbatim.
    pub value: String,
    /// Plain-text stand-in. May be empty.
    pub fallback: String,
    pub capability: RawCapability,
}

impl RawContent {
    pub fn new(format: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            format: format.into(),
            value: value.into(),
            fallback: String::new(),
            capability: RawCapability::Preserved,
        }
    }

    pub fn with_fallback(mut self, fallback: impl Into<String>) -> Self {
        self.fallback = fallback.into();
        self
    }

    pub fn with_capability(mut self, capability: RawCapability) -> Self {
        self.capability = capability;
        self
    }
}
