# Plan: JB File Cache Conflict cancel leaves response uncommitted

**Bug class:** editor-visible write conflict / closeout recovery
**Severity:** medium-high, because the response body can be visible in the
session document while `session-check` still reports an interrupted cycle.
**Observed in:** `tasks/professional/equityfundingsource.md`

## Problem

A JetBrains "File Cache Conflict" was left open overnight during an
`agent-doc finalize` recovery path. The user canceled the disk write. After
that, the last assistant response body was visible in the markdown document,
but it had not crossed a clean `agent-doc write --commit` / `finalize` boundary.

Observed recovery symptoms:

- `session-check` reported a likely direct response patchback without an
  agent-doc cycle.
- `agent-doc write --commit` refused because the captured response baseline no
  longer matched the current document.
- `agent-doc reset --from-current --preserve-session` rebuilt sidecars, but the
  next `write --commit` still saw the same baseline mismatch.
- A later `agent-doc preflight` detected `jb_cache_conflict_cancel`, auto-
  committed the visible response, then found an orphaned response and committed
  a second recovery commit.

## Desired Behavior

When JetBrains cancel leaves the response visible but uncommitted, the recovery
path should converge in one explicit binary-owned repair operation:

1. Detect that the current document already contains the captured response body.
2. Rebuild or retire stale capture/baseline sidecars that can no longer apply to
   the current visible file.
3. Commit the visible response exactly once.
4. Clear or mark the orphaned response capture so preflight does not append or
   commit a duplicate recovery body.
5. Make `session-check` pass after the repair.

## Plan

### Phase 1: Model the cancel state

- Add a fixture for the state where JetBrains reports a visible-buffer conflict,
  the user cancels the disk write, and the response body remains in the document.
- Include sidecars for captured response, stale baseline, snapshot, claimed IPC
  patch, and live-buffer digest.
- Assert that the response body is present in `agent:exchange` before repair.

### Phase 2: Add a response-present repair branch

- In the `write --commit` and `repair` paths, if the captured response body is
  already present in the current document, skip patch reapplication.
- Rebuild the snapshot from the current document only after proving the response
  body is present and prompt text is not duplicated.
- Mark the capture as committed or retired so later preflight does not replay it.

### Phase 3: Tighten reset semantics

- Make `agent-doc reset --from-current --preserve-session` also refresh the
  captured-response baseline metadata needed by `write --commit`, or explain in
  the command output why an additional repair command is required.
- Add a regression for `reset --from-current --preserve-session` followed by
  empty `write --commit` on the visible-response state.

### Phase 4: Preflight idempotence

- Ensure `preflight` auto-commit for `jb_cache_conflict_cancel` cannot both
  commit the visible response and then replay the same orphaned response.
- Add a session-check assertion after preflight repair.

## Verification

- Unit/regression: visible response + stale captured baseline repairs with one
  commit.
- Unit/regression: `reset --from-current --preserve-session` followed by
  `write --commit` succeeds or gives a precise next command.
- SimWorld or replay fixture: JetBrains cancel after file-cache conflict leaves
  visible response, then preflight repairs without duplicate response text.
- Full suite: `make check`.
