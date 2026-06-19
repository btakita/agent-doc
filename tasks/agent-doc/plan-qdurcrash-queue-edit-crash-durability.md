# Plan: #qdurcrash — queue-edit durability across a supervisor/pane crash+restart

## Problem

Operator adds a queue item, the turn starts, the supervisor + tmux pane crashes
and restarts, and the operator-added queue item is **GONE** (virtuousai.md repro,
same crash event as `#routedeadshell`/`#1vhn`).

Root-cause hypothesis: the operator queue add lived only in the editor buffer /
supervisor in-memory CRDT and was never journaled to a crash-durable on-disk
store before the process died, so restart reloaded from the last snapshot/HEAD
(predating the add) and dropped it.

Distinct from `#semmerge` (live-merge drop during a turn) and `#routedeadshell`
(route dispatch into a dead shell).

## ⚠️ Risk gate — intersects the carry-forward invariant (`#fintol2`/`#pcwc`)

The fix's second half — a **supervisor-startup reconcile that merges the on-disk
document queue against the reloaded snapshot** — is a forward-merge / worktree
auto-repair operation in the SAME area that previously broke **7 shipped tests**
(`#fintol2`/`#pcwc`: a post-commit worktree legitimately differs from HEAD via
carried-forward uncommitted edits). That work was reverted to seam-isolated
groundwork only and is flagged **"do not re-wire without operator go + a live
Phase-0 re-baseline."**

Therefore the reconcile half of `#qdurcrash` is **operator-design-gated**, not
auto-drainable. This plan scopes the design; implementation of Phase 2 needs an
explicit operator go + Phase-0 re-baseline.

## Design

### Phase 1 (foundational, lower-risk): journal inbound operator queue patches before ack

When an inbound operator queue patch / buffer save reaches the binary (IPC
listener in `ffi.rs` / `ipc_socket.rs`, or the `write` path), persist the
operator's queue mutation to a **crash-durable on-disk journal**
(`.agent-doc/queue-journal/<doc-hash>.jsonl`, append-only, fsync) **before** the
supervisor mutates in-memory queue state and **before** the ack is returned. The
journal records: doc hash, baseline/snapshot hash at receipt, the queue-component
delta (added/edited/struck item text + id), and a monotonic receipt seq.

This is comparatively isolated (a new append-only sidecar on the receive path)
and does not touch the reconcile/carry-forward logic, so it can ship + be tested
independently. On clean closeout the journal entries whose mutation is now
reflected in the committed snapshot are pruned.

Tests: journal append on inbound queue patch; journal survives a simulated
process exit (write journal, drop the in-memory state, re-read journal); pruning
on clean closeout.

### Phase 2 (operator-design-gated): supervisor-startup reconcile

On supervisor (re)start, before trusting the reloaded snapshot for queue content,
replay the journal: for each journaled queue mutation whose receipt baseline is
consistent with the current document, re-apply it to the queue component so a
crash+restart **replays the pending queue edit instead of losing it**. Must
preserve the carry-forward invariant — a post-commit worktree that legitimately
differs from HEAD via carried-forward edits must NOT be clobbered, and the
on-disk editor edit is the source of truth.

This is the part that re-enters the `#fintol2`/`#pcwc` carry-forward area. It
requires: operator go, a live Phase-0 re-baseline of the 7 previously-broken
tests, and crash-simulation coverage (`kill -9` the supervisor mid-turn,
restart, confirm the item survives).

## Verify

Add a queue item, `kill -9` the supervisor mid-turn, restart, confirm the item
survives. Deterministic SimWorld coverage of the journal-replay path where live
`kill -9` cannot be modeled.

## Status — IMPLEMENTED (additive)

Both phases shipped as an **additive, replay-only** mechanism that sidesteps the
carry-forward risk entirely: it only ever *re-adds* operator queue prompts that
were observed-then-lost, and never removes or rewrites existing content, so it
does NOT re-wire the post-commit worktree reconcile the `#fintol2`/`#pcwc`
invariant guards. Full `make check` (including those tests) stays green.

- **Phase 1 — journal (`queue_journal::record`)**: preflight queue maintenance
  durably appends each observed operator queue prompt to
  `.agent-doc/queue-journal/<doc-hash>.jsonl` (append-only + fsync).
- **Phase 2 — startup replay (`queue_journal::replay_missing` +
  `merge_missing_into_content`)**: `run_with_reap_policy` re-inserts journaled
  prompts absent from the reloaded document. A struck/consumed prompt counts as
  present and is never resurrected.
- **Prune (`queue_journal::clear`)**: every `commit_success` empties the journal
  (queue state is now durable in the snapshot), bounding the journal — and any
  replay — to operator additions observed since the last commit.

### Known gap (follow-up, plugin-side)

An add lost to a crash *before any cycle observes it* (pure editor-buffer, never
flushed to disk / never seen by the binary) cannot be journaled binary-side — it
needs a JB/VS Code plugin **buffer-flush-on-edit** (write the operator's queue
edit to the on-disk document or a sidecar immediately, so the binary observes it
on the next cycle / a restart can replay it). Operator-install/live-verify gated.
