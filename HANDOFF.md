# HANDOFF — #gh136follow (GH #136 follow-ups)

Worktree: `/home/brian/work/btakita/agent-doc-wt/gh136follow`, branch `gh136follow` (from main).
Do NOT push, install, release, merge, or close GH #136.

Siblings (avoid their functions; note overlap here):
- `netadv5` — timeout-as-verdict: R1 focus-timeout SIGKILL, R7 layout sync lock-skip (`src/ffi.rs` sync lock,
  focus RPC timeout path).
- `netadv3` — ports StaleColumnRecycle etc. to NetChannel + lost-message fixes. So this branch does NOT edit
  `formal/tla/StaleColumnRecycle.tla`; new invariants live in a NEW NetChannel model (see below).

## Items and design decisions

(a) `columns <= observed_panes (min 1)` for every publisher.
    Literal reading would forbid the layout from ever growing (an operator opening a split publishes
    `observed + 1` by definition). Decision: the bound is enforced for every publication whose width is
    DERIVED (escalation, `ensure` route, recycle republish — anything that recomputes columns from
    retained state): `columns <= max(last_extent, 1)` where `last_extent` = last observed tmux panes +
    columns the last pass deliberately gated (falls back to the retained width when nothing was ever
    observed). A publication that is itself a positive editor split observation (plugin publication,
    `exact` route, editor_surface `Sync`, explicit command) may widen, is logged as
    `pane_layout_publication_widened`, and its realisation is bounded by the stale gate (a stale pane can
    never add a column from ANY window — item e). Pure fn + TLA model.
(b) 60s focus settle + `superseded` generation-reset race (`1061.md`, issue edit history: duplicate
    `attempt_id`, `merge=seeded retained_columns=0`, generation 6 -> 1). Investigation in progress.
(c) Supervisors stale on OLD code never consume a recycle request. Controller-driven one-time replacement,
    only at a proven idle boundary, authorized by positive evidence (`/proc/<pid>/exe` unlinked), never
    by elapsed time.
(d) Republish the layout when a recycle settles, so a gated document returns without an editor event.
(e) Stale panes in non-`stash` windows are bounded like stash panes (any window other than the target).

## Status

- [ ] (e) gate generalisation + tests
- [ ] (a) width bound + tests
- [ ] (d) republish on recycle + tests
- [ ] (c) idle-boundary replacement + tests
- [ ] (b) cause + fix + SimWorld coder_zscaler regression
- [ ] TLA model `LayoutWidthBound` (NetChannel) + wedges, wired into scripts/run_tla.sh
- [ ] spec text (specs/07-session-tmux-commands.md, specs/08-session-routing.md)
- [ ] `make check` (capture exit status explicitly; RTK may mask it)
