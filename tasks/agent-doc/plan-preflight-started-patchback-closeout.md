# Plan: Preflight-Started Patchback Closeout

**Pending:** `#pfcycle` in `tasks/agent-doc/agent-doc-bugs2.md`
**Status:** Draft

## Problem

A session-document response patchback can land in the target file, but the persisted `agent-doc` cycle state still ends at `preflight_started`. When that happens:

- the response is visible in the document
- no owning repo commit is recorded for the patchback
- `agent-doc session-check` reports `cycle started but no write/commit followed`
- `agent-doc repair` cannot recover because there is no pending/captured response artifact left to replay

This is narrower than the older direct-patchback detector failures. The user-visible text exists, but the closeout state machine never advanced to `response_captured`, `write_applied`, or `committed`, and it did not fail closed before returning control.

## Reproduction Shape

Current live example:

- target doc: `src/session-share/tasks/claudescore-3.md`
- response block: the `#comm` follow-up about Fargate vs SQLite/Supabase persistence is present in the exchange
- `agent-doc session-check /home/brian/work/btakita/agent-loop/src/session-share/tasks/claudescore-3.md`
  returns an interrupt on open cycle `preflight_started`
- `agent-doc repair` reports no pending response found

That means the cycle metadata and the document content disagree in a direction the current repair path does not know how to close.

## Goal

Make this state impossible or recoverable:

1. if a response patchback reaches the target document, cycle state must advance past `preflight_started`
2. if advancement fails, a replayable recovery artifact must exist
3. if neither condition can be proven, the command must fail closed before leaving the document in an ambiguous partially-applied state

## Hypotheses To Check

- the response reached the file through a path that bypassed `response_captured` / `write_applied` state persistence
- the cycle-state write and document write are not atomic, and the process exited after file mutation but before cycle-state promotion
- a recovery/cleanup path deleted the pending artifact too early, leaving `preflight_started` with no replay source
- a submodule or writable-root boundary let the patchback land while the commit/closeout path aborted before updating cycle metadata

## Implementation Steps

- [ ] Trace the `#comm` reproduction through `preflight`, `write`, `run`, `finalize`, recovery hooks, and cycle-state persistence to identify how document content can advance while cycle state stays `preflight_started`
- [ ] Define the invariant in code/spec terms: once a response patchback mutates the target document, the same code path must also persist either `response_captured` / `write_applied` state or a durable recovery artifact before returning
- [ ] Add a fail-closed guard so paths that can no longer replay the response do not leave `preflight_started` behind after mutating the document
- [ ] Add recovery logic for the exact ambiguous shape:
  - file contains a newly appended `### Re:` block
  - cycle state is `preflight_started`
  - no replayable pending artifact exists
  - snapshot/HEAD may or may not already reflect the patchback
- [ ] Ensure submodule-owned documents use the owning repo’s closeout/commit path or report a precise refusal before any patchback lands
- [ ] Add regression coverage for:
  - response lands + cycle state promotion succeeds
  - response lands + process dies before commit, but repair can continue from persisted state
  - response lands + no pending artifact exists, and `session-check` / `preflight` recover or fail closed precisely instead of leaving ambiguous drift
  - same shape inside a submodule-owned document
- [ ] Update the relevant specs and operator docs after behavior is settled
- [ ] Verify with focused tests plus an end-to-end `claudescore-3.md` style reproduction

## Files Likely To Change

- `src/agent-doc/src/write.rs`
- `src/agent-doc/src/run.rs`
- `src/agent-doc/src/preflight.rs`
- `src/agent-doc/src/session_check.rs`
- `src/agent-doc/src/repair.rs`
- `src/agent-doc/src/cycle_state.rs`
- `src/agent-doc/specs/07-commands.md`
- `src/agent-doc/README.md`
- `src/agent-doc/VERSIONS.md`

## Exit Criteria

- A visible assistant patchback cannot coexist with a terminal `preflight_started` cycle unless `agent-doc` can explicitly recover it in the next command
- `session-check` distinguishes this shape from direct patchback bypass and from post-commit local drift
- the `claudescore-3.md` reproduction is either auto-recovered or rejected before ambiguous partial state is written
