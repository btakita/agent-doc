# JB IPC Accept-Loop Wedge Plan (`#jbacceptwedge`)

## Problem

Live IntelliJ sessions reported `IntelliJ plugin owns ipc.sock but has 22
unaccepted connections` during queue drains. Each `agent-doc write` (every
finalize, every queue consume, every reposition) opens a fresh Unix domain
socket connection to `.agent-doc/ipc.sock`; while connections sat unaccepted
in the socket backlog every new IPC attempt timed out and the binary fell
through to the file-IPC fallback, stalling queue drains and CRDT write-back.

## Hypothesis

The plugin-side listener is the FFI listener (`agent_doc_start_ipc_listener`
in `agent-doc-orchestration/src/ipc_socket.rs`) that the JetBrains plugin
starts via JNA in `PatchWatcher.kt`. Its `start_listener` accept loop was
single-threaded and synchronous: it blocked on the apply handler (`handler(trimmed)`,
which can do disk I/O + CRDT merge) before looping back to `accept()`. While
the handler blocked, new connections piled up in the socket backlog.

## Implementation

1. **Per-connection thread (root-cause fix — DONE, `86c68db7`).** Spawn one
   thread per accepted connection so the accept loop returns to `accept()`
   immediately and the backlog can never build up. The handler only captures
   an `extern "C" fn` pointer (`Send + Sync`), so sharing it across threads
   via `Arc` is sound; the FFI handler bound gained `Sync`.
2. **Live ops.log proof (DONE this cycle).** Each accepted connection now
   records `[ipc-socket] ipc_accept_thread_spawned inflight=N` to ops.log via
   `crate::ops_log::log_op`. An `InflightConnectionGuard` decrements the
   counter on thread exit so a panicking handler still releases its slot.
   `inflight>=2` is impossible under the old single-threaded loop, so any
   such entry proves the per-connection path is exercising concurrency.
3. **Regression (DONE this cycle).** `slow_handler_does_not_block_concurrent_connections`
   now also samples `inflight_connection_handlers()` while 3 concurrent slow
   sends are in flight and asserts peak `>= 2`, so the test fails if the
   per-connection path regresses toward the sequential shape even if wall
   clock stays under budget.
4. **Editor SPEC invariant (DONE this cycle).** `editors/SPEC.md` records
   that the shared FFI listener must service each connection on a fresh
   thread and that IntelliJ + VS Code share the same non-wedgeable listener.
5. **No plugin Kotlin change required.** The accept loop is owned by the
   binary; the plugin only owns the per-message callback body. The plugin
   version bump (0.2.169 → 0.2.170) and fresh signed build are the
   install-deliverable for the operator.

## Acceptance

- `cargo test -p agent-doc-orchestration ipc_socket::` passes (18 tests,
  including the strengthened regression that asserts peak `inflight>=2`).
- Binary + cdylib reinstalled: `agent-doc --version` reports 0.34.1 with
  the fix live; `~/.cargo/bin/libagent_doc.so` carries the
  `ipc_accept_thread_spawned` marker.
- Signed plugin build ready:
  `editors/jetbrains/build/distributions/agent-doc-jetbrains-0.2.170-signed.zip`.
- **Operator gate (live IntelliJ install + drive):** install the signed
  zip + restart IntelliJ, drive a normal queue drain or any turn that
  emits concurrent IPC (queue consume while a CRDT write is in flight),
  then confirm `.agent-doc/logs/ops.log` shows
  `[ipc-socket] ipc_accept_thread_spawned inflight=2` (or higher) with no
  socket backlog / IPC timeout diagnostics. That single grep both proves
  the per-connection path is live and exercises concurrency the old loop
  could never reach.

## Files

- `agent-doc-orchestration/src/ipc_socket.rs` — per-connection-thread
  listener + `INFLIGHT_CONNECTION_HANDLERS` + `InflightConnectionGuard` +
  `ipc_accept_thread_ops_marker` / `inflight_connection_handlers` helpers
  + strengthened regression test.
- `editors/SPEC.md` — per-connection listener-thread invariant
  (`#jbacceptwedge`).
- `editors/jetbrains/gradle.properties` — `pluginVersion` 0.2.169 → 0.2.170.
