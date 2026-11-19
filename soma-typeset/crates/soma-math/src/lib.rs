//! TeX-subset math parsing and integer math-box layout.
//!
//! v1 supports a display formula made of symbols, groups, scripts and
//! fractions. It is deliberately small: no radicals, delimiters, matrices,
//! alignment environments, or macros. Unknown commands are errors so the
//! caller can fall back to the source text.
//!
//! ```text
//! x^2 + y_1 = \frac{a}{b}
//! ```
//!
//! The layout is integer-only: every length is in 1/4096 em (the kernel unit),
//! and the result contains positioned glyphs plus rule rectangles. A renderer
//! only paints; it never re-lays-out.

#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]

extern crate alloc;

use alloc::boxed::Box;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::fmt;

use soma_core::Width;

/// Advances and vertical metrics for math glyphs at a given size.
pub trait MathMetrics {
    /// Advance of a short symbol run at `size` (1/4096 em).
    fn advance(&self, text: &str, size: Width) -> Width;
    /// Ascent of the font at `size` (above the baseline).
    fn ascent(&self, size: Width) -> Width;
    /// Descent of the font at `size` (below the baseline).
    fn descent(&self, size: Width) -> Width;
}

/// Math layout constants. Scales are permille of the base size.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MathStyle {
    pub size: Width,
    /// Display style: big-operator limits stack above and below. Inline style
    /// keeps them to the right, as TeX's text style does.
    pub display: bool,
    pub script_scale: u16,
    pub scriptscript_scale: u16,
    pub sup_shift: u16,
    pub sub_shift: u16,
    pub axis_height: u16,
    pub rule_thickness: u16,
    pub frac_gap: u16,
    pub bin_gap: u16,
    pub rel_gap: u16,
    pub punct_gap: u16,
    /// Size of a big operator (`\sum`, `\int`, `\prod`) as permille of the
    /// surrounding size.
    pub big_scale: u16,
}

impl Default for MathStyle {
    fn default() -> Self {
        Self {
            size: Width::EM,
            display: true,
            script_scale: 700,
            scriptscript_scale: 500,
            sup_shift: 400,
            sub_shift: 200,
            axis_height: 250,
            rule_thickness: 40,
            frac_gap: 120,
            bin_gap: 220,
            rel_gap: 280,
            punct_gap: 170,
            big_scale: 1400,
        }
    }
}

/// One positioned glyph. `y` is relative to the math baseline; negative is up.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MathItem {
    pub text: String,
    pub x: Width,
    pub y: Width,
    pub size: Width,
}

/// One rule (a fraction bar) in the math box.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MathRule {
    pub x: Width,
    pub y: Width,
    pub width: Width,
    pub thickness: Width,
}

/// A laid-out formula.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MathLayout {
    pub width: Width,
    /// Extent above the baseline.
    pub height: Width,
    /// Extent below the baseline.
    pub depth: Width,
    pub items: Vec<MathItem>,
    pub rules: Vec<MathRule>,
}

impl MathLayout {
    /// Total height of the box (height + depth).
    pub fn total_height(&self) -> Width {
        Width(self.height.0 + self.depth.0)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MathError {
    Empty,
    MissingArgument,
    UnsupportedCommand(String),
    UnbalancedGroup,
}

impl fmt::Display for MathError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => f.write_str("empty formula"),
            Self::MissingArgument => f.write_str("missing command argument"),
            Self::UnsupportedCommand(name) => write!(f, "unsupported math command \\{name}"),
            Self::UnbalancedGroup => f.write_str("unbalanced braces in formula"),
        }
    }
}

impl core::error::Error for MathError {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Class {
    Ord,
    Bin,
    Rel,
    Open,
    Close,
    Punct,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Node {
    Symbol {
        text: String,
        class: Class,
    },
    Group(Vec<Node>),
    Fraction {
        numerator: Box<Node>,
        denominator: Box<Node>,
    },
    Script {
        base: Box<Node>,
        sup: Option<Box<Node>>,
        sub: Option<Box<Node>>,
    },
    Radical {
        /// `\sqrt[3]{x}` degree.
        index: Option<Box<Node>>,
        body: Box<Node>,
    },
    /// A large operator with optional limits.
    Big {
        text: &'static str,
        /// Limits stack above and below; integrals keep them to the right.
        stacked: bool,
        sup: Option<Box<Node>>,
        sub: Option<Box<Node>>,
    },
    Space(u16),
}

impl Node {
    fn class(&self) -> Class {
        match self {
            Self::Symbol { class, .. } => *class,
            Self::Group(_) => Class::Ord,
            Self::Fraction { .. } => Class::Ord,
            Self::Script { base, .. } => base.class(),
            Self::Radical { .. } => Class::Ord,
            Self::Big { .. } => Class::Ord,
            Self::Space(_) => Class::Ord,
        }
    }
}

/// Parse a formula into a list of nodes.
pub fn parse(source: &str) -> Result<Vec<Node>, MathError> {
    let mut parser = Parser {
        chars: source.chars().collect(),
        pos: 0,
    };
    let nodes = parser.parse_list(false)?;
    if nodes.is_empty() {
        return Err(MathError::Empty);
    }
    Ok(nodes)
}

struct Parser {
    chars: Vec<char>,
    pos: usize,
}

impl Parser {
    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn bump(&mut self) -> Option<char> {
        let character = self.peek()?;
        self.pos += 1;
        Some(character)
    }

    fn skip_spaces(&mut self) {
        while let Some(character) = self.peek() {
            if character.is_whitespace() {
                self.pos += 1;
            } else {
                break;
            }
        }
    }

    fn parse_list(&mut self, in_group: bool) -> Result<Vec<Node>, MathError> {
        let mut nodes = Vec::new();
        loop {
            self.skip_spaces();
            match self.peek() {
                None => {
                    if in_group {
                        return Err(MathError::UnbalancedGroup);
                    }
                    break;
                }
                Some('}') => {
                    if in_group {
                        self.pos += 1;
                        break;
                    }
                    return Err(MathError::UnbalancedGroup);
                }
                Some('^') | Some('_') => {
                    let marker = self.bump().expect("peeked");
                    let argument = self.parse_argument()?;
                    let base = nodes.pop().unwrap_or(Node::Group(Vec::new()));
                    // A big operator takes its limits directly: stacking keeps
                    // `\sum_{i}^{n}` above and below the operator.
                    if let Node::Big {
                        text,
                        stacked,
                        sup,
                        sub,
                    } = base
                    {
                        let (sup, sub) = if marker == '^' {
                            (Some(Box::new(argument)), sub)
                        } else {
                            (sup, Some(Box::new(argument)))
                        };
                        nodes.push(Node::Big {
                            text,
                            stacked,
                            sup,
                            sub,
                        });
                        continue;
                    }
                    let (base, mut sup, mut sub) = match base {
                        Node::Script { base, sup, sub } => (base, sup, sub),
                        other => (Box::new(other), None, None),
                    };
                    if marker == '^' {
                        sup = Some(Box::new(argument));
                    } else {
                        sub = Some(Box::new(argument));
                    }
                    nodes.push(Node::Script { base, sup, sub });
                }
                Some('\\') => {
                    self.pos += 1;
                    let command = self.read_command();
                    match command.as_str() {
                        "frac" => {
                            let numerator = self.parse_group()?;
                            let denominator = self.parse_group()?;
                            nodes.push(Node::Fraction {
                                numerator: Box::new(numerator),
                                denominator: Box::new(denominator),
                            });
                        }
                        "sqrt" => {
                            let index = self.parse_optional_degree()?;
                            let body = self.parse_group()?;
                            nodes.push(Node::Radical {
                                index,
                                body: Box::new(body),
                            });
                        }
                        "left" | "right" => {
                            if let Some(delimiter) = self.parse_delimiter()? {
                                nodes.push(delimiter);
                            }
                        }
                        "sum" | "prod" | "int" | "oint" | "lim" | "sup" | "inf" => {
                            let (text, stacked) = match command.as_str() {
                                "sum" => ("∑", true),
                                "prod" => ("∏", true),
                                "oint" => ("∮", false),
                                "int" => ("∫", false),
                                "lim" => ("lim", true),
                                "sup" => ("sup", true),
                                _ => ("inf", true),
                            };
                            nodes.push(Node::Big {
                                text,
                                stacked,
                                sup: None,
                                sub: None,
                            });
                        }
                        "log" | "ln" | "exp" | "sin" | "cos" | "tan" | "max" | "min" => {
                            nodes.push(Node::Symbol {
                                text: command,
                                class: Class::Ord,
                            });
                        }
                        "," => nodes.push(Node::Space(167)),
                        ";" => nodes.push(Node::Space(278)),
                        "!" => nodes.push(Node::Space(0)),
                        "quad" => nodes.push(Node::Space(1000)),
                        "qquad" => nodes.push(Node::Space(2000)),
                        "text" => {
                            let text = self.read_raw_group()?;
                            if !text.is_empty() {
                                nodes.push(Node::Symbol {
                                    text,
                                    class: Class::Ord,
                                });
                            }
                        }
                        _ => {
                            if let Some(text) = symbol(&command) {
                                nodes.push(Node::Symbol {
                                    text: text.to_string(),
                                    class: class_of(text),
                                });
                            } else {
                                return Err(MathError::UnsupportedCommand(command));
                            }
                        }
                    }
                }
                Some('{') => {
                    let group = self.parse_group()?;
                    nodes.push(group);
                }
                Some(character) => {
                    self.pos += 1;
                    let text = character.to_string();
                    nodes.push(Node::Symbol {
                        class: class_of(&text),
                        text,
                    });
                }
            }
        }
        Ok(nodes)
    }

    /// Optional `[n]` degree of a radical.
    fn parse_optional_degree(&mut self) -> Result<Option<Box<Node>>, MathError> {
        self.skip_spaces();
        if self.peek() != Some('[') {
            return Ok(None);
        }
        self.pos += 1;
        let mut nodes = Vec::new();
        loop {
            self.skip_spaces();
            match self.bump() {
                None => return Err(MathError::UnbalancedGroup),
                Some(']') => break,
                Some('{') => nodes.push(self.parse_group()?),
                Some(character) => {
                    let text = character.to_string();
                    nodes.push(Node::Symbol {
                        class: class_of(&text),
                        text,
                    });
                }
            }
        }
        Ok(Some(Box::new(Node::Group(nodes))))
    }

    /// The delimiter after `\left` / `\right`. A `.` means "no delimiter".
    fn parse_delimiter(&mut self) -> Result<Option<Node>, MathError> {
        self.skip_spaces();
        let character = self.bump().ok_or(MathError::MissingArgument)?;
        if character == '.' {
            return Ok(None);
        }
        if character == '\\' {
            let command = self.read_command();
            let text = match command.as_str() {
                "{" => "{",
                "}" => "}",
                "|" => "‖",
                "langle" => "⟨",
                "rangle" => "⟩",
                "lfloor" => "⌊",
                "rfloor" => "⌋",
                "lceil" => "⌈",
                "rceil" => "⌉",
                _ => return Err(MathError::UnsupportedCommand(command)),
            };
            return Ok(Some(Node::Symbol {
                class: class_of(text),
                text: text.to_string(),
            }));
        }
        let text = character.to_string();
        Ok(Some(Node::Symbol {
            class: class_of(&text),
            text,
        }))
    }

    fn parse_group(&mut self) -> Result<Node, MathError> {
        self.skip_spaces();
        if self.peek() != Some('{') {
            return Err(MathError::MissingArgument);
        }
        self.pos += 1;
        let nodes = self.parse_list(true)?;
        Ok(Node::Group(nodes))
    }

    fn parse_argument(&mut self) -> Result<Node, MathError> {
        self.skip_spaces();
        match self.peek() {
            Some('{') => self.parse_group(),
            Some(_) => {
                let character = self.bump().expect("peeked");
                Ok(Node::Symbol {
                    text: character.to_string(),
                    class: class_of(&character.to_string()),
                })
            }
            None => Err(MathError::MissingArgument),
        }
    }

    fn read_command(&mut self) -> String {
        let mut name = String::new();
        if let Some(character) = self.peek() {
            if !character.is_ascii_alphabetic() {
                self.pos += 1;
                return character.to_string();
            }
        }
        while let Some(character) = self.peek() {
            if character.is_ascii_alphabetic() {
                name.push(character);
                self.pos += 1;
            } else {
                break;
            }
        }
        name
    }

    fn read_raw_group(&mut self) -> Result<String, MathError> {
        self.skip_spaces();
        if self.peek() != Some('{') {
            return Err(MathError::MissingArgument);
        }
        self.pos += 1;
        let mut out = String::new();
        let mut depth = 1usize;
        while let Some(character) = self.bump() {
            match character {
                '{' => {
                    depth += 1;
                    out.push(character);
                }
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        return Ok(out);
                    }
                    out.push(character);
                }
                _ => out.push(character),
            }
        }
        Err(MathError::UnbalancedGroup)
    }
}

fn class_of(text: &str) -> Class {
    match text {
        "+" | "-" | "−" | "±" | "×" | "·" | "/" => Class::Bin,
        "=" | "<" | ">" | "≤" | "≥" | "≠" | "→" | "≈" => Class::Rel,
        "(" | "[" | "{" => Class::Open,
        ")" | "]" | "}" => Class::Close,
        "," | ";" => Class::Punct,
        _ => Class::Ord,
    }
}

fn symbol(command: &str) -> Option<&'static str> {
    Some(match command {
        "cdot" => "·",
        "times" => "×",
        "div" => "÷",
        "pm" => "±",
        "mp" => "∓",
        "le" | "leq" => "≤",
        "ge" | "geq" => "≥",
        "ne" | "neq" => "≠",
        "equiv" => "≡",
        "approx" => "≈",
        "sim" => "∼",
        "propto" => "∝",
        "to" | "rightarrow" => "→",
        "leftarrow" => "←",
        "leftrightarrow" => "↔",
        "Rightarrow" => "⇒",
        "Leftarrow" => "⇐",
        "Leftrightarrow" => "⇔",
        "in" => "∈",
        "notin" => "∉",
        "subset" => "⊂",
        "subseteq" => "⊆",
        "supset" => "⊃",
        "supseteq" => "⊇",
        "cup" => "∪",
        "cap" => "∩",
        "emptyset" => "∅",
        "forall" => "∀",
        "exists" => "∃",
        "partial" => "∂",
        "nabla" => "∇",
        "infty" => "∞",
        "prime" => "′",
        "cdots" => "⋯",
        "ldots" | "dots" => "…",
        "degree" => "°",
        "angle" => "∠",
        "perp" => "⊥",
        "parallel" => "∥",
        "alpha" => "α",
        "beta" => "β",
        "gamma" => "γ",
        "delta" => "δ",
        "epsilon" => "ε",
        "zeta" => "ζ",
        "eta" => "η",
        "theta" => "θ",
        "iota" => "ι",
        "kappa" => "κ",
        "lambda" => "λ",
        "mu" => "μ",
        "nu" => "ν",
        "xi" => "ξ",
        "pi" => "π",
        "rho" => "ρ",
        "sigma" => "σ",
        "tau" => "τ",
        "upsilon" => "υ",
        "phi" => "φ",
        "chi" => "χ",
        "psi" => "ψ",
        "omega" => "ω",
        "Gamma" => "Γ",
        "Delta" => "Δ",
        "Theta" => "Θ",
        "Lambda" => "Λ",
        "Xi" => "Ξ",
        "Pi" => "Π",
        "Sigma" => "Σ",
        "Phi" => "Φ",
        "Psi" => "Ψ",
        "Omega" => "Ω",
        "{" => "{",
        "}" => "}",
        "|" => "‖",
        _ => return None,
    })
}

/// Lay out a formula at the base size from `style`.
pub fn layout(
    source: &str,
    style: &MathStyle,
    metrics: &dyn MathMetrics,
) -> Result<MathLayout, MathError> {
    let nodes = parse(source)?;
    Ok(lay_out(&nodes, style.size, style, metrics))
}

/// Lay out a parsed node list at `size`.
pub fn lay_out(
    nodes: &[Node],
    size: Width,
    style: &MathStyle,
    metrics: &dyn MathMetrics,
) -> MathLayout {
    let mut context = Context {
        style,
        metrics,
        items: Vec::new(),
        rules: Vec::new(),
        cursor: 0,
    };
    let mut previous: Option<Class> = None;
    for node in nodes {
        let class = node.class();
        if let Some(left) = previous {
            context.cursor += gap(left, class, style, size);
        }
        context.node(node, size, 0);
        previous = Some(class);
    }
    let items = context.items;
    let rules = context.rules;
    let mut height = 0i32;
    let mut depth = 0i32;
    for item in &items {
        height = height.max(-item.y.0 + metrics.ascent(item.size).0);
        depth = depth.max(item.y.0 + metrics.descent(item.size).0);
    }
    for rule in &rules {
        height = height.max(-rule.y.0);
        depth = depth.max(rule.y.0 + rule.thickness.0);
    }
    MathLayout {
        width: Width(context.cursor),
        height: Width(height.max(1)),
        depth: Width(depth.max(0)),
        items,
        rules,
    }
}

struct Context<'a> {
    style: &'a MathStyle,
    metrics: &'a dyn MathMetrics,
    items: Vec<MathItem>,
    rules: Vec<MathRule>,
    cursor: i32,
}

impl Context<'_> {
    fn node(&mut self, node: &Node, size: Width, baseline: i32) {
        match node {
            Node::Symbol { text, .. } => {
                self.items.push(MathItem {
                    text: text.clone(),
                    x: Width(self.cursor),
                    y: Width(baseline),
                    size,
                });
                self.cursor += self.metrics.advance(text, size).0;
            }
            Node::Group(nodes) => {
                let mut previous: Option<Class> = None;
                for child in nodes {
                    let class = child.class();
                    if let Some(left) = previous {
                        self.cursor += gap(left, class, self.style, size);
                    }
                    self.node(child, size, baseline);
                    previous = Some(class);
                }
            }
            Node::Space(permille) => {
                self.cursor += scale(size, *permille);
            }
            Node::Fraction {
                numerator,
                denominator,
            } => {
                let scaled = script_size(size, self.style);
                let numerator_box = self.measure(numerator, scaled);
                let denominator_box = self.measure(denominator, scaled);
                let width = numerator_box.0.max(denominator_box.0);
                let axis = scale(size, self.style.axis_height);
                let thickness = scale(size, self.style.rule_thickness).max(1);
                let gap = scale(size, self.style.frac_gap);
                let rule_y = baseline - axis;
                let numerator_baseline = rule_y - gap - numerator_box.1;
                let denominator_baseline = rule_y + thickness + gap + denominator_box.2;
                let left = self.cursor;
                let num_x = left + (width - numerator_box.0) / 2;
                let den_x = left + (width - denominator_box.0) / 2;
                self.node_with_x(numerator, scaled, numerator_baseline, num_x);
                self.node_with_x(denominator, scaled, denominator_baseline, den_x);
                self.rules.push(MathRule {
                    x: Width(left),
                    y: Width(rule_y),
                    width: Width(width),
                    thickness: Width(thickness),
                });
                self.cursor = left + width;
            }
            Node::Radical { index, body } => {
                let thickness = scale(size, self.style.rule_thickness).max(1);
                let index_width = match index {
                    Some(index) => {
                        let script = script_size(size, self.style);
                        let measured = self.measure(index, script);
                        let index_baseline = -(measured.1) - thickness;
                        self.node_with_x(index, script, index_baseline, self.cursor);
                        measured.0
                    }
                    None => 0,
                };
                let body_box = self.measure(body, size);
                // The radical glyph must cover the body: pick the size whose
                // ascent covers `body_box` height plus the bar.
                let needed = (body_box.1 + thickness).max(1);
                let unit_ascent = self.metrics.ascent(size).0.max(1);
                let radical_size = Width(
                    (size.0 as i64 * needed as i64 / unit_ascent as i64)
                        .clamp(size.0 as i64 / 2, (size.0 as i64) * 4) as i32,
                );
                let glyph_advance = self.metrics.advance("√", radical_size).0.max(thickness);
                let body_x = self.cursor + index_width + glyph_advance;
                self.items.push(MathItem {
                    text: String::from("√"),
                    x: Width(self.cursor + index_width),
                    y: Width(thickness),
                    size: radical_size,
                });
                self.node_with_x(body, size, 0, body_x);
                self.rules.push(MathRule {
                    x: Width(body_x),
                    y: Width(-needed),
                    width: Width(body_box.0),
                    thickness: Width(thickness),
                });
                self.cursor = body_x + body_box.0;
            }
            Node::Big {
                text,
                stacked,
                sup,
                sub,
            } => {
                let big = Width(scale(size, self.style.big_scale).max(size.0));
                let advance = self.metrics.advance(text, big).0;
                let ascent = self.metrics.ascent(big).0;
                let descent = self.metrics.descent(big).0;
                let script = script_size(size, self.style);
                let gap = scale(size, self.style.frac_gap).max(1);

                let sup_box = sup.as_ref().map(|node| self.measure(node, script));
                let sub_box = sub.as_ref().map(|node| self.measure(node, script));

                if *stacked && self.style.display {
                    let width = advance
                        .max(sup_box.map(|b| b.0).unwrap_or(0))
                        .max(sub_box.map(|b| b.0).unwrap_or(0));
                    let left = self.cursor;
                    let operator_x = left + (width - advance) / 2;
                    self.items.push(MathItem {
                        text: String::from(*text),
                        x: Width(operator_x),
                        y: Width::ZERO,
                        size: big,
                    });
                    if let (Some(node), Some(box_)) = (sup.as_ref(), sup_box) {
                        let baseline = -(ascent + gap + box_.2);
                        self.node_with_x(node, script, baseline, left + (width - box_.0) / 2);
                    }
                    if let (Some(node), Some(box_)) = (sub.as_ref(), sub_box) {
                        let baseline = descent + gap + box_.1;
                        self.node_with_x(node, script, baseline, left + (width - box_.0) / 2);
                    }
                    self.cursor = left + width;
                } else {
                    self.items.push(MathItem {
                        text: String::from(*text),
                        x: Width(self.cursor),
                        y: Width::ZERO,
                        size: big,
                    });
                    self.cursor += advance;
                    let base_end = self.cursor;
                    let shift = scale(size, self.style.sup_shift);
                    if let (Some(node), Some(box_)) = (sup.as_ref(), sup_box) {
                        let baseline = -(shift.max(ascent) + box_.2);
                        self.node_with_x(node, script, baseline, base_end);
                    }
                    if let (Some(node), Some(box_)) = (sub.as_ref(), sub_box) {
                        let baseline = scale(size, self.style.sub_shift).max(descent) + box_.1;
                        self.node_with_x(node, script, baseline, base_end);
                    }
                    self.cursor = self.cursor.max(base_end);
                }
            }
            Node::Script { base, sup, sub } => {
                self.node(base, size, baseline);
                let base_end = self.cursor;
                let script = script_size(size, self.style);
                let shift = scale(size, self.style.sup_shift);
                let sub_shift = scale(size, self.style.sub_shift);
                if let Some(sup) = sup {
                    let measured = self.measure(sup, script);
                    let y = baseline - shift.max(measured.1 + scale(size, 80));
                    self.node_with_x(sup, script, y, base_end);
                }
                if let Some(sub) = sub {
                    self.node_with_x(sub, script, baseline + sub_shift, base_end);
                }
                self.cursor = self.cursor.max(base_end);
            }
        }
    }

    fn node_with_x(&mut self, node: &Node, size: Width, baseline: i32, x: i32) {
        let saved = self.cursor;
        self.cursor = x;
        self.node(node, size, baseline);
        let end = self.cursor;
        self.cursor = saved.max(end);
    }

    /// Lay out a node temporarily to measure width/height/depth.
    fn measure(&mut self, node: &Node, size: Width) -> (i32, i32, i32) {
        let saved_items = self.items.len();
        let saved_rules = self.rules.len();
        let saved_cursor = self.cursor;
        self.cursor = 0;
        self.node(node, size, 0);
        let width = self.cursor;
        let mut height = 0i32;
        let mut depth = 0i32;
        for item in &self.items[saved_items..] {
            height = height.max(-item.y.0 + self.metrics.ascent(item.size).0);
            depth = depth.max(item.y.0 + self.metrics.descent(item.size).0);
        }
        for rule in &self.rules[saved_rules..] {
            height = height.max(-rule.y.0);
            depth = depth.max(rule.y.0 + rule.thickness.0);
        }
        self.items.truncate(saved_items);
        self.rules.truncate(saved_rules);
        self.cursor = saved_cursor;
        (width, height, depth)
    }
}

fn gap(left: Class, right: Class, style: &MathStyle, size: Width) -> i32 {
    if left == Class::Open || right == Class::Close || right == Class::Punct {
        return 0;
    }
    if left == Class::Bin || right == Class::Bin {
        scale(size, style.bin_gap)
    } else if left == Class::Rel || right == Class::Rel {
        scale(size, style.rel_gap)
    } else {
        0
    }
}

fn script_size(size: Width, style: &MathStyle) -> Width {
    Width(scale(size, style.script_scale).max(1))
}

fn scale(size: Width, permille: u16) -> i32 {
    (size.0 as i64 * permille as i64 / 1000) as i32
}
