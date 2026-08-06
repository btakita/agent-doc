# Plan: coverage-table layout for a growing language axis (`#lzcoveragelayout`)

**Status:** Design — not started. Phase 1 should land *before* the GDScript column.
**Created:** 2026-07-30
**Session:** `tasks/software/lazily.md`
**Backlog:** `#lzcoveragelayout`
**Related:** `tasks/software/plan-lazily-gd.md` (`#lzgdbinding`) — adding the 10th column is what surfaced this.

## The question

With a 10th binding landing and 64 feature rows, does the feature matrix need a new
layout: transpose to one row per language with subtables, or split the languages
across multiple tables?

## Measurement first (2026-07-30)

`lazily-spec/coverage.json` → rendered by `scripts/sync-coverage.mjs`:

| Metric | Value |
|---|---|
| Rows | 64 |
| Mark columns | 9 (10 with GDScript) |
| Feature text length | min 18, **mean 86, max 203** chars |
| All 9 marks combined | ~40 chars |
| Rendered row width | ~130 mean, ~250 max |
| Injection targets | **12** (spec `README.md`, `docs/coverage.md`, 10 sibling READMEs + formal) |

**The feature column is 68% of row width at mean and 84% at max.** A 10th language
adds ~4 characters to a row that is already 130–250 wide.

## Answer: neither of the two proposed layouts

**Transposing** (language rows × feature columns) produces **64 columns**. Strictly
worse. Reject.

**Splitting by language** destroys the only property this table has that nothing
else in the repo provides: **cross-binding parity at a glance**. That is the
drift-detection view, and it is exactly what
`scripts/check-conformance-coverage.sh` exists to protect — "a fixture lands
upstream, every binding stays green, and nobody learns that one of them is not
replaying it." Per-language tables make that class of drift invisible by
construction.

Compounding it: `sync-coverage.mjs` injects the **same full matrix into all 12
targets**, so every binding README currently shows all nine columns. Splitting by
language would leave each binding blind to the other nine.

The language axis is not the problem. The feature column is.

## Phases

### Phase 1 — Move normative prose out of the table cell

**Land this before adding the GDScript column.** It is independent of everything
else here and it addresses the measured problem directly.

- Add `id` and short `label` to each row in `coverage.json`; keep `feature`'s
  86–203 char description as a footnote block below the table, or link into
  `cell-model.md` (80KB) / `protocol.md` (88KB), which already own the normative
  text.
- `sync-coverage.mjs` renders `label` in the cell and emits the footnotes after the
  table.

Effect: table drops from ~250 to ~60 chars wide, the language axis becomes the
dominant dimension it should be, and the layout then scales past 15 languages
without further change.

Exit: rendered table under ~70 chars wide; every row's long-form text reachable in
one hop; all 12 targets regenerate identically.

### Phase 2 — Split the `—` mark (do this with Phase 1)

The legend currently reads `✅ shipped · ~ partial · — absent or not applicable`.
That conflation is a real defect at 10 columns: a language that **cannot** support a
family is indistinguishable from one that simply **has not done the work**.

GDScript forces the issue immediately — "thread-safe family parked because GDScript
threading is awkward" is a different fact from "nobody has written it yet." Split
into a not-applicable mark that carries a reason, and a distinct absent mark.

Exit: legend has four distinct marks; every not-applicable cell has a recorded
reason; no cell is blank.

### Phase 3 — Group by family, keeping all languages

One table per family, section headings matching the `conformance/` directory layout.

**Cost to budget honestly:** the existing feature strings do *not* already encode
families. Checked — **54 distinct prefixes across 64 rows**, mostly unique. So
`family` must be *authored* as a new field, a one-time reviewed pass over all 64
rows. It cannot be inferred from the current text.

Exit: every row carries `family`; per-family tables render; no row is orphaned.

### Phase 4 — Family × language roll-up

A compact summary (~20 rows × 10 short-label columns) as the "where are we" view,
with the per-family detail tables below.

**Requires a stated roll-up rule or it lies:** ✅ only when every row in the family
is ✅, `~` when mixed, `—`/N-A when none. Put the rule in the legend, not just the
generator.

Exit: roll-up regenerates from `rows` alone with no hand-maintained second source.

## Related decision, separable

The 12-target injection is why `coverage.json` is a hot multi-session file. An
alternative once Phase 4 exists: binding READMEs carry the **roll-up (all
languages) plus a link** to `lazily-spec/docs/coverage.md` for the 64-row detail.
Cuts the sync surface from 12 copies of the big table to 12 copies of a small one
while keeping a parity signal in every README at family resolution.

Trade-off to accept explicitly: a binding's README would no longer show *row-level*
parity. Decide separately from layout; do not fold it into Phase 1.

## Constraints

- `coverage.json` is hot across concurrent sessions — schema changes land in **one
  commit**, and the generator change lands with them so no target is left stale.
- `sync-coverage.mjs --check` must stay the CI gate throughout; a layout change that
  cannot be verified by `--check` is not done.
- Do not flip any mark's *value* as part of a layout change. Layout and coverage
  claims are separate commits.
