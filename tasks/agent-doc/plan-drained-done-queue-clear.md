# Plan: Clear Fully Resolved Queues

## Bug

When every prompt in an active `agent:queue auto` is completed via `--done`, the queue can remain in the document after closeout. In the live Corky session, all four queued IDs were reaped into `agent:done`, but `tasks/software/corky.md` still retained:

- the batch dispatch directive
- all four completed `do [#id]` queue lines
- an executable-looking `agent:queue auto` block until the next preflight pass

A follow-up `agent-doc preflight tasks/software/corky.md` auto-struck the already-done prompts and cleared `queue_active`, but halted as `item_modified` before the normal drain path could empty the queue body. The pure queue-maintenance mutation also needed an explicit `agent-doc commit` repair.

## Expected Behavior

After all queue prompts are resolved by `--done` / `agent:done` / review-gated IDs:

- `agent:queue` body is empty
- `queue_active: false` is persisted when present
- `auto` is stripped from the queue opening tag
- the snapshot and HEAD agree in the same binary-owned closeout
- `session-check` passes without requiring manual `agent-doc commit`

## Likely Cause

The preflight queue-maintenance order in `agent-doc-orchestration/src/preflight.rs` is wrong for the all-done case:

1. `strike_done_queue_head_prompts` rewrites the current queue entries.
2. Halt detection compares the rewritten current head against the old snapshot head.
3. That comparison can classify the already-resolved head as `item_modified`.
4. The function returns early from the halt path before the drained-queue cleanup path can clear directive/completed residue.

The finalize path has regression coverage for some `--done` queue-drain cases, but this live repro shows a missing coverage shape: template session, batch dispatch directive, bracketed `do [#id]` prompts, response finalized with multiple `--done` IDs, and a later no-diff preflight maintenance pass.

## Fix Plan

1. Add a regression covering the Corky shape.
   - Use a template document with `queue_active: true`.
   - Include `<!-- agent:queue auto -->`, a `dispatch #preset` directive, and multiple `do [#id]` prompts.
   - Mark all IDs done, then assert the queue body is empty and `queue_active: false` is committed.
   - Add a separate no-diff preflight regression where all queue IDs are already in `agent:done`; assert it drains instead of halting as `item_modified`.

2. Adjust preflight queue maintenance so resolved-head striking cannot trigger `item_modified`.
   - After auto-striking done/gated head prompts, check whether the remaining queue has no executable prompts and only drained residue.
   - Run the drained-queue cleanup before head-modification halt detection, or recompute the snapshot comparison against the struck-equivalent snapshot entries.

3. Make pure queue maintenance cross the commit boundary.
   - If preflight mutates only queue state/body and there is no response body, the binary must still leave snapshot and HEAD consistent.
   - Avoid a recovery path that recommends `agent-doc write --commit` when there is no response to write.

4. Extend session-check coverage.
   - Assert that completed queue residue (`dispatch` plus all-struck `do` lines) in an inactive queue is either auto-repaired or reported with an actionable queue-maintenance repair command.

## Verification

- `cargo test -p agent-doc-orchestration queue`
- targeted finalize/preflight integration tests for the repro
- `make check`
- live replay against `tasks/software/corky.md` or an isolated copy

