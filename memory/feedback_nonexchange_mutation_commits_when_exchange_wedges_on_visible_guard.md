---
name: feedback_nonexchange_mutation_commits_when_exchange_wedges_on_visible_guard
description: "When exchange finalize wedges on the visible-write guard (buffer≠disk), a non-exchange --backlog-gate/--done write still commits"
metadata: 
  node_type: memory
  type: feedback
  originSessionId: 93ec8215-9c4c-4108-9b28-cd7f0b59e7f0
---

When `finalize`/`write --commit` of an **exchange response** wedges with `visible editor buffer differs from the expected disk state; save or discard` (the `#falsetyping-guard` provenance-blind guard firing on CRDT-replica churn — common while a concurrent session churns `src/agent-doc` and the running binary lacks the fix), a **non-exchange** mutation still commits cleanly: `agent-doc write --commit <FILE> --backlog-gate <id> --force-disk` logs `skipping IPC boundary reposition: commit changed non-exchange tracked surfaces` and lands, because backlog/queue/review components don't need the exchange live-buffer reconcile the guard blocks.

**Why:** the visible-write guard protects the exchange write path (live editor buffer authority). Backlog/queue/review edits bypass that reconcile.

**How to apply:** if the exchange response won't persist but you still need to `--done`/`--backlog-gate`/strike a queue head, run the non-exchange `write --commit` with just the granular flag. Also: the exchange response itself often lands via the NEXT `preflight`'s live-editor-buffer convergence (it committed my stuck closeout as a fresh `agent-doc(...)` commit and collapsed accreted boundaries to one). See [[feedback_forcedisk_committed_but_false_closeout_error_stop_retrying]], [[feedback_stuck_response_captured_cycle_mv_state_cycles_json.md]]. Root fix tracked as `#falsetyping-guard` (provenance-aware guard, implemented on branch `falsetyping-guard-wt`).
