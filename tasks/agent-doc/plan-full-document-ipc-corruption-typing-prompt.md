# Plan: full-document IPC corruption while typing a live prompt

**Bug class:** editor-ipc/live-prompt/write-guard
**Pending:** `#ipcfullprompt` in `tasks/agent-doc/agent-doc-bugs2.md`
**Related:** `tasks/agent-doc/plan-ipc-corruption-and-duplicate-during-typing.md`, `tasks/agent-doc/plan-crdt-live-prompt-prefix-duplicate.md`
**Status:** plan only; do not implement from this item without a separate implementation prompt
**Severity:** high - can corrupt the visible session document while the user is still composing a prompt

## Problem

The existing IPC corruption plan covers a user typing in the scratch HTML comment below `agent:exchange`. This report is narrower and more severe: the full-document IPC corruption occurred while the user was typing a prompt.

Expected behavior:

1. A live prompt edit is user-owned until the next preflight cycle.
2. Response closeout, IPC retry, file-IPC fallback, CRDT reconciliation, and boundary reposition may not replace the whole document with content computed before that prompt edit.
3. If the editor buffer changes while an IPC payload is being prepared or applied, the mutation must fail closed and leave the live prompt visible for the next cycle.

Observed behavior:

1. While typing a prompt, the editor-visible document was replaced/corrupted by a full-document IPC path.
2. A duplicate line then appeared.
3. A second full-document IPC corruption followed.

This means at least one active closeout/editor path still has whole-document replacement behavior, stale apply proof, or stale snapshot adoption reachable despite the source-level full-content IPC disablement contracts.

## Constraints

- Do not treat the user's live prompt as assistant response content.
- Do not absorb prompt-bearing drift into the snapshot.
- Do not commit the live prompt as a closeout repair.
- Do not delete or dedupe prompt text unless ownership proof shows it is generated duplicate residue.
- Keep the existing scratch-comment plan separate; this item is about prompt-bearing exchange edits.

## Suspected surfaces

1. `src/agent-doc/src/write.rs`
   - `try_ipc`, socket/file fallback, ack-content sidecar adoption, timeout adoption, snapshot save, and boundary reposition paths.
   - Existing tests around `try_ipc_full_content` prove the disabled full-content helper preserves live prompt drift, so the repro likely reaches a different path.
2. `src/agent-doc/src/flow/document_mutation.rs`
   - Existing `FullContentSourceProof` and visible typing guard decisions should become mandatory for every editor-visible full-buffer mutation, not only named full-content helpers.
3. `src/agent-doc/editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/PatchWatcher.kt`
   - Socket/file IPC apply path must capture editor-local text plus modification stamp after typing debounce and re-check both immediately before mutation.
   - Legacy/foreign `fullContent` payloads must be rejected, not applied.
4. `src/agent-doc/editors/vscode/src/*`
   - Same proof contract for VS Code's document version and workspace edits.
5. CRDT/IPC snapshot reconciliation
   - If ack-content or sidecar content is a whole-document image, it must not become the committed snapshot when prompt-bearing drift appeared after preflight.

## Repro plan

### Phase 1: Capture the real corruption packet

1. Enable ops logging around a controlled repro in the editor where the user types a prompt into `agent:exchange` while a closeout is in progress.
2. Capture:
   - active binary version (`agent-doc --version`)
   - editor plugin version
   - patch id
   - transport (`socket_ipc`, `file_ipc`, CRDT, boundary reposition, ack-content sidecar, or timeout fallback)
   - payload shape (`patches`, `fullContent`, `normalize_prefix_lines`, `reposition_boundary`, `ack_content`)
   - current editor modification stamp/version before and immediately before apply
   - whether typing debounce was active
3. Preserve the pre-apply editor text, post-apply editor text, disk text, baseline, snapshot, and ack-content sidecar for one failing cycle.

### Phase 2: SimWorld regression

Add a deterministic SimWorld scenario:

1. Start from a template-mode document with a prior response and a boundary marker.
2. Preflight/finalize computes a response closeout or repair payload.
3. Before IPC apply, inject a live prompt line inside `agent:exchange` after the latest boundary.
4. Simulate the observed transport from Phase 1.
5. Assert:
   - no whole-document replacement is applied
   - live prompt text remains exactly once
   - response content is not duplicated
   - snapshot does not absorb the prompt
   - session-check fails only if the live prompt needs the next cycle, not because of corruption

### Phase 3: Editor apply proof

For every editor-visible mutation path:

1. Wait for typing idle using the shared debounce.
2. Capture apply proof:
   - exact text hash and length
   - editor generation signal (`Document.modificationStamp` for JetBrains, `TextDocument.version` for VS Code)
3. Re-check text and generation immediately before applying.
4. If either changed, reject without applying:
   - socket IPC returns an error reason such as `stale_editor_buffer`
   - file IPC leaves the patch file in place for retry
   - binary fallback must not escalate to whole-document visible replacement

### Phase 4: Binary stale-prompt guard

Before adopting ack-content, timeout current-file content, or CRDT merge output as snapshot/commit content:

1. Diff against the preflight baseline.
2. If new prompt-bearing exchange text appeared after preflight and is not part of the assistant response patch, classify it as live prompt drift.
3. Fail closed before snapshot save/commit with a typed flow event, for example:
   `flow=document_mutation stage=ipc_snapshot_adoption outcome=blocked reason=live_prompt_drift_after_preflight`.
4. Leave the prompt in the working tree/editor for the next response cycle.

### Phase 5: Disable or quarantine legacy full-document payloads

1. Add a test that any `fullContent` payload received by first-party editor plugins is rejected even when it arrives via socket IPC, file IPC, or a repair/normalization path.
2. Make the rejection visible in ops logs with patch id and source.
3. If a foreign/old plugin emits or applies full-content payloads, the binary should detect the plugin capability version and refuse that route.

## Tests

- Rust unit/integration:
  - live prompt drift after preflight blocks ack-content snapshot adoption
  - live prompt drift after preflight blocks timeout-current adoption
  - file IPC fallback does not write when current editor/disk contains unowned prompt-bearing drift
  - boundary reposition cannot run as a full-buffer replacement when prompt drift exists
- SimWorld:
  - `FinalizeWhileTypingLivePromptWithFullDocumentIpcCorruption`
  - `FinalizeWhileTypingLivePromptThenDuplicateThenSecondCorruption`
- JetBrains:
  - patch apply rejects when document text or modification stamp changes after debounce
  - socket `fullContent` payload returns `stale_or_forbidden_full_content`
  - file-watch `fullContent` payload is deleted/quarantined without applying
- VS Code:
  - same proof and full-content rejection using `TextDocument.version`

## Implemented 2026-05-29 (Phase-1 instrumentation, since the bug is not reliably reproducible)

The user cannot reliably reproduce the corruption, so rather than ship a speculative
editor fix we instrumented every whole-buffer mutation/adoption seam so the **next**
real occurrence is captured automatically. Audit first confirmed the binary-side
fail-closed guards are already comprehensive and wired on both transports
(`guard_ipc_snapshot_adoption_against_live_prompt_drift` /
`..._against_prompt_duplication` at the socket and file IPC adoption paths in
`write.rs`), and they demonstrably fail closed.

What landed:

- **Binary (`write.rs`):** added `log_ipc_snapshot_adoption_allowed`, called after the
  two guards on both the socket and file IPC paths. Previously only *blocked*
  adoptions logged; *allowed* adoptions were silent. Now every adoption emits either
  `ipc_snapshot_adoption_blocked` (existing) or `ipc_snapshot_adoption_allowed`
  (new) with `snap_source`, snapshot/content_ours len+hash, and an independent
  `drift_recheck` / `dup_growth_recheck` — a non-benign re-check on an *allowed* line
  flags a guard-coverage gap. Rust unit tests:
  `ipc_snapshot_adoption_allowed_logs_benign_recheck`,
  `ipc_snapshot_adoption_allowed_is_silent_when_blocked`.
- **JetBrains (`PatchWatcher.kt`, v0.2.135):** added `documentMutationDiagnosticUtil`
  + `postBoundaryExchangeRegionUtil` + `documentMutationContentHashUtil`, logged at
  every editor-visible `setText` site (`applyPatch.component`, `repositionBoundary`).
  Each line records op, patch_id, transport, pre/post fingerprint, mod-stamp, idle
  state, and the **post-boundary user-region length pre vs post** with a
  `user_region_changed` flag — the direct duplication/deletion signal. Kotlin tests:
  `DocumentMutationDiagnosticTest`.

How to capture the packet now: reproduce in IntelliJ (type a prompt during a
closeout); when corruption appears, `idea.log` shows the `[patch-watcher]
document-mutation … user_region_changed=true` line for the offending `setText`, and
`.agent-doc/logs/ops.log` shows the paired `ipc_snapshot_adoption_(allowed|blocked)`
line with the content shape. That pinpoints the transport + payload to fix, which
then drives the Phase-2 SimWorld regression and the real fix.

Status: instrumentation only — no behavioral fix yet. Item stays gated pending a
captured packet.

## Root cause found + fixed 2026-05-29 (the instrumentation paid off)

The v0.2.135 instrumentation captured the real cause of "agent-doc ate the prompt
`go`" on `monsterrodholders.md` — and it was **not** a PatchWatcher `setText` or the
IPC snapshot adoption (those guards fired and recovered correctly). idea.log line:

```
[route] removed duplicate answered prompt tail after exchange boundary in tasks/monsterrodholders.md
```

The binary **route path** (`route.rs::scrub_duplicate_prompt_comments_for_route` →
`template::remove_duplicate_answered_exchange_prompt_tail`) deletes a post-boundary
prompt tail when it text-matches an earlier answered prompt block. Because
`normalize_duplicate_exchange_prompt_line` strips the `❯` marker before comparing,
a freshly-typed `go` (unprefixed) matched a previously-answered `❯ go` and was
deleted — silent data loss of a live prompt. `go`/`yes`/`continue` are exactly the
common short prompts that recur, so this fired often.

**Fix (committed):** `duplicate_exchange_tail_prompt_lines` now requires every tail
line to carry the `❯ ` answered-form marker before it can be treated as removable
replay residue. The marker is added only by the answer/normalize cycle, never by a
user typing a fresh prompt, so it is the ownership proof the plan's constraints
demand. A bare unprefixed post-boundary prompt is always preserved and dispatched.
Tests: `remove_duplicate_answered_tail_{scrubs_prefixed_replay_residue,
preserves_unprefixed_live_prompt}` (core), `route_preserves_unprefixed_live_prompt_
matching_an_answered_prompt` (route), plus the updated prefixed-residue fixtures in
the route/preflight/normalize tests.

This resolves the prompt-deletion symptom. The binary IPC-snapshot guards already
fail-closed + recover for the structural facets (stray `###`, stuck capture), and
the v0.2.135 diagnostics remain in place to catch any residual whole-buffer path.

## Acceptance criteria

1. Repro from Phase 1 no longer corrupts the editor-visible document.
2. Live prompt text typed during closeout remains visible exactly once and is answered on the next cycle.
3. No response heading or prompt line is duplicated.
4. No closeout commit or snapshot absorbs prompt-bearing drift that was not in preflight.
5. Full `make check`, editor plugin tests, and the new SimWorld regression pass.
