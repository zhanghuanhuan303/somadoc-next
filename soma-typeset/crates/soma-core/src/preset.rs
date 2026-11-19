//! Built-in parameter presets.
//!
//! Presets are data, not algorithm branches. The K1 preset delegates to
//! `ParamSet::default_v1`.

use crate::params::ParamSet;

/// The frozen `default-v1` preset.
pub fn default_v1() -> ParamSet {
    ParamSet::default_v1()
}
