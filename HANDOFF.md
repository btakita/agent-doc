# HANDOFF — GH #136 (btakita/agent-doc#136)

Branch `gh136`, worktree `/home/brian/work/btakita/agent-doc-wt/gh136` (from `main` @ v0.35.456).
Do NOT push, `make install`, release, merge, or close the issue. Integrator deletes this file at merge.

## Goal (with all operator steering)

A stale stash-window supervisor pane (`%41`, `supervisor=stale evidence=binary_replaced`) was admitted
as a layout column (`layout_column_pane_stale_focus_admitted ... admission=admitted_focused_document_sole_owner
action=safe_boundary_recycle_requested prior_request=unconsumed ... age_secs` up to 80,071), editor_surface
published `columns = observed_panes + 1` (3-pane flap), slow focus settle.

Steering received:
1. Diagnose AND fix; deterministic regression coverage (SimWorld-style worlds preferred over mocks); SPEC text; full suite.
2. Formal: explicit state machine of the stale-admission / recycle-request lifecycle with invariants, exhaustive
   transition-table test in Rust and/or TLA+ in `formal/tla/` with Reach/Wedge cfgs.
3. Coder remote workspace + JetBrains Remote Dev + Zscaler: correctness must not depend on timing; event/ack-driven;
   remove serial round trips / fixed waits in focus path; any timeout configurable and never in a safety argument.
4. Adversarial channel (delay, reorder, drop, duplicate, reconnect) editor<->controller<->tmux; safety under it,
   liveness only under fairness; Wedge cfg showing pre-fix violates. Keep channel swappable for a future NetChannel.tla.
5. Lost notifications: request must be durable level-triggered state + idempotent consumer keyed by epoch; model Drop;
   Wedge showing fire-and-forget stalls; half-open connections without safety depending on heartbeat timing.

## Root cause (findings)

- Request channel is ALREADY durable + level-triggered (state.db `SupervisorRecycleRequested` epochs; supervisor
  idle watch re-reads via `live_recycle_request` every tick: `agent-doc-start-runtime-io/src/idle_watch.rs:3610`;
  consumer settles only the observed epoch: `agent-doc-supervisor-io/src/recycle_request.rs` `clear_recycle_request_inner`).
  So not a lost one-shot message. BUT:
  a. `recycle_request_is_live` (`agent-doc-supervisor/src/recycle_request.rs`) only kept `stale_supervisor_turn_stage`
     alive past the 900s TTL. An install fan-out (`recycle_supervisors_all_projects_force`, rpc.rs ~11835) writes a
     NEWER epoch with reason `install_fanout`, replacing the projection's reason; it lapses after TTL, so the stale
     supervisor stops seeing any live request. 5/7 `prior_request=unconsumed` in the issue are `install_fanout`. FIXED.
  b. The consumer IS the stale process running OLD bytes, and it defers on an open cycle (`supervisor_recycle_action`
     -> `DeferCycleOpen`, `agent-doc-supervisor/src/lifecycle.rs:395`). A requester-side fix is therefore required:
     admission must not depend on consumption. FIXED via bounded focus exception.
  c. The gate (`agent-doc-sync-io/src/layout_column_audit.rs` `plan_column_admissions`) admitted a stale STASH pane
     for the focused doc even when that realised more columns than the window held -> `columns = observed + 1`. FIXED.
  d. A gated-out column was never acknowledged to the controller's projection worker, which compares against desired
     columns -> `retry_pending` loop (250ms..5s backoff, unbounded attempts). FIXED: effect receipt carries
     `gated_documents`; worker converges against `columns - gated` and drops focus on a gated doc.

## Done

- `agent-doc-supervisor/src/recycle_request.rs`: `recycle_reason_is_binary_replacement`, fan-out reasons live while
  stale; `StaleRecycleRequestState {NotRequested, Pending, Overdue}`, `OutstandingRecycleRequest`,
  `classify_stale_recycle_request`, `STALE_RECYCLE_CONSUME_BOUND_SECS` (=120, env
  `AGENT_DOC_STALE_RECYCLE_CONSUME_BOUND_SECS`), tests.
- `agent-doc-supervisor-io/src/recycle_request.rs`: `read_outstanding_recycle_request` (first-unconsumed time over
  epochs above the highest consumed epoch; refreshes never reset the clock), test.
- `agent-doc-sync-io/src/layout_column_audit.rs`: `stale_focus_admission` (pure transition fn), new
  `ColumnAdmission::{ExcludeStaleFocusedRecycleOverdue, ExcludeStaleFocusedWouldWiden}`, facts `in_stash`/`recycle`,
  gate inputs `window_pane_count`/`pane_turn_active`, gate returns `StaleColumnGateOutcome{col_args, excluded}`,
  `prior_request=` token now `none|pending:...|overdue:...`; tests: exhaustive transition table (240 cases),
  Gh136World SimWorld (widen, overdue, turn-deferral, visible-no-move), time-evolution lifecycle test.
- `agent-doc-sync-io/src/sync.rs`: wires new gate inputs; `SyncRunReport.gated_documents`.
- `agent-doc-controller-io`: `ControllerTmuxLayoutSyncReceipt.gated_documents` (serde default); worker uses
  `layout_columns_without_gated_documents` + `focus_outside_gated_documents` (rpc.rs); `src/main.rs` plumbs it.
- `formal/tla/StaleColumnRecycle.tla` + 8 cfgs, registered in `scripts/run_tla.sh`, README section. Verified locally:
  main cfg 176,435 distinct states no error; all 5 wedges + 2 reach violate as required.

## Remaining

1. Unit tests in rpc.rs for `layout_columns_without_gated_documents` / `focus_outside_gated_documents`.
2. SPEC.md text for GH #136 (bounded focus exception, gated acknowledgement, remote/Zscaler design point, env knob).
3. `cargo clippy` + `make check` (capture `$?`, grep `FAILED`); concurrent `make test` collisions are not reds.
4. Final commit referencing GH #136; update this file.
5. Not addressed (out of scope / report as unresolved): editor_surface publishing 3 columns comes from the editor's own
   observation (`SurfaceIntent::Sync` uses `surface.columns`); the "60s focus settle" root cause was not isolated; the
   generation-reset `superseded` race (issue section "1061.md focus is a collapse"); old stale supervisors running
   pre-fix bytes still defer on an open cycle (only the requester-side bound protects the layout from them).

## Test commands / status

- `cargo test -p agent-doc-sync-io --lib layout_column_audit` -> 23 passed
- `cargo test -p agent-doc-supervisor --lib recycle_request` -> 5 passed
- `cargo test -p agent-doc-supervisor-io --lib recycle_request` -> passing (8 incl. new)
- TLA: `make tla` (or see command loop in this file's history); target/tla jar downloaded with pinned sha.
- Known failing: none known yet; full `make check` not yet run.
