//! TikZ core subset: integer paths, nodes and coordinates (ADR-0092).
//!
//! The input is a `tikzpicture` environment (or its body). The output is a
//! baseline-relative vector box: paths plus positioned math/text items. The
//! parser is deliberately strict: a key or command outside the documented
//! subset is a [`TikzError`], which the layout turns into the visible fallback
//! instead of a wrong diagram.

#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]

extern crate alloc;

mod geom;

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;
use core::fmt;

use geom::{
    from_width, parse_color, parse_degrees, parse_dimension, rotate, scale_permille, sin_cos_deg,
    to_width, TPoint, Unit, FINE,
};
use soma_core::Width;
use soma_math::{MathItem, MathLayout, MathRule};

/// One path point, baseline-relative (negative y is above the baseline).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Point {
    pub x: Width,
    pub y: Width,
}

/// One path segment.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Segment {
    Line(Point),
    Cubic { c1: Point, c2: Point, to: Point },
    Close,
}

/// A stroked and/or filled path.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Path {
    pub start: Point,
    pub segments: Vec<Segment>,
    pub fill: Option<u32>,
    pub stroke: Option<Stroke>,
}

/// Stroke state. An empty `dash` means a solid line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stroke {
    pub color: u32,
    pub width: Width,
    pub dash: Vec<Width>,
}

/// A laid-out picture in the engine's display-box frame.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TikzLayout {
    pub width: Width,
    pub height: Width,
    /// Extent below the baseline. The default TikZ baseline is the bounding
    /// box bottom, so this is zero for every picture we produce today.
    pub depth: Width,
    /// Distance from the box top to the baseline.
    pub baseline: Width,
    pub items: Vec<MathItem>,
    pub rules: Vec<MathRule>,
    pub paths: Vec<Path>,
}

/// A TikZ source error. The message is short and user-facing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TikzError {
    message: String,
}

impl TikzError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for TikzError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

/// Lay out a `tikzpicture`.
///
/// `pt` is the number of internal units in one point (the layout computes it
/// from the document body size). `measure` lays out one node text line at the
/// supplied permille scale and returns its math/text box.
pub fn layout(
    source: &str,
    pt: i64,
    measure: &mut dyn FnMut(&str, i64) -> Option<MathLayout>,
) -> Result<TikzLayout, TikzError> {
    let source = strip_comments(source);
    let body = picture_body(&source);
    let mut parser = Parser::new(body);
    let opts = parser.take_options(Some(pt))?;
    let mut engine = Engine::new(pt, measure, opts);
    parser.parse_all(&mut engine)?;
    Ok(engine.finish())
}

fn picture_body(source: &str) -> &str {
    let trimmed = source.trim();
    let Some(start) = trimmed.find("\\begin{tikzpicture}") else {
        return trimmed;
    };
    let after = &trimmed[start + "\\begin{tikzpicture}".len()..];
    match after.find("\\end{tikzpicture}") {
        Some(end) => &after[..end],
        None => after,
    }
}

fn strip_comments(source: &str) -> String {
    let mut out: Vec<u8> = Vec::with_capacity(source.len());
    let bytes = source.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && (index == 0 || bytes[index - 1] != b'\\') {
            while index < bytes.len() && bytes[index] != b'\n' {
                index += 1;
            }
            continue;
        }
        out.push(bytes[index]);
        index += 1;
    }
    String::from_utf8(out).unwrap_or_default()
}

struct Parser<'a> {
    input: &'a str,
    pos: usize,
}

impl<'a> Parser<'a> {
    fn new(input: &'a str) -> Self {
        Self { input, pos: 0 }
    }

    fn eof(&self) -> bool {
        self.pos >= self.input.len()
    }

    fn rest(&self) -> &'a str {
        &self.input[self.pos..]
    }

    fn skip_ws(&mut self) {
        while let Some(byte) = self.input.as_bytes().get(self.pos) {
            if byte.is_ascii_whitespace() {
                self.pos += 1;
            } else {
                break;
            }
        }
    }

    fn starts(&self, text: &str) -> bool {
        self.rest().starts_with(text)
    }

    fn eat(&mut self, text: &str) -> bool {
        if self.starts(text) {
            self.pos += text.len();
            true
        } else {
            false
        }
    }

    fn take_options(&mut self, pt: Option<i64>) -> Result<Opts, TikzError> {
        self.skip_ws();
        let mut opts = Opts::default();
        if self.rest().starts_with('[') {
            let text = self.take_bracket()?;
            opts.apply(text, pt)?;
        }
        Ok(opts)
    }

    fn take_bracket(&mut self) -> Result<&'a str, TikzError> {
        if !self.eat("[") {
            return Err(TikzError::new("expected `[`"));
        }
        let start = self.pos;
        let mut depth = 1usize;
        let bytes = self.input.as_bytes();
        while self.pos < bytes.len() {
            match bytes[self.pos] {
                b'[' => depth += 1,
                b']' => {
                    depth -= 1;
                    if depth == 0 {
                        let text = &self.input[start..self.pos];
                        self.pos += 1;
                        return Ok(text);
                    }
                }
                b'{' => {
                    self.skip_group()?;
                    continue;
                }
                _ => {}
            }
            self.pos += 1;
        }
        Err(TikzError::new("unterminated option list"))
    }

    fn take_group(&mut self) -> Result<&'a str, TikzError> {
        self.skip_ws();
        if !self.eat("{") {
            return Err(TikzError::new("expected `{`"));
        }
        let start = self.pos;
        let mut depth = 1usize;
        let bytes = self.input.as_bytes();
        while self.pos < bytes.len() {
            match bytes[self.pos] {
                b'{' => depth += 1,
                b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        let text = &self.input[start..self.pos];
                        self.pos += 1;
                        return Ok(text);
                    }
                }
                b'\\' => {
                    self.pos += 2;
                    continue;
                }
                _ => {}
            }
            self.pos += 1;
        }
        Err(TikzError::new("unterminated group"))
    }

    fn skip_group(&mut self) -> Result<(), TikzError> {
        let _ = self.take_group()?;
        Ok(())
    }

    fn parse_all(&mut self, engine: &mut Engine<'_>) -> Result<(), TikzError> {
        let statements = split_statements(self.rest());
        self.pos = self.input.len();
        let mut pending: Vec<String> = statements.into_iter().map(String::from).collect();
        pending.reverse();
        let mut budget = 0usize;
        while let Some(statement) = pending.pop() {
            budget += 1;
            if budget > 4096 {
                return Err(TikzError::new("too many expanded statements"));
            }
            let statement = statement.trim();
            if statement.is_empty() {
                continue;
            }
            if let Some((expanded, trailing)) = expand_foreach(statement)? {
                let mut work: Vec<String> = Vec::new();
                for item in expanded {
                    for part in split_statements(&item) {
                        let part = part.trim();
                        if !part.is_empty() {
                            work.push(part.to_string());
                        }
                    }
                }
                // `\foreach … {…} <tail>` is legal TikZ: the tail runs once,
                // after the loop, as the next statement.
                if let Some(trailing) = trailing {
                    let tail: Vec<String> = split_statements(&trailing)
                        .into_iter()
                        .map(str::trim)
                        .filter(|part| !part.is_empty())
                        .map(String::from)
                        .collect();
                    for item in tail.into_iter().rev() {
                        pending.push(item);
                    }
                }
                for item in work.into_iter().rev() {
                    pending.push(item);
                }
                continue;
            }
            engine.statement(statement)?;
        }
        Ok(())
    }
}

fn split_statements(source: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let bytes = source.as_bytes();
    let mut start = 0usize;
    let mut depth = 0usize;
    let mut index = 0usize;
    while index < bytes.len() {
        match bytes[index] {
            b'{' => depth += 1,
            b'}' => depth = depth.saturating_sub(1),
            b';' if depth == 0 => {
                out.push(&source[start..index]);
                start = index + 1;
            }
            _ => {}
        }
        index += 1;
    }
    if start < source.len() && !source[start..].trim().is_empty() {
        out.push(&source[start..]);
    }
    out
}

type ForeachExpansion = (Vec<String>, Option<String>);

fn expand_foreach(statement: &str) -> Result<Option<ForeachExpansion>, TikzError> {
    let Some(rest) = statement.strip_prefix("\\foreach") else {
        return Ok(None);
    };
    let rest = rest.trim_start();
    let Some(rest) = rest.strip_prefix('\\') else {
        return Err(TikzError::new("\\foreach needs a variable"));
    };
    let end = rest
        .find(|character: char| !character.is_ascii_alphabetic())
        .ok_or_else(|| TikzError::new("\\foreach needs `in`"))?;
    let variable = &rest[..end];
    let rest = rest[end..].trim_start();
    let rest = rest
        .strip_prefix("in")
        .ok_or_else(|| TikzError::new("\\foreach needs `in`"))?
        .trim_start();
    let (list, rest) = read_braced(rest)?;
    let (body, tail) = read_braced(rest.trim_start())?;
    let mut out = Vec::new();
    for item in list.split(',') {
        out.push(body.replace(&alloc::format!("\\{variable}"), item.trim()));
    }
    let tail = tail.trim();
    Ok(Some((out, (!tail.is_empty()).then(|| tail.to_string()))))
}

fn read_braced(source: &str) -> Result<(&str, &str), TikzError> {
    let source = source.trim_start();
    let rest = source
        .strip_prefix('{')
        .ok_or_else(|| TikzError::new("expected `{`"))?;
    let mut depth = 1usize;
    let mut index = 0usize;
    let bytes = rest.as_bytes();
    while index < bytes.len() {
        match bytes[index] {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Ok((&rest[..index], &rest[index + 1..]));
                }
            }
            b'\\' => {
                index += 1;
            }
            _ => {}
        }
        index += 1;
    }
    Err(TikzError::new("unterminated group"))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Anchor {
    Center,
    North,
    South,
    East,
    West,
    NorthEast,
    NorthWest,
    SouthEast,
    SouthWest,
}

#[derive(Clone, Debug)]
struct Opts {
    color: Option<u32>,
    draw: bool,
    fill: bool,
    draw_color: Option<u32>,
    fill_color: Option<u32>,
    text_color: Option<u32>,
    line_width: Option<i64>,
    dash: Option<Vec<i64>>,
    start_arrow: bool,
    end_arrow: bool,
    inner_sep: Option<i64>,
    outer_sep: Option<i64>,
    min_width: Option<i64>,
    min_height: Option<i64>,
    anchor: Anchor,
    circle: bool,
    rounded: Option<i64>,
    scale: i64,
    start: Option<TPoint>,
    step: Option<(i64, i64)>,
}

impl Default for Opts {
    fn default() -> Self {
        Self {
            color: None,
            draw: false,
            fill: false,
            draw_color: None,
            fill_color: None,
            text_color: None,
            line_width: None,
            dash: None,
            start_arrow: false,
            end_arrow: false,
            inner_sep: None,
            outer_sep: None,
            min_width: None,
            min_height: None,
            anchor: Anchor::Center,
            circle: false,
            rounded: None,
            scale: 1000,
            start: None,
            step: None,
        }
    }
}

impl Opts {
    fn apply(&mut self, text: &str, pt: Option<i64>) -> Result<(), TikzError> {
        for option in split_options(text) {
            let option = option.trim();
            if option.is_empty() {
                continue;
            }
            let (key, value) = match option.split_once('=') {
                Some((key, value)) => (key.trim(), Some(value.trim())),
                None => (option, None),
            };
            let length = |value: Option<&str>, default: Option<Unit>| -> Result<i64, TikzError> {
                let value = value.ok_or_else(|| TikzError::new("option needs a value"))?;
                let pt = pt.ok_or_else(|| TikzError::new("dimension without a point size"))?;
                parse_dimension(value, pt, default).ok_or_else(|| {
                    TikzError::new(alloc::format!("unsupported dimension `{value}`"))
                })
            };
            match key {
                "color" => {
                    self.color = Some(
                        parse_color(value.unwrap_or(""))
                            .ok_or_else(|| TikzError::new("unsupported color"))?,
                    )
                }
                "draw" => {
                    self.draw = true;
                    if let Some(value) = value {
                        self.draw_color = Some(
                            parse_color(value)
                                .ok_or_else(|| TikzError::new("unsupported draw color"))?,
                        );
                    }
                }
                "fill" => {
                    self.fill = true;
                    if let Some(value) = value {
                        self.fill_color = Some(
                            parse_color(value)
                                .ok_or_else(|| TikzError::new("unsupported fill color"))?,
                        );
                    }
                }
                "text" => {
                    self.text_color = Some(
                        parse_color(value.unwrap_or(""))
                            .ok_or_else(|| TikzError::new("unsupported text color"))?,
                    )
                }
                "line width" => self.line_width = Some(length(value, Some(Unit::Pt))?),
                "thin" => self.line_width = Some(4 * pt.unwrap_or(0) / 10),
                "very thin" => self.line_width = Some(2 * pt.unwrap_or(0) / 10),
                "semithick" => self.line_width = Some(6 * pt.unwrap_or(0) / 10),
                "thick" => self.line_width = Some(8 * pt.unwrap_or(0) / 10),
                "very thick" => self.line_width = Some(12 * pt.unwrap_or(0) / 10),
                "ultra thick" => self.line_width = Some(16 * pt.unwrap_or(0) / 10),
                "dashed" => {
                    let pt = pt.unwrap_or(0);
                    self.dash = Some(vec![3 * pt, 3 * pt]);
                }
                "dotted" => {
                    let pt = pt.unwrap_or(0);
                    let width = self.line_width.unwrap_or(4 * pt / 10);
                    self.dash = Some(vec![width, 2 * pt])
                }
                "densely dashed" => {
                    let pt = pt.unwrap_or(0);
                    self.dash = Some(vec![3 * pt, 2 * pt])
                }
                "loosely dashed" => {
                    let pt = pt.unwrap_or(0);
                    self.dash = Some(vec![3 * pt, 6 * pt])
                }
                "densely dotted" => {
                    let pt = pt.unwrap_or(0);
                    let width = self.line_width.unwrap_or(4 * pt / 10);
                    self.dash = Some(vec![width, pt])
                }
                "loosely dotted" => {
                    let pt = pt.unwrap_or(0);
                    let width = self.line_width.unwrap_or(4 * pt / 10);
                    self.dash = Some(vec![width, 4 * pt])
                }
                "dash pattern" => {
                    let value = value.ok_or_else(|| TikzError::new("dash pattern needs values"))?;
                    let mut dash = Vec::new();
                    for token in value.split_whitespace() {
                        if token == "on" || token == "off" {
                            continue;
                        }
                        let parsed = parse_dimension(token, pt.unwrap_or(0), Some(Unit::Pt))
                            .ok_or_else(|| TikzError::new("unsupported dash pattern"))?;
                        dash.push(parsed);
                    }
                    if dash.is_empty() {
                        return Err(TikzError::new("empty dash pattern"));
                    }
                    self.dash = Some(dash);
                }
                "->" => self.end_arrow = true,
                "<-" => self.start_arrow = true,
                "<->" => {
                    self.start_arrow = true;
                    self.end_arrow = true;
                }
                "inner sep" => self.inner_sep = Some(length(value, Some(Unit::Em))?),
                "outer sep" => self.outer_sep = Some(length(value, Some(Unit::Em))?),
                "minimum width" => self.min_width = Some(length(value, Some(Unit::Pt))?),
                "minimum height" => self.min_height = Some(length(value, Some(Unit::Pt))?),
                "minimum size" => {
                    let size = length(value, Some(Unit::Pt))?;
                    self.min_width = Some(size);
                    self.min_height = Some(size);
                }
                "anchor" => {
                    self.anchor = match value.unwrap_or("") {
                        "center" => Anchor::Center,
                        "north" => Anchor::North,
                        "south" => Anchor::South,
                        "east" => Anchor::East,
                        "west" => Anchor::West,
                        "north east" => Anchor::NorthEast,
                        "north west" => Anchor::NorthWest,
                        "south east" => Anchor::SouthEast,
                        "south west" => Anchor::SouthWest,
                        other => {
                            return Err(TikzError::new(alloc::format!(
                                "unsupported anchor `{other}`"
                            )))
                        }
                    }
                }
                "above" => self.anchor = Anchor::South,
                "below" => self.anchor = Anchor::North,
                "left" => self.anchor = Anchor::East,
                "right" => self.anchor = Anchor::West,
                "above left" => self.anchor = Anchor::SouthEast,
                "above right" => self.anchor = Anchor::SouthWest,
                "below left" => self.anchor = Anchor::NorthEast,
                "below right" => self.anchor = Anchor::NorthWest,
                "circle" => self.circle = true,
                "rectangle" => self.circle = false,
                "rounded corners" => {
                    self.rounded = Some(match value {
                        Some(value) => length(Some(value), Some(Unit::Pt))?,
                        None => 4 * pt.unwrap_or(0),
                    })
                }
                "scale" => {
                    let value = value.ok_or_else(|| TikzError::new("scale needs a value"))?;
                    let (num, den) = decimal_pair(value)?;
                    self.scale = num * 1000 / den;
                }
                "step" => {
                    let value = value.ok_or_else(|| TikzError::new("step needs a value"))?;
                    let (sx, sy) = parse_step(value, pt)?;
                    self.step = Some((sx, sy));
                }
                "xstep" => {
                    let value = value.ok_or_else(|| TikzError::new("xstep needs a value"))?;
                    let sx = length(Some(value), Some(Unit::Cm))?;
                    self.step = Some((sx, self.step.map(|step| step.1).unwrap_or(sx)));
                }
                "ystep" => {
                    let value = value.ok_or_else(|| TikzError::new("ystep needs a value"))?;
                    let sy = length(Some(value), Some(Unit::Cm))?;
                    self.step = Some((self.step.map(|step| step.0).unwrap_or(sy), sy));
                }
                "xshift" => {
                    let value = length(value, Some(Unit::Pt))?;
                    let start = self.start.unwrap_or(TPoint::ZERO);
                    self.start = Some(start.add(TPoint::new(value, 0)));
                }
                "yshift" => {
                    let value = length(value, Some(Unit::Pt))?;
                    let start = self.start.unwrap_or(TPoint::ZERO);
                    self.start = Some(start.add(TPoint::new(0, value)));
                }
                _ => {
                    // PGF accepts a bare color name as shorthand for `color=…`.
                    if value.is_none() {
                        if let Some(color) = parse_color(key) {
                            self.color = Some(color);
                            continue;
                        }
                    }
                    return Err(TikzError::new(alloc::format!("unsupported option `{key}`")));
                }
            }
        }
        Ok(())
    }
}

fn split_options(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let bytes = text.as_bytes();
    let mut start = 0usize;
    let mut depth = 0usize;
    let mut index = 0usize;
    while index < bytes.len() {
        match bytes[index] {
            b'{' | b'[' => depth += 1,
            b'}' | b']' => depth = depth.saturating_sub(1),
            b',' if depth == 0 => {
                out.push(&text[start..index]);
                start = index + 1;
            }
            _ => {}
        }
        index += 1;
    }
    out.push(&text[start..]);
    out
}

fn decimal_pair(text: &str) -> Result<(i64, i64), TikzError> {
    let (num, den) = if let Some((whole, fraction)) = text.split_once('.') {
        let den = 10i64.pow(fraction.len() as u32);
        let whole = whole
            .parse::<i64>()
            .map_err(|_| TikzError::new("bad number"))?;
        let fraction = if fraction.is_empty() {
            0
        } else {
            fraction
                .parse::<i64>()
                .map_err(|_| TikzError::new("bad number"))?
        };
        (whole * den + fraction, den)
    } else {
        (
            text.parse::<i64>()
                .map_err(|_| TikzError::new("bad number"))?,
            1,
        )
    };
    Ok((num, den))
}

/// Parse a grid step: either `0.5cm` (both axes) or `(0.5,1)`.
fn parse_step(text: &str, pt: Option<i64>) -> Result<(i64, i64), TikzError> {
    let text = text.trim();
    let pt = pt.ok_or_else(|| TikzError::new("grid step without a point size"))?;
    if let Some(inner) = text
        .strip_prefix('(')
        .and_then(|rest| rest.strip_suffix(')'))
    {
        let mut parts = inner.split(',');
        let sx = parts
            .next()
            .and_then(|value| parse_dimension(value, pt, Some(Unit::Cm)))
            .ok_or_else(|| TikzError::new("bad grid x step"))?;
        let sy = parts
            .next()
            .and_then(|value| parse_dimension(value, pt, Some(Unit::Cm)))
            .ok_or_else(|| TikzError::new("bad grid y step"))?;
        if parts.next().is_some() {
            return Err(TikzError::new("bad grid step pair"));
        }
        return Ok((sx, sy));
    }
    let step =
        parse_dimension(text, pt, Some(Unit::Cm)).ok_or_else(|| TikzError::new("bad grid step"))?;
    Ok((step, step))
}

struct Engine<'a> {
    pt: i64,
    measure: &'a mut dyn FnMut(&str, i64) -> Option<MathLayout>,
    defaults: Opts,
    xscale: i64,
    yscale: i64,
    xshift: i64,
    yshift: i64,
    rotate: i64,
    named: Vec<(String, Named)>,
    paths: Vec<RawPath>,
    texts: Vec<RawText>,
    min_x: i64,
    max_x: i64,
    min_y: i64,
    max_y: i64,
}

#[derive(Clone, Copy, Debug)]
struct Named {
    center: TPoint,
    half_w: i64,
    half_h: i64,
    outer: i64,
}

struct RawPath {
    start: TPoint,
    segments: Vec<RawSegment>,
    fill: Option<u32>,
    stroke: Option<RawStroke>,
}

enum RawSegment {
    Line(TPoint),
    Cubic(TPoint, TPoint, TPoint),
    Close,
}

struct RawStroke {
    color: u32,
    width: i64,
    dash: Vec<i64>,
}

struct RawText {
    x: i64,
    baseline: i64,
    layout: MathLayout,
    color: Option<u32>,
}

impl<'a> Engine<'a> {
    fn new(
        pt: i64,
        measure: &'a mut dyn FnMut(&str, i64) -> Option<MathLayout>,
        defaults: Opts,
    ) -> Self {
        let xscale = defaults.scale;
        let yscale = defaults.scale;
        let xshift = defaults.start.map(|point| point.x).unwrap_or(0);
        let yshift = defaults.start.map(|point| point.y).unwrap_or(0);
        Self {
            pt,
            measure,
            defaults,
            xscale,
            yscale,
            xshift,
            yshift,
            rotate: 0,
            named: Vec::new(),
            paths: Vec::new(),
            texts: Vec::new(),
            min_x: i64::MAX,
            max_x: i64::MIN,
            min_y: i64::MAX,
            max_y: i64::MIN,
        }
    }

    fn pt(&self) -> i64 {
        self.pt
    }

    fn transform(&self, point: TPoint) -> TPoint {
        let point = rotate(point, self.rotate);
        TPoint::new(
            scale_permille(point.x, self.xscale) + self.xshift,
            scale_permille(point.y, self.yscale) + self.yshift,
        )
    }

    fn statement(&mut self, statement: &str) -> Result<(), TikzError> {
        let (command, rest) = split_command(statement)?;
        match command {
            "draw" | "fill" | "filldraw" | "path" => {
                let mut parser = Parser::new(rest);
                let mut opts = self.defaults.clone();
                if parser.rest().starts_with('[') {
                    let text = parser.take_bracket()?;
                    opts.apply(text, Some(self.pt()))?;
                }
                match command {
                    "draw" => opts.draw = true,
                    "fill" => opts.fill = true,
                    "filldraw" => {
                        opts.draw = true;
                        opts.fill = true;
                    }
                    _ => {}
                }
                self.path(&mut parser, &opts)
            }
            "node" => {
                let mut parser = Parser::new(rest);
                self.node(&mut parser, None)
            }
            "coordinate" => {
                let mut parser = Parser::new(rest);
                self.coordinate(&mut parser)
            }
            other => Err(TikzError::new(alloc::format!(
                "unsupported TikZ command `\\{other}`"
            ))),
        }
    }

    fn path(&mut self, parser: &mut Parser<'_>, opts: &Opts) -> Result<(), TikzError> {
        let mut current: Option<TPoint> = None;
        let mut start = TPoint::ZERO;
        let mut builder: Option<PathBuilder> = None;
        let mut nodes: Vec<NodeRequest> = Vec::new();
        while !parser.eof() {
            parser.skip_ws();
            if parser.eof() {
                break;
            }
            if parser.eat("--") {
                if parser.eat("cycle") {
                    if let Some(builder) = builder.as_mut() {
                        builder.close();
                        current = Some(start);
                    } else {
                        return Err(TikzError::new("cycle without a path"));
                    }
                    continue;
                }
                // A node may sit between `--` and the next coordinate.
                while parser.eat("node") {
                    nodes.push(self.node_request(parser, current, opts)?);
                }
                let point = self.coordinate_expr(parser, &mut current)?;
                let point = self.transform(point);
                let builder = builder
                    .as_mut()
                    .ok_or_else(|| TikzError::new("line without a start"))?;
                builder.line(point);
                current = Some(point);
                continue;
            }
            if parser.eat("rectangle") {
                let point = self.coordinate_expr(parser, &mut current)?;
                let point = self.transform(point);
                let builder = builder
                    .as_mut()
                    .ok_or_else(|| TikzError::new("rectangle without a start"))?;
                builder.line(TPoint::new(point.x, builder.current.y));
                builder.line(point);
                builder.line(TPoint::new(builder.start.x, point.y));
                builder.close();
                current = Some(point);
                continue;
            }
            if parser.eat("circle") {
                parser.skip_ws();
                let mut radius = None;
                if parser.rest().starts_with('[') {
                    let text = parser.take_bracket()?;
                    let mut local = opts.clone();
                    local.apply(text, Some(self.pt()))?;
                    if let Some(value) = local.inner_sep {
                        // `inner sep` is not a radius; only accept radius below.
                        let _ = value;
                    }
                    for option in split_options(text) {
                        if let Some(value) = option.trim().strip_prefix("radius=") {
                            radius = Some(
                                parse_dimension(value, self.pt(), Some(Unit::Cm))
                                    .ok_or_else(|| TikzError::new("unsupported circle radius"))?,
                            );
                        }
                    }
                } else if parser.rest().starts_with('(') {
                    let text = parser.take_parens()?;
                    radius = Some(
                        parse_dimension(text.trim(), self.pt(), Some(Unit::Cm))
                            .ok_or_else(|| TikzError::new("unsupported circle radius"))?,
                    );
                }
                let radius = radius.ok_or_else(|| TikzError::new("circle needs a radius"))?;
                let center = current.ok_or_else(|| TikzError::new("circle without a center"))?;
                if let Some(previous) = builder.take() {
                    previous.finish(self, opts)?;
                }
                let first = TPoint::new(center.x + radius, center.y);
                let mut circle = PathBuilder::new(first);
                circle.ellipse(center, radius, radius);
                builder = Some(circle);
                current = Some(first);
                self.bounds_add(center.x - radius, center.y - radius);
                self.bounds_add(center.x + radius, center.y + radius);
                continue;
            }
            if parser.eat("ellipse") {
                let mut rx = None;
                let mut ry = None;
                parser.skip_ws();
                if parser.rest().starts_with('[') {
                    let text = parser.take_bracket()?;
                    for option in split_options(text) {
                        let option = option.trim();
                        if let Some(value) = option.strip_prefix("x radius=") {
                            rx = Some(
                                parse_dimension(value, self.pt(), Some(Unit::Cm))
                                    .ok_or_else(|| TikzError::new("unsupported ellipse radius"))?,
                            );
                        } else if let Some(value) = option.strip_prefix("y radius=") {
                            ry = Some(
                                parse_dimension(value, self.pt(), Some(Unit::Cm))
                                    .ok_or_else(|| TikzError::new("unsupported ellipse radius"))?,
                            );
                        }
                    }
                } else if parser.rest().starts_with('(') {
                    let text = parser.take_parens()?;
                    let (x, y) = text
                        .split_once("and")
                        .ok_or_else(|| TikzError::new("ellipse needs `rx and ry`"))?;
                    rx = Some(
                        parse_dimension(x, self.pt(), Some(Unit::Cm))
                            .ok_or_else(|| TikzError::new("unsupported ellipse radius"))?,
                    );
                    ry = Some(
                        parse_dimension(y, self.pt(), Some(Unit::Cm))
                            .ok_or_else(|| TikzError::new("unsupported ellipse radius"))?,
                    );
                }
                let center = current.ok_or_else(|| TikzError::new("ellipse without a center"))?;
                let rx = rx.ok_or_else(|| TikzError::new("ellipse needs an x radius"))?;
                let ry = ry.ok_or_else(|| TikzError::new("ellipse needs a y radius"))?;
                if let Some(previous) = builder.take() {
                    previous.finish(self, opts)?;
                }
                let first = TPoint::new(center.x + rx, center.y);
                let mut ellipse = PathBuilder::new(first);
                ellipse.ellipse(center, rx, ry);
                builder = Some(ellipse);
                current = Some(first);
                self.bounds_add(center.x - rx, center.y - ry);
                self.bounds_add(center.x + rx, center.y + ry);
                continue;
            }
            if parser.eat("arc") {
                let (start_angle, end_angle, radius) = self.arc_parameters(parser)?;
                let center = current.ok_or_else(|| TikzError::new("arc without a start"))?;
                let builder = builder
                    .as_mut()
                    .ok_or_else(|| TikzError::new("arc without a path"))?;
                builder.arc(center, start_angle, end_angle, radius);
                let (s, c) = sin_cos_deg(end_angle);
                let end = TPoint::new(
                    center.x + radius * c / 1_000_000_000,
                    center.y + radius * s / 1_000_000_000,
                );
                self.bounds_add(center.x - radius, center.y - radius);
                self.bounds_add(center.x + radius, center.y + radius);
                current = Some(end);
                continue;
            }
            if parser.eat("grid") {
                let corner = self.coordinate_expr(parser, &mut current)?;
                let corner = self.transform(corner);
                let from = current.ok_or_else(|| TikzError::new("grid without a start"))?;
                if let Some(previous) = builder.take() {
                    previous.finish(self, opts)?;
                }
                self.grid(from, corner, opts)?;
                self.bounds_add(from.x, from.y);
                self.bounds_add(corner.x, corner.y);
                current = Some(corner);
                // The grid is its own set of subpaths; a following `--`
                // continues from the corner like PGF.
                builder = Some(PathBuilder::new(corner));
                continue;
            }
            if parser.eat("-|") {
                let point = self.coordinate_expr(parser, &mut current)?;
                let point = self.transform(point);
                let current_point = current.ok_or_else(|| TikzError::new("-| without a start"))?;
                let builder = builder
                    .as_mut()
                    .ok_or_else(|| TikzError::new("-| without a path"))?;
                builder.line(TPoint::new(point.x, current_point.y));
                builder.line(point);
                current = Some(point);
                continue;
            }
            if parser.eat("|-") {
                let point = self.coordinate_expr(parser, &mut current)?;
                let point = self.transform(point);
                let current_point = current.ok_or_else(|| TikzError::new("|- without a start"))?;
                let builder = builder
                    .as_mut()
                    .ok_or_else(|| TikzError::new("|- without a path"))?;
                builder.line(TPoint::new(current_point.x, point.y));
                builder.line(point);
                current = Some(point);
                continue;
            }
            if parser.eat("node") {
                nodes.push(self.node_request(parser, current, opts)?);
                continue;
            }
            if parser.eat("coordinate") {
                let name = parser.take_node_name()?;
                let point = current.ok_or_else(|| TikzError::new("coordinate without a point"))?;
                if let Some(name) = name {
                    self.named.push((
                        name,
                        Named {
                            center: point,
                            half_w: 0,
                            half_h: 0,
                            outer: 0,
                        },
                    ));
                }
                continue;
            }
            // A bare coordinate starts a new subpath.
            if parser.rest().starts_with('(') || parser.rest().starts_with('+') {
                let point = self.coordinate_expr(parser, &mut current)?;
                let point = self.transform(point);
                if let Some(previous) = builder.take() {
                    previous.finish(self, opts)?;
                }
                start = point;
                current = Some(point);
                builder = Some(PathBuilder::new(point));
                continue;
            }
            return Err(TikzError::new("unsupported path operation"));
        }

        if let Some(builder) = builder {
            builder.finish(self, opts)?;
        }
        for request in nodes {
            self.place_node(request)?;
        }
        Ok(())
    }

    fn arc_parameters(&mut self, parser: &mut Parser<'_>) -> Result<(i64, i64, i64), TikzError> {
        parser.skip_ws();
        if parser.rest().starts_with('[') {
            let text = parser.take_bracket()?;
            let mut start = None;
            let mut end = None;
            let mut radius = None;
            for option in split_options(text) {
                let option = option.trim();
                if let Some(value) = option.strip_prefix("start angle=") {
                    start = parse_degrees(value);
                } else if let Some(value) = option.strip_prefix("end angle=") {
                    end = parse_degrees(value);
                } else if let Some(value) = option.strip_prefix("radius=") {
                    radius = parse_dimension(value, self.pt(), Some(Unit::Cm));
                }
            }
            return Ok((
                start.ok_or_else(|| TikzError::new("arc needs a start angle"))?,
                end.ok_or_else(|| TikzError::new("arc needs an end angle"))?,
                radius.ok_or_else(|| TikzError::new("arc needs a radius"))?,
            ));
        }
        let text = parser.take_parens()?;
        let mut parts = text.split(':');
        let start = parts
            .next()
            .and_then(parse_degrees)
            .ok_or_else(|| TikzError::new("arc needs `start:end:radius`"))?;
        let end = parts
            .next()
            .and_then(parse_degrees)
            .ok_or_else(|| TikzError::new("arc needs `start:end:radius`"))?;
        let radius = parts
            .next()
            .and_then(|value| parse_dimension(value, self.pt(), Some(Unit::Cm)))
            .ok_or_else(|| TikzError::new("arc needs `start:end:radius`"))?;
        if parts.next().is_some() {
            return Err(TikzError::new("too many arc arguments"));
        }
        Ok((start, end, radius))
    }

    /// PGF `grid`: lattice lines between `from` and `to`, one line per step
    /// (default 1cm), including both border lines. `draw` strokes every lattice
    /// line; `fill` has no area, exactly as in PGF, where a grid is a set of
    /// line subpaths.
    fn grid(&mut self, from: TPoint, to: TPoint, opts: &Opts) -> Result<(), TikzError> {
        let default_step = parse_dimension("1", self.pt(), Some(Unit::Cm))
            .ok_or_else(|| TikzError::new("grid step without a point size"))?;
        let (sx, sy) = opts.step.unwrap_or((default_step, default_step));
        let sx = sx.saturating_mul(self.xscale) / 1000;
        let sy = sy.saturating_mul(self.yscale) / 1000;
        if sx <= 0 || sy <= 0 {
            return Err(TikzError::new("grid step must be positive"));
        }
        if opts.draw {
            let stroke = RawStroke {
                color: opts.draw_color.or(opts.color).unwrap_or(0),
                width: opts.line_width.unwrap_or(4 * self.pt() / 10),
                dash: opts.dash.clone().unwrap_or_default(),
            };
            let mut count = 0usize;
            // Vertical lines, with the far edge always drawn.
            let mut x = from.x.min(to.x);
            let x_end = from.x.max(to.x);
            loop {
                count += 1;
                if count > 10_000 {
                    return Err(TikzError::new("grid has too many lines"));
                }
                self.push_grid_line(TPoint::new(x, from.y), TPoint::new(x, to.y), &stroke);
                if x >= x_end {
                    break;
                }
                x = x.saturating_add(sx).min(x_end);
            }
            let mut y = from.y.min(to.y);
            let y_end = from.y.max(to.y);
            loop {
                count += 1;
                if count > 10_000 {
                    return Err(TikzError::new("grid has too many lines"));
                }
                self.push_grid_line(TPoint::new(from.x, y), TPoint::new(to.x, y), &stroke);
                if y >= y_end {
                    break;
                }
                y = y.saturating_add(sy).min(y_end);
            }
        }
        Ok(())
    }

    fn push_grid_line(&mut self, from: TPoint, to: TPoint, stroke: &RawStroke) {
        self.paths.push(RawPath {
            start: from,
            segments: vec![RawSegment::Line(to)],
            fill: None,
            stroke: Some(RawStroke {
                color: stroke.color,
                width: stroke.width,
                dash: stroke.dash.clone(),
            }),
        });
    }

    fn coordinate(&mut self, parser: &mut Parser<'_>) -> Result<(), TikzError> {
        parser.skip_ws();
        let name = parser.take_node_name()?;
        parser.skip_ws();
        if !parser.eat("at") {
            return Err(TikzError::new("coordinate needs `at`"));
        }
        let mut current = None;
        let point = self.coordinate_expr(parser, &mut current)?;
        let point = self.transform(point);
        if let Some(name) = name {
            self.named.push((
                name,
                Named {
                    center: point,
                    half_w: 0,
                    half_h: 0,
                    outer: 0,
                },
            ));
        }
        Ok(())
    }

    fn node(&mut self, parser: &mut Parser<'_>, current: Option<TPoint>) -> Result<(), TikzError> {
        let request = self.node_request(parser, current, &self.defaults.clone())?;
        self.place_node(request)
    }

    fn node_request(
        &mut self,
        parser: &mut Parser<'_>,
        current: Option<TPoint>,
        base: &Opts,
    ) -> Result<NodeRequest, TikzError> {
        let mut opts = base.clone();
        let mut name = None;
        let mut at = None;
        loop {
            parser.skip_ws();
            if parser.rest().starts_with('[') {
                let text = parser.take_bracket()?;
                opts.apply(text, Some(self.pt()))?;
            } else if parser.rest().starts_with('(') {
                name = parser.take_node_name()?;
            } else if parser.eat("at") {
                let mut current = None;
                let point = self.coordinate_expr(parser, &mut current)?;
                at = Some(self.transform(point));
            } else if parser.eat("node") {
                continue;
            } else {
                break;
            }
        }
        let text = if parser.rest().trim_start().starts_with('{') {
            parser.skip_ws();
            parser.take_group()?.to_string()
        } else {
            String::new()
        };
        parser.skip_ws();
        if !parser.rest().trim().is_empty() {
            return Err(TikzError::new("trailing tokens after node"));
        }
        let point = at.or(current).unwrap_or(TPoint::ZERO);
        Ok(NodeRequest {
            opts,
            name,
            at: point,
            text,
        })
    }

    fn place_node(&mut self, request: NodeRequest) -> Result<(), TikzError> {
        let scale = request.opts.scale;
        let mut layout = if request.text.trim().is_empty() {
            MathLayout::default()
        } else {
            if request.text.contains("\\\\") {
                return Err(TikzError::new("multi-line nodes are not supported"));
            }
            (self.measure)(&request.text, scale)
                .ok_or_else(|| TikzError::new("node text is outside the text/math subset"))?
        };
        let text_width = layout.width;
        let text_height = layout.height;
        let text_depth = layout.depth;
        let inner = request
            .opts
            .inner_sep
            .unwrap_or(scale_permille(FINE * 3333 / 10000, scale));
        let line_width = request
            .opts
            .line_width
            .unwrap_or(scale_permille(4 * self.pt() / 10, scale));
        let outer = request.opts.outer_sep.unwrap_or(line_width / 2);
        let min_width = request.opts.min_width.unwrap_or(0);
        let min_height = request.opts.min_height.unwrap_or(0);
        let width = (from_width(text_width) + 2 * inner).max(min_width);
        let height = (from_width(text_height) + from_width(text_depth) + 2 * inner).max(min_height);
        let half_w = width / 2;
        let half_h = height / 2;
        let anchor = anchor_offset(request.opts.anchor, half_w, half_h, outer);
        let center = request.at.sub(anchor);

        if request.opts.circle {
            let radius = half_w.max(half_h);
            self.bounds_add(
                center.x - radius - line_width / 2,
                center.y - radius - line_width / 2,
            );
            self.bounds_add(
                center.x + radius + line_width / 2,
                center.y + radius + line_width / 2,
            );
            if request.opts.fill || request.opts.draw {
                let mut builder = PathBuilder::new(TPoint::new(center.x + radius, center.y));
                builder.ellipse(center, radius, radius);
                self.paths.push(RawPath {
                    start: builder.start,
                    segments: builder.segments,
                    fill: request
                        .opts
                        .fill
                        .then(|| request.opts.fill_color.or(request.opts.color).unwrap_or(0)),
                    stroke: request.opts.draw.then(|| RawStroke {
                        color: request.opts.draw_color.or(request.opts.color).unwrap_or(0),
                        width: line_width,
                        dash: Vec::new(),
                    }),
                });
            }
        } else {
            self.bounds_add(center.x - half_w - outer, center.y - half_h - outer);
            self.bounds_add(center.x + half_w + outer, center.y + half_h + outer);
            if request.opts.fill || request.opts.draw {
                let mut builder =
                    PathBuilder::new(TPoint::new(center.x - half_w, center.y + half_h));
                let radius = request.opts.rounded.unwrap_or(0).min(half_w).min(half_h);
                if radius > 0 {
                    builder.rounded_rect(center, half_w, half_h, radius);
                } else {
                    builder.line(TPoint::new(center.x + half_w, center.y + half_h));
                    builder.line(TPoint::new(center.x + half_w, center.y - half_h));
                    builder.line(TPoint::new(center.x - half_w, center.y - half_h));
                    builder.close();
                }
                let path = RawPath {
                    start: builder.start,
                    segments: builder.segments,
                    fill: request
                        .opts
                        .fill
                        .then(|| request.opts.fill_color.or(request.opts.color).unwrap_or(0)),
                    stroke: request.opts.draw.then(|| RawStroke {
                        color: request.opts.draw_color.or(request.opts.color).unwrap_or(0),
                        width: line_width,
                        dash: Vec::new(),
                    }),
                };
                self.paths.push(path);
            }
        }

        if let Some(color) = request.opts.text_color.or(request.opts.color) {
            for item in &mut layout.items {
                item.color = Some(color);
            }
        }
        let left = center.x - from_width(text_width) / 2;
        let baseline = center.y - (from_width(text_height) - from_width(text_depth)) / 2;
        if !layout.items.is_empty() || !layout.rules.is_empty() {
            self.texts.push(RawText {
                x: left,
                baseline,
                layout,
                color: request.opts.text_color.or(request.opts.color),
            });
        }
        if let Some(name) = request.name {
            self.named.push((
                name,
                Named {
                    center,
                    half_w,
                    half_h,
                    outer,
                },
            ));
        }
        Ok(())
    }

    fn coordinate_expr(
        &mut self,
        parser: &mut Parser<'_>,
        current: &mut Option<TPoint>,
    ) -> Result<TPoint, TikzError> {
        parser.skip_ws();
        let mut update = false;
        if parser.eat("++") {
            update = true;
        } else if parser.eat("+") {
            update = false;
        }
        parser.skip_ws();
        let text = parser.take_parens()?;
        let point = self.parse_coordinate_text(text)?;
        let point = if text.trim_start().starts_with('+') || update {
            current.unwrap_or(TPoint::ZERO).add(point)
        } else {
            point
        };
        if update {
            *current = Some(point);
        }
        Ok(point)
    }

    fn parse_coordinate_text(&mut self, text: &str) -> Result<TPoint, TikzError> {
        let text = text.trim();
        if text.is_empty() {
            return Err(TikzError::new("empty coordinate"));
        }
        if text.starts_with('+') {
            let inner = text.trim_start_matches(['+']).trim();
            let inner = inner
                .strip_prefix('(')
                .and_then(|value| value.strip_suffix(')'))
                .unwrap_or(inner);
            return self.parse_coordinate_text(inner);
        }
        if let Some((name, anchor)) = text.split_once('.') {
            if is_identifier(name.trim()) {
                return self.anchor_point(name.trim(), anchor.trim());
            }
        }
        if is_identifier(text) {
            return self.anchor_point(text, "center");
        }
        if let Some((angle, radius)) = text.split_once(':') {
            let angle = parse_degrees(angle).ok_or_else(|| TikzError::new("bad polar angle"))?;
            let radius = parse_dimension(radius, self.pt(), Some(Unit::Cm))
                .ok_or_else(|| TikzError::new("bad polar radius"))?;
            let (sin, cos) = sin_cos_deg(angle);
            return Ok(TPoint::new(
                radius * cos / 1_000_000_000,
                radius * sin / 1_000_000_000,
            ));
        }
        let (x, y) = text
            .split_once(',')
            .ok_or_else(|| TikzError::new("coordinate needs x,y"))?;
        Ok(TPoint::new(
            parse_dimension(x, self.pt(), Some(Unit::Cm))
                .ok_or_else(|| TikzError::new("bad x coordinate"))?,
            parse_dimension(y, self.pt(), Some(Unit::Cm))
                .ok_or_else(|| TikzError::new("bad y coordinate"))?,
        ))
    }

    fn anchor_point(&self, name: &str, anchor: &str) -> Result<TPoint, TikzError> {
        let named = self
            .named
            .iter()
            .rev()
            .find(|(seen, _)| seen == name)
            .map(|(_, named)| *named)
            .ok_or_else(|| TikzError::new(alloc::format!("unknown coordinate `{name}`")))?;
        let anchor = match anchor {
            "center" => Anchor::Center,
            "north" => Anchor::North,
            "south" => Anchor::South,
            "east" => Anchor::East,
            "west" => Anchor::West,
            "north east" => Anchor::NorthEast,
            "north west" => Anchor::NorthWest,
            "south east" => Anchor::SouthEast,
            "south west" => Anchor::SouthWest,
            other => {
                return Err(TikzError::new(alloc::format!(
                    "unsupported anchor `{other}`"
                )))
            }
        };
        Ok(named.center.add(anchor_offset(
            anchor,
            named.half_w,
            named.half_h,
            named.outer,
        )))
    }

    fn bounds_add(&mut self, x: i64, y: i64) {
        self.min_x = self.min_x.min(x);
        self.max_x = self.max_x.max(x);
        self.min_y = self.min_y.min(y);
        self.max_y = self.max_y.max(y);
    }

    fn finish(self) -> TikzLayout {
        let min_x = if self.min_x == i64::MAX {
            0
        } else {
            self.min_x
        };
        let max_x = if self.max_x == i64::MIN {
            0
        } else {
            self.max_x
        };
        let min_y = if self.min_y == i64::MAX {
            0
        } else {
            self.min_y
        };
        let max_y = if self.max_y == i64::MIN {
            0
        } else {
            self.max_y
        };
        let width = to_width(max_x - min_x);
        let height = to_width(max_y - min_y);
        let mut paths = Vec::with_capacity(self.paths.len());
        for path in self.paths {
            let start = Point {
                x: to_width(path.start.x - min_x),
                y: to_width(min_y - path.start.y),
            };
            let segments = path
                .segments
                .into_iter()
                .map(|segment| match segment {
                    RawSegment::Line(point) => Segment::Line(Point {
                        x: to_width(point.x - min_x),
                        y: to_width(min_y - point.y),
                    }),
                    RawSegment::Cubic(c1, c2, to) => Segment::Cubic {
                        c1: Point {
                            x: to_width(c1.x - min_x),
                            y: to_width(min_y - c1.y),
                        },
                        c2: Point {
                            x: to_width(c2.x - min_x),
                            y: to_width(min_y - c2.y),
                        },
                        to: Point {
                            x: to_width(to.x - min_x),
                            y: to_width(min_y - to.y),
                        },
                    },
                    RawSegment::Close => Segment::Close,
                })
                .collect();
            let stroke = path.stroke.map(|stroke| Stroke {
                color: stroke.color,
                width: to_width(stroke.width),
                dash: stroke.dash.into_iter().map(to_width).collect(),
            });
            paths.push(Path {
                start,
                segments,
                fill: path.fill,
                stroke,
            });
        }
        let mut items = Vec::new();
        let mut rules = Vec::new();
        for text in self.texts {
            let dx = to_width(text.x - min_x).0;
            let dy = to_width(min_y - text.baseline).0;
            for mut item in text.layout.items {
                item.x = Width(item.x.0 + dx);
                item.y = Width(item.y.0 + dy);
                if item.color.is_none() {
                    item.color = text.color;
                }
                items.push(item);
            }
            for mut rule in text.layout.rules {
                rule.x = Width(rule.x.0 + dx);
                rule.y = Width(rule.y.0 + dy);
                rules.push(rule);
            }
        }
        TikzLayout {
            width,
            height,
            depth: Width::ZERO,
            baseline: height,
            items,
            rules,
            paths,
        }
    }
}

struct NodeRequest {
    opts: Opts,
    name: Option<String>,
    at: TPoint,
    text: String,
}

struct PathBuilder {
    start: TPoint,
    current: TPoint,
    segments: Vec<RawSegment>,
}

impl PathBuilder {
    fn new(start: TPoint) -> Self {
        Self {
            start,
            current: start,
            segments: Vec::new(),
        }
    }

    fn line(&mut self, point: TPoint) {
        self.segments.push(RawSegment::Line(point));
        self.current = point;
    }

    fn close(&mut self) {
        self.segments.push(RawSegment::Close);
        self.current = self.start;
    }

    fn ellipse(&mut self, center: TPoint, rx: i64, ry: i64) {
        // Four cubic quarters; kappa = 4/3 * tan(pi/8) in 1e9 units.
        let k = 552_284_750i128;
        let kx = (rx as i128 * k / 1_000_000_000) as i64;
        let ky = (ry as i128 * k / 1_000_000_000) as i64;
        let right = TPoint::new(center.x + rx, center.y);
        let top = TPoint::new(center.x, center.y + ry);
        let left = TPoint::new(center.x - rx, center.y);
        let bottom = TPoint::new(center.x, center.y - ry);
        self.segments.push(RawSegment::Cubic(
            TPoint::new(center.x + rx, center.y + ky),
            TPoint::new(center.x + kx, center.y + ry),
            top,
        ));
        self.segments.push(RawSegment::Cubic(
            TPoint::new(center.x - kx, center.y + ry),
            TPoint::new(center.x - rx, center.y + ky),
            left,
        ));
        self.segments.push(RawSegment::Cubic(
            TPoint::new(center.x - rx, center.y - ky),
            TPoint::new(center.x - kx, center.y - ry),
            bottom,
        ));
        self.segments.push(RawSegment::Cubic(
            TPoint::new(center.x + kx, center.y - ry),
            TPoint::new(center.x + rx, center.y - ky),
            right,
        ));
        self.current = right;
    }

    fn rounded_rect(&mut self, center: TPoint, half_w: i64, half_h: i64, radius: i64) {
        let left = center.x - half_w;
        let right = center.x + half_w;
        let top = center.y + half_h;
        let bottom = center.y - half_h;
        self.line(TPoint::new(left + radius, top));
        self.line(TPoint::new(right - radius, top));
        self.corner(
            TPoint::new(right - radius, top),
            TPoint::new(right, top - radius),
        );
        self.line(TPoint::new(right, top - radius));
        self.line(TPoint::new(right, bottom + radius));
        self.corner(
            TPoint::new(right, bottom + radius),
            TPoint::new(right - radius, bottom),
        );
        self.line(TPoint::new(right - radius, bottom));
        self.line(TPoint::new(left + radius, bottom));
        self.corner(
            TPoint::new(left + radius, bottom),
            TPoint::new(left, bottom + radius),
        );
        self.line(TPoint::new(left, bottom + radius));
        self.line(TPoint::new(left, top - radius));
        self.corner(
            TPoint::new(left, top - radius),
            TPoint::new(left + radius, top),
        );
        self.close();
    }

    fn corner(&mut self, from: TPoint, to: TPoint) {
        // A quadratic corner as one cubic with both controls at the corner.
        let corner = TPoint::new(to.x, from.y);
        self.segments.push(RawSegment::Cubic(corner, corner, to));
        self.current = to;
    }

    fn arc(&mut self, start: TPoint, from_angle: i64, to_angle: i64, radius: i64) {
        let (sin, cos) = sin_cos_deg(from_angle);
        let center = TPoint::new(
            start.x - radius * cos / 1_000_000_000,
            start.y - radius * sin / 1_000_000_000,
        );
        let mut from = from_angle;
        let mut to = to_angle;
        if to <= from {
            to += 360_000_000;
        }
        let mut current = start;
        while from < to {
            let step = (to - from).min(90_000_000);
            let next = from + step;
            let (s0, c0) = sin_cos_deg(from);
            let (s1, c1) = sin_cos_deg(next);
            let p0 = TPoint::new(
                center.x + radius * c0 / 1_000_000_000,
                center.y + radius * s0 / 1_000_000_000,
            );
            let p1 = TPoint::new(
                center.x + radius * c1 / 1_000_000_000,
                center.y + radius * s1 / 1_000_000_000,
            );
            // k = 4/3 * tan(step/4), computed from sin/cos.
            let (sin4, cos4) = sin_cos_deg(step / 4);
            let k = (4i128 * sin4 as i128 * 1_000_000_000) / (3 * cos4 as i128);
            let c1p = TPoint::new(
                p0.x + (radius as i128 * -s0 as i128 * k / 1_000_000_000_000_000_000) as i64,
                p0.y + (radius as i128 * c0 as i128 * k / 1_000_000_000_000_000_000) as i64,
            );
            let c2p = TPoint::new(
                p1.x - (radius as i128 * -s1 as i128 * k / 1_000_000_000_000_000_000) as i64,
                p1.y - (radius as i128 * c1 as i128 * k / 1_000_000_000_000_000_000) as i64,
            );
            self.segments.push(RawSegment::Cubic(c1p, c2p, p1));
            current = p1;
            from = next;
        }
        self.current = current;
    }
}

impl PathBuilder {
    fn finish(self, engine: &mut Engine<'_>, opts: &Opts) -> Result<(), TikzError> {
        if self.segments.is_empty() {
            return Ok(());
        }
        let path = RawPath {
            start: self.start,
            segments: self.segments,
            fill: opts
                .fill
                .then(|| opts.fill_color.or(opts.color).unwrap_or(0)),
            stroke: opts.draw.then(|| RawStroke {
                color: opts.draw_color.or(opts.color).unwrap_or(0),
                width: opts.line_width.unwrap_or(4 * engine.pt() / 10),
                dash: opts.dash.clone().unwrap_or_default(),
            }),
        };
        if path.fill.is_none() && path.stroke.is_none() {
            return Ok(());
        }
        for segment in &path.segments {
            match segment {
                RawSegment::Line(point) => engine.bounds_add(point.x, point.y),
                RawSegment::Cubic(c1, c2, to) => {
                    engine.bounds_add(c1.x, c1.y);
                    engine.bounds_add(c2.x, c2.y);
                    engine.bounds_add(to.x, to.y);
                }
                RawSegment::Close => {}
            }
        }
        engine.bounds_add(path.start.x, path.start.y);
        // Arrow barbs are separate stroked paths so a renderer needs no arrow
        // vocabulary; they are a geometric approximation of PGF's `to` tip.
        if path.stroke.is_some() {
            if opts.start_arrow {
                if let Some((dir, point)) = path_start_direction(&path) {
                    engine.paths.push(arrow_path(point, dir, opts, engine.pt()));
                }
            }
            if opts.end_arrow {
                if let Some((dir, point)) = path_end_direction(&path) {
                    engine.paths.push(arrow_path(point, dir, opts, engine.pt()));
                }
            }
        }
        engine.paths.push(path);
        Ok(())
    }
}

fn arrow_path(end: TPoint, dir: TPoint, opts: &Opts, pt: i64) -> RawPath {
    let width = opts.line_width.unwrap_or(4 * pt / 10);
    let length = 4 * width + 2 * pt;
    let half = 3 * width + pt;
    let (dx, dy) = normalize(dir);
    let base = TPoint::new(
        end.x - length * dx / 1_000_000,
        end.y - length * dy / 1_000_000,
    );
    let perp = TPoint::new(-dy, dx);
    let p1 = TPoint::new(
        base.x + half * perp.x / 1_000_000,
        base.y + half * perp.y / 1_000_000,
    );
    let p2 = TPoint::new(
        base.x - half * perp.x / 1_000_000,
        base.y - half * perp.y / 1_000_000,
    );
    RawPath {
        start: end,
        segments: vec![
            RawSegment::Line(p1),
            RawSegment::Line(end),
            RawSegment::Line(p2),
        ],
        fill: None,
        stroke: Some(RawStroke {
            color: opts.draw_color.or(opts.color).unwrap_or(0),
            width,
            dash: Vec::new(),
        }),
    }
}

fn normalize(point: TPoint) -> (i64, i64) {
    let length =
        isqrt((point.x as i128 * point.x as i128 + point.y as i128 * point.y as i128) as u128)
            as i64;
    if length <= 0 {
        return (1_000_000, 0);
    }
    (
        (point.x as i128 * 1_000_000 / length as i128) as i64,
        (point.y as i128 * 1_000_000 / length as i128) as i64,
    )
}

fn isqrt(value: u128) -> u128 {
    if value < 2 {
        return value;
    }
    let mut x = value;
    let mut y = x.div_ceil(2);
    while y < x {
        x = y;
        y = (x + value / x) / 2;
    }
    x
}

fn path_start_direction(path: &RawPath) -> Option<(TPoint, TPoint)> {
    let first = path.segments.first()?;
    let to = match first {
        RawSegment::Line(point) => *point,
        RawSegment::Cubic(c1, _, _) => *c1,
        RawSegment::Close => return None,
    };
    Some((to.sub(path.start), path.start))
}

fn path_end_direction(path: &RawPath) -> Option<(TPoint, TPoint)> {
    for segment in path.segments.iter().rev() {
        match segment {
            RawSegment::Line(point) => return Some((point.sub(path.start), *point)),
            RawSegment::Cubic(_, c2, to) => return Some((to.sub(*c2), *to)),
            RawSegment::Close => continue,
        }
    }
    None
}

fn anchor_offset(anchor: Anchor, half_w: i64, half_h: i64, outer: i64) -> TPoint {
    let x = half_w + outer;
    let y = half_h + outer;
    match anchor {
        Anchor::Center => TPoint::ZERO,
        Anchor::North => TPoint::new(0, y),
        Anchor::South => TPoint::new(0, -y),
        Anchor::East => TPoint::new(x, 0),
        Anchor::West => TPoint::new(-x, 0),
        Anchor::NorthEast => TPoint::new(x, y),
        Anchor::NorthWest => TPoint::new(-x, y),
        Anchor::SouthEast => TPoint::new(x, -y),
        Anchor::SouthWest => TPoint::new(-x, -y),
    }
}

fn is_identifier(text: &str) -> bool {
    let mut chars = text.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    (first.is_alphabetic() || first == '_')
        && chars.all(|character| character.is_alphanumeric() || matches!(character, '_' | '-'))
}

fn split_command(statement: &str) -> Result<(&str, &str), TikzError> {
    let Some(rest) = statement.strip_prefix('\\') else {
        return Err(TikzError::new("statement does not start with a command"));
    };
    let end = rest
        .find(|character: char| !character.is_ascii_alphabetic())
        .unwrap_or(rest.len());
    if end == 0 {
        return Err(TikzError::new("empty command"));
    }
    Ok((&rest[..end], rest[end..].trim_start()))
}

impl<'a> Parser<'a> {
    fn take_parens(&mut self) -> Result<&'a str, TikzError> {
        self.skip_ws();
        if !self.eat("(") {
            return Err(TikzError::new("expected `(`"));
        }
        let start = self.pos;
        let mut depth = 1usize;
        let bytes = self.input.as_bytes();
        while self.pos < bytes.len() {
            match bytes[self.pos] {
                b'(' => depth += 1,
                b')' => {
                    depth -= 1;
                    if depth == 0 {
                        let text = &self.input[start..self.pos];
                        self.pos += 1;
                        return Ok(text);
                    }
                }
                b'{' => {
                    self.skip_group()?;
                    continue;
                }
                _ => {}
            }
            self.pos += 1;
        }
        Err(TikzError::new("unterminated coordinate"))
    }

    fn take_node_name(&mut self) -> Result<Option<String>, TikzError> {
        self.skip_ws();
        if !self.rest().starts_with('(') {
            return Ok(None);
        }
        let text = self.take_parens()?;
        Ok(Some(text.trim().to_string()))
    }
}
