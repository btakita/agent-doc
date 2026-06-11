# Plan — Realtime editor watcher → durable document model (#rtwatch)

## Problem (operator-stated, live-reproduced 2026-06-11)

The agent-doc document model lags the live editor buffer. The IDEA buffer holds
unsaved edits and is treated as `content_ours`-authoritative over IPC, but the
binary's cycle operates on the **disk file + preflight snapshot/baseline**, which
can be staler than the buffer. Result: agent-side writes (finalize, write,
queue/backlog mutations) compute against a stale view and **clobber legitimate
user queue/exchange content** that exists only in the editor buffer.

This is the unifying root cause behind:
- `#queue-user-edit-overwrite` — content_ours merge drops user queue edits.
- The `test test` failure (2026-06-11): session-check fail-closed on `test test`
  as a "dropped user queue edit," but the operator had deliberately deleted it;
  the lagging model re-added it and momentarily replaced the legitimate queue.
- `#ipcdrift` / `#sjwm` — multi-owner race where the editor buffer and the
  agent/controller write paths disagree on current document state.

Operator directive (verbatim intent): "Our document model must be realtime with
the watcher. Any update that occurs should update the model in realtime... the
editor has buffer state and does not save to disk right away... bring back the
editor watcher to update the realtime model. Can we make both watchers durable?
Where the editor watcher is often more recent than the file on disk?... This
system needs to act real-time... These updates should not cause performance
issues with the rest of the project control pane."

## Operator refinements (2026-06-11, live)

- **"Reliable", not just "durable".** The pre-CPC dual-watcher implementation had
  too many `File Cache Conflict`s. The target is **zero** File Cache Conflicts —
  reliability is the acceptance bar, not merely surviving restart.
- **Sync before apply.** The realtime document model must be **in sync with the
  editor buffer (or disk when no editor has the file open) BEFORE applying
  changes.** Reconcile-then-write, never write-against-stale.
- **Multi-editor CRDT broadcast (expands rung 2/3).** If the file changes on disk
  OR another editor's buffer changes it, the realtime model updates and the
  change is **broadcast to all editor buffers and merged via CRDT**. With the
  same agent-doc open in JB and VS Code, editing either buffer must show in the
  other — with no `File Cache Conflict`. So the model is not just a read
  authority; it is the convergence point that fans changes back out to every
  open buffer.

## Design (FFI-first, per CLAUDE.md Shared Foundation pattern)

1. **Editor watcher (realtime model feed).** The IDEA/VS Code plugin already
   emits buffer-change events over IPC. Bring back a durable editor watcher in
   the FFI layer (`ffi.rs` / `ipc_socket.rs`) that ingests buffer deltas into a
   per-document realtime model (the CRDT in `crdt.rs`) on every change, not only
   on save. The editor buffer becomes a first-class, durable model input.

2. **Two durable watchers, reconciled by recency.** Keep the file watcher
   (`watch.rs`) AND the editor watcher. When the editor has unsaved state, the
   editor watcher's view is **more recent** than disk and wins for read/merge;
   when the buffer is saved/closed, disk is authoritative. Make both durable
   (survive controller restart) with a recency/version stamp so the merge picks
   the newest, instead of the current snapshot-vs-disk guess that clobbers.

3. **Cycle reads the realtime model, not stale disk.** preflight / write /
   finalize / session-check must source "current document" from the realtime
   model (newest of disk vs editor buffer) so agent writes never compute against
   a view staler than what the user sees. Removes the `content_ours` clobber and
   the `buffer differs from disk` write-block whack-a-mole.

4. **Perf-bounded.** Debounce editor deltas; keep ingestion off the
   project-control-pane hot path (separate task/queue), bounded work per event.
   No synchronous model rebuilds on the control-pane thread.

## Rungs (independently shippable, per the `document_watcher.rs` #pcpc4 precedent)

- **Rung 1 — read-authority primitive (LANDED 2026-06-11).** New deterministic,
  seam-isolated module `agent-doc-orchestration/src/realtime_model.rs`:
  `reconcile_current_doc(disk, Option<&BufferState>) -> Reconciliation` plus
  `current_doc(...)` and `buffer_supersedes(prev, next)`. Implements design
  point 2's authority decision — editor buffer wins while it holds **unsaved
  (dirty)** edits, disk wins when saved/absent, clean-buffer-but-disk-differs
  flags `diverged`. 6 deterministic evals (incl. the `#queue-user-edit-overwrite`
  no-clobber scenario). `make check` green; built + installed. No live editor
  needed — pure function, behind the seam, not yet wired into the cycle, so it
  cannot affect this turn's own closeout. This is the hard/risky read-authority
  logic; the remaining rungs are the mechanical wiring + the buffer feed.

- **Rung 2 — feed the durable editor-buffer snapshot (`#rtwfeed`).** Source a
  `BufferState { content, dirty, generation }` from the socket IPC layer
  (`ipc_socket.rs` / `ffi.rs`): the plugin already reports buffer state over IPC;
  expose a query that returns the current buffer content + dirty flag +
  monotonic generation, debounced and off the project-control-pane hot path
  (design points 1 + 4). Durable across controller restart. NEEDS live verify.

- **Rung 3 — wire the cycle read sites (`#rtwwire`).** Replace the direct
  `std::fs::read_to_string(file)` "current document" reads at the cycle entry
  points (`preflight.rs`, `write.rs`, `session_check.rs`) with
  `realtime_model::current_doc(disk, buffer)` so the agent reads
  newest-of(disk, buffer) (design point 3). This is the rung that removes the
  `differs from disk` write-block whack-a-mole — and the rung that touches the
  closeout path, so it MUST run in a dedicated stable turn with live
  edit-during-finalize verification (cannot be done in an affected session).

## Verification (rungs 2–3 need a dedicated stable turn — CANNOT be done in the affected session)

- Deterministic SimWorld coverage: editor-buffer-newer-than-disk → model reflects
  buffer; agent write does not drop buffer-only queue items; saved buffer → disk
  authoritative.
- Live: type a queue item in IDEA without saving, run a finalize cycle, confirm
  the item survives (no `#queue-user-edit-overwrite`).
- This turn is itself the live repro: every agent write got clobbered or blocked
  by the live buffer. Fix + verify must run where the buffer does not race the
  writer (doc closed in IDEA, or a stable dedicated turn).

## Files (anticipated)

- `ffi.rs`, `ipc_socket.rs` — editor watcher ingestion + durable realtime model.
- `crdt.rs`, `merge.rs` — recency-stamped editor-vs-disk reconciliation.
- `watch.rs` — keep file watcher; coordinate durability with editor watcher.
- `preflight.rs`, `write.rs`, `template.rs` — read current doc from realtime model.
- `editors/jetbrains/`, `editors/vscode/` — thin buffer-change emit (already exists; verify durability + reconnect).
- SimWorld tests for the reconciliation + no-clobber invariants.
