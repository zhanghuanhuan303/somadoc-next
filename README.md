# somadoc

**soma-typeset — a Chinese-first, deterministic typesetting engine with a
browser-local Studio.**

The engine, CLI, renderers and Studio live in [`soma-typeset/`](soma-typeset/).
The original project brief is [`prd.md`](prd.md); the normative architecture is
[`soma-typeset/docs/architecture-v2.md`](soma-typeset/docs/architecture-v2.md),
and the dated map is
[`soma-typeset/docs/soma-atlas.html`](soma-typeset/docs/soma-atlas.html).

- Hosted demo: <https://somadoc.art>
- Engine workspace: [`soma-typeset/README.md`](soma-typeset/README.md)
- Open-source layout: [`soma-typeset/REPO_LAYOUT.md`](soma-typeset/REPO_LAYOUT.md)

## Quick start

```sh
cd soma-typeset
cargo test --workspace
bash scripts/gates.sh
```

The browser Studio needs `trunk`, Chromium for the S gates, and a font
directory:

```sh
(cd crates/soma-studio-ui && trunk build --release)
SOMA_FONT_DIR=/path/to/fonts bash scripts/local-assets.sh
bash scripts/studio.sh
```

## License

Apache-2.0 — see [`LICENSE`](LICENSE) and [`NOTICE`](NOTICE). The evaluation
corpus is CC-BY-4.0; see [`soma-typeset/corpus/README.md`](soma-typeset/corpus/README.md).

## Contributing, security, conduct

See [`CONTRIBUTING.md`](CONTRIBUTING.md), [`SECURITY.md`](SECURITY.md) and
[`CODE_OF_CONDUCT.md`](CODE_OF_CONDUCT.md).
