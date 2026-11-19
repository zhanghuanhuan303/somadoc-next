//! Vertical cost model for pagination.
//!
//! The kernel owns the shape; the layout layer consumes it. Keeping the struct
//! here lets the engine configuration crate describe pagination knobs without
//! depending on the layout crate.

/// Vertical cost model for pagination.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VerticalParams {
    /// Weight applied for a page whose fill leaves no freedom. `0` keeps the
    /// page count neutral; the DP then optimises badness only.
    pub page_weight: i64,
    /// Scale for the cubic page badness. A page short by one body line costs
    /// `badness_scale` (100 by default).
    pub badness_scale: i64,
    /// Cost of a page that must hold one block taller than the content area.
    pub overflow_penalty: i64,
    /// Cost of breaking a paragraph after its first line (a one-line page foot).
    pub club_penalty: i64,
    /// Cost of breaking a paragraph before its last line (a one-line page head).
    pub widow_penalty: i64,
}

impl VerticalParams {
    pub const fn default_v1() -> Self {
        Self {
            page_weight: 0,
            badness_scale: 100,
            overflow_penalty: 1_000_000,
            club_penalty: 10_000,
            widow_penalty: 10_000,
        }
    }

    /// Hash of the vertical cost model in a fixed field order. The page result
    /// carries it so two runs with different pagination knobs cannot share
    /// canonical bytes by accident.
    pub fn hash(&self) -> u64 {
        let mut state: u64 = 0xcbf2_9ce4_8422_2325;
        for value in [
            self.page_weight,
            self.badness_scale,
            self.overflow_penalty,
            self.club_penalty,
            self.widow_penalty,
        ] {
            for byte in value.to_le_bytes() {
                state ^= byte as u64;
                state = state.wrapping_mul(0x0000_0100_0000_01b3);
            }
        }
        state
    }
}

impl Default for VerticalParams {
    fn default() -> Self {
        Self::default_v1()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_hash_is_content_addressed() {
        let base = VerticalParams::default_v1();
        assert_eq!(base.hash(), base.hash());
        let mut changed = base;
        changed.widow_penalty += 1;
        assert_ne!(base.hash(), changed.hash());
    }
}
