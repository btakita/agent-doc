# Plan: repair committed `claudescore-2` closeouts that leave visible compact-exchange drift

## Problem

On May 1, 2026, `src/session-share/tasks/claudescore-2.md` reached a misleading "committed" state:

- `agent-doc commit` closed the cycle and `session-check` initially reported success
- the committed `HEAD` snapshot had already reaped the stale completed backlog item
- but the visible document in the `src/session-share` worktree still contained:
  - a bare `compact exchange` prompt
  - a `codex (HEAD)` response heading drift
  - a newer boundary marker than the just-committed file

That left the user-facing document looking unfinished even though the binary-owned closeout path had already declared the cycle committed. A later `session-check` run then failed on the unresolved prompt-bearing drift, and the turn only completed after explicitly running `agent-doc compact <FILE> --component exchange --commit`.

The same bug shape resurfaced again later that day from the Codex side: the repo changes and session-doc updates were applied, but the agent turn still stopped without a clean terminal closeout, forcing a manual "complete the turn" repair request. That confirms this is not only a one-off compact directive bug; it is a broader committed-closeout parity gap where side effects can land without the visible session artifact reaching a clearly finished state.

This is narrower than the earlier "compact exchange directive dispatch" bug. The tool did fail closed on the directive eventually, but the closeout/refresh path still allowed the committed snapshot, live working tree, and editor-visible CRDT state to diverge.

## Desired behavior

When `agent-doc` reports a committed closeout for a session document:

1. the committed file in `HEAD`
2. the refreshed snapshot
3. the live working-tree file
4. the CRDT/editor-visible state

must all agree on the final document body for that cycle.

If the binary can prove that the live file still contains unresolved prompt-bearing drift that belongs to the just-finished turn, it must either:

- reopen and finish the closeout automatically, or
- fail closed before reporting the cycle as committed.

It must not return success while the visible document still needs a manual compaction/repair step.

## Investigation plan

- [ ] Reconstruct the exact closeout timeline for `src/session-share/tasks/claudescore-2.md` from:
  - `.agent-doc/snapshots/...`
  - `.agent-doc/crdt/...`
  - the owning session log / ops log
  - `git show HEAD:tasks/claudescore-2.md`
- [ ] Trace the post-commit divergence path in `src/agent-doc`:
  - snapshot-side boundary reposition
  - working-tree boundary reposition / IPC skip path
  - template heading normalization that can rewrite to `codex (HEAD)`
  - CRDT refresh behavior after commit vs after compact
- [ ] Define the exact invariant for committed closeout parity:
  - prompt-bearing body drift may not remain in the live file after `commit_success`
  - metadata-only/frontmatter drift is allowed only if specs already classify it as non-prompt-bearing
  - submodule-owned session docs must satisfy the same parity contract as root-owned docs
- [ ] Decide where the repair belongs:
  - commit-time parity verification before returning success
  - session-check auto-repair when the drift is provably same-turn binary output
  - compact-time/CRDT refresh tightening so the drift never survives the first closeout
- [ ] Add a deterministic recovery path for this shape:
  - committed snapshot + unresolved visible `compact exchange` prompt
  - committed snapshot + `(HEAD)` heading drift
  - committed snapshot + newer visible boundary marker
- [ ] Add regression coverage for:
  - submodule-owned template/CRDT doc with IPC listener active
  - first commit closes the cycle only when live file and snapshot match
  - rerun after successful closeout yields `no_changes`
  - `compact exchange` follow-up completes in one binary-owned path with no second manual intervention
- [ ] Update the relevant spec/runbook surfaces only after the parity invariant is enforced in code.
