# ADR-0002: Clean-room design; no prior Typst project

- Status: Accepted
- Date: 2026-11-20
- Related: `docs/architecture-v2.md` §2.1, `PROVENANCE.md`

## Context

The earlier prior-research project (a Markdown-to-Typst pipeline) has been
deleted and is no longer part of the workflow. Architecture v2 must not derive
from, reference, or depend on that project.

## Decision

- This workspace is a clean-room implementation of the PRD and architecture v2.
- No code, IR, template, table, or test fixture is copied from the deleted
  project.
- Public references are limited to:
  - the PRD and this workspace's own documents;
  - the published Knuth-Plass algorithm description;
  - CLReq / GB/T and publisher practice for rules, cited by clause (not copied
    tables);
  - Rust standard library and public crate documentation.
- No path dependency or git dependency points at the deleted project or Typst.
- `soma-core` continues to forbid `typst*`, font parsers, Markdown parsers, and
  wasm-bindgen.
- Legacy seed modules in this repository (`soma-core::semantic` and
  `soma-core::text`) are quarantined: they are not part of the frozen
  architecture, no new code may depend on them, and A1 removes them from the
  public API.

## Consequences

- `PROVENANCE.md` records only new code and public references.
- The `soma-doc` schema is designed from scratch (ADR-0008), not migrated by
  copying the legacy module.
- Historical proposals remain under `docs/archive/` with a clear “not
  normative” header; they are not used as design input.
- A code review must reject new references to the deleted project.

## Alternatives rejected

- Reusing the old Typst front end, IR, or templates: contaminates the clean-room
  boundary and reintroduces the dependency tree.
- Keeping the legacy semantic module as the public DocIR: it was not designed
  for templates/pages and is not the clean-room design.
