# Plan: stop duplicate patchback two-commit cleanup churn

## Problem

A response closeout can still take the slow failure path where:

1. the assistant response patchback is written and committed,
2. the same patchback is duplicated in that commit,
3. a later dedupe/repair commit removes the duplicate, and
4. the agent spends additional tool cycles reconciling snapshot, parent
   submodule pointer, and session-check state after the substantive response is
   already visible.

The final document may end clean, but the closeout is operationally wrong:
duplicate response content reaches git history, and the recovery path burns time
and tokens after the response patchback should have been the terminal boundary.

## Evidence

- `src/session-share/tasks/root.md` closeout on 2026-05-10 around the Vercel log
  next-steps response.
- The closeout produced a duplicate `### Re: Vercel log next steps` response in
  one commit, then removed it in a later repair commit.
- `session-check` and `agent-doc dedupe` eventually repaired the live document,
  but only after extra write, dedupe, manual commit, snapshot repair, and parent
  pointer churn.

## Desired Behavior

- A response patchback is committed at most once.
- Adjacent duplicate response blocks are rejected or deduped before the first
  closeout commit, never in a follow-up cleanup commit.
- After `finalize` / `write --commit` returns, only `session-check` and final
  reporting should remain; any extra repair work means the command did not
  actually finish the closeout.
- Snapshot and parent-submodule pointer state are updated atomically with the
  deduped final document.

## Investigation Steps

- [ ] Reconstruct the exact closeout sequence from the `tasks/root.md` commits
      and `.agent-doc/captures/.../cycle-1778468208438.json`.
- [ ] Identify why duplicate-response detection ran after the duplicate commit
      rather than before commit success.
- [ ] Inspect the interaction between `write --commit`, forced disk writes,
      IPC listener presence, `dedupe`, snapshot hash updates, and parent
      submodule pointer commits.
- [ ] Decide whether this is a new guard-ordering bug or a recurrence of
      `#duppb` with an additional post-commit repair/churn phase.

## Implementation Plan

- [ ] Move adjacent duplicate response detection to the pre-commit path for the
      active response body, including forced-disk and fallback write modes.
- [ ] Make duplicate detection operate on the exact final document that will be
      hashed, snapshotted, and committed.
- [ ] If a duplicate is detected after any write mutation, dedupe before commit
      and update the snapshot/capture from the deduped content in the same
      transaction.
- [ ] Treat post-commit duplicate detection as a hard invariant failure with a
      single precise recovery command, not as a normal path that requires
      several agent-driven repair attempts.
- [ ] Ensure parent submodule pointer updates happen only after the owning
      submodule commit and snapshot agree.

## Regression Coverage

- [ ] Add a test for a write path that would append the same response block
      twice and assert only one commit is created for the closeout.
- [ ] Add a submodule-owned session-document test where dedupe, snapshot hash,
      and parent pointer are all verified after the same closeout.
- [ ] Add a SimWorld trace for "duplicate visible before commit" and
      "duplicate detected after mutation but before commit".
- [ ] Add an assertion that the successful closeout path does not require a
      follow-up `agent-doc dedupe` or manual `git commit`.

## Exit Criteria

- The duplicated-patchback/two-commit cleanup shape cannot be reproduced.
- A successful closeout leaves one response block, one owning closeout commit,
  matching snapshot/capture hashes, and an updated parent pointer when needed.
- The agent does not continue through repair churn after a visible response
  patchback unless the closeout command failed with a concrete recovery command.
