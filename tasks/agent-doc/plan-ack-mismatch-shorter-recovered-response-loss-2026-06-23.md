# Plan: ack-mismatch shorter-recovered response loss + CRDT overlay stale hot-loop

## Problem

On 2026-06-23 the last agent-doc turn on `tasks/agent-doc/agent-doc-bugs2.md`
(session `b26b9957-8de6-4eae-bb98-5d7fe4d6b781`, cycle
`cycle-1782234987430`) crashed: the agent response capture could not be
written back, the cycle was left interrupted, and a later preflight had to
auto-close it. The operator-perceived symptom was "the last turn crashed."

Two intertwined defects in the editor-IPC write path produced the crash:

1. **Pending-write writeback refused on `ack_mismatch`** because the editor IPC
   ack sidecar came back *shorter* than the write target (no recovery path).
2. **A CRDT overlay-projection hot loop** that fed stale base state into the
   write path and burned ~10k ops-log lines while the operator typed.

## Evidence

Cycle `cycle-1782234987430`, document `tasks/agent-doc/agent-doc-bugs2.md`,
ops.log + per-session log `.agent-doc/logs/b26b9957-*.log`:

- Pending writeback refused (the crash point), with `recovered` 19 bytes
  *shorter* than `target`:
  ```
  pending_write_writeback file=...agent-doc-bugs2.md patch_id=94825672-... transport=blocked reason=ack_mismatch recovered_len=22071 target_len=22090 action=refuse_external_disk_write
  pending_write_ack_mismatch_editor_refresh file=...agent-doc-bugs2.md transport=blocked reason=untrusted_ack_content_contains_user_drift action=leave_editor_owned_ack_content stale_len=22071 stale_hash=607a81423264
  ```
  Ack sidecar confirmed at `.agent-doc/ack-content/94825672-….md`? — the live
  sidecar `59cd1b58-e7fd-46aa-88a8-811e660ce53c.md` is exactly **22071 bytes**,
  matching `recovered_len`.
- Cycle then detected as interrupted by the next preflight:
  ```
  interrupted_cycle_detected cycle_id=cycle-1782234987430 phase=PreflightStarted event=preflight_started
  ```
  followed by an auto-close `commit_success` + `flow_event outcome=completed`
  and a `postcommit_worktree_check match=false`.
- CRDT overlay-projection hot loop (the feeding root cause), repeating
  per-keystroke while the operator edited the queue (`#qnodemerge4wire`):
  ```
  crdt_merge_base_overlay_stale overlay_len=21353 fallback_len=22362
  crdt_merge_base source=fallback_overlay_projection_mismatch
  editor_op_recorded kind=insert offset=5220..5225 base=0b81f3a34bcc
  ```
  `fallback_overlay_projection_mismatch` appears **10,097** times in `ops.log`
  for this doc — a persistent 1009-byte gap (overlay shorter than fallback)
  that never reconciles and is re-logged on every merge-base call.
- No durable capture exists for this cycle
  (`.agent-doc/captures/483039e7…/cycle-1782234987430.json` absent), so the
  capture-backed response-replay safety net could not recover the turn — the
  response was lost before it was ever persisted.

## Root Cause

### A. Asymmetric `ack_mismatch` safety guard (the crash)

`src/agent-doc/agent-doc-orchestration/src/write/converge.rs`:

- The writeback path (`{source}_writeback`, converge.rs:1047-1071) compares the
  editor-ack `recovered` content to `target`; on mismatch it calls
  `refresh_editor_after_ack_mismatch` and bails with
  `refuse_external_disk_write`.
- `ack_mismatch_refresh_safe` (converge.rs:754-811) is **directional**: it only
  returns `true` when the editor ack has *extra* lines that are stale
  `> **Queue prompt:**` artifacts (`recovered_lines.difference(target_lines)`
  non-empty and all artifacts). When the ack is instead *missing* bytes
  (`recovered_len < target_len`, as here: 22071 < 22090), `recovered_only` is
  empty and the guard returns `false` → the ack is classified as
  `untrusted_ack_content_contains_user_drift`, the write is refused, and the
  cycle is left interrupted.
- There is no path for the legitimate "editor ack lost response bytes"
  case (e.g. plugin applied a truncated patch, or the response body never
  reached the buffer): it is conflated with genuine user drift and fails
  closed-by-bail with no capture-replay fallback, so the response is lost.

### B. CRDT overlay never rebuilds (feeding root cause + hot loop)

`src/agent-doc/agent-doc-orchestration/src/snapshot.rs:1100-1129`
(`crdt_merge_base_state`):

- When the structured overlay sidecar decodes but its markdown projection
  (`overlay_len=21353`) != fallback markdown (`fallback_len=22362`), the code
  logs `crdt_merge_base_overlay_stale` and falls back to the baseline text.
- The fallback is correct for the *current* merge, but the stale overlay is
  never rebuilt from the fallback, so every subsequent merge-base call re-reads
  the same stale overlay, re-projects, re-mismatches, and re-logs — once per
  editor op. This produced the 10,097-line hot loop while the operator typed,
  kept the in-memory CRDT state 1009 bytes behind disk, and fed stale base
  lengths into the pending-write target computation that surfaced as defect A.

## Related Work

- `tasks/agent-doc/plan-direct-chat-owned-pane-response-loss-2026-06-23.md`
  — same "response did not land" symptom class (backlog `#directchatpb2`), but
  that is a direct-chat/recursive-invocation closeout gap; this plan is the
  editor-IPC `ack_mismatch` shorter-recovered variant with a distinct root
  cause in `converge.rs`.
- Backlog `#fbwireverify` / `#qnodemerge4verify` — convergence-gate and
  non-ASCII merge verification in the same edit-during-turn path.
- Contract: "Response replay is capture-backed" (`capture.rs`) — the intended
  safety net, which this crash bypassed because no capture was persisted before
  the refused writeback.

## Fix Plan

### Phase 1 — Stop the response loss (defect A)

- [ ] Add a deterministic regression in
  `src/agent-doc/agent-doc-orchestration/src/write/converge.rs` tests for an
  editor-IPC ack whose content is a strict subset of `target` (shorter by the
  response body, e.g. 22071 vs 22090) and assert: today this refuses the write
  and leaves no recovery (`action=refuse_external_disk_write`).
- [ ] Extend `ack_mismatch_refresh_safe` (or add a sibling predicate) to
  recognize the safe "ack shorter than target, missing bytes are the agent
  response body / structural closeout, not foreign user edits" shape, distinct
  from genuine concurrent user drift. The distinguishing signal: the
  non-exchange components match (already checked via `blank_components_named`)
  AND the recovered exchange is a prefix/substring of the target exchange minus
  a well-formed `### Re:` response block.
- [ ] When that safe-shorter shape holds, do not bail silently: route through
  capture-backed replay — persist the response capture first
  (`capture.rs`), then re-issue the write through the normal
  `agent-doc write --commit` path so the response lands and the cycle reaches
  `committed` instead of `interrupted_cycle_detected`.
- [ ] If the shorter-ack shape is *not* safe (real drift), keep failing closed
  but emit an explicit, single recovery diagnostic naming
  `agent-doc write --commit <FILE>` + `agent-doc session-check <FILE>` and
  retain a pending-retry patch under `.agent-doc/pending/` for the next cycle
  (today no recent pending entry is retained for this class).

### Phase 2 — Kill the overlay hot loop and rebuild (defect B)

- [ ] In `crdt_merge_base_state` (`snapshot.rs:1100-1129`), after detecting
  `FallbackOverlayProjectionMismatch`, rebuild the overlay sidecar from the
  authoritative fallback markdown (one-shot per cycle), so subsequent merge-base
  calls in the same cycle stop re-mismatching. Gate the rebuild behind the CRDT
  lock already acquired there.
- [ ] Rate-limit the `crdt_merge_base_overlay_stale` op-log line (e.g.
  emit once per cycle per doc, or collapse repeated identical
  `overlay_len`/`fallback_len` pairs) so a persistent mismatch can no longer
  produce a 10k-line hot loop.
- [ ] Add a regression asserting that after a stale-overlay detection the next
  merge-base call resolves with `source=overlay` (rebuilt) and emits no second
  stale line for the same baseline.

### Phase 3 — Verification

- [ ] `make check` (clippy + full nextest) in `src/agent-doc`.
- [ ] New unit tests under `agent-doc-orchestration` for both the safe-shorter
  ack path and the overlay rebuild-on-mismatch path.
- [ ] Leave a non-blocking `[operator-verify]` follow-up: live JB/VS Code edit
  of a queue item during a codex turn reproduces zero
  `fallback_overlay_projection_mismatch` spam and a committed (not
  interrupted) cycle.

## Acceptance Criteria

- An editor-IPC ack whose content is shorter than target solely by the agent
  response body no longer ends in `refuse_external_disk_write` +
  `interrupted_cycle_detected`; the response is replayed from capture and the
  cycle commits.
- A persistently-stale CRDT overlay is rebuilt from the authoritative baseline
  on first detection within a cycle, and `crdt_merge_base_overlay_stale` is
  emitted at most once per cycle per document.
- `make check` is green and the two new regressions fail on the pre-fix code.
- The 10k-line hot-loop pattern cannot recur for a static overlay/fallback pair.
