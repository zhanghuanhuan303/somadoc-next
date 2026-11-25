//! TeX-subset math parsing and integer math-box layout.
//!
//! Supported: symbols, groups, scripts, fractions, radicals, big operators with
//! limits, upright function names, `\left…\right` with stretched fences, and
//! `matrix` / `pmatrix` / `bmatrix` / `vmatrix` / `cases` / `aligned`. Unknown
//! commands are errors so the caller can fall back to the source text.
//!
//! ```text
//! x^2 + y_1 = \frac{a}{b} \quad \left( \begin{aligned} a &= 1 \\ b &= 2 \end{aligned} \right)
//! ```
//!
//! The layout is integer-only: every length is in 1/4096 em (the kernel unit),
//! and the result contains positioned glyphs plus rule rectangles. A renderer
//! only paints; it never re-lays-out.

#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]

extern crate alloc;

use alloc::boxed::Box;
use alloc::format;
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
    /// Fraction numerator/denominator scale in text style.
    pub text_frac_scale: u16,
    /// Gap around a fraction bar in text style.
    pub text_frac_gap: u16,
    /// Big-operator scale in text style.
    pub text_big_scale: u16,
}

impl MathStyle {
    /// Numerator/denominator scale: full size in display, script size inline.
    fn fraction_scale(&self) -> u16 {
        if self.display {
            1000
        } else {
            self.text_frac_scale
        }
    }

    fn fraction_gap(&self) -> u16 {
        if self.display {
            self.frac_gap
        } else {
            self.text_frac_gap
        }
    }

    fn big_per_mille(&self) -> u16 {
        if self.display {
            self.big_scale
        } else {
            self.text_big_scale
        }
    }
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
            text_frac_scale: 700,
            text_frac_gap: 80,
            text_big_scale: 1100,
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
    UnbalancedDelimiter,
    UnbalancedEnvironment(String),
}

impl fmt::Display for MathError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => f.write_str("empty formula"),
            Self::MissingArgument => f.write_str("missing command argument"),
            Self::UnsupportedCommand(name) => write!(f, "unsupported math command \\{name}"),
            Self::UnbalancedGroup => f.write_str("unbalanced braces in formula"),
            Self::UnbalancedDelimiter => f.write_str("unbalanced \\left/\\right"),
            Self::UnbalancedEnvironment(name) => {
                write!(f, "unbalanced or unexpected \\end{{{name}}}")
            }
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
    /// `\left…\right`: delimiters stretched to the body.
    Delimited {
        /// Empty string for the null delimiter `.`.
        left: String,
        body: Box<Node>,
        right: String,
    },
    /// `\begin{matrix}` and friends.
    Matrix {
        kind: MatrixKind,
        /// `rows[row][column]`; every row is padded to the widest row.
        rows: Vec<Vec<Vec<Node>>>,
    },
    Space(u16),
}

/// How a matrix is fenced and aligned.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MatrixKind {
    /// No fence, centres every column.
    Plain,
    Paren,
    Bracket,
    /// `\begin{vmatrix}`: vertical bars.
    Vert,
    /// `\begin{cases}`: one left brace, left-aligned columns.
    Cases,
    /// `\begin{aligned}`: columns alternate right/left around `&`.
    Aligned,
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
            Self::Delimited { .. } => Class::Ord,
            Self::Matrix { .. } => Class::Ord,
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
                        "left" => {
                            let left = self.read_delimiter_text()?;
                            let (body, right) = self.read_until_right()?;
                            let body = parse_body(&body)?;
                            nodes.push(Node::Delimited {
                                left,
                                body: Box::new(Node::Group(body)),
                                right,
                            });
                        }
                        "right" => {
                            // A `\right` without `\left`: keep the delimiter so
                            // the source is not silently dropped.
                            let text = self.read_delimiter_text()?;
                            if !text.is_empty() {
                                nodes.push(Node::Symbol {
                                    class: class_of(&text),
                                    text,
                                });
                            }
                        }
                        "begin" => {
                            let name = self.read_raw_group()?;
                            let kind = match name.as_str() {
                                "matrix" => MatrixKind::Plain,
                                "pmatrix" => MatrixKind::Paren,
                                "bmatrix" => MatrixKind::Bracket,
                                "vmatrix" => MatrixKind::Vert,
                                "cases" => MatrixKind::Cases,
                                "aligned" | "align" | "split" => MatrixKind::Aligned,
                                other => {
                                    return Err(MathError::UnsupportedCommand(format!(
                                        "begin{{{other}}}"
                                    )))
                                }
                            };
                            let rows = self.read_environment(&name)?;
                            nodes.push(Node::Matrix { kind, rows });
                        }
                        "end" => {
                            let name = self.read_raw_group()?;
                            return Err(MathError::UnbalancedEnvironment(name));
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
    fn read_delimiter_text(&mut self) -> Result<String, MathError> {
        self.skip_spaces();
        let character = self.bump().ok_or(MathError::MissingArgument)?;
        if character == '.' {
            return Ok(String::new());
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
            return Ok(text.to_string());
        }
        Ok(character.to_string())
    }

    /// Scan to the matching `\right` and return `(body, right delimiter)`.
    fn read_until_right(&mut self) -> Result<(String, String), MathError> {
        let chars = &self.chars;
        let mut depth = 0usize;
        let mut index = self.pos;
        let mut found = None;
        while index < chars.len() {
            if chars[index] != '\\' {
                index += 1;
                continue;
            }
            let (command, next) = read_command_at(chars, index);
            match command.as_str() {
                "left" => depth += 1,
                "right" if depth == 0 => {
                    found = Some((index, next));
                    break;
                }
                "right" => depth -= 1,
                _ => {}
            }
            index = next;
        }
        let Some((right_start, after_right)) = found else {
            return Err(MathError::UnbalancedDelimiter);
        };
        let body: String = chars[self.pos..right_start].iter().collect();
        self.pos = after_right;
        let right = self.read_delimiter_text()?;
        Ok((body, right))
    }

    /// Read `\begin{name}…\end{name}` into rows of cells.
    fn read_environment(&mut self, name: &str) -> Result<Vec<Vec<Vec<Node>>>, MathError> {
        let chars = &self.chars;
        let mut depth = 0usize;
        let mut index = self.pos;
        let mut found = None;
        while index < chars.len() {
            if chars[index] != '\\' {
                index += 1;
                continue;
            }
            let (command, next) = read_command_at(chars, index);
            match command.as_str() {
                "begin" => depth += 1,
                "end" if depth == 0 => {
                    found = Some((index, next));
                    break;
                }
                "end" => depth -= 1,
                _ => {}
            }
            index = next;
        }
        let Some((end_start, after_end)) = found else {
            return Err(MathError::UnbalancedEnvironment(name.to_string()));
        };
        let body: String = chars[self.pos..end_start].iter().collect();

        // `\end{name}`: the environment must close with the same name.
        self.pos = after_end;
        self.skip_spaces();
        if self.peek() != Some('{') {
            return Err(MathError::MissingArgument);
        }
        let close = self.read_raw_group()?;
        if close != name {
            return Err(MathError::UnbalancedEnvironment(close));
        }

        split_matrix(&body)
            .into_iter()
            .map(|row| {
                row.into_iter()
                    .map(|cell| parse_body(&cell))
                    .collect::<Result<Vec<_>, _>>()
            })
            .collect()
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

/// Read one command at `index` (which points at a backslash), without a
/// `Parser`. Returns `(name, index after the name)`.
fn read_command_at(chars: &[char], index: usize) -> (String, usize) {
    let mut cursor = index + 1;
    if cursor >= chars.len() {
        return (String::new(), cursor);
    }
    if !chars[cursor].is_ascii_alphabetic() {
        return (chars[cursor].to_string(), cursor + 1);
    }
    let mut name = String::new();
    while cursor < chars.len() && chars[cursor].is_ascii_alphabetic() {
        name.push(chars[cursor]);
        cursor += 1;
    }
    (name, cursor)
}

/// Parse a fragment that may be empty (a matrix cell).
fn parse_body(source: &str) -> Result<Vec<Node>, MathError> {
    let mut parser = Parser {
        chars: source.chars().collect(),
        pos: 0,
    };
    parser.parse_list(false)
}

/// Split a matrix body into rows and cells on top-level `\\` and `&`.
///
/// Nested environments and braces are respected, so a nested `matrix` or a
/// `\text{a & b}` never splits its parent.
fn split_matrix(body: &str) -> Vec<Vec<String>> {
    let chars: Vec<char> = body.chars().collect();
    let mut rows: Vec<Vec<String>> = Vec::new();
    let mut row: Vec<String> = Vec::new();
    let mut cell_start = 0usize;
    let mut depth = 0usize;
    let mut brace = 0i32;
    let mut index = 0usize;
    while index < chars.len() {
        match chars[index] {
            '{' => {
                brace += 1;
                index += 1;
            }
            '}' => {
                brace -= 1;
                index += 1;
            }
            '&' if brace == 0 && depth == 0 => {
                row.push(chars[cell_start..index].iter().collect());
                index += 1;
                cell_start = index;
            }
            '\\' => {
                let (command, next) = read_command_at(&chars, index);
                match command.as_str() {
                    "begin" => {
                        depth += 1;
                        index = next;
                    }
                    "end" => {
                        depth = depth.saturating_sub(1);
                        index = next;
                    }
                    "\\" if depth == 0 && brace == 0 => {
                        row.push(chars[cell_start..index].iter().collect());
                        rows.push(core::mem::take(&mut row));
                        index = next;
                        cell_start = index;
                    }
                    _ => index = next,
                }
            }
            _ => index += 1,
        }
    }
    row.push(chars[cell_start..].iter().collect());
    // A trailing `\\` leaves one empty row: drop it.
    if row.len() == 1 && row[0].trim().is_empty() && !rows.is_empty() {
        return rows;
    }
    rows.push(row);
    rows
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
                let scaled = Width(scale(size, self.style.fraction_scale()).max(1));
                let numerator_box = self.measure(numerator, scaled);
                let denominator_box = self.measure(denominator, scaled);
                let width = numerator_box.0.max(denominator_box.0);
                let axis = scale(size, self.style.axis_height);
                let thickness = scale(size, self.style.rule_thickness).max(1);
                let gap = scale(size, self.style.fraction_gap());
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
                let big = Width(scale(size, self.style.big_per_mille()).max(size.0));
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
            Node::Delimited { left, body, right } => {
                let body_box = self.measure(body, size);
                let (delim_size, baseline_off) = self.fence_geometry(body_box, size);
                self.delimiter(left, delim_size, baseline_off);
                let body_x = self.cursor;
                self.node_with_x(body, size, 0, body_x);
                self.cursor = body_x + body_box.0;
                self.delimiter(right, delim_size, baseline_off);
            }
            Node::Matrix { kind, rows } => self.matrix(*kind, rows, size),
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
        self.measure_nodes(core::slice::from_ref(node), size)
    }

    /// Lay out a node list temporarily to measure width/height/depth.
    fn measure_nodes(&mut self, nodes: &[Node], size: Width) -> (i32, i32, i32) {
        if nodes.is_empty() {
            return (0, 0, 0);
        }
        let saved_items = self.items.len();
        let saved_rules = self.rules.len();
        let saved_cursor = self.cursor;
        self.cursor = 0;
        let mut previous: Option<Class> = None;
        for child in nodes {
            let class = child.class();
            if let Some(left) = previous {
                self.cursor += gap(left, class, self.style, size);
            }
            self.node(child, size, 0);
            previous = Some(class);
        }
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

    /// Place a node list at an absolute `x` without changing the final cursor.
    fn place_nodes(&mut self, nodes: &[Node], size: Width, baseline: i32, x: i32) {
        let saved = self.cursor;
        self.cursor = x;
        let mut previous: Option<Class> = None;
        for child in nodes {
            let class = child.class();
            if let Some(left) = previous {
                self.cursor += gap(left, class, self.style, size);
            }
            self.node(child, size, baseline);
            previous = Some(class);
        }
        let end = self.cursor;
        self.cursor = saved.max(end);
    }

    /// Delimiter size and baseline offset for a fence around `body`.
    fn fence_geometry(&self, body: (i32, i32, i32), size: Width) -> (Width, i32) {
        let (_, height, depth) = body;
        let needed = (height + depth).max(1);
        let unit = (self.metrics.ascent(size).0 + self.metrics.descent(size).0).max(1);
        let mut delim_size = if unit < needed {
            // Round up, then nudge until the measured extent really covers the
            // body: every metrics lookup truncates, so the ratio alone can be
            // one unit short.
            let estimate = size.0 as i64 * needed as i64;
            let target = (estimate + unit as i64 - 1) / unit as i64;
            Width(target.clamp(size.0 as i64, size.0 as i64 * 4) as i32)
        } else {
            size
        };
        let extent = |context: &Self, size: Width| {
            context.metrics.ascent(size).0 + context.metrics.descent(size).0
        };
        let mut guard = 0;
        while extent(self, delim_size) < needed && delim_size.0 < size.0 * 4 && guard < 64 {
            delim_size = Width(delim_size.0 + (delim_size.0 / 64).max(1));
            guard += 1;
        }
        let ascent = self.metrics.ascent(delim_size).0;
        let descent = self.metrics.descent(delim_size).0;
        // Centre the delimiter box on the body's vertical centre.
        let baseline_off = (depth - height) / 2 + (ascent - descent) / 2;
        (delim_size, baseline_off)
    }

    /// Push one delimiter glyph at the cursor; the empty string is the null
    /// delimiter and advances nothing.
    fn delimiter(&mut self, text: &str, size: Width, baseline: i32) {
        if text.is_empty() {
            return;
        }
        let advance = self.metrics.advance(text, size).0;
        self.items.push(MathItem {
            text: text.to_string(),
            x: Width(self.cursor),
            y: Width(baseline),
            size,
        });
        self.cursor += advance;
    }

    fn matrix(&mut self, kind: MatrixKind, rows: &[Vec<Vec<Node>>], size: Width) {
        let cols = rows.iter().map(|row| row.len()).max().unwrap_or(0);
        if cols == 0 || rows.is_empty() {
            return;
        }
        let row_gap = scale(size, self.style.frac_gap).max(1);
        let col_gap = if kind == MatrixKind::Aligned {
            0
        } else {
            scale(size, self.style.rel_gap)
        };

        let mut widths = alloc::vec![0i32; cols];
        let mut ascent = alloc::vec![0i32; rows.len()];
        let mut descent = alloc::vec![0i32; rows.len()];
        let mut cells: Vec<Vec<(i32, i32, i32)>> = Vec::with_capacity(rows.len());
        for (r, row) in rows.iter().enumerate() {
            let mut line = Vec::with_capacity(cols);
            for (c, width) in widths.iter_mut().enumerate() {
                let empty: &[Node] = &[];
                let nodes = row.get(c).map(|nodes| nodes.as_slice()).unwrap_or(empty);
                let measured = self.measure_nodes(nodes, size);
                *width = (*width).max(measured.0);
                ascent[r] = ascent[r].max(measured.1);
                descent[r] = descent[r].max(measured.2);
                line.push(measured);
            }
            cells.push(line);
        }

        let total: i32 = widths.iter().sum::<i32>() + col_gap * cols.saturating_sub(1) as i32;
        let mut baselines = alloc::vec![0i32; rows.len()];
        for r in 1..rows.len() {
            baselines[r] = baselines[r - 1] + descent[r - 1] + row_gap + ascent[r];
        }
        let height = (0..rows.len())
            .map(|r| ascent[r] - baselines[r])
            .max()
            .unwrap_or(0);
        let depth = (0..rows.len())
            .map(|r| descent[r] + baselines[r])
            .max()
            .unwrap_or(0);

        let (delim_size, baseline_off) = self.fence_geometry((total, height, depth), size);
        let (left, right) = fence_text(kind);
        self.delimiter(left, delim_size, baseline_off);
        let origin = self.cursor;

        let mut col_x = alloc::vec![0i32; cols];
        let mut acc = 0i32;
        for (c, width) in widths.iter().enumerate() {
            col_x[c] = acc;
            acc += width + col_gap;
        }

        for r in 0..rows.len() {
            for c in 0..cols {
                let width = cells[r][c].0;
                let cell_x = match kind {
                    MatrixKind::Aligned if c % 2 == 0 => col_x[c] + widths[c] - width,
                    MatrixKind::Aligned => col_x[c],
                    MatrixKind::Cases => col_x[c],
                    _ => col_x[c] + (widths[c] - width) / 2,
                };
                self.place_nodes(&rows[r][c], size, baselines[r], origin + cell_x);
            }
        }
        self.cursor = origin + total;
        self.delimiter(right, delim_size, baseline_off);
    }
}

/// Fences for a matrix kind.
fn fence_text(kind: MatrixKind) -> (&'static str, &'static str) {
    match kind {
        MatrixKind::Plain | MatrixKind::Aligned => ("", ""),
        MatrixKind::Paren => ("(", ")"),
        MatrixKind::Bracket => ("[", "]"),
        MatrixKind::Vert => ("|", "|"),
        MatrixKind::Cases => ("{", ""),
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
