# Plan: pane-layout / dispatch race

## Operator reproduction (2026-08-03)

> Run Agent Doc on `lazily.md`, then navigate the editor to
> `agent-doc-bugs2.md` **while the lazily.md dispatch is still occurring**. The
> yellow tmux bar offering to run agent-doc on lazily.md appears, and lazily.md's
> pane is left in the agent-doc tmux window.

Confirmed in `pane_layout_projection` at 07:02:00Z:

```
phase=retry_pending  observation=pane_count_mismatch
expected=[agent-doc-bugs2.md, haiven-dev.md]              <- 2, editor-visible
actual  =[agent-doc-bugs2.md, lazily.md, haiven-dev.md]   <- 3
panes   =["%72", "%77", "%95"]
```

Geometry at that moment: `%72` left=0, `%77` left=61, `%95` left=121.

## Mechanism

Two writers race on the same window:

1. **Dispatch** routes (and if needed creates) a pane for its target document.
2. **The editor-surface observation** recomputes `columns` from whatever files
   are visible *now*.

Navigating away mid-dispatch makes the second exclude the document the first is
still setting up. The dispatched pane survives as an extra the layout does not
model, and the window can never satisfy a column set that omits it.

## Two distinct defects, do not conflate them

### A. Permanent `retry_pending` on `pane_count_mismatch`

The projection retries a reconciliation that cannot succeed: it will not close a
pane hosting an active agent, and the desired column set has no slot for it. It
should reach a terminal, named state instead of retrying forever — either by
adopting the extra pane as a column or by declaring the window operator-owned
until the dispatch settles.

### B. Column index is not physical pane order

Convergence compares `expected` and `actual` as ordered **document lists** and
never checks that the panes hosting them are in the corresponding left-to-right
order. At 06:31:30Z this produced `phase=converged observation=synced` with
`panes=["%77","%95"]` while `%77` sat at x=181 and `%95` at x=121 — the pane list
physically reversed, and `%72` at x=0 not modelled at all. Focus then selects
column 0 correctly per the model and lands on the rightmost pane, which is the
operator-visible symptom: *navigating to/from lazily.md focuses the right pane
when the left was expected*.

**Do not "fix" this by making focus index into the physical pane list.** With the
window in this state that selects `%72`, a pane running a different document's
agent — injecting into the wrong session, which is worse than focusing the wrong
pane. The comparison is what is wrong, not the focus lookup.

## Work

1. Make the race observable: log the dispatch's target document and the
   surface-observation generation together, so a pane created by dispatch is
   attributable rather than inferred from a count mismatch.
2. Decide the ownership rule for a dispatched-but-not-visible document — adopt as
   a column, or exclude from the window used for column indexing. It is currently
   neither, which is what makes column indices meaningless.
3. Give `pane_count_mismatch` a terminal outcome. Retrying forever is the bug;
   any named resolution is better than an unbounded retry.
4. Add a physical-order assertion to convergence: `panes[i]` must be the i-th
   pane by `pane_left`, or the layout is not synced.

## Tests

The 07:02:00Z and 06:31:30Z projections are both recorded above with their
geometry, so both states can be replayed as fixtures without a live IDE:

- three panes, two columns, `expected != actual` → must not stay `retry_pending`
  indefinitely.
- two columns whose panes are physically reversed → must **not** report
  `converged`/`synced`. This is the assertion that is missing today; it is what
  let the wrong-pane focus look correct to the system.

## Related

Focus resolution reads `file_panes` in `apply_pane_layout_focus_effect`
(`agent-doc-controller-io/src/project_controller/rpc.rs`); `file_panes` is
produced by `tmux_router::sync` from `columns`. `derive_pane_layout_projection`
(`agent-doc-controller-io/src/project_controller.rs`) is where convergence is
decided.
