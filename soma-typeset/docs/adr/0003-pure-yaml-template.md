# ADR-0003: Pure YAML template; no CSS engine

- Status: Accepted
- Date: 2026-11-20
- Related: `docs/architecture-v2.md` §7

## Context

The template must define page design and styles, while the engine config defines
algorithm behavior. A CSS-like language would immediately grow into a selector
engine, cascade, and media-query surface that this project does not need.

Decision 1 from the architecture review: **pure YAML**.

## Decision

A template is a directory of YAML data and assets:

```text
template/
  manifest.yaml   # page, slots, cover, headers/footers, font refs
  styles.yaml     # rules: match -> properties
  assets/
```

### `styles.yaml` shape

```yaml
schema: soma.styles/1
rules:
  - match: { kind: paragraph, role: body }
    properties: { font: serif, size: 10.5pt, line_height: 1.6 }
  - match: { kind: heading, level: 1 }
    priority: 10
    properties: { font: sans, size: 20pt, keep_with_next: true }
```

Rules are data, not CSS:

- `match` recognizes only declared fields (`kind`, `role`, `class`, `level`,
  `lang`, `section`, `page-master`).
- `priority` is an optional integer (default 0); higher wins, ties are resolved
  by list order (later wins).
- `properties` is a typed set; unknown properties are diagnostics, not ignored.
- No combinators, pseudo-classes, media queries, functions, or scripts.

### Separation from engine config

The template cannot contain `tolerance`, cost weights, `max_kp_atoms`, fallback
flags, or any algorithm knob. Those live in `EngineConfig`/preset. A template
that contains them fails validation.

### Units

Template decimals (`10.5pt`, `18mm`, `1.6`) are parsed as decimal rationals and
converted to integer units with a fixed rounding rule (round-half-up) recorded
in `templateVersion`. Floats do not enter layout.

### Hashing

`templateHash` covers manifest, styles, font references, and the asset manifest.
Same content + same template + same metrics + same engine config must produce
the same bytes.

## Consequences

- No CSS parser dependency; deterministic, small implementation.
- A custom YAML schema and validator are required.
- Designers get a narrower but predictable vocabulary.
- AI-generated templates are just YAML and can be schema-validated.

## Alternatives rejected

- **CSS/CSS-like**: too large, nondeterministic corners, and invites content CSS.
- **Rust code as template**: unsafe for untrusted templates and impossible to
  hash/validate as data.
- **TOML/JSON**: YAML is the better authoring format for nested page/style data;
  JSON remains the canonical output format.
