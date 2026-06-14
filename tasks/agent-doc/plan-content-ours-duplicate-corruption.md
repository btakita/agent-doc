# Plan: `content_ours` snapshot adoption ingests duplicate / malformed component blocks (`#dupcontent`)

**Status:** PLAN ONLY — do not implement the fix. Filed via `#agent-doc-bug` 2026-06-14.

## Summary

When `finalize --stream` falls back to adopting the live-editor / stale-supervisor buffer
(`snap_source=ack_content_sidecar`, `recovery=content_ours_snapshot_next_cycle`,
`snapshot from content_ours`), agent-doc takes that buffer as the snapshot base **without
validating structural invariants**. If the buffer is itself a bad prior CRDT merge it can
contain **duplicate singleton component blocks** and a **truncated/malformed component
opening marker**, and that corruption is written straight to disk. The lint-gate catches the
malformed attribute *after* the corrupt content already landed on disk, interrupting the
cycle but leaving a corrupt working tree.

## Live evidence (monsterrodholders.md, 2026-06-14 ~02:32)

Trigger context: a **route-owned host supervisor (pid 1328300)** was running a binary that
predated the installed binary by ~12 min (supervisor started 02:15:48, `~/.cargo/bin/agent-doc`
mtime 02:28:10); `session-check` correctly warned `#fcc0/#ipcdrift` (stale build).

`finalize --stream` on the doc emitted, in order:
- `mid-turn cycle rotation detected ... #adoc-compact-during-turn-response-loss`
- `IPC proof insufficient ... invariant=live_prompt_drift_after_preflight recovery=content_ours_snapshot_next_cycle snap_source=ack_content_sidecar candidate_len=39216 content_ours_len=40827`
- `snapshot from content_ours (40827 bytes)`
- `Error: [lint-gate] INTERRUPTED ... attribute `tasks=` on `agent:queue` has empty value [agent-doc/empty-attr-value]` at line 88.

Resulting on-disk corruption (vs a structurally-clean 181-line HEAD):
- **273 lines** (≈ +92 duplicated lines).
- **Two `agent:queue` blocks**: the held one (`<!-- agent:queue -->`, items struck) at ~line 88,
  **plus a second, stale, re-armed** `<!-- agent:queue preset="#spec-test-commit-push" priority go -->`
  block at ~line 181 (the spurious re-arm that had already been cleared earlier in the session).
- The first queue marker was itself **truncated mid-attribute**: `<!-- agent:queue tasks=" -->`
  (an `agent:queue` never legitimately carries a `tasks=` attribute — this is a merge boundary
  split, likely the leading bytes of a different marker grafted onto the queue open tag).
- HEAD was unaffected (response never committed); corruption was disk-only, from the
  `content_ours` buffer.

Repair that worked: `cp` the corrupt file for evidence → `git checkout HEAD -- <file>`
(HEAD was clean) → `agent-doc reset --from-current --preserve-session`. Disk == HEAD, single
valid queue block, no malformed attr.

## Root-cause hypothesis

1. The `content_ours` / `ack_content_sidecar` buffer can already be a **corrupt CRDT merge**
   (duplicate singleton components, attribute split across a merge boundary) when a stale
   supervisor + live editor have been racing writes across the session.
2. The `recovery=content_ours_snapshot_next_cycle` path **adopts that buffer verbatim** as the
   next snapshot base and writes it to disk **before** any structural validation; the lint-gate
   runs only afterward and merely interrupts, it does not prevent the corrupt write or repair it.
3. The stale supervisor (already classified stale by session-check) is still trusted as an IPC
   content source, so its bad buffer keeps being ingested.

## Fix approach (to design — NOT to implement here)

1. **Structural validation before adopting any `content_ours`/`ack_content_sidecar` snapshot.**
   Reject (or auto-repair to HEAD/candidate) a buffer that has **>1 opening marker for any
   singleton component** (`agent:exchange`, `agent:status`, `agent:queue`, `agent:backlog`,
   `agent:review`, `agent:icebox`, `agent:done`) or a **malformed/unknown attribute** on a
   component open marker (e.g. `tasks=` on `agent:queue`, or an unterminated quoted attribute).
2. **Fail closed on a known-stale supervisor.** When session-check / the IPC layer already knows
   the serving supervisor is `supervisor_binary_stale` (`#fcc0`, pid started before binary mtime),
   refuse to adopt its buffer as the snapshot base — prefer the candidate or HEAD via
   `converge_document_or_disk()` (ties into `#fcc0`).
3. **Validate the candidate before the disk write, not after.** Run the structural/lint check on
   the merged candidate and abort to a safe base *before* writing, so a malformed merge never
   reaches disk; surface a single repair hint instead of leaving a corrupt working tree.
4. **De-dup on parse.** When reconciling component blocks, collapse duplicate singleton blocks by
   keeping the canonical (HEAD-derived) one and discarding injected duplicates rather than
   concatenating them.

## SimWorld coverage to add

- A `content_ours` buffer containing **two `agent:queue` blocks** is offered for adoption →
  finalize refuses/repairs to a single block; disk never reaches the duplicated state.
- A component open marker with an **unterminated/foreign attribute** (`agent:queue tasks="`) →
  rejected pre-write with a structural error, not an after-the-fact lint interrupt.
- A finalize whose serving supervisor is flagged `supervisor_binary_stale` → IPC buffer adoption
  is skipped in favor of candidate/HEAD.

## Relationships

Same disk-vs-buffer convergence family as `#fcc0` (route writes through
`converge_document_or_disk`), `#compactqattr` (post-compact CRDT re-serialization mangling the
queue marker), and `#adoc-compact-during-turn-response-loss` (mid-turn cycle rotation). Distinct
in that the failure mode here is **duplicated component blocks + a split attribute ingested from
`content_ours`**, not attribute loss or a dropped response.
