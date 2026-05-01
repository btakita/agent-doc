# Plan — `claudescore-2.md` routed rerun still leaves the completed response uncommitted

## Problem

The May 1, 2026 `src/session-share/tasks/claudescore-2.md` repro did not fully close after the earlier `#6btt` parity fix.

Observed shape:

- JetBrains routed `agent-doc` to the live Codex pane and reported that the command was accepted.
- The route layer then failed closed with `no new document cycle started` for the pending project-plan prompt.
- The response was later recovered manually into `tasks/claudescore-2.md`, but the file remained dirty in the owning `src/session-share` repo until a separate human commit.

That means the live rerun still lacks a binary-owned path from "accepted routed recovery / repaired response visible" to "owning repo commit proof exists". Even if the route/ack layer fails closed correctly, the recovery surface must not strand a completed visible response as an uncommitted worktree mutation.

## Goal

Make the routed-rerun / manual-recovery path for active session documents converge on the same closeout invariant as normal `write --commit` / finalize:

1. the response is visible in the session document
2. the owning repo or submodule has commit proof for that response

If either cannot be proven, the tool must keep the turn resumable and explicit instead of leaving the document in a silently half-complete state.

## Desired behavior

- A routed trigger that is accepted but never starts a new document cycle must leave a precise resumable state, not a document that later needs manual completion plus manual commit.
- If a recovery path writes or adopts the assistant response into the live session document, it must either:
  - commit the owning repo/submodule in the same binary-owned closeout path, or
  - fail closed with an explicit "response visible but uncommitted" diagnostic tied to the active turn.
- `session-check`, `repair`, and any adopt-current / recovery path should recognize this exact shape on `claudescore-2.md` and offer a deterministic one-shot closeout path.

## Investigation steps

- [ ] Reconstruct the full `claudescore-2.md` rerun timeline:
  - route acceptance
  - `no new document cycle started` failure
  - any later repair/adopt/current/manual write path that made the response visible
  - the point where commit proof was lost or never attempted in `src/session-share`
- [ ] Trace which code path owns recovery after a routed accepted/no-ack failure:
  - route / startup-miss bookkeeping
  - resumed closeout / recovery adoption
  - commit-proof verification for submodule-owned session docs
- [ ] Decide whether the fix belongs in:
  - route-time recovery state creation
  - repair/adopt-current closeout
  - commit-proof enforcement after recovery writes
- [ ] Ensure the same invariant holds for submodule-owned docs:
  - visible repaired response in `src/session-share/tasks/*.md`
  - correct owning-root commit proof in `src/session-share`
  - dirty parent `agent-loop` must not block the submodule-owned closeout
- [ ] Add regression coverage for:
  - routed trigger accepted, no new cycle ack, later recovery writes response
  - recovery path commits the owning submodule automatically
  - ambiguous ownership or later human edits fail closed instead of auto-committing

## Exit criteria

- The `claudescore-2.md` routed-rerun reproduction either finishes with a committed `src/session-share` closeout or remains explicitly resumable with a precise diagnostic.
- No recovery path can leave a fresh assistant response visible in the session document without owning-root commit proof.
- The regression is covered for the accepted-route/no-ack failure family on submodule-owned session documents.
