//! Content capability matrix (Gate C).
//!
//! Every construct the front end knows about is declared here with a status.
//! `Degraded` means the content survives as text (nothing is dropped) but the
//! structure is not modelled. The table is checked against `docs/capabilities.md`
//! so documentation and code cannot drift.

/// How well a source construct is modelled.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CapabilityStatus {
    /// Modelled as DocIR structure.
    Supported,
    /// Preserved as text, structure lost.
    Degraded,
    /// Not accepted; the source is rejected or ignored as text.
    Rejected,
}

impl CapabilityStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Supported => "supported",
            Self::Degraded => "degraded",
            Self::Rejected => "rejected",
        }
    }
}

/// One declared construct.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Capability {
    /// Stable id used in docs and tests.
    pub syntax: &'static str,
    pub status: CapabilityStatus,
    pub note: &'static str,
}

pub const CAPABILITIES: &[Capability] = &[
    Capability {
        syntax: "atx_heading",
        status: CapabilityStatus::Supported,
        note: "`#`..`######` becomes `Heading`",
    },
    Capability {
        syntax: "paragraph",
        status: CapabilityStatus::Supported,
        note: "blank-line separated blocks",
    },
    Capability {
        syntax: "soft_line_join",
        status: CapabilityStatus::Supported,
        note: "consecutive lines join with a space",
    },
    Capability {
        syntax: "hard_break_spaces",
        status: CapabilityStatus::Supported,
        note: "two trailing spaces become `LineBreak::Hard`",
    },
    Capability {
        syntax: "hard_break_backslash",
        status: CapabilityStatus::Supported,
        note: "trailing backslash becomes `LineBreak::Hard`",
    },
    Capability {
        syntax: "emphasis",
        status: CapabilityStatus::Supported,
        note: "`*x*` and `_x_` become `Emphasis`",
    },
    Capability {
        syntax: "strong",
        status: CapabilityStatus::Supported,
        note: "`**x**` and `__x__` become `Strong`",
    },
    Capability {
        syntax: "inline_code",
        status: CapabilityStatus::Supported,
        note: "`` `x` `` becomes `Code`, one unbreakable atom",
    },
    Capability {
        syntax: "link",
        status: CapabilityStatus::Supported,
        note: "`[text](url)` becomes `Link`",
    },
    Capability {
        syntax: "image",
        status: CapabilityStatus::Supported,
        note: "`![alt](src)` becomes `Image`",
    },
    Capability {
        syntax: "fenced_code",
        status: CapabilityStatus::Supported,
        note: "``` ``` ``` / `~~~` become `CodeBlock`",
    },
    Capability {
        syntax: "list_unordered",
        status: CapabilityStatus::Supported,
        note: "`-`, `*`, `+`; flat lists only",
    },
    Capability {
        syntax: "list_ordered",
        status: CapabilityStatus::Supported,
        note: "`1.` / `1)` with the declared start number",
    },
    Capability {
        syntax: "blockquote",
        status: CapabilityStatus::Supported,
        note: "`>` lines become one `BlockQuote` paragraph",
    },
    Capability {
        syntax: "thematic_break",
        status: CapabilityStatus::Supported,
        note: "`---`, `***`, `___` become `ThematicBreak`",
    },
    Capability {
        syntax: "footnote_def",
        status: CapabilityStatus::Supported,
        note: "`[^label]: text` becomes `FootnoteDef`",
    },
    Capability {
        syntax: "footnote_ref",
        status: CapabilityStatus::Supported,
        note: "`[^label]` becomes `FootnoteRef`",
    },
    Capability {
        syntax: "escape",
        status: CapabilityStatus::Supported,
        note: "backslash escapes the next character",
    },
    Capability {
        syntax: "nested_list",
        status: CapabilityStatus::Degraded,
        note: "indented items join the parent item as text",
    },
    Capability {
        syntax: "setext_heading",
        status: CapabilityStatus::Degraded,
        note: "underlined heading becomes a paragraph plus a rule",
    },
    Capability {
        syntax: "table",
        status: CapabilityStatus::Degraded,
        note: "pipe rows stay text; build `Table` in DocIR to get a table",
    },
    Capability {
        syntax: "html_block",
        status: CapabilityStatus::Degraded,
        note: "kept as literal text, never executed",
    },
    Capability {
        syntax: "html_inline",
        status: CapabilityStatus::Degraded,
        note: "kept as literal text, never executed",
    },
    Capability {
        syntax: "link_reference",
        status: CapabilityStatus::Degraded,
        note: "`[label]: url` definitions stay text",
    },
    Capability {
        syntax: "multi_line_code_span",
        status: CapabilityStatus::Degraded,
        note: "an unterminated span stays literal text",
    },
];

/// Look up one construct.
pub fn capability(syntax: &str) -> Option<&'static Capability> {
    CAPABILITIES.iter().find(|entry| entry.syntax == syntax)
}

/// All ids with a given status.
pub fn with_status(status: CapabilityStatus) -> impl Iterator<Item = &'static Capability> {
    CAPABILITIES
        .iter()
        .filter(move |entry| entry.status == status)
}
