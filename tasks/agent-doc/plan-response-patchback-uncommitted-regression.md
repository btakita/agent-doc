# Plan — response patchback left uncommitted regression

**Pending:** `#patchregr` in `tasks/agent-doc/agent-doc-bugs2.md`
**Status:** Draft

## Problem

`src/session-share/tasks/claudescore-3.md` currently reproduces a regression against the already-shipped `#patchcommit` invariant:

- the assistant response patchback is visibly present in the target document
- the owning `session-share` worktree still carries that response as uncommitted
- the turn nevertheless reached the state where a human had to notice the dirty patchback manually

That should now be impossible. After `#patchcommit`, a visible response patchback is supposed to end in exactly one of two states:

1. the owning git root has a matching commit proof, or
2. the cycle fails closed with resumable state and a precise diagnostic

The current reproduction means one of the closeout paths still escapes that invariant.

## Goal

Determine which response-closeout path allowed the visible patchback to survive without an owning-root commit, then make every such path either complete the commit deterministically or fail closed before reporting success.

## Desired Behavior

- a fresh `### Re:` patchback can never remain visible in the working tree after a reported-success response cycle unless the user explicitly asked to leave it uncommitted
- normal closeout paths agree on the same success invariant:
  - `finalize`
  - manual repair via `write --commit`
  - stop-hook replay / backstop completion
  - any resume/recovery path that claims the cycle is committed
- `session-check` is not merely present in docs; its nonzero result must block successful closeout and surface the real reason
- submodule-owned session docs still use the correct owning root and cannot be misclassified as clean because the wrong repo/root was checked

## Questions To Answer First

- Did this turn actually pass through `agent-doc finalize` / `write --commit`, or did the response land through a direct patchback path outside the binary?
- If `session-check` ran, why did it not stop the cycle on the exact "response patchback visible but never committed" shape it already claims to detect?
- If the closeout was binary-owned, which root did it treat as authoritative:
  - `src/session-share`
  - `src/session-share/claudescore.com`
  - parent `agent-loop`
- Is the failure in commit execution, commit-proof verification, exit-status propagation, or post-commit mutation that re-dirtied the file?

## Implementation Steps

- [ ] Reconstruct the exact `claudescore-3.md` closeout chain from local evidence:
  - visible patchback in the file
  - current git dirt in `src/session-share`
  - any `agent-doc` cycle state, snapshots, hook traces, or session-check traces that identify which closeout path ran
- [ ] Compare the real path against the older `#patchcommit` contract to identify what escaped:
  - commit never attempted
  - commit attempted in the wrong root
  - commit succeeded but success was reported before the proof check
  - proof check failed but the nonzero exit was dropped/ignored
  - a post-commit write re-dirtied the file without reopening closeout
- [ ] Trace all response persistence entry points and ensure they converge on one shared invariant:
  - visible fresh patchback requires owning-root commit proof unless explicitly opt-out
- [ ] Audit stop-hook/backstop completion specifically:
  - if the stop hook replays a final response, prove it also enforces `session-check`
  - ensure hook-owned writes cannot leave the patchback visible while only logging an interrupted check
- [ ] Tighten status/exit propagation:
  - any failed commit-proof or failed `session-check` result must keep the cycle open or error the command
  - do not allow caller wrappers to downgrade that failure into a successful turn
- [ ] Add explicit regression coverage for the current reproduction shape:
  - visible response patchback in a submodule-owned doc with no owning-root commit proof
  - hook/repair/finalize variants of the same shape
  - post-commit mutation reopening the target doc must not be reported as a clean success
- [ ] Update the spec/runbook text only after the regression is executable and covered by tests

## Files Likely To Change

- `src/agent-doc/src/finalize.rs`
- `src/agent-doc/src/write.rs`
- `src/agent-doc/src/session_check.rs`
- `src/agent-doc/src/git.rs`
- `src/agent-doc/src/run.rs`
- `src/agent-doc/src/repair.rs`
- `src/agent-doc/src/main.rs`
- `src/agent-doc/tests/`
- `src/agent-doc/SPEC.md`
- `src/agent-doc/runbooks/commit.md`

## Exit Criteria

- the `claudescore-3.md` reproduction either commits the response in `src/session-share` or fails closed with a concrete reason
- no response-closeout path can report success while the fresh patchback is visible without owning-root commit proof
- the regression is covered by tests for normal finalize, manual repair, and hook/backstop completion paths
