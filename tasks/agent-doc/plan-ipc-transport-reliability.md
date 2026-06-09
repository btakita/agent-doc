# Plan: agent-doc IPC transport reliability (Plugin-insertion primary, fallback rare)

Source: queue item "investigate monsterrodholders.md + this session... fix IPC drift... less IPC and more Plugin insertions... too many File Cache Conflict... Plugin insertion reliable, IPC fallback extremely rare."

## Root-cause map (one defect, three surfaces)

The keystone is the **IPC degrade/timeout state machine** in `agent-doc-orchestration/src/write.rs` plus the sender timeout in `agent-doc-orchestration/src/ipc_socket.rs`:

1. **Degrade latch (`IPC_DEWEDGE_TIMEOUT_THRESHOLD = 2`)** marks a session "degraded" after 2 ack timeouts, after which every write skips the socket and writes to disk, with **no in-session recovery** (it only cleared on a new `session_id`).
2. **False degrade votes** fed that latch:
   - A *confirmed* socket delivery (`Ok(Some(_ack))`) whose **content sidecar** was merely slow (plugin writes it after `saveDocument`, can lag past the 200ms poll budget) was counted as an ack timeout.
   - The sender's **2s** ack budget was below the plugin's **5s** `agent_doc_await_idle` typing-debounce cap, so a busy/typing plugin tripped a false "timeout".
3. **Disk writes manufacture "File Cache Conflict"**: once degraded → raw disk writes → IntelliJ raises memory-vs-disk conflict on the open unsaved buffer (`PatchWatcher.kt` `myConflictResolver.hasConflict`). So #3 is downstream of the degrade latch.
4. The visible-write drift block (#1, `#ipc-crdt-response-drift`) is coupled via the same disk-vs-editor race; the CRDT family was heavily addressed 2026-06-08 (`c9d276b9`, `51b9ed40`, `6b5bba64`, `37783c89`, `b9c3408c`, `#op-scoped-drift-*`) — only the provenance residual remains.

## Phase 1 — DONE (this turn, commit pending)

`#ipc-degrade-false-vote` + `#ipc-ack-timeout-align` + `#ipc-degrade-self-heal`:
- Slow-but-confirmed sidecar no longer votes toward the degrade latch (`write.rs` socket success branch) — recovers via the file-IPC patch queue (still the plugin path), never a raw disk write.
- Sender ack timeout raised 2s → 6s (`ipc_socket.rs IPC_ACK_TIMEOUT_SECS`), above the plugin's 5s await-idle cap; `send_message` connects first so a dead listener still fails fast. Matcher `is_socket_ack_timeout_error` made duration-agnostic.
- Degrade latch is now a self-healing circuit breaker: `ipc_direct_disk_degraded` re-probes `is_listener_active` and clears the marker the moment the plugin socket accepts connections again.
- Tests: `degraded_latch_self_heals_when_listener_recovers`, `is_socket_ack_timeout_error_is_duration_agnostic`; existing degrade tests still green.

## Phase 2 — early-ack protocol (`#ipc-early-ack`) [follow-up]

Decouple the sender's liveness probe from the plugin's apply latency. The plugin should send an early `accepted`/`pending` ack the moment it receives the patch (before `awaitIdleBeforeDocumentMutation`), then a terminal ack/sidecar. FFI-first: define the early-ack message in the `ipc_socket` protocol + `ffi.rs` listener contract, then emit it from `PatchWatcher.handleSocketMessageV2`. Removes the timeout/apply-window coupling entirely.

## Phase 3 — prefer file-IPC over raw disk when degraded (`#ipc-degraded-prefers-file-ipc`) [follow-up]

Even in the rare genuine-degrade case, route writes through the file-watch patch queue (the plugin still applies via Document API) instead of a raw disk write, so a degraded session never manufactures File Cache Conflicts. Disk write becomes the true last resort.

## Phase 4 — visible-write provenance (`#ipc-drift-writeprovenance`) — DONE 2026-06-09

Tag agent-doc's own disk writes with a write-provenance / generation id so `guard_visible_write_reconcile` / `live_buffer_diverges_from_content` can positively attribute a foreign disk change to the supervisor instead of inferring it from the `LIVE_BUFFER_STALE_SKEW_MS` mtime heuristic. Builds on `#op-scoped-drift-*` op provenance; eliminates the residual fail-closed churn the bounded `#ipc-drift-visbuf-reconcile` retry currently papers over.

**Landed:** The producer/consumer halves were already in place from `#pcp2`/`#pcp6` (`debounce.rs::record_write_provenance`/`write_provenance`, provenance attribution in `live_buffer_diverges_from_content`, and `write.rs::atomic_write` stamping via `record_document_write_provenance`). The residual gap was that the **direct-run** document-write path (`run.rs::atomic_write`, used by the four `direct_run_*` writes) did NOT stamp provenance, so a direct-exec foreign-looking disk change fell back to the mtime heuristic. Fix: shared the one `record_document_write_provenance` recorder (`pub(crate)`) so `run.rs::atomic_write` stamps identically. Now every agent-doc document-write path stamps provenance uniformly. Test: `run.rs::direct_run_atomic_write_records_provenance`. Spec: `specs/08b` (uniform-enforcement note) + `specs/process-topology.md` (R4 ✅, shared-recorder edge).
