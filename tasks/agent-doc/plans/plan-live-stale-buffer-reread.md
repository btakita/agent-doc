# Plan: live editor re-read when a connected buffer diverges (`#evmh` live half)

**Bug class:** CRDT/replica-update/queue/stale-editor-buffer
**Related:** `#replica-structure-guard` (0.35.155), `#qdelauth` (0.35.156),
`#evmh` / `#evmhplugin`, `#qdedup-directive-twin`, `#qcellmerge`
**Status:** diagnosed; blocked on the Kotlin plugin half + live-IDE verification

## Symptom (live repro, 2026-08-07)

On `src/haiven-dev/tasks/haiven-dev.md` (haiven-dev submodule scope, controller
pid in `.agent-doc` at `src/haiven-dev`), the operator deleted `- do
[#fpedurabilityharness]` from the queue. The JetBrains plugin (the one live
editor, `source=jetbrains_plugin` client `1636503954231086`) kept the line in its
buffer and re-pushed it via `replica_update` at the **current lineage** — so a
lineage/staleness check at ingestion does not fire. The CRDT merge resurrected
the line; convergence kept it (`authored…max(1)` floor). The line was never
committed (`git -C src/haiven-dev log -S 'do [#fpedurabilityharness]'` is empty),
so there is no committed-deletion signal.

Evidence:
- ops.log shows continuous `controller_crdt_replica_handled … method=replica_update
  source=jetbrains_plugin` for haiven-dev at the current `frame_lineage`.
- The tombstone ledger row for haiven-dev (`queue_document_state`,
  `state_kind='operator_delete_tombstones'`) does NOT contain
  `fpedurabilityharness`, and `observed_active_ids` is `[]` — the deletion was
  never observed because JB re-pushed before any preflight saw the empty queue.

## Root cause

A connected editor whose buffer has diverged from the canonical is never told to
re-read while it stays connected. Two mechanisms exist, neither covers the live
case:

1. `agent_doc_reconnect_buffer_decision` (`src/ffi.rs:1342`, `#evmhplugin`) —
   decides re-read-disk vs keep-buffer, but only fires on IPC listener
   **(re)connect**. JB never reconnected.
2. `pending_rebootstrap` → "replace" on next pull
   (`agent-doc-document-realtime/src/crdt_relay.rs:1288`-`1412`,
   `pull_rebootstrap_for_file`) — forces a buffer replace, but is triggered only
   on recycle / cold start / compact, **not on live divergence**.

Because no live divergence is detected, a stale buffer streams pre-divergence
content back into the canonical. `#replica-structure-guard` only catches the
structural case (unclosed marker) and `#qdelauth` only catches *detected*
deletions; an uncommitted deletion re-pushed before the next preflight is
invisible to both.

## Why the binary-only options are risky (evaluated, not chosen)

- **Real-time tombstoning in the controller** (record an id-removal the instant
  `document_queue_authority_observe` sees the canonical lose it) closes the race,
  but creates a self-clear trap: convergence would suppress a legitimate re-add,
  so preflight never sees it active and the tombstone never clears until a TTL.
  Net effect: a legitimately re-added prompt "vanishes" for the TTL window.
- **Relay-level removal tracker + targeted canonical restore** is stateful,
  parses the queue on every merge, and false-positives on legit delete-then-
  re-add. Restoring only the resurrected id (not the whole document) is fiddly
  and the false-positive suppresses a real new prompt.

## Fix (the unblocked half is plugin-side)

The clean fix is the `#evmh` plugin half: the editor already receives the
canonical projection (delivery packets / `replica_pull`); it should compare its
buffer to that projection and, when divergence exceeds what its own local edits
explain, **re-read from canonical/disk instead of re-pushing**. This is Kotlin
plugin logic (`editors/jetbrains`) and needs a live-IDE eyeball to verify, which
makes it operator-gated.

### Binary-side support (agent-doable, lower-risk companion)

If the plugin needs a typed nudge, add a **bounded live-divergence signal** the
controller can emit when an editor's pushed content disagrees with the canonical
on more than its own recent local edits, WITHOUT mutating the canonical or
tombstoning (avoiding the self-clear trap):

- Detect divergence in `observe_realtime_steering_after_replica_update`
  (`agent-doc-controller-io/src/project_controller/rpc.rs:7833`) — already runs
  per replica_update. Compute whether the editor's pushed text resurrects ids the
  canonical's queue authority lost since the editor's last acked projection.
- Emit a typed `rebootstrap_recommended` event (NOT a forced `pending_rebootstrap`
  canonical mutation). The plugin honors it by replacing its buffer from the
  canonical projection, then re-acknowledges. Because the canonical is not
  mutated, there is no self-clear trap and no false-positive suppression of new
  prompts.
- Keep the `#replica-structure-guard` (0.35.155) and `#qdelauth` (0.35.156) as
  the hard binary-side backstops; this is advisory reconciliation for the
  content-level case only.

## What unblocks it

1. Implement + verify the Kotlin plugin re-read on the `rebootstrap_recommended`
   signal (or on buffer-vs-projection divergence) in `editors/jetbrains`.
2. Operator eyeball: in a live IDE, confirm a divergent JB buffer reconciles to
   the canonical instead of re-pushing a deleted queue line.
3. (Optional companion) the binary-side `rebootstrap_recommended` emission above.

## Regression coverage to add with the fix

- SimWorld: two editors on one document; editor A deletes a `do [#id]`; editor B
  (divergent buffer) pushes the id back; the canonical does NOT resurrect it and
  B is signaled to re-read. Today this resurrects (the bug).
- Unit: `rebootstrap_recommended` fires exactly when a replica_update resurrects
  an id the canonical lost since the pusher's last acked projection, and does not
  fire for a genuine new id or for the pusher's own re-add.
