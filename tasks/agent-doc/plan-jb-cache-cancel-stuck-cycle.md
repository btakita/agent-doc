# Plan: JB File Cache Conflict cancel leaves cycle uncommitted but response applied

**Bug ID:** `#adoc-jb-cache-cancel-stuck-cycle`
**Bug class:** closeout / IPC error-recovery
**Related:** [#jbccfix] (Compact Exchange cache boundary), [#jbccreg], [#jbccdocs], [#adoc-prefix-strip-uncommitted] (downstream of this bug), [#stuckcycledup]
**Severity:** high — every cycle that hits the JB "File Cache Conflict" dialog and the user cancels it requires manual `agent-doc write --commit` recovery, defeating the binary-owned closeout contract.

## Problem

With a JetBrains IDE open on the session document and the agent running `agent-doc finalize <FILE>`, IntelliJ occasionally surfaces a "File Cache Conflict" dialog when its internal cache disagrees with the disk contents about to be applied through file-IPC. The expected paths:

- **Accept:** plugin applies the patch, cycle reaches `committed` normally.
- **Cancel:** plugin should refuse the apply and signal failure so the binary can recover or fail closed.

Observed instead when the user clicks Cancel:

1. The response patch text **is** present in the working tree (file-IPC fallback wrote it to disk despite the dialog cancel, or the patch was already partly applied before the dialog blocked).
2. The cycle state never reaches `committed` — preflight on the next invocation reports "post-commit local drift" and refuses to short-circuit.
3. `git status` shows the modified session document but no commit landed.
4. Recovery: the user runs `agent-doc write --commit <FILE>` manually. That command sometimes succeeds, sometimes triggers the downstream `#adoc-prefix-strip-uncommitted` bug, sometimes requires a bare `git commit` fallback.

The combined effect: every IPC delivery that races the JB cache-conflict dialog wedges the cycle until manual recovery. Several cycles in this session hit it; commits `006f6c95d`, `a824e0e85`, etc. demonstrate the chain of stuck states.

## Repro

1. Open the session document in IntelliJ with the agent-doc plugin running.
2. Trigger any condition that surfaces the File Cache Conflict dialog (often: external write to the file while IDE has cache pending, e.g. `agent-doc compact` followed immediately by `agent-doc finalize`).
3. During `agent-doc finalize <FILE>`, click **Cancel** on the dialog.
4. Inspect: `git status` shows the response in the working tree; `agent-doc preflight <FILE>` reports the cycle is stuck; HEAD does not contain the response.

A deterministic SimWorld scenario should model the cancel branch with three observable outcomes (dialog never shown, accepted, canceled) and assert that the canceled branch either reaches `committed` cleanly OR fails closed with a clear error pointing at the recovery command.

## Root cause hypotheses

1. **The JB plugin applies the patch through VFS before showing the conflict dialog**, so by the time the user clicks Cancel the working tree already has the response. The plugin then returns `false` from the IPC callback (treating Cancel as failure), but the disk write has already happened — the binary's file-IPC fallback does not know the visible write is in flight, sees the in-flight write as "live exchange edit", and bails on the snapshot/commit.
2. **The plugin's Cancel path does not undo the partial apply** — it surfaces the dialog after the change has been queued through `setBinaryContent` or Document API, and Cancel only stops the IDE from refreshing its cache, not the underlying write.
3. **The binary's recovery path conflates this stuck state with `prior_patchback_without_response_body`** instead of recognizing it as `jb_cache_conflict_cancel`, so it cannot offer a tailored repair.

## Plan

### Phase 1: Deterministic SimWorld repro

**File:** `src/sim_world.rs`

1. Add a new `SimCommand` variant or scenario test: `FinalizeWithJbCacheConflictCancel`. Model the three branches (no dialog, accept, cancel) explicitly.
2. The cancel branch should produce the observed pattern: response on disk, snapshot ≠ HEAD, cycle phase `write_applied` not `committed`.
3. Assert the scenario fails today (acts as the failing baseline) and must pass once Phase 3/4 land.

### Phase 2: JB plugin — undo partial apply on Cancel

**Files:** `editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/PatchWatcher.kt`

1. When the File Cache Conflict dialog is shown, the plugin must defer the write until the user resolves the dialog — never queue the change beforehand.
2. On Cancel, the plugin returns `0` (or `2` if appropriate via the v2 callback) from the socket IPC callback so the binary's `is_already_applied_error` gate behaves correctly.
3. On Cancel of a file-IPC apply, the plugin should leave the patch file in place and return without modifying the document — the binary's existing file-IPC retry path will then see the un-applied patch and either retry or surface a clear error.

### Phase 3: Binary — recognize the stuck-cycle pattern and offer tailored recovery

**Files:** `src/preflight.rs`, `src/repair.rs`, `src/write.rs`, `src/session_check.rs`

1. Preflight: distinguish "post-commit local drift" (existing) from "jb_cache_conflict_cancel" (new). Detection: cycle phase is `write_applied`, working-tree response content is structurally equivalent to the captured response body, snapshot ≠ HEAD, no live exchange edits beyond the response.
2. When detected, preflight's `recovered: true` path runs the equivalent of `agent-doc write --commit` automatically (the response is already in the working tree; the binary just needs to commit it through the binary-owned snapshot/commit boundary).
3. `session_check` should not surface a `likely_direct_response_patchback` interruption for this pattern — it is a known recovery path, not a direct patchback.

### Phase 4: Documentation + runbook

**Files:** `runbooks/commit.md`, `runbooks/harness-invocation.md` (or new `runbooks/jb-cache-conflict.md`)

1. Document the dialog Accept/Cancel contract — what the user should expect on each path.
2. Document the recovery command for the cases Phase 3 cannot auto-recover.

### Phase 5: Regression coverage

1. SimWorld scenario from Phase 1 must pass after Phase 3 lands.
2. Integration test (`tests/run_integration.rs` or sibling) that simulates the cancel-branch file shape and asserts `agent-doc preflight` auto-recovers without manual `write --commit`.

## Testing

- Manual repro: open file in IntelliJ, force File Cache Conflict dialog, click Cancel during `agent-doc finalize`; assert cycle reaches `committed` after the next `agent-doc preflight` automatically.
- `make check` clean.
- `cargo test --test run_integration jb_cache_cancel_*` (new tests).
- SimWorld scenario from Phase 1.
