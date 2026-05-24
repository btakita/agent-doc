# Plan: drained non-auto queue residue cleanup

## Problem

After a queue-backed run finishes, the session document can still retain old
`agent:queue` directives and item lines while `queue_active: false`.

Observed case:

- `tasks/software/tsift.md` had `queue_active: false`
- the queue still contained the previous graph-performance batch
- a later non-queue prompt was answered with that old queue as if it were the
  active priority list

The stale lines should not have survived as active-looking work after the run
that drained or deactivated the queue.

## Desired Behavior

When a queue reaches an inactive/drained state, the binary should remove
directive and item residue from the `agent:queue` component when it can prove
the lines belong to the completed queue run.

If safe cleanup cannot be proven, preflight should leave the body intact but
emit an explicit `inactive_queue_residue` warning so agents and users do not
treat it as the active queue.

## Implementation Outline

1. Extend queue drain cleanup beyond `agent:queue auto`.
2. Compare the current file and snapshot to prove the queue was active before
   clearing non-auto directive/item residue.
3. Keep manual inactive queues inert: do not consume or mark queue items done
   merely because they exist.
4. Emit a warning when inactive queue entries remain and cleanup is unsafe.
5. Document that active queue state is the only source of executable queue
   priority.

## Tests

1. A non-auto queue with `queue_active: true` drains after the last completed
   item and leaves an empty `agent:queue` body.
2. A non-auto queue directive such as `preset #...` or `dispatch #...` is
   removed when all associated items are consumed.
3. A document with `queue_active: false` and manually-authored inactive queue
   lines is not executed or consumed.
4. A document with unsafe inactive residue emits `inactive_queue_residue`.
5. Existing auto-queue drain tests continue to pass.

## Likely Files

- `src/agent-doc/src/write.rs`
- `src/agent-doc/src/preflight.rs`
- `src/agent-doc/src/queue.rs`
- `src/agent-doc/tests/finalize_integration.rs`
- `src/agent-doc/tests/run_integration.rs`
