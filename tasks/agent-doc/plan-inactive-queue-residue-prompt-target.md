# Plan: inactive queue residue pollutes later cycles

## Problem

A document can carry `agent:queue` lines even when `queue_active: false`.
Those inactive queue lines are not executable, but they can still influence a
later non-queue response when an agent reads the document as context.

Observed case:

- `tasks/software/tsift.md` had `queue_active: false`
- the queue still contained old graph-performance work items
- a later `#next-steps` prompt asked for SQLite graph performance guidance
- the response treated the stale queue body as the active priority list
- a new inactive queue edit, `do [#gdbpropscan]`, was also classified as a
  prompt-bearing change in the next preflight/plan cycle

The user expectation is that a completed or inactive queue should not keep
feeding future prompt context, and stale queue residue should be cleared or
ignored before it can steer unrelated responses.

## Likely Cause

Queue consumption and drain cleanup are guarded by active queue state. This is
correct for execution, but inactive queue bodies remain visible in the session
document and regular document reads do not distinguish active queue prompts
from stale residue.

The current code already has an explicit regression proving inactive queues are
not consumed by finalize. The missing boundary is separate: inactive queue
content should not be promoted into prompt targets, prompt presets, or
agent-facing priority context unless the queue is activated by `auto`, a start
fence, an exchange trigger, or `queue_active: true`.

## Desired Behavior

Inactive queue bodies should be inert:

1. `preflight` and `plan` should not emit inactive queue line additions as
   prompt targets.
2. `#next-steps` and similar prompt-preset guidance should prefer backlog,
   review, and active queue state, not inactive queue residue.
3. If a queue has fully drained, closeout/preflight should clear directive and
   item residue even for non-auto queue blocks when the binary can prove the
   queue was previously active and is now inactive.
4. If the binary cannot prove safe cleanup, it should surface an explicit
   diagnostic such as `inactive_queue_residue` so the response does not treat
   the queue as active work.

## Implementation Outline

1. Add queue-state metadata to preflight output that separates:
   - active queue prompts
   - inactive queue residue
   - newly edited inactive queue lines
   - drained queue cleanup
2. Narrow prompt-target extraction so diffs inside `agent:queue` are ignored
   unless queue activation resolves true for that cycle.
3. Add stale-residue diagnostics when an inactive queue still contains prompt
   entries or directives after a prior closeout.
4. Extend queue cleanup so a drained queue removes completed/directive residue
   whenever the prior snapshot proves it was active, including non-auto queues.
5. Update agent-facing guidance so context reads and `#next-steps` answers use
   only active queue prompts or explicit backlog/review items.

## Tests

1. Preflight with `queue_active: false` and an edited `agent:queue` line does
   not include that line in `prompt_targets`.
2. Plan output does not require cross-document pending capture for inactive
   queue edits that are unrelated to the active `#agent-doc-bug` prompt.
3. A `#next-steps` prompt in a document with inactive queue residue reports
   backlog/review priority and emits an `inactive_queue_residue` warning rather
   than treating the queue as active.
4. Finalize/preflight clears drained non-auto queue residue when the snapshot
   proves the queue was previously active and all queued ids were completed.
5. Existing inactive-queue safety remains: finalize must not execute or mark
   inactive queue items done merely because they are present in the component.

## Likely Files

- `src/agent-doc/src/preflight.rs`
- `src/agent-doc/src/plan.rs`
- `src/agent-doc/src/diff.rs`
- `src/agent-doc/src/write.rs`
- `src/agent-doc/src/queue.rs`
- `src/agent-doc/tests/finalize_integration.rs`
- `src/agent-doc/tests/run_integration.rs`
