# Plan: queue-sync × pending_done_guard misfire (#queue-sync-auto-pending-done-guard-misfire)

## Problem

When a backlog/icebox `queue` sync attribute (`#backlog-queue-sync-attr`,
`sync`/`append`/`prepend`) populates an **active `auto` queue in the same
cycle**, the freshly-synced `do [#id]` lines appear as new content in the cycle
diff. Preflight's `expect_done_or_gate_ids` is derived from
`do_directive_target_ids(&prompt_targets)`, and the active auto-queue contributes
those synced heads as prompt targets — so every synced item becomes a cycle
`do`-directive. `session_check`'s `pending_done_guard` then fails closed because
those items are still open in `agent:backlog` with no `--done`/`--pending-gate`.

### Live repro (2026-05-31, `tasks/agent-doc/agent-doc-bugs2.md`)

- Backlog marker `<!-- agent:backlog priority queue -->`, queue marker
  `<!-- agent:queue priority auto -->`.
- The directive `do [#backlog-queue-sync-attr]` implemented the queue-sync
  feature and `--done`'d its item.
- The same cycle's queue sync appended `do [#id]` for the other 9 active backlog
  items; `pending_done_guard` then demanded all 9 be resolved this cycle.
- Worked around with the per-cycle `<!-- no-pending-done-guard -->` suppression;
  the 9 items are correctly deferred to the auto-loop.

## Root cause

Agent-generated / freshly-synced queue items are indistinguishable from genuine
user `do [#id]` directives once they land in the cycle diff / active-queue
prompt-target set. `expect_done_or_gate_ids` should reflect only **operator
intent for this cycle**, not the queue maintenance the binary performed.

## Proposed fix

1. **Provenance for synced queue items.** When `run_queue_maintenance` syncs the
   backlog→queue, mark those entries as agent-generated for the cycle (e.g. a
   per-cycle set of synced ids in `cycle_state`, or exclude the sync mutation
   from the diff that feeds `prompt_targets`).
2. **Narrow `expect_done_or_gate_ids`.** Populate it only from (a) genuine user
   exchange `do [#id]` directives and (b) the single consumed queue head — never
   the full freshly-synced auto-queue.
3. **Regression coverage.** A deterministic test: a `do [#x]` exchange directive
   in a cycle that also syncs N backlog items into an `auto` queue must record
   only `x` (and/or the consumed head) in `expect_done_or_gate_ids`, so
   `pending_done_guard` does not demand the synced items be done.

## Notes

- The per-cycle `<!-- no-pending-done-guard -->` escape is the correct stopgap
  for the populating cycle; this plan removes the need for it.
- Relates to `#backlog-queue-sync-attr` (the sync feature) and
  `#backlog-priority-attribute` (priority ordering), both shipped 2026-05-31.
