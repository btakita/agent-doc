# Plan: #kcb5 — editor-less CLI session finalize wedge (no_ack force-disk-only)

## Symptom (live repro 2026-06-20, equityfundingsource.md)

A pure-CLI agent-doc session — Claude Code pane `%158`, **no JetBrains IDE running**,
`controller serve` (pid 2014438, 0.34.27) as the sole daemon — wedges every finalize:

- `admin list` shows the doc actor `ready` with an **active editor-IPC listener**, but
  there is no editor endpoint behind it.
- Every finalize: socket `ack status: error` → file-IPC fallback writes a patch no editor
  applies → `no_ack` → `recovery=retry_without_disk_write` → fail-closed
  `"refusing direct disk write; editor IPC listener active"`.
- Only `finalize --force-disk` succeeds (commits land via CRDT merge).
- A controller-mediated `write --pending-add` into the bugs doc wedges identically
  (`reason=send_failed`) — **the bug blocks capturing itself**.
- Never self-heals: actor not orphaned + binary not stale, so neither M1b orphan-reap,
  drain-lease expiry, nor stale-binary recycle fires.

## Root cause

`agent_doc_orchestration::ipc_socket::is_listener_active(project_root)` returns `true`
whenever the editor-IPC socket is **connectable** (`try_connect` succeeds). After the 08b
in-process-supervisor cutover the **controller** hosts that socket even when no JB plugin
is attached, so a CLI-only session has a connectable socket with **no live editor endpoint
behind it**. The finalize/converge fail-closed guard in `write/converge.rs` keys off
`is_listener_active` alone, so it refuses the disk write (`reason=no_ack_content` /
`no_component_delta`) and wedges, even though there is no editor buffer to protect.

The guard's purpose is correct (never clobber a live editor buffer). The defect is that
"socket connectable" ≠ "live editor present." We need an **editor-presence** signal
distinct from socket connectability, and the fail-closed branch must route to a
controller-host disk write when no editor endpoint exists.

## Relationship to existing items

- **`#fbwire` / `#fullboundary`** already ships a degraded-force-disk decision core
  (`should_force_disk_fallback_for_degraded_ipc`: `listener_nominally_active &&
  consecutive_failures >= threshold`). `#kcb5` is the editor-less case of the same family:
  with no editor, *every* send `no_ack`s, so the degraded threshold is always met — the gap
  is that this path is not consulted before the finalize fail-closed in the CLI-only actor.
- Extends `#cyh0`/`#fcczero` (the FCC/disk-write-guard family), `#wd40` (self-heal gaps),
  `#rt83` (queue_control_paused dispatch flood observed ~2/sec on this doc + an orphaned
  `save-document.signal` patch), and `#editorbufwin` (editor-vs-disk authority).

## Fix direction

Two complementary levers (pick the safest that the live repro validates):

1. **Editor-presence detection** — add a signal that distinguishes "a JB plugin editor
   consumer is registered for this doc" from "the controller socket is merely connectable."
   Candidate sources: the `#8bfz` single-owner consumer lease / consumer registration files
   under `.agent-doc/`, or an explicit `editor_endpoint` ack field the plugin sets and the
   controller does NOT. When no editor endpoint is proven, the disk-write guard must not arm.

2. **Degraded-force-disk wiring** — when the listener is nominally active but a bounded run
   of consecutive `no_ack`/`send_failed`/`error` acks proves no delivery (reuse
   `should_force_disk_fallback_for_degraded_ipc`), route finalize to the controller-host
   disk write (`record_force_disk_fallback`) instead of the `retry_without_disk_write`
   wedge — for a CLI-only actor this converges on the first cycle since every send fails.

Safety invariant preserved: an actor with a **proven live editor endpoint** must STILL
fail closed on `no_ack` (the existing `#editorbufwin` / FCC protection). Only the
editor-less / proven-no-delivery case routes to disk.

## Phases

- **Phase 1 (`#kcb5a`, seam-isolated, safe to land now)** — pure decision primitive
  `decide_editorless_disk_fallback(socket_connectable, editor_endpoint_proven,
  consecutive_no_ack, threshold, force_disk_requested) -> {FailClosed | ForceDiskNoEditor |
  ConvergeViaEditor}` in `agent-doc-orchestration::flow::document_mutation` (or `write/converge.rs`),
  with exhaustive unit + SimWorld coverage. No live-path rewire — groundwork only.
- **Phase 2 (`#kcb5b`, editor-presence signal)** — implement `editor_endpoint_proven`
  (consumer-lease / registration probe, or ack-field), exposed where `converge.rs` can read it.
  SimWorld coverage for present vs absent.
- **Phase 3 (`#kcb5c`, live wiring)** — consult Phase 1 decision in the finalize/converge
  fail-closed branch for a CLI-only actor; on `ForceDiskNoEditor` do the controller-host disk
  write + `record_force_disk_fallback`; emit an ops.log marker
  (`editorless_disk_fallback decision=... #kcb5`). Coordinate with `#fbwire` to avoid a
  double force-disk path.
- **Phase 4 (`#kcb5-verify`, operator-gated)** — editor-less live repro: a CLI-only session
  (no JB IDE) must finalize WITHOUT `--force-disk` and without the no_ack wedge; ops.log shows
  `editorless_disk_fallback decision=force_disk_no_editor #kcb5`; a session WITH a live editor
  must still fail closed on no_ack (no regression of the editor-buffer guard).

## Test strategy

- Unit: `decide_editorless_disk_fallback` truth table (all branches incl. force_disk override
  and proven-editor-still-fails-closed).
- SimWorld: model a CLI-only actor (connectable socket, no editor) draining a finalize and
  asserting it routes to disk; model a live-editor actor and assert it still fail-closes.
- Live (Phase 4, operator): the editor-less repro above + the no-regression check.
