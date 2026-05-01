# Plan: claudescore-3 turn-complete drift

## Problem

`agent-doc` can finish the substantive work for `src/session-share/tasks/claudescore-3.md`, create/push the expected `session-share` commits, and then still leave the live session document dirty in the worktree after `agent-doc commit`.

That means the user-visible turn is not actually complete even though the tool has already reported success.

## Repro shape

1. Route a turn against `src/session-share/tasks/claudescore-3.md`.
2. Let the turn complete normal repo work, including `session-share` commits/pushes.
3. Run `agent-doc commit tasks/claudescore-3.md`.
4. Observe that `git status --short` in `src/session-share` still shows `M tasks/claudescore-3.md`.

## Expected behavior

- snapshot commit succeeds
- repo commit succeeds
- live file is clean in the owning repo worktree
- no extra manual commit is required to finish the same turn

## Investigation targets

1. Compare snapshot-side boundary repositioning with the live-file CRDT/buffer state for template documents.
2. Trace why `agent-doc commit` can produce a new session-share commit while leaving the visible file modified afterward.
3. Check whether the live-file mutation happens before or after the submodule-pointer update path.
4. Prove whether this is the same root cause as `#6btt` or a separate closeout variant.

## Fix bar

The turn should not report committed/success until the owning repo worktree for the target document is clean, or the same cycle should reopen automatically and repair the residual drift before returning success.
