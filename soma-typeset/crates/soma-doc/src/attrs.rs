//! Semantic attributes shared by every node.
//!
//! Attributes are the only way content can influence presentation: a template
//! matches `role`, `classes`, and `anchor`. They never carry physical values
//! (see ADR-0009). `lang` is a language tag for the span, not a font choice.

use alloc::string::String;
use alloc::vec::Vec;

/// Maximum number of classes on one node. A small bound keeps matching cheap
/// and rejects runaway front-end output.
pub const MAX_CLASSES: usize = 16;

/// Maximum length of a role, class, anchor, or language tag, in bytes.
pub const MAX_ATTR_LEN: usize = 128;

/// Role/class/anchor/language of a node.
///
/// `anchor` is the JSON `anchor` key and the source-level id (`<h2 id="x">`,
/// `{#x}`). It is deliberately not called `id` to avoid confusion with
/// [`crate::NodeId`], which is a document-local identity, not an anchor.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Attrs {
    /// Named semantic role, e.g. `body`, `caption`, `footnote`, `aside`.
    pub role: Option<String>,
    /// Template matchers. Order is preserved and part of canonical output.
    pub classes: Vec<String>,
    /// Anchor target for cross references and links.
    pub anchor: Option<String>,
    /// Language tag for this span, e.g. `zh-Hans`, `en`.
    pub lang: Option<String>,
}

impl Attrs {
    pub const fn new() -> Self {
        Self {
            role: None,
            classes: Vec::new(),
            anchor: None,
            lang: None,
        }
    }

    pub fn with_role(mut self, role: impl Into<String>) -> Self {
        self.role = Some(role.into());
        self
    }

    pub fn with_class(mut self, class: impl Into<String>) -> Self {
        self.classes.push(class.into());
        self
    }

    pub fn with_anchor(mut self, anchor: impl Into<String>) -> Self {
        self.anchor = Some(anchor.into());
        self
    }

    pub fn with_lang(mut self, lang: impl Into<String>) -> Self {
        self.lang = Some(lang.into());
        self
    }

    pub fn is_empty(&self) -> bool {
        self.role.is_none()
            && self.classes.is_empty()
            && self.anchor.is_none()
            && self.lang.is_none()
    }

    /// True when `class` is present.
    pub fn has_class(&self, class: &str) -> bool {
        self.classes.iter().any(|candidate| candidate == class)
    }
}
