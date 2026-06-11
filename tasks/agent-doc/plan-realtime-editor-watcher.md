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

- **Rung 2 — feed the durable editor-buffer snapshot (`#rtwfeed`, LANDED
  2026-06-11).** Instead of a *new* IPC query, the durable buffer feed already
  exists: the plugin persists the editor's full buffer text on every change to
  the `.agent-doc/live-buffer/<hash>` sidecar (`debounce::LiveBufferSnapshot`,
  `#pcp6` content path), and `debounce::live_buffer_diverges_from_content` is the
  hard-won, staleness-suppressing classifier (`#pcp2` write-provenance + `#pcp6`
  content-match + mtime skew) that proves whether the editor holds genuine
  *unsaved edits ahead of disk*. Rung 2 is the bridge that promotes that durable
  sidecar to a `BufferState` **only** through that classifier:
  `realtime_model::durable_buffer_state(file, disk) -> Option<BufferState>` plus
  the single cycle entry point `realtime_model::resolve_current_doc(file, disk)
  -> Reconciliation` (read disk + reconcile against the gated feed + emit a
  `realtime_doc_resolve authority=… reason=… diverged=…` ops.log marker). Gating
  on the classifier — not the raw `dirty`/`version` digest — is the safety
  property: a stale buffer or agent-doc's own just-written disk content can never
  win, so this cannot clobber-in-reverse. Path keyed via the same canonicalize
  convention as `guard_visible_write_reconcile` so the sidecar hash matches the
  plugin's reported absolute path. 6 new deterministic evals (pure
  divergence→BufferState mapping + FS-sidecar in-sync/ahead/no-feed cases);
  `make check` green; built + installed (+ `lib-install`). Durable across
  controller restart (reads the persisted sidecar, not in-memory state). Live
  verify still confirms the plugin keeps feeding the sidecar under real typing.

- **Rung 3 — wire the cycle read sites (`#rtwwire`).**
  - **3a — classification reads (LANDED 2026-06-11).** Rerouted the two cycle
    *classification* baselines through `realtime_model::resolve_current_doc`:
    `preflight.rs` `run_with_options` (the frontmatter/diff baseline) and
    `session_check.rs` (the `rc.set_doc_content` guard-sweep cache seed). This is
    the half that removes the `differs from disk` false-block whack-a-mole — the
    dropped-prompt / contamination guards now reason about what the user actually
    sees (newest-of disk vs unsaved buffer) instead of a staler disk view — and
    it changes **no committed output** (preflight commits the snapshot, not this
    read; guards only classify). With no editor attached (the common/CI case)
    `resolve_current_doc` returns disk unchanged, so all 2677 existing
    orchestration tests stay green. `make check` green; built + installed.
  - **3b — write-merge baselines (LANDED 2026-06-11, needs live verify).** Wired
    the two write "current document" merge baselines through
    `resolve_current_doc`: `run_ipc`'s current-doc read (the IPC/template write
    path the live JB plugin uses — its queue/exchange component patch is now
    normalized + parsed against the buffer the user sees, so a patchback cannot
    drop a buffer-only queue item) and the inline `run()` merge "theirs" read (the
    3-way merge incorporates buffer-only edits). Both staleness-gated: the buffer
    wins only when provably unsaved-ahead-of-disk, so agent-doc's own fresh write
    can never be overridden, and `guard_no_stale_snapshot_reset_drift` fails
    closed on a large unsaved deletion (recovery message) rather than clobbering.
    No editor attached returns disk unchanged → all 2677 orchestration tests
    green; `make check` green; built + installed (+ `lib-install`). Commit
    `320a21d5`. **Live verify pending** — the IPC path is exercised by the next
    `agent-doc-bugs2.md` turn (`#rtwverify` queued at its backlog top): type an
    unsaved edit in IDEA, finalize, grep ops.log for `realtime_doc_resolve
    authority=editor_buffer reason=editor_unsaved_newer` + the edit surviving the
    commit.

## Verification

### Done deterministically (rungs 1, 2, 3a) — `make check` green, 2026-06-11

- `realtime_model` evals (12): authority decision (rung 1) + durable-feed bridge
  (rung 2): editor-buffer-ahead-of-disk → feed reflects buffer; saved/in-sync
  buffer → disk authoritative; no editor feed → disk; len/hash-only digest (no
  full content) → disk (cannot fabricate buffer text); `#queue-user-edit-overwrite`
  buffer-only queue item surfaced via the feed.
- No-regression: all 2677 orchestration tests stay green because
  `resolve_current_doc` returns disk unchanged with no editor sidecar present
  (the common/CI path), so the 3a classification rewires are inert without a live
  editor.

### Remaining — operator live verify (CANNOT be agent-done; needs a live editor)

- **3a live proof:** with the new binary installed (`cargo install` + `lib-install`
  done 2026-06-11; restart the session so the supervisor picks up the new binary,
  IDE hot-reloads the cdylib by mtime), type a queue/exchange item in IDEA
  **without saving**, run `agent-doc session-check`/`preflight`, and confirm the
  guards do NOT false-block on `differs from disk`. Grep `ops.log` for
  `realtime_doc_resolve authority=editor_buffer reason=editor_unsaved_newer` paired
  with `live_buffer_classify decision=diverges`.
- **3b (write-merge) is still staged** — only wire the `write.rs` merge baselines
  in a dedicated stable turn, then live-verify: type a queue item in IDEA without
  saving, run a finalize cycle, confirm the item survives in the commit (no
  `#queue-user-edit-overwrite`). Fix + verify must run where the buffer does not
  race the writer (doc closed in IDEA, or a stable dedicated turn).

## Files (anticipated)

- `ffi.rs`, `ipc_socket.rs` — editor watcher ingestion + durable realtime model.
- `crdt.rs`, `merge.rs` — recency-stamped editor-vs-disk reconciliation.
- `watch.rs` — keep file watcher; coordinate durability with editor watcher.
- `preflight.rs`, `write.rs`, `template.rs` — read current doc from realtime model.
- `editors/jetbrains/`, `editors/vscode/` — thin buffer-change emit (already exists; verify durability + reconnect).
- SimWorld tests for the reconciliation + no-clobber invariants.
