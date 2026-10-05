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

- [x] (e) `5eb641072` — `pane_outside_target_window` (snapshot carries `#{window_id}`); gate field renamed
  `in_stash` -> `outside_target_window`. Tests: `gh136e_*` in agent-doc-sync-io layout_column_audit.
- [x] (a) `6c499bcea` — pure `agent_doc_controller::pane_layout::bound_layout_width` applied in
  `publish_pane_layout_desired_invocation` (rpc.rs) via `bound_pane_layout_publication_width`.
  Derived bound = `max(min(retained, observed + gated), asserted, 1)` using only the observation/gated set of
  the RETAINED generation's own pass (`PaneLayoutWidthMemory` in project_controller.rs). Logs
  `pane_layout_publication_width_bounded` / `pane_layout_publication_widened`. Tests: `gh136a_*` (pure,
  exhaustive) + controller `gh136a_*`.
- [x] (d) `6c499bcea` — `handle_supervisor_recycle_settled` -> `republish_pane_layout_after_recycle`
  (publisher `recycle_settled`, FreshIntent, once per gated pass; pure decision
  `recycle_settle_republishes_layout`). Tests: `gh136d_*`.
- [x] (b) generation-reset collapse `8501c3a07` — ensure route on an empty (successor) graph merges over a
  positive live tmux observation (`ensure_route_merge_basis`, `observe_live_layout_documents`). Regression
  `gh136b_a_route_replayed_onto_a_successor_never_collapses_the_live_layout` (SimNet coder_zscaler,
  at-least-once, 48 seeds; mutation-checked).
  60s ceiling: NO 60s timer exists on the controller layout path. The only 60s ceiling is the JetBrains
  plugin's per-request socket timeout (`CpRouteClient.kt:152 SOCKET_REQUEST_TIMEOUT_MS = 60_000L`) on the
  SINGLE-THREAD surface delivery lane (`EditorTabSyncListener.kt` `surfaceDeliveryExecutor`): one request
  the controller never answers (a controller mid-handoff/wedged) blocks every later observation until the
  60s timeout + retry reaches the live controller. Also: generation numbers restart at 1 per controller,
  so log pairing across a handoff fabricates long pairs. NOT fixed here (Kotlin lane; overlaps netadv5's
  timeout work): proposed fix = latest-wins supersede of a stalled in-flight surface request (the controller
  already rejects stale `(client_id, generation, sequence)`), not a shorter timeout.
- [ ] (c) idle-boundary replacement + tests
- [ ] TLA model `LayoutWidthBound` (NetChannel) + wedges, wired into scripts/run_tla.sh
- [ ] spec text (specs/07-session-tmux-commands.md, specs/08-session-routing.md)
- [ ] `make check` (capture exit status explicitly; RTK may mask it)

## Overlap notes
- rpc.rs `publish_pane_layout_desired_invocation`, effect worker (records gated docs), `handle_editor_route_rpc`
  (merge basis), `escalate_focus_to_structural_layout` (claim), `handle_supervisor_recycle_settled` (hook).
  netadv5 R7 (layout sync lock-skip) lives in `src/ffi.rs` / sync lock — not touched here.
- Flake seen once under the parallel lib run: `reliable_sync_status_projects_plane_open_set_without_sidecar_oracle`
  (passes alone 3/3; unrelated to these changes).
