# Plan: Fix finalize sidecar normalization divergence losing patchback

## Problem

When `finalize` writes a response via IPC, the sidecar normalization step can diverge from the editor's view of the file, causing `content_ours` fallback. The response is committed to git but the editor does not display it. The user sees "the response patchback was not written into the document" despite `agent-doc read` (which reads from HEAD) showing it correctly.

### Reproduction (from session-share/tasks/root.md, 2026-05-09)

1. First `finalize` fails at `pending_done_guard` pre-write gate — response is never written to disk.
2. Second `finalize` with `--pending-done vt-submod` delivers IPC patch, but:
   ```
   [write] sidecar normalization diverged — falling back to content_ours (6634 bytes)
   ```
3. Response is committed to git HEAD but editor does not show the patchback.

### Root Cause (hypothesis)

Two issues compound:

1. **`pending_done_guard` is a pre-write gate.** When it fires, the response is never written to disk at all. The user's first `finalize` attempt looks like it worked (prints commit messages from the preflight cleanup) but actually blocked. The error message mentions `commit_already_current` which is confusing — the commit that is "already current" is from preflight, not from this response.

2. **Sidecar normalization divergence on retry.** When the second `finalize` runs, the sidecar (IPC editor sync) normalizes the file content differently from the binary's `content_ours` path. This can happen when:
   - The editor has modified whitespace or line endings between attempts
   - The snapshot baseline drifted during the failed first attempt
   - The boundary marker repositioning from the first attempt's commit left the file in a state the sidecar doesn't expect

## Fix

### Phase 1: Improve sidecar divergence recovery

When sidecar normalization diverges and `content_ours` fallback is used, the binary must also re-deliver the IPC patch with the `content_ours` content so the editor view matches git HEAD. Currently the IPC delivery happens before the divergence check, so the editor gets the sidecar-normalized version (which the binary then rejects).

**Files:** `src/agent-doc/src/write.rs` (IPC delivery + sidecar normalization ordering)

### Phase 2: Surface divergence as user-visible warning

When `content_ours` fallback is used, emit a warning that the editor may need to reload the file. Currently the log line is at `[write]` level which the skill doesn't surface to the user.

**Files:** `src/agent-doc/src/write.rs`, `src/agent-doc/src/finalize.rs`

### Phase 3: Regression test

- SimWorld test: finalize with sidecar normalization divergence → verify editor IPC patch matches final committed content
- SimWorld test: retry finalize after `pending_done_guard` failure → verify patchback is visible in editor view after recovery

## Related

- `plan-finalize-pending-done-cascade.md` — guard cascade issue (implemented in `#finalize-do-cascade`)
- The cascade fix made `--pending-done` work on first attempt, but this sidecar divergence bug means even a successful write may not appear in the editor
