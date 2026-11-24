# TikZ sample

- `figure.tex` — the picture as standalone LaTeX for `pdflatex`; this is the
  visual reference.
- `figure-soma.tex` — the soma input: the same picture plus prose and a grid
  example.

Build the reference:

```sh
pdflatex -interaction=nonstopmode figure.tex
pdftoppm -png -r 300 -singlefile figure.pdf figure-ref
```

Build the soma PDF (fonts and metrics as in `scripts/plex.sh`):

```sh
sh scripts/plex.sh
# examples/templates/out/tikz-sample.pdf
```

The subset and its known differences from PGF are documented in
[`docs/tikz.md`](../../docs/tikz.md) (ADR-0092).
