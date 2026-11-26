# ADR-0120: Content-hashed UI assets and a downloadable source file

- Status: Accepted
- Date: 2026-11-26
- Related: ADR-0118 (browser-local Studio), ADR-0119 (GB2312 subsets)
- Touches: `crates/soma-studio-ui/Trunk.toml`, `scripts/local-assets.sh`,
  `examples/book/welcome.md`, deployment `dist/`

## Context

The first browser-local deployment served fixed asset names
(`soma-studio-ui_bg.wasm`, `soma-studio-ui.js`) with a one-hour cache. The
next deployment replaced `/fonts/v1/` with `/fonts/v2/` and removed the old
figures. A browser that still had the old wasm kept requesting
`/fonts/v1/pdf-subset/...` and `/fig/...`, so PDF export showed font 404s even
though every v2 URL was healthy.

The page already sent `Cache-Control: no-cache` for `index.html`, but the
fixed wasm URL meant a refreshed HTML could still point at a stale module. The
font path and the module were versioned independently, which allowed a module
from one release to outlive its fonts.

## Decision

1. `trunk` builds with `filehash = true`. `index.html` references
   content-hashed JS/CSS/wasm names and remains revalidating; the hashed files
   can be cached for a long time.
2. Font bytes live under a namespace that changes when they change
   (`/fonts/v2/`). A new wasm release references a compatible namespace; old
   modules are never mixed with new font URLs.
3. `scripts/local-assets.sh` copies `examples/book/welcome.md` to
   `dist/welcome.md`, so the default source is also a directly downloadable
   static file (`https://somadoc.art/welcome.md`). The document links to it.
4. The public deployment keeps only the single Markdown entry, the template,
   engine, metrics, hyphenation and `dist/`; the old fixed-name assets and the
   `v1` font tree are not served.

## Consequences

- A browser reload picks up the new `index.html`, which names a new wasm and JS;
  an old wasm cannot be paired with missing fonts. A stale tab needs one reload,
  not a cache purge.
- Hashed assets can be cached aggressively; only `index.html` revalidates.
- `/welcome.md` is available as a normal text file and is the same content the
  embedded source uses.
- No engine, layout, canonical schema or hash changes.

## Alternatives considered

- **Keep fixed names and lower the cache TTL.** Rejected: it still leaves a
  window where an old module looks up deleted font paths.
- **Add v1 font redirects forever.** Rejected: compatibility aliases would
  accumulate and hide the real contract that a module and its fonts ship
  together.
- **Embed fonts in the wasm.** Rejected: a font revision would invalidate the
  engine payload and make first paint larger; ADR-0119 keeps them separate.
