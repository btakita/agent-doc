# Plan: IPC corruption + duplicate response when user types below `/agent:exchange` during finalize

**Bug class:** write/IPC/typing-guard
**Related:** [#jbccfix] (compact-exchange cache boundary), [#stuckcycledup] (already-committed duplicate-response handling)
**Severity:** high — produces user-visible duplicate response bodies and forces manual `agent-doc dedupe` + manual `git commit` recovery

## Problem

While the agent was running `agent-doc finalize <FILE>` and the user was typing into an HTML scratch comment below the `<!-- /agent:exchange -->` close marker, the following happened in sequence (observed in this session, finalize cycle ending in commit `9962dbcf8` for `tasks/agent-doc/agent-doc-bugs2.md`):

1. **Socket IPC failed with ack `status=error`:**
   `[write] socket IPC failed: IPC ack status error: {"type":"ack","status":"error"} — falling back to file IPC`
2. **File-IPC fallback saw "live exchange edits but no ack-content proof and no exchange materialization":**
   `[write] file IPC consumed for <FILE> with live exchange edits but no ack-content proof and no exchange materialization — falling back before snapshot/commit`
3. **IPC timeout path adopted the live file as "already in current":**
   `[write] IPC timeout path: response already in current file; adopting normalized current content`
4. **Commit warned about file being 1286 bytes larger than snapshot:**
   `[commit] WARNING: file is 1286 bytes larger than snapshot for <FILE> — possible out-of-band write (snap=12428, file=13714)`
5. **`session-check` then reported a duplicate response patchback:**
   `[session-check] INTERRUPTED: found consecutive duplicate response patchback at <heading>`
6. **`agent-doc dedupe` removed 3932 bytes; a follow-up `agent-doc write --commit` failed with `Error: empty response — nothing to write`, so the dedupe had to land via a manual `git commit`.**

The "full-document IPC corruption" the user observed appears to be the file-IPC fallback applying the response on top of a live document whose tail already contained the response (from a prior socket-IPC retry that the plugin then rejected with `ack=error`). The duplicate is the same response written twice through two different IPC paths.

## Root cause hypotheses

1. **Socket-IPC `ack=error` is not authoritative enough to short-circuit the file-IPC fallback.** The fallback re-writes a response that the socket leg may already have applied (or partially applied) to the live buffer.
2. **File-IPC fallback's "response already in current file; adopting normalized current content" branch double-counts** when the live file contains both the prior baseline + an in-flight typing edit + the response — adopting the current content makes the snapshot lag by the typing delta.
3. **Typing-guard does not block IPC retries the same way it blocks direct disk writes.** A user editing the post-`/agent:exchange` scratch comment during finalize should pause IPC retries until typing settles, mirroring the `flow::document_mutation` visible-write guard.
4. **`agent-doc write --commit` (no stdin) after dedupe rejects "empty response" instead of treating the dedupe-only file state as a recoverable closeout** — so the user must fall back to raw `git commit` to land the dedupe, defeating the binary-owned closeout contract.

## Plan

### Phase 1: Repro (deterministic SimWorld)

**File:** `src/sim_world.rs` (or a sibling sim file)

1. Model: `Op::FinalizeWithTypingInPostExchangeComment` — agent calls finalize; user races a typing edit into an HTML scratch comment below `</agent:exchange>` while socket IPC returns `ack=error`.
2. Assert: no duplicate response heading in the resulting file; snapshot matches HEAD; `session-check` passes.
3. Until the fix lands, this scenario must reproduce the duplicate today (acts as the failing baseline).

### Phase 2: Socket-IPC ack=error must not trigger file-IPC fallback that re-writes the response [done — binary side]

**File:** `src/ipc_socket.rs`, `src/write.rs` (landed); JB plugin Kotlin work tracked separately

1. **(done)** When socket IPC returns `{"type":"ack","status":"error","reason":"already_applied"}`, the write path treats the response as plugin-applied and stops. Implementation: new `AckClassification` enum + `classify_ack(ack)` + `is_already_applied_error(err)` helpers in `src/ipc_socket.rs`; the main socket-IPC caller in `src/write.rs` (line 8552 area) honors `is_already_applied_error` by logging `ipc_socket_already_applied_skip_file_fallback`, cleaning fallback patch files, and returning `success=true` without falling through.
2. **(remaining)** When the ack reason is genuinely "plugin failed" (not "already applied"), file-IPC fallback should hash-compare the current file's response-suffix against the response body and skip the write when they already match. Add as `#ipcfilehashskip` follow-up.
3. **(remaining)** Plugin protocol contract — the JetBrains plugin and VS Code extension need to actually emit `reason: "already_applied"` when they detect the patch is already in the live buffer. Until then, this binary-side gate is dormant. Add as `#ipcpluginalready` follow-up.

**Tests (`ipc_socket::tests`):** 7 unit tests cover ok / no-status / already_applied (lower + mixed case) / error-no-reason / other-error-reason / malformed-JSON. 2 unit tests cover `is_already_applied_error` matching and rejecting unrelated errors.

**Spec contract:** the plugin ack JSON shape is now part of the IPC protocol: `{"type":"ack","status":"error","reason":"already_applied"}` means "patch text is already in my buffer — do NOT fall through to file IPC re-write".

### Phase 3: Typing-guard around IPC retries

**File:** `src/flow/document_mutation.rs`, `src/write.rs`

1. The visible-write guard already defers direct disk writes when the typing indicator is active. Extend the same defer to IPC retries during finalize: if the typing indicator is active when the socket-IPC ack arrives with `status=error`, wait the bounded debounce before falling back to file IPC.
2. If the indicator does not become idle in the bounded wait, fail closed and surface a `flow_event stage=ipc_retry_guard reason=typing_defer_active_typing:finalize`.

### Phase 4: Dedupe-only recovery must land through `agent-doc write --commit` [done]

**File:** `src/write.rs` (landed)

1. When `agent-doc dedupe <FILE>` succeeds and the working tree differs from HEAD only by the dedupe deletions, a follow-up `agent-doc write --commit <FILE>` (no stdin) must accept "empty stdin + dedupe-only working-tree drift" as a recoverable closeout and commit the dedupe directly through the binary path.
2. Do not regress the existing "empty response — nothing to write" guard for the normal case (no working-tree drift); only relax it when the drift is provably a dedupe outcome.

**Implementation:** `recover_empty_response_for_strict_closeout` calls a new `recover_dedupe_only_drift(file)` helper. The helper compares the live file against `dedupe_responses(show_head(file))`; only when they match exactly (and HEAD itself had duplicates) does it save the snapshot, run `git::commit(file)`, and return `Ok(true)`. Arbitrary drift, drift against a clean HEAD, and absent HEAD all return `Ok(false)` so the existing "empty response" guard still fires.

**Tests (`write::late_fallback_patch_guard_tests`):**
- `recover_dedupe_only_drift_commits_when_file_matches_dedupe_of_head` — happy path: HEAD has a duplicate, file matches `dedupe(HEAD)`, recovery commits and snapshot reflects the deduped response.
- `recover_dedupe_only_drift_skips_when_file_matches_head` — no drift means no recovery.
- `recover_dedupe_only_drift_skips_when_drift_is_not_a_dedupe_outcome` — arbitrary user edit is never auto-committed.

### Phase 5: Regression coverage

1. Unit tests on the new socket-ack reason field and file-IPC fallback hash-skip.
2. Integration test for `dedupe` → `write --commit` recovery path.
3. SimWorld scenario from Phase 1 must pass.

## Testing

- Manual repro: type into a scratch comment below `</agent:exchange>` while `agent-doc finalize` runs against a socket-IPC peer that returns `ack=error` for the first attempt; assert no duplicate heading lands in the file.
- `make check` clean (clippy + full test suite).
- `cargo test --bin agent-doc ipc_socket::tests` and `write::tests::file_ipc_fallback_*` cover the new paths.
