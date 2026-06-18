# SoK: Metadata-Hiding Group Registries with SNARK-Gated State Changes

Working source for a Systematization of Knowledge paper surveying the design
space of metadata-hiding group registries with SNARK-gated state changes.

This paper is **separate from** the experience-report paper at `paper/onym.v4.md`
and its IEEE CSMAG submission at `paper/venue/ieee/`. The SoK targets a
top-tier security conference (IEEE S&P, USENIX Security, CCS, or FC); the
final venue is selected at the integration pass.

## Build

Requires `latexmk` and a TeX Live distribution with `IEEEtran.cls`,
`tikz`, `booktabs`, `cleveref`.

```sh
make            # build main.pdf
make watch      # continuous rebuild (latexmk -pvc)
make clean      # remove aux files, keep PDF
make distclean  # remove all generated files including PDF
make benchmarks # re-run the C1 row benchmark harness
```

## Anonymization for double-blind submission

Edit `main.tex`:

```tex
\anonymousfalse  →  \anonymoustrue
```

This collapses the author block to "Anonymous Submission". The body of
the paper refers to the onym deployment as "the Stellar deployment
described in [SEP-XXXX]" rather than first-person, so no further
redaction should be needed — but verify before submitting.

## Layout

| Path | Purpose |
|---|---|
| `main.tex` | Entry point — class, packages, `\input` lines |
| `sections/` | Eleven section files (00-abstract through 10-conclusion) |
| `figures/` | TikZ source for Figures 1 (taxonomy) and 2 (migration DAG) |
| `tables/` | Five comparison tables for §4 |
| `benchmarks/c1/` | First-party measurement harness for the C1 row |
| `references.bib` | BibTeX |

## Drafting workflow

Sections are drafted, reviewed, and locked one at a time in numerical
order. Each section's lock-commit uses the format
`paper(sok): finalize §N — <title>`. No work begins on section N+1
until section N is locked.

## Measurement methodology (C1 row)

See `benchmarks/c1/methodology.md` for hardware,
software, and pinned versions. The harness writes `results.json`; LaTeX
table cells `\input{}` from generated `.tex` snippets so the PDF stays
in sync with measurements when the benchmark is re-run.

## Status

Scaffolded; no prose drafted yet. Section stubs in `sections/` show the
intended structure of each section as comments.
