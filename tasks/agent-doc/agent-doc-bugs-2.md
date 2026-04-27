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
