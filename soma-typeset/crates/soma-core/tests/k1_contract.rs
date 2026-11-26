//! K1 public contract tests.
//!
//! These tests define the parameter API at the crate seam. They are green as
//! of the K1 green step; keep them green while adding the next K1 slices.

use soma_core::params::{
    CostParams, FallbackParams, FeatureFlags, ParamSet, Permille, TieBreak, TrimPriority, Weight,
};
use soma_core::preset;
use soma_core::units::Width;

fn default_costs() -> CostParams {
    CostParams {
        line: Weight(10),
        badness_scale: Weight(100),
        badness_exponent: 3,
        break_penalty: Weight(100),
        compress_amount: Weight(20),
        compress_count: Weight(5),
        boundary_deviation: Weight(50),
        short_last_scale: Weight(1_000_000),
        short_last_threshold: Width::from_em(2),
        height_scale: Weight(0),
        height_threshold: None,
        emergency: Weight(10_000_000),
        forbidden: Weight(100_000_000),
        fitness: Weight(0),
    }
}

#[test]
fn default_v1_matches_the_frozen_table() {
    let params = ParamSet::default_v1();

    assert_eq!(params.preset.0, "default-v1");
    assert_eq!(params.model_version, 4);
    assert_eq!(params.break_params.tolerance, Permille(3000));
    assert_eq!(params.break_params.emergency_tolerance, Permille(10_000));
    assert_eq!(params.break_params.max_lines, None);
    assert_eq!(params.break_params.max_kp_atoms, None);
    assert_eq!(params.break_params.trim_priority, TrimPriority::EndFirst);
    assert_eq!(params.break_params.tie_break, TieBreak::CostLineStart);
    assert_eq!(params.break_params.costs, default_costs());
    assert_eq!(
        params.break_params.fallback,
        FallbackParams {
            allow_emergency: true,
            allow_forbidden: true,
            allow_overfull: true,
            emergency_stretch: Width::from_em(3),
        }
    );
    assert_eq!(
        params.features,
        FeatureFlags {
            edge_trim: true,
            boundary_deviation: true,
            height_cost: false,
            fitness: false,
            forced_breaks: true,
        }
    );
}

#[test]
fn book_v1_only_raises_the_short_last_threshold() {
    let publish = ParamSet::publish_v1();
    let book = ParamSet::book_v1();

    assert_eq!(book.break_params.tolerance, publish.break_params.tolerance);
    assert_eq!(
        book.break_params.costs.short_last_threshold,
        Width::from_em(3)
    );
    assert_eq!(
        book.break_params.costs.short_last_scale,
        publish.break_params.costs.short_last_scale
    );
    assert_eq!(book.preset.0, "book-v1");
    assert_eq!(
        book.break_params.costs,
        CostParams {
            short_last_threshold: Width::from_em(3),
            ..publish.break_params.costs
        }
    );
}

#[test]
fn preset_module_and_paramset_are_one_source() {
    assert_eq!(preset::default_v1(), ParamSet::default_v1());
}

#[test]
fn validation_rejects_bad_values() {
    let base = ParamSet::default_v1();
    assert!(base.validate().is_ok());

    let mut bad_exponent = base;
    bad_exponent.break_params.costs.badness_exponent = 0;
    let error = bad_exponent.validate().unwrap_err();
    assert!(error.path.contains("badness_exponent"));

    let mut bad_max_lines = base;
    bad_max_lines.break_params.max_lines = Some(0);
    let error = bad_max_lines.validate().unwrap_err();
    assert!(error.path.contains("max_lines"));

    let mut bad_max_kp_atoms = base;
    bad_max_kp_atoms.break_params.max_kp_atoms = Some(0);
    let error = bad_max_kp_atoms.validate().unwrap_err();
    assert!(error.path.contains("max_kp_atoms"));

    let mut bad_emergency = base;
    bad_emergency.break_params.emergency_tolerance = Permille(2999);
    let error = bad_emergency.validate().unwrap_err();
    assert!(error.path.contains("emergency_tolerance"));

    let mut bad_fallback = base;
    bad_fallback.break_params.fallback.allow_emergency = false;
    let error = bad_fallback.validate().unwrap_err();
    assert!(error.path.contains("allow_forbidden"));

    let mut bad_height = base;
    bad_height.break_params.costs.height_scale = Weight(1);
    bad_height.break_params.costs.height_threshold = Some(Width::ZERO);
    let error = bad_height.validate().unwrap_err();
    assert!(error.path.contains("height_threshold"));

    let mut bad_badness = base;
    bad_badness.break_params.costs.badness_scale = Weight(1_000_001);
    let error = bad_badness.validate().unwrap_err();
    assert!(error.path.contains("badness_scale"));
}

#[test]
fn override_changes_only_the_expected_field() {
    let base = ParamSet::default_v1();
    let mut tuned = base;
    tuned.break_params.tolerance = Permille(2500);

    // Every other field must be identical: an override is a single-value edit,
    // not a silently rebuilt preset.
    assert_eq!(tuned.preset, base.preset);
    assert_eq!(tuned.model_version, base.model_version);
    assert_eq!(tuned.rules_version, base.rules_version);
    assert_eq!(tuned.metrics_version, base.metrics_version);
    assert_eq!(tuned.break_params.costs, base.break_params.costs);
    assert_eq!(tuned.break_params.fallback, base.break_params.fallback);
    assert_eq!(
        tuned.break_params.emergency_tolerance,
        base.break_params.emergency_tolerance
    );
    assert_eq!(tuned.break_params.max_lines, base.break_params.max_lines);
    assert_eq!(
        tuned.break_params.max_kp_atoms,
        base.break_params.max_kp_atoms
    );
    assert_eq!(
        tuned.break_params.trim_priority,
        base.break_params.trim_priority
    );
    assert_eq!(tuned.break_params.tie_break, base.break_params.tie_break);
    assert_eq!(tuned.features, base.features);
    assert_ne!(tuned.break_params.tolerance, base.break_params.tolerance);
    assert!(tuned.validate().is_ok());

    // The hash tracks the edit; the preset id does not silently change.
    assert_ne!(tuned.param_hash(), base.param_hash());
}

#[test]
fn param_hash_is_stable_and_weight_sensitive() {
    let base = ParamSet::default_v1();
    let hash = base.param_hash();
    assert_eq!(hash, ParamSet::default_v1().param_hash());

    let mut changed_weight = base;
    changed_weight.break_params.costs.line = Weight(base.break_params.costs.line.0 + 1);
    assert_ne!(hash, changed_weight.param_hash());

    let mut changed_feature = base;
    changed_feature.features.edge_trim = false;
    assert_ne!(hash, changed_feature.param_hash());

    // rules/metrics versions are separate version tags, not part of the
    // BreakParams + flags hash.
    let mut changed_metadata = base;
    changed_metadata.rules_version = changed_metadata.rules_version.wrapping_add(1);
    assert_eq!(hash, changed_metadata.param_hash());
}
