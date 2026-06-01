# Plan: `already_applied` IPC accepts scaffold-corrupted live buffer

**Bug class:** write/IPC/already-applied/live-prompt-drift
**Pending:** `#ipcaa-scaf` in `tasks/agent-doc/agent-doc-bugs2.md`
**Related:** `#zpc2`, `#ipcfullprompt-recur2`, `#prompt-duplicated-while-typing`, `#finalize-retry-ipc-response-duplication`, `#already-applied-snapshot-divergence`
**Status:** plan only; do not implement from this item without a separate implementation prompt
**Severity:** high - commits a structurally invalid session document while the user is typing

## Symptom

Live repro on `tasks/software/lazily-rs.md` immediately after a successful
`agent-doc finalize --stream` cycle:

- `agent-doc preflight tasks/software/lazily-rs.md` fails with:
  `closing marker <!-- /agent:queue --> without matching open`
- The working tree contains duplicated post-exchange scaffold inserted into the
  first `agent:queue` component.
- Partial live typing fragments were embedded in the wrong places:
  `preset #s` inside the first queue and `preset #spec-` before a stray
  `<!-- /agent:queue -->`.
- The duplicated scaffold contains a second `Backlog`, `Queue`, `Review`, and
  `Done` shell plus duplicated lazily-rs backlog items.

Representative malformed tail:

```markdown
## Queue

<!-- agent:queue -->
preset #s
<!-- /agent:queue -->

## Backlog
...
<!-- /agent:done -->
preset #spec-
<!-- /agent:queue -->
```

This is not just an editor-visible glitch: the closeout path committed a snapshot
that already contained the duplicated scaffold.

## Evidence

The closeout cycle was `cycle-1780331470151`, response hash
`ab8c2a6f3866c21209151ffd8a5a337034332de2a5970884b04a4d421249d4cc`.

`ops.log` shows the decisive sequence:

```text
ipc_socket_attempt ... patch_id=97a42d07-779a-4d1c-aa6b-ec3b42ff46af ...
ipc_socket_already_applied_skip_file_fallback ...
ipc_socket_already_applied_live_buffer_diverged ... response_present=true current_len=17683 ... content_ours_len=16542 ... prompt_drift=true
snapshot_save ... len=17683
ipc_socket_already_applied_snapshot ... snap_source=file_read snap_len=17683 ...
commit_staging ... snap_len=17683 file_len=17683
commit_success ...
```

The key violation is `prompt_drift=true` combined with
`snap_source=file_read` and `commit_success`. The ordinary IPC adoption path has
a `live_prompt_drift_after_preflight` guard that replaces the candidate snapshot
with `content_ours`; the socket `already_applied` exception accepted the live
file even though the same drift predicate fired.

## Root Cause Hypothesis

`persist_already_applied_socket_content_ours_snapshot()` treats socket
`already_applied` as proof that the live editor/file state is authoritative if
the expected response is present. In the reproduced state:

1. The expected response was present.
2. The live buffer also contained unowned prompt drift and duplicated scaffold.
3. The function logged `prompt_drift=true`, but did not fail closed.
4. It assigned `IpcRepairDecision::file_read(current)` and saved that corrupted
   `current` as the snapshot.
5. The later commit path saw `snap_len == file_len` and committed it.

This bypasses two existing protections:

- `guard_ipc_snapshot_adoption_against_live_prompt_drift()` is not applied to
  the `already_applied` path.
- `detect_duplicated_scaffold()` is only forensic/detection in some adoption
  paths; in this path the corrupted scaffold can still become the committed
  snapshot.

## Fix Plan

### Phase 1: Make `already_applied` obey the same drift invariant

In `agent-doc-orchestration/src/write.rs`,
`persist_already_applied_socket_content_ours_snapshot()` must not adopt
`file_read(current)` when:

- `ipc_snapshot_would_absorb_live_prompt_drift_after_preflight(base, current, ours)`
  is true, or
- `detect_duplicated_scaffold(current)` returns any finding.

For those cases, choose one of two explicit outcomes:

1. If `content_ours` is structurally valid and contains the response, save
   `content_ours` as the snapshot, record dropped prompt/queue evidence, and
   redeliver/repair the editor-visible buffer without committing the live drift.
2. If the live buffer contains mixed prompt fragments that cannot be safely
   preserved, fail closed before snapshot save/commit with a diagnostic that
   names the corrupted file and patch id.

Do not keep the current `file_read` adoption when `prompt_drift=true`.

### Phase 2: Promote scaffold detection from forensic to blocking on snapshot adoption

Use `detect_duplicated_scaffold()` as a hard guard for every candidate that may
become a snapshot, including socket `already_applied`.

Policy:

- Pure duplicated scaffold may be normalized through
  `repair_duplicate_exchange_close_scaffold()` before commit.
- Mixed scaffold plus live prompt fragments must fail closed; the user-owned
  fragments cannot be deleted silently.
- The failure should preserve the candidate under
  `.agent-doc/logs/ipcfullprompt/` or a sibling artifact path for debugging.

### Phase 3: Patch-id-aware `already_applied`

An `already_applied` ack should prove that the current patch id was applied, not
merely that a response heading already exists in the editor buffer.

- Extend the editor ack shape to include `patch_id`.
- Require the ack patch id to match the current binary patch id.
- If absent or mismatched, fall back to bounded file IPC only after rechecking
  typing/drift guards.

### Phase 4: Recovery guard for already-committed malformed snapshots

Add `session-check` / `repair` recovery for documents already committed in the
bad shape:

- Detect duplicate structural scaffold and unmatched component close markers.
- If a pure duplicated scaffold can be proven agent-owned, repair and commit
  through `agent-doc write --commit` / repair.
- If mixed with prompt fragments, interrupt with exact line ranges and preserve
  the evidence; do not auto-delete user text.

## Regression Tests

Add deterministic coverage near the existing IPC closeout and SimWorld tests:

- `socket_already_applied_with_prompt_drift_uses_content_ours_or_fails_closed`
- `socket_already_applied_with_duplicated_scaffold_fails_before_snapshot_save`
- `socket_already_applied_with_mixed_prompt_scaffold_preserves_evidence`
- `session_check_detects_committed_unmatched_queue_close_after_ipc_scaffold_corruption`
- editor/FFI test: `already_applied` ack without matching `patch_id` is not
  accepted as commit proof.

The failing fixture should use the current `lazily-rs.md` shape: `preset #s`,
duplicated Backlog/Queue/Review/Done scaffold, `preset #spec-`, and a stray
`<!-- /agent:queue -->`.

## Acceptance Criteria

1. A socket `already_applied` ack cannot commit a snapshot when the candidate
   has prompt drift or duplicated scaffold.
2. `prompt_drift=true` in the already-applied diagnostic always leads to either
   `content_ours` snapshot plus preserved dropped-prompt evidence, or a
   fail-closed result before commit.
3. Duplicate scaffold detection blocks snapshot adoption on every IPC transport,
   not only ordinary ack-content/file fallback paths.
4. `agent-doc preflight` on a corrupted file gives a repair/fail-closed message
   that names the unmatched marker and the IPC patch id when available.
5. Full `make check`, targeted IPC tests, and the new SimWorld regression pass.
