# ADR-0016: `soma.engine/1` engine configuration

- Status: Accepted
- Date: 2026-11-20
- Related: architecture v2 §9, ADR-0009 (semantic/presentation boundary),
  ADR-0013 (vertical cost model), `docs/debt.md` D29/D59
- Supersedes: nothing. Closes D29 and D59.

## Context

The engine knobs existed only as Rust structures: `ParamSet`/`BreakConfig` for
the inline kernel and `VerticalParams` for pagination. A caller could change
them, but there was no document a human could review, version, or diff.
Architecture §9 already specified the intended artifact:

```yaml
schema: soma.engine/1
preset: default-v1
window: paragraph
max_kp_atoms: null
fallback:
  allow_emergency: true
  allow_forbidden: true
```

Two constraints shape the implementation:

- **Templates must not accept engine knobs** (ADR-0009, Gate A enforces it), so
  the engine parser cannot live in `soma-template`.
- **Presets are frozen.** Changing a default requires a new preset id, so the
  config exposes *switches and overrides*, not a full weight table.

## Decision

Add `soma-engine` (no_std, no dependencies except `soma-core`).

### Shape

Fixed, two levels, no sequences:

```yaml
schema: soma.engine/1          # required
preset: default-v1             # optional; the only preset
window: paragraph              # optional; only paragraph is implemented
max_kp_atoms: null             # null or 1..=10000
fallback:                      # optional overrides of default-v1
  allow_emergency: true
  allow_forbidden: true
  allow_overfull: true
  emergency_stretch: 3em
vertical:                      # optional overrides of default_v1
  page_weight: 0
  badness_scale: 100
  overflow_penalty: 1000000
  club_penalty: 10000
  widow_penalty: 10000
```

- Unknown keys, unknown sections, duplicate keys, tabs for indentation, and
  values outside their ranges are errors with `line:column:path:reason`.
- Scalars may be plain or quoted; `#` comments are stripped outside quotes.
- Lengths are integer `em` (no fractional em, no other units) because the config
  is engine geometry, not presentation.
- `fallback` and `vertical` are applied to `ParamSet::default_v1()` and
  `VerticalParams::default_v1()`, then validated by the kernel's own
  `ParamSet::validate`. Cross-field rules (for example `allow_forbidden`
  requires `allow_emergency`) stay in one place.

### Results

- `EngineConfig::break_config()` returns the `BreakConfig`; `vertical()` returns
  the `VerticalParams`. No new layout entry point is needed: callers use the
  existing `layout_with`.
- `canonical_json()` and `hash()` are part of the configuration's identity. The
  canonical bytes include the preset id and every applied value, so a config
  file can be cached by hash.
- Because the config resolves to the same `ParamSet`/`VerticalParams` as a
  hand-built configuration, the *page* result needs no new schema field:
  `engineHash` and `verticalHash` already identify the effective model. A config
  equal to `default-v1` produces byte-identical pages to `layout(...)`.
- `VerticalParams` moved from `soma-layout` to `soma-core` (re-exported by the
  layout crate) so the config crate does not depend on the layout layer.

## Consequences

- `soma.engine/1` is a schema in its own right; Gate A checks that the crate
  depends only on `soma-core`, stays `no_std`, and declares the schema id.
- The WASI fixture parses the documented default config and lays out through
  `layout_with`, so the parser runs on both native and wasm; the canonical bytes
  are unchanged because every value matches the preset.
- `window: page` and `window: chapter` are rejected in this model version. The
  kernel already reports `UnsupportedWindow`; accepting them in the config would
  move the failure later.
- Adding a preset is a code change with its own ADR; the config only selects it.

## Alternatives rejected

- **Reuse the template YAML subset.** It would put engine knobs into the
  presentation layer, which ADR-0009 and Gate A forbid, and it would make the
  template schema and the engine schema share an error surface for no gain.
- **Expose every weight in the file.** Presets exist so documents cannot drift
  into unreviewed cost models; the architecture only allows switches and
  window-size overrides.
- **JSON instead of YAML.** The architecture names YAML, and the fixed shape is
  smaller and stricter than a general parser would be.
- **Put the parser in `soma-core`.** The kernel stays a geometry library; config
  parsing is an input adapter.
