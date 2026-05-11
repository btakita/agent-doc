# Plan — route-owned pane reaped after successful finalize commit

**Pending:** `#routereap`
**Status:** Draft

## Problem

When a pane is started via `agent-doc start --route-owned`, the supervisor's
completion thread polls cycle state every 500ms. As soon as it detects a
committed cycle (phase=Committed, new cycle ID), it sets
`route_owned_completion`, kills the child process, and breaks the supervisor
loop. The supervisor then executes `kill-pane -t <pane_id>` at exit.

This is correct for true one-shot dispatches (e.g., a route that injects a
single prompt and expects no further interaction). But for multi-turn session
documents — like `tasks/root.md` with an active backlog and ongoing exchange —
the pane should survive after finalize commits so the user can continue editing
the document and triggering new cycles.

Evidence: `src/session-share/tasks/root.md` pane was reaped after finalize
wrote and committed the VT admin overlay review patchback. The user expected
to continue interacting with the session.

## Root Cause

`start.rs` line ~537-550: `route_owned_cycle_completed_after_start` returns
true as soon as *any* new committed cycle is detected, regardless of whether
the document has pending prompts, backlog items, or an active exchange tail
that signals continued use.

The reaping decision is unconditional on session liveness — it only checks
"did a cycle commit since start?" not "is the session done?"

## Desired Behavior

1. Route-owned panes should NOT be reaped after a committed cycle if the
   session document signals continued interaction:
   - Active backlog items exist
   - Queue is non-empty
   - Exchange tail contains unresolved prompts
   - User edits are pending (document dirty after commit)
2. Route-owned panes SHOULD still be reaped after commit for true one-shot
   dispatches where no further interaction is expected (e.g., `claim`,
   `compact`, single-prompt-and-done patterns).
3. The decision boundary should be auditable in ops.log.

## Implementation Steps

- [ ] Add a post-commit session-liveness check in the route-owned completion
  thread (`start.rs` around line 537-550): after detecting a committed cycle,
  inspect the document for continued-interaction signals before setting
  `route_owned_completion`:
  - parse `agent:backlog` for non-empty items
  - parse `agent:queue` for queued prompts
  - check for uncommitted document changes (dirty working tree)
  - check for unresolved exchange tail prompts
- [ ] Define a `route_owned_reap_policy` enum: `ReapAfterCommit` (current
  one-shot behavior) vs `KeepAlive` (multi-turn session). Default to
  `KeepAlive` when any liveness signal is present.
- [ ] When `KeepAlive` is chosen, the completion thread should NOT set
  `route_owned_completion` and should NOT kill the child. Instead, log
  `route_owned_cycle_committed_keep_alive` and continue polling for the
  next cycle or supervisor shutdown.
- [ ] Add `--route-owned-reap-policy` flag to `agent-doc start` so route
  can explicitly request one-shot reaping when it knows the dispatch is
  terminal.
- [ ] Log the reap decision with provenance: `route_owned_reap_decision
  policy=keep_alive reason=backlog_non_empty` or `policy=reap
  reason=one_shot_dispatch`.
- [ ] Add regressions:
  - route-owned pane with non-empty backlog survives after committed cycle
  - route-owned pane with empty backlog + empty queue + no pending edits
    is reaped after committed cycle
  - route-owned pane with queued prompts survives
  - explicit `--route-owned-reap-policy reap` overrides liveness check

## Files Likely To Change

- `src/agent-doc/src/start.rs` — completion thread, reaping logic
- `src/agent-doc/src/route.rs` — pass reap policy to start
- `src/agent-doc/src/cycle_state.rs` — liveness check helper
- `src/agent-doc/src/component.rs` — backlog/queue emptiness check

## Related

- `#panehang` — hung panes that are NOT reaped (opposite problem)
- `#busychk` — pane busy-check before sync eviction
- plan-active-prompt-pane-reap.md — sync/rebind reaping active panes
- plan-monsterrodholders-previous-pane-reap.md — reroute closing live panes

## Exit Criteria

- Route-owned pane for `tasks/root.md` (non-empty backlog) survives after
  finalize commit.
- One-shot route dispatches still reap correctly.
- Ops log shows clear reap-decision provenance.
