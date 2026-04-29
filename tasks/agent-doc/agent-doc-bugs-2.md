# Plan - report-back must land in the active session document

Source: `src/session-share/tasks/claudescore-3.md`

- [ ] Reproduce the exact `claudescore-3.md` failure chain:
  1. the agent promises "I’ll report back" in the active template/CRDT session document
  2. the next substantive update is produced outside that document
  3. a later captured response includes a direct backlog/pending patch
  4. the write path rejects it with `replace:pending block forbidden`
  5. the cycle is left in `response_captured` until recovery
- [ ] Enforce the document-first reporting invariant: when the active surface is a session document, promised follow-up updates must be appended to that document before or alongside any external reply.
- [ ] Fix the write/repair path so captured backlog mutations are converted into supported pending operations, or are rejected earlier with an explicit resumable failure instead of orphaning the cycle after response capture.
- [ ] Add deterministic recovery for the interrupted `response_captured` case created by forbidden pending/backlog patches.
- [ ] Add regression coverage for both halves of the bug: missed in-document report-back and forbidden pending patch during response write-back.
- [ ] Verify the fix against template/CRDT documents and document the expected operator-facing recovery logs and `session-check` outcome.

## Backlog

- [ ] Fix report-back patchback failure for CRDT template docs when the captured response includes pending/backlog mutations.
- [ ] Enforce document-first reporting when the agent promises a follow-up in the active session document.

<!-- agent:exchange -->
### Re: `#mcrc` response patchback left uncommitted — gpt-5

This is a real regression against the already-shipped response-commit invariant.

The `#asch1` reproduction in `src/boost-client/tasks/monsterrodholders.md` proves a visible response patchback can still survive as plain working-tree dirt after the repair/write phase. That means at least one response-closeout path is still able to stop after the document mutation without proving the owning-root commit boundary.

I captured the implementation plan in `tasks/agent-doc/plan-response-patchback-uncommitted-regression.md`. The work splits into four concrete checks:

1. Reconstruct which closeout path produced the dirty patchback: normal `finalize`, manual `write --commit`, stop-hook replay, or another recovery path.
2. Make every response persistence path share one success invariant: a fresh visible `### Re:` patchback must either have owning-root commit proof or fail closed with resumable diagnostics.
3. Tighten `session-check` / exit propagation so a detected `patchback visible but uncommitted` state blocks success instead of being downgraded or ignored.
4. Add regression coverage for submodule-owned docs, manual repair, hook/backstop replay, and post-commit re-dirtying.

No new backlog item is needed in this patchback because the issue is already captured here. This turn is planning-only; no `src/agent-doc` code change or test run happened in the repair itself.
<!-- agent:boundary:f31d235e -->
<!-- /agent:exchange -->
