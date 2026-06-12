# Plan: JB `Compact Exchange` / finalize raises `File Cache Conflict` — make it CRDT-driven

## Status: CODE-COMPLETE (2026-06-12)

The CRDT-driven compact path shipped as `#w42v` (agent-doc `7b6f022e`, in HEAD):
`compact::apply_compacted_document` now calls `write::try_compact_editor_converge`
— a component-scoped `exchange` `op:replace` over editor IPC (`transport=editor_ipc`)
with the guarded disk write (`compact_exchange_direct_write`) as `transport=disk_fallback`,
and `compact_writeback transport=…` proof in `ops.log`. The last open deliverable
— deterministic concurrent-edit convergence coverage — landed `3083b821`:
`compact_convergence_is_exchange_scoped_preserving_concurrent_queue_edits` (the
convergence patch only `op:replace`s the changed `exchange`, so concurrent `queue`
typing is never clobbered) and `try_compact_editor_converge_converges_via_editor_ipc_with_listener`
(live-listener end-to-end `transport=editor_ipc`). Spec: `specs/12-deterministic-simulation.md`.

**Remaining = operator-drive only:** to dogfood live, run `agent-doc lib-install`
(deploys the already-committed cdylib so the open IDE picks it up via mtime
hot-reload), then trigger `Compact Exchange` with the buffer dirty and confirm
`ops.log` shows `compact_writeback … transport=editor_ipc` and NO `File Cache Conflict`.

## Symptom (operator reports, 2026-06-11)

- "JB `Compact Exchange` should be CRDT driven... I'm getting a `File Cache Conflict`."
- "JB `File Cache Conflict` occurred while I was entering in queue items during
  finalize."

The JetBrains plugin raises a `File Cache Conflict` when `Compact Exchange` (and
finalize concurrent with operator typing) writes the document to disk while the
IDE holds an unsynced in-memory buffer.

## Root cause (CONFIRMED 2026-06-11)

`compact::apply_compacted_document` (and `run_component_compact*`) writes the
compacted document to **disk** via `crate::write::atomic_write_if_current_pub`
(op label `compact_exchange_direct_write`), NOT through the editor IPC/CRDT path.
When the JB file cache holds the buffer open with pending edits (operator typing
queue items), that direct disk write diverges from the IDE buffer →
`File Cache Conflict`.

**The full-content editor-IPC path for compact already exists but is
HARD-DISABLED by design.** `write::try_ipc_full_content_operator_mutation`
exists, and `full_content_source_label(OperatorMutation)` is literally
`"compact_exchange"` — it was built for this. But
`try_ipc_full_content_with_mode` unconditionally logs
`full_content_ipc_disabled ... reason=disabled_by_default` and returns
`Ok(false)` (test: `try_ipc_full_content_returns_false`). So the JB-side
full-document replace was intentionally stubbed off — presumably because a
full-content editor replace could not be safely applied/converged via CRDT at
the time.

**This is therefore NOT a one-line "route through try_ipc" swap.** Making
`Compact Exchange` CRDT-driven means *re-implementing and re-enabling* the
full-content operator-mutation IPC delivery end to end:
1. Decide and document why it was disabled and what changed (the `#q7jm` work
   added JB/VS Code `op: "replace"` component-patch support — does that now make a
   full-content or component-scoped replace safe?).
2. Prefer a **component-scoped** `op: "replace"` convergence of the `exchange`
   component (like `#q7jm`) over a whole-document replace, so the operator's
   concurrent edits to other components/queue survive CRDT merge.
3. Re-enable behind the listener-active scope gate; keep the disk write as
   `transport=disk_fallback` with the existing risk marker.
4. Emit proof: `compact_writeback transport=editor_ipc|disk_fallback`.

## Decision (operator) — CONFIRMED 2026-06-12

Operator direction received (queue: "JB `Compact Exchange` should be CRDT
driven... File Cache Conflict" + the cross-editor-broadcast follow-up). Approved
approach: **component-scoped `exchange` `op:replace` via editor IPC, mirroring
`#q7jm`** — converge the `exchange` component through the listener so the
operator's concurrent edits to other components/queue survive CRDT merge; keep
the disk write as `transport=disk_fallback`.

**Constraint that remains (NOT a decision gate — an execution constraint):** the
implementation edits the very IPC-write/closeout path (`try_ipc_full_content_with_mode`,
`compact.rs`, `ffi.rs`) that a live session uses to finalize its own responses.
Rebuilding the cdylib mid-session hot-reloads that machinery under the open
document → drift. So implementation must run in a **CLEAN session** (operator
restarts the doc session so the live supervisor picks up the rebuilt binary), and
the final live JB two-editor convergence is operator-drive. Tracked as actionable
backlog `#jbcompactcrdt`.

## Repro to capture (operator-drive)

Open the doc in JB, type into the buffer, trigger `Compact Exchange`; capture the
`File Cache Conflict` + `ops.log` (`compact_exchange_direct_write` /
`full_content_ipc_disabled reason=disabled_by_default`).

## Verification (operator-drive live gate)

Needs a live JB editor. After the fix: `Compact Exchange` with the buffer open
and dirty must converge through the editor without a `File Cache Conflict`;
`ops.log` shows `transport=editor_ipc`.

## Files (likely)

- `src/compact.rs`, `src/write.rs` / `try_ipc` path, `ffi.rs`
- JB plugin patch applier (already honors explicit component `op` overrides per `#q7jm`)
- specs covering compact + editor IPC
