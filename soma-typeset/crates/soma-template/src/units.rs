//! Exact length units.
//!
//! Everything resolves to `Width` in 1/4096 em, so the template layer and the
//! layout layer speak one integer unit system (ADR-0006). No `f32`/`f64` is used
//! anywhere: decimals are parsed into an exact rational, and division truncates
//! toward zero — the same rule the kernel uses.
//!
//! Physical units resolve against the **root font size**, because `1em` is
//! defined by the body text. A template must therefore declare an absolute
//! `font-size` on the document element; every other length is derived from it.

use alloc::string::String;
use soma_core::Width;

pub const UNITS_PER_EM: i64 = 4096;

/// Exact rational number with a positive denominator.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rational {
    num: i64,
    den: i64,
}

impl Rational {
    pub const ZERO: Self = Self { num: 0, den: 1 };
    pub const ONE: Self = Self { num: 1, den: 1 };

    pub fn new(num: i64, den: i64) -> Option<Self> {
        if den == 0 {
            return None;
        }
        let (num, den) = if den < 0 { (-num, -den) } else { (num, den) };
        Some(reduce(num, den))
    }

    pub const fn num(self) -> i64 {
        self.num
    }

    pub const fn den(self) -> i64 {
        self.den
    }

    /// Parse a decimal number (`12`, `-1`, `10.5`, `.5`). Exponents are rejected
    /// so the value never depends on a float parser.
    pub fn parse(text: &str) -> Option<Self> {
        let text = text.trim();
        if text.is_empty() {
            return None;
        }
        let (negative, rest) = match text.as_bytes()[0] {
            b'-' => (true, &text[1..]),
            b'+' => (false, &text[1..]),
            _ => (false, text),
        };
        let mut parts = rest.split('.');
        let integer = parts.next().unwrap_or("");
        let fraction = parts.next().unwrap_or("");
        if parts.next().is_some() {
            return None;
        }
        if integer.is_empty() && fraction.is_empty() {
            return None;
        }
        if !integer.bytes().all(|b| b.is_ascii_digit())
            || !fraction.bytes().all(|b| b.is_ascii_digit())
        {
            return None;
        }
        let mut num: i64 = if integer.is_empty() {
            0
        } else {
            integer.parse().ok()?
        };
        let mut den: i64 = 1;
        for digit in fraction.bytes() {
            num = num.checked_mul(10)?.checked_add((digit - b'0') as i64)?;
            den = den.checked_mul(10)?;
        }
        if negative {
            num = -num;
        }
        Rational::new(num, den)
    }
}

fn gcd(mut a: i64, mut b: i64) -> i64 {
    a = a.abs();
    b = b.abs();
    while b != 0 {
        let rest = a % b;
        a = b;
        b = rest;
    }
    if a == 0 {
        1
    } else {
        a
    }
}

fn reduce(num: i64, den: i64) -> Rational {
    let divisor = gcd(num, den);
    Rational {
        num: num / divisor,
        den: den / divisor,
    }
}

/// A length unit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unit {
    /// Relative to the current font size.
    Em,
    Pt,
    Mm,
    Cm,
    In,
}

impl Unit {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Em => "em",
            Self::Pt => "pt",
            Self::Mm => "mm",
            Self::Cm => "cm",
            Self::In => "in",
        }
    }

    fn parse(suffix: &str) -> Option<Self> {
        if suffix.eq_ignore_ascii_case("em") {
            Some(Self::Em)
        } else if suffix.eq_ignore_ascii_case("pt") {
            Some(Self::Pt)
        } else if suffix.eq_ignore_ascii_case("mm") {
            Some(Self::Mm)
        } else if suffix.eq_ignore_ascii_case("cm") {
            Some(Self::Cm)
        } else if suffix.eq_ignore_ascii_case("in") {
            Some(Self::In)
        } else {
            None
        }
    }

    /// Multiply into points: `1in = 72pt`, `1in = 25.4mm`, therefore
    /// `1mm = 360/127 pt` and `1cm = 3600/127 pt`.
    fn to_points(self, value: Rational) -> Option<Rational> {
        match self {
            Self::Pt => Some(value),
            Self::In => Rational::new(value.num.checked_mul(72)?, value.den),
            Self::Mm => Rational::new(value.num.checked_mul(360)?, value.den.checked_mul(127)?),
            Self::Cm => Rational::new(value.num.checked_mul(3600)?, value.den.checked_mul(127)?),
            Self::Em => None,
        }
    }
}

/// A parsed length, not yet resolved against a font size.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LengthSpec {
    pub value: Rational,
    pub unit: Unit,
}

impl LengthSpec {
    pub const fn new(value: Rational, unit: Unit) -> Self {
        Self { value, unit }
    }

    pub const fn zero() -> Self {
        Self {
            value: Rational::ZERO,
            unit: Unit::Em,
        }
    }

    /// Parse `12pt`, `2em`, `1.5mm`, `0`. A bare number is only accepted when it
    /// is exactly zero (an unambiguous empty length).
    pub fn parse(text: &str) -> Option<Self> {
        let text = text.trim();
        let split = text
            .char_indices()
            .find(|(_, character)| character.is_ascii_alphabetic())
            .map(|(offset, _)| offset)
            .unwrap_or(text.len());
        let (number, suffix) = text.split_at(split);
        if number.trim() != number {
            return None;
        }
        let value = Rational::parse(number)?;
        if suffix.is_empty() {
            return if value == Rational::ZERO {
                Some(Self::zero())
            } else {
                None
            };
        }
        let unit = Unit::parse(suffix)?;
        Some(Self::new(value, unit))
    }

    pub const fn is_em(self) -> bool {
        matches!(self.unit, Unit::Em)
    }

    /// Physical length in points, or `None` for `em`.
    pub fn absolute_points(self) -> Option<Rational> {
        self.unit.to_points(self.value)
    }

    /// Resolve to 1/4096 em. `font_size` is the current font size (used by
    /// `em`), `body_pt` is the root font size in points (used by physical
    /// units). Truncates toward zero.
    pub fn resolve(self, font_size: Width, body_pt: Rational) -> Width {
        match self.unit {
            Unit::Em => scale_em(font_size, self.value),
            unit => {
                let points = unit.to_points(self.value).unwrap_or(Rational::ZERO);
                physical_to_em(points, body_pt)
            }
        }
    }
}

fn scale_em(font_size: Width, value: Rational) -> Width {
    let result = (font_size.0 as i128 * value.num() as i128) / value.den() as i128;
    Width(clamp_i32(result))
}

fn physical_to_em(points: Rational, body_pt: Rational) -> Width {
    // em units = points / body_pt * 4096
    let result = (points.num() as i128 * body_pt.den() as i128 * UNITS_PER_EM as i128)
        / (points.den() as i128 * body_pt.num() as i128);
    Width(clamp_i32(result))
}

fn clamp_i32(value: i128) -> i32 {
    if value > i32::MAX as i128 {
        i32::MAX
    } else if value < i32::MIN as i128 {
        i32::MIN
    } else {
        value as i32
    }
}

/// `line-height` value: a multiplier of the font size, or a fixed length.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineHeightSpec {
    Multiple(Rational),
    Length(LengthSpec),
}

impl LineHeightSpec {
    pub fn resolve(self, font_size: Width, body_pt: Rational) -> Width {
        match self {
            Self::Multiple(value) => scale_em(font_size, value),
            Self::Length(length) => length.resolve(font_size, body_pt),
        }
    }
}

/// Format a rational for diagnostics and canonical output (`num/den`).
pub fn rational_text(value: Rational) -> String {
    let mut out = String::new();
    out.push_str(&alloc::format!("{}/{}", value.num(), value.den()));
    out
}
