//! Integer geometry shared by the TikZ parser.
//!
//! Internal coordinates are `1/65536 em`, so a 0.1 cm coordinate, a 0.4 pt
//! line and a rotation keep enough low bits; the public box is converted to
//! the engine's `1/4096 em` with truncation at the boundary.

use soma_core::Width;

/// Internal units per em.
pub(crate) const FINE: i64 = 65_536;

/// A point in TikZ coordinates: x right, y up.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct TPoint {
    pub x: i64,
    pub y: i64,
}

impl TPoint {
    pub const ZERO: Self = Self { x: 0, y: 0 };

    pub const fn new(x: i64, y: i64) -> Self {
        Self { x, y }
    }

    pub fn add(self, other: Self) -> Self {
        Self::new(
            self.x.saturating_add(other.x),
            self.y.saturating_add(other.y),
        )
    }

    pub fn sub(self, other: Self) -> Self {
        Self::new(
            self.x.saturating_sub(other.x),
            self.y.saturating_sub(other.y),
        )
    }
}

/// Convert internal units to the engine's `Width`.
pub(crate) fn to_width(value: i64) -> Width {
    let scaled = (value as i128) * 4096;
    let result = if scaled >= 0 {
        scaled / FINE as i128
    } else {
        -((-scaled) / FINE as i128)
    };
    Width(result.clamp(i32::MIN as i128, i32::MAX as i128) as i32)
}

/// Convert a `Width` back to internal units.
pub(crate) fn from_width(value: Width) -> i64 {
    (value.0 as i128 * FINE as i128 / 4096) as i64
}

/// A parsed unit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Unit {
    Cm,
    Mm,
    In,
    Pt,
    Em,
}

impl Unit {
    fn parse(text: &str) -> Option<Self> {
        match text {
            "cm" => Some(Self::Cm),
            "mm" => Some(Self::Mm),
            "in" => Some(Self::In),
            "pt" => Some(Self::Pt),
            "em" => Some(Self::Em),
            _ => None,
        }
    }
}

/// Parse a PGF dimension into internal units.
///
/// `pt` is the number of internal units in one engine point (1/72 in).
/// Bare numbers use `default_unit`; a dimension without a unit and without a
/// default is an error, as in TikZ.
pub(crate) fn parse_dimension(text: &str, pt: i64, default_unit: Option<Unit>) -> Option<i64> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    let mut index = 0usize;
    let bytes = text.as_bytes();
    let mut negative = false;
    if matches!(bytes.first(), Some(b'+') | Some(b'-')) {
        negative = bytes[0] == b'-';
        index += 1;
    }
    let start = index;
    let mut digits = 0usize;
    let mut dot = false;
    while index < bytes.len() {
        match bytes[index] {
            b'0'..=b'9' => {
                digits += 1;
                index += 1;
            }
            b'.' if !dot => {
                dot = true;
                index += 1;
            }
            _ => break,
        }
    }
    if digits == 0 {
        return None;
    }
    let number = &text[start..index];
    let unit_text = text[index..].trim();
    let unit = if unit_text.is_empty() {
        default_unit?
    } else {
        Unit::parse(unit_text)?
    };
    let (num, den) = decimal_parts(number)?;
    let factor = match unit {
        Unit::Cm => pt.saturating_mul(3600).saturating_div(127),
        Unit::Mm => pt.saturating_mul(360).saturating_div(127),
        Unit::In => pt.saturating_mul(72),
        Unit::Pt => pt,
        Unit::Em => FINE,
    };
    let value = (num as i128)
        .saturating_mul(factor as i128)
        .saturating_div(den as i128) as i64;
    Some(if negative { -value } else { value })
}

/// Parse a bare decimal (degree angle) into micro-degrees.
pub(crate) fn parse_degrees(text: &str) -> Option<i64> {
    let (num, den) = decimal_parts(text.trim())?;
    Some((num as i128 * 1_000_000 / den as i128) as i64)
}

fn decimal_parts(text: &str) -> Option<(i64, i64)> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    let negative = text.starts_with('-');
    let text = text.trim_start_matches(['+', '-']);
    let mut num: i128 = 0;
    let mut den: i128 = 1;
    let mut seen_dot = false;
    let mut digits = 0;
    for byte in text.bytes() {
        match byte {
            b'0'..=b'9' => {
                num = num.checked_mul(10)?.checked_add((byte - b'0') as i128)?;
                if seen_dot {
                    den = den.checked_mul(10)?;
                }
                digits += 1;
            }
            b'.' if !seen_dot => seen_dot = true,
            _ => return None,
        }
    }
    if digits == 0 {
        return None;
    }
    let num = num as i64;
    let den = den as i64;
    Some((if negative { -num } else { num }, den))
}

const TRIG_SCALE: i128 = 1_000_000_000;
const PI: i128 = 3_141_592_654;

/// Sin and cos of an angle in micro-degrees, scaled by 1e9.
///
/// Integer Taylor series on one quadrant; no float, no platform libm.
pub(crate) fn sin_cos_deg(deg_micro: i64) -> (i64, i64) {
    let deg = deg_micro.rem_euclid(360_000_000);
    let quadrant = deg / 90_000_000;
    let rem = deg % 90_000_000;
    let x = rem as i128 * PI / 180_000_000;
    let sin = sin_small(x);
    let cos = cos_small(x);
    let (sin, cos) = match quadrant {
        0 => (sin, cos),
        1 => (cos, -sin),
        2 => (-sin, -cos),
        _ => (-cos, sin),
    };
    (sin as i64, cos as i64)
}

fn sin_small(x: i128) -> i128 {
    let x2 = x * x / TRIG_SCALE;
    let mut term = x;
    let mut sum = x;
    let mut n: i128 = 1;
    while n <= 9 {
        term = -term * x2 / TRIG_SCALE / ((2 * n) * (2 * n + 1));
        sum += term;
        n += 1;
    }
    sum
}

fn cos_small(x: i128) -> i128 {
    let x2 = x * x / TRIG_SCALE;
    let mut term = TRIG_SCALE;
    let mut sum = term;
    let mut n: i128 = 1;
    while n <= 9 {
        term = -term * x2 / TRIG_SCALE / ((2 * n - 1) * (2 * n));
        sum += term;
        n += 1;
    }
    sum
}

/// Rotate a point by an angle in micro-degrees. `cos`/`sin` are 1e9 scaled.
pub(crate) fn rotate(point: TPoint, deg_micro: i64) -> TPoint {
    let (sin, cos) = sin_cos_deg(deg_micro);
    TPoint::new(
        ((point.x as i128 * cos as i128 - point.y as i128 * sin as i128) / TRIG_SCALE) as i64,
        ((point.x as i128 * sin as i128 + point.y as i128 * cos as i128) / TRIG_SCALE) as i64,
    )
}

/// Multiply by a fixed permille scale.
pub(crate) fn scale_permille(value: i64, permille: i64) -> i64 {
    (value as i128 * permille as i128 / 1000) as i64
}

/// Parse an xcolor name or mix (`red!20!white`, `#c00`).
pub(crate) fn parse_color(text: &str) -> Option<u32> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    let mut tokens = text.split('!').map(str::trim);
    let first = tokens.next()?;
    let mut color = named_color(first).or_else(|| hex_color(first))?;
    while let Some(token) = tokens.next() {
        let percent: u32 = token.trim().parse().ok()?;
        let percent = percent.min(100);
        let next = tokens.next().unwrap_or("white");
        let other = named_color(next).or_else(|| hex_color(next))?;
        color = mix(color, other, percent);
    }
    Some(color)
}

fn mix(a: u32, b: u32, percent: u32) -> u32 {
    let channel = |shift: u32| -> u32 {
        let av = (a >> shift) & 0xff;
        let bv = (b >> shift) & 0xff;
        ((av * percent + bv * (100 - percent) + 50) / 100) & 0xff
    };
    (channel(16) << 16) | (channel(8) << 8) | channel(0)
}

fn hex_color(text: &str) -> Option<u32> {
    let digits = text.strip_prefix('#')?;
    let value = |byte: u8| -> Option<u32> {
        match byte {
            b'0'..=b'9' => Some((byte - b'0') as u32),
            b'a'..=b'f' => Some((byte - b'a' + 10) as u32),
            b'A'..=b'F' => Some((byte - b'A' + 10) as u32),
            _ => None,
        }
    };
    let bytes = digits.as_bytes();
    match bytes.len() {
        3 => {
            let r = value(bytes[0])?;
            let g = value(bytes[1])?;
            let b = value(bytes[2])?;
            Some((r * 17) << 16 | (g * 17) << 8 | (b * 17))
        }
        6 => {
            let mut out = 0u32;
            for byte in bytes {
                out = out * 16 + value(*byte)?;
            }
            Some(out)
        }
        _ => None,
    }
}

fn named_color(text: &str) -> Option<u32> {
    Some(match text.to_ascii_lowercase().as_str() {
        "black" => 0x000000,
        "white" => 0xffffff,
        "red" => 0xff0000,
        "green" => 0x00ff00,
        "blue" => 0x0000ff,
        "cyan" => 0x00ffff,
        "magenta" => 0xff00ff,
        "yellow" => 0xffff00,
        "gray" | "grey" => 0x808080,
        "darkgray" | "darkgrey" => 0x404040,
        "lightgray" | "lightgrey" => 0xbfbfbf,
        "brown" => 0xbf8040,
        "lime" => 0xbfff00,
        "olive" => 0x808000,
        "orange" => 0xff8000,
        "pink" => 0xffbfbf,
        "purple" => 0xbf0040,
        "teal" => 0x008080,
        "violet" => 0x8000ff,
        "darkblue" => 0x000080,
        "darkgreen" => 0x008000,
        "darkred" => 0x800000,
        "lightblue" => 0xadd8e6,
        "lightgreen" => 0x90ee90,
        "lightred" => 0xff8080,
        _ => return None,
    })
}
