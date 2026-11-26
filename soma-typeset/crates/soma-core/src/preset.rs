//! Built-in parameter presets.
//!
//! Presets are data, not algorithm branches. The K1 preset delegates to
//! `ParamSet::default_v1`.

use crate::params::ParamSet;

/// The frozen `default-v1` preset.
pub fn default_v1() -> ParamSet {
    ParamSet::default_v1()
}

/// Chinese publishing defaults (tighter tolerance than `default-v1`).
pub fn publish_v1() -> ParamSet {
    ParamSet::publish_v1()
}

/// LaTeX article defaults (10pt class, `latex-v1`).
pub fn latex_v1() -> ParamSet {
    ParamSet::latex_v1()
}

/// `publish-v1` with the K7 line-rhythm term enabled.
pub fn rhythm_v1() -> ParamSet {
    ParamSet::rhythm_v1()
}
