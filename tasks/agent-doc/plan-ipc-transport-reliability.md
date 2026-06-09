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

## Phase 2 — early-ack protocol (`#ipc-early-ack` / `#saev`) — RUST HALF LANDED (dormant), 2026-06-09

Decouple the sender's liveness probe from the plugin's apply latency. The plugin should send an early `accepted`/`pending` ack the moment it receives the patch (before `awaitIdleBeforeDocumentMutation`), then a terminal ack/sidecar. FFI-first: define the early-ack message in the `ipc_socket` protocol + `ffi.rs` listener contract, then emit it from `PatchWatcher.handleSocketMessageV2`. Removes the timeout/apply-window coupling entirely.

**Landed (Rust transport, dormant):** The whole two-phase handshake lives in the Rust transport, so **no Kotlin change is required** — `ipc_socket::start_listener` emits the early `pending` ack on patch receipt (before the blocking apply callback) whenever the incoming patch carries `"early_ack": true`, and `send_message` reads acks in a loop where `AckClassification::Pending` means "listener alive, keep waiting for the terminal ack" (never a false success — the terminal ack is still required). New helpers `message_requests_early_ack` / `early_ack_line` / `early_ack_tagged_message`. Backward/skew-safe across cdylib/CLI version drift via the opt-in `early_ack` message flag: an un-opted sender gets a single terminal ack and the loop runs exactly once (byte-identical to before). Tests: `classify_ack_treats_pending_status_as_pending`, `message_requests_early_ack_reads_flag`, `early_ack_tagging_is_dormant_by_default`, `send_message_handles_early_then_terminal_ack`. Spec `specs/process-topology.md` R2; ffi doc note.

**Gated remainder (`#xkpf`):** flip `EARLY_ACK_ENABLED` to `true` (auto-tags closeout patches) and verify end-to-end in a live IntelliJ/Codex session under real typing load that the early ack eliminates the false ack-timeout / degrade vote without regressing the terminal-ack path. Optionally `lib-install` the cdylib so the live plugin's listener emits early acks. Activating it is strictly safer (liveness-only), so this is verification, not redesign.

## Phase 3 — prefer file-IPC over raw disk when degraded (`#ipc-degraded-prefers-file-ipc`) — DONE 2026-06-09

Even in the rare genuine-degrade case, route writes through the file-watch patch queue (the plugin still applies via Document API) instead of a raw disk write, so a degraded session never manufactures File Cache Conflicts. Disk write becomes the true last resort.

**Landed:** A latched-degraded socket means only the plugin's *socket* listener is wedged — the file-IPC patch queue uses a separate plugin file watcher that is very likely still alive. `try_ipc` (`write.rs`) no longer returns straight to a raw disk write on degrade: it skips only the wedged socket attempt and falls through to the file-IPC patch queue (`write_ipc_and_poll`), where the plugin applies the patch via the Document API — no foreign disk write, no File Cache Conflict. The socket-ack-timeout arm that *newly* latches degraded likewise falls through to file IPC instead of returning. `run_stream` now always calls `try_ipc` when the plugin is installed (dropped the `!ipc_direct_disk_degraded` gate and its dead direct-disk branch), so the stream/finalize path inherits the same routing; the raw disk write only happens on the existing IPC-timeout (`exit 75`) last-resort path when file IPC also fails to deliver. The fire-and-forget `reposition_boundary` signal keeps its direct-disk-skip (no response body). Tests: `try_ipc_prefers_file_ipc_when_socket_degraded`, `try_ipc_degraded_succeeds_via_file_ipc_when_plugin_consumes`. Spec: `specs/process-topology.md` R3 ✅.

## Phase 4 — visible-write provenance (`#ipc-drift-writeprovenance`) — DONE 2026-06-09

Tag agent-doc's own disk writes with a write-provenance / generation id so `guard_visible_write_reconcile` / `live_buffer_diverges_from_content` can positively attribute a foreign disk change to the supervisor instead of inferring it from the `LIVE_BUFFER_STALE_SKEW_MS` mtime heuristic. Builds on `#op-scoped-drift-*` op provenance; eliminates the residual fail-closed churn the bounded `#ipc-drift-visbuf-reconcile` retry currently papers over.

**Landed:** The producer/consumer halves were already in place from `#pcp2`/`#pcp6` (`debounce.rs::record_write_provenance`/`write_provenance`, provenance attribution in `live_buffer_diverges_from_content`, and `write.rs::atomic_write` stamping via `record_document_write_provenance`). The residual gap was that the **direct-run** document-write path (`run.rs::atomic_write`, used by the four `direct_run_*` writes) did NOT stamp provenance, so a direct-exec foreign-looking disk change fell back to the mtime heuristic. Fix: shared the one `record_document_write_provenance` recorder (`pub(crate)`) so `run.rs::atomic_write` stamps identically. Now every agent-doc document-write path stamps provenance uniformly. Test: `run.rs::direct_run_atomic_write_records_provenance`. Spec: `specs/08b` (uniform-enforcement note) + `specs/process-topology.md` (R4 ✅, shared-recorder edge).
