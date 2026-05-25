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

### Phase 2: Socket-IPC ack=error must not trigger file-IPC fallback that re-writes the response

**File:** `src/ipc_socket.rs`, `src/write.rs`

1. When socket IPC returns `{"type":"ack","status":"error"}` with reason `already_applied` (or equivalent — needs a new field on the ack), the write path must treat the response as plugin-applied and stop. Do not fall through to file-IPC re-write.
2. When the ack reason is genuinely "plugin failed" (not "already applied"), file-IPC fallback may proceed but must hash-compare the current file's response-suffix against the response body and skip the write when they already match.
3. Log a `flow_event` distinguishing `socket_ack_already_applied`, `socket_ack_failed`, and `file_ipc_skipped_response_already_present`.

### Phase 3: Typing-guard around IPC retries

**File:** `src/flow/document_mutation.rs`, `src/write.rs`

1. The visible-write guard already defers direct disk writes when the typing indicator is active. Extend the same defer to IPC retries during finalize: if the typing indicator is active when the socket-IPC ack arrives with `status=error`, wait the bounded debounce before falling back to file IPC.
2. If the indicator does not become idle in the bounded wait, fail closed and surface a `flow_event stage=ipc_retry_guard reason=typing_defer_active_typing:finalize`.

### Phase 4: Dedupe-only recovery must land through `agent-doc write --commit`

**File:** `src/write.rs`, `src/repair.rs`

1. When `agent-doc dedupe <FILE>` succeeds and the working tree differs from HEAD only by the dedupe deletions, a follow-up `agent-doc write --commit <FILE>` (no stdin) must accept "empty stdin + dedupe-only working-tree drift" as a recoverable closeout and commit the dedupe directly through the binary path.
2. Do not regress the existing "empty response — nothing to write" guard for the normal case (no working-tree drift); only relax it when the drift is provably a dedupe outcome.

### Phase 5: Regression coverage

1. Unit tests on the new socket-ack reason field and file-IPC fallback hash-skip.
2. Integration test for `dedupe` → `write --commit` recovery path.
3. SimWorld scenario from Phase 1 must pass.

## Testing

- Manual repro: type into a scratch comment below `</agent:exchange>` while `agent-doc finalize` runs against a socket-IPC peer that returns `ack=error` for the first attempt; assert no duplicate heading lands in the file.
- `make check` clean (clippy + full test suite).
- `cargo test --bin agent-doc ipc_socket::tests` and `write::tests::file_ipc_fallback_*` cover the new paths.
