# Plan: claudescore-3 committed-response visibility drift

## Problem

`agent-doc` can land the real response for `src/session-share/tasks/claudescore-3.md` in committed `HEAD`, while the live worktree still carries post-commit drift on the same turn. Downstream agents then see `git status` noise and describe the response as uncommitted even though the substantive answer already landed.

That makes the turn state ambiguous to both humans and tooling: the response is committed, but the visible file state still looks unfinished.

## Repro shape

1. Route or run a turn against `src/session-share/tasks/claudescore-3.md`.
2. Let the response and repo-side doc changes commit successfully.
3. Observe that `HEAD` already contains the new `### Re:` block for the turn.
4. Observe that `git status --short` in `src/session-share` still reports the same file (and sometimes adjacent doc files) as modified afterward.
5. Ask a downstream agent to summarize repo cleanliness; it reports the response as uncommitted because the worktree is still dirty.

## Expected behavior

- the committed response is clearly distinguishable from any later post-commit drift
- `agent-doc` either leaves the live worktree clean after commit or explicitly reports that only post-commit repair drift remains
- downstream tooling should not have to infer committed-turn state from a dirty file that still contains agent-owned residue

## Investigation targets

1. Diff `HEAD` vs live worktree vs `.agent-doc` snapshot for the same completed turn and classify which deltas are substantive vs agent-owned closeout churn.
2. Trace whether the remaining drift is created by CRDT replay, boundary normalization, backlog reconciliation, or post-commit exchange/status repair.
3. Check whether the current success/closeout surfaces expose enough metadata for another agent to tell "response committed, repair drift remains" from "response never committed."
4. Determine whether this should fold into `#cs3turn` as the same underlying repair or needs an explicit committed-state invariant in `agent-doc` outputs.

## Fix bar

After a successful turn, `agent-doc` should either:

- prove `HEAD`, snapshot, and live file parity for the completed response, or
- surface a precise post-commit repair state that cannot be mistaken for an uncommitted response.
