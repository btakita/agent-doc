# Plan: reliable realtime cutover for agent-doc

**Source:** realtime dogfooding failures, IPC/CRDT drift plans, JetBrains undo
breakage observed on 2026-06-27, and required VS Code undo parity.

**Directive:** compatibility with the turn-based legacy is not a constraint.
Choose the fastest route to reliable realtime orchestration.

## Summary

agent-doc should stop treating a stale disk read plus preflight/finalize cycle as
the live authority. The markdown file remains the user's durable, inspectable
document. The reliable realtime shape is a project controller with a
per-document actor, typed event ledger, CRDT-backed document model, editor-safe
patch transport, and a small transaction closeout path for commits.

The current failure mode is structural:

- The legacy turn pipeline still performs heavy preflight, file-watch reruns,
  snapshot repair, queue mutation, finalize, and direct materialization as one
  large cycle.
- The editor path can time out or be considered absent, then fall back to
  visible disk writes. The latest ops log showed `run_ipc_editorless_disk_fallback`
  followed by `write_authority action=routed transport=write_queue` for an open
  JetBrains document.
- The JetBrains plugin has full-document mutation paths:
  `PromptPoller.kt` uses raw `runWriteAction { doc.setText(merged) }`, and
  `PatchWatcher.kt` uses full `document.setText(...)` plus `saveDocument(...)`
  in normal patch paths.
- The VS Code extension already rejects `fullContent` IPC, but normal component,
  reposition, reconnect, and save-signal paths historically still carried
  full-range `WorkspaceEdit.replace(...)` / save-style repair risks in
  `editors/vscode/src/extension.ts`.
- Those combinations can invalidate or degrade editor undo history. The CRDT may
  supply the content, but unsafe editor/disk mutation is the undo breaker.

## User-change supremacy invariants

This cutover must make user text impossible for agent-doc to overwrite
silently. User changes are authoritative inputs, not merge suggestions.

Non-negotiable rules:

- User changes must never be overwritten by an agent patch, repair path,
  projection materialization, IPC fallback, CRDT merge, queue closeout, or
  formatter/normalizer.
- The markdown file is still the durable user document. It is not disposable,
  hidden, or allowed to be silently regenerated from lossy state.
- Every editor buffer change and every external disk change is ingested as a
  user/provenance-bearing event before the controller emits any agent patch or
  materializes the file.
- A controller write is allowed only when it can prove the current editor/disk
  user deltas are included in the candidate document state.
- If an agent operation and a user edit touch the same region, the user edit
  wins by default. The agent operation must be rebased, moved, retried, or
  blocked.
- Unknown markdown must round-trip byte-for-byte unless the operator explicitly
  requests a transformation.
- Deletions require explicit provenance: user deletion, agent patch against a
  known base, queue completion transaction, or recovery action accepted by the
  operator.
- Agent-authored deletion provenance is valid only for text that has not changed
  since the agent's known base. If the user touched the text after that base,
  deletion is blocked until the operator accepts it.
- Repair and normalization paths may fail closed, but they may not "fix" the
  document by dropping unrecognized text.
- If the controller loses confidence, it blocks and preserves all versions
  instead of choosing one.
- Conflict recovery must be additive: write a conflict record, sidecar, or
  visible conflict block. It must not resolve conflict by overwriting user text.

Practical acceptance:

- Typing unsaved text in JetBrains or VS Code, receiving an agent patch, and
  then saving must preserve the typed text.
- Editing the markdown file externally while the controller is running must
  preserve the external edit or fail closed with both versions recoverable.
- A log scan must be able to explain why every deletion was allowed.

## Target model

### 1. Controller is the only mutable authority

Run one project controller process. It owns:

- per-document actors
- typed append-only event ledger
- CRDT document state
- editor presence
- agent session state
- closeout and commit transactions
- materialized projections

CLI commands, JetBrains, VS Code, tmux panes, file watchers, and agents become
clients. They submit events or requests; they do not directly mutate the visible
document as an authority.

Controller authority is bounded by the user-change supremacy rule. The
controller may serialize writes, but it may not choose an agent state over a
newer user state.

Use `agent-doc-orchestration/src/state_backbone.rs` as the durable backbone
direction. Keep `cycle_state.rs` only for closeout transaction compatibility
until the replacement transaction ledger is live.

### 2. Markdown is durable; realtime arbitration is controller-owned

The markdown file is no longer the sole realtime arbitration source. It remains
the durable user-facing document and the format the user can inspect and edit.
The controller owns realtime arbitration because it can see editor buffers,
disk changes, agent operations, and pending closeout state together.

When the controller materializes markdown, it is publishing a state that must
already include all known user edits. Materialization is not permission to
regenerate or discard text.

Rules:

- Editor buffer replicas can be newer than disk.
- Disk can be current only when no live dirty editor replica exists.
- Direct visible disk write is forbidden while any editor has the document open
  or recently reported a live buffer.
- Disk materialization is coalesced and delayed until a quiet window or explicit
  commit boundary.
- External disk edits are ingested as user edits before the next materialization.
- Agent writes rebase on the latest user revision. If rebase cannot prove
  preservation, the write is rejected.
- If the editor path is unavailable, fail closed with an
  `EditorConvergenceRequired` style error instead of falling back to disk.

Delete or hard-disable `run_ipc_editorless_disk_fallback` for editor-visible
documents.

### 3. Editors are CRDT replicas, not sidecar hints

Every editor connection must have:

- stable `editor_id`
- document path hash
- replica generation
- base revision
- dirty/clean status
- cursor or selection state when useful
- last acknowledged patch id

The controller holds N editor replicas per document. The current document view is
computed from the controller CRDT state, not from "disk plus maybe one live
buffer sidecar."

Existing `realtime_model.rs` was a useful read-authority bridge. Replace the
bridge role with a real replica model:

- ingest editor deltas as operations
- merge with CRDT
- broadcast targeted patches to every other open editor
- suppress echo by `editor_id`
- require per-editor ACKs
- garbage-collect dead editor presence safely

### 4. Editor writes must preserve undo

JetBrains and VS Code patch application must be changed before broader realtime
dogfooding. Undo preservation is an editor contract, not a JetBrains-only bug.

Rules:

- No raw `runWriteAction { document.setText(...) }` for realtime mutation.
- No full-document `setText` in normal patch application.
- No normal-path VS Code full-document or full-range
  `WorkspaceEdit.replace(fullRange, content)`.
- Apply minimal ranges with `insertString`, `deleteString`, or `replaceString`
  inside one named JetBrains `WriteCommandAction`.
- Apply minimal VS Code ranges through an undoable edit API
  (`WorkspaceEdit` or `TextEditor.edit`) as one logical operation.
- Do not call `saveDocument` after every realtime patch.
- Do not call VS Code `document.save()` after every realtime patch.
- Save only at explicit materialization or commit boundaries, outside active
  typing windows.
- Treat a full-document replacement as recovery-only and require an explicit
  operator action or closed editor.

Immediate code targets:

- `src/agent-doc/editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/PromptPoller.kt`
- `src/agent-doc/editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/PatchWatcher.kt`
- `src/agent-doc/editors/vscode/src/extension.ts`
- `src/agent-doc/editors/vscode/src/patchPlan.ts`
- `src/agent-doc/editors/vscode/src/patchGuard.ts`

Acceptance:

- User types in JetBrains or VS Code.
- Controller applies an agent patch.
- `Ctrl+Z` first undoes the user's own typing normally when the user's typing is
  the most recent operation.
- When the agent patch is the most recent operation, `Ctrl+Z` undoes exactly the
  agent patch without rolling the document back to an unrelated full-document
  state.
- Additional undo steps do not jump to an unrelated full-document state.
- No File Cache Conflict dialog appears.

### 5. IPC is fire-and-reconcile, not block-and-fallback

Remove synchronous "send patch, wait up to 2s, then write disk" from the hot
path.

New shape:

- Controller records an intended document operation in the ledger.
- Controller updates CRDT state.
- Controller sends targeted editor patches asynchronously.
- Editor ACKs include `editor_id`, `patch_id`, previous generation, and new
  generation.
- Missing ACKs mark the replica stale; they do not trigger direct disk fallback.
- A reconciler retries, disconnects the stale editor, or asks the operator to
  resolve convergence.

The per-document write queue remains useful only as the actor serialization
primitive. It must not be a path back to unsafe visible disk writes.

### 6. Replace heavy preflight with admission

The realtime hot path should not run the legacy preflight stack.

Create a small admission step that answers only:

- is the document actor alive?
- is there exactly one controller authority?
- is editor convergence known?
- is the requested operation allowed by current state?
- which compact projection should the agent receive?

Move these out of the hot path:

- stale actor cleanup
- GC
- layout repair
- markdown repair
- broad drift repair
- session sweep
- verbose JSON report generation
- historical warning aggregation

Those become background maintenance events owned by the controller.

Target: admission should be cheap enough to run for every realtime operation.

### 7. Turn lifecycle becomes transaction closeout

The current turn FSM should not drive realtime state. It should become a
transaction closeout primitive:

- response captured
- response applied to controller model
- queue item consumed in typed backlog state
- disk projection materialized
- git commit written
- transaction completed

Crash recovery should replay from the ledger, pending response capture, and
typed task store. It should not reconstruct truth from a markdown diff.

### 8. Typed backlog replaces markdown-as-queue authority

The queue/backlog should be a typed projection, not parsed markdown as the
primary store.

The markdown document can still render queue state for humans, but the controller
owns:

- task id
- prompt text
- status
- priority/order
- owner
- provenance
- active transaction id
- completion proof

This removes many prompt-prefix, queue-strike, duplicate scaffold, and pending
mutation classes from the document merge path.

### 9. Use operation-scoped affectedness

Use `agent-doc-core/src/turn_scope.rs` style affectedness as the live scheduler
contract:

- `Independent`: continue current agent work.
- `InputAffecting`: update the agent context or schedule a recycle.
- `OutputContended`: pause output and reconcile before applying.
- `StructuralDependency`: restart or re-admit with fresh projection.
- `ProvenanceSpoofed`: fail closed.

This replaces byte-drift panic with operation-level routing.

### 10. Token efficiency comes from projections

Agents should receive compact typed projections, not full preflight dumps and
whole session markdown.

Projection examples:

- active user request
- relevant open tasks
- current transaction state
- changed regions since last agent prompt
- convergence status
- needed file/code context handles
- closeout requirements

Do not inject broad ops logs, full JSON preflight output, raw diffs, or full
session history unless explicitly requested.

## Execution plan

### Phase 0: stop the bleeding

Goal: prevent further editor undo damage before the architecture cutover.

Changes:

- Disable direct visible disk fallback for any document with live or recent
  editor presence.
- Remove or guard `run_ipc_editorless_disk_fallback`.
- Disable raw `PromptPoller.kt` full-buffer `setText` mutation.
- Disable normal-path full-document `PatchWatcher.kt` `setText` mutation.
- Disable normal-path VS Code full-range `WorkspaceEdit.replace` mutation.
- Stop auto-saving after every realtime patch.
- Stop VS Code `document.save()` after every realtime patch.
- Add ops-log assertions that fail tests if an editor-visible path emits
  `disk_fallback`.
- Add a preservation guard: before any visible write, compare the candidate
  output against known editor/disk user deltas and block if a delta would be
  dropped without explicit deletion provenance.
- Add a same-region conflict guard: if the user has edited a region after the
  agent's base revision, the agent patch cannot replace that region.

Acceptance:

- No `run_ipc_editorless_disk_fallback` occurs while JetBrains has the document
  open.
- JetBrains undo remains functional after an agent patch.
- VS Code undo remains functional after an agent patch.
- File Cache Conflict does not appear during active typing.
- Unsaved user text survives an agent patch and later save.
- A stale agent patch against a user-edited region is rejected, not applied.

### Phase 1: editor-safe patch application

Goal: make editor mutation safe enough to dogfood again.

Changes:

- Define the patch protocol as ranges plus base revision, not full replacement
  text for the normal path.
- Add a Kotlin patch applier that computes/applies minimal document edits inside
  one `WriteCommandAction`.
- Add a VS Code patch applier that computes/applies minimal document edits
  through one undoable edit operation.
- Add generation checks before apply.
- ACK success, stale base, rejected patch, and local conflict distinctly.
- Keep full replacement only as explicit recovery.

Verification:

- JetBrains unit tests for minimal patch application.
- VS Code extension tests for minimal patch application and undo-safe save
  behavior.
- Live JetBrains test: type, receive patch, undo, redo, continue typing.
- Live VS Code test: type, receive patch, undo, redo, continue typing.
- Live test: editor dirty, controller patch arrives, no save prompt and no cache
  conflict.

### Phase 2: controller event loop

Goal: replace the legacy watcher loop with a realtime scheduler.

Changes:

- Introduce or promote a single project controller runtime.
- Create one actor per document.
- Route file watcher, editor watcher, CLI, and agent events into the actor.
- Stop `watch.rs` from calling the legacy `run::run_with_context` loop directly.
- Persist all actor decisions as typed ledger events.
- Rebuild projections from the ledger at startup.

Verification:

- Controller restart rebuilds the same document state.
- Events for document A do not block document B.
- A stale or crashed client is removed without wedging the actor.

### Phase 3: CRDT replica convergence

Goal: make realtime document state authoritative.

Changes:

- Replace single live-buffer sidecar authority with per-editor replicas.
- Ingest editor deltas into controller CRDT state.
- Broadcast merged patches to all non-origin editor replicas.
- Track per-editor ACKs and generations.
- Materialize disk from controller state only after convergence or quiet-window
  policy allows it.

Verification:

- JetBrains and VS Code open the same document.
- Typing in either editor appears in the other.
- Agent patch appears in both.
- No File Cache Conflict.
- Undo in both editors remains local and predictable.

Progress 2026-06-27:

- Implemented per-editor CRDT replica registration, update fan-out, target-owned
  pull delivery, and ACK tracking in the supervisor relay path
  (`crdt_relay`, `crdt_relay_host`, `supervisor::ipc`, `start::supervisor_io`).
- Required closeout now fails closed before commit when live MultiReplica
  delivery is still unacknowledged; best-effort commit skips materialization
  instead of committing ahead of convergence.
- JetBrains now wires a production `DocumentListener`/poller through
  `CrdtReplicaManager`, applies remote CRDT text via minimal editor edits, and
  ACKs only after editor application succeeds.
- VS Code now has native replica FFI bindings, a supervisor socket transport,
  a CRDT replica manager, open/close wiring, local delta forwarding, remote
  minimal edit application, and ACK-after-apply tests.
- Automated evidence: VS Code `npm test`; JetBrains
  `./gradlew test --tests com.github.btakita.agentdoc.CrdtReplicaForwarderTest`;
  Rust `cargo test -p agent-doc-orchestration crdt_relay --lib`,
  `cargo test -p agent-doc-orchestration crdtauth5 --lib`,
  `cargo test -p agent-doc-orchestration complete_required_closeout_blocks_until_live_replica_delivery_is_acked --lib`,
  and `cargo check -p agent-doc-orchestration`.
- Remaining Phase 3 evidence is live editor verification: two-editor
  JetBrains/VS Code convergence, no cache conflict, and undo/redo behavior under
  real IDE edit stacks.

### Phase 4: realtime admit and transaction closeout

Goal: remove turn-based preflight/finalize from the hot path.

Changes:

- Add a lightweight `admit` path for agent requests.
- Move maintenance out of admission.
- Convert finalize into closeout transaction replayable from ledger state.
- Persist response capture before processing.
- Consume typed queue items transactionally.
- Commit only after projection materialization is confirmed.

Progress 2026-06-27:

- Added `agent-doc admit <FILE>` as a lightweight admission command in the
  orchestration crate. It reads the current realtime document projection, opens a
  durable response-bound cycle checkpoint, emits JSON, and records
  `realtime_admit ... source=admit action=accepted` in `ops.log`.
- Routed direct `agent-doc run` cycle opening through the same admit helper, so
  child-agent dispatch now enters the response cycle through the lightweight
  realtime admission surface while leaving prompt construction and pre-commit
  behavior unchanged.
- The new admit path does not call the legacy preflight runner, so admission now
  has a tested no-maintenance surface separate from repair, GC, layout repair,
  queue maintenance, diff classification, claim checks, and pre-agent commit.
- Exposed `agent_doc_admit` on the MCP server and switched configured Codex
  queue-continuation instructions from `agent_doc_preflight` to
  `agent_doc_admit` before `agent_doc_plan` / `agent_doc_read` and
  `agent_doc_finalize`. The legacy MCP preflight tool remains available for
  compatibility/reporting, but the live in-pane submission path now points at the
  lightweight admission surface.
- Queue head consumption now writes typed append-only proof-ledger records around
  the mutation: a `QueueHead`/`Recorded` identity proof before any visible
  document change, followed by a `QueueHead`/`Consumed` write-result proof after
  the document and snapshot converge. The queue cleanup escape-hatch tests were
  also moved to the cutover contract: they verify pure strike planning but fail
  closed without editor convergence instead of writing directly on no-listener
  paths.
- Captured CRDT/template patchback recovery now replays through the normal
  strict stream write path instead of the repair-only template writer, so a crash
  after response capture reuses the same capture, materialization, snapshot, and
  commit closeout path as `finalize`.
- Resolved the Codex auto-queue no-listener closeout gap by carrying the
  response-derived queue completion id into captured-response replay and folding
  queue consumption into the stream writer's single materialized document +
  snapshot pair. The path records the same before/after queue proof-ledger rows
  but no longer performs a second visible queue-consume write after recovery.
- Strict committed closeout now appends a typed `TerminalProof` /
  `TerminalStateObserved` proof-ledger row after session-check and actor closeout
  succeed. The record proves the cycle id, committed phase, capture metadata,
  session-check success, actor-closeout persistence, and matching visible file /
  snapshot / HEAD hashes. Detached closeout also uses the explicit force-disk
  pending-maintenance path for terminal reap, while editor-attached closeout
  remains on guarded editor convergence.
- Crash-replay evidence now asserts the same terminal proof row for both
  crash-after-apply (`write_applied` restart recovery) and crash-before-commit
  (captured response replay on next preflight). Git-backed orphaned-response
  repair replays through the normal write/commit closeout transaction; standalone
  non-git repair fixtures keep legacy direct repair semantics.
- Automated evidence: `cargo test -p agent-doc-orchestration admit --lib`,
  `cargo test -p agent-doc-orchestration start_run_cycle_routes_through_realtime_admit --lib`,
  `cargo test -p agent-doc-orchestration queue_consume --lib`,
  `cargo test -p agent-doc mcp`,
  `cargo test -p agent-doc-orchestration configured_mcp_tools --lib`,
  `cargo test -p agent-doc-orchestration stop_marker_fallback_continuation_prefers_configured_mcp_tools --lib`,
  `cargo test -p agent-doc-orchestration complete_required_closeout_reaps_lingering_completed_item --lib`,
  `cargo test -p agent-doc-orchestration complete_required_closeout_blocks_until_live_replica_delivery_is_acked --lib`,
  `cargo test -p agent-doc-orchestration force_disk_closeout_pending_maintenance_bypasses_active_listener --lib`,
  `cargo test -p agent-doc-orchestration terminal_proof --lib`,
  `cargo test -p agent-doc --test run_integration interrupted_run_leaves_write_applied_and_preflight_finishes_commit`,
  `cargo test -p agent-doc --test test_cli mcp_finalize_close_after_capture_recovers_on_next_preflight_once`,
  `cargo test -p agent-doc-orchestration preflight_recovers_response_captured_cycle_without_pending_file --lib`,
  `cargo test -p agent-doc-orchestration preflight_auto_recovers_jb_cache_conflict_cancel_write_applied_with_snapshot_drift --lib`,
  `cargo test -p agent-doc-orchestration recover_replays_capture_without_pending --lib`,
  and `cargo check -p agent-doc`.
- Phase 4 is complete for git-backed realtime admission, queue proof, crash
  replay, and terminal closeout proof. Remaining live-editor validation stays
  tracked by the Phase 3 and Phase 6 live verification gates.

Verification:

- Crash after response capture resumes without losing the response.
- Crash after document apply but before commit resumes to commit or fails closed
  with a clear recovery action.
- Queue prompt cannot disappear without either completion proof or recovery.

### Phase 5: delete legacy surfaces

Goal: reduce the state space.

Remove or permanently disable:

- disk fallback for editor-visible writes
- synchronous IPC wait-and-fallback
- normal-path full-document editor replacement
- normal-path full-range VS Code document replacement
- file watcher direct run loop
- heavy preflight as a required realtime gate
- markdown queue as source of truth
- repair paths that mutate visible files while editors are open

Keep compatibility shims only when they call the controller APIs internally.

Progress 2026-06-27:

- Disabled the convergence-gate timeout path that previously mapped an
  unconverged editor boundary to a `ForceDiskFallback` decision and then
  continued dispatching the next queue item.
- The pure gate now returns `Blocked` at timeout; idle-watch records a
  replayable `convergence_gate_blocked severity=error action=fail_closed`
  artifact once for the boundary and continues deferring instead of dispatching
  through the old escape hatch.
- Disabled the synchronous editor-IPC wait/fallback branch in write convergence:
  once a listener is active/connectable, missing ACK content, missing ACKs,
  send failures, or untrusted ACK mismatches now log
  `transport=blocked action=editor_convergence_required` and fail closed instead
  of returning `Ok(false)` to a disk write, even when no plugin-owner lease is
  present.
- Updated the pure `decide_editorless_disk_fallback` model so a missing,
  controller-hosted, or editor-backed socket with unproven delivery is
  `FailClosed`; only explicit `--force-disk` may select the disk write branch.
- Removed the no-listener headless disk fallback from write convergence. The
  shared `try_editor_converge`, guarded wrapper, plain wrapper, template write,
  apply-template repair, compact writeback, and status write paths now fail
  closed with `transport=blocked reason=no_listener`; only explicit force-disk
  paths remain as disk-write bypasses.
- Removed VS Code extension autosaves from command paths. Session-control
  commands no longer flush the editor buffer, and dispatch/mutation commands now
  fail closed on a dirty document through `ensureDocumentCleanForCommand` instead
  of calling `document.save()`.
- Disabled VS Code full-range `WorkspaceEdit` replacements in the shared
  minimal-edit helper. If the computed replacement would delete the entire
  existing buffer from offset 0, the extension now logs
  `full-document WorkspaceEdit replacement is disabled` and fails the patch
  instead of applying a whole-document visible edit.
- Disabled editor-open repair write paths in the editor plugins. VS Code prompt
  polling now skips dirty tracked buffers instead of saving them, legacy
  `save-document.signal` is consumed with `save_document IPC is disabled`, and
  both VS Code and JetBrains reconnect `reread_disk` repair decisions log
  `reread_disk repair is disabled` while keeping the visible buffer
  editor-owned.
- Disabled the JetBrains socket `save_document` repair message. The plugin now
  logs `save_document IPC is disabled`, records the surface op as `disabled`, and
  returns failure instead of calling `FileDocumentManager.saveDocument`.
- Disabled the JetBrains closed-file VFS whole-buffer patch writer. The
  no-document path now only reads disk to reject full-content IPC and ACK
  already-present replay content; non-noop component patches fail closed with
  `VFS whole-buffer patch apply is disabled` instead of calling
  `VirtualFile.setBinaryContent`.
- Disabled the watch-daemon direct legacy submit loop. File-watch events still
  flow through the controller-owned `document_watcher` actor and persist
  `FileWatchChangeObserved`, but `watch.rs` no longer calls
  `run_with_context`/preflight after a filesystem event.
- Sequential and DAG orchestration no longer require heavy preflight after
  injecting each child prompt. The shared ordered-task step now calls the
  lightweight `agent-doc admit` lifecycle hook and builds the child-agent prompt
  from a synthetic injected-task diff, preserving imperative response-contract
  enforcement for prompt presets and multiline tasks without running repair,
  maintenance, broad diff reporting, or preflight JSON generation on every
  child dispatch.
- Parent batch-change closeout also uses lightweight admission and no longer
  depends on a preflight baseline file before writing the stop message.
- Queue consumption now mirrors its before/after proof-ledger checkpoints into
  the typed state backbone: consumed heads append `QueueHeadSelected` before the
  mutation and `QueueHeadCompleted` after document/snapshot convergence, and the
  queue-consume regression test projects those events back into a completed
  typed queue head.
- Active queue maintenance now records the selected head in the typed state
  backbone after queue normalization/in-progress marking settles. Repeated
  preflights reuse the same selected-head event id as consumption's
  before-mutation checkpoint, so the projection gains an idempotent typed
  `active_head` before the prompt-diff synthesis path runs.
- Queue head projections now carry optional selected-head prompt text. Queue
  consumption records the next remaining head as selected after a non-draining
  consume, and direct run continuation uses the typed prompt text when the
  selected projection node still matches the current queue node key.
- Queue maintenance now records typed deferred queue-head state for
  stop-fence, time-gate, and mid-edit `item_modified` halts by pairing the
  halted prompt text with `QueueHeadDeferred` reasons. Deferred projections are
  non-drainable, clear `active_head`, and clear their stale defer reason when
  the same node is selected again.
- Direct run continuation now requires typed queue-head state for the current
  queue node. The synthetic prompt-diff path and auto-continuation loop stop
  with `unproven_typed_queue_state` when selected/deferred facts are missing or
  stale instead of falling back to markdown prompt text, stop fences, time
  gates, or snapshot head comparisons as authoritative guards.
- Queue maintenance now records a typed `QueueWorklistProjected` event for the
  active queue worklist after normalization and in-progress marking, including
  the rendered queue hash and typed prompt/preset/dispatch entries. Batch
  `orchestrate --from-queue` now resolves tasks from that projection and fails
  closed when the typed worklist is missing, inactive, or stale relative to the
  current rendered queue hash; markdown is used only as the activation/hash
  guard, not as the task source.
- Route queue activation/dispatch, route bug-report backlog filing, free-text
  queue consume, template apply, repair replay, and auto-done pending mutations
  now keep no-listener/no-proof writes strict by default and require explicit
  force-disk plumbing for editorless CLI/test paths. The forced paths log
  `reason=force_disk` while the default paths still fail closed on unproven
  editor convergence.
- Automated evidence: `cargo test -p agent-doc-orchestration convergence_gate
  --lib`, `cargo test -p agent-doc-orchestration convergence_playback --lib`,
  `cargo test -p agent-doc-orchestration editorless --lib`, `cargo test -p
  agent-doc-orchestration without_listener --lib`, `cargo test -p
  agent-doc-orchestration converge_document_or_disk --lib`, `cargo test -p
  agent-doc-orchestration converge_or_disk_write --lib`, `cargo test -p
  agent-doc-orchestration try_compact_editor_converge --lib`, `cargo test -p
  agent-doc-orchestration without_ack_content --lib`, `cargo test -p
  agent-doc-orchestration ack_mismatch --lib`, `cargo check -p
  agent-doc-orchestration`, `cargo test -p agent-doc-orchestration
  document_watcher --lib`, `cargo test -p agent-doc-orchestration watch --lib`,
  VS Code `npm test` including the full-document `WorkspaceEdit`, no-save, and
  reconnect-repair disabled guards, and JetBrains `./gradlew test --tests
  com.github.btakita.agentdoc.NormalizeExchangePrefixesTest` including the VFS
  source guard against `setBinaryContent` and reconnect repair writes,
  `cargo test -p agent-doc-orchestration queue_consume --lib`,
  `cargo test -p agent-doc-orchestration run_queue_maintenance --lib`,
  `cargo test -p agent-doc-orchestration state_backbone --lib`,
  `cargo test -p agent-doc-orchestration state_wire --lib`,
  `cargo test -p agent-doc-orchestration active_queue_prompt_state_prefers_typed_selected_prompt_when_node_matches --lib`,
  `cargo test -p agent-doc-orchestration active_queue_prompt_state --lib`,
  `cargo test -p agent-doc-orchestration active_queue_prompt_diff --lib`,
  `cargo test -p agent-doc-orchestration should_continue_auto_queue --lib`,
  `cargo test -p agent-doc-orchestration run::tests --lib`,
  `cargo test -p agent-doc resolve_task_batch`,
  `cargo test -p agent-doc orchestrate::dag::tests`,
  `cargo test -p agent-doc orchestrate::dispatch::tests`, and
  `cargo check -p agent-doc`.
- Additional automated evidence 2026-06-27: focused force-disk/strict-write
  regressions passed for route activation/enqueue/bug-report, pending auto-done,
  queue consume, repair replay, capture provenance, template apply, and FlowCore
  hot-path guards; `cargo clippy -- -D warnings` passed; `make check` passed in
  `src/agent-doc` with 5,461 tests passed, 241 skipped, doctests passing, the
  ignored closeout simulation target passing, and Lean formal build success.
- Remaining Phase 5 removal: live editor validation still needs to prove no
  editor-visible `disk_fallback` or markdown-authoritative queue path remains in
  normal use.

## Verification matrix

Required before declaring realtime reliable:

- `make check` in `src/agent-doc`
- JetBrains Gradle tests for patch application and undo behavior
- VS Code extension tests for patch application and undo behavior
- model tests for per-document actor ordering
- model tests for missing ACK, stale ACK, duplicate ACK, and crashed editor
- live JetBrains active-typing test
- live JetBrains undo/redo test
- live VS Code undo/redo test
- live two-editor convergence test
- live agent streaming patch test
- crash-restart closeout replay test
- log scan proving no editor-visible `disk_fallback`

## Performance targets

- Admission under 50 ms for a warm controller.
- Editor patch apply under 50 ms for ordinary patches.
- No 2 second synchronous IPC wait in the realtime path.
- Coalesce stream/materialization writes to avoid patch-per-token behavior.
- Agent prompt projection should be sized by active task context, not by full
  session document length.

## Execution environment

Run this cutover in a pure Codex session, preferably in a dedicated worktree.

Do not dogfood the broken agent-doc realtime path while modifying the realtime
orchestrator. The failing components are exactly the components that would be
editing the plan and code. Keep JetBrains/plugin live testing for the explicit
verification phases only, after Phase 0 and Phase 1 protections are in place.

Recommended operating mode:

- stop agent-doc supervisors/watchers for the target docs
- avoid running `agent-doc` against this plan file
- implement through plain Codex shell edits
- run Rust and JetBrains tests directly
- re-enable the editor plugins only for the live undo/convergence tests
- keep logs as evidence, not as realtime control input

## Done means

agent-doc is not reliable realtime until all of these are true:

- editor undo survives agent patches
- VS Code undo survives agent patches
- user edits are never dropped without explicit deletion provenance
- stale agent output never overwrites newer user edits
- no direct disk fallback touches open editor documents
- controller state arbitrates realtime writes while markdown remains the durable
  user document
- multi-editor edits converge without cache conflicts
- agent turns can be admitted from compact projections
- closeout is crash-replayable
- legacy preflight/finalize no longer owns the live orchestration path
