//! Fixed-point geometry for layout decisions.
//!
//! One em is `UNITS_PER_EM` units. The core intentionally does not use floating
//! point: the same input, metrics table, and configuration must produce the
//! same serialized layout on native and wasm targets.

/// Number of fixed-point units in one em.
pub const UNITS_PER_EM: i32 = 4096;

/// A signed width in 1/4096 em units.
///
/// Negative values are useful for adjustments; atom and glue widths should be
/// non-negative.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Width(pub i32);

impl Width {
    pub const ZERO: Self = Self(0);
    pub const EM: Self = Self(UNITS_PER_EM);

    pub const fn new(units: i32) -> Self {
        Self(units)
    }

    pub const fn from_em(em: i32) -> Self {
        Self(em * UNITS_PER_EM)
    }

    pub const fn from_em_frac(num: i32, den: i32) -> Self {
        assert!(den != 0, "width denominator must be non-zero");
        Self(num * UNITS_PER_EM / den)
    }

    pub const fn units(self) -> i32 {
        self.0
    }

    pub const fn abs(self) -> Self {
        Self(if self.0 < 0 { -self.0 } else { self.0 })
    }

    pub const fn max(self, other: Self) -> Self {
        Self(if self.0 > other.0 { self.0 } else { other.0 })
    }

    pub const fn min(self, other: Self) -> Self {
        Self(if self.0 < other.0 { self.0 } else { other.0 })
    }

    pub const fn saturating_add(self, other: Self) -> Self {
        Self(self.0.saturating_add(other.0))
    }

    pub const fn saturating_sub(self, other: Self) -> Self {
        Self(self.0.saturating_sub(other.0))
    }

    /// Multiply by a rational, truncating toward zero.
    ///
    /// The intermediate value is computed in `i64` so normal layout arithmetic
    /// does not overflow.
    pub const fn scale(self, num: i32, den: i32) -> Self {
        assert!(den != 0, "width denominator must be non-zero");
        Self(((self.0 as i64 * num as i64) / den as i64) as i32)
    }
}

impl core::ops::Add for Width {
    type Output = Self;

    fn add(self, other: Self) -> Self {
        Self(self.0 + other.0)
    }
}

impl core::ops::Sub for Width {
    type Output = Self;

    fn sub(self, other: Self) -> Self {
        Self(self.0 - other.0)
    }
}

impl core::ops::Neg for Width {
    type Output = Self;

    fn neg(self) -> Self {
        Self(-self.0)
    }
}

impl core::ops::Mul<i32> for Width {
    type Output = Self;

    fn mul(self, rhs: i32) -> Self {
        Self(self.0 * rhs)
    }
}

impl core::ops::Div<i32> for Width {
    type Output = Self;

    fn div(self, rhs: i32) -> Self {
        Self(self.0 / rhs)
    }
}

/// A cost or demerit. `i64` keeps long-paragraph sums away from overflow.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Cost(pub i64);

impl Cost {
    pub const ZERO: Self = Self(0);

    pub const fn saturating_add(self, other: Self) -> Self {
        Self(self.0.saturating_add(other.0))
    }
}

impl core::ops::Add for Cost {
    type Output = Self;

    fn add(self, other: Self) -> Self {
        Self(self.0 + other.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn em_conversions_are_exact() {
        assert_eq!(Width::from_em(1).units(), 4096);
        assert_eq!(Width::from_em_frac(1, 2).units(), 2048);
        assert_eq!(Width::from_em_frac(1, 4).units(), 1024);
        assert_eq!(Width::from_em_frac(1, 64).units(), 64);
    }

    #[test]
    fn scale_truncates_toward_zero_deterministically() {
        assert_eq!(Width(3).scale(1, 2).units(), 1);
        assert_eq!(Width(-3).scale(1, 2).units(), -1);
    }

    #[test]
    fn arithmetic_is_saturating_where_requested() {
        assert_eq!(Width(i32::MAX).saturating_add(Width(1)), Width(i32::MAX));
        assert_eq!(Width(i32::MIN).saturating_sub(Width(1)), Width(i32::MIN));
    }
}
