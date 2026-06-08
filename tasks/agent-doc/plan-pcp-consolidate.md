# Plan: Consolidate document write/watch authority into the Project Control Pane (#pcp-consolidate)

Source backlog item: `#pcp-consolidate` — "Consolidate document-state authority into the
Project Control Pane process: make the supervisor the single disk writer + single file-watcher,
demote the plugin WatchService to read-only buffer-state reporting (no autonomous
reconcile/write), and reduce the FFI socket listener to a thin EDT apply+ack shim. Serialize
supervisor idle-queue/file-watch writes and agent finalize writes through one in-process write
queue to kill the supervisor self-race. Depends on/absorbs `#ipc-drift-writeprovenance`
(provenance/generation id) + `#nm1x-livebuf-content` (buffer content in sidecar) so drift is
positively classified instead of inferred from the `LIVE_BUFFER_STALE_SKEW_MS` mtime heuristic."

This is the **capstone** that unifies three already-planned threads. It is a multi-surface,
multi-phase refactor (Rust orchestration + FFI + JetBrains/VS Code plugins + specs) and must NOT
be landed in a single cycle — especially not in the same cycle that persists a live dogfooding
response through the very write path being changed. Each phase below is independently shippable
and verifiable.

## Why one capstone

The "File Cache Conflict" / IPC drift / supervisor self-race family (queued as R1–R6) all reduce
to **two independent disk-write authorities with no in-process serialization and no positive
write provenance**:

- The binary's finalize/stream **disk fallback** (`write.rs` → `atomic_write`) and the
  supervisor's **idle-queue/file-watch** writes can both touch the `.md` with only a per-operation
  advisory `flock` between them — not an ordered queue (R6 self-race → exit 75 "could not drain
  the active closeout").
- The JetBrains **WatchService** (`PatchWatcher.kt`) and the binary's disk writer both observe and
  both write the document, and the plugin's `saveDocument` races the disk fallback (R1
  two-watchers/two-writers → IntelliJ "File Cache Conflict").
- `live_buffer_diverges_from_content` infers "foreign disk write vs genuine unsaved editor edit"
  from the `LIVE_BUFFER_STALE_SKEW_MS` **mtime heuristic only** — no write provenance, no buffer
  content — so a real concurrent edit fails closed (`live_prompt_drift_after_preflight`, R4).

The fix is structural: **one writer, one watcher, one ordered write queue, positively-tagged
writes**, with the plugin demoted to a read-only buffer reporter + thin apply/ack shim.

## Relationship to existing specs/plans (do not duplicate)

- `specs/08b-single-process-control-plane.md` — target control plane. The **store actor** already
  owns SQLite as the single durable-state writer; **session actors** already own closeout/cycle
  state. This plan extends that contract to the **`.md` document disk write + filesystem watch**
  (not yet nailed down in 08b) and the plugin demotion.
- `plan-project-controller-session-actor-mvp.md`, `plan-controller-singleton-zero-downtime-handoff.md`,
  `plan-controller-admin-backpressure.md`, `plan-single-owner-session-actor-transition.md`,
  `plan-sqlite-session-store-concurrency.md` — the controller process substrate this builds on.
- `plan-ipc-transport-reliability.md` — Phase 1 (degrade self-heal / ack-timeout align) is DONE;
  its Phases 2–4 (early-ack, degraded-prefers-file-IPC, write-provenance) are **absorbed** here as
  `#pcp5`/`#pcp2`.
- `plan-operation-scoped-drift.md` — the `OpActor`/`OpSource`/`CausalClock`/`TurnScope` substrate
  (`agent-doc-core/src/op_log.rs`, `turn_scope.rs`) that `#pcp2` provenance tagging plugs into.
  `#nm1x-livebuf-content` is absorbed here as `#pcp6`.

## Architecture map (current, as of 2026-06-08)

| Concern | Current owner | File(s) |
|---|---|---|
| `.md` disk write | binary finalize/stream/run + repair disk fallback (`atomic_write`), guarded only by per-op `flock` | `agent-doc-orchestration/src/write.rs`, `stream.rs`, `repair.rs` |
| Supervisor writes | idle-queue / file-watch finalize (spec'd, partially landed) | `agent-doc-orchestration/src/supervisor/`, `project_controller.rs` |
| File watch (binary) | `notify`-based typing/idle indicators | `src/ffi.rs`, `src/notify.rs` |
| File watch (plugin) | JetBrains `WatchService` per project root | `editors/jetbrains/.../PatchWatcher.kt` |
| Patch apply + ack | plugin Document API apply, delete-file / socket ack | `PatchWatcher.kt`, `ipc_socket.rs`, `ffi.rs` (`agent_doc_start_ipc_listener[_v2]`) |
| Write provenance | op-log actor/source/clock in core; **NOT** stamped on disk writes | `agent-doc-core/src/op_log.rs`, `turn_scope.rs` |
| Drift classification | mtime heuristic (`LIVE_BUFFER_STALE_SKEW_MS`) + live-buffer len/hash sidecar | `write.rs` (`guard_visible_write_reconcile`), `debounce.rs` (`live_buffer_diverges_from_content`) |

## Target contract (to be specified in 08b under "Document write and watch authority")

1. The **session actor** (in the controller process) is the single disk writer for its `.md`. All
   `write`/`stream`/`finalize`/repair disk writes, and all supervisor idle/file-watch writes, are
   submitted to that actor and serialized through **one in-process ordered write queue**. The
   per-operation `flock` remains a cross-process backstop, not the primary serializer.
2. The controller owns **one filesystem watcher** per document; the JetBrains/VS Code WatchService
   is demoted to **read-only buffer-state reporting** (it reports buffer content + version via the
   existing typing/digest FFI; it does not autonomously reconcile or write the document).
3. Every binary disk write is stamped with **write provenance** (generation id + `OpActor`) so
   `live_buffer_diverges_from_content` positively attributes a foreign disk change to the
   controller/supervisor instead of inferring from mtime.
4. The FFI socket listener / plugin is reduced to a **thin EDT apply+ack shim**: receive patch →
   apply via Document API → early-ack on receipt → terminal ack — no independent reconcile, no
   disk write.

## Phases (each independently shippable + verifiable)

Dependency order: provenance → write queue → single watcher → early-ack/file-IPC → buffer content
→ plugin demotion. The pure-Rust phases (`#pcp1`–`#pcp4`, `#pcp5`) are SimWorld/unit-testable; the
plugin phases (`#pcp6`, `#pcp7`) require live IntelliJ/VS Code verification and land last.

### `#pcp1` — Spec the document write/watch authority
Add a "Document write and watch authority" section to `specs/08b-single-process-control-plane.md`
(single writer, single watcher, ordered write queue, write provenance, plugin demotion) with
SimWorld verification obligations. Pair the spec with the first code phase's tests. Spec-only;
no behavior change.

### `#pcp2` — Write-provenance / generation id on disk writes (absorbs `#ipc-drift-writeprovenance`)
Stamp each binary disk write with a generation id + `OpActor` provenance marker (sidecar or
extended live-buffer digest) and have `guard_visible_write_reconcile` /
`live_buffer_diverges_from_content` read it. **Shadow mode first**: log provenance and prefer it,
but keep the `LIVE_BUFFER_STALE_SKEW_MS` mtime heuristic as fallback so the change is additive and
cannot wedge the live finalize path. Pure Rust; builds on `op_log.rs`/`turn_scope.rs`.
Tests: provenance round-trips through a foreign-supervisor write; a genuine unsaved editor edit is
no longer misclassified as a foreign write.

### `#pcp3` — In-process ordered write queue in the session actor (kills R6 self-race)
Serialize supervisor idle-queue/file-watch writes and agent finalize writes through one
controller-owned ordered queue. Eliminates the "could not drain the active closeout" / exit 75
self-race. SimWorld coverage: concurrent supervisor-write + agent-finalize on the same document
serialize deterministically; finalize never observes a half-applied supervisor write.

### `#pcp4` — Single controller-owned filesystem watcher (kills R1 two-watchers)
Replace the per-process `notify` watchers with one controller-owned watcher feeding the session
actor. Binary-side first (before plugin demotion). SimWorld: a single watcher event stream drives
typing/idle classification; no duplicate-watcher reconcile races.

### `#pcp5` — Early-ack IPC + degraded-prefers-file-IPC (absorbs `#ipc-early-ack`, `#ipc-degraded-prefers-file-ipc`)
FFI-first: plugin sends an `accepted/pending` ack on patch receipt (before
`awaitIdleBeforeDocumentMutation`), then a terminal ack — decoupling sender liveness from apply
latency. When genuinely degraded, route writes through the file-IPC patch queue (plugin still
applies via Document API) instead of a raw disk write, so a degraded session never manufactures a
File Cache Conflict (R3). Define the message in `ipc_socket` + `ffi.rs`, emit from
`PatchWatcher.handleSocketMessageV2`.

### `#pcp6` — Capture editor-buffer CONTENT in the live-buffer sidecar (absorbs `#nm1x-livebuf-content`)
Write buffer text (not just len/hash) into the `.agent-doc/live-buffer` digest so
`guard_visible_write_reconcile` can scope-classify a GENUINE unsaved editor edit (editor ahead of
disk) against the `TurnScope` instead of failing closed (R4). JB + VS Code plugin change (write
buffer text into the digest) + FFI-side classifier read. **Requires live editor verification.**

### `#pcp7` — Demote plugin WatchService to read-only; thin FFI apply+ack shim (R1/R2/R5)
Demote the JetBrains/VS Code WatchService to read-only buffer-state reporting (no autonomous
reconcile/write), and reduce the FFI socket listener to a thin EDT apply+ack shim. Closes the
TOCTOU window (R5) between the FFI socket handler and the WatchService for the same patch.
**Requires live IntelliJ/Codex verification** (pairs with the existing `#live-verify-batch`
review item). Bump JB `pluginVersion` per `.kt` edit; ship the cdylib with `agent-doc lib-install`.

## Verification obligations (per spec 08b § Verification)

Deterministic SimWorld coverage must model, at minimum:
- concurrent supervisor-write + agent-finalize serialized through the single write queue (`#pcp3`);
- a foreign-supervisor disk write positively attributed by provenance, and a genuine unsaved
  editor edit NOT misclassified as foreign (`#pcp2`, `#pcp6`);
- a single watcher event stream with no duplicate-watcher reconcile (`#pcp4`);
- early-ack decoupling apply latency from sender liveness; degraded session routing through
  file-IPC, never a raw disk write (`#pcp5`).
Live tmux/editor coverage remains a smoke layer for the external pane/editor boundary (`#pcp6`,
`#pcp7`).

## Rollout / safety

- Follow the 08b migration gates (shadow → dual-write → read-switch → authority → removal) for the
  write-authority move; every gate must have a rollback flag.
- Never land a phase that changes the live finalize path in the same cycle that persists the
  session response through it; verify each Rust phase with `make check` + a fresh `cargo install`
  + `cargo build --release`, and each plugin phase against a live editor before claiming the fix is
  live.
