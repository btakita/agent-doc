# Plan — Editor-sync barrier + editor-buffer-as-truth (prevent & recover IPC-truncation drift)

**Status:** design captured 2026-06-26 (operator-directed). Phase 0 first, then 1, then `#crdtsvdom`.

## Origin

Operator, dogfooding `agent-doc-bugs2.md`, hit live keystroke loss + IPC-truncated working
trees on 0.34.52. Two directives:

1. *"Use the editor buffer as the source of truth, not git. The agent should not need to be
   involved with this recovery."* — the binary must reconcile an IPC-truncated working tree
   from the live editor buffer, not force the agent into `git stash`/`reset`/`checkout`.
2. *"Poll-check the editor buffer to ensure the CRDT model is in sync? Stop/reject the
   poll-check when another replica or operation is making a change…like a lock…the editor
   would need to receive the change before we resume accepting the editor poll."* — a
   synchronization barrier so a reconcile never reads a half-applied state.

## The trap to avoid

A **mutex held until the editor acks** is the `live_prompt_drift_after_preflight` / `no_ack`
wedge in disguise: degraded IPC (exactly when this fires) never acks → the lock never
releases → typing blocks. We must **fail _open_ to the editor buffer**, never fail closed to
a held lock or a stale-snapshot discard. The existing wedges all come from operations that
fail closed (refuse-and-stall); any barrier we add inherits fail-open-to-buffer semantics or
we have built a more reliable deadlock.

## Three composable layers

| Layer | What | Status |
|---|---|---|
| **Prevention** — epoch barrier | Binary *defers* dangerous ops (snapshot reset, disk reconcile, merge-base rebuild) while an editor edit is in flight; bounded wait; fail-open. | Phase 1 (new) |
| **Recovery** — editor-buffer-as-truth | On the pre-commit layout guard (`SnapshotDiffersFromHead` + live IPC listener), flush the editor buffer to disk and reset snapshot to HEAD instead of bailing. | **Phase 0 (this brick)** |
| **Corrective** — state-vector dominance | Replace the `overlay_carries_unbaselined_content` markdown line-string check with yrs `StateVector` dominance so ahead/behind/divergent is causal, never discarding live content. | `#crdtsvdom` (Phase 2) |
| **Eliminative** — live delta forwarding | `CrdtReplicaForwarder` + Kotlin `ReplicaTransport` (`#crdtauth5`/`#crdtauth6`): once both replicas exchange yrs deltas live, the poll-check *is* the state-vector handshake and the snapshot-vs-buffer race disappears; the barrier becomes belt-and-suspenders. | existing track |

They compose: Phase 0 is the fail-open primitive Phase 1 lands on; Phase 1 stops the race;
`#crdtsvdom` makes unavoidable races safe; delta-forwarding eventually makes the barrier moot.

## Phase 0 — editor-buffer-as-truth recovery (foundational, Rust-only, testable)

**Where:** `preflight.rs::enforce_no_uncommitted_closeout_drift`, a new recovery arm BEFORE the
final `detect_uncommitted_closeout_drift` bail (alongside the existing route-queue /
jb-cache-conflict / late-ipc-overapplication arms).

**Trigger (all required):**
- `rc.snapshot_commit_status()` is `SnapshotDiffersFromHead { .. }` (the shape that bails today).
- No `detect_unstarted_prompt_bearing_diff` (that has its own normal path — mirror the guard).
- A live IPC listener is active for the project (`ipc_socket::is_listener_active`) — the editor
  buffer is only authoritative when a live editor is attached.
- `rc.head_content()` is available (we reset snapshot to it).

**Action (fail-open):**
1. `ipc_socket::send_save_document(project_root, path, patch_id)` — flush the editor buffer to
   disk; the editor buffer (operator's edits + last committed response) becomes the on-disk
   content. The agent never touches git.
2. Reset the snapshot to HEAD (`snapshot::save(file, &head)`), so the editor's uncommitted
   edits become the normal next-cycle prompt diff (editor-on-disk vs snapshot=HEAD).
3. `rc.invalidate_snapshot_content()`; log `ipc_truncation_recovered_from_editor_buffer`.
4. Return `Ok(true)` so the guard proceeds to a normal diff.

**Fail-open rule:** if the listener is absent or `send_save_document` errors, return `Ok(false)`
and fall through to the existing bail+hint — never block, never discard. (Operator stopgap when
no listener: a single editor save.)

**Why snapshot→HEAD, not snapshot→editor:** the snapshot is the *committed baseline*; HEAD is
authoritative there. Editor = HEAD + uncommitted edits. Setting snapshot=HEAD makes exactly the
operator's edits show as the diff; the finalize then commits editor+response in one boundary.

**Test:** factor the post-flush reconcile into a pure helper `reconcile_truncated_worktree_to_head`
(given editor content already on disk + head) and unit-test snapshot==HEAD + returns true; the
IPC `send_save_document` is the only untested boundary and is gated by `is_listener_active`.

## Phase 1 — epoch barrier (prevention; cross-language)

- **Editor side (JB/VSCode):** maintain a monotonic `edit_epoch`, bumped on every local
  `DocumentEvent` (the plugin already emits `editor_op_recorded` per keystroke — same hook).
  Expose `edit_epoch` + the CRDT `StateVector` over IPC (extend the `State` method).
- **Binary side:** before any dangerous op, read `edit_epoch`/`last_synced_epoch`. If an edit
  is in flight (`edit_epoch > last_synced`), **defer** the op for a bounded budget
  (≤150–250ms) for the editor to settle. On settle → proceed. On **timeout → fail open:** treat
  the editor buffer as truth (Phase 0 flush), never the stale snapshot.
- **Poll-check = state-vector exchange, not text compare.** "In sync" = neither side has updates
  the other lacks. Diverged → exchange deltas + merge (don't discard).
- Bounded + fail-open is the whole safety story; an unbounded barrier is the wedge.

## Phase 2 — `#crdtsvdom` (corrective; deepest, red-first)

Replace `overlay_carries_unbaselined_content` (`snapshot.rs:1138`, markdown line-multiset) with
yrs `StateVector` dominance: overlay dominates baseline → ahead/preserve (covers pure deletions
+ within-line edits on duplicate lines the string check misses); baseline dominates → behind
/discard+log; neither → divergent/3-way-merge. Extend the `#crdtedgetests` matrix with
"additive edit on a line that duplicates an existing baseline line" + "pure single-line deletion
racing finalize" (both red against today's string check). See
`plan-crdt-overlay-live-keystroke-preservation.md` for the live ops.log evidence.

## Sequencing & safety

Build Phase 0 → 1 → 2 in focused cycles against a **quiesced** session (never recompile the live
merge path while it is actively dropping keystrokes). Phase 0 ships first (Rust-only, unit-tested).
Phase 1 needs the plugin epoch plumbing. Phase 2 is red-first with SimWorld coverage. Install +
`admin recycle` only at a clean boundary.
