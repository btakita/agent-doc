# Plan — finalize `--pending-add` should populate go-mode backlog queues

## Problem

`agent:backlog priority queue` plus an active `go` queue is supposed to be a
continuous backlog drain. Preflight already covers the case where a new open
backlog id is present before `run_queue_maintenance`: in go mode it appends the
fresh id into `agent:queue` (`#backlog-queue-attr-populates-in-go-mode`).

The missing path is same-cycle backlog capture. `agent-doc finalize` /
`agent-doc write --commit` applies `--pending-add*` mutations during closeout,
after preflight queue maintenance has already run. Those freshly added backlog
items can therefore commit without a matching queue head, even though the
backlog component carries the `queue` attribute and the document is in go-mode.

## Current Evidence

- `agent-doc-orchestration/src/preflight/maintenance.rs` already has
  `run_queue_maintenance_go_mode_appends_fresh_backlog_into_nondrained_queue`.
- `agent-doc-orchestration/src/write.rs` records same-cycle pending ids through
  `cycle_state::record_pending_added_ids`, then runs pending maintenance and
  queue consumption, but does not rerun backlog→queue convergence after the
  pending add.
- A naive full `run_queue_maintenance` before queue consumption would be risky:
  priority sorting or sync mode changes could move the active head before the
  response is consumed.

## Fix Shape

1. Add a closeout-local queue convergence step for same-cycle pending additions.
   Run it after pending mutations are applied and before the commit boundary,
   with ordering chosen so the response cannot consume a newly promoted head.
2. Scope the convergence narrowly:
   - only when the document has an active go/start queue;
   - only when `agent:backlog` has a recognized `queue` attribute;
   - only for ids recorded in `cycle_state.pending_added_ids`;
   - only if the id is still an open backlog item, is not already queued or done,
     and is not `[operator-verify]`.
3. Preserve the existing active queue order. Append same-cycle added ids behind
   current runnable heads instead of applying a full priority reorder during the
   same closeout.
4. Keep the existing non-go amplification guard: plain persisted-active queues
   still hold same-cycle pending adds until a later activation.

## Regression Coverage

- CLI/integration: template doc with `queue: go`,
  `<!-- agent:queue priority go -->`, and
  `<!-- agent:backlog priority queue -->`; run `agent-doc finalize --pending-add`
  while a queue head is active; assert the committed doc contains both the new
  backlog item and a matching queued `do [#id]`, and `session-check` passes.
- Non-go control: the same pending add under `queue_active: true` without go/start
  does not append a queue head.
- Consumption safety: completing the current queue head and adding a new backlog
  id in one finalize consumes/strikes only the original head; the new id remains
  queued for a later cycle.
- Guard safety: the same-cycle queued id does not expand
  `expect_done_or_gate_ids` or trip `pending_done_guard`.

## Acceptance

- Same-cycle `--pending-add` work is not stranded outside the active go-mode
  queue on documents that opt into `agent:backlog ... queue`.
- The previous amplification protections still hold for non-go queues.
- `make check` passes before install/push.
