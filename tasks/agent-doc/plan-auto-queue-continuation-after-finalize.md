# Plan: Auto queue stops after first finalized item

## Bug

An `agent:queue auto` run can stop after the first completed item even though
the queue remains active and still contains prompt entries.

Observed case:

- `tasks/software/tsift.md` had `queue_active: true`
- the queue contained `do [#gval]`, `do [#gobs]`, and `do [#gfed]`
- the first item finalized successfully and was marked `~do [#gval]~`
- `#gobs` and `#gfed` stayed in the queue and live backlog
- no next `agent-doc` cycle was launched automatically

This is surprising because the document still advertises an active auto queue,
but the harness-visible run behaved like a single item execution.

## Likely Cause

Queue maintenance in `preflight` correctly preserves `queue_active` and
synthesizes the current queue head as a prompt when a later `agent-doc <FILE>`
invocation has no document diff.

The missing piece is after successful closeout. The direct run/finalize path
does not have a parent-owned continuation loop that re-enters preflight for the
next queued prompt. Once the current item reaches `finalize` and
`session-check`, the process exits, leaving remaining queue prompts for a
future manual invocation.

That creates a mismatch between queue state and operator expectation:
`agent:queue auto` looks like a batch runner, but only one item is consumed per
explicit invocation.

## Desired Behavior

For `agent:queue auto` with `queue_active: true`, a direct `agent-doc <FILE>`
cycle should either:

1. continue to the next prompt after each successful finalized item until the
   queue drains, hits a stop/time gate, halts on item mutation, or a step fails;
   or
2. explicitly document and surface that auto queues are single-step resumable,
   including a clear closeout message that remaining items require another
   `agent-doc <FILE>` invocation.

Prefer option 1 unless it conflicts with harness limits. It matches the visible
`auto` contract and avoids silent partial queue execution.

## Implementation Outline

1. Audit the direct invocation lifecycle in `src/agent-doc/src/run.rs`,
   `src/agent-doc/src/flow/closeout.rs`, and queue maintenance in
   `src/agent-doc/src/preflight.rs`.
2. Add a queue-continuation coordinator after successful finalize/session-check:
   - inspect the updated document and snapshot after the current item closes
   - if `queue_active: true` and the queue has a remaining prompt head, start
     the next binary-owned cycle from a fresh preflight
   - preserve the normal per-item finalize/write+commit boundary
   - stop deterministically on stop fence, time gate, item mutation,
     verification failure, or closeout/session-check interruption
3. Keep manual repo commits and document finalization ordered per existing
   closeout rules: each queued item owns its own implementation and finalize
   boundary before the next item begins.
4. Add user-visible diagnostics when the queue stops before draining, naming
   the completed item count, next remaining prompt, and stop reason.

## Tests

- Unit: after one prompt is marked complete, queue parsing still identifies the
  next non-completed prompt as the head while preserving completed residue.
- Integration: direct `agent-doc <FILE>` with `agent:queue auto` and three
  `do #id` items runs three fresh preflight/finalize cycles and leaves the queue
  drained or non-auto with all three items done.
- Integration: if item 2 fails verification or `session-check`, item 1 remains
  committed, item 2 remains open or gated according to the failure, item 3 is
  not launched, and the response records the interruption.
- Integration: a stop fence or future time gate at the next head stops the
  continuation loop without consuming later prompts.
- Regression: a later explicit `agent-doc <FILE>` invocation still resumes an
  active single-step queue when continuation was intentionally interrupted.

## Likely Files

- `src/agent-doc/src/run.rs`
- `src/agent-doc/src/flow/closeout.rs`
- `src/agent-doc/src/preflight.rs`
- `src/agent-doc/src/queue.rs`
- `src/agent-doc/src/plan.rs`
- `src/agent-doc/tests/run_integration.rs`
- `src/agent-doc/specs/07-orchestration-commands.md`
